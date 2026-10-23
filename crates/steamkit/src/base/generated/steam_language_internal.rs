//! Wire serialisation for the non-protobuf Steam message and header structures.
//!
//! Every field is written and read little endian, one at a time, so the byte
//! layout is exact. The protobuf header type (`CMsgProtoBufHeader`) itself
//! comes from `steamkit_protos`.
//!
//! # Where the structures live
//!
//! The plain structures from the Valve definition files are generated into
//! [`crate::base::generated::steam_language::internal`]. That module says
//! *what* a message contains; this one says *how* it is laid out on the wire.
//! The generated types are re-exported here too.
//!
//! The two protobuf headers are the exception. The generated structures only
//! keep the raw header bytes, so [`MsgHdrProtoBuf`] and [`MsgGCHdrProtoBuf`]
//! are defined below with a typed `CMsgProtoBufHeader` instead.
//!
//! # SteamID fields
//!
//! SteamID fields are stored as the raw `u64`. Use `SteamID::from_id(value)`
//! to get a [`SteamID`].

use prost::Message as ProstMessage;

use crate::base::generated::steam_language::internal as generated;
use crate::base::generated::steam_language::{
    EMsg, EAppUsageEvent, EChatAction, EChatActionResult, EChatEntryType, EChatInfoType,
    EChatPermission, EChatRoomEnterResponse, EChatRoomType, EClanRank, EClanRelationship,
    EDenyReason, EIntroducerRouting, EResult, EServerType, EUdpPacketType, EUniverse,
};
use crate::base::msg_base::Reader;
use crate::error::SteamError;
use steamkit_protos::gc::CMsgProtoBufHeader as GcCMsgProtoBufHeader;
use steamkit_protos::steam::CMsgProtoBufHeader;

pub use crate::base::generated::steam_language::internal::{
    Accept, ChallengeData, ConnectData, Datagram, Disconnect, ExtendedClientMsgHdr,
    MsgChannelEncryptRequest, MsgChannelEncryptResponse, MsgChannelEncryptResult,
    MsgClientAppUsageEvent, MsgClientChatAction, MsgClientChatActionResult, MsgClientChatEnter,
    MsgClientChatMemberInfo, MsgClientChatMsg, MsgClientChatRoomInfo, MsgClientCreateChat,
    MsgClientCreateChatResponse, MsgClientGenericResponse, MsgClientGetFriendsWhoPlayGame,
    MsgClientGetFriendsWhoPlayGameResponse, MsgClientGetLegacyGameKey,
    MsgClientGetLegacyGameKeyResponse, MsgClientJoinChat, MsgClientJustStrings,
    MsgClientLogOnResponse,
    MsgClientLoggedOff, MsgClientLogon, MsgClientMarketingMessageUpdate2,
    MsgClientOGSBeginSession, MsgClientOGSBeginSessionResponse, MsgClientOGSEndSession,
    MsgClientOGSEndSessionResponse, MsgClientOGSWriteRow, MsgClientP2PIntroducerMessage,
    MsgClientServerUnavailable, MsgClientSetIgnoreFriend, MsgClientSetIgnoreFriendResponse,
    MsgClientUpdateGuestPassesList, MsgClientVACBanStatus, MsgGCHdr, MsgGSDeny, MsgGSApprove,
    MsgGSGetPlayStatsResponse, MsgGSGetReputationResponse, MsgGSGetUserGroupStatus,
    MsgGSGetUserGroupStatusResponse, MsgGSKick, MsgGSPerformHardwareSurvey, MsgHdr, UdpHeader,
};

/// The write half of a serialisable wire type.
pub trait SteamSerializable: Send + Sync {
    /// Appends this value's wire representation to `out`.
    fn write(&self, out: &mut Vec<u8>);
}

