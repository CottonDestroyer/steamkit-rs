//! The client that connects to the Steam network.
//!
//! It owns a [`CMClient`] for the connection itself and adds the message
//! handlers and the callback queue on top.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::base::client_msg::IClientMsg;
use crate::base::generated::steam_language::EMsg;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamError;
use crate::steam::authentication::steam_authentication::SteamAuthentication;
use crate::steam::cm_client::{CMClient, ClientMsgDispatcher, ConnectionFactory};
use crate::steam::discovery::server_record::ServerRecord;
use crate::steam::discovery::smart_cm_server_list::SmartCMServerList;
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::handlers::steam_apps::steam_apps::SteamApps;
use crate::steam::handlers::steam_auth_ticket::steam_auth_ticket::SteamAuthTicket;
use crate::steam::handlers::steam_cloud::steam_cloud::SteamCloud;
use crate::steam::handlers::steam_content::steam_content::SteamContent;
use crate::steam::handlers::steam_friends::steam_friends::SteamFriends;
use crate::steam::handlers::steam_game_coordinator::steam_game_coordinator::SteamGameCoordinator;
use crate::steam::handlers::steam_game_server::steam_game_server::SteamGameServer;
use crate::steam::handlers::steam_master_server::steam_master_server::SteamMasterServer;
use crate::steam::handlers::steam_matchmaking::steam_matchmaking::SteamMatchmaking;
use crate::steam::handlers::steam_networking::steam_networking::SteamNetworking;
use crate::steam::handlers::steam_screenshots::steam_screenshots::SteamScreenshots;
use crate::steam::handlers::steam_unified_messages::steam_unified_messages::SteamUnifiedMessages;
use crate::steam::handlers::steam_user::steam_user::SteamUser;
use crate::steam::handlers::steam_user_stats::steam_user_stats::SteamUserStats;
use crate::steam::handlers::steam_workshop::steam_workshop::SteamWorkshop;
use crate::steam::steam_client::async_job_manager::AsyncJobManager;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::callback_mgr::callback_mgr::{CallbackManager, SubscriptionId};
use crate::steam::steam_client::callbacks::{ConnectedCallback, DisconnectedCallback};
use crate::steam::steam_client::configuration::steam_configuration::SteamConfiguration;
use crate::types::job_id::{AsyncJob, JobID};

/// Seconds between the Unix epoch and the epoch of Steam's global ids
/// (2005-01-01T00:00:00Z).
const GLOBAL_ID_EPOCH_SECONDS: u64 = 1_104_537_600;

/// A registered client message handler.
struct HandlerEntry {
    type_id: Option<TypeId>,
    handler: Arc<dyn ClientMsgHandler>,
    any: Option<Arc<dyn Any + Send + Sync>>,
}

/// Forwards the [`CMClient`] events to the [`SteamClient`].
struct SteamClientDispatcher {
    client: Weak<SteamClient>,
}

#[async_trait]
impl ClientMsgDispatcher for SteamClientDispatcher {
    async fn on_client_msg_received(&self, _client: &CMClient, packet: &dyn IPacketMsg) -> bool {
        match self.client.upgrade() {
            Some(client) => client.on_client_msg_received(packet).await,
            None => false,
        }
    }

    async fn on_client_connected(&self, _client: &CMClient) {
        if let Some(client) = self.client.upgrade() {
            client.on_client_connected().await;
        }
    }

    async fn on_client_disconnected(&self, _client: &CMClient, user_initiated: bool) {
        if let Some(client) = self.client.upgrade() {
            client.on_client_disconnected(user_initiated).await;
        }
    }
}

/// A client that connects to the Steam network. Also keeps the message handlers
/// and the callback queue.
pub struct SteamClient {
    cm: Arc<CMClient>,
    weak: Weak<SteamClient>,
    handlers: RwLock<Vec<HandlerEntry>>,
    current_job_id: AtomicU32,
    process_start_time: u32,
    callback_queue_tx: mpsc::UnboundedSender<Box<dyn CallbackMsg>>,
    callback_queue_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Box<dyn CallbackMsg>>>,
    job_manager: AsyncJobManager,
    callback_manager: CallbackManager,
    authentication: std::sync::OnceLock<SteamAuthentication>,
}

impl fmt::Debug for SteamClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamClient")
            .field("id", &self.cm.id())
            .field("is_connected", &self.cm.is_connected())
            .field("handlers", &self.handlers.read().len())
            .finish()
    }
}

impl SteamClient {
    /// Creates a client with the default configuration.
    pub fn new() -> Arc<SteamClient> {
        Self::with_configuration(Arc::new(SteamConfiguration::create_default()))
    }

