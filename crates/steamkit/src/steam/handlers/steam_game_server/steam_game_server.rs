//! Logging on as a game server and reporting its status.

use std::fmt;
use std::net::IpAddr;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use steamkit_protos::steam::{CMsgClientLogOff, CMsgClientLogon, CMsgGsServerType};

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::internal::MsgClientLogon;
use crate::base::generated::steam_language::{EAccountType, EMsg, EResult, EServerFlags};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::handlers::steam_user::callbacks::LoggedOnCallback;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::steam_id::SteamID;
use crate::util::net_helpers::NetHelpers;

use super::callbacks::{StatusReplyCallback, TicketAuthCallback};

/// What's needed to log on as a game server.
#[derive(Clone, Debug, Default)]
pub struct LogOnDetails {
    /// The game server's login token.
    pub token: Option<String>,
    /// The app id this game server will serve.
    pub app_id: u32,
}

/// The current status of the game server.
#[derive(Clone, Debug)]
pub struct StatusDetails {
    /// The app id this game server is serving.
    pub app_id: u32,
    /// The server's basic state, as flags.
    pub server_flags: EServerFlags,
    /// The directory the game data is in.
    pub game_directory: Option<String>,
    /// The IP address the game server listens on.
    pub address: Option<IpAddr>,
    /// The port the game server listens on.
    pub port: u32,
    /// The port the game server responds to queries on.
    pub query_port: u32,
    /// The current version of the game server.
    pub version: Option<String>,
}

impl Default for StatusDetails {
    fn default() -> Self {
        Self {
            app_id: 0,
            server_flags: EServerFlags::None,
            game_directory: None,
            address: None,
            port: 0,
            query_port: 0,
            version: None,
        }
    }
}

/// Interacts with the Steam network as a game server.
pub struct SteamGameServer {
    client: Weak<SteamClient>,
}

impl fmt::Debug for SteamGameServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamGameServer").finish_non_exhaustive()
    }
}

impl SteamGameServer {
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

    /// Whether the server is about to close the connection on purpose.
    pub fn expect_disconnection(&self) -> bool {
        self.client().map(|client| client.expect_disconnection()).unwrap_or(false)
    }

    /// Logs on as a persistent game server. The client has to be connected
    /// already. The result comes in a [`LoggedOnCallback`].
    pub fn log_on(&self, details: &LogOnDetails) -> SteamResult<()> {
        let client = self.client()?;

        let token = details.token.as_deref().unwrap_or_default();
        if token.is_empty() {
            return Err(SteamError::InvalidArgument(
                "LogOn requires a game server token to be set in 'details'.".to_string(),
            ));
        }

        if !client.is_connected() {
            client.post_callback(Box::new(LoggedOnCallback::from_result(
                EResult::NoConnection,
            )));
            return Ok(());
        }

        let universe = client.configuration().universe();
        let gs_id = SteamID::from_parts_instanced(EAccountType::GameServer, universe, 0, 0);

        let mut logon = ClientMsgProtobuf::<CMsgClientLogon>::new(EMsg::ClientLogonGameServer);

        logon.set_session_id(0);
        // Write the raw id: `set_steam_id` rebuilds the id from its parts and
        // would use the desktop account instance, but a game server id has
        // instance 0.
        logon.proto_header_mut().steamid = Some(gs_id.convert_to_u64());

        let local_ip = client.local_ip().ok_or_else(|| {
            SteamError::Connection("the local IP address is not known yet".to_string())
        })?;
        let mut obfuscated_ip = NetHelpers::get_msg_ip_address(&local_ip);
        NetHelpers::obfuscate_private_ip(&mut obfuscated_ip);
        logon.body.obfuscated_private_ip = Some(obfuscated_ip);

        logon.body.protocol_version = Some(MsgClientLogon::CURRENT_PROTOCOL);
        logon.body.client_os_type = Some(crate::util::utils::get_os_type() as u32);
        logon.body.game_server_app_id = Some(details.app_id as i32);
        logon.body.machine_id = crate::util::hardware_utils::get_machine_id(
            client.configuration().machine_info_provider(),
        );
        logon.body.game_server_token = Some(token.to_string());

        client.send(&logon)?;

        Ok(())
    }