/// The read half of a serialisable wire type.
pub trait SteamSerializableDeserialize: SteamSerializable + Default + Sized {
    /// Reads a value of this type from `r`.
    fn read(r: &mut Reader<'_>) -> Result<Self, SteamError>;
}

/// A type that can be both written and read.
pub trait ISteamSerializable: SteamSerializableDeserialize {}

impl<T> ISteamSerializable for T where T: SteamSerializableDeserialize {}

/// A message header.
pub trait ISteamSerializableHeader: ISteamSerializable {
    /// Sets the message type carried by this header.
    fn set_emsg(&mut self, msg: EMsg);
}

/// A message body with a fixed message type.
pub trait ISteamSerializableMessage: ISteamSerializable {
    /// The network message type of this body.
    fn get_emsg(&self) -> EMsg;
}

/// A game coordinator message header.
pub trait IGCSerializableHeader: ISteamSerializable {
    /// Sets the game coordinator message type carried by this header.
    fn set_gc_emsg(&mut self, msg: u32);
}

/// A game coordinator message body with a fixed message type.
pub trait IGCSerializableMessage: ISteamSerializable {
    /// The game coordinator message type of this body.
    fn get_gc_emsg(&self) -> u32;
}

/// Shorter alias for [`ISteamSerializableHeader`].
pub use ISteamSerializableHeader as SteamSerializableHeader;
/// Shorter alias for [`ISteamSerializableMessage`].
pub use ISteamSerializableMessage as SteamSerializableMessage;
/// Shorter alias for [`IGCSerializableHeader`].
pub use IGCSerializableHeader as GcSerializableHeader;
/// Shorter alias for [`IGCSerializableMessage`].
pub use IGCSerializableMessage as GcSerializableMessage;

/// A Valve enum that travels on the wire as an integer.
pub trait WireEnum: Copy {
    /// The numeric value written to the wire.
    fn wire_value(self) -> i32;
    /// Parses a numeric value read from the wire.
    fn from_wire_value(value: i32) -> Option<Self>;
    /// The number of bytes this enum occupies on the wire.
    fn wire_size() -> usize {
        4
    }
}

/// A value that can be written to, and read from, the Steam wire format.
///
/// This lives on the type because a `macro_rules!` `ty` fragment can't be
/// matched against literal tokens, so the macro can't tell a `u32` field from
/// an enum field on its own.
///
/// Byte arrays aren't covered: their length comes from the surrounding frame,
/// so the structures that have one (`MsgClientP2PIntroducerMessage`, the
/// protobuf headers) write them by hand.
pub trait WireValue: Sized {
    /// Writes this value to the wire.
    fn write_wire(&self, out: &mut Vec<u8>);
    /// Reads a value of this type from the wire.
    fn read_wire(r: &mut Reader<'_>) -> Result<Self, SteamError>;
}

macro_rules! impl_wire_value {
    ( $( $ty:ty ),* $(,)? ) => {
        $(
            impl WireValue for $ty {
                fn write_wire(&self, out: &mut Vec<u8>) {
                    let raw = self.wire_value();
                    if Self::wire_size() == 1 {
                        out.push(raw as u8);
                    } else {
                        out.extend_from_slice(&raw.to_le_bytes());
                    }
                }

                fn read_wire(r: &mut Reader<'_>) -> Result<Self, SteamError> {
                    let raw = if Self::wire_size() == 1 {
                        i32::from(r.read_u8()?)
                    } else {
                        r.read_i32()?
                    };
                    Self::from_wire_value(raw)
                        .ok_or_else(|| SteamError::Malformed(format!("unknown enum value {raw}")))
                }
            }
        )*
    };
}

macro_rules! impl_wire_enum {
    ( $( $ty:ty ),* $(,)? ) => {
        $(
            impl WireEnum for $ty {
                fn wire_value(self) -> i32 {
                    self.value()
                }

                fn from_wire_value(value: i32) -> Option<Self> {
                    Self::from_value(value)
                }
            }
        )*
    };
}

impl_wire_enum!(
    EMsg,
    EAppUsageEvent,
    EChatAction,
    EChatActionResult,
    EChatEntryType,
    EChatInfoType,
    EChatPermission,
    EChatRoomEnterResponse,
    EChatRoomType,
    EClanRank,
    EClanRelationship,
    EDenyReason,
    EIntroducerRouting,
    EResult,
    EServerType,
    EUniverse,
);

impl WireEnum for EUdpPacketType {
    fn wire_value(self) -> i32 {
        self.value() as i32
    }

    fn from_wire_value(value: i32) -> Option<Self> {
        u8::try_from(value).ok().and_then(EUdpPacketType::from_value)
    }

