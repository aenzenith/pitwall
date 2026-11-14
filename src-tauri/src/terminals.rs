//! The terminals' own window (`Settings::terminal_place`, `window`): the terminal panel taken out
//! of the main window into a window of its own, whose page shows the same terminals (each one's
//! output is the core's, `terminal_buffer`). Made when the panel goes there, and again when the
//! main window's page comes up while it stands there; closed, the panel goes back where it stood.
//!
//! What the panel's page hands from one window to the other (its tabs, the one shown) is the
//! UI's own, opaque here: kept in memory only, never written down.
//!
//! Its place is kept as the mini tower's is (`tower.rs`): AppKit's points on macOS, the top
//! counted downwards from the main screen's; physical pixels elsewhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};

use crate::i18n::t;
use crate::tower::{self, Rect, Screen};
use crate::{tray, windows, AppState, MAIN, TERMINALS};

/// Its size the first time, as `tauri.conf.json` has it.
const SIZE: (f64, f64) = (900.0, 560.0);
/// The smallest it may be (`minWidth`, `minHeight`).
const MIN: (f64, f64) = (520.0, 300.0);
/// Where the point a panel was let go at lies in the window made for it: in its title bar, near
/// its left edge, as if it had been carried there by it.
const GRIP: (f64, f64) = (40.0, 14.0);
/// The largest panel state its page may leave here.
const MAX_PANEL: usize = 512 * 1024;
/// A place is written down once the window has stood still this long: a drag is one write.
const STILL: Duration = Duration::from_millis(400);

/// The panel's state as its page last gave it: what the window's page starts from, and what the
/// main window's page takes back when the panel leaves the window.
static PANEL: Mutex<Option<String>> = Mutex::new(None);
/// The window's next coming up gives it the keyboard.
static KEYBOARD: AtomicBool = AtomicBool::new(false);
/// Where the window was last seen standing, and when: written down once it has stood still.
static LEFT: Mutex<Option<([i32; 4], Instant)>> = Mutex::new(None);
/// A thread is waiting for it to stand still.
static WRITING: AtomicBool = AtomicBool::new(false);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The panel stands in its window now (as Settings says, whether or not the window is up yet).
pub fn out<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<AppState>().is_some_and(|state| state.core.settings().terminal_place.mode() == "window")
}

/// The window is up, shown or minimised: it is never hidden, only closed.
pub fn up<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(TERMINALS).is_some()
}

/// The panel goes to `mode` (`dock`, `float` or `window`). To its window it takes `panel`, its
/// page's state, and comes up there with the keyboard, at `at` when it was let go at a point of
/// the screen (from the page: points on macOS, CSS pixels elsewhere). Out of its window it
/// gives the main window's page its state back (the one given, else the window's page's last),
/// and the main window comes up in the window's place.
pub fn set_place<R: Runtime>(app: &AppHandle<R>, mode: &str, panel: Option<String>, at: Option<[i32; 2]>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let panel = panel.filter(|panel| panel.len() <= MAX_PANEL);
    if mode == "window" {
        if let Some(panel) = &panel {
            *lock(&PANEL) = Some(panel.clone());
        }
    }
    let from = state.core.set_terminal_place(mode)?;
    if mode == "window" {
        open(app, true, at);
        return Ok(());
    }

    let window = app.get_webview_window(TERMINALS);
    let panel = if from == "window" {
        windows::forget(TERMINALS);
        let kept = lock(&PANEL).take();
        panel.or(kept)
    } else {
        panel
    };
    if let Some(panel) = panel {
        let _ = app.emit_to(MAIN, "panel-state", panel);
    }
    if from == "window" || window.is_some() {
        tray::show_main(app);
    }
    // Not `close`: that would ask the window first (`closing`), and its panel is home already.
    if let Some(window) = window {
        let _ = window.destroy();
    }
    Ok(())
}

/// The panel's state as the window's page last left it: what that page starts from as it mounts,
/// again after a reload.
pub fn panel() -> Option<String> {
    lock(&PANEL).clone()
}