    /// Creates a client with the given configuration.
    pub fn with_config(config: SteamConfiguration) -> Arc<SteamClient> {
        Self::with_configuration(Arc::new(config))
    }

    /// Creates a client with the default configuration and the given identifier.
    pub fn with_identifier(identifier: impl Into<String>) -> Arc<SteamClient> {
        Self::with_configuration_and_identifier(
            Arc::new(SteamConfiguration::create_default()),
            identifier,
        )
    }

    /// Creates a client with the given shared configuration.
    pub fn with_configuration(configuration: Arc<SteamConfiguration>) -> Arc<SteamClient> {
        Self::with_configuration_and_identifier(configuration, random_identifier())
    }

    /// Creates a client with the given configuration and identifier. An empty
    /// identifier is replaced with a generated one.
    pub fn with_configuration_and_identifier(
        configuration: Arc<SteamConfiguration>,
        identifier: impl Into<String>,
    ) -> Arc<SteamClient> {
        let identifier = identifier.into();
        let identifier = if identifier.is_empty() {
            random_identifier()
        } else {
            identifier
        };

        // Start gathering machine info now so it's (hopefully) ready by the
        // time we log on.
        crate::util::hardware_utils::HardwareUtils::init(configuration.machine_info_provider().clone());

        let (callback_queue_tx, callback_queue_rx) = mpsc::unbounded_channel::<Box<dyn CallbackMsg>>();

        let client = Arc::new_cyclic(|weak| {
            let dispatcher: Arc<dyn ClientMsgDispatcher> =
                Arc::new(SteamClientDispatcher { client: weak.clone() });
            let cm = Arc::new(CMClient::new(configuration.clone(), &identifier, dispatcher));

            SteamClient {
                cm,
                weak: weak.clone(),
                handlers: RwLock::new(Vec::new()),
                current_job_id: AtomicU32::new(0),
                process_start_time: capture_process_start_time(),
                callback_queue_tx,
                callback_queue_rx: tokio::sync::Mutex::new(callback_queue_rx),
                job_manager: AsyncJobManager::new(),
                callback_manager: CallbackManager::new(),
                authentication: std::sync::OnceLock::new(),
            }
        });

        add_default_handlers(&client);

        client
    }

    /// The configuration for this client.
    pub fn configuration(&self) -> &SteamConfiguration {
        self.cm.configuration()
    }

    /// The configuration, shared with any other clients using it.
    pub fn configuration_arc(&self) -> &Arc<SteamConfiguration> {
        self.cm.configuration()
    }

    /// A unique identifier for this client.
    pub fn identifier(&self) -> &str {
        self.cm.id()
    }

    /// Same as [`SteamClient::identifier`].
    pub fn id(&self) -> &str {
        self.cm.id()
    }

    /// The list of CM servers.
    pub fn servers(&self) -> &Arc<SmartCMServerList> {
        self.cm.servers()
    }

    /// The local IP of the connection.
    pub fn local_ip(&self) -> Option<std::net::IpAddr> {
        self.cm.local_ip()
    }

    /// The endpoint we're connected to.
    pub fn current_end_point(&self) -> Option<std::net::SocketAddr> {
        self.cm.current_end_point()
    }

    /// Our public IP address, known after a successful logon.
    pub fn public_ip(&self) -> Option<std::net::IpAddr> {
        self.cm.public_ip()
    }

    /// The country code of our public IP, according to Steam.
    pub fn ip_country_code(&self) -> Option<String> {
        self.cm.ip_country_code()
    }

    /// The country code of the account.
    pub fn user_country_code(&self) -> Option<String> {
        self.cm.user_country_code()
    }

    /// The universe of this client.
    pub fn universe(&self) -> crate::base::generated::steam_language::EUniverse {
        self.cm.universe()
    }

    /// Whether we're connected to a CM server.
    pub fn is_connected(&self) -> bool {
        self.cm.is_connected()
    }

    /// The session token assigned by the server.
    pub fn session_token(&self) -> u64 {
        self.cm.session_token()
    }

    /// The cell id Steam recommends for this client.
    pub fn cell_id(&self) -> Option<u32> {
        self.cm.cell_id()
    }

    /// The session id.
    pub fn session_id(&self) -> Option<i32> {
        self.cm.session_id()
    }

    /// The SteamID.
    pub fn steam_id(&self) -> Option<crate::types::steam_id::SteamID> {
        self.cm.steam_id()
    }

    /// The timeout for connecting to a CM server.
    pub fn connection_timeout(&self) -> Duration {
        self.cm.connection_timeout()
    }

