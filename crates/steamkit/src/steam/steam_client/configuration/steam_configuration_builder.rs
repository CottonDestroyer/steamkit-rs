//! The builder for [`SteamConfiguration`].

use std::sync::Arc;
use std::time::Duration;

use crate::base::generated::steam_language::EUniverse;
use crate::networking::steam3::protocol_type::ProtocolTypes;
use crate::steam::discovery::i_server_list_provider::IServerListProvider;

use super::i_machine_info_provider::IMachineInfoProvider;
use super::i_steam_configuration_builder::ISteamConfigurationBuilder;
use super::steam_configuration::{default_http_client_factory, default_web_api_base_address, HttpClientFactory, SteamConfiguration};
use super::steam_configuration_state::{PersonaStateFlags, SteamConfigurationState};

/// Builds a [`SteamConfiguration`].
pub struct SteamConfigurationBuilder {
    state: SteamConfigurationState,
}

impl Default for SteamConfigurationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SteamConfigurationBuilder {
    /// Creates a builder with the default configuration.
    pub fn new() -> Self {
        Self {
            state: Self::create_default_state(),
        }
    }

    /// The default configuration state.
    pub fn create_default_state() -> SteamConfigurationState {
        SteamConfigurationState {
            allow_directory_fetch: true,
            cell_id: 0,
            connection_timeout: Duration::from_secs(5),
            default_persona_state_flags: PersonaStateFlags::DEFAULT,
            http_client_factory: default_http_client_factory(),
            machine_info_provider: crate::util::hardware_utils::MachineInfoProvider::get_default_provider(),
            protocol_types: ProtocolTypes::TCP | ProtocolTypes::WEB_SOCKET,
            server_list_provider: Arc::new(crate::steam::discovery::memory_server_list_provider::MemoryServerListProvider::new()),
            universe: EUniverse::Public,
            web_api_base_address: default_web_api_base_address(),
            web_api_key: String::new(),
        }
    }

    /// Builds the configuration.
    pub fn build(&self) -> SteamConfiguration {
        SteamConfiguration::from_state(self.state.clone())
    }
}

impl ISteamConfigurationBuilder for SteamConfigurationBuilder {
    fn with_cell_id(&mut self, cell_id: u32) -> &mut Self {
        self.state.cell_id = cell_id;
        self
    }

    fn with_connection_timeout(&mut self, connection_timeout: Duration) -> &mut Self {
        self.state.connection_timeout = connection_timeout;
        self
    }

    fn with_default_persona_state_flags(&mut self, persona_state_flags: PersonaStateFlags) -> &mut Self {
        self.state.default_persona_state_flags = persona_state_flags;
        self
    }

    fn with_directory_fetch(&mut self, allow_directory_fetch: bool) -> &mut Self {
        self.state.allow_directory_fetch = allow_directory_fetch;
        self
    }

    fn with_http_client_factory(&mut self, factory_function: HttpClientFactory) -> &mut Self {
        self.state.http_client_factory = factory_function;
        self
    }

    fn with_machine_info_provider(&mut self, machine_info_provider: Arc<dyn IMachineInfoProvider>) -> &mut Self {
        self.state.machine_info_provider = machine_info_provider;
        self
    }

    fn with_protocol_types(&mut self, protocol_types: ProtocolTypes) -> &mut Self {
        self.state.protocol_types = protocol_types;
        self
    }

    fn with_server_list_provider(&mut self, provider: Arc<dyn IServerListProvider>) -> &mut Self {
        self.state.server_list_provider = provider;
        self
    }

    fn with_universe(&mut self, universe: EUniverse) -> &mut Self {
        self.state.universe = universe;
        self
    }

    fn with_web_api_base_address(&mut self, base_address: reqwest::Url) -> &mut Self {
        self.state.web_api_base_address = base_address;
        self
    }

    fn with_web_api_key(&mut self, web_api_key: String) -> &mut Self {
        self.state.web_api_key = web_api_key;
        self
    }
}
