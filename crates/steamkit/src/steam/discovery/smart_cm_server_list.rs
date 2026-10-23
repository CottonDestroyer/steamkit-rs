//! The list of known CM servers: tracks which ones are good or bad and picks
//! the next one to try.
//!
//! It doesn't hold the `SteamConfiguration` (which owns the list), only the
//! [`SteamConfigurationState`] it needs, to avoid a reference cycle.
//!
//! The Steam Directory query goes through a pluggable loader:
//!
//! * [`SmartCMServerList::new`] uses the Steam Directory Web API. Its HTTP
//!   client is created on first use, so creating a configuration never calls
//!   the configured HTTP client factory.
//! * [`SmartCMServerList::with_directory_loader`] takes your own loader.
//! * [`SmartCMServerList::with_provider`] doesn't use the directory at all.

use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, OnceLock, RwLock as StdRwLock};
use std::time::{Duration, SystemTime};

use futures::future::BoxFuture;
use parking_lot::RwLock;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::error::SteamError;
use crate::networking::steam3::protocol_type::ProtocolTypes;
use crate::steam::steam_client::configuration::steam_configuration::{
    default_web_api_base_address, HttpClientFactory, HttpClientPurpose,
};
use crate::steam::steam_client::configuration::steam_configuration_state::SteamConfigurationState;
use crate::steam::webapi::steam_directory::SteamDirectory;
use crate::steam::webapi::web_api::AsyncInterface;

use super::i_server_list_provider::IServerListProvider;
use super::server_record::ServerRecord;

/// Fallback WebSocket server, used if the server list can't be fetched any
/// other way.
static DEFAULT_SERVER_WEBSOCKET: LazyLock<StdRwLock<String>> = LazyLock::new(|| {
    StdRwLock::new(String::from("cmp1-sea1.steamserver.net:443"))
});

/// Fallback TCP/UDP server, used if the server list can't be fetched any other
/// way.
static DEFAULT_SERVER_NETFILTER: LazyLock<StdRwLock<String>> = LazyLock::new(|| {
    StdRwLock::new(String::from("ext1-sea1.steamserver.net:27017"))
});

/// Timeout of the Steam Directory request.
const DIRECTORY_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The quality a server has been marked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerQuality {
    /// Known good server.
    Good,
    /// Known bad server.
    Bad,
}

/// Loads the list of CM servers from the Steam Directory.
pub type ServerListDirectoryLoader =
    Arc<dyn Fn() -> BoxFuture<'static, Result<Vec<ServerRecord>, SteamError>> + Send + Sync>;

/// A directory loader that fetches the CM list from the Steam Directory Web API.
///
/// * `base_address` - base address of the Web API.
/// * `cell_id` - the cell id to send along.
/// * `api_key` - optional Web API key.
/// * `http_client` - the HTTP client to use; a default one is created if `None`.
pub fn steam_directory_loader(
    base_address: reqwest::Url,
    cell_id: u32,
    api_key: Option<String>,
    http_client: Option<reqwest::Client>,
) -> ServerListDirectoryLoader {
    Arc::new(move || {
        let base_address = base_address.clone();
        let cell_id = cell_id;
        let api_key = api_key.clone();
        let http_client = http_client.clone();

        Box::pin(async move {
            load_server_list_from_steam_directory(
                base_address,
                cell_id,
                api_key.as_deref(),
                http_client,
            )
            .await
        })
    })
}

/// Like [`steam_directory_loader`], but the HTTP client is built from `factory`
/// the first time the directory is queried, and reused after that. That way
/// creating a configuration doesn't call the user's HTTP client factory.
pub fn steam_directory_loader_with_factory(
    base_address: reqwest::Url,
    cell_id: u32,
    api_key: Option<String>,
    factory: HttpClientFactory,
) -> ServerListDirectoryLoader {
    let client = Arc::new(OnceLock::new());

    Arc::new(move || {
        let base_address = base_address.clone();
        let cell_id = cell_id;
        let api_key = api_key.clone();
        let factory = factory.clone();
        let client = client.clone();

        Box::pin(async move {
            let http_client = client
                .get_or_init(|| factory(HttpClientPurpose::WebAPI))
                .clone();

            load_server_list_from_steam_directory(
                base_address,
                cell_id,
                api_key.as_deref(),
                Some(http_client),
            )
            .await
        })
    })
}

