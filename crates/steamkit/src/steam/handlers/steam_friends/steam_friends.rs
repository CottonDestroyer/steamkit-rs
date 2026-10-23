//! Friends, clans, chat rooms and persona state.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use parking_lot::RwLock;
use steamkit_protos::steam::{
    CMsgClientAccountInfo, CMsgClientAddFriend, CMsgClientChangeStatus,
    CMsgClientChatGetFriendMessageHistory, CMsgClientChatGetFriendMessageHistoryForOfflineMessages,
    CMsgClientChatGetFriendMessageHistoryResponse, CMsgClientChatInvite, CMsgClientFriendMsg,
    CMsgClientFriendProfileInfo, CMsgClientFriendsList, CMsgClientPersonaState,
    CMsgClientRemoveFriend, CMsgClientRequestFriendData, CMsgPersonaChangeResponse,
};

use crate::base::client_msg::{ClientMsg, ClientMsgProtobuf, IClientMsg};
use crate::base::generated::steam_language::internal::{
    MsgClientChatAction, MsgClientChatMemberInfo, MsgClientChatMsg, MsgClientJoinChat,
    MsgClientSetIgnoreFriend,
};
use crate::base::generated::steam_language::{
    EAccountType, EChatAction, EChatEntryType, EChatInfoType, EChatMemberStateChange,
    EClanRelationship, EClientPersonaStateFlag, EFriendRelationship, EMsg, EPersonaState,
};
use crate::base::packet_base::IPacketMsg;
use crate::error::{SteamError, SteamResult};
use crate::steam::handlers::client_msg_handler::ClientMsgHandler;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::steam::steam_client::steam_client::SteamClient;
use crate::types::game_id::GameID;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;

use super::callbacks::{
    ChatActionResultCallback, ChatEnterCallback, ChatInviteCallback, ChatMemberInfoCallback,
    ChatMsgCallback, ChatRoomInfoCallback, ClanStateCallback, FriendAddedCallback,
    FriendMsgCallback, FriendMsgEchoCallback, FriendMsgHistoryCallback, FriendsListCallback,
    IgnoreFriendCallback, PersonaChangeCallback, PersonaStateCallback, ProfileInfoCallback,
};
use super::friend_cache::AccountCache;

/// The account instance flag that marks a clan chat.
const CHAT_INSTANCE_FLAG_CLAN: u64 = 0x0008_0000;

/// The friend and clan lists of the local user.
#[derive(Default)]
struct FriendLists {
    friend_list: Vec<SteamID>,
    clan_list: Vec<SteamID>,
}

/// Whether the given raw SteamID belongs to a clan account.
fn is_clan_account(raw: u64) -> bool {
    ((raw >> 52) & 0x0F) == EAccountType::Clan.value() as u64
}

/// Converts a clan SteamID to the SteamID of its group chat (fixes up the
/// account instance and account type).
fn to_chat_steam_id(raw: u64) -> u64 {
    if !is_clan_account(raw) {
        return raw;
    }

    let cleared = raw & !(0x000F_FFFF_u64 << 32) & !(0x0F_u64 << 52);
    cleared | (CHAT_INSTANCE_FLAG_CLAN << 32) | ((EAccountType::Chat.value() as u64) << 52)
}

/// Everything to do with other users: friends, clans, chat and persona state.
pub struct SteamFriends {
    client: Weak<SteamClient>,
    lists: RwLock<FriendLists>,
    cache: AccountCache,
}

impl SteamFriends {
    /// Creates the handler and stores a weak back-reference to the client.
    pub fn new(client: &Arc<SteamClient>) -> Self {
        Self {
            client: Arc::downgrade(client),
            lists: RwLock::new(FriendLists::default()),
            cache: AccountCache::new(),
        }
    }

    /// Upgrades the weak client reference.
    fn client(&self) -> SteamResult<Arc<SteamClient>> {
        self.client
            .upgrade()
            .ok_or_else(|| SteamError::Connection("SteamClient has been dropped".to_string()))
    }

