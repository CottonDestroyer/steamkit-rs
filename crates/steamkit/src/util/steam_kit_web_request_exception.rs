//! The error of a failed HTTP request.

use std::fmt;

use reqwest::header::HeaderMap;
use reqwest::{Response, StatusCode};

use crate::error::SteamError;

/// A failed HTTP request.
pub struct SteamKitWebRequestException {
    /// What went wrong.
    message: String,
    /// The status code of the response.
    status_code: StatusCode,
    /// The headers of the response.
    headers: HeaderMap,
    /// The underlying error, if any.
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl SteamKitWebRequestException {
    /// Creates the error for a response, keeping its status code and headers.
    pub fn new(message: impl Into<String>, response: &Response) -> SteamKitWebRequestException {
        SteamKitWebRequestException {
            message: message.into(),
            status_code: response.status(),
            headers: response.headers().clone(),
            source: None,
        }
    }

    /// Creates the error from just a message. The status code is `200 OK` and
    /// there are no headers.
    pub fn from_message(message: impl Into<String>) -> SteamKitWebRequestException {
        SteamKitWebRequestException {
            message: message.into(),
            status_code: StatusCode::OK,
            headers: HeaderMap::new(),
            source: None,
        }
    }

    /// Creates the error from a message and an underlying error.
    pub fn with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> SteamKitWebRequestException {
        SteamKitWebRequestException {
            message: message.into(),
            status_code: StatusCode::OK,
            headers: HeaderMap::new(),
            source: Some(Box::new(source)),
        }
    }

    /// The status code of the response.
    pub fn status_code(&self) -> StatusCode {
        self.status_code
    }

    /// The headers of the response.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// What went wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Default for SteamKitWebRequestException {
    fn default() -> SteamKitWebRequestException {
        SteamKitWebRequestException {
            message: String::new(),
            status_code: StatusCode::OK,
            headers: HeaderMap::new(),
            source: None,
        }
    }
}

impl fmt::Display for SteamKitWebRequestException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (HTTP {})",
            self.message,
            self.status_code.as_u16()
        )
    }
}

impl fmt::Debug for SteamKitWebRequestException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SteamKitWebRequestException")
            .field("message", &self.message)
            .field("status_code", &self.status_code)
            .field("headers", &self.headers)
            .field("source", &self.source.as_ref().map(|error| error.to_string()))
            .finish()
    }
}

impl std::error::Error for SteamKitWebRequestException {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|error| error.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<SteamKitWebRequestException> for SteamError {
    fn from(value: SteamKitWebRequestException) -> Self {
        SteamError::Other(value.to_string())
    }
}
