//! Messages as received from the network: the parsed header plus the offset
//! where the body starts. The body itself is parsed later, by the handler that
//! wants it.

use crate::base::generated::steam_language::EMsg;
use crate::base::generated::steam_language_internal::{
    ExtendedClientMsgHdr, MsgHdr, MsgHdrProtoBuf, SteamSerializableDeserialize,
};
use crate::base::msg_base::Reader;
use crate::error::SteamError;
use crate::types::job_id::JobID;

/// A message received from the network.
///
/// Unlike [`IClientMsg`](crate::base::client_msg::IClientMsg), this only gives
/// access to the header and the raw bytes.
pub trait IPacketMsg: Send + Sync {
    /// Whether this packet message is protobuf backed.
    fn is_proto(&self) -> bool;

    /// The network message type of this packet message.
    fn msg_type(&self) -> EMsg;

    /// The target job id of this packet message.
    fn target_job_id(&self) -> JobID;

    /// The source job id of this packet message.
    fn source_job_id(&self) -> JobID;

    /// The raw bytes of the message.
    fn get_data(&self) -> &[u8];
}

/// Returns the message type of `msg`, or an `InvalidArgument` error naming
/// `name` if there is no message.
pub fn get_msg_type_with_null_check(
    msg: Option<&dyn IPacketMsg>,
    name: &str,
) -> Result<EMsg, SteamError> {
    match msg {
        Some(msg) => Ok(msg.msg_type()),
        None => Err(SteamError::InvalidArgument(format!(
            "{name} must not be null"
        ))),
    }
}

/// A protobuf-backed packet.
pub struct PacketClientMsgProtobuf {
    msg_type: EMsg,
    /// The parsed header of this packet.
    pub(crate) header: MsgHdrProtoBuf,
    /// The offset of the body inside the raw data.
    pub(crate) body_offset: u64,
    payload: Vec<u8>,
}

impl PacketClientMsgProtobuf {
    /// Parses the protobuf header of `data` so the job ids can be read. The
    /// body starts right after the header.
    pub fn new(e_msg: EMsg, data: Vec<u8>) -> Result<Self, SteamError> {
        let mut reader = Reader::new(&data);
        let header = MsgHdrProtoBuf::read(&mut reader)?;
        Ok(PacketClientMsgProtobuf {
            msg_type: e_msg,
            header,
            body_offset: reader.position() as u64,
            payload: data,
        })
    }

    /// The header of this packet message.
    pub fn header(&self) -> &MsgHdrProtoBuf {
        &self.header
    }

    /// The offset of the body inside the raw data.
    pub fn body_offset(&self) -> u64 {
        self.body_offset
    }
}

impl IPacketMsg for PacketClientMsgProtobuf {
    fn is_proto(&self) -> bool {
        true
    }

    fn msg_type(&self) -> EMsg {
        self.msg_type
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.proto.jobid_target.unwrap_or(u64::MAX))
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.proto.jobid_source.unwrap_or(u64::MAX))
    }

    fn get_data(&self) -> &[u8] {
        &self.payload
    }
}

/// A packet with an extended header.
pub struct PacketClientMsg {
    msg_type: EMsg,
    /// The parsed header of this packet.
    pub(crate) header: ExtendedClientMsgHdr,
    /// The offset of the body inside the raw data.
    pub(crate) body_offset: u64,
    payload: Vec<u8>,
}

impl PacketClientMsg {
    /// Parses the extended header of `data` so the job ids can be read. The
    /// body starts right after the header.
    pub fn new(e_msg: EMsg, data: Vec<u8>) -> Result<Self, SteamError> {
        let mut reader = Reader::new(&data);
        let header = ExtendedClientMsgHdr::read(&mut reader)?;
        Ok(PacketClientMsg {
            msg_type: e_msg,
            header,
            body_offset: reader.position() as u64,
            payload: data,
        })
    }

    /// The header of this packet message.
    pub fn header(&self) -> &ExtendedClientMsgHdr {
        &self.header
    }

    /// The offset of the body inside the raw data.
    pub fn body_offset(&self) -> u64 {
        self.body_offset
    }
}

impl IPacketMsg for PacketClientMsg {
    fn is_proto(&self) -> bool {
        false
    }

    fn msg_type(&self) -> EMsg {
        self.msg_type
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.target_job_id)
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.source_job_id)
    }

    fn get_data(&self) -> &[u8] {
        &self.payload
    }
}

/// A packet with a basic header.
pub struct PacketMsg {
    msg_type: EMsg,
    /// The parsed header of this packet.
    pub(crate) header: MsgHdr,
    /// The offset of the body inside the raw data.
    pub(crate) body_offset: u64,
    payload: Vec<u8>,
}

impl PacketMsg {
    /// Parses the header of `data` so the job ids can be read. The body starts
    /// right after the header.
    pub fn new(e_msg: EMsg, data: Vec<u8>) -> Result<Self, SteamError> {
        let mut reader = Reader::new(&data);
        let header = MsgHdr::read(&mut reader)?;
        Ok(PacketMsg {
            msg_type: e_msg,
            header,
            body_offset: reader.position() as u64,
            payload: data,
        })
    }

    /// The header of this packet message.
    pub fn header(&self) -> &MsgHdr {
        &self.header
    }

    /// The offset of the body inside the raw data.
    pub fn body_offset(&self) -> u64 {
        self.body_offset
    }
}

impl IPacketMsg for PacketMsg {
    fn is_proto(&self) -> bool {
        false
    }

    fn msg_type(&self) -> EMsg {
        self.msg_type
    }

    fn target_job_id(&self) -> JobID {
        JobID::from_value(self.header.target_job_id)
    }

    fn source_job_id(&self) -> JobID {
        JobID::from_value(self.header.source_job_id)
    }

    fn get_data(&self) -> &[u8] {
        &self.payload
    }
}