    /// The local user's persona name. `None` until the account info has
    /// arrived (just before `AccountInfoCallback`).
    pub fn get_persona_name(&self) -> Option<String> {
        self.cache.local_user().name
    }

    /// Sets the local user's persona name and broadcasts it over the network.
    pub fn set_persona_name(&self, name: &str) -> SteamResult<()> {
        let client = self.client()?;

        // cache the local name right away, so that early calls to
        // set_persona_state don't reset the set name
        self.cache.set_local_user_name(name.to_string());

        let mut state_msg =
            ClientMsgProtobuf::<CMsgClientChangeStatus>::new(EMsg::ClientChangeStatus);

        state_msg.body.persona_state = Some(self.cache.local_user_persona_state() as u32);
        state_msg.body.player_name = Some(name.to_string());

        client.send(&state_msg)?;

        Ok(())
    }

    /// The local user's persona state.
    pub fn get_persona_state(&self) -> EPersonaState {
        self.cache.local_user_persona_state()
    }

    /// Sets the local user's persona state and broadcasts it over the network.
    pub fn set_persona_state(&self, state: EPersonaState) -> SteamResult<()> {
        let client = self.client()?;

        self.cache.set_local_user_persona_state(state);

        let mut state_msg =
            ClientMsgProtobuf::<CMsgClientChangeStatus>::new(EMsg::ClientChangeStatus);

        state_msg.body.persona_state = Some(state as u32);

        client.send(&state_msg)?;

        Ok(())
    }

    /// The friend count of the local user.
    pub fn get_friend_count(&self) -> usize {
        self.lists.read().friend_list.len()
    }

    /// A friend by index, or an all-zero SteamID if the index is out of range.
    pub fn get_friend_by_index(&self, index: usize) -> SteamID {
        self.lists
            .read()
            .friend_list
            .get(index)
            .copied()
            .unwrap_or_else(SteamID::new)
    }

    /// Gets the persona name of a friend.
    pub fn get_friend_persona_name(&self, steam_id: SteamID) -> Option<String> {
        self.cache.user(steam_id).name
    }

    /// Gets the persona state of a friend.
    pub fn get_friend_persona_state(&self, steam_id: SteamID) -> EPersonaState {
        self.cache.user(steam_id).persona_state
    }

    /// How a user relates to the local user.
    pub fn get_friend_relationship(&self, steam_id: SteamID) -> EFriendRelationship {
        self.cache.user(steam_id).relationship
    }

    /// The name of the game a friend is playing; `None` if the friend isn't
    /// cached yet.
    pub fn get_friend_game_played_name(&self, steam_id: SteamID) -> Option<String> {
        self.cache.user(steam_id).game_name
    }

    /// The game a friend is playing; 0 if the friend isn't cached yet.
    pub fn get_friend_game_played(&self, steam_id: SteamID) -> GameID {
        GameID::from_value(self.cache.user(steam_id).game_id)
    }

    /// The SHA-1 hash of a friend's avatar.
    pub fn get_friend_avatar(&self, steam_id: SteamID) -> Option<Vec<u8>> {
        self.cache.user(steam_id).avatar_hash
    }

    /// The number of clans the local user is in.
    pub fn get_clan_count(&self) -> usize {
        self.lists.read().clan_list.len()
    }

    /// A clan by index, or an all-zero SteamID if the index is out of range.
    pub fn get_clan_by_index(&self, index: usize) -> SteamID {
        self.lists
            .read()
            .clan_list
            .get(index)
            .copied()
            .unwrap_or_else(SteamID::new)
    }

    /// Gets the name of a clan.
    pub fn get_clan_name(&self, steam_id: SteamID) -> Option<String> {
        self.cache.clan(steam_id).name
    }

    /// How a clan relates to the local user.
    pub fn get_clan_relationship(&self, steam_id: SteamID) -> EClanRelationship {
        self.cache.clan(steam_id).relationship
    }

