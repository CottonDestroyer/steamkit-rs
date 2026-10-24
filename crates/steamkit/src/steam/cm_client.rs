//! The base client that owns the connection to a CM server.
//!
//! [`SteamClient`](crate::steam::steam_client::steam_client::SteamClient) owns a `CMClient`
//! and plugs into it through the [`ClientMsgDispatcher`] trait.

use std::fmt;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::RwLock;
use prost::Message;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg, SteamIDRaw};
use crate::base::generated::steam_language::{EMsg, EResult};
use crate::base::packet_base::{IPacketMsg, PacketClientMsg, PacketClientMsgProtobuf, PacketMsg};
use crate::error::SteamError;
use crate::networking::steam3::envelope_encrypted_connection::EnvelopeEncryptedConnection;
use crate::networking::steam3::i_connection::{Connection, ConnectionEvent};
use crate::networking::steam3::protocol_type::ProtocolTypes;
use crate::networking::steam3::tcp_connection::TcpConnection;
use crate::networking::steam3::udp_connection::UdpConnection;
use crate::networking::steam3::web_socket_connection::WebSocketConnection;
use crate::steam::discovery::server_record::ServerRecord;
use crate::steam::discovery::smart_cm_server_list::{ServerQuality, SmartCMServerList};
use crate::steam::steam_client::configuration::steam_configuration::{
    default_user_agent, SteamConfiguration,
};
use crate::types::steam_id::SteamID;
use crate::util::debug_log::DebugLog;
use crate::util::debug_network_listener::IDebugNetworkListener;
use crate::util::net_helpers::NetHelpers;
use steamkit_protos::steam::{
    CMsgClientHello, CMsgClientHeartBeat, CMsgClientLogonResponse, CMsgClientLoggedOff,
    CMsgClientSessionToken, CMsgMulti, CMsgProtoBufHeader,
};

/// The protocol version sent in `ClientHello` (same value as
/// `MsgClientLogon::CURRENT_PROTOCOL`).
pub const CURRENT_PROTOCOL: u32 = 65581;

/// The bit that marks a message as protobuf-backed.
const PROTO_MASK: u32 = 0x8000_0000;

/// The size of the extended client message header.
const EXTENDED_HEADER_SIZE: usize = 36;

/// The offset of the SteamID inside the extended client message header.
const EXTENDED_HEADER_STEAM_ID_OFFSET: usize = 24;

/// The offset of the session id inside the extended client message header.
const EXTENDED_HEADER_SESSION_ID_OFFSET: usize = 32;

/// How many CM servers to try when the server asks us to move.
const SWITCH_ATTEMPTS: usize = 3;

/// How many times in a row the client moves to another CM server without a
/// successful logon before giving up and reporting the disconnect. Without a
/// limit, a server that keeps closing the connection would make the client
/// reconnect (and repeat its logon) forever.
pub const MAX_CONSECUTIVE_SERVER_SWITCHES: u32 = 5;

/// Receives the messages and lifecycle events of a [`CMClient`].
#[async_trait]
pub trait ClientMsgDispatcher: Send + Sync {
    /// Handles a received client message.
    /// Returning `false` drops the connection.
    async fn on_client_msg_received(&self, client: &CMClient, packet: &dyn IPacketMsg) -> bool;

    /// Called once the client is securely connected.
    async fn on_client_connected(&self, client: &CMClient);

    /// Called when the connection is gone.
    async fn on_client_disconnected(&self, client: &CMClient, user_initiated: bool);
}

/// Creates the connection for a given protocol selection. Set it with
/// [`CMClient::set_connection_factory`].
pub type ConnectionFactory = Arc<
    dyn Fn(&CMClient, &ServerRecord, ProtocolTypes) -> Result<Arc<dyn Connection>, SteamError>
        + Send
        + Sync,
>;

/// The default connection factory: TCP first, then WebSocket, then UDP.
///
/// SteamKit2 prefers WebSocket, but Steam closes a WebSocket CM session after a
/// minute or two while a TCP one stays up, so TCP goes first here. The
/// WebSocket is opened with `tokio-tungstenite`, so the `CMWebSocket` HTTP
/// client purpose from the configuration isn't used for it.
fn default_connection_factory(
    client: &CMClient,
    record: &ServerRecord,
    protocol: ProtocolTypes,
) -> Result<Arc<dyn Connection>, SteamError> {
    let universe = client.universe();
    let debug_network_listener = client.debug_network_listener();

    if protocol.has(ProtocolTypes::TCP) {
        return Ok(Arc::new(EnvelopeEncryptedConnection::with_debug_network_listener(
            Arc::new(TcpConnection::new()),
            universe,
            debug_network_listener,
        )));
    }

    if protocol.has(ProtocolTypes::WEB_SOCKET) {
        return Ok(Arc::new(
            WebSocketConnection::new_tungstenite_for_host_with_user_agent(
                record.get_host(),
                default_user_agent(),
            ),
        ));
    }

    if protocol.has(ProtocolTypes::UDP) {
        return Ok(Arc::new(EnvelopeEncryptedConnection::with_debug_network_listener(
            Arc::new(UdpConnection::new()),
            universe,
            debug_network_listener,
        )));
    }

    Err(SteamError::InvalidArgument(
        "Protocol bitmask has no supported protocols set.".to_owned(),
    ))
}

