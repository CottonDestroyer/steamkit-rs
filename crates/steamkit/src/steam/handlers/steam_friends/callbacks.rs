//! Callbacks posted by [`SteamFriends`](super::steam_friends::SteamFriends).

use std::any::Any;
use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, NaiveDateTime, Utc};
use steamkit_protos::steam::{
    c_msg_client_clan_state, c_msg_client_friends_list, c_msg_client_persona_state,
    c_msg_ip_address, CMsgClientAddFriendResponse, CMsgClientChatGetFriendMessageHistoryResponse,
    CMsgClientChatInvite, CMsgClientClanState, CMsgClientFriendMsgIncoming,
    CMsgClientFriendProfileInfoResponse, CMsgClientFriendsList, CMsgIpAddress,
    CMsgPersonaChangeResponse,
};

use crate::base::client_msg::{ClientMsg, ClientMsgProtobuf};
use crate::base::generated::steam_language::internal::{
    MsgClientChatActionResult, MsgClientChatEnter, MsgClientChatMemberInfo, MsgClientChatMsg,
    MsgClientChatRoomInfo, MsgClientSetIgnoreFriendResponse,
};
use crate::base::generated::steam_language::{
    EAccountFlags, EAccountType, EChatAction, EChatActionResult, EChatEntryType, EChatInfoType,
    EChatMemberStateChange, EChatPermission, EChatRoomEnterResponse, EChatRoomType,
    EClanPermission, EClientPersonaStateFlag, EFriendRelationship, EPersonaState,
    EPersonaStateFlag, EResult, EUniverse,
};
use crate::base::msg_base::MsgBase;
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::game_id::GameID;
use crate::types::global_id::GlobalID;
use crate::types::job_id::JobID;
use crate::types::key_value::KeyValue;
use crate::types::message_object::MessageObject;
use crate::types::steam_id::SteamID;

/// Converts a Unix timestamp to a UTC date time.
fn date_time_from_unix_time(seconds: u64) -> DateTime<Utc> {
    let secs = i64::try_from(seconds).unwrap_or(i64::MAX);
    DateTime::<Utc>::from_timestamp(secs, 0)
        .or_else(|| DateTime::<Utc>::from_timestamp(0, 0))
        .unwrap_or_else(|| {
            DateTime::<Utc>::from_naive_utc_and_offset(NaiveDateTime::default(), Utc)
        })
}

/// Converts the IP address in a protobuf message to an [`IpAddr`].
fn ip_address_from_msg(msg: &CMsgIpAddress) -> Option<IpAddr> {
    match &msg.ip {
        Some(c_msg_ip_address::Ip::V4(raw)) => {
            let octets = [
                ((raw & 0xFF00_0000) >> 24) as u8,
                ((raw & 0x00FF_0000) >> 8) as u8,
                ((raw & 0x0000_FF00) << 8) as u8,
                ((raw & 0x0000_00FF) << 24) as u8,
            ];
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        Some(c_msg_ip_address::Ip::V6(raw)) => {
            let mut octets = [0u8; 16];
            if raw.len() != 16 {
                return None;
            }
            octets.copy_from_slice(raw);
            Some(IpAddr::from(octets))
        }
        None => None,
    }
}

/// Reads a `KeyValue` as an enum member. Accepts the member's name or its
/// numeric value.
fn key_value_as_enum<T: Copy>(
    value: &KeyValue,
    members: &[(&str, T)],
    value_of: impl Fn(T) -> i32,
) -> Option<T> {
    let text = value.as_string()?.trim();

    if text.is_empty() {
        return None;
    }

    if let Ok(number) = text.parse::<i32>() {
        return members
            .iter()
            .find(|(_, member)| value_of(*member) == number)
            .map(|(_, member)| *member);
    }

    members
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, member)| *member)
}

/// The `EClanPermission` members and the names they parse from.
const CLAN_PERMISSION_MEMBERS: &[(&str, EClanPermission)] = &[
    ("Nobody", EClanPermission::Nobody),
    ("Owner", EClanPermission::Owner),
    ("Officer", EClanPermission::Officer),
    ("OwnerAndOfficer", EClanPermission::OwnerAndOfficer),
    ("Member", EClanPermission::Member),
    ("Moderator", EClanPermission::Moderator),
    (
        "OwnerOfficerModerator",
        EClanPermission::OwnerOfficerModerator,
    ),
    ("AllMembers", EClanPermission::AllMembers),
    ("OGGGameOwner", EClanPermission::OGGGameOwner),
    ("NonMember", EClanPermission::NonMember),
    ("MemberAllowed", EClanPermission::MemberAllowed),
    ("ModeratorAllowed", EClanPermission::ModeratorAllowed),
    ("OfficerAllowed", EClanPermission::OfficerAllowed),
    ("OwnerAllowed", EClanPermission::OwnerAllowed),
    ("Anybody", EClanPermission::Anybody),
];

