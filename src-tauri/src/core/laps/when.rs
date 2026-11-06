//! When a job's moments are, on this Mac's clock (chrono `Local`): the next one after a moment,
//! the next few for the dialog, a day's, and the days taken out ("Bundan sonra her <gün>").
//! A nonexistent local time (DST forward) moves to the next valid minute; an ambiguous one (DST
//! back) runs at its first occurrence.
//!
//! Every moment is worked out from the wall clock of its own day, never by adding an interval to
//! the last one, so a clock change or a DST night moves no later moment. The work is done against
//! any `chrono::TimeZone` (`Local` in the app), which lets the tests run a DST night of their own.

use chrono::{Datelike, Local, NaiveDate, TimeDelta, TimeZone, Utc};

use super::cron::Cron;
use super::types::{LapKind, When};

/// The smallest interval: a script job's, a job with an agent's (minutes).
pub const MIN_SCRIPT_MINUTES: u32 = 1;
pub const MIN_AGENT_MINUTES: u32 = 15;

const DAY_MINUTES: u32 = 24 * 60;

/// How far ahead a moment is looked for, in days: five years, so a 29 February is always found,
/// and a form that never comes round (a 30 February) ends the search instead of running on.
const HORIZON_DAYS: usize = 5 * 366;

const TIME: &str = "laps.when.error.time";
const INTERVAL: &str = "laps.when.error.interval";
const INTERVAL_SCRIPT: &str = "laps.when.error.interval.script";
const LONG: &str = "laps.when.error.long";
const WINDOW: &str = "laps.when.error.window";
const DAYS: &str = "laps.when.error.days";
const PAST: &str = "laps.when.error.past";
const OFTEN: &str = "laps.when.error.often";
const NEVER: &str = "laps.when.error.never";
const REFILL: &str = "laps.when.error.refill";

/// Why `when` is refused for a job of `kind`, as a catalog key (`laps.when.error.*`); none: fine.
///
/// Refused: a time that isn't `HH:MM`; an interval below the kind's smallest
/// (`MIN_AGENT_MINUTES` with an agent, `MIN_SCRIPT_MINUTES` without) or above a day; a window
/// that starts after it ends; a day outside 1–7; a `Once` that isn't after `now`; `Refill` for a
/// job without an agent (it has no agent to refill); a cron that doesn't parse; a form with no
/// moment in the next five years (`HORIZON_DAYS`: a 30 February); and, with an agent, a form
/// whose moments can come closer than its smallest interval (a cron's, or an interval's from a
/// day's last moment to the next day's first).
pub fn refused(when: &When, kind: LapKind, now: u64) -> Option<&'static str> {
    refused_in(&Local, when, kind, now)
}

/// `key`, one of `refused`'s, says only that the schedule's time is past or never comes again,
/// nothing about its form: a moment that already came (a run's) is no less right for it.
pub fn about_time(key: &str) -> bool {
    key == PAST || key == NEVER
}

/// The first moment strictly after `after` (ms), none if there is none (a `Once` past, `Refill`:
/// the caller knows the refill) or it is more than five years away. `days_off`: dates
/// (`YYYY-MM-DD`) skipped.
pub fn next_after(when: &When, after: u64, days_off: &[String]) -> Option<u64> {
    next_in(&Local, when, after, days_off)
}

/// The next `n` moments after `after`.
pub fn upcoming(when: &When, after: u64, days_off: &[String], n: usize) -> Vec<u64> {
    upcoming_in(&Local, when, after, days_off, n)
}

/// Every moment on the local date `date` (`YYYY-MM-DD`), days off ignored; none for `Refill` or a
/// date that doesn't read.
pub fn on_day(when: &When, date: &str) -> Vec<u64> {
    parse_date(date).map(|date| on_day_in(&Local, when, date)).unwrap_or_default()
}

/// `when` without the weekday `day` (1 Monday – 7 Sunday); none when its form has no weekdays to
/// take one out of (`Once`, `Refill`), it doesn't run on that day anyway, or it would leave no day
/// at all. A cron whose day-of-month field is restricted gives none too: cron runs the dates that
/// field names whatever their weekday (`Cron::without_weekday`).
pub fn without_weekday(when: &When, day: u8) -> Option<When> {
    if !has_weekday(when, day) {
        return None;
    }
    match when {
        When::Daily { at, days } => Some(When::Daily { at: at.clone(), days: without(days, day)? }),
        When::Every { minutes, from, to, days } => Some(When::Every {
            minutes: *minutes,
            from: from.clone(),
            to: to.clone(),
            days: without(days, day)?,
        }),
        When::Cron { expr } => Some(When::Cron { expr: Cron::parse(expr).ok()?.without_weekday(expr, day)? }),
        When::Once { .. } | When::Refill => None,
    }
}

