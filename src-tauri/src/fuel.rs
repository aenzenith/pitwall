//! The Fuel page's data: Claude's plan limits (`usage.rs`) and today's tokens with their API price
//! (`spend.rs`). Both are read off the main thread, sent to the main window as `fuel` when they
//! change, and a limit passing 90 % brings one notification per window.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::i18n::t;
use crate::spend::{SessionSpend, Spend, SpendToday};
use crate::usage::{Status, UsageState, UsageWatch};
use crate::{AppState, MAIN};

/// How often today's logs are read again and the limits looked at; the limits hold themselves
/// back to one request every 15 minutes.
const EVERY: Duration = Duration::from_secs(30);
/// The share of a limit that brings the notification.
const ALERT_AT: f64 = 90.0;
/// The limits that notify: the session, and the week across all models.
const ALERTED: [&str; 2] = ["five_hour", "seven_day"];
/// The notification's identifier: a click on it opens the Fuel page.
pub const NOTIFICATION_ID: &str = "pitwall:fuel";

/// What the page shows.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FuelView {
    pub limits: UsageState,
    pub today: Option<SpendToday>,
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

    /// Keeps both fresh for as long as the app runs.
    pub fn start(app: &AppHandle) {
        let app = app.clone();
        thread::spawn(move || loop {
            if let Some(state) = app.try_state::<AppState>() {
                state.fuel.usage.refresh(false);
                state.fuel.read_spend(&app);
            }
            thread::sleep(EVERY);
        });
    }

    /// From the page: on opening it and from its refresh button (`force`).
    pub fn refresh(&self, app: &AppHandle, force: bool) {
        self.usage.refresh(force);
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
        self.alert(app);
    }

    fn send(&self, app: &AppHandle) {
        let _ = app.emit_to(MAIN, "fuel", self.view());
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