    /// The SHA-1 hash of a clan's avatar.
    pub fn get_clan_avatar(&self, steam_id: SteamID) -> Option<Vec<u8>> {
        self.cache.clan(steam_id).avatar_hash
    }

    /// Sends a chat message to a friend.
    pub fn send_chat_message(
        &self,
        target: SteamID,
        msg_type: EChatEntryType,
        message: &str,
    ) -> SteamResult<()> {
        let client = self.client()?;

        let mut chat_msg = ClientMsgProtobuf::<CMsgClientFriendMsg>::new(EMsg::ClientFriendMsg);

        chat_msg.body.steamid = Some(target.convert_to_u64());
        chat_msg.body.chat_entry_type = Some(msg_type as i32);
        chat_msg.body.message = Some(message.as_bytes().to_vec());

        client.send(&chat_msg)?;

        Ok(())
    }

    /// Sends a friend request to a user.
    pub fn add_friend(&self, account_name_or_email: &str) -> SteamResult<()> {
        let client = self.client()?;

        let mut add_friend = ClientMsgProtobuf::<CMsgClientAddFriend>::new(EMsg::ClientAddFriend);

        add_friend.body.accountname_or_email_to_add = Some(account_name_or_email.to_string());

        client.send(&add_friend)?;

        Ok(())
    }

    /// Sends a friend request to a user.
    pub fn add_friend_by_steam_id(&self, steam_id: SteamID) -> SteamResult<()> {
        let client = self.client()?;

        let mut add_friend = ClientMsgProtobuf::<CMsgClientAddFriend>::new(EMsg::ClientAddFriend);

        add_friend.body.steamid_to_add = Some(steam_id.convert_to_u64());

        client.send(&add_friend)?;

        Ok(())
    }

    /// Removes a friend from your friends list.
    pub fn remove_friend(&self, steam_id: SteamID) -> SteamResult<()> {
        let client = self.client()?;

        let mut remove_friend =
            ClientMsgProtobuf::<CMsgClientRemoveFriend>::new(EMsg::ClientRemoveFriend);

        remove_friend.body.friendid = Some(steam_id.convert_to_u64());

        client.send(&remove_friend)?;

        Ok(())
    }

    /// Attempts to join a chat room.
    pub fn join_chat(&self, steam_id: SteamID) -> SteamResult<()> {
        let client = self.client()?;

        let raw = steam_id.convert_to_u64();
        let chat_id = if is_clan_account(raw) {
            steam_id.to_chat_id()
        } else {
            raw
        };

        let mut join_chat = ClientMsg::<MsgClientJoinChat>::new(EMsg::ClientJoinChat);
        join_chat.body.steam_id_chat = chat_id;

        client.send(&join_chat)?;

        Ok(())
    }

    /// Attempts to leave a chat room.
    pub fn leave_chat(&self, steam_id: SteamID) -> SteamResult<()> {
        let client = self.client()?;

        let chat_id = to_chat_steam_id(steam_id.convert_to_u64());

        let mut leave_chat = ClientMsg::<MsgClientChatMemberInfo>::new(EMsg::ClientChatMemberInfo);
        leave_chat.body.steam_id_chat = chat_id;
        leave_chat.body.r#type = EChatInfoType::StateChange;

        // Our SteamID can be missing if we're not connected; the server
        // ignores it anyway.
        let local_steam_id = client.steam_id().map(|id| id.convert_to_u64()).unwrap_or_default();

        leave_chat.payload.write_u64(local_steam_id); // ChatterActedOn
        leave_chat
            .payload
            .write_u32(EChatMemberStateChange::Left.value() as u32); // StateChange
        leave_chat.payload.write_u64(local_steam_id); // ChatterActedBy

        client.send(&leave_chat)?;

        Ok(())
    }