/// The background task that periodically sends the heartbeat message.
struct HeartBeat {
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

/// The base client that handles the connection to a CM server. Use it through
/// [`SteamClient`](crate::steam::steam_client::steam_client::SteamClient).
pub struct CMClient {
    configuration: Arc<SteamConfiguration>,
    id: String,
    servers: Arc<SmartCMServerList>,
    dispatcher: Arc<dyn ClientMsgDispatcher>,
    connection_factory: RwLock<Option<ConnectionFactory>>,
    connection: RwLock<Option<Arc<dyn Connection>>>,
    connection_cancellation: RwLock<Option<CancellationToken>>,
    outbound: RwLock<Option<mpsc::UnboundedSender<Bytes>>>,
    expect_disconnection: AtomicBool,
    reconnect_requested: AtomicBool,
    /// The serialized `ClientLogon` of the current session, repeated when the
    /// client moves to another CM server. `None` when there's no logged-on
    /// session worth resuming.
    pending_logon: RwLock<Option<Vec<u8>>>,
    /// The number of server switches since the last successful logon.
    consecutive_switches: AtomicU32,
    /// Bumped by every `connect`. A dispatch loop serves one generation and
    /// only touches the client's state while it's current.
    generation: AtomicU64,
    /// The latest generation whose disconnect has been announced, so each
    /// session announces it exactly once.
    announced_generation: AtomicU64,
    is_connected: AtomicBool,
    connected_handled: AtomicBool,
    session_token: AtomicU64,
    cell_id: RwLock<Option<u32>>,
    session_id: RwLock<Option<i32>>,
    steam_id: RwLock<Option<SteamID>>,
    public_ip: RwLock<Option<IpAddr>>,
    ip_country_code: RwLock<Option<String>>,
    user_country_code: RwLock<Option<String>>,
    debug_network_listener: RwLock<Option<Arc<dyn IDebugNetworkListener>>>,
    heart_beat: RwLock<Option<HeartBeat>>,
}

/// A connection plus its event stream. The dispatch loop owns this so it can
/// swap it out when the server asks us to move.
struct ConnectionSession {
    connection: Arc<dyn Connection>,
    /// The connection's events, in order, none dropped.
    events: mpsc::UnboundedReceiver<ConnectionEvent>,
}

impl fmt::Debug for CMClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CMClient")
            .field("id", &self.id)
            .field("is_connected", &self.is_connected.load(Ordering::SeqCst))
            .field("session_id", &*self.session_id.read())
            .field("steam_id", &*self.steam_id.read())
            .field("cell_id", &*self.cell_id.read())
            .finish()
    }
}

impl CMClient {
    /// Creates a client with the given configuration. The identifier must not
    /// be empty (the [`SteamClient`](crate::steam::steam_client::steam_client::SteamClient)
    /// constructors check this).
    pub fn new(
        configuration: Arc<SteamConfiguration>,
        identifier: &str,
        dispatcher: Arc<dyn ClientMsgDispatcher>,
    ) -> CMClient {
        let servers = configuration.server_list().clone();

        CMClient {
            configuration,
            id: identifier.to_owned(),
            servers,
            dispatcher,
            connection_factory: RwLock::new(None),
            connection: RwLock::new(None),
            connection_cancellation: RwLock::new(None),
            outbound: RwLock::new(None),
            expect_disconnection: AtomicBool::new(false),
            reconnect_requested: AtomicBool::new(false),
            pending_logon: RwLock::new(None),
            consecutive_switches: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            announced_generation: AtomicU64::new(0),
            is_connected: AtomicBool::new(false),
            connected_handled: AtomicBool::new(false),
            session_token: AtomicU64::new(0),
            cell_id: RwLock::new(None),
            session_id: RwLock::new(None),
            steam_id: RwLock::new(None),
            public_ip: RwLock::new(None),
            ip_country_code: RwLock::new(None),
            user_country_code: RwLock::new(None),
            debug_network_listener: RwLock::new(None),
            heart_beat: RwLock::new(None),
        }
    }

    /// The configuration for this client.
    pub fn configuration(&self) -> &Arc<SteamConfiguration> {
        &self.configuration
    }

    /// A unique identifier for this client.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The list of CM servers.
    pub fn servers(&self) -> &Arc<SmartCMServerList> {
        &self.servers
    }

    /// The local IP of the connection.
    pub fn local_ip(&self) -> Option<IpAddr> {
        self.connection.read().as_ref().and_then(|connection| connection.local_ip())
    }

    /// The endpoint we're connected to.
    pub fn current_end_point(&self) -> Option<SocketAddr> {
        self.connection.read().as_ref().and_then(|connection| connection.current_end_point())
    }

    /// Our public IP address, known after a successful logon.
    pub fn public_ip(&self) -> Option<IpAddr> {
        *self.public_ip.read()
    }

    /// The country code of our public IP, according to Steam.
    pub fn ip_country_code(&self) -> Option<String> {
        self.ip_country_code.read().clone()
    }

    /// The country code of the account.
    pub fn user_country_code(&self) -> Option<String> {
        self.user_country_code.read().clone()
    }

    /// The universe of this client.
    pub fn universe(&self) -> crate::base::generated::steam_language::EUniverse {
        self.configuration.universe()
    }

    /// Whether we're connected to a CM server.
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    /// The session token assigned by the server.
    pub fn session_token(&self) -> u64 {
        self.session_token.load(Ordering::SeqCst)
    }

