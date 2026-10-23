//! Querying the game master server for server lists.

use std::net::IpAddr;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::CMsgClientGmsServerQuery;

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{EMsg, ERegionCode};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::util::net_helpers::NetHelpers;

use super::callbacks::QueryCallback;

/// What to query the master server for.
#[derive(Clone, Debug)]
pub struct QueryDetails {
    /// The app id used when querying servers.
    pub app_id: u32,

    /// The query filter. See
    /// <https://developer.valvesoftware.com/wiki/Master_Server_Query_Protocol>
    /// for the format.
    pub filter: Option<String>,

    /// The region to return servers from.
    pub region: ERegionCode,

    /// An IP address to GeoIP-locate, to get servers close to it.
    pub geo_located_ip: Option<IpAddr>,

    /// The maximum number of servers to return.
    pub max_servers: u32,
}

impl Default for QueryDetails {
    fn default() -> Self {
        Self {
            app_id: 0,
            filter: None,
            // The default region.
            region: ERegionCode::USEast,
            geo_located_ip: None,
            max_servers: 0,
        }
    }
}

/// Queries the game master server for server lists.
pub struct SteamMasterServer {
    client: Weak<SteamClient>,
}

impl std::fmt::Debug for SteamMasterServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamMasterServer").finish()
    }
}

impl SteamMasterServer {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client
            .upgrade()
            .ok_or_else(|| SteamError::Connection("SteamClient has been dropped".to_string()))
    }

    /// Requests a list of servers from the game master server.
    pub async fn server_query(&self, details: &QueryDetails) -> SteamResult<QueryCallback> {
        let client = self.client()?;

        let job_id = client.get_next_job_id();
        let job = client.start_job::<QueryCallback>(job_id);

        let mut query =
            ClientMsgProtobuf::<CMsgClientGmsServerQuery>::new(EMsg::ClientGMSServerQuery);
        query.set_source_job_id(job_id);

        query.body.app_id = Some(details.app_id);

        if let Some(geo_located_ip) = &details.geo_located_ip {
            query.body.geo_location_ip = Some(NetHelpers::get_ip_address_as_uint(geo_located_ip)?);
        }

        query.body.filter_text = details.filter.clone();
        query.body.region_code = Some(u32::from(details.region.value()));
        query.body.max_servers = Some(details.max_servers);

        client.send(&query)?;

        job.wait().await
    }
}

#[async_trait]
impl ClientMsgHandler for SteamMasterServer {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::GMSClientServerQueryResponse => {
                Some(Box::new(QueryCallback::from_packet(packet_msg)?))
            }
            // Not a message we handle.
            _ => None,
        };

        if let Some(callback) = callback {
            client.post_callback(callback);
        }

        Ok(())
    }
}
