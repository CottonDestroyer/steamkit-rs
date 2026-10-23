//! Game coordinator messages as received from the network, before the body is parsed.

use crate::base::generated::steam_language_internal::{
    MsgGCHdr, MsgGCHdrProtoBuf, SteamSerializableDeserialize,
};
use crate::base::msg_base::Reader;
use crate::error::SteamError;
use crate::types::job_id::JobID;

/// A game coordinator message received from the network.
pub trait IPacketGCMsg: Send + Sync {
    /// Whether this packet message is protobuf backed.
    fn is_proto(&self) -> bool;

    /// The network message type of this packet message.
    fn msg_type(&self) -> u32;

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
    msg: Option<&dyn IPacketGCMsg>,
    name: &str,
) -> Result<u32, SteamError> {
    match msg {
        Some(msg) => Ok(msg.msg_type()),
        None => Err(SteamError::InvalidArgument(format!(
            "{name} must not be null"
        ))),
    }
}

/// A protobuf-backed game coordinator packet.
pub struct PacketClientGCMsgProtobuf {
    msg_type: u32,
    target_job_id: JobID,
    source_job_id: JobID,
    /// The parsed header of this packet.
    pub(crate) header: MsgGCHdrProtoBuf,
    payload: Vec<u8>,
}

impl PacketClientGCMsgProtobuf {
    /// Parses the protobuf header of `data` so the job ids can be read.
    pub fn new(e_msg: u32, data: Vec<u8>) -> Result<Self, SteamError> {
        let mut reader = Reader::new(&data);
        let header = MsgGCHdrProtoBuf::read(&mut reader)?;
        Ok(PacketClientGCMsgProtobuf {
            msg_type: e_msg,
            target_job_id: JobID::from_value(header.proto.job_id_target.unwrap_or(u64::MAX)),
            source_job_id: JobID::from_value(header.proto.job_id_source.unwrap_or(u64::MAX)),
            header,
            payload: data,
        })
    }

    /// The header of this packet message.
    pub fn header(&self) -> &MsgGCHdrProtoBuf {
        &self.header
    }
}

impl IPacketGCMsg for PacketClientGCMsgProtobuf {
    fn is_proto(&self) -> bool {
        true
    }

    fn msg_type(&self) -> u32 {
        self.msg_type
    }

    fn target_job_id(&self) -> JobID {
        self.target_job_id
    }

    fn source_job_id(&self) -> JobID {
        self.source_job_id
    }

    fn get_data(&self) -> &[u8] {
        &self.payload
    }
}

/// A game coordinator packet with a plain header.
pub struct PacketClientGCMsg {
    msg_type: u32,
    target_job_id: JobID,
    source_job_id: JobID,
    /// The parsed header of this packet.
    pub(crate) header: MsgGCHdr,
    payload: Vec<u8>,
}

impl PacketClientGCMsg {
    /// Parses the GC header of `data` so the job ids can be read.
    pub fn new(e_msg: u32, data: Vec<u8>) -> Result<Self, SteamError> {
        let mut reader = Reader::new(&data);
        let header = MsgGCHdr::read(&mut reader)?;
        Ok(PacketClientGCMsg {
            msg_type: e_msg,
            target_job_id: JobID::from_value(header.target_job_id),
            source_job_id: JobID::from_value(header.source_job_id),
            header,
            payload: data,
        })
    }

    /// The header of this packet message.
    pub fn header(&self) -> &MsgGCHdr {
        &self.header
    }
}

impl IPacketGCMsg for PacketClientGCMsg {
    fn is_proto(&self) -> bool {
        false
    }

    fn msg_type(&self) -> u32 {
        self.msg_type
    }

    fn target_job_id(&self) -> JobID {
        self.target_job_id
    }

    fn source_job_id(&self) -> JobID {
        self.source_job_id
    }

    fn get_data(&self) -> &[u8] {
        &self.payload
    }
}
