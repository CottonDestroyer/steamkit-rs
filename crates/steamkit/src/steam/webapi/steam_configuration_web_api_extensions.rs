//! Making Web API interfaces from a `SteamConfiguration`.

use crate::steam::steam_client::configuration::steam_configuration::{
    HttpClientPurpose, SteamConfiguration,
};

use super::web_api::{AsyncInterface, Interface};

/// Makes Web API interfaces from a `SteamConfiguration`. The free functions
/// [`get_web_api_interface`] and [`get_async_web_api_interface`] do the same
/// without the trait.
pub trait SteamConfigurationWebApiExtensions {
    /// An interface for calling the given Web API interface.
    fn get_web_api_interface(&self, iface: &str) -> Interface;

    /// An async interface for calling the given Web API interface.
    fn get_async_web_api_interface(&self, iface: &str) -> AsyncInterface;
}

impl SteamConfigurationWebApiExtensions for SteamConfiguration {
    /// An interface for calling the given Web API interface.
    fn get_web_api_interface(&self, iface: &str) -> Interface {
        Interface::new(
            get_http_client_for_web_api(self),
            self.web_api_base_address().clone(),
            iface,
            self.web_api_key(),
        )
    }

    /// An async interface for calling the given Web API interface.
    fn get_async_web_api_interface(&self, iface: &str) -> AsyncInterface {
        AsyncInterface::new(
            get_http_client_for_web_api(self),
            self.web_api_base_address().clone(),
            iface,
            self.web_api_key(),
        )
    }
}

/// An interface for calling the given Web API interface.
pub fn get_web_api_interface(config: &SteamConfiguration, iface: &str) -> Interface {
    Interface::new(
        get_http_client_for_web_api(config),
        config.web_api_base_address().clone(),
        iface,
        config.web_api_key(),
    )
}

/// An async interface for calling the given Web API interface.
pub fn get_async_web_api_interface(config: &SteamConfiguration, iface: &str) -> AsyncInterface {
    AsyncInterface::new(
        get_http_client_for_web_api(config),
        config.web_api_base_address().clone(),
        iface,
        config.web_api_key(),
    )
}

/// The HTTP client a configuration's Web API interfaces use.
///
/// A `reqwest::Client` can't be reconfigured after it's built, so the base
/// address lives on the interface and the timeout is applied per request. A
/// timeout set by the factory itself isn't overridden.
pub(crate) fn get_http_client_for_web_api(config: &SteamConfiguration) -> reqwest::Client {
    let factory = config.http_client_factory();

    factory(HttpClientPurpose::WebAPI)
}