    /// Sends a message to a chat room.
    pub fn send_chat_room_message(
        &self,
        steam_id_chat: SteamID,
        msg_type: EChatEntryType,
        message: &str,
    ) -> SteamResult<()> {
        let client = self.client()?;

        let chat_id = to_chat_steam_id(steam_id_chat.convert_to_u64());

        let mut chat_msg = ClientMsg::<MsgClientChatMsg>::new(EMsg::ClientChatMsg);

        chat_msg.body.chat_msg_type = msg_type;
        chat_msg.body.steam_id_chat_room = chat_id;
        // Can be missing if we're not connected; the server ignores it anyway.
        chat_msg.body.steam_id_chatter = client.steam_id().map(|id| id.convert_to_u64()).unwrap_or_default();

        chat_msg.payload.write_null_term_string(message);

        client.send(&chat_msg)?;

        Ok(())
    }

    /// Invites a user to a chat room. The result comes in a
    /// [`ChatActionResultCallback`].
    pub fn invite_user_to_chat(
        &self,
        steam_id_user: SteamID,
        steam_id_chat: SteamID,
    ) -> SteamResult<()> {
        let client = self.client()?;

        let chat_id = to_chat_steam_id(steam_id_chat.convert_to_u64());

        let mut invite_msg = ClientMsgProtobuf::<CMsgClientChatInvite>::new(EMsg::ClientChatInvite);

        invite_msg.body.steam_id_chat = Some(chat_id);
        invite_msg.body.steam_id_invited = Some(steam_id_user.convert_to_u64());
        // The Steam client also sends who did the inviting, so we do too.
        invite_msg.body.steam_id_patron =
            Some(client.steam_id().map(|id| id.convert_to_u64()).unwrap_or_default());

        client.send(&invite_msg)?;

        Ok(())
    }

    /// Kicks the specified chat member from the given chat room.
    pub fn kick_chat_member(
        &self,
        steam_id_chat: SteamID,
        steam_id_member: SteamID,
    ) -> SteamResult<()> {
        self.chat_action(steam_id_chat, steam_id_member, EChatAction::Kick)
    }

    /// Bans the specified chat member from the given chat room.
    pub fn ban_chat_member(
        &self,
        steam_id_chat: SteamID,
        steam_id_member: SteamID,
    ) -> SteamResult<()> {
        self.chat_action(steam_id_chat, steam_id_member, EChatAction::Ban)
    }

    /// Unbans the specified SteamID from the given chat room.
    pub fn unban_chat_member(
        &self,
        steam_id_chat: SteamID,
        steam_id_member: SteamID,
    ) -> SteamResult<()> {
        self.chat_action(steam_id_chat, steam_id_member, EChatAction::UnBan)
    }

    /// Sends a chat action to the given chat room.
    fn chat_action(
        &self,
        steam_id_chat: SteamID,
        steam_id_member: SteamID,
        action: EChatAction,
    ) -> SteamResult<()> {
        let client = self.client()?;

        let chat_id = to_chat_steam_id(steam_id_chat.convert_to_u64());

        let mut action_msg = ClientMsg::<MsgClientChatAction>::new(EMsg::ClientChatAction);

        action_msg.body.steam_id_chat = chat_id;
        action_msg.body.steam_id_user_to_act_on = steam_id_member.convert_to_u64();
        action_msg.body.chat_action = action;

        client.send(&action_msg)?;

        Ok(())
    }

    /// Requests the persona state of a list of users. Results come in
    /// [`PersonaStateCallback`]s. Without info flags, the configured default
    /// flags are used.
    pub fn request_friend_info(
        &self,
        steam_id_list: &[SteamID],
        requested_info: Option<EClientPersonaStateFlag>,
    ) -> SteamResult<()> {
        let client = self.client()?;

        let requested_info =
            requested_info.unwrap_or_else(|| client.configuration().default_persona_state_flags());

        let mut request =
            ClientMsgProtobuf::<CMsgClientRequestFriendData>::new(EMsg::ClientRequestFriendData);

        request.body.friends = steam_id_list.iter().map(|id| id.convert_to_u64()).collect();
        request.body.persona_state_requested = Some(requested_info as u32);

        client.send(&request)?;

        Ok(())
    }

