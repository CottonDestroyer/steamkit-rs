//! Loading content servers from the `IContentServerDirectoryService` Web API.

use steamkit_protos::steam::{
    CContentServerDirectoryGetServersForSteamPipeRequest,
    CContentServerDirectoryGetServersForSteamPipeResponse,
};
use tokio_util::sync::CancellationToken;

use crate::error::SteamError;
use crate::steam::cdn::server::{ConnectionProtocol, Server};
use crate::steam::steam_client::configuration::steam_configuration::SteamConfiguration;

use super::steam_configuration_web_api_extensions::get_async_web_api_interface;

/// Loads content servers from the `IContentServerDirectoryService` Web API.
pub struct ContentServerDirectoryService;

impl ContentServerDirectoryService {
    /// Loads a list of content servers.
    pub async fn load(
        configuration: &SteamConfiguration,
    ) -> Result<Vec<Server>, SteamError> {
        Self::load_core(configuration, None, None, CancellationToken::new()).await
    }

    /// Loads a list of content servers, cancellable.
    pub async fn load_with_cancellation(
        configuration: &SteamConfiguration,
        cancellation: CancellationToken,
    ) -> Result<Vec<Server>, SteamError> {
        Self::load_core(configuration, None, None, cancellation).await
    }

    /// Loads a list of content servers; `cell_id` is the preferred cell id.
    pub async fn load_with_cell_id(
        configuration: &SteamConfiguration,
        cell_id: u32,
        cancellation: CancellationToken,
    ) -> Result<Vec<Server>, SteamError> {
        Self::load_core(configuration, Some(cell_id), None, cancellation).await
    }

    /// Loads a list of content servers. `cell_id` is the preferred cell id and
    /// `max_num_servers` caps the result.
    pub async fn load_with_cell_id_and_max_num_servers(
        configuration: &SteamConfiguration,
        cell_id: u32,
        max_num_servers: u32,
        cancellation: CancellationToken,
    ) -> Result<Vec<Server>, SteamError> {
        Self::load_core(
            configuration,
            Some(cell_id),
            Some(max_num_servers),
            cancellation,
        )
        .await
    }

    /// The actual query.
    async fn load_core(
        configuration: &SteamConfiguration,
        cell_id: Option<u32>,
        max_num_servers: Option<u32>,
        cancellation: CancellationToken,
    ) -> Result<Vec<Server>, SteamError> {
        let directory = get_async_web_api_interface(configuration, "IContentServerDirectoryService");

        let mut request = CContentServerDirectoryGetServersForSteamPipeRequest {
            cell_id: Some(cell_id.unwrap_or_else(|| configuration.cell_id())),
            ..Default::default()
        };

        if let Some(max_num_servers) = max_num_servers {
            request.max_servers = Some(max_num_servers);
        }

        if cancellation.is_cancelled() {
            return Err(SteamError::Cancelled);
        }

        let response = directory
            .call_protobuf_with_request::<
                CContentServerDirectoryGetServersForSteamPipeResponse,
                CContentServerDirectoryGetServersForSteamPipeRequest,
            >(
                reqwest::Method::GET,
                "GetServersForSteamPipe",
                &request,
                1,
                &[],
            )
            .await?;

        if cancellation.is_cancelled() {
            return Err(SteamError::Cancelled);
        }

        Ok(Self::convert_server_list(&response.body))
    }

    /// Converts the protobuf response to CDN server records.
    pub fn convert_server_list(
        response: &CContentServerDirectoryGetServersForSteamPipeResponse,
    ) -> Vec<Server> {
        let mut server_records = Vec::with_capacity(response.servers.len());

        for child in &response.servers {
            let https_support = child.https_support.as_deref();
            let protocol = if https_support == Some("mandatory") {
                ConnectionProtocol::Https
            } else {
                ConnectionProtocol::Http
            };

            server_records.push(Server {
                protocol,
                host: child.host.clone(),
                v_host: child.vhost.clone(),
                port: if protocol == ConnectionProtocol::Https {
                    443
                } else {
                    80
                },
                server_type: child.r#type.clone(),
                source_id: child.source_id.unwrap_or_default(),
                cell_id: child.cell_id.unwrap_or_default() as u32,
                load: child.load.unwrap_or_default(),
                weighted_load: child.weighted_load.unwrap_or_default(),
                num_entries: child.num_entries_in_client_list.unwrap_or_default(),
                steam_china_only: child.steam_china_only.unwrap_or_default(),
                use_as_proxy: child.use_as_proxy.unwrap_or_default(),
                proxy_request_path_template: child.proxy_request_path_template.clone(),
                allowed_app_ids: child.allowed_app_ids.clone(),
                bypass_proxies_of_type: child.bypass_proxies_of_type.clone(),
            });
        }

        server_records
    }
}