/// The window's page leaves its panel's state here, for a reload and for the main window's page
/// when the panel goes back there. One too large to be a panel's is let go.
pub fn keep_panel(panel: String) {
    if panel.len() <= MAX_PANEL {
        *lock(&PANEL) = Some(panel);
    }
}

/// Something the main window's page asks of the panel while it is in its window (`ask`, JSON:
/// show a project's terminals, a new tab…), passed on to the window's page as `panel-ask`, kept
/// in order until it listens. With `show`, the window comes up with the keyboard.
pub fn ask<R: Runtime>(app: &AppHandle<R>, ask: &str, show: bool) {
    if !out(app) {
        return;
    }
    // Sent on as JSON written here, never as the text given.
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(ask).and_then(|ask| serde_json::to_string(&ask)) else {
        return;
    };
    if show {
        open(app, true, None);
    }
    if !windows::wait_for(TERMINALS, "panel-ask", &payload) {
        let _ = app.emit_str_to(TERMINALS, "panel-ask", payload);
    }
}

/// A terminal to show (`reveal-terminal`) while the panel is in its window: the window comes up
/// with the keyboard, and every page hears it, the window's own once it listens.
pub fn reveal<R: Runtime>(app: &AppHandle<R>, payload: &impl serde::Serialize) {
    let Ok(payload) = serde_json::to_string(payload) else {
        return;
    };
    open(app, true, None);
    if !windows::wait_for(TERMINALS, "reveal-terminal", &payload) {
        let _ = app.emit_str("reveal-terminal", payload);
    }
}

/// The main window's page is up: with the panel in its window and no such window yet (a start,
/// the window made again), it comes up beside it, leaving it the keyboard.
pub fn main_ready<R: Runtime>(app: &AppHandle<R>) {
    if out(app) && app.get_webview_window(TERMINALS).is_none() {
        open(app, false, None);
    }
}

/// Brings the window up: made, named and put in its place if need be, or put at `at` (a point it
/// was let go at), above other windows as Settings says. It shows once its page is ready, with
/// the keyboard (`keyboard`) or leaving it where it is.
pub fn open<R: Runtime>(app: &AppHandle<R>, keyboard: bool, at: Option<[i32; 2]>) {
    let made = app.get_webview_window(TERMINALS).is_none();
    let Some(window) = windows::get_or_create(app, TERMINALS) else {
        return;
    };
    let _ = window.set_title(&t!("core.window.terminals"));
    if made || at.is_some() {
        place(&window, at);
    }
    if let Some(state) = app.try_state::<AppState>() {
        let _ = window.set_always_on_top(state.core.settings().terminal_place.on_top);
    }
    if keyboard {
        KEYBOARD.store(true, Ordering::SeqCst);
    }
    // As for the main window: the Dock icon comes while it is open.
    tray::set_dock_visible(app, true);
    windows::open(app, TERMINALS);
}

/// Puts the window up once its page is ready (`windows::present`): in front with the keyboard
/// when that was asked for, else in front of Pitwall's other windows without it.
pub fn present<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(TERMINALS) else {
        return;
    };
    let _ = window.unminimize();
    if KEYBOARD.swap(false, Ordering::SeqCst) {
        let _ = windows::show(&window);
        let _ = window.set_focus();
    } else {
        show_quietly(&window);
    }
}

/// Shows the window without the keyboard: the window that has it keeps it.
#[cfg(target_os = "macos")]
fn show_quietly<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let app = window.app_handle().clone();
    let _ = window.run_on_main_thread(move || {
        // SAFETY: as in `tray::focus_alone`: this window's NSWindow, on the main thread.
        let ns_window = unsafe { &*(ns_window as *const objc2_app_kit::NSWindow) };
        // Not `show`, which makes it the key window.
        ns_window.orderFront(None);
        windows::tell(&app, TERMINALS, true);
    });
}

