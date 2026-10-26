//! The Fuel page's data: Claude's plan limits (`usage.rs`) and today's tokens with their API price
//! (`spend.rs`). Both are read off the main thread, sent to the main window, the popover and the
//! mini tower as `fuel` when they change, and a limit passing 90 % brings one notification per
//! window. The session's share left also goes beside the icon in the menu bar (`bar`). Each look
//! at the logs also goes to the core for the Today page (`DayFuel`), with the share of the
//! week's limit the day took: told from the week as read at the day's first reading and after a
//! refill, which the core writes into the day's file (`WeekMark`).
//!
//! Each Claude account (`accounts/`) has its limits read: the page, the menu bar, the alert and a
//! board card waiting for the refill follow the account in use; the week's share of the day is
//! the main account's (only its readings are written into the day's file).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::accounts::{self, AccountView};
use crate::clock::day_name;
use crate::core::{Core, DayFuel, WeekMark};
use crate::i18n::t;
use crate::registry::now_ms;
use crate::settings::Settings;
use crate::spend::{SessionSpend, Spend, SpendToday};
use crate::usage::{Source, Status, UsageState, UsageWatch, Window};
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

/// What the page shows: the limits of the account in use, and every account's.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FuelView {
    pub limits: UsageState,
    pub today: Option<SpendToday>,
    /// The main account first, then the others as they were added.
    pub accounts: Vec<AccountView>,
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
    app: AppHandle,
    /// Each account's limits, by its id (`accounts::MAIN` for the main one), as listed.
    usage: Mutex<Vec<(String, UsageWatch)>>,
    spend: Mutex<Spend>,
    /// Today's numbers as of the last read, so asking for them never waits on a read.
    today: Mutex<Option<SpendToday>>,
    /// The same per session, for the Sessions page.
    sessions: Mutex<HashMap<String, SessionSpend>>,
    /// The reset of each limit the door has had, and whether the user was told of it: one
    /// notification per window, by the account's id and the window's.
    alerted: Mutex<Alerted>,
    /// The week's limit through the day.
    week: Mutex<WeekDay>,
}

/// By the account's id and the window's: the window's refill as the alert met it, and whether
/// the user was told.
type Alerted = HashMap<(String, String), (Option<i64>, bool)>;