/// The `EChatPermission` members and the names they parse from.
const CHAT_PERMISSION_MEMBERS: &[(&str, EChatPermission)] = &[
    ("Close", EChatPermission::Close),
    ("Invite", EChatPermission::Invite),
    ("Talk", EChatPermission::Talk),
    ("Kick", EChatPermission::Kick),
    ("Mute", EChatPermission::Mute),
    ("SetMetadata", EChatPermission::SetMetadata),
    ("ChangePermissions", EChatPermission::ChangePermissions),
    ("Ban", EChatPermission::Ban),
    ("ChangeAccess", EChatPermission::ChangeAccess),
    (
        "EveryoneNotInClanDefault",
        EChatPermission::EveryoneNotInClanDefault,
    ),
    ("EveryoneDefault", EChatPermission::EveryoneDefault),
    ("MemberDefault", EChatPermission::MemberDefault),
    ("OfficerDefault", EChatPermission::OfficerDefault),
    ("OwnerDefault", EChatPermission::OwnerDefault),
    ("Mask", EChatPermission::Mask),
];

/// The most specific persona state flag in a raw bit set (the generated enum
/// can't hold a combination).
fn persona_state_flag_from_bits(bits: u32) -> EPersonaStateFlag {
    EPersonaStateFlag::from_value(bits as i32).unwrap_or_else(|| {
        EPersonaStateFlag::ALL
            .iter()
            .copied()
            .find(|flag| bits & (flag.value() as u32) != 0)
            .unwrap_or(EPersonaStateFlag::HasRichPresence)
    })
}

/// The most specific account flag in a raw bit set (the generated enum can't
/// hold a combination).
fn account_flags_from_bits(bits: u32) -> EAccountFlags {
    EAccountFlags::from_value(bits as i32).unwrap_or_else(|| {
        EAccountFlags::ALL
            .iter()
            .copied()
            .find(|flag| bits & (flag.value() as u32) != 0)
            .unwrap_or(EAccountFlags::NormalUser)
    })
}

/// A member of a chat room.
#[derive(Clone, Debug)]
pub struct ChatMemberInfo {
    message_object: MessageObject,
}

impl ChatMemberInfo {
    /// Creates an empty chat member info.
    pub(crate) fn new() -> Self {
        Self {
            message_object: MessageObject::new(),
        }
    }

    /// Creates a chat member info from an existing key value set.
    pub(crate) fn from_key_values(key_values: KeyValue) -> Self {
        Self {
            message_object: MessageObject::with_key_values(key_values),
        }
    }

    /// Fills this in from the payload. Returns `false` if it couldn't be read.
    pub(crate) fn read_from_stream(&mut self, payload: &mut MsgBase) -> bool {
        let start = (payload.position as usize).min(payload.payload.len());

        let (result, consumed) = {
            let mut cursor = std::io::Cursor::new(&payload.payload[start..]);
            let result = self.message_object.read_from_stream(&mut cursor);
            (result, cursor.position())
        };

        payload.position = start as u64 + consumed;

        result
    }

    /// The key values backing this member info.
    pub fn key_values(&self) -> &KeyValue {
        self.message_object.key_values()
    }

    /// The clan permission details of this chat member.
    pub fn details(&self) -> EClanPermission {
        key_value_as_enum(
            self.key_values().get("Details"),
            CLAN_PERMISSION_MEMBERS,
            |member| member.value(),
        )
        .unwrap_or(EClanPermission::Nobody)
    }

    /// The permissions this user has with the chatroom.
    pub fn permissions(&self) -> EChatPermission {
        key_value_as_enum(
            self.key_values().get("Permissions"),
            CHAT_PERMISSION_MEMBERS,
            |member| member.value(),
        )
        .unwrap_or(EChatPermission::Close)
    }

    /// The [`SteamID`] of this user.
    pub fn steam_id(&self) -> SteamID {
        SteamID::from_id(self.key_values().get("SteamID").as_unsigned_long(0))
    }
}

/// Posted when someone's friend details (name, status, game, ...) change.
#[derive(Clone, Debug)]
pub struct PersonaStateCallback {
    job_id: JobID,
    status_flags: EClientPersonaStateFlag,
    friend_id: SteamID,
    state: EPersonaState,
    state_flags: EPersonaStateFlag,
    game_app_id: u32,
    game_id: GameID,
    game_name: String,
    game_server_ip: IpAddr,
    game_server_port: u32,
    query_port: u32,
    source_steam_id: SteamID,
    game_data_blob: Option<Vec<u8>>,
    name: String,
    avatar_hash: Option<Vec<u8>>,
    last_log_off: DateTime<Utc>,
    last_log_on: DateTime<Utc>,
    clan_rank: u32,
    clan_tag: String,
    online_session_instances: u32,
}

impl PersonaStateCallback {
    /// Builds the callback from a persona state entry sent by the server.
    pub(crate) fn new(
        friend: &c_msg_client_persona_state::Friend,
        status_flags: EClientPersonaStateFlag,
    ) -> Self {
        Self {
            job_id: JobID::new(),
            status_flags,
            friend_id: SteamID::from_id(friend.friendid.unwrap_or_default()),
            state: EPersonaState::from_value(friend.persona_state.unwrap_or_default() as i32)
                .unwrap_or(EPersonaState::Offline),
            state_flags: persona_state_flag_from_bits(
                friend.persona_state_flags.unwrap_or_default(),
            ),
            game_app_id: friend.game_played_app_id.unwrap_or_default(),
            game_id: GameID::from_value(friend.gameid.unwrap_or_default()),
            game_name: friend.game_name.clone().unwrap_or_default(),
            game_server_ip: ip_address_from_msg(&CMsgIpAddress {
                ip: Some(c_msg_ip_address::Ip::V4(
                    friend.game_server_ip.unwrap_or_default(),
                )),
            })
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            game_server_port: friend.game_server_port.unwrap_or_default(),
            query_port: friend.query_port.unwrap_or_default(),
            source_steam_id: SteamID::from_id(friend.steamid_source.unwrap_or_default()),
            game_data_blob: friend.game_data_blob.clone(),
            name: friend.player_name.clone().unwrap_or_default(),
            avatar_hash: friend.avatar_hash.clone(),
            last_log_off: date_time_from_unix_time(u64::from(
                friend.last_logoff.unwrap_or_default(),
            )),
            last_log_on: date_time_from_unix_time(u64::from(friend.last_logon.unwrap_or_default())),
            clan_rank: friend.clan_rank.unwrap_or_default(),
            clan_tag: friend.clan_tag.clone().unwrap_or_default(),
            online_session_instances: friend.online_session_instances.unwrap_or_default(),
        }
    }