#[cfg(not(target_os = "macos"))]
fn show_quietly<R: Runtime>(window: &WebviewWindow<R>) {
    let main = window.app_handle().get_webview_window(MAIN).filter(|main| main.is_focused().unwrap_or(false));
    let _ = windows::show(window);
    // Shown, a window takes the keyboard here: the main window has it back.
    if let Some(main) = main {
        let _ = main.set_focus();
    }
}

/// The user closes the window (its close button, or its page): the panel goes back where it
/// stood before, its state with it to the main window's page, and nothing more waits for the
/// window's page. With the main window put away too (not minimised), the Dock icon goes.
pub fn closing<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    windows::forget(TERMINALS);
    if state.core.terminals_closed() {
        if let Some(panel) = lock(&PANEL).take() {
            let _ = app.emit_to(MAIN, "panel-state", panel);
        }
    }
    let main = app.get_webview_window(MAIN).is_some_and(|main| main.is_visible().unwrap_or(false) || main.is_minimized().unwrap_or(false));
    if !main {
        tray::set_dock_visible(app, false);
    }
}

/// Above other windows, or not, as Settings now says.
pub fn apply_on_top<R: Runtime>(app: &AppHandle<R>, on: bool) {
    if let Some(window) = app.get_webview_window(TERMINALS) {
        let _ = window.set_always_on_top(on);
    }
}

/// Where the window stands as it is made: by its title bar at `at` (a point it was let go at)
/// on that point's screen; else where it was left (`left`) while that is still on a screen;
/// else over the main window (`over`), else in the middle of the main screen. As large as it
/// was left (else `SIZE`), never smaller than `MIN` nor larger than its screen, `scale` turning
/// those into the screens' units. None without a screen.
fn settle(left: Option<Rect>, at: Option<(f64, f64)>, over: Option<Rect>, screens: &[Screen], scale: f64) -> Option<Rect> {
    let main = screens.first()?;
    let (w, h) = left.map_or((SIZE.0 * scale, SIZE.1 * scale), |left| (left.w, left.h));
    // Inside the screen's free area and off its Dock, in whole points (or pixels).
    let fit = |rect: Rect, screen: &Screen| {
        let area = screen.area;
        let sized = Rect { w: rect.w.max(MIN.0 * scale).min(area.w), h: rect.h.max(MIN.1 * scale).min(area.h), ..rect };
        let to = tower::clear(sized.within(area), area, screen.dock);
        Rect { x: to.x.round(), y: to.y.round(), w: to.w.round(), h: to.h.round() }
    };

    if let Some((x, y)) = at {
        let rect = Rect { x: x - GRIP.0 * scale, y: y - GRIP.1 * scale, w, h };
        let point = Rect { x, y, w: 1.0, h: 1.0 };
        let screen = screens.iter().find(|screen| screen.frame.overlaps(point)).copied().or_else(|| tower::screen_of(rect, screens)).unwrap_or(*main);
        return Some(fit(rect, &screen));
    }
    if let Some(left) = left {
        if let Some(screen) = tower::screen_of(left, screens) {
            return Some(Rect { w: left.w.min(screen.area.w), h: left.h.min(screen.area.h), ..left });
        }
    }
    let centred = |frame: Rect| Rect { x: frame.x + (frame.w - w) / 2.0, y: frame.y + (frame.h - h) / 2.0, w, h };
    Some(match over.and_then(|over| tower::screen_of(over, screens).map(|screen| (over, screen))) {
        Some((over, screen)) => fit(centred(over), &screen),
        None => fit(centred(main.area), main),
    })
}