    /// Requests the persona state of a user. Results come in a
    /// [`PersonaStateCallback`]. Without info flags, the configured default
    /// flags are used.
    pub fn request_friend_info_for_steam_id(
        &self,
        steam_id: SteamID,
        requested_info: Option<EClientPersonaStateFlag>,
    ) -> SteamResult<()> {
        self.request_friend_info(&[steam_id], requested_info)
    }

    /// Ignores or unignores a friend. The result comes in an
    /// [`IgnoreFriendCallback`] with the returned job id.
    pub fn ignore_friend(&self, steam_id: SteamID, set_ignore: bool) -> SteamResult<JobID> {
        let client = self.client()?;

        let mut ignore = ClientMsg::<MsgClientSetIgnoreFriend>::new(EMsg::ClientSetIgnoreFriend);
        let job_id = client.get_next_job_id();
        ignore.set_source_job_id(job_id);

        // Our SteamID can be missing if we're not connected; the server
        // ignores it anyway.
        ignore.body.my_steam_id = client.steam_id().map(|id| id.convert_to_u64()).unwrap_or_default();
        ignore.body.ignore = u8::from(set_ignore);
        ignore.body.steam_id_friend = steam_id.convert_to_u64();

        client.send(&ignore)?;

        Ok(job_id)
    }

    /// Requests a user's profile info. The result comes in a
    /// [`ProfileInfoCallback`] with the returned job id.
    pub fn request_profile_info(&self, steam_id: SteamID) -> SteamResult<JobID> {
        let client = self.client()?;

        let mut request =
            ClientMsgProtobuf::<CMsgClientFriendProfileInfo>::new(EMsg::ClientFriendProfileInfo);
        let job_id = client.get_next_job_id();
        request.set_source_job_id(job_id);

        request.body.steamid_friend = Some(steam_id.convert_to_u64());

        client.send(&request)?;

        Ok(job_id)
    }

    /// Requests the last few messages with a friend. The result comes in a
    /// [`FriendMsgHistoryCallback`].
    pub fn request_message_history(&self, steam_id: SteamID) -> SteamResult<()> {
        let client = self.client()?;

        let mut request = ClientMsgProtobuf::<CMsgClientChatGetFriendMessageHistory>::new(
            EMsg::ClientChatGetFriendMessageHistory,
        );

        request.body.steamid = Some(steam_id.convert_to_u64());

        client.send(&request)?;

        Ok(())
    }

    /// Requests all offline messages, which also marks them as read. The
    /// result comes in a [`FriendMsgHistoryCallback`].
    pub fn request_offline_messages(&self) -> SteamResult<()> {
        let client = self.client()?;

        let request =
            ClientMsgProtobuf::<CMsgClientChatGetFriendMessageHistoryForOfflineMessages>::new(
                EMsg::ClientChatGetFriendMessageHistoryForOfflineMessages,
            );

        client.send(&request)?;

        Ok(())
    }

    /// Caches the local persona name.
    fn handle_account_info(&self, packet_msg: &dyn IPacketMsg) -> SteamResult<()> {
        let acc_info = ClientMsgProtobuf::<CMsgClientAccountInfo>::from_packet(packet_msg)?;

        self.cache
            .set_local_user_name(acc_info.body.persona_name.clone().unwrap_or_default());

        Ok(())
    }

    /// Handles the response of a message history request.
    fn handle_friend_message_history_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let history_response =
            ClientMsgProtobuf::<CMsgClientChatGetFriendMessageHistoryResponse>::from_packet(
                packet_msg,
            )?;

        let callback = FriendMsgHistoryCallback::new(
            &history_response.body,
            client.configuration().universe(),
        );

        client.post_callback(Box::new(callback));

