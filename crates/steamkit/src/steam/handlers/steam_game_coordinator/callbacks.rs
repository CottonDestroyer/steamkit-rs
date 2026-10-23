//! Callbacks posted by [`SteamGameCoordinator`](super::steam_game_coordinator::SteamGameCoordinator).

use std::any::Any;
use std::fmt;

use steamkit_protos::steam::CMsgGcClient;

use crate::base::client_msg::ClientMsgProtobuf;
use crate::base::gc::packet_base_gc::{
    IPacketGCMsg, PacketClientGCMsg, PacketClientGCMsgProtobuf,
};
use crate::base::packet_base::IPacketMsg;
use crate::error::SteamResult;
use crate::steam::steam_client::callback_mgr::callback_msg::CallbackMsg;
use crate::types::job_id::JobID;
use crate::util::msg_util::{get_gc_msg, is_proto_buf};

/// A game coordinator message received from the network, either protobuf-backed
/// or with a plain header.
pub enum PacketGCMsg {
    /// A protobuf-backed GC message.
    Protobuf(PacketClientGCMsgProtobuf),
    /// A game coordinator message with a plain header.
    Plain(PacketClientGCMsg),
}

impl fmt::Debug for PacketGCMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketGCMsg::Protobuf(msg) => f
                .debug_struct("Protobuf")
                .field("msg_type", &msg.msg_type())
                .finish(),
            PacketGCMsg::Plain(msg) => f
                .debug_struct("Plain")
                .field("msg_type", &msg.msg_type())
                .finish(),
        }
    }
}

impl IPacketGCMsg for PacketGCMsg {
    fn is_proto(&self) -> bool {
        match self {
            PacketGCMsg::Protobuf(msg) => msg.is_proto(),
            PacketGCMsg::Plain(msg) => msg.is_proto(),
        }
    }

    fn msg_type(&self) -> u32 {
        match self {
            PacketGCMsg::Protobuf(msg) => msg.msg_type(),
            PacketGCMsg::Plain(msg) => msg.msg_type(),
        }
    }

    fn target_job_id(&self) -> JobID {
        match self {
            PacketGCMsg::Protobuf(msg) => msg.target_job_id(),
            PacketGCMsg::Plain(msg) => msg.target_job_id(),
        }
    }

    fn source_job_id(&self) -> JobID {
        match self {
            PacketGCMsg::Protobuf(msg) => msg.source_job_id(),
            PacketGCMsg::Plain(msg) => msg.source_job_id(),
        }
    }

    fn get_data(&self) -> &[u8] {
        match self {
            PacketGCMsg::Protobuf(msg) => msg.get_data(),
            PacketGCMsg::Plain(msg) => msg.get_data(),
        }
    }
}

/// Parses the packet of a game coordinator message.
fn get_packet_gc_msg(e_msg: u32, data: &[u8]) -> SteamResult<PacketGCMsg> {
    // Strip the protobuf flag.
    let real_e_msg = get_gc_msg(e_msg);

    if is_proto_buf(e_msg) {
        Ok(PacketGCMsg::Protobuf(PacketClientGCMsgProtobuf::new(
            real_e_msg,
            data.to_vec(),
        )?))
    } else {
        Ok(PacketGCMsg::Plain(PacketClientGCMsg::new(
            real_e_msg,
            data.to_vec(),
        )?))
    }
}

/// Posted when a game coordinator message arrives.
#[derive(Debug)]
pub struct MessageCallback {
    job_id: JobID,
    /// The raw message type, with the protobuf flag if there is one.
    e_msg: u32,
    app_id: u32,
    message: PacketGCMsg,
}

impl MessageCallback {
    /// Builds the callback from a received `ClientFromGC` message.
    pub(crate) fn from_packet(packet_msg: &dyn IPacketMsg) -> SteamResult<Self> {
        let msg = ClientMsgProtobuf::<CMsgGcClient>::from_packet(packet_msg)?;
        let gc_msg = msg.body;

        let e_msg = gc_msg.msgtype.unwrap_or_default();
        let app_id = gc_msg.appid.unwrap_or_default();
        let payload = gc_msg.payload.unwrap_or_default();

        let message = get_packet_gc_msg(e_msg, &payload)?;
        let job_id = message.target_job_id();

        Ok(Self {
            job_id,
            e_msg,
            app_id,
            message,
        })
    }

    /// The game coordinator message type.
    pub fn e_msg(&self) -> u32 {
        get_gc_msg(self.e_msg)
    }

    /// The app id of the game coordinator the message is from.
    pub fn app_id(&self) -> u32 {
        self.app_id
    }

    /// Whether the message is protobuf-backed.
    pub fn is_proto(&self) -> bool {
        is_proto_buf(self.e_msg)
    }

    /// The actual message.
    pub fn message(&self) -> &PacketGCMsg {
        &self.message
    }
}

impl CallbackMsg for MessageCallback {
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
