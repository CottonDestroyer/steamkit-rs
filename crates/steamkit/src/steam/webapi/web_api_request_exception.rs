//! The error type of failed Web API requests.

use crate::error::SteamError;

/// A failed Web API request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WebAPIRequestException {
    /// The server answered with a non-success status code.
    #[error("Response status code does not indicate success: {status} ({reason}).")]
    Status {
        /// The HTTP status code of the response.
        status: u16,
        /// The reason phrase of the response.
        reason: String,
    },

    /// The request failed for some other reason.
    #[error("{message}")]
    Other {
        /// The message that describes the error.
        message: String,
    },
}

impl WebAPIRequestException {
    /// Creates the error for a non-success response.
    pub fn status(status: u16, reason: impl Into<String>) -> Self {
        WebAPIRequestException::Status {
            status,
            reason: reason.into(),
        }
    }

    /// Creates the error for a failure without a status code.
    pub fn other(message: impl Into<String>) -> Self {
        WebAPIRequestException::Other {
            message: message.into(),
        }
    }
}

impl From<WebAPIRequestException> for SteamError {
    /// Converts into [`SteamError`].
    fn from(value: WebAPIRequestException) -> Self {
        let message = value.to_string();

        match value {
            WebAPIRequestException::Status { .. } => SteamError::Connection(message),
            WebAPIRequestException::Other { .. } => SteamError::Other(message),
        }
    }
}
