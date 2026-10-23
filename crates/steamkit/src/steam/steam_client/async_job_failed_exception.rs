//! The error for a job that Steam failed on its end. Converts into
//! [`SteamError`].

use std::error::Error;
use std::fmt;

use crate::error::SteamError;

/// Steam hit a remote error while handling a pending job.
#[derive(Debug)]
pub struct AsyncJobFailedException {
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl AsyncJobFailedException {
    /// Creates the error with the default message.
    pub fn new() -> Self {
        Self {
            message: "The job failed remotely.".to_owned(),
            source: None,
        }
    }

    /// Creates the error with the given message.
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    /// Creates the error with the given message and cause.
    pub fn with_cause(message: impl Into<String>, cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self {
            message: message.into(),
            source: Some(cause.into()),
        }
    }

    /// The message describing the error.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Default for AsyncJobFailedException {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AsyncJobFailedException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Some(source) => write!(f, "{}: {}", self.message, source),
            None => f.write_str(&self.message),
        }
    }
}

impl Error for AsyncJobFailedException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn Error + 'static))
    }
}

impl From<AsyncJobFailedException> for SteamError {
    fn from(_: AsyncJobFailedException) -> Self {
        SteamError::JobFailed
    }
}
