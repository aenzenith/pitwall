//! The Sessions page's data (`core::SessionsView`): built when the page asks (`claude_sessions`),
//! and once the main window's page has asked, sent to it as `sessions` when it changes, at most
//! once a second, while that window is up. Nobody asking, nothing is built.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::core::SessionsView;
use crate::{AppState, MAIN};

/// How often the page's data is looked at while it is wanted.
const EVERY: Duration = Duration::from_secs(1);
/// Today's times grow by themselves while a session works; that alone is sent this often.
const TICK: Duration = Duration::from_secs(30);

/// What was last sent: the whole view and the view without today's times (both without `now`),
/// and when.
#[derive(Default)]
struct Sent {
    whole: String,
    shape: String,
    at: Option<Instant>,
}

#[derive(Default)]
pub struct Sessions {
    /// The main window's page asked once: from then on it is kept up to date.
    asked: AtomicBool,
    sent: Mutex<Sent>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The view as two strings to compare: whole, and without today's times.
fn fingerprints(view: &SessionsView) -> (String, String) {
    let whole = serde_json::to_string(&(view.hook, view.unlisted, &view.sessions)).unwrap_or_default();
    let mut rows = view.sessions.clone();
    for row in &mut rows {
        row.today = None;
    }
    let shape = serde_json::to_string(&(view.hook, view.unlisted, &rows)).unwrap_or_default();
    (whole, shape)
}

impl Sessions {
    /// The page asks: the view now, and from now on its changes.
    pub fn ask(&self, app: &AppHandle) -> SessionsView {
        self.asked.store(true, Ordering::SeqCst);
        let view = build(app);
        let (whole, shape) = fingerprints(&view);
        *lock(&self.sent) = Sent { whole, shape, at: Some(Instant::now()) };
        view
    }

    /// Keeps the page up to date for as long as the app runs.
    pub fn start(app: &AppHandle) {
        let app = app.clone();
        thread::spawn(move || loop {
            thread::sleep(EVERY);
            if let Some(state) = app.try_state::<AppState>() {
                state.sessions.send_changes(&app);
            }
        });
    }

    /// Sends the view when it changed, while the main window's page wants it and is up.
    fn send_changes(&self, app: &AppHandle) {
        if !self.asked.load(Ordering::SeqCst) {
            return;
        }
        let visible = app.get_webview_window(MAIN).is_some_and(|main| main.is_visible().unwrap_or(false));
        if !visible {
            return;
        }

        let view = build(app);
        let (whole, shape) = fingerprints(&view);
        {
            let mut sent = lock(&self.sent);
            let ticked = sent.at.is_none_or(|at| at.elapsed() >= TICK);
            if shape == sent.shape && (whole == sent.whole || !ticked) {
                return;
            }
            *sent = Sent { whole, shape, at: Some(Instant::now()) };
        }

        let _ = app.emit_to(MAIN, "sessions", view);
    }
}

fn build(app: &AppHandle) -> SessionsView {
    match app.try_state::<AppState>() {
        Some(state) => state.core.sessions_view(&state.fuel.sessions()),
        None => SessionsView { now: crate::registry::now_ms(), hook: false, sessions: Vec::new(), unlisted: 0 },
    }
}
