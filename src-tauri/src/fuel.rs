//! The Fuel page's data: Claude's plan limits (`usage.rs`) and today's tokens with their API price
//! (`spend.rs`). Both are read off the main thread, sent to the main window and the popover as
//! `fuel` when they change, and a limit passing 90 % brings one notification per window. The
//! session's share left also goes beside the icon in the menu bar (`bar`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::i18n::t;
use crate::registry::now_ms;
use crate::settings::Settings;
use crate::spend::{SessionSpend, Spend, SpendToday};
use crate::usage::{Status, UsageState, UsageWatch};
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
    /// The reset of each limit already notified about: one notification per window.
    alerted: Mutex<HashMap<String, Option<i64>>>,
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

    /// Reads today's new log lines; tells the page when the totals moved.
    fn read_spend(&self, app: &AppHandle) {
        let (today, sessions) = {
            let mut spend = lock(&self.spend);
            spend.refresh();
            (spend.today(), spend.by_session())
        };
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
            crate::tray::refresh(app, &state.core.snapshot());
        }
        self.alert(app);
    }

    /// To the Fuel page and the popover's dials.
    fn send(&self, app: &AppHandle) {
        let view = self.view();
        let _ = app.emit_to(POPOVER, "fuel", view.clone());
        let _ = app.emit_to(MAIN, "fuel", view);
    }

    /// One notification when the session or the week passes 90 %, unless Settings turned it off.
    fn alert(&self, app: &AppHandle) {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if !state.core.settings().fuel_alert {
            return;
        }

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
            {
                let mut alerted = lock(&self.alerted);
                if alerted.get(&window.id) == Some(&window.resets_at) {
                    continue;
                }
                alerted.insert(window.id.clone(), window.resets_at);
            }

            let used = window.used.round().min(100.0) as u32;
            let title = if window.id == "five_hour" { t!("fuel.alert.session", used = used) } else { t!("fuel.alert.week", used = used) };
            let body = match window.resets_at {
                Some(at) => t!("fuel.alert.refills", time = crate::clock::clock(at)),
                None => String::new(),
            };
            crate::notify(app, NOTIFICATION_ID.to_string(), title, body);
        }
    }
}
