//! Local time, in the system's time zone on every platform: the day a moment falls on, a day's
//! midnights, and the wall-clock time. Shared by the day's timeline, the day's spend and Fuel.

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Timelike};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

fn local(ms: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(i64::try_from(ms).ok()?).single()
}

/// Year, month and day of `ms` (since the epoch), local time.
pub fn local_date(ms: u64) -> (i32, u32, u32) {
    match local(ms) {
        Some(at) => (at.year(), at.month(), at.day()),
        None => (1970, 1, 1),
    }
}

/// `YYYY-MM-DD` of `ms`, local time.
pub fn day_name(ms: u64) -> String {
    let (year, month, day) = local_date(ms);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The local midnight that starts `day` of `month`, in ms. A day past the month's end counts on
/// into the next month (day 32 of January is 1 February). Where the clocks skip midnight, the
/// day starts at its first hour that exists.
pub fn local_midnight(year: i32, month: u32, day: u32) -> Option<u64> {
    let date = NaiveDate::from_ymd_opt(year, month, 1)?.checked_add_days(Days::new(u64::from(day.checked_sub(1)?)))?;

    (0..=3).find_map(|hour| {
        let start = Local.from_local_datetime(&date.and_hms_opt(hour, 0, 0)?).earliest()?;
        u64::try_from(start.timestamp_millis()).ok()
    })
}

/// The local day holding `now`: its midnight, the next one, and its `YYYY-MM-DD`. Should the
/// time zone fail, the UTC day.
pub fn local_day(now: u64) -> (u64, u64, String) {
    let (year, month, day) = local_date(now);
    let date = format!("{year:04}-{month:02}-{day:02}");

    match (local_midnight(year, month, day), local_midnight(year, month, day + 1)) {
        (Some(start), Some(end)) if start <= now && now < end => (start, end, date),
        _ => {
            let start = now - now % DAY_MS;
            (start, start + DAY_MS, utc_date(now))
        }
    }
}

fn utc_date(ms: u64) -> String {
    let at = i64::try_from(ms).ok().and_then(DateTime::from_timestamp_millis).unwrap_or_default();
    at.format("%Y-%m-%d").to_string()
}

/// `at` (ms) as the local wall-clock time, `16:40`.
pub fn clock(at: i64) -> String {
    match Local.timestamp_millis_opt(at).single() {
        Some(time) => format!("{:02}:{:02}", time.hour(), time.minute()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The day's timeline and spend count from these bounds; a month's last day must end where
    /// the next month's first begins.
    #[test]
    fn a_day_runs_from_its_midnight_to_the_next_across_a_month_end() {
        let (start, end, date) = local_day(crate::registry::now_ms());
        assert!(start < end && end - start <= 25 * 3_600_000, "{start}..{end}");
        assert_eq!(day_name(start), date);
        assert_ne!(day_name(end), date);

        let last = local_midnight(2026, 1, 31).unwrap();
        let next = local_midnight(2026, 1, 32).unwrap();
        assert_eq!(next, local_midnight(2026, 2, 1).unwrap());
        assert_eq!(day_name(last), "2026-01-31");
        assert_eq!(day_name(next), "2026-02-01");
        assert!(local_midnight(2026, 13, 1).is_none());
    }
}