    /// The debug network listener, if any.
    pub fn debug_network_listener(
        &self,
    ) -> Option<Arc<dyn crate::util::debug_network_listener::IDebugNetworkListener>> {
        self.cm.debug_network_listener()
    }

    /// Sets the debug network listener.
    pub fn set_debug_network_listener(
        &self,
        listener: Option<Arc<dyn crate::util::debug_network_listener::IDebugNetworkListener>>,
    ) {
        self.cm.set_debug_network_listener(listener);
    }

    /// Whether the server is about to close the connection on purpose (e.g. after a log off).
    pub fn expect_disconnection(&self) -> bool {
        self.cm.expect_disconnection()
    }

    /// Sets whether the server is about to close the connection on purpose.
    pub fn set_expect_disconnection(&self, expect: bool) {
        self.cm.set_expect_disconnection(expect);
    }

    /// Sets the factory that creates this client's connections.
    pub fn set_connection_factory(&self, factory: ConnectionFactory) {
        self.cm.set_connection_factory(factory);
    }

    /// The underlying CM client.
    pub fn cm_client(&self) -> &Arc<CMClient> {
        &self.cm
    }

    /// The job manager tracking this client's pending requests.
    pub fn job_manager(&self) -> &AsyncJobManager {
        &self.job_manager
    }

    /// The callback manager that routes callbacks to subscribers.
    pub fn callback_manager(&self) -> &CallbackManager {
        &self.callback_manager
    }

    /// The authentication handler.
    pub fn authentication(&self) -> &SteamAuthentication {
        self.authentication
            .get_or_init(|| SteamAuthentication::new(self.weak.clone()))
    }

    /// Adds a message handler. Adding the same handler twice keeps the first.
    ///
    /// A handler added this way can't be fetched with
    /// [`SteamClient::get_handler`], since its type isn't known here; use
    /// [`SteamClient::add_typed_handler`] for that.
    pub fn add_handler(&self, handler: Arc<dyn ClientMsgHandler>) {
        let mut handlers = self.handlers.write();

        if handlers.iter().any(|entry| Arc::ptr_eq(&entry.handler, &handler)) {
            tracing::warn!("the handler is already registered");
            return;
        }

        handlers.push(HandlerEntry {
            type_id: None,
            handler,
            any: None,
        });
    }

    /// Adds a message handler that can be fetched with
    /// [`SteamClient::get_handler`] later.
    pub fn add_typed_handler<T>(&self, handler: T) -> Result<(), SteamError>
    where
        T: ClientMsgHandler + 'static,
    {
        let type_id = TypeId::of::<T>();

        if self.handlers.read().iter().any(|entry| entry.type_id == Some(type_id)) {
            return Err(SteamError::InvalidArgument(format!(
                "a handler of type \"{:?}\" is already registered",
                type_id
            )));
        }

        let handler: Arc<T> = Arc::new(handler);
        let as_handler: Arc<dyn ClientMsgHandler> = handler.clone();
        let as_any: Arc<dyn Any + Send + Sync> = handler;

        self.handlers.write().push(HandlerEntry {
            type_id: Some(type_id),
            handler: as_handler,
            any: Some(as_any),
        });

        Ok(())
    }

    /// Removes a handler.
    pub fn remove_handler(&self, handler: &Arc<dyn ClientMsgHandler>) -> bool {
        let mut handlers = self.handlers.write();
        let before = handlers.len();
        handlers.retain(|entry| !Arc::ptr_eq(&entry.handler, handler));
        handlers.len() != before
    }

    /// Removes the handler of type `T`.
    pub fn remove_handler_type<T>(&self) -> bool
    where
        T: ClientMsgHandler + 'static,
    {
        let type_id = TypeId::of::<T>();
        let mut handlers = self.handlers.write();
        let before = handlers.len();
        handlers.retain(|entry| entry.type_id != Some(type_id));
        handlers.len() != before
    }

    /// The handler of type `T`, if it's registered.
    pub fn get_handler<T>(&self) -> Option<Arc<T>>
    where
        T: ClientMsgHandler + 'static,
    {
        let type_id = TypeId::of::<T>();
        let handlers = self.handlers.read();
        let entry = handlers.iter().find(|entry| entry.type_id == Some(type_id))?;
        entry.any.clone()?.downcast::<T>().ok()
    }

    /// The handler of type `T`, or an error if it isn't registered.
    pub fn get_required_handler<T>(&self) -> Result<Arc<T>, SteamError>
    where
        T: ClientMsgHandler + 'static,
    {
        self.get_handler::<T>().ok_or_else(|| {
            SteamError::NotFound(format!(
                "a handler of type \"{:?}\" is not registered",
                TypeId::of::<T>()
            ))
        })
    }