/// Loads the server list from the Steam Directory Web API
/// (`ISteamDirectory/GetCMListForConnect`), keeping the `websockets` and
/// `netfilter` servers.
pub async fn load_server_list_from_steam_directory(
    base_address: reqwest::Url,
    cell_id: u32,
    api_key: Option<&str>,
    http_client: Option<reqwest::Client>,
) -> Result<Vec<ServerRecord>, SteamError> {
    let client = match http_client {
        Some(client) => client,
        None => reqwest::Client::builder()
            .timeout(DIRECTORY_REQUEST_TIMEOUT)
            .build()
            .map_err(|error| SteamError::Connection(error.to_string()))?,
    };

    let directory = AsyncInterface::new(
        client,
        base_address,
        "ISteamDirectory",
        api_key.unwrap_or_default(),
    );

    SteamDirectory::load_with_interface(&directory, cell_id, None, CancellationToken::new()).await
}

/// Bookkeeping for one server / protocol combination.
#[derive(Clone, Debug)]
struct ServerInfo {
    record: ServerRecord,
    protocol: ProtocolTypes,
    last_bad_connection_time_utc: Option<SystemTime>,
}

/// The mutable state of the server list.
#[derive(Debug, Default)]
struct ServerListState {
    servers: Vec<ServerInfo>,
    servers_last_refresh: Option<SystemTime>,
}

/// Smart list of CM servers.
pub struct SmartCMServerList {
    provider: Arc<dyn IServerListProvider>,
    allow_directory_fetch: bool,
    directory_loader: Option<ServerListDirectoryLoader>,
    state: RwLock<ServerListState>,
    resolve_lock: Mutex<()>,
    server_list_before_refresh_time_span: AtomicU64,
    bad_connection_memory_time_span: AtomicU64,
}

impl fmt::Debug for SmartCMServerList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmartCMServerList")
            .field("allow_directory_fetch", &self.allow_directory_fetch)
            .field("has_directory_loader", &self.directory_loader.is_some())
            .finish()
    }
}

impl SmartCMServerList {
    /// Creates a list from the configuration state, which supplies the server
    /// list provider, whether directory fetches are allowed, and the directory
    /// settings.
    pub fn new(state: Arc<SteamConfigurationState>) -> Self {
        let provider = state.server_list_provider.clone();
        let allow_directory_fetch = state.allow_directory_fetch;

        let directory_loader = if allow_directory_fetch {
            Some(steam_directory_loader_with_factory(
                state.web_api_base_address.clone(),
                state.cell_id,
                Some(state.web_api_key.clone()),
                state.http_client_factory.clone(),
            ))
        } else {
            None
        };

        let mut list = SmartCMServerList::with_provider(provider, allow_directory_fetch);
        list.directory_loader = directory_loader;
        list
    }

    /// Creates a list that only uses the given provider (no directory fetch).
    pub fn with_provider(
        provider: Arc<dyn IServerListProvider>,
        allow_directory_fetch: bool,
    ) -> Self {
        SmartCMServerList {
            provider,
            allow_directory_fetch,
            directory_loader: None,
            state: RwLock::new(ServerListState::default()),
            resolve_lock: Mutex::new(()),
            server_list_before_refresh_time_span: AtomicU64::new(
                Duration::from_secs(7 * 24 * 60 * 60).as_millis() as u64,
            ),
            bad_connection_memory_time_span: AtomicU64::new(
                Duration::from_secs(5 * 60).as_millis() as u64,
            ),
        }
    }

    /// Creates a list with a provider and your own Steam Directory loader.
    pub fn with_directory_loader(
        provider: Arc<dyn IServerListProvider>,
        allow_directory_fetch: bool,
        directory_loader: ServerListDirectoryLoader,
    ) -> Self {
        SmartCMServerList {
            allow_directory_fetch,
            directory_loader: Some(directory_loader),
            ..SmartCMServerList::with_provider(provider, allow_directory_fetch)
        }
    }

    /// Creates a list with a provider and the Steam Directory Web API.
    pub fn with_steam_directory(
        provider: Arc<dyn IServerListProvider>,
        allow_directory_fetch: bool,
        cell_id: u32,
    ) -> Self {
        SmartCMServerList::with_directory_loader(
            provider,
            allow_directory_fetch,
            steam_directory_loader(
                default_web_api_base_address(),
                cell_id,
                None,
                None,
            ),
        )
    }

