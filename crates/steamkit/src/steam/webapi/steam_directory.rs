//! Loading CM servers from the Steam Directory Web API.

use tokio_util::sync::CancellationToken;

use crate::error::SteamError;
use crate::steam::discovery::server_record::ServerRecord;
use crate::steam::steam_client::configuration::steam_configuration::SteamConfiguration;

use super::steam_configuration_web_api_extensions::get_async_web_api_interface;
use super::web_api::{web_api_args, AsyncInterface, WebApiArgument, WebApiValue};

/// Loads CM servers from the Steam Directory Web API.
pub struct SteamDirectory;

impl SteamDirectory {
    /// Loads a list of CM servers.
    pub async fn load(
        configuration: &SteamConfiguration,
    ) -> Result<Vec<ServerRecord>, SteamError> {
        Self::load_core(configuration, None, CancellationToken::new()).await
    }

    /// Loads a list of CM servers, cancellable.
    pub async fn load_with_cancellation(
        configuration: &SteamConfiguration,
        cancellation: CancellationToken,
    ) -> Result<Vec<ServerRecord>, SteamError> {
        Self::load_core(configuration, None, cancellation).await
    }

    /// Loads a list of CM servers. `max_num_servers` caps the result; the API
    /// usually returns that many per server type (socket and WebSocket).
    pub async fn load_with_max_num_servers(
        configuration: &SteamConfiguration,
        max_num_servers: i32,
        cancellation: CancellationToken,
    ) -> Result<Vec<ServerRecord>, SteamError> {
        Self::load_core(configuration, Some(max_num_servers), cancellation).await
    }

    /// The actual query.
    async fn load_core(
        configuration: &SteamConfiguration,
        max_num_servers: Option<i32>,
        cancellation: CancellationToken,
    ) -> Result<Vec<ServerRecord>, SteamError> {
        let directory = get_async_web_api_interface(configuration, "ISteamDirectory");

        Self::load_with_interface(
            &directory,
            configuration.cell_id(),
            max_num_servers,
            cancellation,
        )
        .await
    }

    /// Loads a list of CM servers through an already prepared Steam Directory
    /// interface. The discovery module reuses this part.
    pub async fn load_with_interface(
        directory: &AsyncInterface,
        cell_id: u32,
        max_num_servers: Option<i32>,
        cancellation: CancellationToken,
    ) -> Result<Vec<ServerRecord>, SteamError> {
        let mut args = web_api_args([(
            "cellid",
            WebApiValue::Text(cell_id.to_string()),
        )]);

        if let Some(max_num_servers) = max_num_servers {
            args.push(WebApiArgument::new(
                "maxcount",
                WebApiValue::Text(max_num_servers.to_string()),
            ));
        }

        if cancellation.is_cancelled() {
            return Err(SteamError::Cancelled);
        }

        let response = directory
            .call_with_method(reqwest::Method::GET, "GetCMListForConnect", 1, &args)
            .await?;

        if cancellation.is_cancelled() {
            return Err(SteamError::Cancelled);
        }

        if !response.get("success").as_boolean(false) {
            let message = response.get("message").as_string().unwrap_or("unknown");

            return Err(SteamError::Other(format!(
                "Steam Web API returned failure - {message}"
            )));
        }

        let socket_list = response.get("serverlist");
        let mut server_records = Vec::with_capacity(socket_list.children().len());

        for child in socket_list.children() {
            let Some(endpoint) = child.get("endpoint").value() else {
                continue;
            };

            let record = match child.get("type").value() {
                Some("websockets") => ServerRecord::create_websocket_server(endpoint).await?,
                Some("netfilter") => ServerRecord::create_dns_socket_server(endpoint).await?,
                // There's also a "china" type (WebSockets); we skip it.
                _ => continue,
            };

            server_records.push(record);
        }

        Ok(server_records)
    }
}
