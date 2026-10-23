//! Client messages: protobuf-backed ([`ClientMsgProtobuf`]) and struct-backed ([`ClientMsg`], [`Msg`]).

use crate::base::generated::steam_language::EMsg;
use crate::base::generated::steam_language_internal::{
    ExtendedClientMsgHdr, ISteamSerializableHeader, ISteamSerializableMessage, MsgHdr,
    MsgHdrProtoBuf, SteamSerializable, SteamSerializableDeserialize,
};
use crate::base::msg_base::{MsgBase, Reader};
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamError;
use crate::types::job_id::JobID;
use crate::types::steam_id::SteamID;
use steamkit_protos::steam::CMsgProtoBufHeader;

/// A unified interface into client messages.
pub trait IClientMsg: Send + Sync {
    /// Whether this client message is protobuf backed.
    fn is_proto(&self) -> bool;

    /// The network message type of this client message.
    fn msg_type(&self) -> EMsg;

    /// The session id of this client message.
    fn session_id(&self) -> i32;

    /// Sets the session id of this client message.
    fn set_session_id(&mut self, id: i32);

    /// The SteamID of this client message.
    fn steam_id(&self) -> Option<SteamID>;

    /// Sets the SteamID of this client message.
    fn set_steam_id(&mut self, id: SteamID);

    /// The target job id of this client message.
    fn target_job_id(&self) -> JobID;

    /// Sets the target job id of this client message.
    fn set_target_job_id(&mut self, id: JobID);

    /// The source job id of this client message.
    fn source_job_id(&self) -> JobID;

    /// Sets the source job id of this client message.
    fn set_source_job_id(&mut self, id: JobID);

    /// Serialises this client message to a byte array.
    fn serialize(&self) -> Vec<u8>;
}

/// Access to the raw 64-bit value of a [`SteamID`], account instance bits
/// included. This is what goes into the protobuf header.
pub trait SteamIDRaw {
    /// The raw 64-bit value of this SteamID.
    fn raw_u64(&self) -> u64;
}

impl SteamIDRaw for SteamID {
    fn raw_u64(&self) -> u64 {
        self.value()
    }
}

/// Empty body for header-only protobuf messages.
///
/// Use `ClientMsgProtobuf` with this body to read a message when you only care
/// about the header. Serializing it writes just the header.
#[derive(Clone, Copy, PartialEq, Eq, ::prost::Message)]
pub struct SteamProtobufHeaderOnly {}

/// A protobuf-backed client message.
pub struct ClientMsgProtobuf<TBody: prost::Message + Default = SteamProtobufHeaderOnly> {
    /// The header of this message.
    pub header: MsgHdrProtoBuf,
    /// The body of this message.
    pub body: TBody,
    /// The raw payload that follows the body.
    pub payload: MsgBase,
}

impl<TBody> ClientMsgProtobuf<TBody>
where
    TBody: prost::Message + Default,
{
    /// Creates a new message of the given type.
    pub fn new(e_msg: EMsg) -> Self {
        Self::with_payload_reserve(e_msg, 0)
    }

    /// Like [`new`](Self::new), but reserves `payload_reserve` bytes for the payload.
    pub fn with_payload_reserve(e_msg: EMsg, payload_reserve: usize) -> Self {
        let mut header = MsgHdrProtoBuf::default();
        header.msg = e_msg;
        ClientMsgProtobuf {
            header,
            body: TBody::default(),
            payload: MsgBase::new(payload_reserve),
        }
    }

    /// Creates a reply to `reply_to`, targeting its source job.
    pub fn reply(e_msg: EMsg, reply_to: &dyn IClientMsg) -> Self {
        let mut msg = Self::new(e_msg);
        msg.set_target_job_id(reply_to.source_job_id());
        msg
    }

    /// Parses a received packet. The body is decoded from the bytes after the header.
    pub fn from_packet(msg: &dyn IPacketMsg) -> Result<Self, SteamError> {
        if !msg.is_proto() {
            return Err(SteamError::InvalidArgument(
                "ClientMsgProtobuf used for non-proto message!".to_string(),
            ));
        }

        let data = msg.get_data();
        let mut reader = Reader::new(data);
        let header = MsgHdrProtoBuf::read(&mut reader)?;
        let body_offset = reader.position();

        let mut body_bytes = data.get(body_offset..).ok_or_else(|| {
            SteamError::Malformed("protobuf body offset is outside of the packet".to_string())
        })?;
        let body = TBody::decode(&mut body_bytes)?;

        let mut payload = MsgBase::new(0);
        if !body_bytes.is_empty() {
            payload.write_bytes(body_bytes);
            payload.seek(0);
        }

        Ok(ClientMsgProtobuf {
            header,
            body,
            payload,
        })
    }

    /// The body structure of this message.
    pub fn body(&self) -> &TBody {
        &self.body
    }

    /// The mutable body structure of this message.
    pub fn body_mut(&mut self) -> &mut TBody {
        &mut self.body
    }

    /// Replaces the body structure of this message.
    pub fn set_body(&mut self, body: TBody) {
        self.body = body;
    }

    /// The header of this message.
    pub fn header(&self) -> &MsgHdrProtoBuf {
        &self.header
    }

    /// The mutable header of this message.
    pub fn header_mut(&mut self) -> &mut MsgHdrProtoBuf {
        &mut self.header
    }

    /// Shorthand for the protobuf header.
    pub fn proto_header(&self) -> &CMsgProtoBufHeader {
        &self.header.proto
    }

    /// Mutable shorthand for the protobuf header.
    pub fn proto_header_mut(&mut self) -> &mut CMsgProtoBufHeader {
        &mut self.header.proto
    }

    /// The payload buffer of this message.
    pub fn payload(&self) -> &MsgBase {
        &self.payload
    }

    /// The mutable payload buffer of this message.
    pub fn payload_mut(&mut self) -> &mut MsgBase {
        &mut self.payload
    }
}