    /// Takes the next callback off the queue, if there is one.
    pub fn get_callback(&self) -> Option<Box<dyn CallbackMsg>> {
        let mut receiver = self.callback_queue_rx.try_lock().ok()?;
        receiver.try_recv().ok()
    }

    /// Waits for the next callback and takes it off the queue.
    pub async fn wait_for_callback(&self) -> Result<Box<dyn CallbackMsg>, SteamError> {
        let mut receiver = self.callback_queue_rx.lock().await;
        receiver.recv().await.ok_or(SteamError::Cancelled)
    }

    /// Waits for the next callback and takes it off the queue. `None` if the
    /// timeout runs out first.
    pub async fn wait_for_callback_timeout(&self, timeout: Duration) -> Option<Box<dyn CallbackMsg>> {
        let mut receiver = self.callback_queue_rx.lock().await;
        match tokio::time::timeout(timeout, receiver.recv()).await {
            Ok(callback) => callback,
            Err(_) => None,
        }
    }

    /// Waits for the next callback and takes it off the queue.
    pub async fn wait_for_callback_async(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn CallbackMsg>, SteamError> {
        let mut receiver = self.callback_queue_rx.lock().await;
        tokio::select! {
            _ = cancellation.cancelled() => Err(SteamError::Cancelled),
            callback = receiver.recv() => callback.ok_or(SteamError::Cancelled),
        }
    }

    /// Posts a callback to the queue, and hands it to the job waiting for it, if
    /// any. Mostly used by message handlers.
    pub fn post_callback(&self, msg: Box<dyn CallbackMsg>) {
        let job_id = msg.job_id();

        // The job manager clones what it needs, so the queue keeps the original.
        self.job_manager.try_complete_job(job_id, &*msg);

        let _ = self.callback_queue_tx.send(msg);
    }

    /// Posts a callback of a known type.
    pub fn post_callback_typed<T>(&self, msg: T)
    where
        T: CallbackMsg + 'static,
    {
        self.post_callback(Box::new(msg));
    }

    /// Runs one queued callback. Returns `false` if the queue was empty.
    pub fn run_callbacks(&self) -> bool {
        match self.get_callback() {
            Some(callback) => {
                self.callback_manager.dispatch(callback);
                true
            }
            None => false,
        }
    }

    /// Waits for one callback and runs it.
    pub async fn run_wait_callbacks(&self) -> Result<(), SteamError> {
        let callback = self.wait_for_callback().await?;
        self.callback_manager.dispatch(callback);
        Ok(())
    }

    /// Waits for one callback and runs it. Returns `false` if the timeout runs
    /// out first.
    pub async fn run_wait_callbacks_timeout(&self, timeout: Duration) -> bool {
        match self.wait_for_callback_timeout(timeout).await {
            Some(callback) => {
                self.callback_manager.dispatch(callback);
                true
            }
            None => false,
        }
    }

    /// Runs all queued callbacks, waiting up to `timeout` for the first one.
    pub async fn run_wait_all_callbacks(&self, timeout: Duration) {
        if !self.run_wait_callbacks_timeout(timeout).await {
            return;
        }

        while self.run_callbacks() {}
    }

    /// Subscribes `callback_func` to callbacks of type `T`.
    pub fn subscribe<T>(&self, callback_func: Box<dyn Fn(&T) + Send + Sync>) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.callback_manager.subscribe(callback_func)
    }

    /// Subscribes `callback_func` to callbacks of type `T` for one job.
    pub fn subscribe_job<T>(
        &self,
        job_id: JobID,
        callback_func: Box<dyn Fn(&T) + Send + Sync>,
    ) -> SubscriptionId
    where
        T: CallbackMsg + 'static,
    {
        self.callback_manager.subscribe_job(job_id, callback_func)
    }