    /// What changed.
    pub fn status_flags(&self) -> EClientPersonaStateFlag {
        self.status_flags
    }

    /// The friend ID.
    pub fn friend_id(&self) -> SteamID {
        self.friend_id
    }

    /// The state.
    pub fn state(&self) -> EPersonaState {
        self.state
    }

    /// The state flags.
    pub fn state_flags(&self) -> EPersonaStateFlag {
        self.state_flags
    }

    /// The game app ID.
    pub fn game_app_id(&self) -> u32 {
        self.game_app_id
    }

    /// The game ID.
    pub fn game_id(&self) -> GameID {
        self.game_id
    }

    /// The name of the game.
    pub fn game_name(&self) -> &str {
        &self.game_name
    }

    /// The game server IP.
    pub fn game_server_ip(&self) -> IpAddr {
        self.game_server_ip
    }

    /// The game server port.
    pub fn game_server_port(&self) -> u32 {
        self.game_server_port
    }

    /// The query port.
    pub fn query_port(&self) -> u32 {
        self.query_port
    }

    /// The source steam ID.
    pub fn source_steam_id(&self) -> SteamID {
        self.source_steam_id
    }

    /// The game data blob.
    pub fn game_data_blob(&self) -> Option<&[u8]> {
        self.game_data_blob.as_deref()
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The avatar hash.
    pub fn avatar_hash(&self) -> Option<&[u8]> {
        self.avatar_hash.as_deref()
    }

    /// The last log off.
    pub fn last_log_off(&self) -> DateTime<Utc> {
        self.last_log_off
    }

    /// The last log on.
    pub fn last_log_on(&self) -> DateTime<Utc> {
        self.last_log_on
    }

    /// The clan rank.
    pub fn clan_rank(&self) -> u32 {
        self.clan_rank
    }

    /// The clan tag.
    pub fn clan_tag(&self) -> &str {
        &self.clan_tag
    }

    /// The online session instances.
    pub fn online_session_instances(&self) -> u32 {
        self.online_session_instances
    }
}

impl CallbackMsg for PersonaStateCallback {
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

/// Posted when a clan's state changes.
#[derive(Clone, Debug)]
pub struct ClanStateCallback {
    job_id: JobID,
    clan_id: SteamID,
    account_flags: EAccountFlags,
    chat_room_private: bool,
    clan_name: Option<String>,
    avatar_hash: Option<Vec<u8>>,
    member_total_count: u32,
    member_online_count: u32,
    member_chatting_count: u32,
    member_in_game_count: u32,
    events: Vec<ClanEvent>,
    announcements: Vec<ClanEvent>,
}

/// An event or announcement posted by a clan.
#[derive(Clone, Debug)]
pub struct ClanEvent {
    id: GlobalID,
    event_time: DateTime<Utc>,
    headline: String,
    game_id: GameID,
    just_posted: bool,
}

impl ClanEvent {
    /// Builds the event from its protobuf form.
    fn new(clan_event: &c_msg_client_clan_state::Event) -> Self {
        Self {
            id: GlobalID::from_value(clan_event.gid.unwrap_or_default()),
            event_time: date_time_from_unix_time(u64::from(
                clan_event.event_time.unwrap_or_default(),
            )),
            headline: clan_event.headline.clone().unwrap_or_default(),
            game_id: GameID::from_value(clan_event.game_id.unwrap_or_default()),
            just_posted: clan_event.just_posted.unwrap_or_default(),
        }
    }

    /// The globally unique ID for this specific event.
    pub fn id(&self) -> GlobalID {
        self.id
    }

    /// The event time.
    pub fn event_time(&self) -> DateTime<Utc> {
        self.event_time
    }

    /// The headline of the event.
    pub fn headline(&self) -> &str {
        &self.headline
    }

    /// The [`GameID`] associated with this event, if any.
    pub fn game_id(&self) -> GameID {
        self.game_id
    }

    /// Whether this event was just posted.
    pub fn just_posted(&self) -> bool {
        self.just_posted
    }
}

impl ClanStateCallback {
    /// Builds the callback from a received `ClientClanState` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let clan_state = ClientMsgProtobuf::<CMsgClientClanState>::from_packet(packet_msg)?;
        let msg = &clan_state.body;

        let (clan_name, avatar_hash) = match &msg.name_info {
            Some(name_info) => (name_info.clan_name.clone(), name_info.sha_avatar.clone()),
            None => (None, None),
        };