impl<TBody> IClientMsg for ClientMsgProtobuf<TBody>
where
    TBody: prost::Message + Default,
{
    fn is_proto(&self) -> bool {
        true
    }

    fn msg_type(&self) -> EMsg {
        self.header.msg
    }

    fn session_id(&self) -> i32 {
        self.header.proto.client_sessionid.unwrap_or(0)
    }

    fn set_session_id(&mut self, id: i32) {
        self.header.proto.client_sessionid = Some(id);
    }

    fn steam_id(&self) -> Option<SteamID> {
        Some(SteamID::from_id(self.header.proto.steamid.unwrap_or(0)))
    }

    fn set_steam_id(&mut self, id: SteamID) {
        self.header.proto.steamid = Some(id.raw_u64());
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.proto.jobid_target.unwrap_or(u64::MAX))
    }

    fn set_target_job_id(&mut self, id: JobID) {
        self.header.proto.jobid_target = Some(id.value());
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.proto.jobid_source.unwrap_or(u64::MAX))
    }

    fn set_source_job_id(&mut self, id: JobID) {
        self.header.proto.jobid_source = Some(id.value());
    }

    fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.header.write(&mut out);
        out.extend_from_slice(&self.body.encode_to_vec());
        out.extend_from_slice(&self.payload.payload);
        out
    }
}

/// A struct-backed client message with an extended header.
pub struct ClientMsg<TBody>
where
    TBody: SteamSerializableDeserialize + ISteamSerializableMessage,
{
    /// The header of this message.
    pub header: ExtendedClientMsgHdr,
    /// The body of this message.
    pub body: TBody,
    /// The raw payload that follows the body.
    pub payload: MsgBase,
}

impl<TBody> ClientMsg<TBody>
where
    TBody: SteamSerializableDeserialize + ISteamSerializableMessage,
{
    /// Creates a new message of the given type. The type is stored in the header too.
    pub fn new(e_msg: EMsg) -> Self {
        Self::with_payload_reserve(e_msg, 0)
    }

    /// Like [`new`](Self::new), but reserves `payload_reserve` bytes for the payload.
    pub fn with_payload_reserve(e_msg: EMsg, payload_reserve: usize) -> Self {
        let mut header = ExtendedClientMsgHdr::default();
        header.set_emsg(e_msg);
        ClientMsg {
            header,
            body: TBody::default(),
            payload: MsgBase::new(payload_reserve),
        }
    }

    /// Creates a reply to `reply_to`, targeting its source job.
    pub fn reply(e_msg: EMsg, reply_to: &dyn IClientMsg) -> Self {
        let mut msg = Self::new(e_msg);
        msg.set_target_job_id(reply_to.source_job_id());
        msg
    }

    /// Parses a received packet.
    pub fn from_packet(msg: &dyn IPacketMsg) -> Result<Self, SteamError> {
        let data = msg.get_data();
        let mut reader = Reader::new(data);
        let header = ExtendedClientMsgHdr::read(&mut reader)?;
        let body = TBody::read(&mut reader)?;

        let mut payload = MsgBase::new(0);
        let remaining = reader.read_up_to(reader.remaining());
        if !remaining.is_empty() {
            payload.write_bytes(&remaining);
            payload.seek(0);
        }

        Ok(ClientMsg {
            header,
            body,
            payload,
        })
    }

    /// The body structure of this message.
    pub fn body(&self) -> &TBody {
        &self.body
    }

    /// The mutable body structure of this message.
    pub fn body_mut(&mut self) -> &mut TBody {
        &mut self.body
    }

    /// The header of this message.
    pub fn header(&self) -> &ExtendedClientMsgHdr {
        &self.header
    }

    /// The mutable header of this message.
    pub fn header_mut(&mut self) -> &mut ExtendedClientMsgHdr {
        &mut self.header
    }

    /// The payload buffer of this message.
    pub fn payload(&self) -> &MsgBase {
        &self.payload
    }

    /// The mutable payload buffer of this message.
    pub fn payload_mut(&mut self) -> &mut MsgBase {
        &mut self.payload
    }
}