    /// Fallback WebSocket server, used if the server list can't be fetched any
    /// other way.
    pub fn default_server_websocket() -> String {
        DEFAULT_SERVER_WEBSOCKET
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Sets the fallback WebSocket server.
    pub fn set_default_server_websocket(value: &str) {
        let mut guard = DEFAULT_SERVER_WEBSOCKET
            .write()
            .unwrap_or_else(|error| error.into_inner());
        *guard = value.to_string();
    }

    /// Fallback TCP/UDP server, used if the server list can't be fetched any
    /// other way.
    pub fn default_server_netfilter() -> String {
        DEFAULT_SERVER_NETFILTER
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Sets the fallback TCP/UDP server.
    pub fn set_default_server_netfilter(value: &str) {
        let mut guard = DEFAULT_SERVER_NETFILTER
            .write()
            .unwrap_or_else(|error| error.into_inner());
        *guard = value.to_string();
    }

    /// How long the cached server list is used before refreshing it from the
    /// Steam Directory.
    pub fn server_list_before_refresh_time_span(&self) -> Duration {
        Duration::from_millis(self.server_list_before_refresh_time_span.load(Ordering::Relaxed))
    }

    /// Sets how long the cached server list is used before refreshing it.
    pub fn set_server_list_before_refresh_time_span(&self, value: Duration) {
        self.server_list_before_refresh_time_span
            .store(value.as_millis() as u64, Ordering::Relaxed);
    }

    /// How long a server's bad state is remembered.
    pub fn bad_connection_memory_time_span(&self) -> Duration {
        Duration::from_millis(self.bad_connection_memory_time_span.load(Ordering::Relaxed))
    }

    /// Sets how long a server's bad state is remembered.
    pub fn set_bad_connection_memory_time_span(&self, value: Duration) {
        self.bad_connection_memory_time_span
            .store(value.as_millis() as u64, Ordering::Relaxed);
    }

    /// Resets servers whose last bad connection is older than
    /// [`SmartCMServerList::bad_connection_memory_time_span`].
    pub fn reset_old_scores(&self) {
        let cutoff = SystemTime::now()
            .checked_sub(self.bad_connection_memory_time_span())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let mut state = self.state.write();
        for server in state.servers.iter_mut() {
            if let Some(last_bad) = server.last_bad_connection_time_utc {
                if last_bad < cutoff {
                    server.last_bad_connection_time_utc = None;
                }
            }
        }
    }

    /// Resets the state of all servers.
    pub fn reset_bad_servers(&self) {
        let mut state = self.state.write();
        for server in state.servers.iter_mut() {
            server.last_bad_connection_time_utc = None;
        }
    }

    /// Replaces the list.
    ///
    /// * `write_provider` - also store the new list in the server list provider.
    /// * `servers_time` - when the list was last updated; now if `None`.
    pub async fn replace_list(
        &self,
        endpoint_list: Vec<ServerRecord>,
        write_provider: bool,
        servers_time: Option<SystemTime>,
    ) -> Result<(), SteamError> {
        let mut distinct_end_points: Vec<ServerRecord> = Vec::with_capacity(endpoint_list.len());
        for record in endpoint_list {
            if !distinct_end_points.contains(&record) {
                distinct_end_points.push(record);
            }
        }

        {
            let mut state = self.state.write();
            state.servers_last_refresh = Some(servers_time.unwrap_or_else(SystemTime::now));
            state.servers.clear();

            for record in &distinct_end_points {
                add_core(&mut state.servers, record);
            }
        }

        if write_provider {
            self.provider.update_server_list(distinct_end_points).await?;
        }

        Ok(())
    }

    /// Marks the quality of a server. Returns `true` if the server is in the
    /// list.
    pub fn try_mark(
        &self,
        end_point: SocketAddr,
        protocol_types: ProtocolTypes,
        quality: ServerQuality,
    ) -> bool {
        let now = SystemTime::now();
        let host = end_point.ip().to_string();

        let mut state = self.state.write();

        let matched: Vec<usize> = state
            .servers
            .iter()
            .enumerate()
            .filter(|(_, server)| match quality {
                ServerQuality::Good => {
                    server.record.end_point() == end_point
                        && server.protocol.has(protocol_types)
                }
                // A failure marks every endpoint of the host.
                ServerQuality::Bad => {
                    server.record.get_host() == host || server.record.end_point() == end_point
                }
            })
            .map(|(index, _)| index)
            .collect();

        if matched.is_empty() {
            return false;
        }

        for index in matched {
            match quality {
                ServerQuality::Good => state.servers[index].last_bad_connection_time_utc = None,
                ServerQuality::Bad => state.servers[index].last_bad_connection_time_utc = Some(now),
            }
        }

        true
    }

    /// Marks a server as bad; every endpoint of that host is marked at the same time.
    pub fn mark_server_as_bad(&self, end_point: SocketAddr) -> bool {
        self.try_mark(end_point, ProtocolTypes::ALL, ServerQuality::Bad)
    }

    /// Marks a server as good.
    pub fn mark_server_as_good(
        &self,
        end_point: SocketAddr,
        protocol_types: ProtocolTypes,
    ) -> bool {
        self.try_mark(end_point, protocol_types, ServerQuality::Good)
    }

    /// The next server to try that supports at least
    /// `supported_protocol_types`. Fetches the list first if needed; `None` if
    /// there's no candidate or the fetch failed.
    pub async fn get_next_server_candidate_async(
        &self,
        supported_protocol_types: ProtocolTypes,
    ) -> Option<ServerRecord> {
        if let Err(error) = self.start_fetching_servers().await {
            tracing::debug!("Failed to retrieve server list: {error}");
            return None;
        }

        self.get_next_server_candidate_internal(supported_protocol_types)
    }

    /// The next server to try, from the list as it is now (doesn't fetch).
    pub fn get_next_server_candidate(
        &self,
        supported_protocol_types: ProtocolTypes,
    ) -> Option<ServerRecord> {
        let state = self.state.read();

        best_candidate(&state.servers, supported_protocol_types)
    }

    /// All servers in the list.
    pub fn get_all_end_points(&self) -> Vec<ServerRecord> {
        let state = self.state.read();
        let mut end_points: Vec<ServerRecord> = Vec::with_capacity(state.servers.len());

        for server in &state.servers {
            if !end_points.contains(&server.record) {
                end_points.push(server.record.clone());
            }
        }

        end_points
    }

    /// Refreshes the server list: from the Web API first if directory fetches
    /// are allowed, then from the provider.
    pub async fn force_refresh_server_list(&self) -> Result<(), SteamError> {
        let _guard = self.resolve_lock.lock().await;

        self.resolve_server_list(true).await
    }

    /// Picks the candidate from the current list.
    fn get_next_server_candidate_internal(
        &self,
        supported_protocol_types: ProtocolTypes,
    ) -> Option<ServerRecord> {
        self.reset_old_scores();

        let state = self.state.read();

        best_candidate(&state.servers, supported_protocol_types)
    }

    /// Fetches the server list if needed.
    async fn start_fetching_servers(&self) -> Result<(), SteamError> {
        let span = self.server_list_before_refresh_time_span();

        if self.list_is_fresh(span) {
            return Ok(());
        }

        let _guard = self.resolve_lock.lock().await;

        if self.list_is_fresh(span) {
            return Ok(());
        }

        // The list is there but stale, so force a refresh.
        let force_refresh = !self.state.read().servers.is_empty();

        self.resolve_server_list(force_refresh).await
    }

    /// Whether the list is populated and fresh enough to use as-is.
    fn list_is_fresh(&self, span: Duration) -> bool {
        let state = self.state.read();

        if state.servers.is_empty() {
            return false;
        }

        let last_refresh = state.servers_last_refresh.unwrap_or(SystemTime::UNIX_EPOCH);

        SystemTime::now()
            .duration_since(last_refresh)
            .unwrap_or(Duration::ZERO)
            < span
    }

    /// Resolves the server list from the Steam Directory, the provider or the
    /// fallback servers.
    async fn resolve_server_list(&self, mut force_refresh: bool) -> Result<(), SteamError> {
        let provider_refresh_time = self.provider.last_server_list_refresh().await?;
        let span = self.server_list_before_refresh_time_span();
        let mut already_tried_directory_fetch = false;

        // The first time around, check whether the cache is old enough to
        // refresh it from the API first.
        if !force_refresh
            && SystemTime::now()
                .duration_since(provider_refresh_time)
                .unwrap_or(Duration::ZERO)
                >= span
        {
            force_refresh = true;
        }

        // Only refresh from the API if that's allowed at all.
        if force_refresh && self.allow_directory_fetch {
            if let Some(loader) = &self.directory_loader {
                tracing::debug!("Querying SteamDirectory for a fresh server list");

                let directory_list = loader().await?;
                already_tried_directory_fetch = true;

                if !directory_list.is_empty() {
                    tracing::debug!(
                        "Resolved {} servers from SteamDirectory",
                        directory_list.len()
                    );
                    self.replace_list(directory_list, true, Some(SystemTime::now()))
                        .await?;
                    return Ok(());
                }

                tracing::debug!("Could not query SteamDirectory, falling back to provider");
            }
        } else {
            tracing::debug!("Resolving server list using the provider");
        }

        let server_list = self.provider.fetch_server_list().await?;

        // The provider's list is fresh enough and not empty.
        if !server_list.is_empty() {
            tracing::debug!("Resolved {} servers from the provider", server_list.len());
            self.replace_list(server_list, false, Some(provider_refresh_time))
                .await?;
            return Ok(());
        }

        // API fetches not allowed: no servers.
        if !self.allow_directory_fetch {
            tracing::debug!(
                "Server list provider had no entries, and AllowDirectoryFetch is false"
            );
            self.replace_list(Vec::new(), false, Some(SystemTime::UNIX_EPOCH))
                .await?;
            return Ok(());
        }

        // Don't fetch again if the forced refresh already tried.
        if !already_tried_directory_fetch {
            if let Some(loader) = &self.directory_loader {
                tracing::debug!("Server list provider had no entries, will query SteamDirectory");

                let directory_list = loader().await?;

                if !directory_list.is_empty() {
                    tracing::debug!(
                        "Resolved {} servers from SteamDirectory",
                        directory_list.len()
                    );
                    self.replace_list(directory_list, true, Some(SystemTime::now()))
                        .await?;
                    return Ok(());
                }
            }
        }

        // Last resort: the fallback servers.
        tracing::debug!(
            "Server list provider had no entries, SteamDirectory failed, falling back to default servers"
        );

        let default_servers = vec![
            ServerRecord::create_websocket_server(&Self::default_server_websocket()).await?,
            ServerRecord::create_dns_socket_server(&Self::default_server_netfilter()).await?,
        ];

        self.replace_list(default_servers, false, Some(SystemTime::UNIX_EPOCH))
            .await?;

        Ok(())
    }
}

/// Picks the best candidate: servers that never failed first, then by how long
/// ago they failed. Ties keep their order in the list.
fn best_candidate(
    servers: &[ServerInfo],
    supported_protocol_types: ProtocolTypes,
) -> Option<ServerRecord> {
    let mut best: Option<(usize, &ServerInfo)> = None;

    for (index, server) in servers.iter().enumerate() {
        if !server.protocol.has(supported_protocol_types) {
            continue;
        }

        let key = server.last_bad_connection_time_utc.unwrap_or(SystemTime::UNIX_EPOCH);

        match best {
            None => best = Some((index, server)),
            Some((_, current)) => {
                let current_key =
                    current.last_bad_connection_time_utc.unwrap_or(SystemTime::UNIX_EPOCH);

                // Only strictly older entries win, so ties keep their order.
                if key < current_key {
                    best = Some((index, server));
                }
            }
        }
    }

    best.map(|(_, server)| {
        tracing::debug!(
            "Next server candidate: {} ({:?})",
            server.record.end_point(),
            server.protocol
        );

        ServerRecord::new(server.record.endpoint().clone(), server.protocol)
    })
}

/// Adds a server record, once for each protocol it supports.
fn add_core(servers: &mut Vec<ServerInfo>, record: &ServerRecord) {
    for protocol in protocol_flags(record.protocol_types()) {
        servers.push(ServerInfo {
            record: record.clone(),
            protocol,
            last_bad_connection_time_utc: None,
        });
    }
}

/// Splits a protocol set into its flags, in TCP, UDP, WebSocket order.
fn protocol_flags(protocols: ProtocolTypes) -> Vec<ProtocolTypes> {
    let mut flags = Vec::new();

    if protocols.has(ProtocolTypes::TCP) {
        flags.push(ProtocolTypes::TCP);
    }

    if protocols.has(ProtocolTypes::UDP) {
        flags.push(ProtocolTypes::UDP);
    }

    if protocols.has(ProtocolTypes::WEB_SOCKET) {
        flags.push(ProtocolTypes::WEB_SOCKET);
    }

    flags
}
