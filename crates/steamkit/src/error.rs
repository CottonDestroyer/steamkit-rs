//! The crate-wide error type.

use std::fmt;

/// Shorthand for `Result<T, SteamError>`.
pub type SteamResult<T> = Result<T, SteamError>;

/// The error type used throughout the crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SteamError {
    /// A socket, file or pipe level failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// The connection to the Steam network could not be established, or was lost.
    #[error("connection error: {0}")]
    Connection(String),

    /// The remote end closed the connection.
    #[error("connection closed by peer")]
    ConnectionClosed,

    /// The operation did not complete within its deadline.
    #[error("timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// The server rejected the job (`EMsg::DestJobFailed`).
    #[error("job failed: the request was rejected by the server")]
    JobFailed,

    /// The operation was cancelled by the caller.
    #[error("operation was cancelled")]
    Cancelled,

    /// Authentication failed.
    #[error("authentication failed: {0}")]
    Authentication(String),

    /// The peer sent data that does not match the protocol.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// A message could not be decoded.
    #[error("malformed message: {0}")]
    Malformed(String),

    /// A compressed payload could not be decompressed.
    #[error("decompression failed: {0}")]
    Decompression(String),

    /// A cryptographic operation failed.
    #[error("cryptographic error: {0}")]
    Crypto(String),

    /// A caller supplied an argument the callee cannot work with.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The requested resource does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// JSON (de)serialisation failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// A protobuf payload could not be decoded.
    #[error("protobuf decode error: {0}")]
    Protobuf(#[from] prost::DecodeError),

    /// A protobuf payload could not be encoded.
    #[error("protobuf encode error: {0}")]
    ProtobufEncode(#[from] prost::EncodeError),

    /// Anything that does not fit a more specific variant.
    #[error("{0}")]
    Other(String),
}

impl SteamError {
    /// Builds a [`SteamError::Connection`] from anything displayable.
    pub fn connection(message: impl fmt::Display) -> Self {
        SteamError::Connection(message.to_string())
    }

    /// Builds a [`SteamError::Protocol`] from anything displayable.
    pub fn protocol(message: impl fmt::Display) -> Self {
        SteamError::Protocol(message.to_string())
    }

    /// Builds a [`SteamError::InvalidArgument`] from anything displayable.
    pub fn invalid_argument(message: impl fmt::Display) -> Self {
        SteamError::InvalidArgument(message.to_string())
    }
}
