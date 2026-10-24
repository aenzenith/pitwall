//! The Fuel page's data: Claude's plan limits (`usage.rs`) and today's tokens with their API price
//! (`spend.rs`). Both are read off the main thread, sent to the main window, the popover and the
//! mini tower as `fuel` when they change, and a limit passing 90 % brings one notification per
//! window. The session's share left also goes beside the icon in the menu bar (`bar`). Each look
//! at the logs also goes to the core for the Today page (`DayFuel`), with the share of the
//! week's limit the day took: told from the week as read at the day's first reading and after a
//! refill, which the core writes into the day's file (`WeekMark`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::clock::day_name;
use crate::core::{Core, DayFuel, WeekMark};
use crate::i18n::t;
use crate::registry::now_ms;
use crate::settings::Settings;
use crate::spend::{SessionSpend, Spend, SpendToday};
use crate::usage::{Status, UsageState, UsageWatch, Window};
use crate::{AppState, MAIN, POPOVER};

/// How often today's logs are read again and the limits looked at; the limits hold themselves
/// back to one request every few minutes, as Settings says (`Settings::fuel_every_ms`).
const EVERY: Duration = Duration::from_secs(30);
/// The share of a limit that brings the notification.
const ALERT_AT: f64 = 90.0;
/// The limits that notify: the session, and the week across all models.
const ALERTED: [&str; 2] = ["five_hour", "seven_day"];
/// The notification's identifier: a click on it opens the Fuel page.
pub const NOTIFICATION_ID: &str = "pitwall:fuel";
/// At or under this share left, a limit shows red: the page's `LOW_LEFT` (src/lib/fuel.ts).
const LOW_LEFT: f64 = 10.0;
/// A limit whose refill moves by more than this is a new window: a new week, a new session.
/// Less is the same one: the API's refill moment moves by a fraction of a second from one
/// reading to the next.
const HOUR_MS: i64 = 60 * 60 * 1000;
/// Past this much of the day's use for each of its share measured, the share was worked out
/// (`DayFuel::estimated`).
const ESTIMATED: f64 = 1.02;
/// Past this much, too little of the day was measured to tell its share: the limit is read in
/// whole percents, and a point or two measured late in the day would be scaled up many times.
const UNTOLD: f64 = 1.25;

/// What the page shows.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FuelView {
    pub limits: UsageState,
    pub today: Option<SpendToday>,
}

/// The session's fuel as the menu bar shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarFuel {
    /// Percent left, 0–100.
    pub left: u8,
    /// Little enough left to show red.
    pub low: bool,
    /// The limits couldn't be read this time: an older reading, shown dimmed.
    pub stale: bool,
}

pub struct Fuel {
    usage: UsageWatch,
    spend: Mutex<Spend>,
    /// Today's numbers as of the last read, so asking for them never waits on a read.
    today: Mutex<Option<SpendToday>>,
    /// The same per session, for the Sessions page.
    sessions: Mutex<HashMap<String, SessionSpend>>,
    /// The reset of each limit the door has had, and whether the user was told of it: one
    /// notification per window.
    alerted: Mutex<HashMap<String, (Option<i64>, bool)>>,
    /// The week's limit through the day.
    week: Mutex<WeekDay>,
}