    /// The cell id Steam recommends for this client.
    pub fn cell_id(&self) -> Option<u32> {
        *self.cell_id.read()
    }

    /// The session id.
    pub fn session_id(&self) -> Option<i32> {
        *self.session_id.read()
    }

    /// The SteamID.
    pub fn steam_id(&self) -> Option<SteamID> {
        *self.steam_id.read()
    }

    /// The timeout for connecting to a CM server.
    pub fn connection_timeout(&self) -> Duration {
        self.configuration.connection_timeout()
    }

    /// The debug network listener, if any.
    pub fn debug_network_listener(&self) -> Option<Arc<dyn IDebugNetworkListener>> {
        self.debug_network_listener.read().clone()
    }

    /// Sets the debug network listener.
    pub fn set_debug_network_listener(&self, listener: Option<Arc<dyn IDebugNetworkListener>>) {
        *self.debug_network_listener.write() = listener;
    }

    /// Whether the server is about to close the connection on purpose (e.g. after a log off).
    pub fn expect_disconnection(&self) -> bool {
        self.expect_disconnection.load(Ordering::SeqCst)
    }

    /// Sets whether the server is about to close the connection on purpose.
    pub fn set_expect_disconnection(&self, expect: bool) {
        self.expect_disconnection.store(expect, Ordering::SeqCst);
    }

    /// Sets the factory that creates this client's connections.
    pub fn set_connection_factory(&self, factory: ConnectionFactory) {
        *self.connection_factory.write() = Some(factory);
    }

    /// Writes a line to the debug log under `<client id>/<category>`, and emits
    /// a `tracing` event too.
    pub fn log_debug(&self, category: &str, message: &str) {
        let full_category = format!("{}/{}", self.id, category);

        tracing::debug!(client = %self.id, category = %full_category, "{}", message);

        DebugLog::write_line(&full_category, message);
    }

    /// Connects to a CM server; if `cm_server` is `None`, the next one from the
    /// server list is used.
    ///
    /// A logged-on session that the server closes is moved to another server
    /// automatically. Any other disconnect is reported, and it's up to you to
    /// call this again.
    pub async fn connect(self: &Arc<Self>, cm_server: Option<ServerRecord>) -> Result<(), SteamError> {
        self.disconnect_with(true).await;

        // A new session; never repeat an earlier session's logon on it.
        *self.pending_logon.write() = None;
        self.consecutive_switches.store(0, Ordering::SeqCst);
        self.reconnect_requested.store(false, Ordering::SeqCst);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let cancellation = CancellationToken::new();
        *self.connection_cancellation.write() = Some(cancellation.clone());
        self.expect_disconnection.store(false, Ordering::SeqCst);
        self.connected_handled.store(false, Ordering::SeqCst);

        let record = match cm_server {
            Some(record) => Some(record),
            None => {
                self.servers
                    .get_next_server_candidate_async(self.configuration.protocol_types())
                    .await
            }
        };

        let Some(record) = record else {
            self.log_debug("CMClient", "Server record task returned no result.");
            self.announced_generation.fetch_max(generation, Ordering::SeqCst);
            self.on_client_disconnected(false).await;
            return Err(SteamError::Connection("no server candidate is available".to_owned()));
        };

        // Host names were already resolved when the server record was created.
        let end_point = record.end_point();

        let session = match self.open_session(record, generation).await {
            Ok(session) => session,
            Err(error) => {
                // No session, so a later `disconnect` has nothing to announce.
                self.announced_generation.fetch_max(generation, Ordering::SeqCst);
                return Err(error);
            }
        };
        let connect_target = session.connection.clone();

        let this = Arc::clone(self);
        let task_cancellation = cancellation.clone();
        tokio::spawn(async move {
            this.dispatch_loop(session, task_cancellation, generation).await;
        });

        if let Err(error) = connect_target.connect(end_point, self.connection_timeout()).await {
            self.log_debug("CMClient", &format!("Unhandled exception when attempting to connect to Steam: {}", error));
            // The dispatch loop has nothing to serve. Some transports also raise
            // `Disconnected` for a failed attempt; the generation check in
            // `end_session` makes sure the failure is announced once and doesn't
            // start a move to another server.
            cancellation.cancel();
            self.end_session(generation, &connect_target, false).await;
            return Err(error);
        }

        Ok(())
    }

    /// Disconnects. Counts as user-initiated.
    pub async fn disconnect(&self) {
        self.disconnect_with(true).await;
    }

    /// Disconnects, saying whether it was user-initiated.
    pub async fn disconnect_with(&self, user_initiated: bool) {
        let generation = self.generation.load(Ordering::SeqCst);

        self.stop_heart_beat();

        // Cancel the dispatch loop first so it never acts on the `Disconnected`
        // event this raises.
        if let Some(cancellation) = self.connection_cancellation.write().take() {
            cancellation.cancel();
        }

        // Dropping the outbound queue stops the writer task.
        *self.outbound.write() = None;

        let connection = self.connection.write().take();
        if let Some(connection) = &connection {
            connection.disconnect(user_initiated).await;
        }

        // The connection can be missing while we're moving to another server;
        // the session is still live, so announce its end here.
        if self.claim_disconnect_announcement(generation) {
            self.teardown(connection.as_ref(), user_initiated).await;
        }
    }