        Ok(())
    }

    /// Handles the friend list the server sends us.
    fn handle_friends_list(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let list = ClientMsgProtobuf::<CMsgClientFriendsList>::from_packet(packet_msg)?;
        let body = &list.body;

        if let Some(steam_id) = client.steam_id() {
            self.cache.set_local_user_steam_id(steam_id);
        }

        let incremental = body.bincremental.unwrap_or_default();

        if !incremental {
            // if we're not an incremental update, the message contains all
            // friends, so we should clear our current list
            let mut lists = self.lists.write();
            lists.friend_list.clear();
            lists.clan_list.clear();
        }

        // we have to request information for all of our friends because steam
        // only sends persona information for online friends
        let mut req_info =
            ClientMsgProtobuf::<CMsgClientRequestFriendData>::new(EMsg::ClientRequestFriendData);

        req_info.body.persona_state_requested =
            Some(client.configuration().default_persona_state_flags() as u32);

        {
            let mut lists = self.lists.write();
            let mut friends_to_remove = Vec::new();
            let mut clans_to_remove = Vec::new();

            for friend_obj in &body.friends {
                let friend_id = SteamID::from_id(friend_obj.ulfriendid.unwrap_or_default());

                if friend_id.account_type() == EAccountType::Individual {
                    let relationship = EFriendRelationship::from_value(
                        friend_obj.efriendrelationship.unwrap_or_default() as i32,
                    )
                    .unwrap_or(EFriendRelationship::None);

                    self.cache
                        .with_user(friend_id, |user| user.relationship = relationship);

                    if lists.friend_list.contains(&friend_id) {
                        // if this is a friend on our list, and they removed us,
                        // mark them for removal
                        if relationship == EFriendRelationship::None {
                            friends_to_remove.push(friend_id);
                        }
                    } else {
                        // we don't know about this friend yet, lets add them
                        lists.friend_list.push(friend_id);
                    }
                } else if friend_id.account_type() == EAccountType::Clan {
                    let relationship = EClanRelationship::from_value(
                        friend_obj.efriendrelationship.unwrap_or_default() as i32,
                    )
                    .unwrap_or(EClanRelationship::None);

                    self.cache
                        .with_clan(friend_id, |clan| clan.relationship = relationship);

                    if lists.clan_list.contains(&friend_id) {
                        // mark clans we were removed/kicked from
                        if relationship == EClanRelationship::None
                            || relationship == EClanRelationship::Kicked
                        {
                            clans_to_remove.push(friend_id);
                        }
                    } else {
                        // don't know about this clan, add it
                        lists.clan_list.push(friend_id);
                    }
                }

                if !incremental {
                    // request persona state for our friend & clan list when it's
                    // a non-incremental update
                    req_info.body.friends.push(friend_id.convert_to_u64());
                }
            }

            // remove anything we marked for removal
            lists
                .friend_list
                .retain(|id| !friends_to_remove.contains(id));
            lists.clan_list.retain(|id| !clans_to_remove.contains(id));
        }

        if !req_info.body.friends.is_empty() {
            client.send(&req_info)?;
        }

        let callback = FriendsListCallback::from_body(body);
        client.post_callback(Box::new(callback));

        Ok(())
    }

    /// Handles the persona state the server sends us.
    fn handle_persona_state(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let per_state = ClientMsgProtobuf::<CMsgClientPersonaState>::from_packet(packet_msg)?;
        let body = &per_state.body;

        let flags = body.status_flags.unwrap_or_default();
        let status_flags = EClientPersonaStateFlag::from_value(flags as i32).unwrap_or_else(|| {
            EClientPersonaStateFlag::ALL
                .iter()
                .copied()
                .find(|flag| flags & (flag.value() as u32) != 0)
                .unwrap_or(EClientPersonaStateFlag::Status)
        });

        for friend in &body.friends {
            let friend_id = SteamID::from_id(friend.friendid.unwrap_or_default());

            if friend_id.account_type() == EAccountType::Individual {
                self.cache.with_user(friend_id, |user| {
                    if flags & (EClientPersonaStateFlag::PlayerName as u32) != 0 {
                        user.name = friend.player_name.clone();
                    }

                    if flags & (EClientPersonaStateFlag::Presence as u32) != 0 {
                        user.avatar_hash = friend.avatar_hash.clone();
                        user.persona_state = EPersonaState::from_value(
                            friend.persona_state.unwrap_or_default() as i32,
                        )
                        .unwrap_or(EPersonaState::Offline);
                        user.persona_state_flags = friend.persona_state_flags.unwrap_or_default();
                    }

                    if flags & (EClientPersonaStateFlag::GameDataBlob as u32) != 0 {
                        user.game_name = friend.game_name.clone();
                        user.game_id = friend.gameid.unwrap_or_default();
                        user.game_app_id = friend.game_played_app_id.unwrap_or_default();
                    }
                });
            } else if friend_id.account_type() == EAccountType::Clan {
                self.cache.with_clan(friend_id, |clan| {
                    if flags & (EClientPersonaStateFlag::PlayerName as u32) != 0 {
                        clan.name = friend.player_name.clone();
                    }

                    if flags & (EClientPersonaStateFlag::Presence as u32) != 0 {
                        clan.avatar_hash = friend.avatar_hash.clone();
                    }
                });
            }

            let callback = PersonaStateCallback::new(friend, status_flags);
            client.post_callback(Box::new(callback));
        }

        Ok(())
    }

    /// Handles the response to a persona name or state change.
    fn handle_persona_change_response(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let response = ClientMsgProtobuf::<CMsgPersonaChangeResponse>::from_packet(packet_msg)?;

        // update our cache to what steam says our name is
        self.cache
            .set_local_user_name(response.body.player_name.clone().unwrap_or_default());

        let callback = PersonaChangeCallback::new(packet_msg.target_job_id(), &response.body);
        client.post_callback(Box::new(callback));

        Ok(())
    }
}