impl<TBody> IClientMsg for ClientMsg<TBody>
where
    TBody: SteamSerializableDeserialize + ISteamSerializableMessage,
{
    fn is_proto(&self) -> bool {
        false
    }

    fn msg_type(&self) -> EMsg {
        self.header.msg
    }

    fn session_id(&self) -> i32 {
        self.header.session_id
    }

    fn set_session_id(&mut self, id: i32) {
        self.header.session_id = id;
    }

    fn steam_id(&self) -> Option<SteamID> {
        Some(SteamID::from_id(self.header.steam_id))
    }

    fn set_steam_id(&mut self, id: SteamID) {
        self.header.steam_id = id.raw_u64();
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.target_job_id)
    }

    fn set_target_job_id(&mut self, id: JobID) {
        self.header.target_job_id = id.value();
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.source_job_id)
    }

    fn set_source_job_id(&mut self, id: JobID) {
        self.header.source_job_id = id.value();
    }

    fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.header.write(&mut out);
        self.body.write(&mut out);
        out.extend_from_slice(&self.payload.payload);
        out
    }
}

/// A struct-backed message without session or client information.
pub struct Msg<TBody>
where
    TBody: SteamSerializableDeserialize + ISteamSerializableMessage,
{
    /// The header of this message.
    pub header: MsgHdr,
    /// The body of this message.
    pub body: TBody,
    /// The raw payload that follows the body.
    pub payload: MsgBase,
    /// Session id. `MsgHdr` has no field for it, so it's only kept locally.
    pub session_id: i32,
    /// SteamID. `MsgHdr` has no field for it, so it's only kept locally.
    pub steam_id: Option<SteamID>,
}

impl<TBody> Msg<TBody>
where
    TBody: SteamSerializableDeserialize + ISteamSerializableMessage,
{
    /// Creates a new message of the given type.
    pub fn new(e_msg: EMsg) -> Self {
        Self::with_payload_reserve(e_msg, 0)
    }

    /// Like [`new`](Self::new), but reserves `payload_reserve` bytes for the payload.
    pub fn with_payload_reserve(e_msg: EMsg, payload_reserve: usize) -> Self {
        let mut header = MsgHdr::default();
        header.set_emsg(e_msg);
        Msg {
            header,
            body: TBody::default(),
            payload: MsgBase::new(payload_reserve),
            session_id: 0,
            steam_id: None,
        }
    }

    /// Creates a reply to `reply_to`, targeting its source job.
    pub fn reply(e_msg: EMsg, reply_to: &dyn IClientMsg) -> Self {
        let mut msg = Self::new(e_msg);
        msg.set_target_job_id(reply_to.source_job_id());
        msg
    }

    /// Parses a received packet.
    pub fn from_packet(msg: &dyn IPacketMsg) -> Result<Self, SteamError> {
        let data = msg.get_data();
        let mut reader = Reader::new(data);
        let header = MsgHdr::read(&mut reader)?;
        let body = TBody::read(&mut reader)?;

        let mut payload = MsgBase::new(0);
        let remaining = reader.read_up_to(reader.remaining());
        if !remaining.is_empty() {
            payload.write_bytes(&remaining);
            payload.seek(0);
        }

        Ok(Msg {
            header,
            body,
            payload,
            session_id: 0,
            steam_id: None,
        })
    }

    /// The body structure of this message.
    pub fn body(&self) -> &TBody {
        &self.body
    }

    /// The mutable body structure of this message.
    pub fn body_mut(&mut self) -> &mut TBody {
        &mut self.body
    }

    /// The header of this message.
    pub fn header(&self) -> &MsgHdr {
        &self.header
    }

    /// The mutable header of this message.
    pub fn header_mut(&mut self) -> &mut MsgHdr {
        &mut self.header
    }

    /// The payload buffer of this message.
    pub fn payload(&self) -> &MsgBase {
        &self.payload
    }

    /// The mutable payload buffer of this message.
    pub fn payload_mut(&mut self) -> &mut MsgBase {
        &mut self.payload
    }
}

impl<TBody> IClientMsg for Msg<TBody>
where
    TBody: SteamSerializableDeserialize + ISteamSerializableMessage,
{
    fn is_proto(&self) -> bool {
        false
    }

    fn msg_type(&self) -> EMsg {
        self.header.msg
    }

    fn session_id(&self) -> i32 {
        self.session_id
    }

    fn set_session_id(&mut self, id: i32) {
        self.session_id = id;
    }

    fn steam_id(&self) -> Option<SteamID> {
        self.steam_id
    }

    fn set_steam_id(&mut self, id: SteamID) {
        self.steam_id = Some(id);
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.target_job_id)
    }

    fn set_target_job_id(&mut self, id: JobID) {
        self.header.target_job_id = id.value();
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.source_job_id)
    }

    fn set_source_job_id(&mut self, id: JobID) {
        self.header.source_job_id = id.value();
    }

    fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.header.write(&mut out);
        self.body.write(&mut out);
        out.extend_from_slice(&self.payload.payload);
        out
    }
}
