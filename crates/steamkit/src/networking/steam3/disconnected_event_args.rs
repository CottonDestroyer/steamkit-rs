//! Arguments of the `Disconnected` event.

/// Raised when the connection is closed or lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisconnectedEventArgs {
    /// `true` if the disconnect was requested locally, through `disconnect`.
    pub user_initiated: bool,
}

impl DisconnectedEventArgs {
    /// Creates a new instance.
    pub fn new(user_initiated: bool) -> Self {
        Self { user_initiated }
    }
}