    /// Removes a subscription.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.callback_manager.unsubscribe(id);
    }

    /// The next job id for a job-based message. Thread-safe.
    pub fn get_next_job_id(&self) -> JobID {
        let sequence = self.current_job_id.fetch_add(1, Ordering::SeqCst).wrapping_add(1) & 0xF_FFFF;
        let value = (u64::from(self.process_start_time) << 20) | u64::from(sequence);
        JobID::from_value(value)
    }

    /// Creates and registers a job for a request that's about to be sent. Fails
    /// right away if the client isn't connected.
    pub fn start_job<T>(&self, job_id: JobID) -> AsyncJob<T>
    where
        T: CallbackMsg + Clone + 'static,
    {
        let job = self.job_manager.start_job::<T>(job_id);

        if !self.is_connected() {
            self.job_manager.fail_job(job_id);
        }

        job
    }

    /// Connects to a CM server; if `cm_server` is `None`, the next one from the
    /// server list is used.
    ///
    /// A logged-on session that the server closes is moved to another server
    /// automatically. Any other disconnect is posted as a `DisconnectedCallback`,
    /// and it's up to you to call this again.
    pub async fn connect(&self, cm_server: Option<ServerRecord>) -> Result<(), SteamError> {
        self.cm.connect(cm_server).await
    }

    /// Disconnects.
    pub async fn disconnect(&self) {
        self.cm.disconnect().await;
    }

    /// Sends a client message.
    pub fn send(&self, msg: &dyn IClientMsg) -> Result<(), SteamError> {
        self.cm.send(msg)
    }

    /// Writes a line to the debug log.
    pub fn log_debug(&self, category: &str, message: &str) {
        self.cm.log_debug(category, message);
    }

    /// Feeds a packet into the client as if it came from the connection, so
    /// tests can drive the handlers without a live connection. Returns `true`
    /// if every handler accepted it.
    pub async fn receive_test_packet_msg(&self, packet: &dyn IPacketMsg) -> bool {
        self.on_client_msg_received(packet).await
    }

    /// Handles a received client message, after the [`CMClient`] has had its
    /// turn.
    async fn on_client_msg_received(&self, packet: &dyn IPacketMsg) -> bool {
        // Some messages are handled here before the registered handlers see
        // them.
        match packet.msg_type() {
            EMsg::JobHeartbeat => self.job_manager.heartbeat_job(packet.target_job_id()),
            EMsg::DestJobFailed => self.job_manager.fail_job(packet.target_job_id()),
            _ => {}
        }

        let handlers: Vec<Arc<dyn ClientMsgHandler>> =
            self.handlers.read().iter().map(|entry| entry.handler.clone()).collect();

        for handler in handlers {
            if let Err(error) = handler.handle_msg(self, packet).await {
                self.log_debug("SteamClient", &format!("Unhandled exception from handler: {}", error));
                self.cm.disconnect_with(false).await;
                return false;
            }
        }

        true
    }

    /// Called once the client is securely connected.
    async fn on_client_connected(&self) {
        self.job_manager.set_timeouts_enabled(true);
        self.post_callback_typed(ConnectedCallback::new());
    }

    /// Called when the connection is gone.
    async fn on_client_disconnected(&self, user_initiated: bool) {
        self.post_callback_typed(DisconnectedCallback::new(user_initiated));

        // We're disconnected, so cancel all pending jobs.
        self.job_manager.cancel_pending_jobs();
        self.job_manager.set_timeouts_enabled(false);

        self.clear_handler_caches();
    }

    fn clear_handler_caches(&self) {
        if let Some(handler) = self.get_handler::<SteamMatchmaking>() {
            handler.clear_lobby_cache();
        }
    }
}

/// Adds the built-in handlers. SteamFriends has to come before SteamUser
/// because of `AccountInfoCallback`.
fn add_default_handlers(client: &Arc<SteamClient>) {
    let _ = client.add_typed_handler(SteamFriends::new(client));
    let _ = client.add_typed_handler(SteamUser::new(client));
    let _ = client.add_typed_handler(SteamApps::new(client));
    let _ = client.add_typed_handler(SteamGameCoordinator::new(client));
    let _ = client.add_typed_handler(SteamGameServer::new(client));
    let _ = client.add_typed_handler(SteamUserStats::new(client));
    let _ = client.add_typed_handler(SteamMasterServer::new(client));
    let _ = client.add_typed_handler(SteamCloud::new(client));
    let _ = client.add_typed_handler(SteamWorkshop::new(client));
    let _ = client.add_typed_handler(SteamUnifiedMessages::new(client));
    let _ = client.add_typed_handler(SteamScreenshots::new(client));
    let _ = client.add_typed_handler(SteamMatchmaking::new(client));
    let _ = client.add_typed_handler(SteamNetworking::new(client));
    let _ = client.add_typed_handler(SteamContent::new(client));
    let _ = client.add_typed_handler(SteamAuthTicket::new(client));
}

fn random_identifier() -> String {
    format!("{:032x}", rand::random::<u128>())
}

fn capture_process_start_time() -> u32 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (now.as_secs().saturating_sub(GLOBAL_ID_EPOCH_SECONDS) as u32) & 0x3FFF_FFFF
}