#[async_trait]
impl ClientMsgHandler for SteamFriends {
    async fn handle_msg(
        &self,
        client: &SteamClient,
        packet_msg: &dyn IPacketMsg,
    ) -> SteamResult<()> {
        let callback: Option<Box<dyn CallbackMsg>> = match packet_msg.msg_type() {
            EMsg::ClientClanState => Some(Box::new(ClanStateCallback::from_packet(packet_msg)?)),
            EMsg::ClientFriendMsgIncoming => {
                Some(Box::new(FriendMsgCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientFriendMsgEchoToSender => {
                Some(Box::new(FriendMsgEchoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientAddFriendResponse => {
                Some(Box::new(FriendAddedCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientChatEnter => Some(Box::new(ChatEnterCallback::from_packet(packet_msg)?)),
            EMsg::ClientChatMsg => Some(Box::new(ChatMsgCallback::from_packet(packet_msg)?)),
            EMsg::ClientChatMemberInfo => {
                Some(Box::new(ChatMemberInfoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientChatRoomInfo => {
                Some(Box::new(ChatRoomInfoCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientChatActionResult => {
                Some(Box::new(ChatActionResultCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientChatInvite => Some(Box::new(ChatInviteCallback::from_packet(packet_msg)?)),
            EMsg::ClientSetIgnoreFriendResponse => {
                Some(Box::new(IgnoreFriendCallback::from_packet(packet_msg)?))
            }
            EMsg::ClientFriendProfileInfoResponse => {
                Some(Box::new(ProfileInfoCallback::from_packet(packet_msg)?))
            }
            _ => None,
        };

        if let Some(callback) = callback {
            client.post_callback(callback);
            return Ok(());
        }

        // Some messages need access to the client or post their callbacks
        // differently.
        match packet_msg.msg_type() {
            EMsg::ClientPersonaState => self.handle_persona_state(client, packet_msg)?,
            EMsg::ClientFriendsList => self.handle_friends_list(client, packet_msg)?,
            EMsg::ClientChatGetFriendMessageHistoryResponse => {
                self.handle_friend_message_history_response(client, packet_msg)?
            }
            EMsg::ClientAccountInfo => self.handle_account_info(packet_msg)?,
            EMsg::ClientPersonaChangeResponse => {
                self.handle_persona_change_response(client, packet_msg)?
            }
            _ => {}
        }

        Ok(())
    }
}
