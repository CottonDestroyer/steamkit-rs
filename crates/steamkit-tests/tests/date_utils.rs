//! `DateUtils` tests.
//!
//! `SystemTime` has no formatter and `chrono` isn't a dependency here, so the
//! test formats the timestamp itself with a small civil calendar conversion.

use steamkit::util::date_utils::DateUtils;

/// The timestamp under test.
const TIMESTAMP: u64 = 1_722_966_409;

/// Converts a number of days since the unix epoch into a civil date.
///
/// This is the well known "days from civil" algorithm; it is exact for the
/// proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };

    (year, month, day)
}

/// Formats a unix timestamp as ISO 8601 UTC with seven fractional digits,
/// e.g. `2024-08-06T17:46:49.0000000Z`.
fn format_round_trip_utc(timestamp: u64) -> String {
    let seconds = (timestamp % 60) as u32;
    let minutes = ((timestamp / 60) % 60) as u32;
    let hours = ((timestamp / 3600) % 24) as u32;
    let days = (timestamp / 86_400) as i64;

    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.0000000Z"
    )
}

#[test]
fn roundtrips_datetime_utc() {
    let date = DateUtils::date_time_from_unix_time(TIMESTAMP)
        .expect("the timestamp must be a representable date");

    // A `SystemTime` is always UTC, so there's no "kind" to check.
    assert_eq!("2024-08-06T17:46:49.0000000Z", format_round_trip_utc(TIMESTAMP));

    let round_trip = DateUtils::date_time_to_unix_time(date)
        .expect("the date must convert back into a unix timestamp");
    assert_eq!(TIMESTAMP, round_trip);
}