        let (member_total_count, member_online_count, member_chatting_count, member_in_game_count) =
            match &msg.user_counts {
                Some(counts) => (
                    counts.members.unwrap_or_default(),
                    counts.online.unwrap_or_default(),
                    counts.chatting.unwrap_or_default(),
                    counts.in_game.unwrap_or_default(),
                ),
                None => (0, 0, 0, 0),
            };

        Ok(Self {
            job_id: JobID::new(),
            clan_id: SteamID::from_id(msg.steamid_clan.unwrap_or_default()),
            account_flags: account_flags_from_bits(msg.clan_account_flags.unwrap_or_default()),
            chat_room_private: msg.chat_room_private.unwrap_or_default(),
            clan_name,
            avatar_hash,
            member_total_count,
            member_online_count,
            member_chatting_count,
            member_in_game_count,
            events: msg.events.iter().map(ClanEvent::new).collect(),
            announcements: msg.announcements.iter().map(ClanEvent::new).collect(),
        })
    }

    /// The [`SteamID`] of the clan that posted this state update.
    pub fn clan_id(&self) -> SteamID {
        self.clan_id
    }

    /// The account flags.
    pub fn account_flags(&self) -> EAccountFlags {
        self.account_flags
    }

    /// The privacy of the chat room.
    pub fn chat_room_private(&self) -> bool {
        self.chat_room_private
    }

    /// The name of the clan.
    pub fn clan_name(&self) -> Option<&str> {
        self.clan_name.as_deref()
    }

    /// The SHA-1 avatar hash.
    pub fn avatar_hash(&self) -> Option<&[u8]> {
        self.avatar_hash.as_deref()
    }

    /// The total number of members in this clan.
    pub fn member_total_count(&self) -> u32 {
        self.member_total_count
    }

    /// The number of members in this clan that are currently online.
    pub fn member_online_count(&self) -> u32 {
        self.member_online_count
    }

    /// The number of members in this clan that are currently chatting.
    pub fn member_chatting_count(&self) -> u32 {
        self.member_chatting_count
    }

    /// The number of members in this clan that are currently in-game.
    pub fn member_in_game_count(&self) -> u32 {
        self.member_in_game_count
    }

    /// Any events associated with this clan state update.
    pub fn events(&self) -> &[ClanEvent] {
        &self.events
    }

    /// Any announcements associated with this clan state update.
    pub fn announcements(&self) -> &[ClanEvent] {
        &self.announcements
    }
}

impl CallbackMsg for ClanStateCallback {
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

/// Posted when the client receives its friends list.
#[derive(Clone, Debug)]
pub struct FriendsListCallback {
    job_id: JobID,
    incremental: bool,
    friend_list: Vec<Friend>,
}

/// A single entry in the friends list.
#[derive(Clone, Debug)]
pub struct Friend {
    steam_id: SteamID,
    relationship: EFriendRelationship,
}

impl Friend {
    /// Builds the entry from its protobuf form.
    fn new(friend: &c_msg_client_friends_list::Friend) -> Self {
        Self {
            steam_id: SteamID::from_id(friend.ulfriendid.unwrap_or_default()),
            relationship: EFriendRelationship::from_value(
                friend.efriendrelationship.unwrap_or_default() as i32,
            )
            .unwrap_or(EFriendRelationship::None),
        }
    }

    /// The SteamID of the friend.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The relationship to this friend.
    pub fn relationship(&self) -> EFriendRelationship {
        self.relationship
    }
}

impl FriendsListCallback {
    /// Builds the callback from a received `ClientFriendsList` message.
    pub(crate) fn from_body(msg: &CMsgClientFriendsList) -> Self {
        Self {
            job_id: JobID::new(),
            incremental: msg.bincremental.unwrap_or_default(),
            friend_list: msg.friends.iter().map(Friend::new).collect(),
        }
    }

    /// Whether this is an incremental update (only the changes) rather than
    /// the full list.
    pub fn incremental(&self) -> bool {
        self.incremental
    }

    /// The friend list.
    pub fn friend_list(&self) -> &[Friend] {
        &self.friend_list
    }
}

impl CallbackMsg for FriendsListCallback {
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

/// Posted when a message from a friend arrives.
#[derive(Clone, Debug)]
pub struct FriendMsgCallback {
    job_id: JobID,
    sender: SteamID,
    entry_type: EChatEntryType,
    from_limited_account: bool,
    message: Option<String>,
}

impl FriendMsgCallback {
    /// Builds the callback from a received `ClientFriendMsgIncoming` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let friend_msg = ClientMsgProtobuf::<CMsgClientFriendMsgIncoming>::from_packet(packet_msg)?;
        let msg = &friend_msg.body;

        Ok(Self {
            job_id: JobID::new(),
            sender: SteamID::from_id(msg.steamid_from.unwrap_or_default()),
            entry_type: EChatEntryType::from_value(msg.chat_entry_type.unwrap_or_default())
                .unwrap_or(EChatEntryType::Invalid),
            from_limited_account: msg.from_limited_account.unwrap_or_default(),
            message: msg.message.as_ref().map(|data| {
                String::from_utf8_lossy(data)
                    .trim_end_matches('\0')
                    .to_string()
            }),
        })
    }

    /// The sender.
    pub fn sender(&self) -> SteamID {
        self.sender
    }

    /// The chat entry type.
    pub fn entry_type(&self) -> EChatEntryType {
        self.entry_type
    }

    /// Whether the message is from a limited account.
    pub fn from_limited_account(&self) -> bool {
        self.from_limited_account
    }

