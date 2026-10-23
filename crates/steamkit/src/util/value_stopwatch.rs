//! A tiny stopwatch.

use std::time::{Duration, Instant};

use crate::error::SteamError;

/// Measures the time since it was started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueStopwatch {
    /// When it was started; `None` if it never was.
    start_timestamp: Option<Instant>,
}

impl ValueStopwatch {
    /// Starts a new stopwatch.
    pub(crate) fn start_new() -> ValueStopwatch {
        ValueStopwatch {
            start_timestamp: Some(Instant::now()),
        }
    }

    /// The time since it was started. An error if it was never started.
    pub(crate) fn get_elapsed_time(&self) -> Result<Duration, SteamError> {
        match self.start_timestamp {
            Some(start) => Ok(start.elapsed()),
            None => Err(SteamError::InvalidArgument(
                "An uninitialized, or 'default', ValueStopwatch cannot be used to get elapsed time."
                    .to_string(),
            )),
        }
    }
}

impl Default for ValueStopwatch {
    fn default() -> ValueStopwatch {
        ValueStopwatch { start_timestamp: None }
    }
}
