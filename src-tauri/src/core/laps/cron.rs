//! A five-field cron expression: minute, hour, day of month, month, day of week; lists (`1,5`),
//! ranges (`1-5`), steps (`*/15`, `1-30/5`, `5/10`), names (`jan`, `mon`), `7` and `0` both Sunday. As
//! cron has it, when both day fields are restricted, a day matching either runs.
//!
//! Our own parser, no crate: what it takes is what Vixie cron (and macOS's) takes of those five
//! fields, and nothing more (no `@daily`, no `L`, `W`, `#` or `?`), so an expression copied from a
//! crontab runs at the same moments here.

/// A parsed expression: each field as the set of the values it matches, bit `n` for value `n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    /// 0–59.
    minutes: u64,
    /// 0–23.
    hours: u64,
    /// 1–31.
    days: u64,
    /// 1–12.
    months: u64,
    /// 0–6, Sunday 0 (a `7` is folded into it).
    weekdays: u64,
    /// The day-of-month field begins with `*` (`*`, `*/2`). Cron's rule: when either day field
    /// begins with `*`, a day must match both; when neither does, a day matching either runs.
    any_day: bool,
    /// The day-of-week field begins with `*`.
    any_weekday: bool,
}

/// What one field takes: its smallest and largest value, and its names (the first one being
/// `min`).
struct Field {
    min: u32,
    max: u32,
    names: &'static [&'static str],
}

const MINUTE: Field = Field { min: 0, max: 59, names: &[] };
const HOUR: Field = Field { min: 0, max: 23, names: &[] };
const DAY: Field = Field { min: 1, max: 31, names: &[] };
const MONTH: Field = Field {
    min: 1,
    max: 12,
    names: &["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"],
};
/// 0 and 7 are both Sunday, as in every cron.
const WEEKDAY: Field = Field { min: 0, max: 7, names: &["sun", "mon", "tue", "wed", "thu", "fri", "sat"] };

const REFUSED: &str = "laps.when.error.cron";

/// Every weekday, as cron numbers them (0 Sunday – 6 Saturday).
const ALL_WEEKDAYS: u64 = 0b111_1111;

fn has(set: u64, value: u32) -> bool {
    value < 64 && (set >> value) & 1 == 1
}

/// A weekday 1 (Monday) – 7 (Sunday) as cron's bit (0 Sunday – 6 Saturday).
fn weekday_bit(day: u32) -> u32 {
    day % 7
}

impl Field {
    /// One value: a number within the field, or one of its names in any letter case.
    fn value(&self, text: &str) -> Option<u32> {
        let value = if !text.is_empty() && text.len() <= 4 && text.bytes().all(|b| b.is_ascii_digit()) {
            text.parse::<u32>().ok()?
        } else {
            let index = self.names.iter().position(|name| name.eq_ignore_ascii_case(text))?;
            self.min + u32::try_from(index).ok()?
        };
        (self.min..=self.max).contains(&value).then_some(value)
    }

    /// The field's set: a comma-separated list of `*`, `a`, `a-b`, each with an optional `/step`
    /// (`a/step` running from `a` to the field's end). A range that runs backwards (`fri-mon`), a
    /// value outside the field, an empty item or a step of 0 refuse the whole field.
    fn parse(&self, text: &str) -> Option<u64> {
        let mut set = 0u64;
        for item in text.split(',') {
            let (range, step) = match item.split_once('/') {
                Some((range, step)) if !step.is_empty() && step.len() <= 4 && step.bytes().all(|b| b.is_ascii_digit()) => {
                    (range, Some(step.parse::<u32>().ok()?))
                }
                Some(_) => return None,
                None => (item, None),
            };
            let (low, high) = if range == "*" {
                (self.min, self.max)
            } else if let Some((low, high)) = range.split_once('-') {
                (self.value(low)?, self.value(high)?)
            } else {
                let value = self.value(range)?;
                (value, if step.is_some() { self.max } else { value })
            };
            let step = usize::try_from(step.unwrap_or(1)).ok()?;
            if low > high || step == 0 {
                return None;
            }
            for value in (low..=high).step_by(step) {
                set |= 1 << value;
            }
        }
        Some(set)
    }
}

