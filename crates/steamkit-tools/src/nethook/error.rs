//! Errors of the NetHook dump analyzer.

use std::fmt;

use steamkit::error::SteamError;

/// Result alias used throughout the analyzer.
pub type NetHookResult<T> = Result<T, NetHookError>;

/// Everything that can go wrong while reading a NetHook dump.
#[derive(Debug)]
pub enum NetHookError {
    /// A file of the dump could not be read.
    Io {
        /// Path that failed to load.
        path: String,
        /// Underlying I/O failure.
        source: std::io::Error,
    },

    /// The captured data does not follow the Steam wire format.
    Malformed {
        /// Human readable description of the problem.
        message: String,
    },

    /// A protobuf body could not be decoded.
    Protobuf {
        /// Human readable description of the problem.
        message: String,
    },

    /// The SteamKit library rejected the data.
    Steam {
        /// Underlying library failure.
        source: SteamError,
    },
}

impl NetHookError {
    /// Builds a [`NetHookError::Malformed`].
    pub fn malformed(message: impl Into<String>) -> Self {
        NetHookError::Malformed {
            message: message.into(),
        }
    }

    /// Builds a [`NetHookError::Protobuf`].
    pub fn protobuf(message: impl Into<String>) -> Self {
        NetHookError::Protobuf {
            message: message.into(),
        }
    }
}

impl fmt::Display for NetHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetHookError::Io { path, source } => write!(f, "unable to read '{path}': {source}"),
            NetHookError::Malformed { message } => write!(f, "malformed capture: {message}"),
            NetHookError::Protobuf { message } => write!(f, "unable to decode protobuf: {message}"),
            NetHookError::Steam { source } => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for NetHookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NetHookError::Io { source, .. } => Some(source),
            NetHookError::Steam { source } => Some(source),
            _ => None,
        }
    }
}

impl From<SteamError> for NetHookError {
    fn from(source: SteamError) -> Self {
        NetHookError::Steam { source }
    }
}