/// `when` with the weekday `day` back. A list of days that holds all seven again goes back to
/// empty (every day), a cron's day-of-week field to `*`.
pub fn with_weekday(when: &When, day: u8) -> When {
    if !(1..=7).contains(&day) {
        return when.clone();
    }
    match when {
        When::Daily { at, days } => When::Daily { at: at.clone(), days: with(days, day) },
        When::Every { minutes, from, to, days } => When::Every {
            minutes: *minutes,
            from: from.clone(),
            to: to.clone(),
            days: with(days, day),
        },
        When::Cron { expr } => match Cron::parse(expr) {
            Ok(cron) => When::Cron { expr: cron.with_weekday(expr, day) },
            Err(_) => when.clone(),
        },
        When::Once { .. } | When::Refill => when.clone(),
    }
}

/// Whether `when` runs on weekday `day` at all. A form with no weekdays (`Once`, `Refill`) is
/// kept from none, so it says yes; a cron says what its day-of-week field takes.
pub fn has_weekday(when: &When, day: u8) -> bool {
    if !(1..=7).contains(&day) {
        return false;
    }
    match when {
        When::Daily { days, .. } | When::Every { days, .. } => on_weekday(days, day),
        When::Cron { expr } => Cron::parse(expr).is_ok_and(|cron| cron.has_weekday(day)),
        When::Once { .. } | When::Refill => true,
    }
}

/// The local date (`YYYY-MM-DD`) and weekday (1–7) of `ms`.
pub fn date_of(ms: u64) -> (String, u8) {
    let date = date_in(&Local, ms).or_else(|| date_in(&Utc, ms)).unwrap_or_default();
    (name(date), weekday(date))
}

/* ---------- the work, against any time zone ---------- */

/// A repeating form as the clock has it: the minutes of a day it runs at (0–1439, in their order)
/// and which days it runs on.
struct Plan {
    minutes: Vec<u32>,
    days: Days,
}

enum Days {
    /// 1 Monday – 7 Sunday; empty: every day.
    Weekdays(Vec<u8>),
    Cron(Cron),
}

impl Plan {
    /// None for `Once` and `Refill`, and for a form that doesn't read (a bad time, an interval of
    /// 0, a cron that doesn't parse), which has no moment at all.
    fn of(when: &When) -> Option<Self> {
        match when {
            When::Daily { at, days } => Some(Self { minutes: vec![minute_of(at)?], days: Days::Weekdays(days.clone()) }),
            When::Every { minutes, from, to, days } => {
                let (first, last) = window(from.as_deref(), to.as_deref())?;
                let step = usize::try_from(*minutes).ok().filter(|&step| step > 0)?;
                Some(Self { minutes: (first..=last).step_by(step).collect(), days: Days::Weekdays(days.clone()) })
            }
            When::Cron { expr } => {
                let cron = Cron::parse(expr).ok()?;
                Some(Self { minutes: cron.minutes_of_day(), days: Days::Cron(cron) })
            }
            When::Once { .. } | When::Refill => None,
        }
    }

    fn runs_on(&self, date: NaiveDate) -> bool {
        match &self.days {
            Days::Weekdays(days) => on_weekday(days, weekday(date)),
            Days::Cron(cron) => cron.runs_on(date.day(), date.month(), u32::from(weekday(date))),
        }
    }

    /// Its moments on `date` in `zone`, in their order, each once (DST forward can bring several
    /// minutes to the same one).
    fn moments<Z: TimeZone>(&self, zone: &Z, date: NaiveDate) -> Vec<u64> {
        if !self.runs_on(date) {
            return Vec::new();
        }
        let mut moments: Vec<u64> = self.minutes.iter().filter_map(|&minute| instant(zone, date, minute)).collect();
        moments.sort_unstable();
        moments.dedup();
        moments
    }

    /// Its first moment on `date` in `zone` after `after`. A day's minutes fall on moments in
    /// their order (a skipped time moves forward, a repeated one is its first), so the first
    /// minute past `after` is found by halving, not by working out all 1440 of an every-minute
    /// cron.
    fn first_after<Z: TimeZone>(&self, zone: &Z, date: NaiveDate, after: u64) -> Option<u64> {
        if !self.runs_on(date) {
            return None;
        }
        let past = self.minutes.partition_point(|&minute| instant(zone, date, minute).is_none_or(|at| at <= after));
        self.minutes[past..].iter().find_map(|&minute| instant(zone, date, minute).filter(|&at| at > after))
    }

    /// The shortest time between two of its moments, in minutes of the clock (a DST night aside):
    /// between two of a day's, or from a day's last to the next day's first when two days in a row
    /// run (looked for over the horizon from `now`).
    fn shortest_gap<Z: TimeZone>(&self, zone: &Z, now: u64) -> u32 {
        let (Some(&first), Some(&last)) = (self.minutes.first(), self.minutes.last()) else {
            return DAY_MINUTES;
        };
        let within = self.minutes.windows(2).map(|pair| pair[1] - pair[0]).min().unwrap_or(DAY_MINUTES);
        let across = DAY_MINUTES - last + first;
        if across >= within {
            return within;
        }
        let Some(today) = date_in(zone, now) else {
            return within;
        };
        let mut yesterday = false;
        for date in today.iter_days().take(HORIZON_DAYS) {
            let runs = self.runs_on(date);
            if runs && yesterday {
                return across;
            }
            yesterday = runs;
        }
        within
    }
}

