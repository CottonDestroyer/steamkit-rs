//! The configuration a client uses.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::base::generated::steam_language::{EClientPersonaStateFlag, EUniverse};
use crate::networking::steam3::protocol_type::ProtocolTypes;
use crate::steam::discovery::smart_cm_server_list::SmartCMServerList;

use super::i_machine_info_provider::IMachineInfoProvider;
use super::steam_configuration_builder::SteamConfigurationBuilder;
use super::steam_configuration_state::{PersonaStateFlags, SteamConfigurationState};

/// What an HTTP client is for, so the factory can configure it accordingly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HttpClientPurpose {
    /// WebSocket connections to CM servers.
    CMWebSocket,
    /// The Steam Web API.
    WebAPI,
    /// Downloading game content.
    CDN,
}

/// Creates an HTTP client for a given purpose. Clients are expensive to build,
/// so reuse them.
pub type HttpClientFactory = Arc<dyn Fn(HttpClientPurpose) -> reqwest::Client + Send + Sync>;

/// The base address of the Steam Web API.
pub fn default_web_api_base_address() -> reqwest::Url {
    reqwest::Url::parse("https://api.steampowered.com/")
        .expect("the default Steam Web API address is a valid URL")
}

/// The `User-Agent` sent with every HTTP request, including the CM WebSocket
/// upgrade.
pub fn default_user_agent() -> String {
    format!("SteamKit/{}", env!("CARGO_PKG_VERSION"))
}

/// The default HTTP client factory.
pub fn default_http_client_factory() -> HttpClientFactory {
    Arc::new(|_purpose: HttpClientPurpose| {
        reqwest::Client::builder()
            .user_agent(default_user_agent())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// A client configuration. Don't change it after handing it to a client.
pub struct SteamConfiguration {
    state: Arc<SteamConfigurationState>,
    server_list: Arc<SmartCMServerList>,
}

impl fmt::Debug for SteamConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamConfiguration")
            .field("state", &self.state)
            .field("server_list", &"Arc<SmartCMServerList>")
            .finish()
    }
}

impl SteamConfiguration {
    /// Creates a configuration, configured by `configurator`.
    pub fn create<F>(configurator: F) -> SteamConfiguration
    where
        F: FnOnce(&mut SteamConfigurationBuilder),
    {
        let mut builder = SteamConfigurationBuilder::new();
        configurator(&mut builder);
        builder.build()
    }

    /// Creates a configuration with the default settings.
    pub fn create_default() -> SteamConfiguration {
        SteamConfigurationBuilder::create_default_state().into_configuration()
    }

    /// Creates a configuration from a filled-in state.
    pub fn from_state(state: SteamConfigurationState) -> SteamConfiguration {
        state.into_configuration()
    }

    /// The raw state of this configuration.
    pub fn state(&self) -> &Arc<SteamConfigurationState> {
        &self.state
    }

    /// Whether to fetch the server list from the Steam Directory.
    pub fn allow_directory_fetch(&self) -> bool {
        self.state.allow_directory_fetch
    }

    /// The Steam cell to prefer.
    pub fn cell_id(&self) -> u32 {
        self.state.cell_id
    }

    /// The timeout for connecting to Steam servers.
    pub fn connection_timeout(&self) -> Duration {
        self.state.connection_timeout
    }

    /// The first of the default persona state flags requested for a new friend.
    /// The generated enum can't hold a combination; for the full set use
    /// [`SteamConfiguration::default_persona_state_flags_mask`].
    pub fn default_persona_state_flags(&self) -> EClientPersonaStateFlag {
        self.state
            .default_persona_state_flags
            .representative()
            .unwrap_or(EClientPersonaStateFlag::PlayerName)
    }

    /// The default persona state flags, as a bit mask.
    pub fn default_persona_state_flags_mask(&self) -> PersonaStateFlags {
        self.state.default_persona_state_flags
    }

    /// The factory that creates HTTP clients.
    pub fn http_client_factory(&self) -> &HttpClientFactory {
        &self.state.http_client_factory
    }

    /// The machine info provider used when logging on.
    pub fn machine_info_provider(&self) -> &Arc<dyn IMachineInfoProvider> {
        &self.state.machine_info_provider
    }

    /// The transports to use when connecting.
    pub fn protocol_types(&self) -> ProtocolTypes {
        self.state.protocol_types
    }

    /// The server list provider to use.
    pub fn server_list_provider(&self) -> &Arc<dyn crate::steam::discovery::i_server_list_provider::IServerListProvider> {
        &self.state.server_list_provider
    }

    /// The universe to connect to.
    pub fn universe(&self) -> EUniverse {
        self.state.universe
    }

    /// The base address of the Steam Web API.
    pub fn web_api_base_address(&self) -> &reqwest::Url {
        &self.state.web_api_base_address
    }

    /// The Web API key attached to requests.
    pub fn web_api_key(&self) -> &str {
        &self.state.web_api_key
    }

    /// The server list. Clients sharing this configuration share the list too.
    pub fn server_list(&self) -> &Arc<SmartCMServerList> {
        &self.server_list
    }
}

impl SteamConfigurationState {
    /// Turns the state into a configuration.
    pub fn into_configuration(self) -> SteamConfiguration {
        let state = Arc::new(self);
        let server_list = Arc::new(SmartCMServerList::new(state.clone()));

        SteamConfiguration { state, server_list }
    }
}
