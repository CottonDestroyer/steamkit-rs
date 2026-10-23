//! The common interface and base type of game coordinator messages.

use crate::base::msg_base::MsgBase;
use crate::error::SteamError;
use crate::types::job_id::JobID;

/// A unified interface into game coordinator messages.
pub trait IClientGCMsg: Send + Sync {
    /// Whether this game coordinator message is protobuf backed.
    fn is_proto(&self) -> bool;

    /// The network message type of this game coordinator message.
    fn msg_type(&self) -> u32;

    /// The target job id of this game coordinator message.
    fn target_job_id(&self) -> JobID;

    /// Sets the target job id of this game coordinator message.
    fn set_target_job_id(&mut self, id: JobID);

    /// The source job id of this game coordinator message.
    fn source_job_id(&self) -> JobID;

    /// Sets the source job id of this game coordinator message.
    fn set_source_job_id(&mut self, id: JobID);

    /// Serializes this message.
    fn serialize(&self) -> Vec<u8>;

    /// Fills this message in from serialized `data`.
    fn deserialize(&mut self, data: &[u8]) -> Result<(), SteamError>;
}

/// Header plus payload buffer, shared by all game coordinator messages.
pub struct GCMsgBase<THeader> {
    /// The header of this message.
    pub header: THeader,
    /// The payload buffer of this message.
    pub payload: MsgBase,
}

impl<THeader> GCMsgBase<THeader> {
    /// Creates a game coordinator message base with the given header.
    pub fn new(header: THeader, payload_reserve: usize) -> Self {
        GCMsgBase {
            header,
            payload: MsgBase::new(payload_reserve),
        }
    }
}
