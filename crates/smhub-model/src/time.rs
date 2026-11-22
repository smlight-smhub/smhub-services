//! Time helpers without a date library: RFC 3339 timestamps for the API and the
//! `YYYY-MM-DD HH:MM:SS.ffffff` form the existing database uses.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch, now.
pub fn now_unix() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Days since 1970-01-01 from a civil date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn split(secs: i64) -> (i64, u32, u32, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    (y, mo, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// RFC 3339 in UTC, e.g. `2026-11-06T01:02:03Z`.
pub fn rfc3339(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = split(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// RFC 3339 for a `SystemTime`.
pub fn rfc3339_system(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(0),
        Err(_) => 0,
    };
    rfc3339(secs)
}

/// The current time in the database's form, `2026-11-06 01:02:03.123456`, UTC.
pub fn db_now() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let secs = i64::try_from(now.as_secs()).unwrap_or(0);
    let (y, mo, d, h, mi, s) = split(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{:06}", now.subsec_micros())
}

/// Parse `YYYY-MM-DD HH:MM:SS[.ffffff]` (database) or `YYYY-MM-DDTHH:MM:SS[.f][Z]` into Unix
/// seconds, treating it as UTC.
pub fn parse_utc(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let y = num(0, 4)?;
    let mo = u32::try_from(num(5, 7)?).ok()?;
    let d = u32::try_from(num(8, 10)?).ok()?;
    let h = num(11, 13)?;
    let mi = num(14, 16)?;
    let se = num(17, 19)?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_116_103), "2026-10-27T15:48:23Z");
        assert_eq!(parse_utc("2026-10-27 15:48:23.123456"), Some(1_793_116_103));
        assert_eq!(parse_utc("2026-10-27T15:48:23Z"), Some(1_793_116_103));
        assert_eq!(parse_utc("bad"), None);
    }
}
