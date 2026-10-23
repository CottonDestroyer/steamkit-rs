//! The error type of the authentication flows.

use crate::base::generated::steam_language::EResult;
use crate::error::SteamError;

/// Returned when `SteamAuthentication` fails to authenticate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationException {
    /// The operation failed with a Steam result code.
    #[error("{message} with result {result:?}.")]
    Failed {
        /// The message that describes the error.
        message: String,
        /// The result code that describes the error.
        result: EResult,
    },

    /// The operation failed for a reason that has no result code.
    #[error("{message}")]
    Other {
        /// The message that describes the error.
        message: String,
    },
}

impl AuthenticationException {
    /// Creates an error that carries a Steam result code.
    pub fn failed(message: impl Into<String>, result: EResult) -> Self {
        AuthenticationException::Failed {
            message: message.into(),
            result,
        }
    }

    /// Creates an error without a result code.
    pub fn other(message: impl Into<String>) -> Self {
        AuthenticationException::Other {
            message: message.into(),
        }
    }

    /// The result of the authentication request, if the failure carried one.
    pub fn result(&self) -> Option<EResult> {
        match self {
            AuthenticationException::Failed { result, .. } => Some(*result),
            AuthenticationException::Other { .. } => None,
        }
    }
}

impl From<AuthenticationException> for SteamError {
    /// Converts into [`SteamError`].
    fn from(value: AuthenticationException) -> Self {
        SteamError::Authentication(value.to_string())
    }
}
