//! Callbacks posted by [`SteamMatchmaking`](super::steam_matchmaking::SteamMatchmaking).

use std::any::Any;

use crate::base::generated::steam_language::{EChatRoomEnterResponse, EResult};
use crate::steam::steam_client::callback_mgr::callback_msg::{CallbackMsg, CallbackMsgBase};
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

use super::lobby::{Lobby, Member};

/// The response to `get_lobby_list`.
#[derive(Clone, Debug)]
pub struct GetLobbyListCallback {
    base: CallbackMsgBase,
    app_id: u32,
    result: EResult,
    lobbies: Vec<Lobby>,
}

impl GetLobbyListCallback {
    /// Creates a new callback.
    pub(crate) fn new(job_id: JobID, app_id: u32, result: EResult, lobbies: Vec<Lobby>) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            result,
            lobbies,
        }
    }

    /// The id of the app the lobbies belong to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The list of lobbies matching the criteria of the request.
    pub fn lobbies(&self) -> &[Lobby] {
        &self.lobbies
    }
}

impl CallbackMsg for GetLobbyListCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to `create_lobby`.
#[derive(Clone, Debug)]
pub struct CreateLobbyCallback {
    base: CallbackMsgBase,
    app_id: u32,
    result: EResult,
    lobby_steam_id: SteamID,
}

impl CreateLobbyCallback {
    /// Creates a new callback.
    pub(crate) fn new(
        job_id: JobID,
        app_id: u32,
        result: EResult,
        lobby_steam_id: SteamID,
    ) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            result,
            lobby_steam_id,
        }
    }

    /// The id of the app the created lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The SteamID of the created lobby.
    pub fn lobby_steam_id(&self) -> SteamID {
        self.lobby_steam_id
    }
}

impl CallbackMsg for CreateLobbyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to `set_lobby_data` and `set_lobby_member_data`.
#[derive(Clone, Debug)]
pub struct SetLobbyDataCallback {
    base: CallbackMsgBase,
    app_id: u32,
    result: EResult,
    lobby_steam_id: SteamID,
}

impl SetLobbyDataCallback {
    /// Creates a new callback.
    pub(crate) fn new(
        job_id: JobID,
        app_id: u32,
        result: EResult,
        lobby_steam_id: SteamID,
    ) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            result,
            lobby_steam_id,
        }
    }

    /// The id of the app the targeted lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The SteamID of the targeted lobby.
    pub fn lobby_steam_id(&self) -> SteamID {
        self.lobby_steam_id
    }
}

impl CallbackMsg for SetLobbyDataCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to `set_lobby_owner`.
#[derive(Clone, Debug)]
pub struct SetLobbyOwnerCallback {
    base: CallbackMsgBase,
    app_id: u32,
    result: EResult,
    lobby_steam_id: SteamID,
}

impl SetLobbyOwnerCallback {
    /// Creates a new callback.
    pub(crate) fn new(
        job_id: JobID,
        app_id: u32,
        result: EResult,
        lobby_steam_id: SteamID,
    ) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            result,
            lobby_steam_id,
        }
    }

    /// The id of the app the targeted lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The SteamID of the targeted lobby.
    pub fn lobby_steam_id(&self) -> SteamID {
        self.lobby_steam_id
    }
}

impl CallbackMsg for SetLobbyOwnerCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to `join_lobby`.
#[derive(Clone, Debug)]
pub struct JoinLobbyCallback {
    base: CallbackMsgBase,
    app_id: u32,
    chat_room_enter_response: EChatRoomEnterResponse,
    lobby: Option<Lobby>,
}

