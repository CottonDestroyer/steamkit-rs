//! Body types for messages whose protobuf type doesn't follow the usual
//! naming scheme.

use steamkit::base::generated::steam_language::EMsg;

use crate::nethook::message_type_finder::ProtobufType;
use crate::nethook::proto_registry::{self, ProtobufDecoder};
use crate::nethook::well_known_app_ids as app;

/// Game coordinator message numbers that have an override.
mod gc_message {
    /// `k_EMsgGCClientHello`.
    pub const CLIENT_HELLO: u32 = 4006;
    /// `k_EMsgGCClientWelcome`.
    pub const CLIENT_WELCOME: u32 = 4004;
    /// `k_EMsgGCServerHello`.
    pub const SERVER_HELLO: u32 = 4007;
    /// `k_EMsgGCServerWelcome`.
    pub const SERVER_WELCOME: u32 = 4005;
    /// `k_EMsgGCClientConnectionStatus`.
    pub const CLIENT_CONNECTION_STATUS: u32 = 4009;
    /// `k_EMsgGCServerConnectionStatus`.
    pub const SERVER_CONNECTION_STATUS: u32 = 4010;
    /// `k_ESOMsg_Create`.
    pub const SO_CREATE: u32 = 21;
    /// `k_ESOMsg_Destroy`.
    pub const SO_DESTROY: u32 = 23;
    /// `k_ESOMsg_Update`.
    pub const SO_UPDATE: u32 = 22;
    /// `k_ESOMsg_UpdateMultiple`.
    pub const SO_UPDATE_MULTIPLE: u32 = 26;
    /// `k_EMsgGCToClientTeamsInfo`.
    pub const DOTA_TEAMS_INFO: u32 = 8136;
    /// `k_EMsgClientToGCMyTeamInfoRequest`.
    pub const DOTA_MY_TEAM_INFO_REQUEST: u32 = 8137;
}

/// The protobuf body type of a message number, if it has an override.
pub fn body_map(emsg: EMsg) -> Option<ProtobufType> {
    let (name, decoder): (&'static str, ProtobufDecoder) = match emsg {
        EMsg::ClientLogonGameServer => {
            ("CMsgClientLogon", proto_registry::decode_steam_CMsgClientLogon)
        }
        EMsg::ClientGamesPlayed
        | EMsg::ClientGamesPlayedNoDataBlob
        | EMsg::ClientGamesPlayedWithDataBlob => (
            "CMsgClientGamesPlayed",
            proto_registry::decode_steam_CMsgClientGamesPlayed,
        ),
        EMsg::ClientToGC | EMsg::ClientFromGC => {
            ("CMsgGCClient", proto_registry::decode_steam_CMsgGcClient)
        }
        EMsg::ClientFriendMsgIncoming | EMsg::ClientFriendMsgEchoToSender => (
            "CMsgClientFriendMsgIncoming",
            proto_registry::decode_steam_CMsgClientFriendMsgIncoming,
        ),
        EMsg::ClientCurrentUIMode => {
            ("CMsgClientUIMode", proto_registry::decode_steam_CMsgClientUiMode)
        }
        EMsg::ClientGetNumberOfCurrentPlayersDP => (
            "CMsgDPGetNumberOfCurrentPlayers",
            proto_registry::decode_steam_CMsgDpGetNumberOfCurrentPlayers,
        ),
        EMsg::ClientGetNumberOfCurrentPlayersDPResponse => (
            "CMsgDPGetNumberOfCurrentPlayersResponse",
            proto_registry::decode_steam_CMsgDpGetNumberOfCurrentPlayersResponse,
        ),
        EMsg::AMGameServerUpdate => (
            "CMsgGameServerData",
            proto_registry::decode_steam_CMsgGameServerData,
        ),
        EMsg::ClientDPUpdateAppJobReport => (
            "CDataPublisher_ClientUpdateAppJob_Notification",
            proto_registry::decode_steam_CDataPublisherClientUpdateAppJobNotification,
        ),
        EMsg::ClientPlayingSessionState => (
            "CMsgClientPlayingSessionState",
            proto_registry::decode_steam_CMsgClientPlayingSessionState,
        ),
        EMsg::ClientNetworkingCertRequestResponse => (
            "CMsgClientNetworkingCertReply",
            proto_registry::decode_steam_CMsgClientNetworkingCertReply,
        ),
        _ => return None,
    };

    Some(ProtobufType { name, decoder })
}