/// The week's limit through a day: where each stretch of it started, and the latest reading.
#[derive(Default)]
struct WeekDay {
    /// `YYYY-MM-DD`, local.
    date: String,
    marks: Vec<WeekMark>,
    /// The week's limit used, as last read, and when it refills.
    last: Option<f64>,
    resets: Option<i64>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Fuel {
    /// `claude_projects` is Claude Code's log folder. Nothing is read until `start`.
    pub fn new(app: &AppHandle, claude_projects: PathBuf) -> Self {
        let handle = app.clone();
        let usage = UsageWatch::new(move || {
            if let Some(state) = handle.try_state::<AppState>() {
                state.fuel.limits_changed(&handle);
            }
        });

        Self {
            usage,
            spend: Mutex::new(Spend::new(claude_projects)),
            today: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            alerted: Mutex::new(HashMap::new()),
            week: Mutex::new(WeekDay::default()),
        }
    }

    /// Today's tokens and cost per session, as of the last read.
    pub fn sessions(&self) -> HashMap<String, SessionSpend> {
        lock(&self.sessions).clone()
    }

    pub fn view(&self) -> FuelView {
        FuelView { limits: self.usage.state(), today: lock(&self.today).clone() }
    }

    /// The session's fuel for the menu bar: none while Settings hides it, before the first
    /// reading, and on a plan without a session limit.
    pub fn bar(&self, settings: &Settings) -> Option<BarFuel> {
        if !settings.fuel_in_bar {
            return None;
        }
        let limits = self.usage.state();
        let session = limits.usage.as_ref()?.windows.iter().find(|window| window.id == "five_hour")?;
        // A window whose reset has passed has refilled, as the page shows it (`current`).
        let refilled = session.resets_at.is_some_and(|at| at <= now_ms() as i64);
        let left = if refilled { 100.0 } else { (100.0 - session.used).clamp(0.0, 100.0) };
        let stale = !matches!(limits.status, Status::Ready | Status::Loading);
        Some(BarFuel { left: left.round() as u8, low: left <= LOW_LEFT, stale })
    }

    /// Keeps both fresh for as long as the app runs.
    pub fn start(app: &AppHandle) {
        let app = app.clone();
        thread::spawn(move || loop {
            if let Some(state) = app.try_state::<AppState>() {
                state.fuel.usage.refresh(false, state.core.settings().fuel_every_ms());
                state.fuel.read_spend(&app);
                // A session that refilled meanwhile shows full in the menu bar too.
                crate::tray::refresh(&app, &state.core.snapshot());
            }
            thread::sleep(EVERY);
        });
    }

    /// From the page: on opening it and from its refresh button (`force`). `every_ms` is
    /// Settings' rest between two requests nobody asked for.
    pub fn refresh(&self, app: &AppHandle, force: bool, every_ms: i64) {
        self.usage.refresh(force, every_ms);
        let app = app.clone();
        thread::spawn(move || {
            if let Some(state) = app.try_state::<AppState>() {
                state.fuel.read_spend(&app);
            }
        });
    }

    /// Reads today's new log lines; tells the page when the totals moved, and hands the day's
    /// tokens to the core for the Today page.
    fn read_spend(&self, app: &AppHandle) {
        let state = app.try_state::<AppState>();
        let (today, sessions, since) = {
            let mut spend = lock(&self.spend);
            spend.refresh();
            let today = spend.today();
            let first = state.as_ref().and_then(|state| self.week_of(&state.core, &today.date).marks.first().map(|mark| mark.t));
            let since = first.map(|at| spend.since(at));
            (today, spend.by_session(), since)
        };
        if let Some(state) = &state {
            state.core.day_fuel(self.day_fuel(&state.core, &today, &sessions, since));
        }
        *lock(&self.sessions) = sessions;

        let changed = {
            let mut cached = lock(&self.today);
            let changed = cached.as_ref() != Some(&today);
            *cached = Some(today);
            changed
        };

        if changed {
            self.send(app);
        }
    }

    fn limits_changed(&self, app: &AppHandle) {
        self.send(app);
        if let Some(state) = app.try_state::<AppState>() {
            self.note_week(&state.core);
            crate::tray::refresh(app, &state.core.snapshot());
            // A board card that waits for the session's refill follows what was just read.
            if let Some(at) = self.refill_read() {
                state.core.board_refill(at);
            }
        }
        self.alert(app);
    }

    /// When the 5-hour session refills, as a reading that has just come says; none while the
    /// last request failed (what is kept then is an older reading), the session hasn't started,
    /// or the moment it names is already past (as `refill`: that is no refill to wait for).
    fn refill_read(&self) -> Option<u64> {
        let limits = self.usage.state();
        if limits.status != Status::Ready {
            return None;
        }
        let session = limits.usage?.windows.into_iter().find(|window| window.id == "five_hour")?;
        u64::try_from(session.resets_at?).ok().filter(|at| *at > now_ms())
    }

    /// When the 5-hour session refills, as far as the last good reading knows a moment still
    /// ahead: what a board card can be told to wait for (`commands::board_schedule`).
    pub fn refill(&self) -> Option<u64> {
        let session = self.usage.state().usage?.windows.into_iter().find(|window| window.id == "five_hour")?;
        u64::try_from(session.resets_at?).ok().filter(|at| *at > now_ms())
    }

    /// The week's limit through `date`: from the day's file the first time the day is met.
    fn week_of(&self, core: &Core, date: &str) -> std::sync::MutexGuard<'_, WeekDay> {
        let mut week = lock(&self.week);
        if week.date != date {
            let (marks, last) = core.week_marks(date);
            let resets = marks.last().and_then(|mark| mark.resets);
            *week = WeekDay { date: date.to_string(), marks, last, resets };
        }
        week
    }

