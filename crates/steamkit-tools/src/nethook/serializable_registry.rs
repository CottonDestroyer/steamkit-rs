//! Maps message numbers to the non-protobuf wire structs in
//! `steamkit::base::generated::steam_language_internal`.

use steamkit::base::generated::steam_language::EMsg;
use steamkit::base::generated::steam_language_internal::SteamSerializableDeserialize;
use steamkit::base::msg_base::Reader;
use steamkit::error::SteamError;

use crate::nethook::body::SerializableBody;

/// The decoder of a message of the SteamKit wire format that is not protobuf
/// backed.
pub type SerializableDecoder =
    fn(&mut Reader<'_>) -> Result<Box<dyn SerializableBody>, SteamError>;

macro_rules! serializable_types {
    ($($fn_name:ident => $ty:ty => $name:literal => $emsg:ident),* $(,)?) => {
        $(
            /// Decodes the message from the reader.
            fn $fn_name(reader: &mut Reader<'_>) -> Result<Box<dyn SerializableBody>, SteamError> {
                Ok(Box::new(<$ty>::read(reader)?))
            }
        )*

        /// The body type of a message number that is not protobuf backed.
        pub fn non_protobuf_message_body_type(emsg: EMsg) -> Option<(&'static str, SerializableDecoder)> {
            match emsg {
                $(EMsg::$emsg => Some(($name, $fn_name)),)*
                _ => None,
            }
        }
    };
}

serializable_types! {
    decode_msg_client_just_strings => steamkit::base::generated::steam_language::internal::MsgClientJustStrings => "MsgClientJustStrings" => Invalid,
    decode_msg_channel_encrypt_request => steamkit::base::generated::steam_language::internal::MsgChannelEncryptRequest => "MsgChannelEncryptRequest" => ChannelEncryptRequest,
    decode_msg_channel_encrypt_response => steamkit::base::generated::steam_language::internal::MsgChannelEncryptResponse => "MsgChannelEncryptResponse" => ChannelEncryptResponse,
    decode_msg_channel_encrypt_result => steamkit::base::generated::steam_language::internal::MsgChannelEncryptResult => "MsgChannelEncryptResult" => ChannelEncryptResult,
    decode_msg_client_logon => steamkit::base::generated::steam_language::internal::MsgClientLogon => "MsgClientLogon" => ClientLogon,
    decode_msg_client_vac_ban_status => steamkit::base::generated::steam_language::internal::MsgClientVACBanStatus => "MsgClientVACBanStatus" => ClientVACBanStatus,
    decode_msg_client_app_usage_event => steamkit::base::generated::steam_language::internal::MsgClientAppUsageEvent => "MsgClientAppUsageEvent" => ClientAppUsageEvent,
    decode_msg_client_update_guest_passes_list => steamkit::base::generated::steam_language::internal::MsgClientUpdateGuestPassesList => "MsgClientUpdateGuestPassesList" => ClientUpdateGuestPassesList,
    decode_msg_client_p2p_introducer_message => steamkit::base::generated::steam_language::internal::MsgClientP2PIntroducerMessage => "MsgClientP2PIntroducerMessage" => ClientP2PIntroducerMessage,
    decode_msg_client_ogs_begin_session => steamkit::base::generated::steam_language::internal::MsgClientOGSBeginSession => "MsgClientOGSBeginSession" => ClientOGSBeginSession,
    decode_msg_client_ogs_begin_session_response => steamkit::base::generated::steam_language::internal::MsgClientOGSBeginSessionResponse => "MsgClientOGSBeginSessionResponse" => ClientOGSBeginSessionResponse,
    decode_msg_client_ogs_end_session => steamkit::base::generated::steam_language::internal::MsgClientOGSEndSession => "MsgClientOGSEndSession" => ClientOGSEndSession,
    decode_msg_client_ogs_end_session_response => steamkit::base::generated::steam_language::internal::MsgClientOGSEndSessionResponse => "MsgClientOGSEndSessionResponse" => ClientOGSEndSessionResponse,
    decode_msg_client_ogs_write_row => steamkit::base::generated::steam_language::internal::MsgClientOGSWriteRow => "MsgClientOGSWriteRow" => ClientOGSWriteRow,
    decode_msg_client_get_friends_who_play_game => steamkit::base::generated::steam_language::internal::MsgClientGetFriendsWhoPlayGame => "MsgClientGetFriendsWhoPlayGame" => ClientGetFriendsWhoPlayGame,
    decode_msg_client_get_friends_who_play_game_response => steamkit::base::generated::steam_language::internal::MsgClientGetFriendsWhoPlayGameResponse => "MsgClientGetFriendsWhoPlayGameResponse" => ClientGetFriendsWhoPlayGameResponse,
    decode_msg_gs_perform_hardware_survey => steamkit::base::generated::steam_language::internal::MsgGSPerformHardwareSurvey => "MsgGSPerformHardwareSurvey" => GSPerformHardwareSurvey,
    decode_msg_gs_get_play_stats_response => steamkit::base::generated::steam_language::internal::MsgGSGetPlayStatsResponse => "MsgGSGetPlayStatsResponse" => GSGetPlayStatsResponse,
    decode_msg_gs_get_reputation_response => steamkit::base::generated::steam_language::internal::MsgGSGetReputationResponse => "MsgGSGetReputationResponse" => GSGetReputationResponse,
    decode_msg_gs_deny => steamkit::base::generated::steam_language::internal::MsgGSDeny => "MsgGSDeny" => GSDeny,
    decode_msg_gs_approve => steamkit::base::generated::steam_language::internal::MsgGSApprove => "MsgGSApprove" => GSApprove,
    decode_msg_gs_kick => steamkit::base::generated::steam_language::internal::MsgGSKick => "MsgGSKick" => GSKick,
    decode_msg_gs_get_user_group_status => steamkit::base::generated::steam_language::internal::MsgGSGetUserGroupStatus => "MsgGSGetUserGroupStatus" => GSGetUserGroupStatus,
    decode_msg_gs_get_user_group_status_response => steamkit::base::generated::steam_language::internal::MsgGSGetUserGroupStatusResponse => "MsgGSGetUserGroupStatusResponse" => GSGetUserGroupStatusResponse,
    decode_msg_client_join_chat => steamkit::base::generated::steam_language::internal::MsgClientJoinChat => "MsgClientJoinChat" => ClientJoinChat,
    decode_msg_client_chat_enter => steamkit::base::generated::steam_language::internal::MsgClientChatEnter => "MsgClientChatEnter" => ClientChatEnter,
    decode_msg_client_chat_msg => steamkit::base::generated::steam_language::internal::MsgClientChatMsg => "MsgClientChatMsg" => ClientChatMsg,
    decode_msg_client_chat_member_info => steamkit::base::generated::steam_language::internal::MsgClientChatMemberInfo => "MsgClientChatMemberInfo" => ClientChatMemberInfo,
    decode_msg_client_chat_action => steamkit::base::generated::steam_language::internal::MsgClientChatAction => "MsgClientChatAction" => ClientChatAction,
    decode_msg_client_chat_action_result => steamkit::base::generated::steam_language::internal::MsgClientChatActionResult => "MsgClientChatActionResult" => ClientChatActionResult,
    decode_msg_client_chat_room_info => steamkit::base::generated::steam_language::internal::MsgClientChatRoomInfo => "MsgClientChatRoomInfo" => ClientChatRoomInfo,
    decode_msg_client_set_ignore_friend => steamkit::base::generated::steam_language::internal::MsgClientSetIgnoreFriend => "MsgClientSetIgnoreFriend" => ClientSetIgnoreFriend,
    decode_msg_client_set_ignore_friend_response => steamkit::base::generated::steam_language::internal::MsgClientSetIgnoreFriendResponse => "MsgClientSetIgnoreFriendResponse" => ClientSetIgnoreFriendResponse,
    decode_msg_client_logged_off => steamkit::base::generated::steam_language::internal::MsgClientLoggedOff => "MsgClientLoggedOff" => ClientLoggedOff,
    decode_msg_client_log_on_response => steamkit::base::generated::steam_language::internal::MsgClientLogOnResponse => "MsgClientLogOnResponse" => ClientLogOnResponse,
    decode_msg_client_server_unavailable => steamkit::base::generated::steam_language::internal::MsgClientServerUnavailable => "MsgClientServerUnavailable" => ClientServerUnavailable,
    decode_msg_client_create_chat => steamkit::base::generated::steam_language::internal::MsgClientCreateChat => "MsgClientCreateChat" => ClientCreateChat,
    decode_msg_client_create_chat_response => steamkit::base::generated::steam_language::internal::MsgClientCreateChatResponse => "MsgClientCreateChatResponse" => ClientCreateChatResponse,
    decode_msg_client_marketing_message_update2 => steamkit::base::generated::steam_language::internal::MsgClientMarketingMessageUpdate2 => "MsgClientMarketingMessageUpdate2" => ClientMarketingMessageUpdate2,
    decode_msg_client_get_legacy_game_key => steamkit::base::generated::steam_language::internal::MsgClientGetLegacyGameKey => "MsgClientGetLegacyGameKey" => ClientGetLegacyGameKey,
    decode_msg_client_get_legacy_game_key_response => steamkit::base::generated::steam_language::internal::MsgClientGetLegacyGameKeyResponse => "MsgClientGetLegacyGameKeyResponse" => ClientGetLegacyGameKeyResponse,
}