    /// The message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl CallbackMsg for FriendMsgCallback {
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

/// Posted when a message we sent from another session is echoed back to us.
#[derive(Clone, Debug)]
pub struct FriendMsgEchoCallback {
    job_id: JobID,
    recipient: SteamID,
    entry_type: EChatEntryType,
    from_limited_account: bool,
    message: Option<String>,
}

impl FriendMsgEchoCallback {
    /// Builds the callback from a received `ClientFriendMsgIncoming` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let friend_echo_msg =
            ClientMsgProtobuf::<CMsgClientFriendMsgIncoming>::from_packet(packet_msg)?;
        let msg = &friend_echo_msg.body;

        Ok(Self {
            job_id: JobID::new(),
            recipient: SteamID::from_id(msg.steamid_from.unwrap_or_default()),
            entry_type: EChatEntryType::from_value(msg.chat_entry_type.unwrap_or_default())
                .unwrap_or(EChatEntryType::Invalid),
            from_limited_account: msg.from_limited_account.unwrap_or_default(),
            message: msg.message.as_ref().map(|data| {
                String::from_utf8_lossy(data)
                    .trim_end_matches('\0')
                    .to_string()
            }),
        })
    }

    /// The recipient.
    pub fn recipient(&self) -> SteamID {
        self.recipient
    }

    /// The chat entry type.
    pub fn entry_type(&self) -> EChatEntryType {
        self.entry_type
    }

    /// Whether the message is from a limited account.
    pub fn from_limited_account(&self) -> bool {
        self.from_limited_account
    }

    /// The message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl CallbackMsg for FriendMsgEchoCallback {
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

/// Posted with the message history of a friend.
#[derive(Clone, Debug)]
pub struct FriendMsgHistoryCallback {
    job_id: JobID,
    result: EResult,
    steam_id: SteamID,
    messages: Vec<FriendMessage>,
}

/// A single message sent to or received from a friend.
#[derive(Clone, Debug)]
pub struct FriendMessage {
    steam_id: SteamID,
    unread: bool,
    message: String,
    timestamp: DateTime<Utc>,
}

impl FriendMessage {
    /// The SteamID of the user that wrote the message.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// Whether the message hasn't been read yet, i.e. it's an offline message.
    pub fn unread(&self) -> bool {
        self.unread
    }

    /// The actual message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The time (in UTC) when the message was sent.
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }
}

impl FriendMsgHistoryCallback {
    /// Builds the callback from a `ClientChatGetFriendMessageHistoryResponse`.
    pub(crate) fn new(
        msg: &CMsgClientChatGetFriendMessageHistoryResponse,
        universe: EUniverse,
    ) -> Self {
        let messages = msg
            .messages
            .iter()
            .map(|m| {
                let sender_id = SteamID::from_parts(
                    EAccountType::Individual,
                    universe,
                    m.accountid.unwrap_or_default(),
                );
                let timestamp =
                    date_time_from_unix_time(u64::from(m.timestamp.unwrap_or_default()));

                FriendMessage {
                    steam_id: sender_id,
                    unread: m.unread.unwrap_or_default(),
                    message: m.message.clone().unwrap_or_default(),
                    timestamp,
                }
            })
            .collect();

        Self {
            job_id: JobID::new(),
            result: EResult::from_value(msg.success.unwrap_or_default() as i32)
                .unwrap_or(EResult::Invalid),
            steam_id: SteamID::from_id(msg.steamid.unwrap_or_default()),
            messages,
        }
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The SteamID of the user with whom these messages were exchanged.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The messages exchanged with the user. Offline messages have
    /// [`FriendMessage::unread`] set.
    pub fn messages(&self) -> &[FriendMessage] {
        &self.messages
    }
}

impl CallbackMsg for FriendMsgHistoryCallback {
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

/// Posted in response to adding a user to the friends list.
#[derive(Clone, Debug)]
pub struct FriendAddedCallback {
    job_id: JobID,
    result: EResult,
    steam_id: SteamID,
    persona_name: String,
}

impl FriendAddedCallback {
    /// Builds the callback from a received `ClientAddFriendResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let friend_response =
            ClientMsgProtobuf::<CMsgClientAddFriendResponse>::from_packet(packet_msg)?;
        let msg = &friend_response.body;

        Ok(Self {
            job_id: JobID::new(),
            result: EResult::from_value(msg.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            steam_id: SteamID::from_id(msg.steam_id_added.unwrap_or_default()),
            persona_name: msg.persona_name_added.clone().unwrap_or_default(),
        })
    }

    /// The result of the request.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The SteamID of the friend that was added.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The persona name of the friend.
    pub fn persona_name(&self) -> &str {
        &self.persona_name
    }
}

impl CallbackMsg for FriendAddedCallback {
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

/// Posted in response to trying to join a chat.
#[derive(Clone, Debug)]
pub struct ChatEnterCallback {
    job_id: JobID,
    chat_id: SteamID,
    friend_id: SteamID,
    chat_room_type: EChatRoomType,
    owner_id: SteamID,
    clan_id: SteamID,
    chat_flags: u8,
    enter_response: EChatRoomEnterResponse,
    num_chat_members: i32,
    chat_room_name: String,
    chat_members: Option<Vec<ChatMemberInfo>>,
}

impl ChatEnterCallback {
    /// Builds the callback from a received `ClientChatEnter` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let chat_enter = ClientMsg::<MsgClientChatEnter>::from_packet(packet_msg)?;
        let (
            steam_id_chat,
            steam_id_friend,
            chat_room_type,
            steam_id_owner,
            steam_id_clan,
            chat_flags,
            enter_response,
            num_members,
        ) = {
            let msg = &chat_enter.body;
            (
                msg.steam_id_chat,
                msg.steam_id_friend,
                msg.chat_room_type,
                msg.steam_id_owner,
                msg.steam_id_clan,
                msg.chat_flags,
                msg.enter_response,
                msg.num_members,
            )
        };

