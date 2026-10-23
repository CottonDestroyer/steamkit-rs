//! Typed `u64` handles, like [`PublishedFileID`].

use std::fmt;
use std::hash::Hash;

/// A `u64` wrapped in its own type, so different kinds of ids can't be mixed
/// up.
pub trait UInt64Handle: Copy + PartialEq + Eq + Hash {
    /// The wrapped value.
    fn value(&self) -> u64;
}

/// A handle to a file published on the Steam workshop.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct PublishedFileID(u64);

impl PublishedFileID {
    /// Creates an invalid handle (`u64::MAX`).
    pub fn new() -> PublishedFileID {
        PublishedFileID(u64::MAX)
    }

    /// Creates a handle from its value.
    pub fn from_value(file_id: u64) -> PublishedFileID {
        PublishedFileID(file_id)
    }

    /// The wrapped value.
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl UInt64Handle for PublishedFileID {
    fn value(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for PublishedFileID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<PublishedFileID> for u64 {
    fn from(file: PublishedFileID) -> u64 {
        file.0
    }
}

impl From<u64> for PublishedFileID {
    fn from(file_id: u64) -> PublishedFileID {
        PublishedFileID(file_id)
    }
}