fn refused_in<Z: TimeZone>(zone: &Z, when: &When, kind: LapKind, now: u64) -> Option<&'static str> {
    let floor = if kind.has_agent() { MIN_AGENT_MINUTES } else { MIN_SCRIPT_MINUTES };
    match when {
        When::Daily { at, days } => {
            if minute_of(at).is_none() {
                return Some(TIME);
            }
            if !days_valid(days) {
                return Some(DAYS);
            }
        }
        When::Every { minutes, from, to, days } => {
            let Some((first, last)) = window(from.as_deref(), to.as_deref()) else {
                return Some(TIME);
            };
            if first > last {
                return Some(WINDOW);
            }
            if *minutes < floor {
                return Some(if kind.has_agent() { INTERVAL } else { INTERVAL_SCRIPT });
            }
            if *minutes > DAY_MINUTES {
                return Some(LONG);
            }
            if !days_valid(days) {
                return Some(DAYS);
            }
        }
        When::Once { at } => return (*at <= now).then_some(PAST),
        When::Refill => return (!kind.has_agent()).then_some(REFILL),
        When::Cron { expr } => {
            if let Err(key) = Cron::parse(expr) {
                return Some(key);
            }
        }
    }
    let Some(plan) = Plan::of(when) else {
        return Some(NEVER);
    };
    if next_in(zone, when, now, &[]).is_none() {
        return Some(NEVER);
    }
    if kind.has_agent() && plan.shortest_gap(zone, now) < floor {
        return Some(OFTEN);
    }
    None
}

fn next_in<Z: TimeZone>(zone: &Z, when: &When, after: u64, days_off: &[String]) -> Option<u64> {
    let off: Vec<NaiveDate> = days_off.iter().filter_map(|date| parse_date(date)).collect();
    if let When::Once { at } = when {
        let is_off = date_in(zone, *at).is_some_and(|date| off.contains(&date));
        return (*at > after && !is_off).then_some(*at);
    }
    let plan = Plan::of(when)?;
    // From the day before `after`'s: one of its moments may have moved past midnight.
    let first = date_in(zone, after)?.pred_opt()?;
    first
        .iter_days()
        .take(HORIZON_DAYS + 2)
        .filter(|date| !off.contains(date))
        .find_map(|date| plan.first_after(zone, date, after))
}

fn upcoming_in<Z: TimeZone>(zone: &Z, when: &When, after: u64, days_off: &[String], n: usize) -> Vec<u64> {
    let mut moments = Vec::with_capacity(n.min(64));
    let mut from = after;
    while moments.len() < n {
        let Some(at) = next_in(zone, when, from, days_off) else {
            break;
        };
        moments.push(at);
        from = at;
    }
    moments
}

fn on_day_in<Z: TimeZone>(zone: &Z, when: &When, date: NaiveDate) -> Vec<u64> {
    match when {
        When::Once { at } if date_in(zone, *at) == Some(date) => vec![*at],
        _ => Plan::of(when).map(|plan| plan.moments(zone, date)).unwrap_or_default(),
    }
}

/// The moment (ms) the local `minute` (0–1439) of `date` is in `zone`: a time the clocks skip
/// (DST forward) is the next minute that exists, one they pass twice (DST back) its first
/// occurrence.
fn instant<Z: TimeZone>(zone: &Z, date: NaiveDate, minute: u32) -> Option<u64> {
    let mut at = date.and_hms_opt(minute / 60, minute % 60, 0)?;
    for _ in 0..=DAY_MINUTES {
        if let Some(time) = zone.from_local_datetime(&at).earliest() {
            return u64::try_from(time.timestamp_millis()).ok();
        }
        at = at.checked_add_signed(TimeDelta::minutes(1))?;
    }
    None
}

fn date_in<Z: TimeZone>(zone: &Z, ms: u64) -> Option<NaiveDate> {
    Some(zone.timestamp_millis_opt(i64::try_from(ms).ok()?).single()?.date_naive())
}

/// `HH:MM` as a minute of the day; none unless it is two digits, a colon and two digits of a time
/// that exists (`08:30`, not `8:30` or `24:00`).
fn minute_of(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' || ![0, 1, 3, 4].iter().all(|&i| bytes[i].is_ascii_digit()) {
        return None;
    }
    let digit = |i: usize| u32::from(bytes[i] - b'0');
    let (hour, minute) = (digit(0) * 10 + digit(1), digit(3) * 10 + digit(4));
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// An interval's window as minutes of the day: `from` (none: 00:00) to `to` (none: 23:59); none
/// when either is given and isn't a time.
fn window(from: Option<&str>, to: Option<&str>) -> Option<(u32, u32)> {
    let first = from.map_or(Some(0), minute_of)?;
    let last = to.map_or(Some(DAY_MINUTES - 1), minute_of)?;
    Some((first, last))
}

