//! The builder interface for [`SteamConfiguration`](crate::steam::steam_client::configuration::steam_configuration::SteamConfiguration).

use std::sync::Arc;
use std::time::Duration;

use crate::base::generated::steam_language::EUniverse;
use crate::networking::steam3::protocol_type::ProtocolTypes;
use crate::steam::discovery::i_server_list_provider::IServerListProvider;

use super::i_machine_info_provider::IMachineInfoProvider;
use super::steam_configuration::HttpClientFactory;
use super::steam_configuration_state::PersonaStateFlags;

/// Configures a [`SteamConfiguration`](crate::steam::steam_client::configuration::steam_configuration::SteamConfiguration) before it's
/// created.
pub trait ISteamConfigurationBuilder {
    /// Sets the Steam cell to prefer.
    fn with_cell_id(&mut self, cell_id: u32) -> &mut Self;

    /// Sets the connection timeout.
    fn with_connection_timeout(&mut self, connection_timeout: Duration) -> &mut Self;

    /// Sets the default persona state flags to request from Steam.
    fn with_default_persona_state_flags(&mut self, persona_state_flags: PersonaStateFlags) -> &mut Self;

    /// Sets whether to fetch the server list from the Steam Directory.
    fn with_directory_fetch(&mut self, allow_directory_fetch: bool) -> &mut Self;

    /// Sets the factory that creates HTTP clients.
    fn with_http_client_factory(&mut self, factory_function: HttpClientFactory) -> &mut Self;

    /// Sets the machine info provider.
    fn with_machine_info_provider(&mut self, machine_info_provider: Arc<dyn IMachineInfoProvider>) -> &mut Self;

    /// Sets which transports to use.
    fn with_protocol_types(&mut self, protocol_types: ProtocolTypes) -> &mut Self;

    /// Sets the server list provider.
    fn with_server_list_provider(&mut self, provider: Arc<dyn IServerListProvider>) -> &mut Self;

    /// Sets the universe.
    fn with_universe(&mut self, universe: EUniverse) -> &mut Self;

    /// Sets the Steam Web API address.
    fn with_web_api_base_address(&mut self, base_address: reqwest::Url) -> &mut Self;

    /// Sets the Web API key to attach to requests.
    fn with_web_api_key(&mut self, web_api_key: String) -> &mut Self;
}
