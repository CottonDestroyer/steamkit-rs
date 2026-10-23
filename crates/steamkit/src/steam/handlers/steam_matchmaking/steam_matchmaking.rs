//! Creating, joining and looking up lobbies.
//!
//! Requests fail with an error when we're not logged on.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use dashmap::DashMap;
use steamkit_protos::steam::{
    CMsgClientMmsCreateLobby, CMsgClientMmsCreateLobbyResponse, CMsgClientMmsGetLobbyData,
    CMsgClientMmsGetLobbyList, CMsgClientMmsGetLobbyListResponse, CMsgClientMmsInviteToLobby,
    CMsgClientMmsJoinLobby, CMsgClientMmsJoinLobbyResponse, CMsgClientMmsLeaveLobby,
    CMsgClientMmsLeaveLobbyResponse, CMsgClientMmsLobbyData, CMsgClientMmsSetLobbyData,
    CMsgClientMmsSetLobbyDataResponse, CMsgClientMmsSetLobbyOwner,
    CMsgClientMmsSetLobbyOwnerResponse, CMsgClientMmsUserJoinedLobby,
    CMsgClientMmsUserLeftLobby,
};

use crate::base::client_msg::{ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::{EChatRoomEnterResponse, ELobbyType, EMsg, EResult};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::handlers::steam_friends::steam_friends::SteamFriends;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;
use crate::util::net_helpers::NetHelpers;

use super::callbacks::{
    CreateLobbyCallback, GetLobbyListCallback, JoinLobbyCallback, LeaveLobbyCallback,
    LobbyDataCallback, SetLobbyDataCallback, SetLobbyOwnerCallback, UserJoinedLobbyCallback,
    UserLeftLobbyCallback,
};
use super::lobby::{Filter, Lobby, Member};
use super::lobby_cache::LobbyCache;

/// The request body of a pending lobby manipulation, kept until the response
/// arrives.
#[derive(Clone, Debug)]
enum LobbyManipulationRequest {
    /// The body of a `ClientMMSCreateLobby` request.
    CreateLobby(CMsgClientMmsCreateLobby),
    /// The body of a `ClientMMSSetLobbyData` request.
    SetLobbyData(CMsgClientMmsSetLobbyData),
    /// The body of a `ClientMMSSetLobbyOwner` request.
    SetLobbyOwner(CMsgClientMmsSetLobbyOwner),
}

/// Creates, joins and looks up lobbies.
pub struct SteamMatchmaking {
    client: Weak<SteamClient>,
    lobby_manipulation_requests: DashMap<JobID, LobbyManipulationRequest>,
    lobby_cache: LobbyCache,
}

impl std::fmt::Debug for SteamMatchmaking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamMatchmaking")
            .field("pending_manipulations", &self.lobby_manipulation_requests.len())
            .finish()
    }
}

