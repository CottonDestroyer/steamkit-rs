//! Game coordinator messages: protobuf-backed and struct-backed.

use crate::base::gc::msg_base_gc::IClientGCMsg;
use crate::base::gc::packet_base_gc::IPacketGCMsg;
use crate::base::generated::steam_language_internal::{
    IGCSerializableHeader, IGCSerializableMessage, MsgGCHdr, MsgGCHdrProtoBuf, SteamSerializable,
    SteamSerializableDeserialize,
};
use crate::base::msg_base::{MsgBase, Reader};
use crate::error::SteamError;
use crate::types::job_id::JobID;
use steamkit_protos::gc::CMsgProtoBufHeader;

/// A protobuf-backed game coordinator message.
pub struct ClientGCMsgProtobuf<TBody>
where
    TBody: prost::Message + Default,
{
    /// The header of this message.
    pub header: MsgGCHdrProtoBuf,
    /// The body of this message.
    pub body: TBody,
    /// The raw payload that follows the body.
    pub payload: MsgBase,
}

impl<TBody> ClientGCMsgProtobuf<TBody>
where
    TBody: prost::Message + Default,
{
    /// Creates a new message of the given GC message type.
    pub fn new(e_msg: u32) -> Self {
        Self::with_payload_reserve(e_msg, 0)
    }

    /// Like [`new`](Self::new), but reserves `payload_reserve` bytes for the payload.
    pub fn with_payload_reserve(e_msg: u32, payload_reserve: usize) -> Self {
        let mut header = MsgGCHdrProtoBuf::default();
        header.set_gc_emsg(e_msg);
        ClientGCMsgProtobuf {
            header,
            body: TBody::default(),
            payload: MsgBase::new(payload_reserve),
        }
    }

    /// Creates a reply to `reply_to`, targeting its source job.
    pub fn reply(e_msg: u32, reply_to: &dyn IClientGCMsg) -> Self {
        let mut msg = Self::new(e_msg);
        msg.set_target_job_id(reply_to.source_job_id());
        msg
    }

    /// Parses a received GC packet.
    pub fn from_packet(msg: &dyn IPacketGCMsg) -> Result<Self, SteamError> {
        if !msg.is_proto() {
            return Err(SteamError::InvalidArgument(
                "ClientGCMsgProtobuf used for non-proto message!".to_string(),
            ));
        }
        let mut result = Self::new(msg.msg_type());
        result.deserialize(msg.get_data())?;
        Ok(result)
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
    pub fn header(&self) -> &MsgGCHdrProtoBuf {
        &self.header
    }

    /// The mutable header of this message.
    pub fn header_mut(&mut self) -> &mut MsgGCHdrProtoBuf {
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

impl<TBody> IClientGCMsg for ClientGCMsgProtobuf<TBody>
where
    TBody: prost::Message + Default,
{
    fn is_proto(&self) -> bool {
        true
    }

    fn msg_type(&self) -> u32 {
        self.header.msg
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.proto.job_id_target.unwrap_or(u64::MAX))
    }

    fn set_target_job_id(&mut self, id: JobID) {
        self.header.proto.job_id_target = Some(id.value());
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.proto.job_id_source.unwrap_or(u64::MAX))
    }

    fn set_source_job_id(&mut self, id: JobID) {
        self.header.proto.job_id_source = Some(id.value());
    }

    fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.header.write(&mut out);
        out.extend_from_slice(&self.body.encode_to_vec());
        out.extend_from_slice(&self.payload.payload);
        out
    }

    fn deserialize(&mut self, data: &[u8]) -> Result<(), SteamError> {
        let mut reader = Reader::new(data);
        let header = MsgGCHdrProtoBuf::read(&mut reader)?;

        let remaining = reader.remaining();
        let body_bytes = reader.read_up_to(remaining);
        let body = TBody::decode(body_bytes.as_slice())?;

        let mut payload = MsgBase::new(0);
        if !body_bytes.is_empty() {
            payload.write_bytes(&body_bytes);
            payload.seek(0);
        }

        self.header = header;
        self.body = body;
        self.payload = payload;
        Ok(())
    }
}

/// A struct-backed game coordinator message.
pub struct ClientGCMsg<TBody>
where
    TBody: SteamSerializableDeserialize + IGCSerializableMessage,
{
    /// The header of this message.
    pub header: MsgGCHdr,
    /// The body of this message.
    pub body: TBody,
    /// The raw payload that follows the body.
    pub payload: MsgBase,
    /// The GC message type, taken from the body type.
    pub msg_type: u32,
}

impl<TBody> ClientGCMsg<TBody>
where
    TBody: SteamSerializableDeserialize + IGCSerializableMessage,
{
    /// Creates a new message. The message type comes from the body type.
    pub fn new(payload_reserve: usize) -> Self {
        let body = TBody::default();
        let msg_type = body.get_gc_emsg();
        ClientGCMsg {
            header: MsgGCHdr::default(),
            body,
            payload: MsgBase::new(payload_reserve),
            msg_type,
        }
    }

    /// Creates a reply to `reply_to`, targeting its source job.
    pub fn reply(reply_to: &dyn IClientGCMsg) -> Self {
        let mut msg = Self::new(0);
        msg.set_target_job_id(reply_to.source_job_id());
        msg
    }

    /// Parses a received GC packet.
    pub fn from_packet(msg: &dyn IPacketGCMsg) -> Result<Self, SteamError> {
        if msg.is_proto() {
            return Err(SteamError::InvalidArgument(
                "ClientGCMsg used for proto message!".to_string(),
            ));
        }
        let mut result = Self::new(0);
        result.deserialize(msg.get_data())?;
        Ok(result)
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
    pub fn header(&self) -> &MsgGCHdr {
        &self.header
    }

    /// The mutable header of this message.
    pub fn header_mut(&mut self) -> &mut MsgGCHdr {
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

impl<TBody> IClientGCMsg for ClientGCMsg<TBody>
where
    TBody: SteamSerializableDeserialize + IGCSerializableMessage,
{
    fn is_proto(&self) -> bool {
        false
    }

    fn msg_type(&self) -> u32 {
        self.msg_type
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

    fn deserialize(&mut self, data: &[u8]) -> Result<(), SteamError> {
        let mut reader = Reader::new(data);
        let header = MsgGCHdr::read(&mut reader)?;
        let body = TBody::read(&mut reader)?;

        let remaining = reader.remaining();
        let rest = reader.read_up_to(remaining);
        let mut payload = MsgBase::new(0);
        if !rest.is_empty() {
            payload.write_bytes(&rest);
            payload.seek(0);
        }

        self.header = header;
        self.body = body;
        self.payload = payload;
        Ok(())
    }
}