        let mut payload = chat_enter.payload;

        // The Steam client always reads the chat room name, whatever the
        // enter response is.
        let chat_room_name = payload.read_null_term_string()?;

        let mut chat_members = None;
        if enter_response == EChatRoomEnterResponse::Success {
            // The rest of the payload is only there if entering succeeded.
            let mut members = Vec::new();
            for _ in 0..num_members {
                let mut member_info = ChatMemberInfo::new();
                member_info.read_from_stream(&mut payload);
                members.push(member_info);
            }
            chat_members = Some(members);
        }

        Ok(Self {
            job_id: JobID::new(),
            chat_id: SteamID::from_id(steam_id_chat),
            friend_id: SteamID::from_id(steam_id_friend),
            chat_room_type,
            owner_id: SteamID::from_id(steam_id_owner),
            clan_id: SteamID::from_id(steam_id_clan),
            chat_flags,
            enter_response,
            num_chat_members: num_members,
            chat_room_name,
            chat_members,
        })
    }

    /// The SteamID of the chat room.
    pub fn chat_id(&self) -> SteamID {
        self.chat_id
    }

    /// The friend ID.
    pub fn friend_id(&self) -> SteamID {
        self.friend_id
    }

    /// The type of the chat room.
    pub fn chat_room_type(&self) -> EChatRoomType {
        self.chat_room_type
    }

    /// The SteamID of the chat room owner.
    pub fn owner_id(&self) -> SteamID {
        self.owner_id
    }

    /// The SteamID of the clan that owns this chat room.
    pub fn clan_id(&self) -> SteamID {
        self.clan_id
    }

    /// The chat flags.
    pub fn chat_flags(&self) -> u8 {
        self.chat_flags
    }

    /// The chat enter response.
    pub fn enter_response(&self) -> EChatRoomEnterResponse {
        self.enter_response
    }

    /// The number of users currently in this chat room.
    pub fn num_chat_members(&self) -> i32 {
        self.num_chat_members
    }

    /// The name of the chat room.
    pub fn chat_room_name(&self) -> &str {
        &self.chat_room_name
    }

    /// The members of the chat room.
    pub fn chat_members(&self) -> Option<&[ChatMemberInfo]> {
        self.chat_members.as_deref()
    }
}

impl CallbackMsg for ChatEnterCallback {
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

/// Posted when a chat room message arrives.
#[derive(Clone, Debug)]
pub struct ChatMsgCallback {
    job_id: JobID,
    chatter_id: SteamID,
    chat_room_id: SteamID,
    chat_msg_type: EChatEntryType,
    message: String,
}

impl ChatMsgCallback {
    /// Builds the callback from a received `ClientChatMsg` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let chat_msg = ClientMsg::<MsgClientChatMsg>::from_packet(packet_msg)?;
        let msg = &chat_msg.body;

        let message = String::from_utf8_lossy(&chat_msg.payload.payload)
            .trim_end_matches('\0')
            .to_string();

        Ok(Self {
            job_id: JobID::new(),
            chatter_id: SteamID::from_id(msg.steam_id_chatter),
            chat_room_id: SteamID::from_id(msg.steam_id_chat_room),
            chat_msg_type: msg.chat_msg_type,
            message,
        })
    }

    /// The SteamID of the chatter.
    pub fn chatter_id(&self) -> SteamID {
        self.chatter_id
    }

    /// The SteamID of the chat room.
    pub fn chat_room_id(&self) -> SteamID {
        self.chat_room_id
    }