/// The week's limit through a day: where each stretch of it started, and the latest reading.
#[derive(Default)]
struct WeekDay {
    /// `YYYY-MM-DD`, local.
    date: String,
    marks: Vec<WeekMark>,
    /// The week's limit used, as last read, and when it refills.
    last: Option<f64>,
    resets: Option<i64>,
    /// Another account than the main one was in use or at work this day (as far as this run of
    /// Pitwall saw): the day's tokens are no longer the main account's alone, so their share of
    /// its week isn't told.
    mixed: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Fuel {
    /// `claude_projects` is Claude Code's log folder. Nothing is read until `start`.
    pub fn new(app: &AppHandle, claude_projects: PathBuf) -> Self {
        Self {
            app: app.clone(),
            usage: Mutex::new(vec![(accounts::MAIN.to_string(), watch(app, accounts::MAIN, Source::account(None)))]),
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

    /// The watch of each account `settings` lists: one is made for an account added (and read at
    /// once) and dropped for one removed.
    pub fn follow_accounts(&self, settings: &Settings) {
        let wanted: Vec<(String, Source)> = std::iter::once((accounts::MAIN.to_string(), Source::account(None)))
            .chain(settings.accounts.others.iter().map(|account| (account.id.clone(), Source::account(Some(&account.dir)))))
            .collect();
        let made = {
            let mut usage = lock(&self.usage);
            let mut kept: Vec<(String, UsageWatch)> = Vec::new();
            let mut made = Vec::new();
            for (id, source) in wanted {
                match usage.iter().position(|(known, watch)| *known == id && *watch.source() == source) {
                    Some(at) => kept.push(usage.remove(at)),
                    None => {
                        let new = watch(&self.app, &id, source);
                        made.push(new.clone());
                        kept.push((id, new));
                    }
                }
            }
            *usage = kept;
            made
        };
        for watch in made {
            watch.refresh(false, settings.fuel_every_ms());
        }
    }

    /// The limits of the account `id` as last read; being read for one not watched yet (never
    /// another account's).
    fn state_of(&self, id: &str) -> UsageState {
        let watch = lock(&self.usage).iter().find(|(known, _)| known == id).map(|(_, watch)| watch.clone());
        watch.map(|watch| watch.state()).unwrap_or_else(UsageState::loading)
    }

    /// The limits of the account in use, as last read.
    fn limits(&self) -> UsageState {
        self.state_of(&in_use_id(&self.settings()))
    }

    fn settings(&self) -> Settings {
        self.app.try_state::<AppState>().map(|state| state.core.settings()).unwrap_or_default()
    }

    pub fn view(&self) -> FuelView {
        let (settings, running) = match self.app.try_state::<AppState>() {
            Some(state) => (state.core.settings(), state.core.claudes_by_account()),
            None => (Settings::default(), HashMap::new()),
        };
        let home = dirs::home_dir().unwrap_or_default();
        let in_use = in_use_id(&settings);
        let state_of = |id: &str| self.state_of(id);
        let count = |id: &str| running.get(id).copied().unwrap_or(0);

        let main = AccountView {
            id: accounts::MAIN.into(),
            name: settings.accounts.main_name.clone(),
            main: true,
            dir: accounts::shown(&home, &accounts::main_dir(&home).to_string_lossy()),
            shared: false,
            sync: false,
            in_use: in_use == accounts::MAIN,
            missing: false,
            limits: state_of(accounts::MAIN),
            running: count(accounts::MAIN),
        };
        let others = settings.accounts.others.iter().map(|account| AccountView {
            id: account.id.clone(),
            name: Some(account.name.clone()),
            main: false,
            dir: accounts::shown(&home, &account.dir),
            shared: account.shared,
            sync: account.sync,
            in_use: in_use == account.id,
            missing: !std::path::Path::new(&account.dir).is_dir(),
            limits: state_of(&account.id),
            running: count(&account.id),
        });
        let accounts: Vec<AccountView> = std::iter::once(main).chain(others).collect();
        FuelView { limits: self.state_of(&in_use), today: lock(&self.today).clone(), accounts }
    }

    /// The session's fuel for the menu bar: none while Settings hides it, before the first
    /// reading, and on a plan without a session limit. The account in use's.
    pub fn bar(&self, settings: &Settings) -> Option<BarFuel> {
        if !settings.fuel_in_bar {
            return None;
        }
        let limits = self.state_of(&in_use_id(settings));
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
                let settings = state.core.settings();
                state.fuel.follow_accounts(&settings);
                state.fuel.refresh_all(false, settings.fuel_every_ms());
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
        self.refresh_all(force, every_ms);
        let app = app.clone();
        thread::spawn(move || {
            if let Some(state) = app.try_state::<AppState>() {
                state.fuel.read_spend(&app);
            }
        });
    }

    /// Every account's limits, each as its own watch allows (`UsageWatch::refresh`).
    fn refresh_all(&self, force: bool, every_ms: i64) {
        let watches: Vec<UsageWatch> = lock(&self.usage).iter().map(|(_, watch)| watch.clone()).collect();
        for watch in watches {
            watch.refresh(force, every_ms);
        }
    }

    /// The accounts changed (one added, removed, renamed, switched to): the watches follow and
    /// every window hears of it; what follows the account in use (the menu bar) follows too.
    pub fn accounts_changed(&self, app: &AppHandle) {
        if let Some(state) = app.try_state::<AppState>() {
            self.follow_accounts(&state.core.settings());
            crate::tray::refresh(app, &state.core.snapshot());
            if let Some(at) = self.refill_read() {
                state.core.board_refill(at);
            }
        }
        self.send(app);
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

    /// The account `id`'s limits changed: every window hears of it. The main account's week goes
    /// into the day's file; the account in use's moves the menu bar, the board's refill and the
    /// alert.
    fn limits_changed(&self, app: &AppHandle, id: &str) {
        self.send(app);
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if id == accounts::MAIN {
            self.note_week(&state.core);
        }
        if in_use_id(&state.core.settings()) != id {
            return;
        }
        crate::tray::refresh(app, &state.core.snapshot());
        // A board card that waits for the session's refill follows what was just read.
        if let Some(at) = self.refill_read() {
            state.core.board_refill(at);
        }
        self.alert(app, id);
    }

    /// When the 5-hour session refills, as a reading that has just come says; none while the
    /// last request failed (what is kept then is an older reading), the session hasn't started,
    /// or the moment it names is already past (as `refill`: that is no refill to wait for).
    fn refill_read(&self) -> Option<u64> {
        let limits = self.limits();
        if limits.status != Status::Ready {
            return None;
        }
        let session = limits.usage?.windows.into_iter().find(|window| window.id == "five_hour")?;
        u64::try_from(session.resets_at?).ok().filter(|at| *at > now_ms())
    }

    /// When the 5-hour session refills, as far as the last good reading knows a moment still
    /// ahead: what a board card can be told to wait for (`commands::board_schedule`).
    pub fn refill(&self) -> Option<u64> {
        let session = self.limits().usage?.windows.into_iter().find(|window| window.id == "five_hour")?;
        u64::try_from(session.resets_at?).ok().filter(|at| *at > now_ms())
    }

    /// The week's limit through `date`: from the day's file the first time the day is met.
    fn week_of(&self, core: &Core, date: &str) -> std::sync::MutexGuard<'_, WeekDay> {
        let mut week = lock(&self.week);
        if week.date != date {
            let (marks, last) = core.week_marks(date);
            let resets = marks.last().and_then(|mark| mark.resets);
            *week = WeekDay { date: date.to_string(), marks, last, resets, mixed: false };
        }
        week
    }

    /// A good reading of the week's limit. The day's first, and the first after the week
    /// refilled, start a stretch of the day, written into its file.
    fn note_week(&self, core: &Core) {
        let limits = self.state_of(accounts::MAIN);
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
        let others = core.accounts().in_use.is_some() || core.claudes_by_account().iter().any(|(id, count)| id != accounts::MAIN && *count > 0);
        let mut week = self.week_of(core, &today.date);
        week.mixed |= others;
        let (share, estimated) = match (week.last, since) {
            _ if week.mixed => (None, false),
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
    fn alert(&self, app: &AppHandle, id: &str) {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };

        let limits = self.state_of(id);
        // With more than one account, the notification says whose limit it is.
        let settings = state.core.settings();
        let account = (!settings.accounts.others.is_empty()).then(|| accounts::name_of(&settings.accounts, Some(id)));
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
            let key = (id.to_string(), window.id.clone());
            let before = told_before(&alerted, &key, window);
            if before == Some(true) {
                continue;
            }

            let used = window.used.round().min(100.0) as u32;
            let title = if window.id == "five_hour" { t!("fuel.alert.session", used = used) } else { t!("fuel.alert.week", used = used) };
            let body = match (window.resets_at, &account) {
                (Some(at), Some(account)) => t!("fuel.alert.refillsOf", account = account, time = crate::clock::clock(at)),
                (Some(at), None) => t!("fuel.alert.refills", time = crate::clock::clock(at)),
                (None, Some(account)) => account.clone(),
                (None, None) => String::new(),
            };
            // With the alert off the door has it all the same, once, for the phone: its
            // notification is owed until the alert is switched on.
            let told = state.core.fuel_high(NOTIFICATION_ID, title, body, before.is_some());
            alerted.insert(key, (window.resets_at, told));
        }
    }
}

/// Whether the user was told of `window` passing 90 %: none for a window not met yet. One window
/// is one refill moment, give or take what the API's moment moves between readings (`HOUR_MS`).
fn told_before(alerted: &Alerted, key: &(String, String), window: &Window) -> Option<bool> {
    let same = |reset: Option<i64>| match (reset, window.resets_at) {
        (Some(at), Some(next)) => (next - at).abs() <= HOUR_MS,
        (at, next) => at == next,
    };
    alerted.get(key).filter(|(reset, _)| same(*reset)).map(|(_, told)| *told)
}

/// A watch of the account `id`'s limits, which tells `Fuel` when they change.
fn watch(app: &AppHandle, id: &str, source: Source) -> UsageWatch {
    let (handle, id) = (app.clone(), id.to_string());
    UsageWatch::new(source, move || {
        if let Some(state) = handle.try_state::<AppState>() {
            state.fuel.limits_changed(&handle, &id);
        }
    })
}

/// The id of the account in use, as a Claude would start with it (`accounts::start_with`): the
/// main one's unless another listed one, whose folder is there, is.
fn in_use_id(settings: &Settings) -> String {
    match accounts::start_with(&settings.accounts, None) {
        Ok(Some(account)) => account.id.clone(),
        _ => accounts::MAIN.to_string(),
    }
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
