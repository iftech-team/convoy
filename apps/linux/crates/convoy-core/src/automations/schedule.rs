//! When an automation runs, in the computer's local time.
//!
//! The core crate has no date dependency, so local time comes from the C
//! library (`localtime_r`, `localtime_s` on Windows), which knows the zone's
//! daylight-saving rules. Everything here takes the zone as a [`Zone`], so the
//! rules are tested with explicit offsets rather than with whatever zone the
//! test machine is in.
//!
//! Times are wall-clock times. One that does not exist on a day — inside a
//! spring-forward gap — runs at the first minute that does. One that happens
//! twice — inside a fall-back hour — runs once, at the first.

use crate::time::{civil_from_days, days_from_civil};
use serde::{Deserialize, Serialize};

/// Seconds since the Unix epoch.
pub type Instant = i64;

/// A time zone: how far ahead of UTC the wall clock is at an instant.
pub trait Zone {
    /// Seconds east of UTC at `at`.
    fn offset(&self, at: Instant) -> i64;
}

/// The computer's own zone.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalZone;

impl Zone for LocalZone {
    fn offset(&self, at: Instant) -> i64 {
        local_offset(at).unwrap_or(0)
    }
}

/// The wall clock read back as if it were UTC, minus the instant itself.
#[cfg(unix)]
fn local_offset(at: Instant) -> Option<i64> {
    let time = at as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call; `localtime_r` is the
    // thread-safe form and writes only into `tm`.
    let filled = unsafe { libc::localtime_r(&time, &mut tm) };
    if filled.is_null() {
        return None;
    }
    Some(wall_seconds(&tm) - at)
}

#[cfg(windows)]
fn local_offset(at: Instant) -> Option<i64> {
    let time = at as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: as above; `localtime_s` is the CRT's thread-safe form.
    let failed = unsafe { libc::localtime_s(&mut tm, &time) };
    if failed != 0 {
        return None;
    }
    Some(wall_seconds(&tm) - at)
}

#[cfg(any(unix, windows))]
fn wall_seconds(tm: &libc::tm) -> i64 {
    days_from_civil(
        tm.tm_year as i64 + 1900,
        tm.tm_mon as i64 + 1,
        tm.tm_mday as i64,
    ) * 86_400
        + tm.tm_hour as i64 * 3600
        + tm.tm_min as i64 * 60
        + tm.tm_sec as i64
}

/// A fixed offset with optional changes, for tests and previews: the offset
/// in force at an instant is the last change at or before it.
#[derive(Debug, Clone, Default)]
pub struct RuleZone {
    pub initial: i64,
    /// `(instant, offset from then on)`, in order.
    pub changes: Vec<(Instant, i64)>,
}

impl Zone for RuleZone {
    fn offset(&self, at: Instant) -> i64 {
        self.changes
            .iter()
            .take_while(|(when, _)| *when <= at)
            .last()
            .map(|(_, offset)| *offset)
            .unwrap_or(self.initial)
    }
}

/// Days of the week, Monday first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Day {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Day {
    pub const ALL: [Day; 7] = [
        Day::Mon,
        Day::Tue,
        Day::Wed,
        Day::Thu,
        Day::Fri,
        Day::Sat,
        Day::Sun,
    ];

    /// The weekday of a day counted from 1970-01-01, a Thursday.
    pub fn of(days: i64) -> Day {
        Day::ALL[(days + 3).rem_euclid(7) as usize]
    }
}

/// When an automation runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Schedule {
    /// Every day at a time.
    Daily { at: String },
    /// On the days picked, at a time.
    Weekdays { at: String, days: Vec<Day> },
    /// From a time of day (midnight unless given), then every so many hours
    /// until midnight; the count starts again each day.
    Every {
        hours: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
}