    /// The chat entry type.
    pub fn chat_msg_type(&self) -> EChatEntryType {
        self.chat_msg_type
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl CallbackMsg for ChatMsgCallback {
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

/// Posted when chat member info arrives.
#[derive(Clone, Debug)]
pub struct ChatMemberInfoCallback {
    job_id: JobID,
    chat_room_id: SteamID,
    info_type: EChatInfoType,
    state_change_info: Option<StateChangeDetails>,
}

/// What changed about a chat member.
#[derive(Clone, Debug)]
pub struct StateChangeDetails {
    chatter_acted_on: SteamID,
    state_change: EChatMemberStateChange,
    chatter_acted_by: SteamID,
    member_info: Option<ChatMemberInfo>,
}

impl StateChangeDetails {
    /// Reads the state change from the payload.
    fn from_payload(payload: &mut MsgBase) -> SteamResult<Self> {
        let chatter_acted_on = SteamID::from_id(payload.read_u64()?);
        let state_change = EChatMemberStateChange::from_value(payload.read_i32()?)
            .unwrap_or(EChatMemberStateChange::Entered);
        let chatter_acted_by = SteamID::from_id(payload.read_u64()?);

        let member_info = if state_change == EChatMemberStateChange::Entered {
            let mut member_info = ChatMemberInfo::new();
            member_info.read_from_stream(payload);
            Some(member_info)
        } else {
            None
        };

        Ok(Self {
            chatter_acted_on,
            state_change,
            chatter_acted_by,
            member_info,
        })
    }

    /// The SteamID of the chatter that was acted on.
    pub fn chatter_acted_on(&self) -> SteamID {
        self.chatter_acted_on
    }

    /// The state change for the acted on SteamID.
    pub fn state_change(&self) -> EChatMemberStateChange {
        self.state_change
    }

    /// The SteamID of whoever acted on [`StateChangeDetails::chatter_acted_on`].
    pub fn chatter_acted_by(&self) -> SteamID {
        self.chatter_acted_by
    }

    /// The member info of a user who joined the room. Only set when
    /// [`StateChangeDetails::state_change`] is [`EChatMemberStateChange::Entered`].
    pub fn member_info(&self) -> Option<&ChatMemberInfo> {
        self.member_info.as_ref()
    }
}

impl ChatMemberInfoCallback {
    /// Builds the callback from a received `ClientChatMemberInfo` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let memb_info = ClientMsg::<MsgClientChatMemberInfo>::from_packet(packet_msg)?;
        let (steam_id_chat, info_type) = {
            let msg = &memb_info.body;
            (msg.steam_id_chat, msg.r#type)
        };

        let state_change_info = match info_type {
            EChatInfoType::StateChange => {
                let mut payload = memb_info.payload;
                Some(StateChangeDetails::from_payload(&mut payload)?)
            }
            _ => None,
        };

        Ok(Self {
            job_id: JobID::new(),
            chat_room_id: SteamID::from_id(steam_id_chat),
            info_type,
            state_change_info,
        })
    }

    /// The SteamID of the chat room.
    pub fn chat_room_id(&self) -> SteamID {
        self.chat_room_id
    }

    /// The info type.
    pub fn info_type(&self) -> EChatInfoType {
        self.info_type
    }

    /// The state change, for [`EChatInfoType::StateChange`] updates.
    pub fn state_change_info(&self) -> Option<&StateChangeDetails> {
        self.state_change_info.as_ref()
    }
}

impl CallbackMsg for ChatMemberInfoCallback {
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

/// Posted when chat room info arrives.
#[derive(Clone, Debug)]
pub struct ChatRoomInfoCallback {
    job_id: JobID,
    chat_room_id: SteamID,
    info_type: EChatInfoType,
}

impl ChatRoomInfoCallback {
    /// Builds the callback from a received `ClientChatRoomInfo` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let room_info = ClientMsg::<MsgClientChatRoomInfo>::from_packet(packet_msg)?;
        let msg = &room_info.body;

        Ok(Self {
            job_id: JobID::new(),
            chat_room_id: SteamID::from_id(msg.steam_id_chat),
            info_type: msg.r#type,
        })
    }

    /// The SteamID of the chat room.
    pub fn chat_room_id(&self) -> SteamID {
        self.chat_room_id
    }

    /// The info type.
    pub fn info_type(&self) -> EChatInfoType {
        self.info_type
    }
}

impl CallbackMsg for ChatRoomInfoCallback {
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

/// Posted when a chat action has completed.
#[derive(Clone, Debug)]
pub struct ChatActionResultCallback {
    job_id: JobID,
    chat_room_id: SteamID,
    chatter_id: SteamID,
    action: EChatAction,
    result: EChatActionResult,
}

impl ChatActionResultCallback {
    /// Builds the callback from a received `ClientChatActionResult` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let action_result = ClientMsg::<MsgClientChatActionResult>::from_packet(packet_msg)?;
        let result = &action_result.body;

        Ok(Self {
            job_id: JobID::new(),
            chat_room_id: SteamID::from_id(result.steam_id_chat),
            chatter_id: SteamID::from_id(result.steam_id_user_acted_on),
            action: result.chat_action,
            result: result.action_result,
        })
    }

    /// The SteamID of the chat room the action was performed in.
    pub fn chat_room_id(&self) -> SteamID {
        self.chat_room_id
    }

    /// The SteamID of the chat member the action was performed on.
    pub fn chatter_id(&self) -> SteamID {
        self.chatter_id
    }

    /// The chat action that was performed.
    pub fn action(&self) -> EChatAction {
        self.action
    }

    /// The result of the chat action.
    pub fn result(&self) -> EChatActionResult {
        self.result
    }
}

impl CallbackMsg for ChatActionResultCallback {
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

/// Posted when a chat invite arrives.
#[derive(Clone, Debug)]
pub struct ChatInviteCallback {
    job_id: JobID,
    invited_id: SteamID,
    chat_room_id: SteamID,
    patron_id: SteamID,
    chat_room_type: EChatRoomType,
    friend_chat_id: SteamID,
    chat_room_name: String,
    game_id: GameID,
}

impl ChatInviteCallback {
    /// Builds the callback from a received `ClientChatInvite` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let chat_invite = ClientMsgProtobuf::<CMsgClientChatInvite>::from_packet(packet_msg)?;
        let invite = &chat_invite.body;

        Ok(Self {
            job_id: JobID::new(),
            invited_id: SteamID::from_id(invite.steam_id_invited.unwrap_or_default()),
            chat_room_id: SteamID::from_id(invite.steam_id_chat.unwrap_or_default()),
            patron_id: SteamID::from_id(invite.steam_id_patron.unwrap_or_default()),
            chat_room_type: EChatRoomType::from_value(invite.chatroom_type.unwrap_or_default())
                .unwrap_or(EChatRoomType::Friend),
            friend_chat_id: SteamID::from_id(invite.steam_id_friend_chat.unwrap_or_default()),
            chat_room_name: invite.chat_name.clone().unwrap_or_default(),
            game_id: GameID::from_value(invite.game_id.unwrap_or_default()),
        })
    }

    /// The SteamID of the user who was invited to the chat.
    pub fn invited_id(&self) -> SteamID {
        self.invited_id
    }

    /// The chat room SteamID.
    pub fn chat_room_id(&self) -> SteamID {
        self.chat_room_id
    }

    /// The SteamID of the user who performed the invitation.
    pub fn patron_id(&self) -> SteamID {
        self.patron_id
    }

    /// The chat room type.
    pub fn chat_room_type(&self) -> EChatRoomType {
        self.chat_room_type
    }

    /// The SteamID of the chat friend.
    pub fn friend_chat_id(&self) -> SteamID {
        self.friend_chat_id
    }

    /// The name of the chat room.
    pub fn chat_room_name(&self) -> &str {
        &self.chat_room_name
    }

    /// The GameID associated with this chat room, if it's a game lobby.
    pub fn game_id(&self) -> GameID {
        self.game_id
    }
}

impl CallbackMsg for ChatInviteCallback {
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

/// Posted in response to ignoring (or unignoring) a friend.
#[derive(Clone, Debug)]
pub struct IgnoreFriendCallback {
    job_id: JobID,
    result: EResult,
}

impl IgnoreFriendCallback {
    /// Builds the callback from a received `ClientSetIgnoreFriendResponse` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response = ClientMsg::<MsgClientSetIgnoreFriendResponse>::from_packet(packet_msg)?;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: response.body.result,
        })
    }

    /// The result of ignoring a friend.
    pub fn result(&self) -> EResult {
        self.result
    }
}

impl CallbackMsg for IgnoreFriendCallback {
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

/// Posted in response to requesting a user's profile info.
#[derive(Clone, Debug)]
pub struct ProfileInfoCallback {
    job_id: JobID,
    result: EResult,
    steam_id: SteamID,
    time_created: DateTime<Utc>,
    real_name: String,
    city_name: String,
    state_name: String,
    country_name: String,
    headline: String,
    summary: String,
}

impl ProfileInfoCallback {
    /// Builds the callback from a `ClientFriendProfileInfoResponse`.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let response_msg =
            ClientMsgProtobuf::<CMsgClientFriendProfileInfoResponse>::from_packet(packet_msg)?;
        let response = &response_msg.body;