    /// A good reading of the week's limit. The day's first, and the first after the week
    /// refilled, start a stretch of the day, written into its file.
    fn note_week(&self, core: &Core) {
        let limits = self.usage.state();
        if limits.status != Status::Ready {
            return;
        }
        let Some(window) = limits.usage.and_then(|usage| usage.windows.into_iter().find(|window| window.id == "seven_day")) else {
            return;
        };

        let now = now_ms();
        let mut week = self.week_of(core, &day_name(now));
        // A week that ran out unused may come back unstarted, with no refill time.
        let refilled = match (week.resets, window.resets_at) {
            (Some(at), Some(next)) => next > at + HOUR_MS,
            (Some(at), None) => now as i64 >= at,
            (None, _) => false,
        };
        let mark = if week.marks.is_empty() {
            Some(WeekMark { t: now, used: window.used, before: None, resets: window.resets_at })
        } else if refilled {
            let before = week.last.or(week.marks.last().map(|mark| mark.used));
            Some(WeekMark { t: now, used: window.used, before, resets: window.resets_at })
        } else {
            None
        };

        if let Some(mark) = mark {
            core.record_week(&mark);
            week.marks.push(mark);
        }
        week.last = Some(window.used);
        week.resets = window.resets_at;
    }

    /// The day's tokens and the share of the week's limit they took. Measured from the day's
    /// marks; when the day's use began before the first of them, the share is scaled up by the
    /// use before it, at the rate after it, as long as that is a small part of the day (`UNTOLD`).
    /// `since`: the tokens and cost from the first mark on.
    fn day_fuel(&self, core: &Core, today: &SpendToday, sessions: &HashMap<String, SessionSpend>, since: Option<(u64, f64)>) -> DayFuel {
        let week = self.week_of(core, &today.date);
        let (share, estimated) = match (week.last, since) {
            (Some(last), Some((tokens_after, cost_after))) => {
                let measured = spent(&week.marks, last);
                // By cost, or by tokens when no model used has a price.
                let (all, after) = if today.cost_usd > 0.0 { (today.cost_usd, cost_after) } else { (today.tokens.total as f64, tokens_after as f64) };
                if all <= 0.0 {
                    (Some(measured), false)
                } else if after <= 0.0 || all / after > UNTOLD {
                    // All or most of it before the limit was read: no telling.
                    (None, false)
                } else {
                    let factor = all / after;
                    (Some(measured * factor), factor > ESTIMATED)
                }
            }
            _ => (None, false),
        };
        DayFuel { date: today.date.clone(), tokens: today.tokens.total, week: share, estimated, used: week.last, sessions: sessions.clone() }
    }

    /// To the Fuel page, the popover's dials and the mini tower's share.
    fn send(&self, app: &AppHandle) {
        let view = self.view();
        let _ = app.emit_to(POPOVER, "fuel", view.clone());
        let _ = app.emit_to(crate::tower::LABEL, "fuel", view.clone());
        let _ = app.emit_to(MAIN, "fuel", view);
    }

    /// Once when the session or the week passes 90 %: handed to the core (`core/signals.rs`),
    /// which shows the notification unless Settings turned it off, or it is quiet.
    fn alert(&self, app: &AppHandle) {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };

        let limits = self.usage.state();
        if limits.status != Status::Ready {
            return;
        }
        let Some(usage) = limits.usage else {
            return;
        };

        for window in usage.windows.iter().filter(|window| ALERTED.contains(&window.id.as_str())) {
            if window.used < ALERT_AT {
                continue;
            }
            // Held until the door has answered: one notification per window, whoever asks.
            let mut alerted = lock(&self.alerted);
            let before = told_before(&alerted, window);
            if before == Some(true) {
                continue;
            }

            let used = window.used.round().min(100.0) as u32;
            let title = if window.id == "five_hour" { t!("fuel.alert.session", used = used) } else { t!("fuel.alert.week", used = used) };
            let body = match window.resets_at {
                Some(at) => t!("fuel.alert.refills", time = crate::clock::clock(at)),
                None => String::new(),
            };
            // With the alert off the door has it all the same, once, for the phone: its
            // notification is owed until the alert is switched on.
            let told = state.core.fuel_high(NOTIFICATION_ID, title, body, before.is_some());
            alerted.insert(window.id.clone(), (window.resets_at, told));
        }
    }
}

/// Whether the user was told of `window` passing 90 %: none for a window not met yet. One window
/// is one refill moment, give or take what the API's moment moves between readings (`HOUR_MS`).
fn told_before(alerted: &HashMap<String, (Option<i64>, bool)>, window: &Window) -> Option<bool> {
    let same = |reset: Option<i64>| match (reset, window.resets_at) {
        (Some(at), Some(next)) => (next - at).abs() <= HOUR_MS,
        (at, next) => at == next,
    };
    alerted.get(&window.id).filter(|(reset, _)| same(*reset)).map(|(_, told)| *told)
}

/// What the week's limit took in a day, in percent: each stretch from its mark to what the next
/// one says it had used before the refill, the last one to `last`.
fn spent(marks: &[WeekMark], last: f64) -> f64 {
    marks
        .iter()
        .enumerate()
        .map(|(at, mark)| {
            let end = marks.get(at + 1).and_then(|next| next.before).unwrap_or(last);
            (end - mark.used).max(0.0)
        })
        .sum()
}