impl Cron {
    /// The expression, or why not as a catalog key (`laps.when.error.cron`).
    pub fn parse(expr: &str) -> Result<Self, &'static str> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = fields[..] else {
            return Err(REFUSED);
        };
        let field = |spec: &Field, text: &str| spec.parse(text).ok_or(REFUSED);
        let weekdays = field(&WEEKDAY, weekday)?;

        Ok(Self {
            minutes: field(&MINUTE, minute)?,
            hours: field(&HOUR, hour)?,
            days: field(&DAY, day)?,
            months: field(&MONTH, month)?,
            weekdays: (weekdays | (weekdays >> 7)) & ALL_WEEKDAYS,
            any_day: day.starts_with('*'),
            any_weekday: weekday.starts_with('*'),
        })
    }

    /// Whether a local minute matches: its minute, hour, day of month (1–31), month (1–12) and
    /// weekday (1 Monday – 7 Sunday). `when.rs` walks the days (`runs_on`) and then their minutes
    /// (`minutes_of_day`) instead, so it need not ask of every minute of five years.
    #[cfg(test)]
    pub fn matches(&self, minute: u32, hour: u32, day: u32, month: u32, weekday: u32) -> bool {
        has(self.minutes, minute) && has(self.hours, hour) && self.runs_on(day, month, weekday)
    }

    /// Whether a local day can hold a moment: its day of month (1–31), month (1–12) and weekday
    /// (1 Monday – 7 Sunday), by cron's rule for the two day fields.
    pub fn runs_on(&self, day: u32, month: u32, weekday: u32) -> bool {
        if !has(self.months, month) {
            return false;
        }
        let by_day = has(self.days, day);
        let by_weekday = (1..=7).contains(&weekday) && has(self.weekdays, weekday_bit(weekday));
        if self.any_day || self.any_weekday {
            by_day && by_weekday
        } else {
            by_day || by_weekday
        }
    }

    /// The minutes of a day (0–1439) it runs at on a day it runs, in their order.
    pub fn minutes_of_day(&self) -> Vec<u32> {
        (0..24)
            .filter(|&hour| has(self.hours, hour))
            .flat_map(|hour| (0..60).filter(|&minute| has(self.minutes, minute)).map(move |minute| hour * 60 + minute))
            .collect()
    }

    /// Whether its day-of-week field takes weekday `day` (1–7); `*` takes them all.
    pub fn has_weekday(&self, day: u8) -> bool {
        (1..=7).contains(&day) && has(self.weekdays, weekday_bit(u32::from(day)))
    }

    /// The same expression without weekday `day` (1–7) in its day-of-week field, as an explicit
    /// list; none when that leaves no day.
    ///
    /// None too when the day-of-month field is restricted (`1,15`): cron then runs a day that
    /// matches either field, so taking the weekday out of one would still run the dates that fall
    /// on it, and writing a list where `*` was would add every other weekday to those dates.
    pub fn without_weekday(&self, expr: &str, day: u8) -> Option<String> {
        if !self.any_day || !self.has_weekday(day) {
            return None;
        }
        let left = self.weekdays & !(1 << weekday_bit(u32::from(day)));
        (left != 0).then(|| with_weekday_field(expr, &list(left)))
    }

    /// With weekday `day` back. A field that takes every day again goes back to `*`, unless the
    /// day-of-month field is restricted (where `*` would mean "on those dates alone").
    pub fn with_weekday(&self, expr: &str, day: u8) -> String {
        if !(1..=7).contains(&day) || self.has_weekday(day) {
            return expr.to_string();
        }
        let set = self.weekdays | (1 << weekday_bit(u32::from(day)));
        let field = if set == ALL_WEEKDAYS && self.any_day { "*".to_string() } else { list(set) };
        with_weekday_field(expr, &field)
    }
}

/// A weekday set as cron's explicit list, `0,1,2,4`.
fn list(weekdays: u64) -> String {
    (0..7).filter(|&day| has(weekdays, day)).map(|day| day.to_string()).collect::<Vec<_>>().join(",")
}

/// `expr` with its day-of-week field replaced by `field`, the other four kept as written.
fn with_weekday_field(expr: &str, field: &str) -> String {
    let mut fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return expr.to_string();
    }
    fields[4] = field;
    fields.join(" ")
}