    /// `PacketType` is a single byte on the wire.
    fn wire_size() -> usize {
        1
    }
}

impl_wire_value!(
    EMsg,
    EAppUsageEvent,
    EChatAction,
    EChatActionResult,
    EChatEntryType,
    EChatInfoType,
    EChatPermission,
    EChatRoomEnterResponse,
    EChatRoomType,
    EClanRank,
    EClanRelationship,
    EDenyReason,
    EIntroducerRouting,
    EResult,
    EServerType,
    EUniverse,
    EUdpPacketType,
);

macro_rules! impl_wire_value_primitive {
    ( $( $ty:ident : $write:ident , $read:ident );* $(;)? ) => {
        $(
            impl WireValue for $ty {
                fn write_wire(&self, out: &mut Vec<u8>) {
                    $write(self, out);
                }

                fn read_wire(r: &mut Reader<'_>) -> Result<Self, SteamError> {
                    $read(r)
                }
            }
        )*
    };
}

fn write_u8(v: &u8, out: &mut Vec<u8>) {
    out.push(*v);
}
fn write_i8(v: &i8, out: &mut Vec<u8>) {
    out.push(*v as u8);
}
fn write_u16(v: &u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_i16(v: &i16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_u32(v: &u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_i32(v: &i32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_u64(v: &u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_i64(v: &i64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_f32(v: &f32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_f64(v: &f64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_bool(v: &bool, out: &mut Vec<u8>) {
    out.push(if *v { 1u8 } else { 0u8 });
}

fn read_u8(r: &mut Reader<'_>) -> Result<u8, SteamError> {
    r.read_u8()
}
fn read_i8(r: &mut Reader<'_>) -> Result<i8, SteamError> {
    r.read_i8()
}
fn read_u16(r: &mut Reader<'_>) -> Result<u16, SteamError> {
    r.read_u16()
}
fn read_i16(r: &mut Reader<'_>) -> Result<i16, SteamError> {
    r.read_i16()
}
fn read_u32(r: &mut Reader<'_>) -> Result<u32, SteamError> {
    r.read_u32()
}
fn read_i32(r: &mut Reader<'_>) -> Result<i32, SteamError> {
    r.read_i32()
}
fn read_u64(r: &mut Reader<'_>) -> Result<u64, SteamError> {
    r.read_u64()
}
fn read_i64(r: &mut Reader<'_>) -> Result<i64, SteamError> {
    r.read_i64()
}
fn read_f32(r: &mut Reader<'_>) -> Result<f32, SteamError> {
    r.read_f32()
}
fn read_f64(r: &mut Reader<'_>) -> Result<f64, SteamError> {
    r.read_f64()
}
/// Reads a boolean byte.
///
/// Only `1` counts as true; any other byte reads back as `false`, which is
/// how Steam's own structs behave.
fn read_bool(r: &mut Reader<'_>) -> Result<bool, SteamError> {
    Ok(r.read_u8()? == 1)
}

impl_wire_value_primitive!(
    u8 : write_u8 , read_u8;
    i8 : write_i8 , read_i8;
    u16 : write_u16 , read_u16;
    i16 : write_i16 , read_i16;
    u32 : write_u32 , read_u32;
    i32 : write_i32 , read_i32;
    u64 : write_u64 , read_u64;
    i64 : write_i64 , read_i64;
    f32 : write_f32 , read_f32;
    f64 : write_f64 , read_f64;
    bool : write_bool , read_bool;
);

/// Reads a four byte enum value.
///
/// Returns [`SteamError::Malformed`] when the value does not name a member of
/// the enum.
pub fn read_enum<T: WireEnum>(r: &mut Reader<'_>) -> Result<T, SteamError> {
    let raw = r.read_i32()?;
    T::from_wire_value(raw).ok_or_else(|| SteamError::Malformed(format!("unknown enum value {raw}")))
}

/// The flag that marks a message type as protobuf backed.
const PROTO_MASK: u32 = 0x8000_0000;

/// Strips the protobuf flag and returns the message type.
pub fn get_msg(msg: u32) -> Option<EMsg> {
    EMsg::from_value((msg & !PROTO_MASK) as i32)
}

/// Strips the protobuf flag from a game coordinator message type.
pub fn get_gc_msg(msg: u32) -> u32 {
    msg & !PROTO_MASK
}

/// Sets the protobuf flag on a message type if `protobuf` is true.
pub fn make_msg(msg: EMsg, protobuf: bool) -> u32 {
    let raw = msg.value() as u32;
    if protobuf {
        raw | PROTO_MASK
    } else {
        raw
    }
}

/// Sets the protobuf flag on a game coordinator message type if `protobuf`
/// is true.
pub fn make_gc_msg(msg: u32, protobuf: bool) -> u32 {
    if protobuf {
        msg | PROTO_MASK
    } else {
        msg
    }
}

/// Implements [`SteamSerializable`] and [`SteamSerializableDeserialize`] for a
/// plain wire structure.
///
/// The wire behaviour of each field comes from its own type, via [`WireValue`].
macro_rules! steam_serializable {
    (
        $ty:ty {
            $( $field:ident : $kind:ty = $default:expr ),* $(,)?
        }
    ) => {
        impl SteamSerializable for $ty {
            fn write(&self, out: &mut Vec<u8>) {
                let _ = out;
                $( WireValue::write_wire(&self.$field, out); )*
            }
        }

        impl SteamSerializableDeserialize for $ty {
            fn read(r: &mut Reader<'_>) -> Result<Self, SteamError> {
                let _ = r;
                Ok(Self {
                    $( $field: WireValue::read_wire(r)?, )*
                })
            }
        }
    };
}

/// Implements [`ISteamSerializableMessage`] with a fixed message type.
macro_rules! steam_message {
    ( $ty:ty => $emsg:expr ) => {
        impl ISteamSerializableMessage for $ty {
            fn get_emsg(&self) -> EMsg {
                $emsg
            }
        }
    };
}

/// Implements [`ISteamSerializableHeader`] for a header that carries a `msg`.
macro_rules! steam_header {
    ( $ty:ty ) => {
        impl ISteamSerializableHeader for $ty {
            fn set_emsg(&mut self, msg: EMsg) {
                self.msg = msg;
            }
        }
    };
}

/// The protobuf client message header.
///
/// Wire layout: the protobuf-flagged message type (4 bytes), the length of the
/// protobuf header (4 bytes) and then that many bytes of `CMsgProtoBufHeader`.
#[derive(Clone, Debug, PartialEq)]
pub struct MsgHdrProtoBuf {
    /// The network message type.
    pub msg: EMsg,
    /// The length of the serialised protobuf header, in bytes.
    pub header_length: i32,
    /// The protobuf header itself.
    pub proto: CMsgProtoBufHeader,
}

impl Default for MsgHdrProtoBuf {
    fn default() -> Self {
        MsgHdrProtoBuf {
            msg: EMsg::Invalid,
            header_length: 0,
            proto: CMsgProtoBufHeader::default(),
        }
    }
}

impl SteamSerializable for MsgHdrProtoBuf {
    fn write(&self, out: &mut Vec<u8>) {
        let proto_bytes = self.proto.encode_to_vec();
        out.extend_from_slice(&(make_msg(self.msg, true) as i32).to_le_bytes());
        out.extend_from_slice(&(proto_bytes.len() as i32).to_le_bytes());
        out.extend_from_slice(&proto_bytes);
    }
}

impl SteamSerializableDeserialize for MsgHdrProtoBuf {
    fn read(r: &mut Reader<'_>) -> Result<Self, SteamError> {
        let raw = r.read_i32()?;
        let msg = get_msg(raw as u32).unwrap_or(EMsg::Invalid);
        let header_length = r.read_i32()?;
        if header_length < 0 {
            return Err(SteamError::Malformed(format!(
                "negative protobuf header length {header_length}"
            )));
        }
        let bytes = r.read_bytes(header_length as usize)?;
        let proto = CMsgProtoBufHeader::decode(bytes.as_slice())?;
        Ok(MsgHdrProtoBuf {
            msg,
            header_length,
            proto,
        })
    }
}

steam_header!(MsgHdrProtoBuf);

/// The protobuf game coordinator message header.
///
/// Wire layout: the protobuf-flagged message type (4 bytes), the length of the
/// protobuf header (4 bytes) and then that many bytes of `CMsgProtoBufHeader`.
#[derive(Clone, Debug, PartialEq)]
pub struct MsgGCHdrProtoBuf {
    /// The game coordinator message type.
    pub msg: u32,
    /// The length of the serialised protobuf header, in bytes.
    pub header_length: i32,
    /// The protobuf header itself.
    pub proto: GcCMsgProtoBufHeader,
}

impl Default for MsgGCHdrProtoBuf {
    fn default() -> Self {
        MsgGCHdrProtoBuf {
            msg: 0,
            header_length: 0,
            proto: GcCMsgProtoBufHeader::default(),
        }
    }
}

impl SteamSerializable for MsgGCHdrProtoBuf {
    fn write(&self, out: &mut Vec<u8>) {
        let proto_bytes = self.proto.encode_to_vec();
        out.extend_from_slice(&make_gc_msg(self.msg, true).to_le_bytes());
        out.extend_from_slice(&(proto_bytes.len() as i32).to_le_bytes());
        out.extend_from_slice(&proto_bytes);
    }
}

impl SteamSerializableDeserialize for MsgGCHdrProtoBuf {
    fn read(r: &mut Reader<'_>) -> Result<Self, SteamError> {
        let raw = r.read_u32()?;
        let header_length = r.read_i32()?;
        if header_length < 0 {
            return Err(SteamError::Malformed(format!(
                "negative protobuf header length {header_length}"
            )));
        }
        let bytes = r.read_bytes(header_length as usize)?;
        let proto = GcCMsgProtoBufHeader::decode(bytes.as_slice())?;
        Ok(MsgGCHdrProtoBuf {
            msg: get_gc_msg(raw),
            header_length,
            proto,
        })
    }
}

impl IGCSerializableHeader for MsgGCHdrProtoBuf {
    fn set_gc_emsg(&mut self, msg: u32) {
        self.msg = msg;
    }
}

steam_serializable!(generated::UdpHeader {
    magic: u32 = 0x3130_5356,
    payload_size: u16 = 0,
    packet_type: EUdpPacketType = EUdpPacketType::Invalid,
    flags: u8 = 0,
    source_conn_id: u32 = 512,
    dest_conn_id: u32 = 0,
    seq_this: u32 = 0,
    seq_ack: u32 = 0,
    packets_in_msg: u32 = 0,
    msg_start_seq: u32 = 0,
    msg_size: u32 = 0,
});

steam_serializable!(generated::ChallengeData {
    challenge_value: u32 = 0,
    server_load: u32 = 0,
});

steam_serializable!(generated::ConnectData {
    challenge_value: u32 = 0,
});

steam_serializable!(generated::Accept {});

steam_serializable!(generated::Datagram {});

steam_serializable!(generated::Disconnect {});

steam_serializable!(generated::MsgHdr {
    msg: EMsg = EMsg::Invalid,
    target_job_id: u64 = u64::MAX,
    source_job_id: u64 = u64::MAX,
});

steam_header!(generated::MsgHdr);

steam_serializable!(generated::ExtendedClientMsgHdr {
    msg: EMsg = EMsg::Invalid,
    header_size: u8 = 36,
    header_version: u16 = 2,
    target_job_id: u64 = u64::MAX,
    source_job_id: u64 = u64::MAX,
    header_canary: u8 = 239,
    steam_id: u64 = 0,
    session_id: i32 = 0,
});

steam_header!(generated::ExtendedClientMsgHdr);

steam_serializable!(generated::MsgGCHdr {
    header_version: u16 = 1,
    target_job_id: u64 = u64::MAX,
    source_job_id: u64 = u64::MAX,
});

impl IGCSerializableHeader for generated::MsgGCHdr {
    /// Deliberately a no-op: this header has no message type field to set.
    fn set_gc_emsg(&mut self, _msg: u32) {}
}

impl SteamSerializable for generated::MsgHdrProtoBuf {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(make_msg(self.msg, true) as i32).to_le_bytes());
        out.extend_from_slice(&self.header_length.to_le_bytes());
        out.extend_from_slice(&self.proto);
    }
}

impl SteamSerializableDeserialize for generated::MsgHdrProtoBuf {
    fn read(r: &mut Reader<'_>) -> Result<Self, SteamError> {
        let raw = r.read_i32()?;
        let header_length = r.read_i32()?;
        if header_length < 0 {
            return Err(SteamError::Malformed(format!(
                "negative protobuf header length {header_length}"
            )));
        }
        let proto = r.read_bytes(header_length as usize)?;
        Ok(generated::MsgHdrProtoBuf {
            msg: get_msg(raw as u32).unwrap_or(EMsg::Invalid),
            header_length,
            proto,
        })
    }
}

impl SteamSerializable for generated::MsgGCHdrProtoBuf {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&make_gc_msg(self.msg, true).to_le_bytes());
        out.extend_from_slice(&self.header_length.to_le_bytes());
        out.extend_from_slice(&self.proto);
    }
}

impl SteamSerializableDeserialize for generated::MsgGCHdrProtoBuf {
    fn read(r: &mut Reader<'_>) -> Result<Self, SteamError> {
        let raw = r.read_u32()?;
        let header_length = r.read_i32()?;
        if header_length < 0 {
            return Err(SteamError::Malformed(format!(
                "negative protobuf header length {header_length}"
            )));
        }
        let proto = r.read_bytes(header_length as usize)?;
        Ok(generated::MsgGCHdrProtoBuf {
            msg: get_gc_msg(raw),
            header_length,
            proto,
        })
    }
}

impl IGCSerializableHeader for generated::MsgGCHdrProtoBuf {
    fn set_gc_emsg(&mut self, msg: u32) {
        self.msg = msg;
    }
}

steam_serializable!(generated::MsgClientJustStrings {});

steam_message!(generated::MsgClientJustStrings => EMsg::Invalid);

steam_serializable!(generated::MsgClientGenericResponse {
    result: EResult = EResult::Invalid,
});

steam_message!(generated::MsgClientGenericResponse => EMsg::Invalid);

steam_serializable!(generated::MsgChannelEncryptRequest {
    protocol_version: u32 = 1,
    universe: EUniverse = EUniverse::Invalid,
});

steam_message!(generated::MsgChannelEncryptRequest => EMsg::ChannelEncryptRequest);

steam_serializable!(generated::MsgChannelEncryptResponse {
    protocol_version: u32 = 1,
    key_size: u32 = 128,
});

steam_message!(generated::MsgChannelEncryptResponse => EMsg::ChannelEncryptResponse);

steam_serializable!(generated::MsgChannelEncryptResult {
    result: EResult = EResult::Invalid,
});

steam_message!(generated::MsgChannelEncryptResult => EMsg::ChannelEncryptResult);

steam_serializable!(generated::MsgClientLogon {});

steam_message!(generated::MsgClientLogon => EMsg::ClientLogon);

steam_serializable!(generated::MsgClientVACBanStatus {
    num_bans: u32 = 0,
});

steam_message!(generated::MsgClientVACBanStatus => EMsg::ClientVACBanStatus);

steam_serializable!(generated::MsgClientAppUsageEvent {
    app_usage_event: EAppUsageEvent = EAppUsageEvent::GameLaunch,
    game_id: u64 = 0,
    offline: u16 = 0,
});

steam_message!(generated::MsgClientAppUsageEvent => EMsg::ClientAppUsageEvent);

steam_serializable!(generated::MsgClientUpdateGuestPassesList {
    result: EResult = EResult::Invalid,
    count_guest_passes_to_give: i32 = 0,
    count_guest_passes_to_redeem: i32 = 0,
});

steam_message!(generated::MsgClientUpdateGuestPassesList => EMsg::ClientUpdateGuestPassesList);

/// The fixed size of the `Data` buffer of `MsgClientP2PIntroducerMessage`.
///
/// The buffer is always 1450 bytes on the wire, no matter how much payload
/// `DataLen` announces.
const P2P_INTRODUCER_DATA_SIZE: usize = 1450;

impl SteamSerializable for generated::MsgClientP2PIntroducerMessage {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.steam_id.to_le_bytes());
        out.extend_from_slice(&self.routing_type.wire_value().to_le_bytes());
        // `Data` is a fixed size buffer, so exactly `P2P_INTRODUCER_DATA_SIZE`
        // bytes always go on the wire.
        let take = self.data.len().min(P2P_INTRODUCER_DATA_SIZE);
        out.extend_from_slice(&self.data[..take]);
        out.resize(out.len() + (P2P_INTRODUCER_DATA_SIZE - take), 0);
        out.extend_from_slice(&self.data_len.to_le_bytes());
    }
}

impl SteamSerializableDeserialize for generated::MsgClientP2PIntroducerMessage {
    fn read(r: &mut Reader<'_>) -> Result<Self, SteamError> {
        Ok(generated::MsgClientP2PIntroducerMessage {
            steam_id: r.read_u64()?,
            routing_type: read_enum(r)?,
            data: r.read_bytes(P2P_INTRODUCER_DATA_SIZE)?,
            data_len: r.read_u32()?,
        })
    }
}

steam_message!(
    generated::MsgClientP2PIntroducerMessage => EMsg::ClientP2PIntroducerMessage
);

steam_serializable!(generated::MsgClientOGSBeginSession {
    account_type: u8 = 0,
    account_id: u64 = 0,
    app_id: u32 = 0,
    time_started: u32 = 0,
});

steam_message!(generated::MsgClientOGSBeginSession => EMsg::ClientOGSBeginSession);

steam_serializable!(generated::MsgClientOGSBeginSessionResponse {
    result: EResult = EResult::Invalid,
    collecting_any: bool = false,
    collecting_details: bool = false,
    session_id: u64 = 0,
});

steam_message!(
    generated::MsgClientOGSBeginSessionResponse => EMsg::ClientOGSBeginSessionResponse
);

steam_serializable!(generated::MsgClientOGSEndSession {
    session_id: u64 = 0,
    time_ended: u32 = 0,
    reason_code: i32 = 0,
    count_attributes: i32 = 0,
});

steam_message!(generated::MsgClientOGSEndSession => EMsg::ClientOGSEndSession);

steam_serializable!(generated::MsgClientOGSEndSessionResponse {
    result: EResult = EResult::Invalid,
});

steam_message!(generated::MsgClientOGSEndSessionResponse => EMsg::ClientOGSEndSessionResponse);

steam_serializable!(generated::MsgClientOGSWriteRow {
    session_id: u64 = 0,
    count_attributes: i32 = 0,
});

steam_message!(generated::MsgClientOGSWriteRow => EMsg::ClientOGSWriteRow);

steam_serializable!(generated::MsgClientGetFriendsWhoPlayGame {
    game_id: u64 = 0,
});

steam_message!(
    generated::MsgClientGetFriendsWhoPlayGame => EMsg::ClientGetFriendsWhoPlayGame
);

steam_serializable!(generated::MsgClientGetFriendsWhoPlayGameResponse {
    result: EResult = EResult::Invalid,
    game_id: u64 = 0,
    count_friends: u32 = 0,
});

steam_message!(
    generated::MsgClientGetFriendsWhoPlayGameResponse => EMsg::ClientGetFriendsWhoPlayGameResponse
);

steam_serializable!(generated::MsgGSPerformHardwareSurvey {
    flags: u32 = 0,
});

steam_message!(generated::MsgGSPerformHardwareSurvey => EMsg::GSPerformHardwareSurvey);

steam_serializable!(generated::MsgGSGetPlayStatsResponse {
    result: EResult = EResult::Invalid,
    rank: i32 = 0,
    lifetime_connects: u32 = 0,
    lifetime_minutes_played: u32 = 0,
});

steam_message!(generated::MsgGSGetPlayStatsResponse => EMsg::GSGetPlayStatsResponse);

steam_serializable!(generated::MsgGSGetReputationResponse {
    result: EResult = EResult::Invalid,
    reputation_score: u32 = 0,
    banned: bool = false,
    banned_ip: u32 = 0,
    banned_port: u16 = 0,
    banned_game_id: u64 = 0,
    time_ban_expires: u32 = 0,
});

steam_message!(generated::MsgGSGetReputationResponse => EMsg::GSGetReputationResponse);

steam_serializable!(generated::MsgGSDeny {
    steam_id: u64 = 0,
    deny_reason: EDenyReason = EDenyReason::InvalidVersion,
});

steam_message!(generated::MsgGSDeny => EMsg::GSDeny);

steam_serializable!(generated::MsgGSApprove {
    steam_id: u64 = 0,
});

steam_message!(generated::MsgGSApprove => EMsg::GSApprove);

steam_serializable!(generated::MsgGSKick {
    steam_id: u64 = 0,
    deny_reason: EDenyReason = EDenyReason::InvalidVersion,
    wait_til_map_change: i32 = 0,
});

steam_message!(generated::MsgGSKick => EMsg::GSKick);

steam_serializable!(generated::MsgGSGetUserGroupStatus {
    steam_id_user: u64 = 0,
    steam_id_group: u64 = 0,
});

steam_message!(generated::MsgGSGetUserGroupStatus => EMsg::GSGetUserGroupStatus);

steam_serializable!(generated::MsgGSGetUserGroupStatusResponse {
    steam_id_user: u64 = 0,
    steam_id_group: u64 = 0,
    clan_relationship: EClanRelationship = EClanRelationship::None,
    clan_rank: EClanRank = EClanRank::None,
});

steam_message!(
    generated::MsgGSGetUserGroupStatusResponse => EMsg::GSGetUserGroupStatusResponse
);

steam_serializable!(generated::MsgClientJoinChat {
    steam_id_chat: u64 = 0,
    is_voice_speaker: bool = false,
});

steam_message!(generated::MsgClientJoinChat => EMsg::ClientJoinChat);

steam_serializable!(generated::MsgClientChatEnter {
    steam_id_chat: u64 = 0,
    steam_id_friend: u64 = 0,
    chat_room_type: EChatRoomType = EChatRoomType::Friend,
    steam_id_owner: u64 = 0,
    steam_id_clan: u64 = 0,
    chat_flags: u8 = 0,
    enter_response: EChatRoomEnterResponse = EChatRoomEnterResponse::Success,
    num_members: i32 = 0,
});

steam_message!(generated::MsgClientChatEnter => EMsg::ClientChatEnter);

steam_serializable!(generated::MsgClientChatMsg {
    steam_id_chatter: u64 = 0,
    steam_id_chat_room: u64 = 0,
    chat_msg_type: EChatEntryType = EChatEntryType::Invalid,
});

steam_message!(generated::MsgClientChatMsg => EMsg::ClientChatMsg);

steam_serializable!(generated::MsgClientChatMemberInfo {
    steam_id_chat: u64 = 0,
    r#type: EChatInfoType = EChatInfoType::StateChange,
});

steam_message!(generated::MsgClientChatMemberInfo => EMsg::ClientChatMemberInfo);

steam_serializable!(generated::MsgClientChatAction {
    steam_id_chat: u64 = 0,
    steam_id_user_to_act_on: u64 = 0,
    chat_action: EChatAction = EChatAction::InviteChat,
});

steam_message!(generated::MsgClientChatAction => EMsg::ClientChatAction);

steam_serializable!(generated::MsgClientChatActionResult {
    steam_id_chat: u64 = 0,
    steam_id_user_acted_on: u64 = 0,
    chat_action: EChatAction = EChatAction::InviteChat,
    action_result: EChatActionResult = EChatActionResult::Success,
});

steam_message!(generated::MsgClientChatActionResult => EMsg::ClientChatActionResult);

steam_serializable!(generated::MsgClientChatRoomInfo {
    steam_id_chat: u64 = 0,
    r#type: EChatInfoType = EChatInfoType::StateChange,
});

steam_message!(generated::MsgClientChatRoomInfo => EMsg::ClientChatRoomInfo);

steam_serializable!(generated::MsgClientSetIgnoreFriend {
    my_steam_id: u64 = 0,
    steam_id_friend: u64 = 0,
    ignore: u8 = 0,
});

steam_message!(generated::MsgClientSetIgnoreFriend => EMsg::ClientSetIgnoreFriend);

steam_serializable!(generated::MsgClientSetIgnoreFriendResponse {
    friend_id: u64 = 0,
    result: EResult = EResult::Invalid,
});

steam_message!(generated::MsgClientSetIgnoreFriendResponse => EMsg::ClientSetIgnoreFriendResponse);

steam_serializable!(generated::MsgClientLoggedOff {
    result: EResult = EResult::Invalid,
    sec_min_reconnect_hint: i32 = 0,
    sec_max_reconnect_hint: i32 = 0,
});

steam_message!(generated::MsgClientLoggedOff => EMsg::ClientLoggedOff);

steam_serializable!(generated::MsgClientLogOnResponse {
    result: EResult = EResult::Invalid,
    out_of_game_heartbeat_rate_sec: i32 = 0,
    in_game_heartbeat_rate_sec: i32 = 0,
    client_supplied_steam_id: u64 = 0,
    ip_public: u32 = 0,
    server_real_time: u32 = 0,
});

steam_message!(generated::MsgClientLogOnResponse => EMsg::ClientLogOnResponse);

steam_serializable!(generated::MsgClientServerUnavailable {
    jobid_sent: u64 = 0,
    e_msg_sent: u32 = 0,
    e_server_type_unavailable: EServerType = EServerType::First,
});

steam_message!(generated::MsgClientServerUnavailable => EMsg::ClientServerUnavailable);

steam_serializable!(generated::MsgClientCreateChat {
    chat_room_type: EChatRoomType = EChatRoomType::Friend,
    game_id: u64 = 0,
    steam_id_clan: u64 = 0,
    permission_officer: EChatPermission = EChatPermission::Close,
    permission_member: EChatPermission = EChatPermission::Close,
    permission_all: EChatPermission = EChatPermission::Close,
    members_max: u32 = 0,
    chat_flags: u8 = 0,
    steam_id_friend_chat: u64 = 0,
    steam_id_invited: u64 = 0,
});

steam_message!(generated::MsgClientCreateChat => EMsg::ClientCreateChat);

steam_serializable!(generated::MsgClientCreateChatResponse {
    result: EResult = EResult::Invalid,
    steam_id_chat: u64 = 0,
    chat_room_type: EChatRoomType = EChatRoomType::Friend,
    steam_id_friend_chat: u64 = 0,
});

steam_message!(generated::MsgClientCreateChatResponse => EMsg::ClientCreateChatResponse);

steam_serializable!(generated::MsgClientMarketingMessageUpdate2 {
    marketing_message_update_time: u32 = 0,
    count: u32 = 0,
});

steam_message!(generated::MsgClientMarketingMessageUpdate2 => EMsg::ClientMarketingMessageUpdate2);

steam_serializable!(generated::MsgClientGetLegacyGameKey {
    app_id: u32 = 0,
});

steam_message!(generated::MsgClientGetLegacyGameKey => EMsg::ClientGetLegacyGameKey);

steam_serializable!(generated::MsgClientGetLegacyGameKeyResponse {
    app_id: u32 = 0,
    result: EResult = EResult::Invalid,
    length: u32 = 0,
});

steam_message!(generated::MsgClientGetLegacyGameKeyResponse => EMsg::ClientGetLegacyGameKeyResponse);

#[cfg(test)]
mod temp_wire_verification {
    use super::*;
    use crate::base::generated::steam_language::internal as gen;

    fn roundtrip<T>(value: &T, expected_len: usize, label: &str)
    where
        T: SteamSerializable + SteamSerializableDeserialize + PartialEq + std::fmt::Debug,
    {
        let mut buf = Vec::new();
        value.write(&mut buf);
        assert_eq!(buf.len(), expected_len, "{label}: serialized length");
        let mut r = Reader::new(&buf);
        let back = match T::read(&mut r) {
            Ok(v) => v,
            Err(e) => panic!("{label}: read failed: {e}"),
        };
        assert_eq!(*value, back, "{label}: round trip mismatch");
        assert_eq!(r.position(), buf.len(), "{label}: trailing bytes");
    }

    #[test]
    fn wire_layout_sizes() {
        roundtrip(&gen::UdpHeader::default(), 36, "UdpHeader");
        roundtrip(&gen::ChallengeData::default(), 8, "ChallengeData");
        roundtrip(&gen::ConnectData::default(), 4, "ConnectData");
        roundtrip(&gen::Accept::default(), 0, "Accept");
        roundtrip(&gen::Datagram::default(), 0, "Datagram");
        roundtrip(&gen::Disconnect::default(), 0, "Disconnect");
        roundtrip(&gen::MsgHdr::default(), 20, "MsgHdr");
        roundtrip(&gen::ExtendedClientMsgHdr::default(), 36, "ExtendedClientMsgHdr");
        roundtrip(&gen::MsgGCHdr::default(), 18, "MsgGCHdr");
        roundtrip(&gen::MsgClientJustStrings::default(), 0, "MsgClientJustStrings");
        roundtrip(&gen::MsgClientGenericResponse::default(), 4, "MsgClientGenericResponse");
        roundtrip(&gen::MsgChannelEncryptRequest::default(), 8, "MsgChannelEncryptRequest");
        roundtrip(&gen::MsgChannelEncryptResponse::default(), 8, "MsgChannelEncryptResponse");
        roundtrip(&gen::MsgChannelEncryptResult::default(), 4, "MsgChannelEncryptResult");
        roundtrip(&gen::MsgClientLogon::default(), 0, "MsgClientLogon");
        roundtrip(&gen::MsgClientVACBanStatus::default(), 4, "MsgClientVACBanStatus");
        roundtrip(&gen::MsgClientAppUsageEvent::default(), 14, "MsgClientAppUsageEvent");
        roundtrip(&gen::MsgClientUpdateGuestPassesList::default(), 12, "MsgClientUpdateGuestPassesList");
        roundtrip(&gen::MsgClientOGSBeginSession::default(), 17, "MsgClientOGSBeginSession");
        roundtrip(&gen::MsgClientOGSBeginSessionResponse::default(), 14, "MsgClientOGSBeginSessionResponse");
        roundtrip(&gen::MsgClientOGSEndSession::default(), 20, "MsgClientOGSEndSession");
        roundtrip(&gen::MsgClientOGSEndSessionResponse::default(), 4, "MsgClientOGSEndSessionResponse");
        roundtrip(&gen::MsgClientOGSWriteRow::default(), 12, "MsgClientOGSWriteRow");
        roundtrip(&gen::MsgClientGetFriendsWhoPlayGame::default(), 8, "MsgClientGetFriendsWhoPlayGame");
        roundtrip(&gen::MsgClientGetFriendsWhoPlayGameResponse::default(), 16, "MsgClientGetFriendsWhoPlayGameResponse");
        roundtrip(&gen::MsgGSPerformHardwareSurvey::default(), 4, "MsgGSPerformHardwareSurvey");
        roundtrip(&gen::MsgGSGetPlayStatsResponse::default(), 16, "MsgGSGetPlayStatsResponse");
        roundtrip(&gen::MsgGSGetReputationResponse::default(), 27, "MsgGSGetReputationResponse");
        roundtrip(&gen::MsgGSDeny::default(), 12, "MsgGSDeny");
        roundtrip(&gen::MsgGSApprove::default(), 8, "MsgGSApprove");
        roundtrip(&gen::MsgGSKick::default(), 16, "MsgGSKick");
        roundtrip(&gen::MsgGSGetUserGroupStatus::default(), 16, "MsgGSGetUserGroupStatus");
        roundtrip(&gen::MsgGSGetUserGroupStatusResponse::default(), 24, "MsgGSGetUserGroupStatusResponse");
        roundtrip(&gen::MsgClientJoinChat::default(), 9, "MsgClientJoinChat");
        roundtrip(&gen::MsgClientChatEnter::default(), 45, "MsgClientChatEnter");
        roundtrip(&gen::MsgClientChatMsg::default(), 20, "MsgClientChatMsg");
        roundtrip(&gen::MsgClientChatMemberInfo::default(), 12, "MsgClientChatMemberInfo");
        roundtrip(&gen::MsgClientChatAction::default(), 20, "MsgClientChatAction");
        roundtrip(&gen::MsgClientChatActionResult::default(), 24, "MsgClientChatActionResult");
        roundtrip(&gen::MsgClientChatRoomInfo::default(), 12, "MsgClientChatRoomInfo");
        roundtrip(&gen::MsgClientSetIgnoreFriend::default(), 17, "MsgClientSetIgnoreFriend");
        roundtrip(&gen::MsgClientSetIgnoreFriendResponse::default(), 12, "MsgClientSetIgnoreFriendResponse");
        roundtrip(&gen::MsgClientLoggedOff::default(), 12, "MsgClientLoggedOff");
        roundtrip(&gen::MsgClientLogOnResponse::default(), 28, "MsgClientLogOnResponse");
        roundtrip(&gen::MsgClientServerUnavailable::default(), 16, "MsgClientServerUnavailable");
        roundtrip(&gen::MsgClientCreateChat::default(), 53, "MsgClientCreateChat");
        roundtrip(&gen::MsgClientCreateChatResponse::default(), 24, "MsgClientCreateChatResponse");
        roundtrip(&gen::MsgClientMarketingMessageUpdate2::default(), 8, "MsgClientMarketingMessageUpdate2");
        roundtrip(&gen::MsgClientGetLegacyGameKey::default(), 4, "MsgClientGetLegacyGameKey");
        roundtrip(&gen::MsgClientGetLegacyGameKeyResponse::default(), 12, "MsgClientGetLegacyGameKeyResponse");

        let p2p = gen::MsgClientP2PIntroducerMessage {
            steam_id: 0x0100_0000_0000_0002,
            routing_type: EIntroducerRouting::P2PNetworking,
            data: vec![0xABu8; 1450],
            data_len: 1450,
        };
        roundtrip(&p2p, 1466, "MsgClientP2PIntroducerMessage");
    }

    #[test]
    fn udp_header_bytes_are_byte_exact() {
        let mut buf = Vec::new();
        let hdr = gen::UdpHeader {
            magic: 0x3130_5356,
            payload_size: 0x1234,
            packet_type: EUdpPacketType::ChallengeReq,
            flags: 0x07,
            source_conn_id: 0x1111_1111,
            dest_conn_id: 0x2222_2222,
            seq_this: 0x3333_3333,
            seq_ack: 0x4444_4444,
            packets_in_msg: 0x5555_5555,
            msg_start_seq: 0x6666_6666,
            msg_size: 0x7777_7777,
        };
        hdr.write(&mut buf);

        let expected: Vec<u8> = vec![
            0x56, 0x53, 0x30, 0x31, // magic
            0x34, 0x12, // payload size
            0x01, // packet type - ONE byte
            0x07, // flags
            0x11, 0x11, 0x11, 0x11, // source conn id
            0x22, 0x22, 0x22, 0x22, // dest conn id
            0x33, 0x33, 0x33, 0x33, // seq this
            0x44, 0x44, 0x44, 0x44, // seq ack
            0x55, 0x55, 0x55, 0x55, // packets in msg
            0x66, 0x66, 0x66, 0x66, // msg start seq
            0x77, 0x77, 0x77, 0x77, // msg size
        ];
        assert_eq!(buf, expected);
        assert_eq!(buf.len(), 36);

        let mut r = Reader::new(&buf);
        let back = gen::UdpHeader::read(&mut r).expect("udp header read");
        assert_eq!(back, hdr);
    }

    #[test]
    fn enum_widths_are_four_bytes_except_udp_packet_type() {
        let mut buf = Vec::new();
        gen::MsgHdr::default().write(&mut buf);
        assert_eq!(buf.len(), 20, "MsgHdr = 4 byte EMsg + 2 x u64");

        let mut buf = Vec::new();
        gen::MsgClientGenericResponse::default().write(&mut buf);
        assert_eq!(buf.len(), 4, "EResult is four bytes");

        let mut buf = Vec::new();
        gen::MsgClientChatEnter::default().write(&mut buf);
        assert_eq!(buf.len(), 45, "EChatRoomType + EChatRoomEnterResponse are four bytes");

        let mut buf = Vec::new();
        gen::MsgClientOGSBeginSessionResponse::default().write(&mut buf);
        assert_eq!(buf.len(), 14, "two bools are one byte each");

        let mut buf = Vec::new();
        gen::MsgClientP2PIntroducerMessage {
            steam_id: 1,
            routing_type: EIntroducerRouting::P2PVoiceChat,
            data: Vec::new(),
            data_len: 0,
        }
        .write(&mut buf);
        assert_eq!(buf.len(), 1466, "u64 + 4 byte enum + 1450 bytes + u32");
    }
}

