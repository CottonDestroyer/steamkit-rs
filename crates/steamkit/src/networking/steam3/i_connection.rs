//! The connection trait and its events.
//!
//! [`IConnection`] is implemented by the transports (TCP, UDP, WebSocket). Their
//! events come through [`ConnectionEvents`], in two forms:
//!
//! * [`ConnectionEvents::events`] is one ordered, lossless stream of
//!   [`ConnectionEvent`]s. Anything protocol-related should use this, and the
//!   crate itself only uses this.
//! * Three per-event `broadcast` receivers, for callers that only care about one
//!   kind of event. A receiver that falls more than [`CONNECTION_EVENT_CAPACITY`]
//!   events behind gets `RecvError::Lagged` and loses events, and the three
//!   channels aren't ordered relative to each other.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc};

use crate::error::SteamError;

use super::disconnected_event_args::DisconnectedEventArgs;
use super::net_msg_event_args::NetMsgEventArgs;
use super::protocol_type::ProtocolTypes;

/// Capacity of each per-event broadcast channel. The ordered stream from
/// [`ConnectionEvents::events`] is unbounded.
pub const CONNECTION_EVENT_CAPACITY: usize = 64;

/// A transport connection to a CM server.
#[async_trait]
pub trait IConnection: Send + Sync {
    /// The remote endpoint of the current connection.
    ///
    /// This is `Some` between the `Connected` and `Disconnected` events,
    /// inclusive.
    fn current_end_point(&self) -> Option<SocketAddr>;

    /// The type of communication protocol that this connection uses.
    fn protocol_types(&self) -> ProtocolTypes;

    /// Connects to the specified endpoint.
    async fn connect(&self, end_point: SocketAddr, timeout: Duration) -> Result<(), SteamError>;

    /// Disconnects. `user_initiated` is `true` when the caller asked for it.
    async fn disconnect(&self, user_initiated: bool);

    /// Sends the specified data packet.
    async fn send(&self, data: Bytes) -> Result<(), SteamError>;

    /// The local IP of the underlying socket, when one is bound.
    fn local_ip(&self) -> Option<IpAddr>;
}

/// One event raised by a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// A message was received.
    NetMsgReceived(NetMsgEventArgs),
    /// The connection is established.
    Connected,
    /// The connection is gone.
    Disconnected(DisconnectedEventArgs),
}

/// The events of a connection. Every [`IConnection`] in this crate implements
/// this too.
pub trait ConnectionEvents: Send + Sync {
    /// Subscribes to all three events as one stream.
    ///
    /// The stream is unbounded, so no event is ever dropped, and it delivers the
    /// events in the order the connection raised them. Subscribe before calling
    /// `connect`, or the first events are missed.
    fn events(&self) -> mpsc::UnboundedReceiver<ConnectionEvent>;

    /// Subscribes to the `NetMsgReceived` event.
    fn net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs>;

    /// Subscribes to the `Connected` event.
    fn connected(&self) -> broadcast::Receiver<()>;

    /// Subscribes to the `Disconnected` event.
    fn disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs>;
}

/// A connection that also publishes its events.
/// [`EnvelopeEncryptedConnection`](super::envelope_encrypted_connection::EnvelopeEncryptedConnection)
/// takes its inner connection as an `Arc<dyn Connection>`.
pub trait Connection: IConnection + ConnectionEvents {}

impl<T> Connection for T where T: IConnection + ConnectionEvents + ?Sized {}

/// The sending halves of the connection event channels, used by every
/// connection type. Clones share the same channels.
#[derive(Clone, Debug)]
pub struct ConnectionEventSenders {
    net_msg_received: broadcast::Sender<NetMsgEventArgs>,
    connected: broadcast::Sender<()>,
    disconnected: broadcast::Sender<DisconnectedEventArgs>,
    /// Subscribers of the ordered stream. The lock is held while an event is
    /// handed to all of them, which gives events raised from different tasks
    /// one order.
    subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<ConnectionEvent>>>>,
}

impl ConnectionEventSenders {
    /// Creates the event channels; `capacity` is the size of each broadcast channel.
    pub fn new(capacity: usize) -> Self {
        let (net_msg_received, _) = broadcast::channel(capacity);
        let (connected, _) = broadcast::channel(capacity);
        let (disconnected, _) = broadcast::channel(capacity);

        Self {
            net_msg_received,
            connected,
            disconnected,
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Creates a new set of event channels with [`CONNECTION_EVENT_CAPACITY`].
    pub fn with_default_capacity() -> Self {
        Self::new(CONNECTION_EVENT_CAPACITY)
    }

    /// Subscribes to the ordered, lossless stream of all three events.
    pub fn subscribe_events(&self) -> mpsc::UnboundedReceiver<ConnectionEvent> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.subscribers.lock().push(sender);
        receiver
    }

    /// The number of live subscribers of the ordered stream.
    pub fn event_subscriber_count(&self) -> usize {
        let mut subscribers = self.subscribers.lock();
        subscribers.retain(|subscriber| !subscriber.is_closed());
        subscribers.len()
    }

    /// Subscribes to the `NetMsgReceived` event.
    pub fn subscribe_net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs> {
        self.net_msg_received.subscribe()
    }

    /// Subscribes to the `Connected` event.
    pub fn subscribe_connected(&self) -> broadcast::Receiver<()> {
        self.connected.subscribe()
    }

    /// Subscribes to the `Disconnected` event.
    pub fn subscribe_disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs> {
        self.disconnected.subscribe()
    }

    /// Raises the `NetMsgReceived` event. It's dropped if nobody is subscribed.
    pub fn emit_net_msg_received(&self, args: NetMsgEventArgs) {
        let mut subscribers = self.subscribers.lock();
        if self.net_msg_received.receiver_count() > 0 {
            let _ = self.net_msg_received.send(args.clone());
        }
        publish(&mut subscribers, ConnectionEvent::NetMsgReceived(args));
    }

    /// Raises the `Connected` event.
    pub fn emit_connected(&self) {
        let mut subscribers = self.subscribers.lock();
        let _ = self.connected.send(());
        publish(&mut subscribers, ConnectionEvent::Connected);
    }

    /// Raises the `Disconnected` event.
    pub fn emit_disconnected(&self, args: DisconnectedEventArgs) {
        let mut subscribers = self.subscribers.lock();
        let _ = self.disconnected.send(args);
        publish(&mut subscribers, ConnectionEvent::Disconnected(args));
    }
}

/// Hands an event to every subscriber of the ordered stream and drops the ones
/// that have gone away.
fn publish(subscribers: &mut Vec<mpsc::UnboundedSender<ConnectionEvent>>, event: ConnectionEvent) {
    if let [only] = subscribers.as_slice() {
        // Common case: one subscriber, so no need to clone the event.
        if only.send(event).is_err() {
            subscribers.clear();
        }
        return;
    }

    subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
}

impl Default for ConnectionEventSenders {
    fn default() -> Self {
        Self::with_default_capacity()
    }
}
