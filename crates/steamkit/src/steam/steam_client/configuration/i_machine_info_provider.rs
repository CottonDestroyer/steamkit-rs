//! Machine information used for Steam Guard.

/// Information about the machine the program runs on.
///
/// For Steam Guard these have to stay the same on the same machine (or
/// container, etc.), or it's treated as a new machine and you may have to
/// authenticate again.
pub trait IMachineInfoProvider: Send + Sync {
    /// A unique machine id, or `None` if there isn't one.
    fn machine_guid(&self) -> Option<Vec<u8>>;

    /// The primary MAC address, or `None` if there isn't one.
    fn mac_address(&self) -> Option<Vec<u8>>;

    /// The boot disk's unique id, or `None` if there isn't one.
    fn disk_id(&self) -> Option<Vec<u8>>;
}
