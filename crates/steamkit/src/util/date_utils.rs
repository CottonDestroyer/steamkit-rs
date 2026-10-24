//! Converting between Unix timestamps and [`SystemTime`].

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::SteamError;

/// The largest timestamp we accept: `9999-12-31T23:59:59Z`.
const MAX_UNIX_SECONDS: u64 = 253_402_300_799;

/// Converts a Unix timestamp to a point in time. Same as
/// [`DateUtils::date_time_from_unix_time`].
pub fn date_time_from_unix_time(unix_time: u64) -> Result<SystemTime, SteamError> {
    DateUtils::date_time_from_unix_time(unix_time)
}

/// Converts a point in time to a Unix timestamp. Same as
/// [`DateUtils::date_time_to_unix_time`].
pub fn date_time_to_unix_time(time: SystemTime) -> Result<u64, SteamError> {
    DateUtils::date_time_to_unix_time(time)
}

/// Date helpers.
#[derive(Debug)]
pub struct DateUtils;

impl DateUtils {
    /// Converts a Unix timestamp (seconds) to a point in time. Timestamps after
    /// the year 9999 are an error.
    pub fn date_time_from_unix_time(unix_time: u64) -> Result<SystemTime, SteamError> {
        if unix_time > MAX_UNIX_SECONDS {
            return Err(SteamError::InvalidArgument(format!(
                "The unix timestamp {unix_time} is outside of the representable date range."
            )));
        }

        Ok(UNIX_EPOCH + Duration::from_secs(unix_time))
    }

    /// Converts a point in time to a Unix timestamp (seconds), truncating any
    /// fraction. Times before the epoch are an error.
    pub fn date_time_to_unix_time(time: SystemTime) -> Result<u64, SteamError> {
        match time.duration_since(UNIX_EPOCH) {
            Ok(duration) => Ok(duration.as_secs()),
            Err(error) => Err(SteamError::InvalidArgument(format!(
                "The date is before the unix epoch ({} seconds before it).",
                error.duration().as_secs()
            ))),
        }
    }
}