        Ok(Self {
            job_id: packet_msg.target_job_id(),
            result: EResult::from_value(response.eresult.unwrap_or_default())
                .unwrap_or(EResult::Invalid),
            steam_id: SteamID::from_id(response.steamid_friend.unwrap_or_default()),
            time_created: date_time_from_unix_time(u64::from(
                response.time_created.unwrap_or_default(),
            )),
            real_name: response.real_name.clone().unwrap_or_default(),
            city_name: response.city_name.clone().unwrap_or_default(),
            state_name: response.state_name.clone().unwrap_or_default(),
            country_name: response.country_name.clone().unwrap_or_default(),
            headline: response.headline.clone().unwrap_or_default(),
            summary: response.summary.clone().unwrap_or_default(),
        })
    }

    /// The result of requesting profile info.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// The [`SteamID`] this info belongs to.
    pub fn steam_id(&self) -> SteamID {
        self.steam_id
    }

    /// The time this account was created.
    pub fn time_created(&self) -> DateTime<Utc> {
        self.time_created
    }

    /// The real name.
    pub fn real_name(&self) -> &str {
        &self.real_name
    }

    /// The name of the city.
    pub fn city_name(&self) -> &str {
        &self.city_name
    }

    /// The name of the state.
    pub fn state_name(&self) -> &str {
        &self.state_name
    }

    /// The name of the country.
    pub fn country_name(&self) -> &str {
        &self.country_name
    }

    /// The headline.
    pub fn headline(&self) -> &str {
        &self.headline
    }

    /// The summary.
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

impl CallbackMsg for ProfileInfoCallback {
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

/// Posted in response to
/// [`SteamFriends::set_persona_name`](crate::steam::handlers::steam_friends::steam_friends::SteamFriends::set_persona_name)
/// or [`SteamFriends::set_persona_state`](crate::steam::handlers::steam_friends::steam_friends::SteamFriends::set_persona_state).
#[derive(Clone, Debug)]
pub struct PersonaChangeCallback {
    job_id: JobID,
    result: EResult,
    name: String,
}

impl PersonaChangeCallback {
    /// Builds the callback from a received `ClientPersonaChangeResponse` message.
    pub(crate) fn new(job_id: JobID, msg: &CMsgPersonaChangeResponse) -> Self {
        Self {
            job_id,
            result: EResult::from_value(msg.result.unwrap_or_default() as i32)
                .unwrap_or(EResult::Invalid),
            name: msg.player_name.clone().unwrap_or_default(),
        }
    }

    /// The result of changing this client's persona information.
    pub fn result(&self) -> EResult {
        self.result
    }

    /// Our name, according to Steam.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl CallbackMsg for PersonaChangeCallback {
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