/// `YYYY-MM-DD`, as the core writes a day off.
fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' || !digits(0..4) || !digits(5..7) || !digits(8..10) {
        return None;
    }
    NaiveDate::from_ymd_opt(text[0..4].parse().ok()?, text[5..7].parse().ok()?, text[8..10].parse().ok()?)
}

fn name(date: NaiveDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

/// 1 Monday – 7 Sunday.
fn weekday(date: NaiveDate) -> u8 {
    u8::try_from(date.weekday().number_from_monday()).unwrap_or(1)
}

fn on_weekday(days: &[u8], day: u8) -> bool {
    days.is_empty() || days.contains(&day)
}

fn days_valid(days: &[u8]) -> bool {
    days.iter().all(|day| (1..=7).contains(day))
}

/// The days of `days` (empty: all seven) without `day`, in their order; none when none is left.
fn without(days: &[u8], day: u8) -> Option<Vec<u8>> {
    let left: Vec<u8> = (1..=7).filter(|&other| other != day && on_weekday(days, other)).collect();
    (!left.is_empty()).then_some(left)
}

/// The days of `days` with `day`, in their order; empty again when that makes all seven.
fn with(days: &[u8], day: u8) -> Vec<u8> {
    let back: Vec<u8> = (1..=7).filter(|&other| other == day || on_weekday(days, other)).collect();
    if back.len() == 7 {
        Vec::new()
    } else {
        back
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, MappedLocalTime, NaiveDateTime};

    use super::*;

    /// A clock with no DST (+03:00, as Istanbul's), so the moments don't hang on the machine's.
    fn plain() -> FixedOffset {
        FixedOffset::east_opt(3 * 3600).unwrap()
    }

    /// Berlin's clock in 2026, made by hand (no time-zone crate): CET (+1), and CEST (+2) from
    /// 29 March 01:00 UTC to 25 October 01:00 UTC.
    #[derive(Clone, Copy)]
    struct Berlin;

    impl Berlin {
        fn offset(hours: i32) -> FixedOffset {
            FixedOffset::east_opt(hours * 3600).unwrap()
        }
    }

    impl TimeZone for Berlin {
        type Offset = FixedOffset;

        fn from_offset(_: &FixedOffset) -> Self {
            Berlin
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<FixedOffset> {
            let fits = |hours: i32| {
                let offset = Self::offset(hours);
                (self.offset_from_utc_datetime(&(*local - TimeDelta::hours(i64::from(hours)))) == offset).then_some(offset)
            };
            match (fits(2), fits(1)) {
                (Some(summer), Some(winter)) => MappedLocalTime::Ambiguous(summer, winter),
                (Some(one), None) | (None, Some(one)) => MappedLocalTime::Single(one),
                (None, None) => MappedLocalTime::None,
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            let start = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap().and_hms_opt(1, 0, 0).unwrap();
            let end = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(1, 0, 0).unwrap();
            Self::offset(if (start..end).contains(utc) { 2 } else { 1 })
        }
    }

    /// `YYYY-MM-DD HH:MM` on `zone`'s clock, as ms (its first occurrence).
    fn at<Z: TimeZone>(zone: &Z, text: &str) -> u64 {
        let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap();
        u64::try_from(zone.from_local_datetime(&naive).earliest().unwrap().timestamp_millis()).unwrap()
    }

    fn daily(at: &str, days: &[u8]) -> When {
        When::Daily { at: at.into(), days: days.to_vec() }
    }

    fn every(minutes: u32, from: Option<&str>, to: Option<&str>, days: &[u8]) -> When {
        When::Every { minutes, from: from.map(Into::into), to: to.map(Into::into), days: days.to_vec() }
    }

    fn cron(expr: &str) -> When {
        When::Cron { expr: expr.into() }
    }

    fn date(text: &str) -> NaiveDate {
        parse_date(text).unwrap()
    }

    /// A wrong moment runs an agent when nobody asked for it and spends quota, or never runs the
    /// job; nothing says so. Each form's next moment, on a clock without DST.
    #[test]
    fn the_next_moment_of_every_form() {
        let zone = plain();
        let local = |text: &str| at(&zone, text);
        let once = |text: &str| When::Once { at: local(text) };
        let off = |dates: &[&str]| dates.iter().map(|date| date.to_string()).collect::<Vec<_>>();
        let window = every(240, Some("09:00"), Some("21:00"), &[]);

        // (what, the form, after, the days off, the next moment); 2026-11-04 is a Wednesday.
        type Case = (&'static str, When, &'static str, Vec<String>, Option<&'static str>);
        let cases: Vec<Case> = vec![
            ("daily, later today", daily("08:30", &[]), "2026-11-04 08:00", vec![], Some("2026-11-04 08:30")),
            ("daily, strictly after", daily("08:30", &[]), "2026-11-04 08:30", vec![], Some("2026-11-05 08:30")),
            ("weekdays, from a Friday", daily("08:30", &[1, 2, 3, 4, 5]), "2026-11-06 09:00", vec![], Some("2026-11-09 08:30")),
            ("daily, a day off", daily("08:30", &[]), "2026-11-04 09:00", off(&["2026-11-05"]), Some("2026-11-06 08:30")),
            ("every 4 h, inside the window", window.clone(), "2026-11-04 13:00", vec![], Some("2026-11-04 17:00")),
            ("every 4 h, the window's last moment", window.clone(), "2026-11-04 17:00", vec![], Some("2026-11-04 21:00")),
            ("every 4 h, past the window", window.clone(), "2026-11-04 21:00", vec![], Some("2026-11-05 09:00")),
            ("every 4 h, before the window", window.clone(), "2026-11-04 03:00", vec![], Some("2026-11-04 09:00")),
            (
                "every 4 h, a window ending before a step",
                every(240, Some("09:00"), Some("20:59"), &[]),
                "2026-11-04 17:00",
                vec![],
                Some("2026-11-05 09:00"),
            ),
            ("every 30 min, the whole day", every(30, None, None, &[]), "2026-11-04 23:45", vec![], Some("2026-11-05 00:00")),
            ("every 45 min from 22:00", every(45, Some("22:00"), None, &[]), "2026-11-04 23:00", vec![], Some("2026-11-04 23:30")),
            ("every 45 min from 22:00, next day", every(45, Some("22:00"), None, &[]), "2026-11-04 23:30", vec![], Some("2026-11-05 22:00")),
            ("every, weekend only", every(60, Some("10:00"), Some("12:00"), &[6, 7]), "2026-11-04 11:00", vec![], Some("2026-11-07 10:00")),
            ("every, a day off", every(60, Some("10:00"), Some("12:00"), &[]), "2026-11-04 12:00", off(&["2026-11-05"]), Some("2026-11-06 10:00")),
            ("every 0 min never runs", every(0, None, None, &[]), "2026-11-04 12:00", vec![], None),
            ("once, ahead", once("2026-11-20 07:15"), "2026-11-04 09:00", vec![], Some("2026-11-20 07:15")),
            ("once, past", once("2026-11-01 07:15"), "2026-11-04 09:00", vec![], None),
            ("once, its own moment", once("2026-11-20 07:15"), "2026-11-20 07:15", vec![], None),
            ("once, its day off", once("2026-11-20 07:15"), "2026-11-04 09:00", off(&["2026-11-20"]), None),
            ("refill: the caller's", When::Refill, "2026-11-04 09:00", vec![], None),
            ("cron, steps and ranges", cron("*/15 9-17 * * 1-5"), "2026-11-06 17:50", vec![], Some("2026-11-09 09:00")),
            ("cron, steps inside the hours", cron("*/15 9-17 * * 1-5"), "2026-11-04 10:01", vec![], Some("2026-11-04 10:15")),
            ("cron, names in any case", cron("0 9 * * mon,wed,FRI"), "2026-11-04 09:00", vec![], Some("2026-11-06 09:00")),
            ("cron, days of the month", cron("30 8 1,15 * *"), "2026-11-15 09:00", vec![], Some("2026-12-01 08:30")),
            ("cron, a step from a value", cron("5/20 * * * *"), "2026-11-04 10:06", vec![], Some("2026-11-04 10:25")),
            ("cron, a stepped range", cron("1-30/10 * * * *"), "2026-11-04 10:22", vec![], Some("2026-11-04 11:01")),
            ("cron, 7 is Sunday", cron("0 10 * * 7"), "2026-11-04 12:00", vec![], Some("2026-11-08 10:00")),
            ("cron, 0 is Sunday", cron("0 10 * * 0"), "2026-11-04 12:00", vec![], Some("2026-11-08 10:00")),
            ("cron, a weekday range to 7", cron("0 10 * * 6-7"), "2026-11-08 10:00", vec![], Some("2026-11-14 10:00")),
            ("cron, both day fields: a date", cron("0 12 10 * 5"), "2026-11-07 00:00", vec![], Some("2026-11-10 12:00")),
            ("cron, both day fields: a weekday", cron("0 12 10 * 5"), "2026-11-10 12:00", vec![], Some("2026-11-13 12:00")),
            ("cron, a `*/` day field wants both", cron("0 12 */10 * 5"), "2026-11-04 00:00", vec![], Some("2026-12-11 12:00")),
            ("cron, the 31st skips short months", cron("0 0 31 * *"), "2026-11-04 00:00", vec![], Some("2026-12-31 00:00")),
            ("cron, 29 February", cron("0 9 29 2 *"), "2026-11-04 00:00", vec![], Some("2028-02-29 09:00")),
            ("cron, 30 February never", cron("0 0 30 2 *"), "2026-11-04 00:00", vec![], None),
            ("cron, month names", cron("0 6 1 jan,JUL *"), "2026-11-04 00:00", vec![], Some("2027-01-01 06:00")),
            ("cron, the year's last minute", cron("59 23 31 12 *"), "2026-12-31 23:59", vec![], Some("2027-12-31 23:59")),
            ("cron, a day off", cron("0 9 * * *"), "2026-11-04 10:00", off(&["2026-11-05"]), Some("2026-11-06 09:00")),
            ("cron that doesn't parse", cron("0 9 * *"), "2026-11-04 10:00", vec![], None),
        ];

        for (what, when, after, days_off, next) in cases {
            assert_eq!(next_in(&zone, &when, local(after), &days_off), next.map(local), "{what}");
        }

        // The dialog's next four, and a day's moments.
        let hours = |times: &[&str]| times.iter().map(|time| local(&format!("2026-11-04 {time}"))).collect::<Vec<_>>();
        assert_eq!(upcoming_in(&zone, &window, local("2026-11-04 08:00"), &[], 4), hours(&["09:00", "13:00", "17:00", "21:00"]));
        assert_eq!(on_day_in(&zone, &window, date("2026-11-04")), hours(&["09:00", "13:00", "17:00", "21:00"]));
        assert_eq!(on_day_in(&zone, &cron("0 9 * * 1-5"), date("2026-11-08")), Vec::<u64>::new());
        assert_eq!(on_day_in(&zone, &once("2026-11-04 07:15"), date("2026-11-04")), hours(&["07:15"]));
        assert!(on_day_in(&zone, &When::Refill, date("2026-11-04")).is_empty());
    }

    /// DST forward: a time the clocks skip runs at the next minute that exists, once; DST back: a
    /// time that comes twice runs at its first only. A second run would be an agent's session
    /// nobody asked for.
    #[test]
    fn a_dst_night_moves_skipped_times_on_and_runs_repeated_ones_once() {
        let zone = Berlin;
        let utc = |text: &str| at(&Utc, text);
        let utcs = |texts: &[&str]| texts.iter().map(|text| utc(text)).collect::<Vec<_>>();

        // 29 March 2026: 02:00–02:59 don't exist; 03:00 CEST is 01:00 UTC.
        assert_eq!(next_in(&zone, &daily("02:30", &[]), utc("2026-03-28 23:00"), &[]), Some(utc("2026-03-29 01:00")));
        assert_eq!(next_in(&zone, &daily("02:30", &[]), utc("2026-03-29 01:00"), &[]), Some(utc("2026-03-30 00:30")));
        let morning = every(30, Some("01:00"), Some("04:00"), &[]);
        assert_eq!(
            on_day_in(&zone, &morning, date("2026-03-29")),
            utcs(&["2026-03-29 00:00", "2026-03-29 00:30", "2026-03-29 01:00", "2026-03-29 01:30", "2026-03-29 02:00"])
        );
        assert_eq!(on_day_in(&zone, &cron("0 * * * *"), date("2026-03-29")).len(), 23);

        // 25 October 2026: 02:00–02:59 come twice, first in CEST (00:00–00:59 UTC).
        assert_eq!(next_in(&zone, &daily("02:30", &[]), utc("2026-10-24 22:00"), &[]), Some(utc("2026-10-25 00:30")));
        assert_eq!(next_in(&zone, &daily("02:30", &[]), utc("2026-10-25 00:30"), &[]), Some(utc("2026-10-26 01:30")));
        assert_eq!(next_in(&zone, &cron("*/30 * * * *"), utc("2026-10-25 00:30"), &[]), Some(utc("2026-10-25 02:00")));
        assert_eq!(
            upcoming_in(&zone, &cron("*/30 * * * *"), utc("2026-10-24 23:45"), &[], 4),
            utcs(&["2026-10-25 00:00", "2026-10-25 00:30", "2026-10-25 02:00", "2026-10-25 02:30"])
        );
        assert_eq!(on_day_in(&zone, &cron("0 * * * *"), date("2026-10-25")).len(), 24);
        assert_eq!(date_in(&zone, utc("2026-10-24 22:30")), Some(date("2026-10-25")));
    }

    /// What the dialog and `lap_save` refuse: a schedule let through runs an agent more often than
    /// the floor, or never, without a word.
    #[test]
    fn a_schedule_is_refused_with_its_reason() {
        use LapKind::{Agent, Script, ScriptAgent};

        let zone = plain();
        let now = at(&zone, "2026-11-04 12:00");
        let once = |ms: u64| When::Once { at: ms };

        let cases: Vec<(&str, When, LapKind, Option<&str>)> = vec![
            ("daily", daily("08:30", &[]), Agent, None),
            ("weekdays", daily("08:30", &[1, 2, 3, 4, 5]), Agent, None),
            ("an hour of one digit", daily("8:30", &[]), Agent, Some(TIME)),
            ("hour 24", daily("24:00", &[]), Agent, Some(TIME)),
            ("minute 60", daily("09:60", &[]), Agent, Some(TIME)),
            ("not a time", daily("ab:cd", &[]), Script, Some(TIME)),
            ("day 0", daily("09:00", &[0]), Agent, Some(DAYS)),
            ("day 8", every(60, None, None, &[1, 8]), Agent, Some(DAYS)),
            ("every 15 min with an agent", every(15, None, None, &[]), Agent, None),
            ("every 10 min with an agent", every(10, None, None, &[]), ScriptAgent, Some(INTERVAL)),
            ("every 10 min, a script", every(10, None, None, &[]), Script, None),
            ("every minute, a script", every(1, None, None, &[]), Script, None),
            ("every 0 min, a script", every(0, None, None, &[]), Script, Some(INTERVAL_SCRIPT)),
            ("every 24 h", every(1440, Some("09:00"), None, &[]), Agent, None),
            ("every 25 h", every(1500, None, None, &[]), Agent, Some(LONG)),
            ("a window backwards", every(60, Some("21:00"), Some("09:00"), &[]), Agent, Some(WINDOW)),
            ("a window of one moment", every(60, Some("09:00"), Some("09:00"), &[]), Agent, None),
            ("a window's bad time", every(60, Some("9:00"), None, &[]), Agent, Some(TIME)),
            ("every 17 min: 12 min over midnight", every(17, None, None, &[]), Agent, Some(OFTEN)),
            ("every 17 min, a script", every(17, None, None, &[]), Script, None),
            ("every 17 min, no two days in a row", every(17, None, None, &[1, 3, 5]), Agent, None),
            ("every 17 min, Sunday then Monday", every(17, None, None, &[7, 1]), Agent, Some(OFTEN)),
            ("every 1439 min: a minute over midnight", every(1439, None, None, &[]), Agent, Some(OFTEN)),
            ("once ahead", once(now + 60_000), Agent, None),
            ("once now", once(now), Agent, Some(PAST)),
            ("once past", once(now - 60_000), Script, Some(PAST)),
            ("refill with an agent", When::Refill, Agent, None),
            ("refill, a script", When::Refill, Script, Some(REFILL)),
            ("cron", cron("*/15 9-17 * * mon-fri"), Agent, None),
            ("cron, four fields", cron("* * * *"), Script, Some("laps.when.error.cron")),
            ("cron, six fields", cron("0 * * * * *"), Script, Some("laps.when.error.cron")),
            ("cron, minute 60", cron("60 * * * *"), Script, Some("laps.when.error.cron")),
            ("cron, hour 24", cron("0 24 * * *"), Script, Some("laps.when.error.cron")),
            ("cron, day 0", cron("0 0 0 * *"), Script, Some("laps.when.error.cron")),
            ("cron, month 13", cron("0 0 * 13 *"), Script, Some("laps.when.error.cron")),
            ("cron, weekday 8", cron("0 0 * * 8"), Script, Some("laps.when.error.cron")),
            ("cron, a range backwards", cron("0 0 * * fri-mon"), Script, Some("laps.when.error.cron")),
            ("cron, a step of 0", cron("*/0 * * * *"), Script, Some("laps.when.error.cron")),
            ("cron, an empty item", cron("0 0 1,,2 * *"), Script, Some("laps.when.error.cron")),
            ("cron, a name out of its field", cron("0 0 mon * *"), Script, Some("laps.when.error.cron")),
            ("cron, a whole name", cron("0 0 * * monday"), Script, Some("laps.when.error.cron")),
            ("cron, a macro", cron("@daily"), Script, Some("laps.when.error.cron")),
            ("cron, Quartz's ?", cron("0 0 ? * *"), Script, Some("laps.when.error.cron")),
            ("cron, every 5 min with an agent", cron("*/5 * * * *"), Agent, Some(OFTEN)),
            ("cron, every 5 min, a script", cron("*/5 * * * *"), Script, None),
            ("cron, every minute, a script", cron("* * * * *"), Script, None),
            ("cron, 10 min apart in an hour", cron("0,10 9 * * *"), Agent, Some(OFTEN)),
            ("cron, 10 min apart over midnight", cron("5,55 0,23 * * *"), Agent, Some(OFTEN)),
            ("cron, over midnight but on Mondays only", cron("5,55 0,23 * * 1"), Agent, None),
            ("cron, 30 February", cron("0 0 30 2 *"), Agent, Some(NEVER)),
            ("cron, the 31st of short months", cron("0 0 31 4,6,9,11 *"), Script, Some(NEVER)),
            ("cron, every minute of 30 February", cron("* * 30 2 *"), Agent, Some(NEVER)),
            ("cron, 29 February", cron("0 9 29 2 *"), Agent, None),
        ];

        for (what, when, kind, reason) in cases {
            assert_eq!(refused_in(&zone, &when, kind, now), reason, "{what}");
        }
    }

    /// "Bundan sonra her <gün>" and "geri aç": taking a weekday out of a form and putting it back
    /// must give the same moments again, and the day must really stop running. A cron whose dates
    /// would still run on that day is left alone (none), never rewritten into one that runs more.
    #[test]
    fn a_weekday_taken_out_and_put_back() {
        let zone = plain();
        let local = |text: &str| at(&zone, text);
        // Four weeks of moments, from Monday 2 November.
        let soon = |when: &When| {
            date("2026-11-02").iter_days().take(28).flat_map(|day| on_day_in(&zone, when, day)).collect::<Vec<_>>()
        };

        // (the form, the weekday, what it is without it)
        let cases: Vec<(When, u8, Option<When>)> = vec![
            (daily("08:30", &[]), 3, Some(daily("08:30", &[1, 2, 4, 5, 6, 7]))),
            (daily("08:30", &[1, 2, 3, 4, 5]), 5, Some(daily("08:30", &[1, 2, 3, 4]))),
            (daily("08:30", &[3]), 3, None),
            (daily("08:30", &[1, 2]), 3, None),
            (every(240, Some("09:00"), Some("21:00"), &[]), 7, Some(every(240, Some("09:00"), Some("21:00"), &[1, 2, 3, 4, 5, 6]))),
            (every(60, None, None, &[6, 7]), 6, Some(every(60, None, None, &[7]))),
            (cron("*/30 9-17 * * *"), 3, Some(cron("*/30 9-17 * * 0,1,2,4,5,6"))),
            (cron("0 9 * * mon-fri"), 1, Some(cron("0 9 * * 2,3,4,5"))),
            (cron("0  9 */2 *  *"), 7, Some(cron("0 9 */2 * 1,2,3,4,5,6"))),
            (cron("0 9 * * 7"), 7, None),
            (cron("0 9 * * 1-5"), 6, None),
            (cron("0 9 1,15 * *"), 3, None),
            (cron("0 9 1 * mon"), 1, None),
            (When::Once { at: local("2026-11-04 09:00") }, 3, None),
            (When::Refill, 3, None),
        ];

        for (when, day, without) in cases {
            assert_eq!(without_weekday(&when, day), without, "{when:?} without {day}");
            let Some(without) = without else { continue };

            assert!(has_weekday(&when, day) && !has_weekday(&without, day), "{when:?} without {day}");
            let moments = soon(&without);
            assert!(!moments.is_empty(), "{without:?}");
            assert!(moments.iter().all(|&ms| weekday(date_in(&zone, ms).unwrap()) != day), "{without:?} still runs on {day}");
            assert_eq!(soon(&when).into_iter().filter(|&ms| weekday(date_in(&zone, ms).unwrap()) != day).collect::<Vec<_>>(), moments);

            let back = with_weekday(&without, day);
            assert!(has_weekday(&back, day), "{back:?}");
            assert_eq!(soon(&back), soon(&when), "{when:?}: {back:?}");
        }

        // A list or a field that holds every day again is written as every day.
        assert_eq!(with_weekday(&daily("08:30", &[1, 2, 4, 5, 6, 7]), 3), daily("08:30", &[]));
        assert_eq!(with_weekday(&cron("*/30 9-17 * * 0,1,2,4,5,6"), 3), cron("*/30 9-17 * * *"));
        // Where the dates field is restricted, `*` would mean "those dates alone": the list stays.
        assert_eq!(with_weekday(&cron("0 9 1 * 0-5"), 6), cron("0 9 1 * 0,1,2,3,4,5,6"));
        assert_eq!(with_weekday(&daily("08:30", &[]), 3), daily("08:30", &[]));
        assert!(!has_weekday(&daily("08:30", &[]), 0) && !has_weekday(&daily("08:30", &[]), 8));

        // The calendar's day and weekday of a moment.
        let (day, weekday) = date_of(crate::registry::now_ms());
        assert_eq!((day.len(), (1..=7).contains(&weekday)), (10, true));
        assert_eq!(parse_date(&day).map(super::weekday), Some(weekday));

        // The contract's one-minute question agrees with the day walk: 10:15 on a Wednesday.
        let parsed = Cron::parse("*/15 9-17 * * 1-5").unwrap();
        assert!(parsed.matches(15, 10, 4, 11, 3) && !parsed.matches(15, 10, 8, 11, 7) && !parsed.matches(16, 10, 4, 11, 3));
    }
}