    /// Logs on as an anonymous game server. The client has to be connected
    /// already. The result comes in a [`LoggedOnCallback`].
    ///
    /// `app_id` is the app this server serves, or 0 for the default.
    pub fn log_on_anonymous(&self, app_id: u32) -> SteamResult<()> {
        let client = self.client()?;

        if !client.is_connected() {
            client.post_callback(Box::new(LoggedOnCallback::from_result(
                EResult::NoConnection,
            )));
            return Ok(());
        }

        let universe = client.configuration().universe();
        let gs_id = SteamID::from_parts_instanced(EAccountType::AnonGameServer, universe, 0, 0);

        let mut logon = ClientMsgProtobuf::<CMsgClientLogon>::new(EMsg::ClientLogon);

        logon.set_session_id(0);
        // Write the raw id: `set_steam_id` rebuilds the id from its parts and
        // would use the desktop account instance, but an anonymous game server
        // id has instance 0.
        logon.proto_header_mut().steamid = Some(gs_id.convert_to_u64());

        let local_ip = client.local_ip().ok_or_else(|| {
            SteamError::Connection("the local IP address is not known yet".to_string())
        })?;
        let mut obfuscated_ip = NetHelpers::get_msg_ip_address(&local_ip);
        NetHelpers::obfuscate_private_ip(&mut obfuscated_ip);
        logon.body.obfuscated_private_ip = Some(obfuscated_ip);

        logon.body.protocol_version = Some(MsgClientLogon::CURRENT_PROTOCOL);
        logon.body.client_os_type = Some(crate::util::utils::get_os_type() as u32);
        logon.body.game_server_app_id = Some(app_id as i32);
        logon.body.machine_id = crate::util::hardware_utils::get_machine_id(
            client.configuration().machine_info_provider(),
        );

        client.send(&logon)?;

        Ok(())
    }

    /// Logs off. The server then disconnects us and a `DisconnectedCallback`
    /// is posted.
    pub fn log_off(&self) -> SteamResult<()> {
        let client = self.client()?;

        client.set_expect_disconnection(true);

        let log_off = ClientMsgProtobuf::<CMsgClientLogOff>::new(EMsg::ClientLogOff);
        client.send(&log_off)?;

        Ok(())
    }

    /// Sends the server's status to Steam. The reply comes in a
    /// [`StatusReplyCallback`].
    pub fn send_status(&self, details: &StatusDetails) -> SteamResult<()> {
        let client = self.client()?;

        let game_ip_address = match details.address {
            Some(IpAddr::V4(v4)) => Some(u32::from_be_bytes(v4.octets())),
            Some(IpAddr::V6(_)) => {
                return Err(SteamError::InvalidArgument(
                    "Only IPv4 addresses are supported.".to_string(),
                ))
            }
            None => None,
        };

        let mut status = ClientMsgProtobuf::<CMsgGsServerType>::new(EMsg::GSServerType);

        status.body.app_id_served = Some(details.app_id);
        status.body.flags = Some(details.server_flags.value() as u32);
        status.body.game_dir = details.game_directory.clone();
        status.body.game_port = Some(details.port);
        status.body.game_query_port = Some(details.query_port);
        status.body.game_version = details.version.clone();
        status.body.deprecated_game_ip_address = game_ip_address;

        client.send(&status)?;


        Ok(())
    }
}

#[async_trait]
impl ClientMsgHandler for SteamGameServer {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::GSStatusReply => Some(Box::new(StatusReplyCallback::from_packet(packet_msg)?)),
            EMsg::ClientTicketAuthComplete => {
                Some(Box::new(TicketAuthCallback::from_packet(packet_msg)?))
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
