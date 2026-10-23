//! Callbacks posted by [`SteamMasterServer`](super::steam_master_server::SteamMasterServer).

use std::any::Any;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use steamkit_protos::steam::{
    c_msg_gms_client_server_query_response, CMsgGmsClientServerQueryResponse,
};

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::job_id::JobID;
use crate::util::net_helpers::NetHelpers;

/// The response to
/// [`SteamMasterServer::server_query`](crate::steam::handlers::steam_master_server::steam_master_server::SteamMasterServer::server_query).
#[derive(Clone, Debug)]
pub struct QueryCallback {
    job_id: JobID,
    servers: Vec<Server>,
}

/// One server from a master server query.
#[derive(Clone, Debug)]
pub struct Server {
    end_point: SocketAddr,
    authed_players: u32,
}

impl Server {
    /// Builds the server from its protobuf form.
    fn from_server(server: &c_msg_gms_client_server_query_response::Server) -> Self {
        // Steam doesn't always send an address; use the unspecified address
        // rather than dropping the entry.
        let ip = server
            .server_ip
            .as_ref()
            .and_then(|ip| NetHelpers::get_ip_address_from_msg(ip))
            .unwrap_or_else(|| IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let port = u16::try_from(server.query_port.unwrap_or_default()).unwrap_or(0);

        Self {
            end_point: SocketAddr::new(ip, port),
            authed_players: server.auth_players.unwrap_or_default(),
        }
    }

    /// The server's address and port.
    pub fn end_point(&self) -> SocketAddr {
        self.end_point
    }

    /// The number of Steam authenticated players on this server.
    pub fn authed_players(&self) -> u32 {
        self.authed_players
    }
}

impl QueryCallback {
    /// Builds the callback from a `GMSClientServerQueryResponse`.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let query_response =
            ClientMsgProtobuf::<CMsgGmsClientServerQueryResponse>::from_packet(packet_msg)?;

        let servers = query_response
            .body
            .servers
            .iter()
            .map(Server::from_server)
            .collect();

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            servers,
        })
    }

    /// The list of servers.
    pub fn servers(&self) -> &[Server] {
        &self.servers
    }
}

impl CallbackMsg for QueryCallback {
    fn job_id(&self) -> JobID {
        self.job_id
    }

    fn set_job_id(&mut self, id: JobID) {
        self.job_id = id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