impl JoinLobbyCallback {
    /// Creates a new callback.
    pub(crate) fn new(
        job_id: JobID,
        app_id: u32,
        chat_room_enter_response: EChatRoomEnterResponse,
        lobby: Option<Lobby>,
    ) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            chat_room_enter_response,
            lobby,
        }
    }

    /// The id of the app the targeted lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The result of the request.
    pub fn chat_room_enter_response(&self) -> EChatRoomEnterResponse {
        self.chat_room_enter_response
    }

    /// The joined lobby if the enter response is
    /// [`EChatRoomEnterResponse::Success`], otherwise `None`.
    pub fn lobby(&self) -> Option<&Lobby> {
        self.lobby.as_ref()
    }
}

impl CallbackMsg for JoinLobbyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to `leave_lobby`.
#[derive(Clone, Debug)]
pub struct LeaveLobbyCallback {
    base: CallbackMsgBase,
    app_id: u32,
    result: EResult,
    lobby_steam_id: SteamID,
}

impl LeaveLobbyCallback {
    /// Creates a new callback.
    pub(crate) fn new(
        job_id: JobID,
        app_id: u32,
        result: EResult,
        lobby_steam_id: SteamID,
    ) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            result,
            lobby_steam_id,
        }
    }

    /// The id of the app the targeted lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The SteamID of the targeted lobby.
    pub fn lobby_steam_id(&self) -> SteamID {
        self.lobby_steam_id
    }
}

impl CallbackMsg for LeaveLobbyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The response to `get_lobby_data`, also posted whenever Steam sends updated
/// lobby data.
#[derive(Clone, Debug)]
pub struct LobbyDataCallback {
    base: CallbackMsgBase,
    app_id: u32,
    lobby: Lobby,
}

impl LobbyDataCallback {
    /// Creates a new callback.
    pub(crate) fn new(job_id: JobID, app_id: u32, lobby: Lobby) -> Self {
        Self {
            base: CallbackMsgBase::with_job_id(job_id),
            app_id,
            lobby,
        }
    }

    /// The id of the app the updated lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The lobby that was updated.
    pub fn lobby(&self) -> &Lobby {
        &self.lobby
    }
}

impl CallbackMsg for LobbyDataCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when Steam tells us a user joined a lobby.
#[derive(Clone, Debug)]
pub struct UserJoinedLobbyCallback {
    base: CallbackMsgBase,
    app_id: u32,
    lobby_steam_id: SteamID,
    user: Member,
}

impl UserJoinedLobbyCallback {
    /// Creates a new callback.
    pub(crate) fn new(app_id: u32, lobby_steam_id: SteamID, user: Member) -> Self {
        Self {
            base: CallbackMsgBase::new(),
            app_id,
            lobby_steam_id,
            user,
        }
    }

    /// The id of the app the lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The SteamID of the lobby that a member joined.
    pub fn lobby_steam_id(&self) -> SteamID {
        self.lobby_steam_id
    }

    /// The lobby member that joined.
    pub fn user(&self) -> &Member {
        &self.user
    }
}

impl CallbackMsg for UserJoinedLobbyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Posted when Steam tells us a user left a lobby.
#[derive(Clone, Debug)]
pub struct UserLeftLobbyCallback {
    base: CallbackMsgBase,
    app_id: u32,
    lobby_steam_id: SteamID,
    user: Member,
}

impl UserLeftLobbyCallback {
    /// Creates a new callback.
    pub(crate) fn new(app_id: u32, lobby_steam_id: SteamID, user: Member) -> Self {
        Self {
            base: CallbackMsgBase::new(),
            app_id,
            lobby_steam_id,
            user,
        }
    }

    /// The id of the app the lobby belongs to.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// The SteamID of the lobby that a member left.
    pub fn lobby_steam_id(&self) -> SteamID {
        self.lobby_steam_id
    }

    /// The lobby member that left.
    pub fn user(&self) -> &Member {
        &self.user
    }
}

impl CallbackMsg for UserLeftLobbyCallback {
    fn job_id(&self) -> JobID {
        self.base.job_id()
    }

    fn set_job_id(&mut self, id: JobID) {
        self.base.set_job_id(id);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