impl SteamMatchmaking {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
            lobby_manipulation_requests: DashMap::new(),
            lobby_cache: LobbyCache::new(),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client.upgrade().ok_or_else(|| {
            SteamError::Connection("SteamClient has been dropped".to_string())
        })
    }

    /// Creates a new lobby.
    ///
    /// * `app_id` - the app the lobby belongs to.
    /// * `lobby_type` - the type of the lobby.
    /// * `max_members` - the maximum number of members.
    /// * `lobby_flags` - the lobby flags.
    /// * `metadata` - the lobby metadata; `None` means none.
    pub async fn create_lobby(
        &self,
        app_id: u32,
        lobby_type: ELobbyType,
        max_members: i32,
        lobby_flags: i32,
        metadata: Option<&BTreeMap<String, String>>,
    ) -> SteamResult<CreateLobbyCallback> {
        let client = self.client()?;

        let Some(cell_id) = client.cell_id() else {
            return Err(SteamError::Connection("User not logged in.".to_string()));
        };

        let persona_name = client
            .get_required_handler::<SteamFriends>()?
            .get_persona_name();

        let mut create_lobby =
            ClientMsgProtobuf::<CMsgClientMmsCreateLobby>::new(EMsg::ClientMMSCreateLobby);

        {
            let body = create_lobby.body_mut();
            body.app_id = Some(app_id);
            body.lobby_type = Some(lobby_type.value());
            body.max_members = Some(max_members);
            body.lobby_flags = Some(lobby_flags);
            body.cell_id = Some(cell_id);
            body.public_ip = Some(NetHelpers::get_msg_ip_address(&public_ip_of(&client)));
            body.persona_name_owner = persona_name;
            body.metadata = Lobby::encode_metadata(metadata);
        }

        let job_id = client.get_next_job_id();
        create_lobby.set_source_job_id(job_id);

        let job = client.start_job::<CreateLobbyCallback>(job_id);

        self.lobby_manipulation_requests.insert(
            job_id,
            LobbyManipulationRequest::CreateLobby(create_lobby.body().clone()),
        );

        if let Err(error) = self.send(&mut create_lobby, app_id) {
            self.lobby_manipulation_requests.remove(&job_id);
            return Err(error);
        }

        self.finish_manipulation(job_id, job.wait().await)
    }

    /// Updates a lobby: its type, maximum number of members, flags and metadata.
    pub async fn set_lobby_data(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
        lobby_type: ELobbyType,
        max_members: i32,
        lobby_flags: i32,
        metadata: Option<&BTreeMap<String, String>>,
    ) -> SteamResult<SetLobbyDataCallback> {
        let client = self.client()?;

        let mut set_lobby_data =
            ClientMsgProtobuf::<CMsgClientMmsSetLobbyData>::new(EMsg::ClientMMSSetLobbyData);

        {
            let body = set_lobby_data.body_mut();
            body.app_id = Some(app_id);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
            body.steam_id_member = Some(0);
            body.lobby_type = Some(lobby_type.value());
            body.max_members = Some(max_members);
            body.lobby_flags = Some(lobby_flags);
            body.metadata = Lobby::encode_metadata(metadata);
        }

        let job_id = client.get_next_job_id();
        set_lobby_data.set_source_job_id(job_id);

        let job = client.start_job::<SetLobbyDataCallback>(job_id);

        self.lobby_manipulation_requests.insert(
            job_id,
            LobbyManipulationRequest::SetLobbyData(set_lobby_data.body().clone()),
        );

        if let Err(error) = self.send(&mut set_lobby_data, app_id) {
            self.lobby_manipulation_requests.remove(&job_id);
            return Err(error);
        }

        self.finish_manipulation(job_id, job.wait().await)
    }

    /// Updates the current user's member metadata in a lobby.
    pub async fn set_lobby_member_data(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
        metadata: &BTreeMap<String, String>,
    ) -> SteamResult<SetLobbyDataCallback> {
        let client = self.client()?;

        let Some(steam_id) = client.steam_id() else {
            return Err(SteamError::Connection("User not logged in.".to_string()));
        };

        let mut set_lobby_data =
            ClientMsgProtobuf::<CMsgClientMmsSetLobbyData>::new(EMsg::ClientMMSSetLobbyData);

        {
            let body = set_lobby_data.body_mut();
            body.app_id = Some(app_id);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
            body.steam_id_member = Some(steam_id.convert_to_u64());
            body.metadata = Lobby::encode_metadata(Some(metadata));
        }

        let job_id = client.get_next_job_id();
        set_lobby_data.set_source_job_id(job_id);

        let job = client.start_job::<SetLobbyDataCallback>(job_id);

        self.lobby_manipulation_requests.insert(
            job_id,
            LobbyManipulationRequest::SetLobbyData(set_lobby_data.body().clone()),
        );

        if let Err(error) = self.send(&mut set_lobby_data, app_id) {
            self.lobby_manipulation_requests.remove(&job_id);
            return Err(error);
        }

        self.finish_manipulation(job_id, job.wait().await)
    }

    /// Changes the owner of a lobby.
    pub async fn set_lobby_owner(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
        new_owner: SteamID,
    ) -> SteamResult<SetLobbyOwnerCallback> {
        let client = self.client()?;

        let mut set_lobby_owner =
            ClientMsgProtobuf::<CMsgClientMmsSetLobbyOwner>::new(EMsg::ClientMMSSetLobbyOwner);

        {
            let body = set_lobby_owner.body_mut();
            body.app_id = Some(app_id);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
            body.steam_id_new_owner = Some(new_owner.convert_to_u64());
        }

        let job_id = client.get_next_job_id();
        set_lobby_owner.set_source_job_id(job_id);

        let job = client.start_job::<SetLobbyOwnerCallback>(job_id);

        self.lobby_manipulation_requests.insert(
            job_id,
            LobbyManipulationRequest::SetLobbyOwner(set_lobby_owner.body().clone()),
        );

        if let Err(error) = self.send(&mut set_lobby_owner, app_id) {
            self.lobby_manipulation_requests.remove(&job_id);
            return Err(error);
        }

        self.finish_manipulation(job_id, job.wait().await)
    }

    /// Requests a list of lobbies.
    ///
    /// * `filters` - optional filters.
    /// * `max_lobbies` - optional maximum number of lobbies; `-1` means no limit.
    pub async fn get_lobby_list(
        &self,
        app_id: u32,
        filters: Option<&[Box<dyn Filter>]>,
        max_lobbies: i32,
    ) -> SteamResult<GetLobbyListCallback> {
        let client = self.client()?;

        let Some(cell_id) = client.cell_id() else {
            return Err(SteamError::Connection("User not logged in.".to_string()));
        };

        let mut get_lobbies =
            ClientMsgProtobuf::<CMsgClientMmsGetLobbyList>::new(EMsg::ClientMMSGetLobbyList);

        {
            let body = get_lobbies.body_mut();
            body.app_id = Some(app_id);
            body.cell_id = Some(cell_id);
            body.public_ip = Some(NetHelpers::get_msg_ip_address(&public_ip_of(&client)));
            body.num_lobbies_requested = Some(max_lobbies);

            if let Some(filters) = filters {
                body.filters = filters.iter().map(|filter| filter.serialize()).collect();
            }
        }

        let job_id = client.get_next_job_id();
        get_lobbies.set_source_job_id(job_id);

        let job = client.start_job::<GetLobbyListCallback>(job_id);

        if let Err(error) = self.send(&mut get_lobbies, app_id) {
            return Err(error);
        }

        job.wait().await
    }

    /// Joins a lobby.
    pub async fn join_lobby(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
    ) -> SteamResult<JoinLobbyCallback> {
        let client = self.client()?;

        let Some(persona_name) = client
            .get_required_handler::<SteamFriends>()?
            .get_persona_name()
        else {
            return Err(SteamError::Connection("User not logged in.".to_string()));
        };

        let mut join_lobby =
            ClientMsgProtobuf::<CMsgClientMmsJoinLobby>::new(EMsg::ClientMMSJoinLobby);

        {
            let body = join_lobby.body_mut();
            body.app_id = Some(app_id);
            body.persona_name = Some(persona_name);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
        }

        let job_id = client.get_next_job_id();
        join_lobby.set_source_job_id(job_id);

        let job = client.start_job::<JoinLobbyCallback>(job_id);

        if let Err(error) = self.send(&mut join_lobby, app_id) {
            return Err(error);
        }

        job.wait().await
    }

    /// Leaves a lobby.
    pub async fn leave_lobby(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
    ) -> SteamResult<LeaveLobbyCallback> {
        let client = self.client()?;

        let mut leave_lobby =
            ClientMsgProtobuf::<CMsgClientMmsLeaveLobby>::new(EMsg::ClientMMSLeaveLobby);

        {
            let body = leave_lobby.body_mut();
            body.app_id = Some(app_id);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
        }

        let job_id = client.get_next_job_id();
        leave_lobby.set_source_job_id(job_id);

        let job = client.start_job::<LeaveLobbyCallback>(job_id);

        if let Err(error) = self.send(&mut leave_lobby, app_id) {
            return Err(error);
        }

        job.wait().await
    }

    /// Requests a lobby's data.
    pub async fn get_lobby_data(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
    ) -> SteamResult<LobbyDataCallback> {
        let client = self.client()?;

        let mut get_lobby_data =
            ClientMsgProtobuf::<CMsgClientMmsGetLobbyData>::new(EMsg::ClientMMSGetLobbyData);

        {
            let body = get_lobby_data.body_mut();
            body.app_id = Some(app_id);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
        }

        let job_id = client.get_next_job_id();
        get_lobby_data.set_source_job_id(job_id);

        let job = client.start_job::<LobbyDataCallback>(job_id);

        if let Err(error) = self.send(&mut get_lobby_data, app_id) {
            return Err(error);
        }

        job.wait().await
    }

    /// Invites a user to a lobby. Steam doesn't tell us whether the invite
    /// worked.
    pub fn invite_to_lobby(
        &self,
        app_id: u32,
        lobby_steam_id: SteamID,
        user_steam_id: SteamID,
    ) -> SteamResult<()> {
        self.client()?;

        let mut invite =
            ClientMsgProtobuf::<CMsgClientMmsInviteToLobby>::new(EMsg::ClientMMSInviteToLobby);

        {
            let body = invite.body_mut();
            body.app_id = Some(app_id);
            body.steam_id_lobby = Some(lobby_steam_id.convert_to_u64());
            body.steam_id_user_invited = Some(user_steam_id.convert_to_u64());
        }

        self.send(&mut invite, app_id)
    }

    /// A lobby from the local cache, if it's there. Doesn't send any request.
    pub fn get_lobby(&self, app_id: u32, lobby_steam_id: SteamID) -> Option<Lobby> {
        self.lobby_cache.get_lobby(app_id, lobby_steam_id)
    }

    /// Sends a matchmaking message for the given app.
    pub fn send<T>(&self, msg: &mut ClientMsgProtobuf<T>, app_id: u32) -> SteamResult<()>
    where
        T: prost::Message + Default,
    {
        msg.proto_header_mut().routing_appid = Some(app_id);

        let client = self.client()?;
        client.send(msg)
    }

    /// Removes every cached lobby.
    pub fn clear_lobby_cache(&self) {
        self.lobby_cache.clear();
    }

    /// Finishes a lobby manipulation job. Handled responses remove their
    /// request from the pending ones; this also removes it when the job fails
    /// or is cancelled (on disconnect, for example).
    fn finish_manipulation<T>(&self, job_id: JobID, result: SteamResult<T>) -> SteamResult<T> {
        if result.is_err() {
            self.lobby_manipulation_requests.remove(&job_id);
        }

        result
    }

    /// Handles the response of a create lobby request.
    fn handle_create_lobby_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response =
            ClientMsgProtobuf::<CMsgClientMmsCreateLobbyResponse>::from_packet(packet_msg)?;
        let body = response.body();
        let job_id = response.target_job_id();

        if let Some((_, request)) = self.lobby_manipulation_requests.remove(&job_id) {
            if is_ok(body.eresult) {
                if let LobbyManipulationRequest::CreateLobby(create_lobby) = request {
                    let steam_id = client.steam_id();

                    let members = vec![Member::new(
                        steam_id.unwrap_or_default(),
                        create_lobby.persona_name_owner.clone().unwrap_or_default(),
                        None,
                    )];

                    let metadata = Lobby::decode_metadata(
                        &create_lobby.metadata,
                        create_lobby.metadata_kv.as_deref(),
                    )?;

                    self.lobby_cache.cache_lobby(
                        create_lobby.app_id.unwrap_or(0),
                        Lobby::new(
                            SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
                            lobby_type_of(create_lobby.lobby_type),
                            create_lobby.lobby_flags.unwrap_or(0),
                            steam_id,
                            Some(metadata),
                            create_lobby.max_members.unwrap_or(0),
                            1,
                            Some(members),
                            None,
                            None,
                        ),
                    );
                }
            }
        }

        client.post_callback(Box::new(CreateLobbyCallback::new(
            job_id,
            body.app_id.unwrap_or(0),
            result_of(body.eresult),
            SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
        )));

        Ok(())
    }

    /// Handles the response of a set lobby data request.
    fn handle_set_lobby_data_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response =
            ClientMsgProtobuf::<CMsgClientMmsSetLobbyDataResponse>::from_packet(packet_msg)?;
        let body = response.body();
        let job_id = response.target_job_id();

        if let Some((_, request)) = self.lobby_manipulation_requests.remove(&job_id) {
            if is_ok(body.eresult) {
                if let LobbyManipulationRequest::SetLobbyData(set_lobby_data) = request {
                    let app_id = set_lobby_data.app_id.unwrap_or(0);
                    let lobby_steam_id = SteamID::from_id(set_lobby_data.steam_id_lobby.unwrap_or(0));

                    if let Some(lobby) = self.lobby_cache.get_lobby(app_id, lobby_steam_id) {
                        let metadata = Lobby::decode_metadata(
                            &set_lobby_data.metadata,
                            set_lobby_data.metadata_kv.as_deref(),
                        )?;

                        if set_lobby_data.steam_id_member.unwrap_or(0) == 0 {
                            self.lobby_cache.cache_lobby(
                                app_id,
                                Lobby::new(
                                    lobby.steam_id(),
                                    lobby_type_of(set_lobby_data.lobby_type),
                                    set_lobby_data.lobby_flags.unwrap_or(0),
                                    lobby.owner_steam_id(),
                                    Some(metadata),
                                    set_lobby_data.max_members.unwrap_or(0),
                                    lobby.num_members(),
                                    Some(lobby.members().to_vec()),
                                    lobby.distance(),
                                    lobby.weight(),
                                ),
                            );
                        } else {
                            let member_id =
                                SteamID::from_id(set_lobby_data.steam_id_member.unwrap_or(0));

                            let members: Vec<Member> = lobby
                                .members()
                                .iter()
                                .map(|member| {
                                    if member.steam_id() == member_id {
                                        Member::new(
                                            member.steam_id(),
                                            member.persona_name(),
                                            Some(metadata.clone()),
                                        )
                                    } else {
                                        member.clone()
                                    }
                                })
                                .collect();

                            self.lobby_cache
                                .update_lobby_members(app_id, &lobby, Some(members));
                        }
                    }
                }
            }
        }

        client.post_callback(Box::new(SetLobbyDataCallback::new(
            job_id,
            body.app_id.unwrap_or(0),
            result_of(body.eresult),
            SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
        )));

        Ok(())
    }

    /// Handles the response of a set lobby owner request.
    fn handle_set_lobby_owner_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response =
            ClientMsgProtobuf::<CMsgClientMmsSetLobbyOwnerResponse>::from_packet(packet_msg)?;
        let body = response.body();
        let job_id = response.target_job_id();

        if let Some((_, request)) = self.lobby_manipulation_requests.remove(&job_id) {
            if is_ok(body.eresult) {
                if let LobbyManipulationRequest::SetLobbyOwner(set_lobby_owner) = request {
                    self.lobby_cache.update_lobby_owner(
                        body.app_id.unwrap_or(0),
                        SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
                        SteamID::from_id(set_lobby_owner.steam_id_new_owner.unwrap_or(0)),
                    );
                }
            }
        }

        client.post_callback(Box::new(SetLobbyOwnerCallback::new(
            job_id,
            body.app_id.unwrap_or(0),
            result_of(body.eresult),
            SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
        )));

        Ok(())
    }

    /// Handles the response of a get lobby list request.
    fn handle_get_lobby_list_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response =
            ClientMsgProtobuf::<CMsgClientMmsGetLobbyListResponse>::from_packet(packet_msg)?;
        let body = response.body();
        let job_id = response.target_job_id();
        let app_id = body.app_id.unwrap_or(0);

        let mut lobby_list = Vec::with_capacity(body.lobbies.len());

        for lobby in &body.lobbies {
            let existing_lobby = self
                .lobby_cache
                .get_lobby(app_id, SteamID::from_id(lobby.steam_id.unwrap_or(0)));

            let members = existing_lobby
                .as_ref()
                .map(|existing| existing.members().to_vec());

            let metadata =
                Lobby::decode_metadata(&lobby.metadata, lobby.metadata_kv.as_deref())?;

            lobby_list.push(Lobby::new(
                SteamID::from_id(lobby.steam_id.unwrap_or(0)),
                lobby_type_of(lobby.lobby_type),
                lobby.lobby_flags.unwrap_or(0),
                existing_lobby.as_ref().and_then(|existing| existing.owner_steam_id()),
                Some(metadata),
                lobby.max_members.unwrap_or(0),
                lobby.num_members.unwrap_or(0),
                members,
                lobby.distance,
                lobby.weight,
            ));
        }

        for lobby in &lobby_list {
            self.lobby_cache.cache_lobby(app_id, lobby.clone());
        }

        client.post_callback(Box::new(GetLobbyListCallback::new(
            job_id,
            app_id,
            result_of(body.eresult),
            lobby_list,
        )));

        Ok(())
    }

    /// Handles the response of a join lobby request.
    fn handle_join_lobby_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response =
            ClientMsgProtobuf::<CMsgClientMmsJoinLobbyResponse>::from_packet(packet_msg)?;
        let body = response.body();
        let job_id = response.target_job_id();
        let app_id = body.app_id.unwrap_or(0);

        let mut joined_lobby: Option<Lobby> = None;

        if body.steam_id_lobby.is_some() {
            let mut members = Vec::with_capacity(body.members.len());

            for member in &body.members {
                let metadata =
                    Lobby::decode_metadata(&member.metadata, member.metadata_kv.as_deref())?;

                members.push(Member::new(
                    SteamID::from_id(member.steam_id.unwrap_or(0)),
                    member.persona_name.clone().unwrap_or_default(),
                    Some(metadata),
                ));
            }

            let cached_lobby = self.lobby_cache.get_lobby(
                app_id,
                SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
            );

            let metadata = Lobby::decode_metadata(&body.metadata, body.metadata_kv.as_deref())?;

            let lobby = Lobby::new(
                SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
                lobby_type_of(body.lobby_type),
                body.lobby_flags.unwrap_or(0),
                Some(SteamID::from_id(body.steam_id_owner.unwrap_or(0))),
                Some(metadata),
                body.max_members.unwrap_or(0),
                members.len() as i32,
                Some(members),
                cached_lobby.as_ref().and_then(|cached| cached.distance()),
                cached_lobby.as_ref().and_then(|cached| cached.weight()),
            );

            self.lobby_cache.cache_lobby(app_id, lobby.clone());
            joined_lobby = Some(lobby);
        }

        client.post_callback(Box::new(JoinLobbyCallback::new(
            job_id,
            app_id,
            EChatRoomEnterResponse::from_value(body.chat_room_enter_response.unwrap_or(0))
                .unwrap_or(EChatRoomEnterResponse::Error),
            joined_lobby,
        )));

        Ok(())
    }

    /// Handles the response of a leave lobby request.
    fn handle_leave_lobby_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response =
            ClientMsgProtobuf::<CMsgClientMmsLeaveLobbyResponse>::from_packet(packet_msg)?;
        let body = response.body();

        if is_ok(body.eresult) {
            self.lobby_cache.clear_lobby_members(
                body.app_id.unwrap_or(0),
                SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
            );
        }

        client.post_callback(Box::new(LeaveLobbyCallback::new(
            response.target_job_id(),
            body.app_id.unwrap_or(0),
            result_of(body.eresult),
            SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
        )));

        Ok(())
    }

    /// Handles the lobby data Steam sends us.
    fn handle_lobby_data(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response = ClientMsgProtobuf::<CMsgClientMmsLobbyData>::from_packet(packet_msg)?;
        let body = response.body();
        let app_id = body.app_id.unwrap_or(0);

        let cached_lobby = self
            .lobby_cache
            .get_lobby(app_id, SteamID::from_id(body.steam_id_lobby.unwrap_or(0)));

        let members = if body.members.is_empty() {
            cached_lobby
                .as_ref()
                .map(|cached| cached.members().to_vec())
                .unwrap_or_default()
        } else {
            let mut members = Vec::with_capacity(body.members.len());

            for member in &body.members {
                let metadata =
                    Lobby::decode_metadata(&member.metadata, member.metadata_kv.as_deref())?;

                members.push(Member::new(
                    SteamID::from_id(member.steam_id.unwrap_or(0)),
                    member.persona_name.clone().unwrap_or_default(),
                    Some(metadata),
                ));
            }

            members
        };

        let metadata = Lobby::decode_metadata(&body.metadata, body.metadata_kv.as_deref())?;

        let updated_lobby = Lobby::new(
            SteamID::from_id(body.steam_id_lobby.unwrap_or(0)),
            lobby_type_of(body.lobby_type),
            body.lobby_flags.unwrap_or(0),
            Some(SteamID::from_id(body.steam_id_owner.unwrap_or(0))),
            Some(metadata),
            body.max_members.unwrap_or(0),
            body.num_members.unwrap_or(0),
            Some(members),
            cached_lobby.as_ref().and_then(|cached| cached.distance()),
            cached_lobby.as_ref().and_then(|cached| cached.weight()),
        );

        self.lobby_cache.cache_lobby(app_id, updated_lobby.clone());

        client.post_callback(Box::new(LobbyDataCallback::new(
            response.target_job_id(),
            app_id,
            updated_lobby,
        )));

        Ok(())
    }

    /// Handles the message that a user joined a lobby.
    fn handle_user_joined_lobby(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response = ClientMsgProtobuf::<CMsgClientMmsUserJoinedLobby>::from_packet(packet_msg)?;
        let body = response.body();
        let app_id = body.app_id.unwrap_or(0);
        let lobby_steam_id = SteamID::from_id(body.steam_id_lobby.unwrap_or(0));

        let Some(lobby) = self.lobby_cache.get_lobby(app_id, lobby_steam_id) else {
            return Ok(());
        };

        if lobby.members().is_empty() {
            return Ok(());
        }

        let joining_member = self.lobby_cache.add_lobby_member(
            app_id,
            &lobby,
            SteamID::from_id(body.steam_id_user.unwrap_or(0)),
            body.persona_name.as_deref().unwrap_or_default(),
        );

        if let Some(joining_member) = joining_member {
            client.post_callback(Box::new(UserJoinedLobbyCallback::new(
                app_id,
                lobby_steam_id,
                joining_member,
            )));
        }

        Ok(())
    }

    /// Handles the message that a user left a lobby.
    fn handle_user_left_lobby(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response = ClientMsgProtobuf::<CMsgClientMmsUserLeftLobby>::from_packet(packet_msg)?;
        let body = response.body();
        let app_id = body.app_id.unwrap_or(0);
        let lobby_steam_id = SteamID::from_id(body.steam_id_lobby.unwrap_or(0));

        let Some(lobby) = self.lobby_cache.get_lobby(app_id, lobby_steam_id) else {
            return Ok(());
        };

        if lobby.members().is_empty() {
            return Ok(());
        }

        let Some(leaving_member) = self.lobby_cache.remove_lobby_member(
            app_id,
            &lobby,
            SteamID::from_id(body.steam_id_user.unwrap_or(0)),
        ) else {
            return Ok(());
        };

        if Some(leaving_member.steam_id()) == client.steam_id() {
            self.lobby_cache.clear_lobby_members(app_id, lobby_steam_id);
        }

        client.post_callback(Box::new(UserLeftLobbyCallback::new(
            app_id,
            lobby_steam_id,
            leaving_member,
        )));

        Ok(())
    }
}

