//! Callbacks posted by [`SteamGameServer`](super::steam_game_server::SteamGameServer).

use std::any::Any;

use steamkit_protos::steam::{CMsgClientTicketAuthComplete, CMsgGsStatusReply};

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::generated::steam_language::EAuthSessionResponse;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::game_id::GameID;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

/// Posted when the game server gets a status reply.
#[derive(Clone, Debug)]
pub struct StatusReplyCallback {
    job_id: JobID,
    is_secure: bool,
}

impl StatusReplyCallback {
    /// Builds the callback from a received `GSStatusReply` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let status_reply = ClientMsgProtobuf::<CMsgGsStatusReply>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: JobID::new(),
            is_secure: status_reply.body.is_secure.unwrap_or_default(),
        })
    }

    /// Whether this game server is VAC secure.
    pub fn is_secure(&self) -> bool {
        self.is_secure
    }
}

impl CallbackMsg for StatusReplyCallback {
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

/// Posted when ticket authentication has completed.
#[derive(Clone, Debug)]
pub struct TicketAuthCallback {
    job_id: JobID,
    steam_id: SteamID,
    game_id: GameID,
    state: u32,
    auth_session_response: u32,
    ticket_crc: u32,
    ticket_sequence: u32,
}

impl TicketAuthCallback {
    /// Builds the callback from a received `ClientTicketAuthComplete` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let auth_complete = ClientMsgProtobuf::<CMsgClientTicketAuthComplete>::from_packet(packet_msg)?;
        let tick_auth = &auth_complete.body;

        Ok(Self {
            job_id: JobID::new(),
            steam_id: SteamID::from_id(tick_auth.steam_id.unwrap_or_default()),
            game_id: GameID::from_value(tick_auth.game_id.unwrap_or_default()),
            state: tick_auth.estate.unwrap_or_default(),
            auth_session_response: tick_auth.eauth_session_response.unwrap_or_default(),
            ticket_crc: tick_auth.ticket_crc.unwrap_or_default(),
            ticket_sequence: tick_auth.ticket_sequence.unwrap_or_default(),
        })
    }

    /// The SteamID the ticket auth completed for.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The game id the ticket was for.
    pub fn game_id(&self) -> GameID {
        self.game_id
    }

    /// The authentication state.
    pub fn state(&self) -> u32 {
        self.state
    }

    /// The auth session response, or `None` if Steam sent a value that isn't
    /// in [`EAuthSessionResponse`]. The raw number is in
    /// [`TicketAuthCallback::auth_session_response_value`].
    pub fn auth_session_response(&self) -> Option<EAuthSessionResponse> {
        EAuthSessionResponse::from_value(self.auth_session_response as i32)
    }

    /// The raw auth session response.
    pub fn auth_session_response_value(&self) -> u32 {
        self.auth_session_response
    }

    /// The ticket CRC.
    pub fn ticket_crc(&self) -> u32 {
        self.ticket_crc
    }

    /// The ticket sequence.
    pub fn ticket_sequence(&self) -> u32 {
        self.ticket_sequence
    }
}

impl CallbackMsg for TicketAuthCallback {
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