    /// Creates a connection to the given server and starts its writer task.
    ///
    /// The dispatch loop isn't started here; it owns the session so it can
    /// replace it when the server asks us to move. The connection is only
    /// installed while `generation` is current, so a superseded session can't
    /// install its connection over the new one.
    async fn open_session(
        self: &Arc<Self>,
        record: ServerRecord,
        generation: u64,
    ) -> Result<ConnectionSession, SteamError> {
        let protocol = ProtocolTypes(
            record.protocol_types().raw() & self.configuration.protocol_types().raw(),
        );

        let factory = self.connection_factory.read().clone();
        let connection = match factory {
            Some(factory) => factory(self, &record, protocol)?,
            None => default_connection_factory(self, &record, protocol)?,
        };

        // Subscribe to events before starting the connection so none are missed.
        let events = connection.events();

        // The outbound queue keeps messages in order even though `send` is sync.
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Bytes>();

        {
            let mut slot = self.connection.write();
            if self.generation.load(Ordering::SeqCst) != generation {
                return Err(SteamError::Cancelled);
            }
            *slot = Some(connection.clone());
            *self.outbound.write() = Some(outbound_tx);
        }

        let writer_connection = connection.clone();
        tokio::spawn(async move {
            while let Some(bytes) = outbound_rx.recv().await {
                if let Err(error) = writer_connection.send(bytes).await {
                    tracing::debug!(%error, "failed to send a message to Steam");
                }
            }
        });

        Ok(ConnectionSession { connection, events })
    }

    /// Removes `connection` from the client, if it's still the client's
    /// connection and `generation` is still current.
    fn release_connection(&self, generation: u64, connection: &Arc<dyn Connection>) {
        let mut slot = self.connection.write();
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if slot.as_ref().is_some_and(|current| same_connection(current, connection)) {
            *slot = None;
            *self.outbound.write() = None;
        }
    }

    /// Claims the right to announce the disconnect of `generation`. Returns
    /// `true` exactly once per generation, so the dispatch loop and a concurrent
    /// `disconnect` don't both announce it.
    fn claim_disconnect_announcement(&self, generation: u64) -> bool {
        self.announced_generation.fetch_max(generation, Ordering::SeqCst) < generation
    }

    /// Whether a session the server closed is worth resuming on another CM
    /// server: a logon was accepted (or is still pending), and the disconnect
    /// wasn't expected (logging off sets that expectation).
    fn can_resume_session(&self) -> bool {
        !self.expect_disconnection.load(Ordering::SeqCst) && self.pending_logon.read().is_some()
    }

    /// Counts a move to another CM server. Returns `false` once we've moved
    /// [`MAX_CONSECUTIVE_SERVER_SWITCHES`] times without a successful logon.
    fn record_switch_attempt(&self) -> bool {
        self.consecutive_switches.fetch_add(1, Ordering::SeqCst) < MAX_CONSECUTIVE_SERVER_SWITCHES
    }

    /// Ends the session of the dispatch loop serving `generation` and announces
    /// the disconnect, unless that already happened.
    ///
    /// The client's connection is only removed while `generation` is current;
    /// a dispatch loop superseded by a later `connect` must not tear down the
    /// newer session.
    async fn end_session(
        &self,
        generation: u64,
        connection: &Arc<dyn Connection>,
        user_initiated: bool,
    ) {
        let current = {
            let mut slot = self.connection.write();
            if self.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let current = slot.take();
            *self.outbound.write() = None;
            current
        };

        if !self.claim_disconnect_announcement(generation) {
            return;
        }

        let connection = current.unwrap_or_else(|| connection.clone());
        self.teardown(Some(&connection), user_initiated).await;
    }

