//! The protobuf flag on message numbers.

use crate::base::generated::steam_language::EMsg;

/// The bit that marks a message as protobuf-backed.
const PROTO_MASK: u32 = 0x8000_0000;

/// Removes the protobuf flag from a message number.
const EMSG_MASK: u32 = !PROTO_MASK;

/// Strips the protobuf flag and returns the `EMsg`. Same as [`MsgUtil::get_msg`].
pub fn get_msg(msg: u32) -> Option<EMsg> {
    MsgUtil::get_msg(msg)
}

/// Strips the protobuf flag and returns the message number. Same as
/// [`MsgUtil::get_gc_msg`].
pub fn get_gc_msg(msg: u32) -> u32 {
    MsgUtil::get_gc_msg(msg)
}

/// Whether the message number has the protobuf flag. Same as
/// [`MsgUtil::is_proto_buf`].
pub fn is_proto_buf(msg: u32) -> bool {
    MsgUtil::is_proto_buf(msg)
}

/// Builds a message number, with the protobuf flag if asked. Same as
/// [`MsgUtil::make_msg`].
pub fn make_msg(msg: EMsg, protobuf: bool) -> u32 {
    MsgUtil::make_msg(msg, protobuf)
}

/// Builds a game coordinator message number, with the protobuf flag if asked.
/// Same as [`MsgUtil::make_gc_msg`].
pub fn make_gc_msg(msg: u32, protobuf: bool) -> u32 {
    MsgUtil::make_gc_msg(msg, protobuf)
}

/// Helpers for message numbers and their protobuf flag.
#[derive(Debug)]
pub struct MsgUtil;

impl MsgUtil {
    /// Strips the protobuf flag and returns the `EMsg`, or `None` if the number
    /// isn't a known message.
    pub fn get_msg(msg: u32) -> Option<EMsg> {
        EMsg::from_value((msg & EMSG_MASK) as i32)
    }

    /// Strips the protobuf flag from an `EMsg`.
    pub fn get_msg_from(msg: EMsg) -> Option<EMsg> {
        EMsg::from_value((msg.value() as u32 & EMSG_MASK) as i32)
    }

    /// Strips the protobuf flag and returns the number. Use this for game
    /// coordinator messages, whose numbers aren't in `EMsg`.
    pub fn get_gc_msg(msg: u32) -> u32 {
        msg & EMSG_MASK
    }

    /// Whether the message number has the protobuf flag.
    pub fn is_proto_buf(msg: u32) -> bool {
        (msg & PROTO_MASK) > 0
    }

    /// Always `false`: an `EMsg` can't carry the flag. Use
    /// [`MsgUtil::is_proto_buf`] with the raw number instead.
    pub fn is_proto_buf_from(msg: EMsg) -> bool {
        (msg.value() as u32 & PROTO_MASK) > 0
    }

    /// Builds a message number, with the protobuf flag if asked. Returns the
    /// raw number since a flagged value isn't a valid `EMsg`.
    pub fn make_msg(msg: EMsg, protobuf: bool) -> u32 {
        let raw = msg.value() as u32;

        if protobuf {
            raw | PROTO_MASK
        } else {
            raw
        }
    }

    /// Builds a game coordinator message number, with the protobuf flag if asked.
    pub fn make_gc_msg(msg: u32, protobuf: bool) -> u32 {
        if protobuf {
            msg | PROTO_MASK
        } else {
            msg
        }
    }
}
