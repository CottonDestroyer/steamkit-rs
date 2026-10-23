//! Callbacks posted by [`SteamAuthTicket`](super::steam_auth_ticket::SteamAuthTicket).

use std::any::Any;

use steamkit_protos::steam::{CMsgClientAuthListAck, CMsgClientTicketAuthComplete};

use crate::base::generated::steam_language::EAuthSessionResponse;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::game_id::GameID;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

/// Posted when Steam accepts our auth ticket as valid.
#[derive(Clone, Debug)]
pub struct TicketAcceptedCallback {
    job_id: JobID,
    app_ids: Vec<u32>,
    active_tickets_crc: Vec<u32>,
    message_sequence: u32,
}

impl TicketAcceptedCallback {
    /// Builds the callback from the body of a `ClientAuthListAck` message.
    pub(crate) fn new(job_id: JobID, body: &CMsgClientAuthListAck) -> Self {
        Self {
            job_id,
            app_ids: body.app_ids.clone(),
            active_tickets_crc: body.ticket_crc.clone(),
            message_sequence: body.message_sequence.unwrap_or_default(),
        }
    }

    /// The app ids of the games that have generated tickets.
    pub fn app_ids(&self) -> &[u32] {
        &self.app_ids
    }

    /// The CRC32 hashes of the activated tickets.
    pub fn active_tickets_crc(&self) -> &[u32] {
        &self.active_tickets_crc
    }

    /// The number of this message in the sequence.
    pub fn message_sequence(&self) -> u32 {
        self.message_sequence
    }
}

impl CallbackMsg for TicketAcceptedCallback {
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

/// Posted when a generated ticket was used to authenticate the user.
#[derive(Clone, Debug)]
pub struct TicketAuthCompleteCallback {
    job_id: JobID,
    auth_session_response: EAuthSessionResponse,
    state: u32,
    game_id: GameID,
    owner_steam_id: SteamID,
    steam_id: SteamID,
    ticket_crc: u32,
    ticket_sequence: u32,
}

impl TicketAuthCompleteCallback {
    /// Builds the callback from the body of a `ClientTicketAuthComplete` message.
    pub(crate) fn new(job_id: JobID, body: &CMsgClientTicketAuthComplete) -> Self {
        Self {
            job_id,
            auth_session_response: EAuthSessionResponse::from_value(
                body.eauth_session_response.unwrap_or_default() as i32,
            )
            .unwrap_or(EAuthSessionResponse::UserNotConnectedToSteam),
            state: body.estate.unwrap_or_default(),
            game_id: GameID::from_value(body.game_id.unwrap_or_default()),
            owner_steam_id: SteamID::from_id(body.owner_steam_id.unwrap_or_default()),
            steam_id: SteamID::from_id(body.steam_id.unwrap_or_default()),
            ticket_crc: body.ticket_crc.unwrap_or_default(),
            ticket_sequence: body.ticket_sequence.unwrap_or_default(),
        }
    }

    /// The Steam response to the authentication request.
    pub fn auth_session_response(&self) -> EAuthSessionResponse {
        self.auth_session_response
    }

    /// The authentication state.
    pub fn state(&self) -> u32 {
        self.state
    }

    /// The id of the game the token was generated for.
    pub fn game_id(&self) -> GameID {
        self.game_id
    }

    /// The SteamID of the game owner.
    pub fn owner_steam_id(&self) -> SteamID {
        self.owner_steam_id
    }

    /// The SteamID of the game server.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The CRC of the ticket.
    pub fn ticket_crc(&self) -> u32 {
        self.ticket_crc
    }

    /// The sequence of the ticket.
    pub fn ticket_sequence(&self) -> u32 {
        self.ticket_sequence
    }
}

impl CallbackMsg for TicketAuthCompleteCallback {
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
