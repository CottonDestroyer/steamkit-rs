//! Global ids, and UGC handles which are built on them.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::types::steam_id::BitVector64;

/// Seconds between the Unix epoch and the Steam epoch (2005-01-01T00:00:00Z).
const STEAM_EPOCH_UNIX_SECONDS: u64 = 1_104_537_600;

/// A globally unique id on the Steam network, unique across all racks and
/// servers of a universe.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GlobalID {
    gid_bits: BitVector64,
}

impl GlobalID {
    /// Creates an invalid id (`u64::MAX`).
    pub fn new() -> GlobalID {
        GlobalID::from_value(u64::MAX)
    }

    /// Creates an id from its 64-bit value.
    pub fn from_value(gid: u64) -> GlobalID {
        GlobalID {
            gid_bits: BitVector64::new(gid),
        }
    }

    /// The 64-bit value.
    pub fn value(&self) -> u64 {
        self.gid_bits.data()
    }

    /// Sets the 64-bit value.
    pub fn set_value(&mut self, value: u64) {
        self.gid_bits.set_data(value);
    }

    /// The sequential count.
    pub fn sequential_count(&self) -> u32 {
        self.gid_bits.get(0, 0xF_FFFF) as u32
    }

    /// Sets the sequential count.
    pub fn set_sequential_count(&mut self, value: u32) {
        self.gid_bits.set(0, 0xF_FFFF, value as u64);
    }

    /// When the server that generated this id started. Steam epoch, one second
    /// resolution.
    pub fn start_time(&self) -> SystemTime {
        let start_time = self.gid_bits.get(20, 0x3_FFFF_FFFF) as u64;
        UNIX_EPOCH + Duration::from_secs(STEAM_EPOCH_UNIX_SECONDS + start_time)
    }

    /// Sets when the server that generated this id started.
    pub fn set_start_time(&mut self, value: SystemTime) {
        let unix_seconds = match value.duration_since(UNIX_EPOCH) {
            Ok(delta) => delta.as_secs() as i64,
            Err(err) => -(err.duration().as_secs() as i64),
        };
        let seconds_since_steam_epoch = unix_seconds - STEAM_EPOCH_UNIX_SECONDS as i64;
        self.gid_bits
            .set(20, 0x3_FFFF_FFFF, seconds_since_steam_epoch as u64);
    }

    /// The process id of the server that generated this id.
    pub fn process_id(&self) -> u32 {
        self.gid_bits.get(50, 0xF) as u32
    }

    /// Sets the process id of the server that generated this id.
    pub fn set_process_id(&mut self, value: u32) {
        self.gid_bits.set(50, 0xF, value as u64);
    }

    /// The box id of the server that generated this id.
    pub fn box_id(&self) -> u32 {
        self.gid_bits.get(54, 0x3_FF) as u32
    }

    /// Sets the box id of the server that generated this id.
    pub fn set_box_id(&mut self, value: u32) {
        self.gid_bits.set(54, 0x3_FF, value as u64);
    }
}

impl fmt::Display for GlobalID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

impl fmt::Debug for GlobalID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<GlobalID> for u64 {
    fn from(gid: GlobalID) -> u64 {
        gid.value()
    }
}

impl From<u64> for GlobalID {
    fn from(gid: u64) -> GlobalID {
        GlobalID::from_value(gid)
    }
}

/// A handle to a piece of user generated content.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UGCHandle {
    global_id: GlobalID,
}

impl UGCHandle {
    /// Creates an invalid handle.
    pub fn new() -> UGCHandle {
        UGCHandle {
            global_id: GlobalID::new(),
        }
    }

    /// Creates a handle from its 64-bit value.
    pub fn from_value(ugc_id: u64) -> UGCHandle {
        UGCHandle {
            global_id: GlobalID::from_value(ugc_id),
        }
    }

    /// The 64-bit value.
    pub fn value(&self) -> u64 {
        self.global_id.value()
    }

    /// The [`GlobalID`] this handle is built on.
    pub fn global_id(&self) -> GlobalID {
        self.global_id
    }
}

impl fmt::Display for UGCHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

impl fmt::Debug for UGCHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<UGCHandle> for u64 {
    fn from(handle: UGCHandle) -> u64 {
        handle.value()
    }
}

impl From<u64> for UGCHandle {
    fn from(ugc_id: u64) -> UGCHandle {
        UGCHandle::from_value(ugc_id)
    }
}