    /// Moves to a different CM server after the server asked us to.
    ///
    /// The session isn't really over (the connection is fine, the server just
    /// wants us elsewhere), so no disconnect is announced. This can't call
    /// [`CMClient::connect`], since that starts a dispatch loop and the dispatch
    /// loop is what calls this.
    ///
    /// Returns [`SteamError::Cancelled`] if the client was disconnected or
    /// reconnected in the meantime.
    async fn switch_session(
        self: &Arc<Self>,
        session: &mut ConnectionSession,
        cancellation: &CancellationToken,
        generation: u64,
    ) -> Result<(), SteamError> {
        self.stop_heart_beat();

        // Take the old connection out of the client but DON'T disconnect it
        // yet: its disconnect event would reach the dispatch loop (still
        // listening on the old stream) and end the loop doing this switch.
        let old_connection = {
            let mut slot = self.connection.write();
            if self.generation.load(Ordering::SeqCst) != generation {
                return Err(SteamError::Cancelled);
            }
            *self.outbound.write() = None;
            slot.take()
        };

        let mut last_error = None;

        for _ in 0..SWITCH_ATTEMPTS {
            if cancellation.is_cancelled() {
                last_error = Some(SteamError::Cancelled);
                break;
            }

            let mut record = self
                .servers
                .get_next_server_candidate_async(self.configuration.protocol_types())
                .await;

            if record.is_none() {
                // Every server in the list is marked bad; fetch a fresh list
                // before giving up.
                let _ = self.servers.force_refresh_server_list().await;

                record = self
                    .servers
                    .get_next_server_candidate_async(self.configuration.protocol_types())
                    .await;
            }

            let Some(record) = record else {
                break;
            };

            let end_point = record.end_point();
            self.log_debug(
                "CMClient",
                &format!("Switching to another CM server: {end_point}"),
            );

            let new_session = match self.open_session(record, generation).await {
                Ok(new_session) => new_session,
                Err(SteamError::Cancelled) => {
                    last_error = Some(SteamError::Cancelled);
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            let new_connection = new_session.connection.clone();

            match new_connection
                .connect(end_point, self.connection_timeout())
                .await
            {
                Ok(()) if cancellation.is_cancelled() => {
                    // We were disconnected while this connection was being made;
                    // it mustn't survive the switch.
                    self.release_connection(generation, &new_connection);
                    new_connection.disconnect(true).await;
                    last_error = Some(SteamError::Cancelled);
                    break;
                }
                Ok(()) => {
                    // Replacing the session drops the old stream, so the old
                    // connection's disconnect never reaches the dispatch loop.
                    *session = new_session;

                    // The new server sends its own hello, and the logon is
                    // repeated after it.
                    self.connected_handled.store(false, Ordering::SeqCst);
                    self.is_connected.store(false, Ordering::SeqCst);
                    *self.session_id.write() = None;
                    *self.steam_id.write() = None;

                    if let Some(old_connection) = old_connection {
                        old_connection.disconnect(false).await;
                    }

                    return Ok(());
                }
                Err(error) => {
                    // No answer from this server; try another one so a single
                    // unreachable server doesn't end the session.
                    self.log_debug(
                        "CMClient",
                        &format!("The CM server at {end_point} did not answer: {error}"),
                    );
                    last_error = Some(error);

                    self.release_connection(generation, &new_connection);
                }
            }
        }

        if let Some(old_connection) = old_connection {
            old_connection.disconnect(false).await;
        }

        Err(last_error.unwrap_or_else(|| {
            SteamError::Connection("no server candidate is available".to_owned())
        }))
    }

    /// Sends a client message, filling in our session id and SteamID.
    ///
    /// The message is taken by shared reference, so the session id and SteamID
    /// are written into the serialized bytes rather than into `msg`.
    pub fn send(&self, msg: &dyn IClientMsg) -> Result<(), SteamError> {
        let serialized = self.serialize_message(msg);

        self.log_debug("CMClient", &format!("CM: --> {:?}", msg.msg_type()));

        if let Some(listener) = self.debug_network_listener.read().clone() {
            listener.on_outgoing_network_message(msg.msg_type(), &serialized);
        }

        // Remember the logon so it can be repeated on another CM server.
        if msg.msg_type() == EMsg::ClientLogon {
            *self.pending_logon.write() = Some(serialized.clone());
        }

        let outbound = self.outbound.read().clone();
        let Some(outbound) = outbound else {
            return Err(SteamError::Connection(
                "the client is not connected to Steam".to_owned(),
            ));
        };

        outbound
            .send(Bytes::from(serialized))
            .map_err(|_| SteamError::Connection("the connection to Steam has been closed".to_owned()))
    }

    /// Serializes a message and fills in our session id and SteamID.
    fn serialize_message(&self, msg: &dyn IClientMsg) -> Vec<u8> {
        let session_id = *self.session_id.read();
        let steam_id = *self.steam_id.read();

        if session_id.is_none() && steam_id.is_none() {
            return msg.serialize();
        }

        stamp_identity(msg, msg.serialize(), session_id, steam_id)
    }

    /// Called once the client is securely connected.
    pub async fn on_client_connected(&self) {
        let mut request = ClientMsgProtobuf::<CMsgClientHello>::new(EMsg::ClientHello);
        request.body_mut().protocol_version = Some(CURRENT_PROTOCOL);

        let _ = self.send(&request);

        self.dispatcher.on_client_connected(self).await;
    }

    /// Called when the connection is gone.
    pub async fn on_client_disconnected(&self, user_initiated: bool) {
        self.dispatcher.on_client_disconnected(self, user_initiated).await;
    }

    /// Parses a raw payload into a packet. Returns `None` if it can't be parsed.
    pub fn get_packet_msg(&self, data: Vec<u8>) -> Option<Box<dyn IPacketMsg>> {
        if data.len() < 4 {
            self.log_debug(
                "CMClient",
                &format!(
                    "PacketMsg too small to contain a message, was only {} bytes. Message: 0x{}",
                    data.len(),
                    hex::encode(&data)
                ),
            );
            return None;
        }

        let raw = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let e_msg = EMsg::from_value((raw & !PROTO_MASK) as i32).unwrap_or(EMsg::Invalid);

        match e_msg {
            // These are always plain `MsgHdr` messages.
            EMsg::ChannelEncryptRequest | EMsg::ChannelEncryptResponse | EMsg::ChannelEncryptResult => {
                return match PacketMsg::new(e_msg, data) {
                    Ok(packet) => Some(Box::new(packet) as Box<dyn IPacketMsg>),
                    Err(error) => {
                        self.log_debug("CMClient", &format!("Exception deserializing emsg {}: {}", e_msg.value(), error));
                        None
                    }
                };
            }
            _ => {}
        }

        let result = if raw & PROTO_MASK != 0 {
            PacketClientMsgProtobuf::new(e_msg, data).map(|packet| Box::new(packet) as Box<dyn IPacketMsg>)
        } else {
            PacketClientMsg::new(e_msg, data).map(|packet| Box::new(packet) as Box<dyn IPacketMsg>)
        };

        match result {
            Ok(packet) => Some(packet),
            Err(error) => {
                self.log_debug(
                    "CMClient",
                    &format!("Exception deserializing emsg {} ({}): {}", e_msg.value(), raw & PROTO_MASK != 0, error),
                );
                None
            }
        }
    }

    /// Runs the inbound dispatch loop for a connection.
    async fn dispatch_loop(
        self: Arc<Self>,
        mut session: ConnectionSession,
        cancellation: CancellationToken,
        generation: u64,
    ) {
        loop {
            if self.reconnect_requested.swap(false, Ordering::SeqCst) {
                if !self.record_switch_attempt() {
                    self.log_debug(
                        "CMClient",
                        "Not moving to another CM server: too many moves without a logon",
                    );
                    self.end_session(generation, &session.connection, false).await;
                    break;
                }

                if let Err(error) = self.switch_session(&mut session, &cancellation, generation).await {
                    self.log_debug(
                        "CMClient",
                        &format!("Unable to switch to another CM server: {error}"),
                    );
                    let user_initiated = cancellation.is_cancelled();
                    self.end_session(generation, &session.connection, user_initiated).await;
                    break;
                }
                continue;
            }

            // `biased`: once we've been disconnected, nothing else still queued
            // may be acted on.
            let event = tokio::select! {
                biased;
                _ = cancellation.cancelled() => break,
                event = session.events.recv() => event,
            };

            match event {
                Some(ConnectionEvent::NetMsgReceived(args)) => {
                    let packet = self.get_packet_msg(args.data);
                    let Some(packet) = packet else {
                        self.log_debug("CMClient", "Packet message failed to parse, shutting down connection");
                        self.disconnect_with(false).await;
                        break;
                    };

                    self.log_debug(
                        "CMClient",
                        &format!("CM: <-- {:?}", packet.msg_type()),
                    );

                    if !self.dispatch_client_msg(Some(packet.as_ref())).await {
                        break;
                    }
                }
                Some(ConnectionEvent::Connected) => {
                    self.handle_connected(&session.connection).await;
                }
                Some(ConnectionEvent::Disconnected(args)) => {
                    // A logged-on session the server closed is moved to another
                    // CM server. Not one that was logged off, whose logon failed,
                    // or that never logged on: repeating the logon there would log
                    // the user back on against their will.
                    let resume = !args.user_initiated && self.can_resume_session();
                    if !resume {
                        self.end_session(generation, &session.connection, args.user_initiated)
                            .await;
                        break;
                    }

                    if !self.record_switch_attempt() {
                        self.log_debug(
                            "CMClient",
                            "Not reconnecting to Steam: too many reconnections without a logon",
                        );
                        self.end_session(generation, &session.connection, false).await;
                        break;
                    }

                    if let Err(error) = self.switch_session(&mut session, &cancellation, generation).await {
                        self.log_debug(
                            "CMClient",
                            &format!("Unable to reconnect to Steam: {error}"),
                        );
                        let user_initiated = cancellation.is_cancelled();
                        self.end_session(generation, &session.connection, user_initiated).await;
                        break;
                    }
                }
                None => {
                    // The connection went away without a disconnect event.
                    self.end_session(generation, &session.connection, false).await;
                    break;
                }
            }
        }
    }

    /// Handles the connection's `Connected` event.
    async fn handle_connected(&self, connection: &Arc<dyn Connection>) {
        if self.connected_handled.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Some(end_point) = connection.current_end_point() {
            self.servers.try_mark(end_point, connection.protocol_types(), ServerQuality::Good);
        }

        self.is_connected.store(true, Ordering::SeqCst);

        self.on_client_connected().await;

        // If the previous server sent us here ("try another CM"), repeat the
        // logon after the new server's hello.
        let pending = self.pending_logon.read().clone();
        if let Some(bytes) = pending {
            let outbound = self.outbound.read().clone();
            if let Some(outbound) = outbound {
                let _ = outbound.send(Bytes::from(bytes));
            }
        }
    }

    /// Runs the dispatch pipeline for a received message: the base handling
    /// first, then the dispatcher.
    async fn dispatch_client_msg(self: &Arc<Self>, packet: Option<&dyn IPacketMsg>) -> bool {
        let Some(packet) = packet else {
            self.log_debug("CMClient", "Packet message failed to parse, shutting down connection");
            self.disconnect_with(false).await;
            return false;
        };

        if !self.on_client_msg_received(packet).await {
            return false;
        }

        self.dispatcher.on_client_msg_received(self, packet).await
    }

    /// The base handling of a received message.
    async fn on_client_msg_received(self: &Arc<Self>, packet: &dyn IPacketMsg) -> bool {
        // Multi messages are logged after they're unpacked.
        if packet.msg_type() != EMsg::Multi {
            if let Some(listener) = self.debug_network_listener.read().clone() {
                listener.on_incoming_network_message(packet.msg_type(), packet.get_data());
            }
        }

        match packet.msg_type() {
            EMsg::Multi => return self.handle_multi(packet).await,
            EMsg::ClientLogOnResponse => self.handle_logon_response(packet).await,
            EMsg::ClientLoggedOff => self.handle_logged_off(packet).await,
            EMsg::ClientServerUnavailable => self.handle_server_unavailable(packet).await,
            EMsg::ClientSessionToken => self.handle_session_token(packet).await,
            _ => {}
        }

        true
    }

    /// Unpacks a `Multi` message and dispatches the messages it carries.
    async fn handle_multi(self: &Arc<Self>, packet: &dyn IPacketMsg) -> bool {
        if !packet.is_proto() {
            self.log_debug("CMClient", "HandleMulti got non-proto MsgMulti!!");
            return true;
        }

        let Ok(msg_multi) = ClientMsgProtobuf::<CMsgMulti>::from_packet(packet) else {
            return true;
        };

        let payload = msg_multi.body().message_body.clone().unwrap_or_default();
        let unzipped = msg_multi.body().size_unzipped.unwrap_or(0);

        let data = if unzipped > 0 {
            match decompress_gzip(&payload) {
                Ok(data) => data,
                Err(error) => {
                    self.log_debug("CMClient", &format!("Failed to decompress multi message: {}", error));
                    return true;
                }
            }
        } else {
            payload
        };

        let mut offset = 0usize;
        while offset + 4 <= data.len() {
            let sub_size = i32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
            offset += 4;

            if sub_size <= 0 {
                break;
            }

            let sub_size = sub_size as usize;
            if offset + sub_size > data.len() {
                break;
            }

            let sub_data = data[offset..offset + sub_size].to_vec();
            offset += sub_size;

            match self.get_packet_msg(sub_data) {
                Some(sub_packet) => {
                    // Boxed, or unpacking and dispatching would be an
                    // infinitely sized async recursion.
                    if !Box::pin(self.dispatch_client_msg(Some(sub_packet.as_ref()))).await {
                        return false;
                    }
                }
                // An unparseable message inside a Multi disconnects. Just
                // returning `false` would end the dispatch loop and leave the
                // connection open with nobody reading it.
                None => return Box::pin(self.dispatch_client_msg(None)).await,
            }
        }

        true
    }

    async fn handle_logon_response(self: &Arc<Self>, packet: &dyn IPacketMsg) {
        if !packet.is_proto() {
            // A non-protobuf logon response comes from connecting without ever
            // sending a ClientLogon. It always fails; nothing to do.
            self.log_debug(
                "CMClient",
                "Got non-proto logon response, this is indicative of no logon attempt after connecting.",
            );
            return;
        }

        let Ok(response) = ClientMsgProtobuf::<CMsgClientLogonResponse>::from_packet(packet) else {
            return;
        };

        let logon_result = response
            .body()
            .eresult
            .and_then(EResult::from_value)
            .unwrap_or(EResult::Invalid);

        if logon_result == EResult::OK {
            let header = response.proto_header();

            *self.session_id.write() = header.client_sessionid;
            *self.steam_id.write() = header.steamid.map(SteamID::from_id);
            *self.cell_id.write() = response.body().cell_id;
            *self.public_ip.write() = response
                .body()
                .public_ip
                .as_ref()
                .and_then(NetHelpers::get_ip_address_from_msg);
            *self.ip_country_code.write() = response.body().ip_country_code.clone();
            *self.user_country_code.write() = response.body().user_country.clone();

            let heartbeat_seconds = response.body().legacy_out_of_game_heartbeat_seconds.unwrap_or(0);

            self.restart_heart_beat(Duration::from_secs(heartbeat_seconds.max(0) as u64));

            // We're logged on, so the moves that got us here are done.
            self.consecutive_switches.store(0, Ordering::SeqCst);
        } else if logon_result == EResult::TryAnotherCM || logon_result == EResult::ServiceUnavailable {
            let connection = self.connection.read().clone();
            if let Some(connection) = connection {
                if let Some(end_point) = connection.current_end_point() {
                    self.servers.try_mark(end_point, connection.protocol_types(), ServerQuality::Bad);
                }
            }

            // Move to another server ourselves; the connection this arrived on
            // is still perfectly healthy.
            self.reconnect_requested.store(true, Ordering::SeqCst);
        } else {
            // The logon was refused (wrong password, expired token, Steam Guard
            // code needed). It would fail the same way on another server, and
            // retrying a wrong password is how accounts get rate limited.
            *self.pending_logon.write() = None;
        }
    }

    async fn handle_logged_off(self: &Arc<Self>, packet: &dyn IPacketMsg) {
        *self.session_id.write() = None;
        *self.steam_id.write() = None;
        *self.cell_id.write() = None;
        *self.public_ip.write() = None;
        *self.ip_country_code.write() = None;
        *self.user_country_code.write() = None;

        self.stop_heart_beat();

        // The session is over; never log back on automatically. Especially not
        // after `LoggedInElsewhere`: logging back on would kick the other
        // session, which would then do the same to us.
        *self.pending_logon.write() = None;

        if !packet.is_proto() {
            return;
        }

        let Ok(logoff) = ClientMsgProtobuf::<CMsgClientLoggedOff>::from_packet(packet) else {
            return;
        };

        let logoff_result = logoff
            .body()
            .eresult
            .and_then(EResult::from_value)
            .unwrap_or(EResult::Invalid);

        if logoff_result == EResult::TryAnotherCM || logoff_result == EResult::ServiceUnavailable {
            let connection = self.connection.read().clone();
            if let Some(connection) = connection {
                if let Some(end_point) = connection.current_end_point() {
                    self.servers.try_mark(end_point, connection.protocol_types(), ServerQuality::Bad);
                }
            }
        }
    }

    async fn handle_server_unavailable(self: &Arc<Self>, packet: &dyn IPacketMsg) {
        self.log_debug(
            "SteamClient",
            &format!("A server was not available for request: '{}'", packet.msg_type().value()),
        );
        self.disconnect_with(false).await;
    }

    async fn handle_session_token(self: &Arc<Self>, packet: &dyn IPacketMsg) {
        let Ok(token) = ClientMsgProtobuf::<CMsgClientSessionToken>::from_packet(packet) else {
            return;
        };

        self.session_token.store(token.body().token.unwrap_or(0), Ordering::SeqCst);
    }

    /// Resets the session state and announces the disconnect. Callers make sure
    /// this runs once per session.
    async fn teardown(&self, connection: Option<&Arc<dyn Connection>>, user_initiated: bool) {
        self.is_connected.store(false, Ordering::SeqCst);

        let expect_disconnection = self.expect_disconnection.load(Ordering::SeqCst);

        if !user_initiated && !expect_disconnection {
            if let Some(connection) = connection {
                if let Some(end_point) = connection.current_end_point() {
                    self.servers.try_mark(end_point, connection.protocol_types(), ServerQuality::Bad);
                }
            }
        }

        if user_initiated {
            // The user disconnected, so the session is over.
            *self.pending_logon.write() = None;
        }

        *self.session_id.write() = None;
        *self.steam_id.write() = None;

        self.stop_heart_beat();

        self.on_client_disconnected(user_initiated || expect_disconnection).await;
    }

    fn restart_heart_beat(self: &Arc<Self>, delay: Duration) {
        self.stop_heart_beat();

        let delay = if delay.is_zero() { Duration::from_secs(1) } else { delay };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };

        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let this = Arc::clone(self);

        let task = handle.spawn(async move {
            let mut interval = tokio::time::interval(delay);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {
                        let request = ClientMsgProtobuf::<CMsgClientHeartBeat>::new(EMsg::ClientHeartBeat);
                        let _ = this.send(&request);
                    }
                }
            }
        });

