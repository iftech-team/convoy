//! Just enough calendar arithmetic to produce and check the ISO-8601 stamps
//! stored in `activity[].at`. Avoids a date dependency in the core crate; the
//! UI formats dates with `glib::DateTime`.

use std::time::{SystemTime, UNIX_EPOCH};

/// `new Date().toISOString()` — always UTC, always millisecond precision.
pub fn now_iso8601() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    iso8601(now.as_secs() as i64, now.subsec_millis())
}

pub fn iso8601(seconds: i64, milliseconds: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{milliseconds:03}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Stand-in for `Number.isFinite(Date.parse(value))`. Accepts the ISO-8601
/// forms the app writes (and reads back), including an explicit offset.
/// Deliberately narrower than `Date.parse`, which also accepts legacy formats
/// no Convoy build has ever produced.
pub fn parse_iso8601(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> { value.get(from..to)?.parse().ok() };
    let digits = |from: usize, to: usize| -> bool {
        value
            .get(from..to)
            .is_some_and(|part| part.bytes().all(|b| b.is_ascii_digit()))
    };
    if !digits(0, 4) || bytes[4] != b'-' || !digits(5, 7) || bytes[7] != b'-' || !digits(8, 10) {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut seconds = days_from_civil(year, month, day) * 86_400;
    if bytes.len() == 10 {
        return Some(seconds);
    }
    if bytes[10] != b'T' && bytes[10] != b' ' {
        return None;
    }
    if !digits(11, 13) || bytes.get(13) != Some(&b':') || !digits(14, 16) {
        return None;
    }
    let (hour, minute) = (number(11, 13)?, number(14, 16)?);
    if hour > 23 || minute > 59 {
        return None;
    }
    seconds += hour * 3600 + minute * 60;
    let mut index = 16;
    if bytes.get(index) == Some(&b':') {
        if !digits(17, 19) {
            return None;
        }
        let second = number(17, 19)?;
        if second > 60 {
            return None;
        }
        seconds += second;
        index = 19;
        if bytes.get(index) == Some(&b'.') {
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index == start {
                return None;
            }
        }
    }
    match bytes.get(index) {
        None => Some(seconds),
        Some(b'Z') if index + 1 == bytes.len() => Some(seconds),
        Some(sign @ (b'+' | b'-')) => {
            let sign = if *sign == b'-' { 1 } else { -1 };
            if !digits(index + 1, index + 3) {
                return None;
            }
            let hours = number(index + 1, index + 3)?;
            let minutes = match bytes.len() - index {
                6 if bytes[index + 3] == b':' => number(index + 4, index + 6)?,
                5 => number(index + 3, index + 5)?,
                3 => 0,
                _ => return None,
            };
            if hours > 23 || minutes > 59 {
                return None;
            }
            Some(seconds + sign * (hours * 3600 + minutes * 60))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_reparses_the_epoch() {
        assert_eq!(iso8601(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(parse_iso8601("1970-01-01T00:00:00.000Z"), Some(0));
    }

    #[test]
    fn round_trips_a_known_instant() {
        // 2026-09-24T10:52:03Z
        let stamp = iso8601(1_790_247_123, 456);
        assert_eq!(stamp, "2026-09-24T10:52:03.456Z");
        assert_eq!(parse_iso8601(&stamp), Some(1_790_247_123));
    }

    #[test]
    fn accepts_offsets_and_rejects_nonsense() {
        assert_eq!(
            parse_iso8601("2026-09-24T12:00:00+02:00"),
            parse_iso8601("2026-09-24T10:00:00Z")
        );
        assert_eq!(
            parse_iso8601("2026-09-24"),
            parse_iso8601("2026-09-24T00:00:00Z")
        );
        assert!(parse_iso8601("not a date").is_none());
        assert!(parse_iso8601("2026-13-01T00:00:00Z").is_none());
        assert!(parse_iso8601("").is_none());
    }

    #[test]
    fn now_is_well_formed() {
        let now = now_iso8601();
        assert!(parse_iso8601(&now).is_some(), "{now}");
        assert!(now.ends_with('Z'));
    }
}