#[async_trait]
impl ClientMsgHandler for SteamMatchmaking {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        match packet_msg.msg_type() {
            EMsg::ClientMMSCreateLobbyResponse => {
                self.handle_create_lobby_response(client, packet_msg)?;
            }
            EMsg::ClientMMSSetLobbyDataResponse => {
                self.handle_set_lobby_data_response(client, packet_msg)?;
            }
            EMsg::ClientMMSSetLobbyOwnerResponse => {
                self.handle_set_lobby_owner_response(client, packet_msg)?;
            }
            EMsg::ClientMMSLobbyData => {
                self.handle_lobby_data(client, packet_msg)?;
            }
            EMsg::ClientMMSGetLobbyListResponse => {
                self.handle_get_lobby_list_response(client, packet_msg)?;
            }
            EMsg::ClientMMSJoinLobbyResponse => {
                self.handle_join_lobby_response(client, packet_msg)?;
            }
            EMsg::ClientMMSLeaveLobbyResponse => {
                self.handle_leave_lobby_response(client, packet_msg)?;
            }
            EMsg::ClientMMSUserJoinedLobby => {
                self.handle_user_joined_lobby(client, packet_msg)?;
            }
            EMsg::ClientMMSUserLeftLobby => {
                self.handle_user_left_lobby(client, packet_msg)?;
            }
            _ => {}
        }

        Ok(())
    }
}

/// Our public IP address, or the unspecified address if it isn't known yet.
fn public_ip_of(client: &SteamClient) -> IpAddr {
    client
        .public_ip()
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

/// Whether the result code means success.
fn is_ok(eresult: Option<i32>) -> bool {
    eresult.unwrap_or(0) == EResult::OK.value()
}

/// Converts a raw result code into an [`EResult`].
fn result_of(eresult: Option<i32>) -> EResult {
    EResult::from_value(eresult.unwrap_or(0)).unwrap_or(EResult::Invalid)
}

/// Converts a raw lobby type into an [`ELobbyType`].
fn lobby_type_of(lobby_type: Option<i32>) -> ELobbyType {
    ELobbyType::from_value(lobby_type.unwrap_or(0)).unwrap_or(ELobbyType::Private)
}