/// Puts the window where it stands as it is made (`settle`); one let go at a point is left there
/// from now on.
#[cfg(target_os = "macos")]
fn place<R: Runtime>(window: &WebviewWindow<R>, at: Option<[i32; 2]>) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let main = window.app_handle().get_webview_window(MAIN).and_then(|main| main.ns_window().ok()).map(|main| main as usize);
    let app = window.app_handle().clone();

    let _ = window.run_on_main_thread(move || {
        use objc2_app_kit::NSWindow;

        let (Some(mtm), Some(state)) = (objc2::MainThreadMarker::new(), app.try_state::<AppState>()) else {
            return;
        };
        let (screens, top) = tower::mac::screens(mtm);
        // SAFETY: as in `tray::focus_alone`: the main window's NSWindow, on the main thread.
        let over = main.map(|main| tower::mac::frame(unsafe { &*(main as *const NSWindow) }, top));
        let left = state.core.settings().terminal_place.window.map(Rect::of);
        let at = at.map(|[x, y]| (f64::from(x), f64::from(y)));
        let Some(to) = settle(left, at, over, &screens, 1.0) else {
            return;
        };
        // SAFETY: as above, this window's.
        tower::mac::set_frame(unsafe { &*(ns_window as *const NSWindow) }, to, top);
        if at.is_some() {
            state.core.set_terminals_frame(to.rounded());
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn place<R: Runtime>(window: &WebviewWindow<R>, at: Option<[i32; 2]>) {
    use tauri::{PhysicalPosition, PhysicalSize};

    let app = window.app_handle();
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let main = app.get_webview_window(MAIN);
    let over = main.as_ref().and_then(tower::frame);
    // The page's point is in its CSS pixels.
    let page = main.as_ref().and_then(|main| main.scale_factor().ok()).unwrap_or(1.0);
    let at = at.map(|[x, y]| (f64::from(x) * page, f64::from(y) * page));
    let left = state.core.settings().terminal_place.window.map(Rect::of);
    let scale = window.scale_factor().unwrap_or(1.0);
    let Some(to) = settle(left, at, over, &tower::screens(app), scale) else {
        return;
    };

    // The frame is the window's outside; its size is set without the system's frame round it.
    let (outer, inner) = (window.outer_size().ok(), window.inner_size().ok());
    let (edge_w, edge_h) = outer.zip(inner).map_or((0, 0), |(outer, inner)| (outer.width.saturating_sub(inner.width), outer.height.saturating_sub(inner.height)));
    let (w, h) = ((to.w.round() as u32).saturating_sub(edge_w), (to.h.round() as u32).saturating_sub(edge_h));
    let _ = window.set_size(PhysicalSize::new(w.max(1), h.max(1)));
    let _ = window.set_position(PhysicalPosition::new(to.x.round() as i32, to.y.round() as i32));
    if at.is_some() {
        state.core.set_terminals_frame(to.rounded());
    }
}

/// The window moved or changed its size: where it stands now is where it comes back, after a
/// close and after a restart. Only while it shows as a window: not minimised, nor full screen (nor
/// maximised, off macOS).
pub fn frame_changed<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(TERMINALS) else {
        return;
    };
    let maximized = cfg!(not(target_os = "macos")) && window.is_maximized().unwrap_or(false);
    if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(true) || window.is_fullscreen().unwrap_or(true) || maximized {
        return;
    }
    let Some(frame) = tower::frame(&window).map(Rect::rounded) else {
        return;
    };

    *lock(&LEFT) = Some((frame, Instant::now()));
    if WRITING.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    thread::spawn(move || loop {
        thread::sleep(STILL);
        let mut left = lock(&LEFT);
        match *left {
            Some((_, at)) if at.elapsed() < STILL => {}
            Some((frame, _)) => {
                *left = None;
                drop(left);
                if let Some(state) = app.try_state::<AppState>() {
                    state.core.set_terminals_frame(frame);
                }
            }
            // Under the lock: a move after this starts a thread of its own.
            None => {
                WRITING.store(false, Ordering::SeqCst);
                return;
            }
        }
    });
}

/// The app is on its way out: a place still waiting to stand still is written now.
pub fn flush(core: &crate::core::Core) {
    if let Some((frame, _)) = lock(&LEFT).take() {
        core.set_terminals_frame(frame);
    }
}
