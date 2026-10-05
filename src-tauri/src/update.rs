//! Pitwall's own updates. A new version is asked for at the latest GitHub release (`latest.json`,
//! `tauri-plugin-updater`), downloaded in the background and checked against the public key in
//! `tauri.conf.json`. It is installed only when the user restarts for it, or when the app quits:
//! Pitwall never restarts by itself. A restart puts back what was open (`core::restore`).
//!
//! The request carries the plugin's user agent and nothing of the user's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error, UpdaterExt};

use crate::core::Restored;
use crate::i18n::t;
use crate::registry::now_ms;
use crate::{tray, AppState, MAIN};

/// How often a new version is asked for while the app runs.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The first ask waits until the app has settled.
const FIRST: Duration = Duration::from_secs(20);
/// A download's progress reaches the window at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// The notification's identifier: a click on it opens the window.
pub const NOTIFICATION_ID: &str = "pitwall:update";

/// What the sidebar's brand row and Settings › About show.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    /// The installed version.
    pub current: String,
    /// `off` (this build never asks: a development run, no key) | `idle` | `checking` |
    /// `downloading` | `ready` | `available` (a new version this install can't put in its own
    /// place) | `installing` | `failed`
    pub status: &'static str,
    /// The new version: from `downloading` on, and with `available` and `failed`.
    pub version: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// When the release last answered, in ms.
    pub checked_at: Option<u64>,
    /// The last ask got no answer.
    pub offline: bool,
    /// With `failed`: `download` | `signature` | `install`.
    pub error: Option<&'static str>,
    /// A ready update is installed when the app quits, too.
    pub on_quit: bool,
}

pub struct Update {
    view: Mutex<UpdateView>,
    /// The downloaded update, its signature checked, until it is installed.
    ready: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>,
    /// One ask or download at a time.
    busy: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The key updates are checked against; empty until the project has one.
fn public_key(app: &AppHandle) -> String {
    let plugins = &app.config().plugins.0;
    plugins.get("updater").and_then(|updater| updater.get("pubkey")).and_then(|key| key.as_str()).unwrap_or_default().trim().to_string()
}

/// What an update replaces: the `.app` on macOS, the AppImage on Linux.
fn install_path() -> Option<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(appimage));
    }
    let exe = std::env::current_exe().ok()?;
    tauri_plugin_updater::extract_path_from_executable(&exe).ok()
}

/// A disk image, or the randomized read-only place macOS runs a quarantined app from: nothing
/// can be replaced there, whatever the permissions say.
fn read_only_place(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.starts_with("/Volumes/") || path.contains("/AppTranslocation/")
}

/// The user may replace `path` without being asked for a password.
#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let allowed = |path: &Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes())
            // SAFETY: a NUL-terminated path; access only reads it.
            .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
    };
    allowed(path) && path.parent().is_some_and(allowed)
}

