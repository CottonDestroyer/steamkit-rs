//! Which transports to use.

use std::ops::{BitOr, BitOrAssign};

/// The transports to use when talking to Steam, as a bit set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProtocolTypes(pub u32);

impl ProtocolTypes {
    /// No protocol.
    pub const NONE: ProtocolTypes = ProtocolTypes(0);

    /// TCP.
    pub const TCP: ProtocolTypes = ProtocolTypes(1 << 0);

    /// UDP.
    pub const UDP: ProtocolTypes = ProtocolTypes(1 << 1);

    /// WebSockets (HTTP / TLS).
    pub const WEB_SOCKET: ProtocolTypes = ProtocolTypes(1 << 2);

    /// All available protocol types.
    pub const ALL: ProtocolTypes = ProtocolTypes(
        ProtocolTypes::TCP.0 | ProtocolTypes::UDP.0 | ProtocolTypes::WEB_SOCKET.0,
    );

    /// Whether any of the bits in `other` are set in `self`. Any, not all.
    pub fn has(self, other: ProtocolTypes) -> bool {
        (self.0 & other.0) > 0
    }

    /// The raw bit mask.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// The individual flags that are set, in TCP, UDP, WebSocket order.
    pub fn flags(self) -> impl Iterator<Item = ProtocolTypes> {
        [
            ProtocolTypes::TCP,
            ProtocolTypes::UDP,
            ProtocolTypes::WEB_SOCKET,
        ]
        .into_iter()
        .filter(move |flag| self.has(*flag))
    }
}

impl Default for ProtocolTypes {
    fn default() -> Self {
        ProtocolTypes::NONE
    }
}

impl BitOr for ProtocolTypes {
    type Output = ProtocolTypes;

    fn bitor(self, rhs: ProtocolTypes) -> ProtocolTypes {
        ProtocolTypes(self.0 | rhs.0)
    }
}

impl BitOrAssign for ProtocolTypes {
    fn bitor_assign(&mut self, rhs: ProtocolTypes) {
        self.0 |= rhs.0;
    }
}
