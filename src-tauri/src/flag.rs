//! The chequered flag at a quit: the window's side of `core/flag.rs`.
//!
//! How a quit reaches Pitwall, and which of them ask first:
//!
//! - Pitwall's own menus and buttons come through `request_quit`: the popover's quit button
//!   (`commands::quit`), the tray menu's Quit on Windows and Linux (`tray::menu`) and, on macOS,
//!   the app menu's "Quit Pitwall" with its ⌘Q (`own_quit_item`). While Settings says so and
//!   something is open, such a quit brings the window up with the dialog instead.
//! - Everything else never comes here and quits as it always did: a restart for an update
//!   (`AppHandle::restart`, `update.rs`) and its install on the way out (`RunEvent::Exit`);
//!   AppKit's own `terminate:`, which is how the Dock's Quit and the system logging out,
//!   restarting or shutting down all arrive (one Apple event; the three can't be told apart
//!   here, so none of them asks); a signal (a development run replacing the app); and a second
//!   instance, which hands over to this one and leaves by itself.
//!
//! A held quit never traps the app: without a window the quit goes on, a dialog that didn't come
//! up within `SHOW_WITHIN` lets it go on, and so does a second quit asked for meanwhile.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::{tray, windows, AppState, MAIN};

/// The main window's page hears this when a quit is held: its dialog comes up.
const ASK: &str = "flag-ask";
/// How long a quit waits at most for the projects' Git state to be read again.
pub const FRESH: Duration = Duration::from_millis(1200);
/// A held quit whose dialog isn't up after this long goes on: the window couldn't show it.
const SHOW_WITHIN: Duration = Duration::from_secs(6);
/// A quit asked for again this soon after one was held is the same press (⌘Q held down, a
/// double click): it has its dialog, and is no answer to it.
const SAME_PRESS: Duration = Duration::from_millis(1500);

/// One quit is weighed at a time.
static WEIGHING: AtomicBool = AtomicBool::new(false);
/// The quits weighed so far, and the last of them whose dialog came up.
static ASKED: AtomicU64 = AtomicU64::new(0);
static SHOWN: AtomicU64 = AtomicU64::new(0);
/// When the last quit was held.
static HELD_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// A quit asked for from one of Pitwall's own menus or buttons: it goes on at once, or the main
/// window comes up with what is still open. Off the caller's thread: it may read Git.
pub fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    if WEIGHING.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    thread::spawn(move || {
        let held = hold(&app);
        WEIGHING.store(false, Ordering::SeqCst);
        if !held {
            app.exit(0);
        }
    });
}

/// Whether the quit is held: true once the window is on its way up with the dialog.
fn hold<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let core = &state.core;

    if core.quit_waits() {
        let same_press = HELD_AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some_and(|at| at.elapsed() < SAME_PRESS);
        // Asked again while the dialog is up, or while it never came: the core lets it go on.
        return same_press || core.quit_asked();
    }
    if !core.settings().flag.at_quit {
        return false;
    }

    core.flag_refresh(FRESH);
    // Counted before the core says a quit waits: a page that learns of it from the core's
    // answer and says its dialog is up (`shown`) says so of this quit, never of the one before.
    let ask = ASKED.fetch_add(1, Ordering::SeqCst) + 1;
    if !core.quit_asked() {
        return false;
    }
    // Without the window there is no dialog to answer.
    if windows::get_or_create(app, MAIN).is_none() {
        core.quit_dropped();
        return false;
    }

    *HELD_AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
    tray::show_main(app);
    // A page that isn't listening yet asks for itself once it is (`flag_state`: `asked`).
    let _ = app.emit_to(MAIN, ASK, ());

    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(SHOW_WITHIN);
        let unshown = ASKED.load(Ordering::SeqCst) == ask && SHOWN.load(Ordering::SeqCst) != ask;
        if unshown && app.try_state::<AppState>().is_some_and(|state| state.core.quit_waits()) {
            app.exit(0);
        }
    });
    true
}

/// The dialog is up for the quit that was held last.
pub fn shown() {
    SHOWN.store(ASKED.load(Ordering::SeqCst), Ordering::SeqCst);
}

/// The app menu's Quit, on macOS.
#[cfg(target_os = "macos")]
const QUIT_ITEM: &str = "pitwall:app-quit";

/// On macOS the app menu's Quit is AppKit's own (`terminate:`), which nothing here can hold. It
/// is taken out and an item of Pitwall's put in its place, with the same words and the same ⌘Q.
/// Should any step fail, AppKit's stays, and ⌘Q quits as it always did.
#[cfg(target_os = "macos")]
pub fn own_quit_item(app: &AppHandle) {
    use tauri::menu::MenuItem;

    // Whether Pitwall's item went in.
    let swap = || -> tauri::Result<bool> {
        // The app's own submenu comes first, and Quit is its last item (`Menu::default`).
        let Some(menu) = app.menu() else {
            return Ok(false);
        };
        let submenus = menu.items()?;
        let Some(own) = submenus.first().and_then(|item| item.as_submenu()) else {
            return Ok(false);
        };
        let items = own.items()?;
        let Some(quit) = items.last().and_then(|item| item.as_predefined_menuitem()) else {
            return Ok(false);
        };

        // Pitwall's goes in before AppKit's comes out: the menu is never without a Quit. Should
        // AppKit's stay after all, it is the first with ⌘Q, and the key quits as it always did.
        own.append(&MenuItem::with_id(app, QUIT_ITEM, quit.text()?, true, Some("CmdOrCtrl+Q"))?)?;
        let _ = own.remove(quit);
        Ok(true)
    };
    match swap() {
        Ok(true) => {}
        Ok(false) => return,
        Err(_error) => {
            #[cfg(debug_assertions)]
            eprintln!("[pitwall] quit item: {_error}");
            return;
        }
    }

    app.on_menu_event(|app, event| {
        if event.id().as_ref() == QUIT_ITEM {
            request_quit(app);
        }
    });
}