/// `HH:MM` as minutes past midnight.
pub fn parse_hhmm(value: &str) -> Option<u32> {
    let (hours, minutes) = value.split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    if !hours
        .bytes()
        .chain(minutes.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

impl Schedule {
    /// What is wrong with it, if anything.
    pub fn problem(&self) -> Option<&'static str> {
        match self {
            Schedule::Daily { at } => parse_hhmm(at).is_none().then_some("Pick a time as HH:MM."),
            Schedule::Weekdays { at, days } => {
                if parse_hhmm(at).is_none() {
                    Some("Pick a time as HH:MM.")
                } else if days.is_empty() {
                    Some("Pick at least one day.")
                } else {
                    None
                }
            }
            Schedule::Every { hours, from } => {
                if !(1..=24).contains(hours) {
                    Some("Pick between 1 and 24 hours.")
                } else if from
                    .as_deref()
                    .is_some_and(|from| parse_hhmm(from).is_none())
                {
                    Some("Pick a time as HH:MM.")
                } else {
                    None
                }
            }
        }
    }

    /// The wall-clock minutes it runs at on a day, in order.
    fn minutes_on(&self, day: Day) -> Vec<u32> {
        match self {
            Schedule::Daily { at } => parse_hhmm(at).into_iter().collect(),
            Schedule::Weekdays { at, days } => {
                if days.contains(&day) {
                    parse_hhmm(at).into_iter().collect()
                } else {
                    Vec::new()
                }
            }
            Schedule::Every { hours, from } => {
                let start = from.as_deref().and_then(parse_hhmm).unwrap_or(0);
                let step = (*hours).clamp(1, 24) * 60;
                (0..)
                    .map(|k| start + k * step)
                    .take_while(|minute| *minute < 24 * 60)
                    .collect()
            }
        }
    }
}

/// The local date, as days since 1970-01-01, of an instant.
pub fn local_day(zone: &dyn Zone, at: Instant) -> i64 {
    (at + zone.offset(at)).div_euclid(86_400)
}

/// The instant a wall-clock minute of a local day happens. In a gap, the
/// first minute after it; when it happens twice, the first time.
pub fn resolve(zone: &dyn Zone, day: i64, minute: u32) -> Instant {
    let wall = day * 86_400 + minute as i64 * 60;
    // The offsets in force around that day; at most one change in a day.
    let mut offsets = [
        zone.offset(wall - 86_400),
        zone.offset(wall),
        zone.offset(wall + 86_400),
    ];
    offsets.sort_unstable();
    let mut found: Option<Instant> = None;
    for offset in offsets {
        let candidate = wall - offset;
        if zone.offset(candidate) == offset {
            found = Some(found.map_or(candidate, |earlier: Instant| earlier.min(candidate)));
        }
    }
    if let Some(at) = found {
        return at;
    }
    // A gap: the first whole minute whose wall clock reads at or after it.
    let (low, high) = (wall - offsets[2], wall - offsets[0]);
    let mut at = low.div_euclid(60) * 60;
    while at < high && at + zone.offset(at) < wall {
        at += 60;
    }
    at
}

/// The first time the schedule runs strictly after `after`.
pub fn next_run(schedule: &Schedule, after: Instant, zone: &dyn Zone) -> Option<Instant> {
    if schedule.problem().is_some() {
        return None;
    }
    let today = local_day(zone, after);
    // A day either side covers an offset change; eight days reach the next
    // of any weekday.
    for day in today - 1..=today + 8 {
        let mut best: Option<Instant> = None;
        for minute in schedule.minutes_on(Day::of(day)) {
            let at = resolve(zone, day, minute);
            if at > after {
                best = Some(best.map_or(at, |earlier| earlier.min(at)));
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

/// The next `count` runs after `after`.
pub fn next_runs(
    schedule: &Schedule,
    after: Instant,
    count: usize,
    zone: &dyn Zone,
) -> Vec<Instant> {
    let mut runs = Vec::with_capacity(count);
    let mut from = after;
    while runs.len() < count {
        let Some(next) = next_run(schedule, from, zone) else {
            break;
        };
        runs.push(next);
        from = next;
    }
    runs
}

/// `2026-10-11 09:00` in the zone, for a session's title.
pub fn local_stamp(zone: &dyn Zone, at: Instant) -> String {
    let wall = at + zone.offset(at);
    let (year, month, day) = civil_from_days(wall.div_euclid(86_400));
    let rest = wall.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60
    )
}

/// `20261011-0900`, for a branch name.
pub fn compact_stamp(zone: &dyn Zone, at: Instant) -> String {
    local_stamp(zone, at)
        .replace(['-', ':'], "")
        .replace(' ', "-")
}

/// The local wall time of a date, for tests and callers that build one.
pub fn wall(year: i64, month: i64, day: i64) -> i64 {
    days_from_civil(year, month, day)
}