/// Installing at quit must ask nothing and leave nothing running: not on Windows (the installer
/// shows a window and the process ends inside `install`), not for a system package (a password
/// prompt), not where the user can't write.
fn quiet_install() -> bool {
    #[cfg(unix)]
    {
        use tauri::utils::config::BundleType;

        let package = matches!(tauri::utils::platform::bundle_type(), Some(BundleType::Deb | BundleType::Rpm));
        !package && install_path().is_some_and(|path| !read_only_place(&path) && writable(&path))
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Which step a failed download failed at.
fn download_error(error: &Error) -> &'static str {
    match error {
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion => "signature",
        _ => "download",
    }
}

impl Update {
    pub fn new(app: &AppHandle) -> Self {
        // A development run never replaces itself, and without a key nothing could be trusted.
        let on = crate::is_bundled() && !public_key(app).is_empty();

        Self {
            view: Mutex::new(UpdateView {
                current: app.package_info().version.to_string(),
                status: if on { "idle" } else { "off" },
                version: None,
                downloaded: 0,
                total: None,
                checked_at: None,
                offline: false,
                error: None,
                on_quit: on && quiet_install(),
            }),
            ready: Mutex::new(None),
            busy: AtomicBool::new(false),
        }
    }

    pub fn view(&self) -> UpdateView {
        lock(&self.view).clone()
    }

    fn set(&self, app: &AppHandle, change: impl FnOnce(&mut UpdateView)) {
        let view = {
            let mut view = lock(&self.view);
            change(&mut view);
            view.clone()
        };
        let _ = app.emit_to(MAIN, "update", view);
    }

    /// Asks by itself for as long as the app runs, unless Settings turned that off.
    pub fn start(app: &AppHandle) {
        let app = app.clone();
        thread::spawn(move || {
            thread::sleep(FIRST);
            loop {
                if let Some(state) = app.try_state::<AppState>() {
                    if state.core.settings().auto_update {
                        state.update.check(&app);
                    }
                }
                thread::sleep(EVERY);
            }
        });
    }

    /// Asks for a new version now and downloads one that is there. The answer arrives as
    /// `update` events.
    pub fn check(&self, app: &AppHandle) {
        if lock(&self.view).status == "off" || self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        // An update that waits stays in sight while a newer one is asked for.
        self.set(app, |view| {
            if view.status != "ready" {
                view.status = "checking";
                view.error = None;
            }
        });

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(state) = app.try_state::<AppState>() {
                state.update.ask(&app).await;
                state.update.busy.store(false, Ordering::SeqCst);
            }
        });
    }

    async fn ask(&self, app: &AppHandle) {
        let found = match app.updater() {
            Ok(updater) => updater.check().await,
            Err(error) => Err(error),
        };
        let waiting = lock(&self.ready).as_ref().map(|(update, _)| update.version.clone());
        // What the status falls back to: an update already downloaded still waits.
        let rest = |view: &mut UpdateView| {
            (view.status, view.version) = match &waiting {
                Some(version) => ("ready", Some(version.clone())),
                None => ("idle", None),
            };
        };

        let update = match found {
            Ok(Some(update)) if waiting.as_deref() != Some(update.version.as_str()) => update,
            // Nothing newer than what runs or waits, or no build of this release for this system.
            Ok(_) | Err(Error::TargetNotFound(_) | Error::TargetsNotFound(_)) => {
                return self.set(app, |view| {
                    rest(view);
                    view.checked_at = Some(now_ms());
                    view.offline = false;
                });
            }
            Err(error) => {
                eprintln!("[pitwall] update: {error}");
                return self.set(app, |view| {
                    rest(view);
                    view.offline = true;
                });
            }
        };

        let version = update.version.clone();
        if install_path().is_none_or(|path| read_only_place(&path)) {
            return self.set(app, |view| {
                view.status = "available";
                view.version = Some(version);
                view.checked_at = Some(now_ms());
                view.offline = false;
            });
        }

        self.set(app, |view| {
            view.status = "downloading";
            view.version = Some(version.clone());
            view.downloaded = 0;
            view.total = None;
            view.checked_at = Some(now_ms());
            view.offline = false;
        });

        let mut sent = Instant::now();
        let progress = |chunk: usize, total: Option<u64>| {
            let view = {
                let mut view = lock(&self.view);
                view.downloaded += chunk as u64;
                view.total = total;
                view.clone()
            };
            if sent.elapsed() >= PROGRESS_EVERY {
                sent = Instant::now();
                let _ = app.emit_to(MAIN, "update", view);
            }
        };

        match update.download(progress, || {}).await {
            Ok(bytes) => {
                *lock(&self.ready) = Some((update, bytes));
                self.set(app, |view| view.status = "ready");
            }
            Err(error) => {
                eprintln!("[pitwall] update: {error}");
                let step = download_error(&error);
                self.set(app, |view| {
                    view.status = "failed";
                    view.error = Some(step);
                });
            }
        }
    }

    /// The user restarts for the update: what is open is written down, the update installed and
    /// the app started again. Returns only when that failed; the update then still waits. On
    /// Windows, where everything was closed before the attempt, a failure restarts the app as it
    /// is, so that it all comes back.
    pub fn install(&self, app: &AppHandle) -> Result<(), &'static str> {
        let Some((update, bytes)) = lock(&self.ready).take() else {
            return Err("install");
        };
        let Some(state) = app.try_state::<AppState>() else {
            return Err("install");
        };
        self.set(app, |view| view.status = "installing");

        let window = app.get_webview_window(MAIN).is_some_and(|main| main.is_visible().unwrap_or(false));
        if let Err(error) = state.core.save_restore(&update.version, window) {
            eprintln!("[pitwall] update: nothing will be restored: {error}");
        }
        // On Windows the process ends inside `install`, with no exit event: quit's cleanup first.
        #[cfg(windows)]
        state.core.dispose();

        match update.install(&bytes) {
            Ok(()) => app.restart(),
            Err(error) => {
                eprintln!("[pitwall] update: {error}");
                if cfg!(windows) {
                    app.restart();
                }
                state.core.discard_restore();
                *lock(&self.ready) = Some((update, bytes));
                self.set(app, |view| {
                    view.status = "failed";
                    view.error = Some("install");
                });
                Err("install")
            }
        }
    }

    /// The app quits with an update waiting: it is installed on the way out, where that asks
    /// nothing (`quiet_install`). Nothing is restored after a quit the user chose.
    pub fn install_on_quit(&self) {
        if !lock(&self.view).on_quit {
            return;
        }
        if let Some((update, bytes)) = lock(&self.ready).take() {
            if let Err(error) = update.install(&bytes) {
                eprintln!("[pitwall] update: {error}");
            }
        }
    }
}

/// At the start after a restart for an update: the window comes back when it was up, and one
/// quiet notification says what was put back, when the version asked for is the one running.
pub fn restored(app: &AppHandle, restored: &Restored) {
    if restored.window {
        tray::show_main(app);
    }
    if app.package_info().version.to_string() != restored.version {
        return;
    }

    let title = t!("update.restored.title", version = restored.version);
    let body = t!("update.restored.body", servers = restored.servers, terminals = restored.terminals, sessions = restored.sessions);
    crate::notify::post(NOTIFICATION_ID.to_string(), title, body, None);
}