        *self.heart_beat.write() = Some(HeartBeat { cancel, task });
    }

    fn stop_heart_beat(&self) {
        if let Some(heart_beat) = self.heart_beat.write().take() {
            heart_beat.cancel.cancel();
            heart_beat.task.abort();
        }
    }
}

/// Writes our session id and SteamID into a serialized message. Only the
/// protobuf header and the extended header have room for them; messages with
/// a plain header come back unchanged.
fn stamp_identity(
    msg: &dyn IClientMsg,
    mut bytes: Vec<u8>,
    session_id: Option<i32>,
    steam_id: Option<SteamID>,
) -> Vec<u8> {
    if msg.is_proto() {
        // Layout: message type (4 bytes), protobuf header length (4 bytes), the
        // protobuf header, then the body.
        if bytes.len() < 8 {
            return bytes;
        }

        let header_length = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if header_length < 0 {
            return bytes;
        }

        let header_length = header_length as usize;
        if bytes.len() < 8 + header_length {
            return bytes;
        }

        let Ok(mut header) = CMsgProtoBufHeader::decode(&bytes[8..8 + header_length]) else {
            return bytes;
        };

        if let Some(session_id) = session_id {
            header.client_sessionid = Some(session_id);
        }
        if let Some(steam_id) = steam_id {
            header.steamid = Some(steam_id.raw_u64());
        }

        let encoded = header.encode_to_vec();

        let mut stamped = Vec::with_capacity(bytes.len() + encoded.len());
        stamped.extend_from_slice(&bytes[..4]);
        stamped.extend_from_slice(&(encoded.len() as i32).to_le_bytes());
        stamped.extend_from_slice(&encoded);
        stamped.extend_from_slice(&bytes[8 + header_length..]);
        return stamped;
    }

    // The extended header: message type (4), header size (1), header version
    // (2), target job id (8), source job id (8), canary (1), SteamID (8) and
    // session id (4).
    if bytes.len() >= EXTENDED_HEADER_SIZE && bytes[4] as usize == EXTENDED_HEADER_SIZE {
        if let Some(steam_id) = steam_id {
            bytes[EXTENDED_HEADER_STEAM_ID_OFFSET..EXTENDED_HEADER_STEAM_ID_OFFSET + 8]
                .copy_from_slice(&steam_id.raw_u64().to_le_bytes());
        }
        if let Some(session_id) = session_id {
            bytes[EXTENDED_HEADER_SESSION_ID_OFFSET..EXTENDED_HEADER_SESSION_ID_OFFSET + 4]
                .copy_from_slice(&session_id.to_le_bytes());
        }
    }

    bytes
}

/// Whether two handles point at the same connection. Only the data pointers
/// are compared, since the vtable half of a `dyn` pointer isn't guaranteed to
/// be unique.
fn same_connection(a: &Arc<dyn Connection>, b: &Arc<dyn Connection>) -> bool {
    std::ptr::eq(Arc::as_ptr(a) as *const (), Arc::as_ptr(b) as *const ())
}

fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>, SteamError> {
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|error| SteamError::Decompression(error.to_string()))?;
    Ok(decompressed)
}
