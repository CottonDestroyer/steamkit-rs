//! A minimal `EMsg`.
//!
//! NetHook2 only needs the numeric message type and `k_EMsgMulti`; message
//! names come from Steam's own `PchMsgNameFromEMsg` at runtime. The full
//! enum lives in `steamkit::base::generated::steam_language::EMsg` if you
//! need it.

/// A Steam `EMsg` value as it appears on the wire (a little-endian `uint32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct EMsg(pub u32);

impl EMsg {
    /// `k_EMsgMulti`: a batch of several messages.
    pub const MULTI: EMsg = EMsg(1);

    /// Builds an `EMsg` from the raw little-endian value read off the wire.
    pub const fn from_raw(value: u32) -> EMsg {
        EMsg(value)
    }

    /// Returns the raw wire value.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl From<u16> for EMsg {
    fn from(value: u16) -> EMsg {
        EMsg(value as u32)
    }
}

impl From<u32> for EMsg {
    fn from(value: u32) -> EMsg {
        EMsg(value)
    }
}