/// The game coordinator body type of a message number, if it has an
/// override.
pub fn gc_body_map(app_id: u32, gc_emsg: u32) -> Option<ProtobufType> {
    let (name, decoder): (&'static str, ProtobufDecoder) = match app_id {
        app::TEAM_FORTRESS_2 => match gc_emsg {
            gc_message::CLIENT_HELLO | gc_message::SERVER_HELLO => (
                "CMsgClientHello",
                proto_registry::decode_tf2_CMsgClientHello,
            ),
            gc_message::CLIENT_WELCOME | gc_message::SERVER_WELCOME => (
                "CMsgClientWelcome",
                proto_registry::decode_tf2_CMsgClientWelcome,
            ),
            gc_message::SO_CREATE | gc_message::SO_DESTROY | gc_message::SO_UPDATE => (
                "CMsgSOSingleObject",
                proto_registry::decode_tf2_CMsgSoSingleObject,
            ),
            gc_message::SO_UPDATE_MULTIPLE => (
                "CMsgSOMultipleObjects",
                proto_registry::decode_tf2_CMsgSoMultipleObjects,
            ),
            _ => return None,
        },
        app::DOTA_2 => match gc_emsg {
            gc_message::CLIENT_HELLO | gc_message::SERVER_HELLO => (
                "CMsgClientHello",
                proto_registry::decode_dota2_CMsgClientHello,
            ),
            gc_message::CLIENT_WELCOME | gc_message::SERVER_WELCOME => (
                "CMsgClientWelcome",
                proto_registry::decode_dota2_CMsgClientWelcome,
            ),
            gc_message::CLIENT_CONNECTION_STATUS | gc_message::SERVER_CONNECTION_STATUS => (
                "CMsgConnectionStatus",
                proto_registry::decode_dota2_CMsgConnectionStatus,
            ),
            gc_message::SO_CREATE | gc_message::SO_DESTROY | gc_message::SO_UPDATE => (
                "CMsgSOSingleObject",
                proto_registry::decode_dota2_CMsgSoSingleObject,
            ),
            gc_message::SO_UPDATE_MULTIPLE => (
                "CMsgSOMultipleObjects",
                proto_registry::decode_dota2_CMsgSoMultipleObjects,
            ),
            gc_message::DOTA_TEAMS_INFO => (
                "CMsgDOTATeamsInfo",
                proto_registry::decode_dota2_CMsgDotaTeamsInfo,
            ),
            gc_message::DOTA_MY_TEAM_INFO_REQUEST => (
                "CMsgDOTAMyTeamInfoRequest",
                proto_registry::decode_dota2_CMsgDotaMyTeamInfoRequest,
            ),
            _ => return None,
        },
        app::COUNTER_STRIKE_2 => match gc_emsg {
            gc_message::CLIENT_HELLO | gc_message::SERVER_HELLO => (
                "CMsgClientHello",
                proto_registry::decode_csgo_CMsgClientHello,
            ),
            gc_message::CLIENT_WELCOME | gc_message::SERVER_WELCOME => (
                "CMsgClientWelcome",
                proto_registry::decode_csgo_CMsgClientWelcome,
            ),
            gc_message::CLIENT_CONNECTION_STATUS | gc_message::SERVER_CONNECTION_STATUS => (
                "CMsgConnectionStatus",
                proto_registry::decode_csgo_CMsgConnectionStatus,
            ),
            gc_message::SO_CREATE | gc_message::SO_DESTROY | gc_message::SO_UPDATE => (
                "CMsgSOSingleObject",
                proto_registry::decode_csgo_CMsgSoSingleObject,
            ),
            gc_message::SO_UPDATE_MULTIPLE => (
                "CMsgSOMultipleObjects",
                proto_registry::decode_csgo_CMsgSoMultipleObjects,
            ),
            _ => return None,
        },
        app::DEADLOCK => match gc_emsg {
            gc_message::CLIENT_HELLO | gc_message::SERVER_HELLO => (
                "CMsgClientHello",
                proto_registry::decode_deadlock_CMsgClientHello,
            ),
            gc_message::CLIENT_WELCOME | gc_message::SERVER_WELCOME => (
                "CMsgClientWelcome",
                proto_registry::decode_deadlock_CMsgClientWelcome,
            ),
            gc_message::CLIENT_CONNECTION_STATUS | gc_message::SERVER_CONNECTION_STATUS => (
                "CMsgConnectionStatus",
                proto_registry::decode_deadlock_CMsgConnectionStatus,
            ),
            gc_message::SO_CREATE | gc_message::SO_DESTROY | gc_message::SO_UPDATE => (
                "CMsgSOSingleObject",
                proto_registry::decode_deadlock_CMsgSoSingleObject,
            ),
            gc_message::SO_UPDATE_MULTIPLE => (
                "CMsgSOMultipleObjects",
                proto_registry::decode_deadlock_CMsgSoMultipleObjects,
            ),
            _ => return None,
        },
        _ => return None,
    };

    Some(ProtobufType { name, decoder })
}
