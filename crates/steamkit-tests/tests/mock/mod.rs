//! A scripted connection for the networking and CM client tests.
//!
//! [`MockConnection`] implements the library's connection traits without a
//! socket. The test plays the server: it decides when the connection raises
//! `Connected`, which messages arrive, and when the server goes away, and it
//! inspects what the client sent.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use steamkit::networking::steam3::disconnected_event_args::DisconnectedEventArgs;
use steamkit::networking::steam3::i_connection::{
    ConnectionEvent, ConnectionEventSenders, ConnectionEvents, IConnection,
};
use steamkit::networking::steam3::net_msg_event_args::NetMsgEventArgs;
use steamkit::networking::steam3::protocol_type::ProtocolTypes;
use steamkit::SteamError;
use tokio::sync::{broadcast, mpsc};

/// What `connect` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectBehaviour {
    /// Succeeds and raises `Connected` straight away, like the TCP transport.
    Connected,
    /// Succeeds without raising `Connected`; the test raises it.
    Silent,
    /// Fails, like an unreachable server.
    Fail,
    /// Fails and also raises `Disconnected`, like the WebSocket transport.
    FailWithDisconnected,
}

/// A connection whose events the test controls.
#[derive(Debug)]
pub struct MockConnection {
    events: ConnectionEventSenders,
    behaviour: ConnectBehaviour,
    end_point: Mutex<Option<SocketAddr>>,
    sent: Mutex<Vec<Vec<u8>>>,
    disconnect_calls: Mutex<Vec<bool>>,
    disconnected: AtomicBool,
    connects: AtomicUsize,
}

impl MockConnection {
    pub fn new(behaviour: ConnectBehaviour) -> Arc<Self> {
        Arc::new(Self {
            events: ConnectionEventSenders::with_default_capacity(),
            behaviour,
            end_point: Mutex::new(None),
            sent: Mutex::new(Vec::new()),
            disconnect_calls: Mutex::new(Vec::new()),
            disconnected: AtomicBool::new(false),
            connects: AtomicUsize::new(0),
        })
    }

    /// Everything the client sent, in order.
    pub fn sent(&self) -> Vec<Vec<u8>> {
        self.sent.lock().unwrap().clone()
    }

    /// The `EMsg` values (without the protobuf bit) of everything sent.
    pub fn sent_msg_types(&self) -> Vec<u32> {
        self.sent().iter().map(|data| raw_msg_type(data)).collect()
    }

    /// The arguments of every `disconnect` call.
    pub fn disconnect_calls(&self) -> Vec<bool> {
        self.disconnect_calls.lock().unwrap().clone()
    }

    /// The number of `connect` calls.
    pub fn connect_count(&self) -> usize {
        self.connects.load(Ordering::SeqCst)
    }

    /// The number of live subscribers of the ordered event stream.
    pub fn subscriber_count(&self) -> usize {
        self.events.event_subscriber_count()
    }

    /// The server sends a message.
    pub fn receive(&self, data: Vec<u8>) {
        let end_point = self.end_point.lock().unwrap().unwrap_or_else(default_end_point);
        self.events.emit_net_msg_received(NetMsgEventArgs::new(data, end_point));
    }

    /// The connection is established.
    pub fn raise_connected(&self) {
        self.disconnected.store(false, Ordering::SeqCst);
        self.events.emit_connected();
    }

    /// The server closes the connection.
    pub fn server_disconnect(&self) {
        if !self.disconnected.swap(true, Ordering::SeqCst) {
            self.events.emit_disconnected(DisconnectedEventArgs::new(false));
        }
    }
}

#[async_trait]
impl IConnection for MockConnection {
    fn current_end_point(&self) -> Option<SocketAddr> {
        *self.end_point.lock().unwrap()
    }

    fn protocol_types(&self) -> ProtocolTypes {
        ProtocolTypes::TCP
    }

    async fn connect(&self, end_point: SocketAddr, _timeout: Duration) -> Result<(), SteamError> {
        self.connects.fetch_add(1, Ordering::SeqCst);

        match self.behaviour {
            ConnectBehaviour::Connected => {
                *self.end_point.lock().unwrap() = Some(end_point);
                self.raise_connected();
                Ok(())
            }
            ConnectBehaviour::Silent => {
                *self.end_point.lock().unwrap() = Some(end_point);
                Ok(())
            }
            ConnectBehaviour::Fail => Err(SteamError::Connection("refused".to_owned())),
            ConnectBehaviour::FailWithDisconnected => {
                self.events.emit_disconnected(DisconnectedEventArgs::new(false));
                Err(SteamError::Connection("refused".to_owned()))
            }
        }
    }

    async fn disconnect(&self, user_initiated: bool) {
        self.disconnect_calls.lock().unwrap().push(user_initiated);
        if !self.disconnected.swap(true, Ordering::SeqCst) {
            self.events.emit_disconnected(DisconnectedEventArgs::new(user_initiated));
        }
    }

    async fn send(&self, data: Bytes) -> Result<(), SteamError> {
        self.sent.lock().unwrap().push(data.to_vec());
        Ok(())
    }

    fn local_ip(&self) -> Option<IpAddr> {
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    }
}

impl ConnectionEvents for MockConnection {
    fn events(&self) -> mpsc::UnboundedReceiver<ConnectionEvent> {
        self.events.subscribe_events()
    }

    fn net_msg_received(&self) -> broadcast::Receiver<NetMsgEventArgs> {
        self.events.subscribe_net_msg_received()
    }

    fn connected(&self) -> broadcast::Receiver<()> {
        self.events.subscribe_connected()
    }

    fn disconnected(&self) -> broadcast::Receiver<DisconnectedEventArgs> {
        self.events.subscribe_disconnected()
    }
}

pub fn default_end_point() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 27017))
}

/// The `EMsg` of a serialised message, without the protobuf bit.
pub fn raw_msg_type(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]]) & !0x8000_0000
}

/// Waits until `condition` holds, failing the test after five seconds.
pub async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting until {what}");
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// Gives the background tasks time to act on anything that is still queued,
/// for the tests that assert that something does *not* happen.
pub async fn settle() {
    tokio::time::sleep(Duration::from_millis(100)).await;
}
