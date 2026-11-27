//! Zero-dependency UTC timestamp formatting and epoch helpers.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn civil_from_unix_secs(secs: u64) -> (i64, u64, u64, u64, u64, u64) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil-from-days algorithm (Gregorian calendar, UTC).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year, month, day, hour, minute, second)
}

/// Formats Unix epoch seconds as RFC-3339 / ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn unix_secs_to_iso8601(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = civil_from_unix_secs(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Formats the current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_utc_iso() -> String {
    unix_secs_to_iso8601(now_secs())
}

/// Formats Unix epoch seconds as compact UTC (`YYYYMMDDTHHMMSSZ`), safe for filenames.
pub fn unix_secs_to_compact_utc(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = civil_from_unix_secs(secs);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Formats the current UTC time as compact `YYYYMMDDTHHMMSSZ`, safe for filenames.
pub fn now_utc_compact() -> String {
    unix_secs_to_compact_utc(now_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch_leap_day_and_compact_utc_accurately() {
        assert_eq!(unix_secs_to_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_secs_to_compact_utc(0), "19700101T000000Z");
        // 2024-02-29T23:59:59Z = 1709251199
        assert_eq!(unix_secs_to_iso8601(1_709_251_199), "2024-02-29T23:59:59Z");
        assert_eq!(unix_secs_to_compact_utc(1_709_251_199), "20240229T235959Z");
        // 2026-11-26T09:30:00Z = 1795685400, 2026-11-27T09:30:00Z = 1795771800
        assert_eq!(unix_secs_to_iso8601(1_795_685_400), "2026-11-26T09:30:00Z");
        assert_eq!(unix_secs_to_iso8601(1_795_771_800), "2026-11-27T09:30:00Z");
    }
}
