use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    image::Image,
    Emitter,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, LogicalPosition, Manager, Monitor, PhysicalPosition, Runtime, Theme, WebviewWindow,
};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::core::Snapshot;
use crate::i18n::t;
use crate::{windows, AppState, MAIN, POPOVER, SWITCHER};

const TRAY_ID: &str = "pitwall";

/// The app that was in front before the popover or switcher took focus, so dismissing them
/// hands focus straight back (as Spotlight does) instead of raising Pitwall's main window.
/// 0 = Pitwall itself was in front; nothing to hand back.
static PREVIOUS_APP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Remember who is in front, unless it is Pitwall already.
fn note_front_app<R: Runtime>(app: &AppHandle<R>) {
    let ours = [MAIN, POPOVER, SWITCHER]
        .iter()
        .any(|label| app.get_webview_window(label).and_then(|w| w.is_focused().ok()).unwrap_or(false));

    #[cfg(target_os = "macos")]
    let pid = if ours {
        0
    } else {
        objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|front| front.processIdentifier())
            .filter(|pid| *pid as u32 != std::process::id())
            .unwrap_or(0)
    };
    #[cfg(not(target_os = "macos"))]
    let pid = {
        let _ = ours;
        0
    };

    PREVIOUS_APP.store(pid, std::sync::atomic::Ordering::SeqCst);
}

/// After an explicit dismiss (shortcut, Esc, a pick): the app from before gets focus back.
/// Returns whether there was one to hand it to.
fn hand_back_focus() -> bool {
    let pid = PREVIOUS_APP.swap(0, std::sync::atomic::Ordering::SeqCst);

    #[cfg(target_os = "macos")]
    if pid != 0 {
        if let Some(previous) = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            return previous.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = pid;
    false
}

/// Whether Pitwall is the active app. Opened by its shortcut, the switcher (a non-activating
/// panel) leaves it inactive; opened from the popover, it is active.
pub fn pitwall_active() -> bool {
    #[cfg(target_os = "macos")]
    return objc2_app_kit::NSRunningApplication::currentApplication().isActive();
    #[cfg(not(target_os = "macos"))]
    true
}

/// Hides a focused window once Pitwall has stepped back. Hidden while Pitwall is still the
/// active app, its focus would pass to Pitwall's main window, which then flashed up for a few
/// milliseconds; so the window goes when it loses focus (its blur handler hides it) and, should
/// that not come, after `wait`.
pub fn hide_once_focus_moves<R: Runtime>(window: WebviewWindow<R>, wait: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        if window.is_visible().unwrap_or(false) {
            let _ = windows::hide(&window);
        }
    });
}

/// Closes the switcher on purpose and returns to whatever was in front before: that app first,
/// then the switcher, so no Pitwall window comes up in between.
pub fn dismiss_switcher<R: Runtime>(app: &AppHandle<R>) {
    if let Some(switcher) = app.get_webview_window(SWITCHER) {
        if switcher.is_visible().unwrap_or(false) {
            if hand_back_focus() {
                hide_once_focus_moves(switcher, Duration::from_millis(300));
            } else {
                let _ = windows::hide(&switcher);
            }
        }
    }
}

/// Closes the popover on purpose (Esc) and returns to whatever was in front before, in the
/// same order as the switcher.
pub fn dismiss_popover<R: Runtime>(app: &AppHandle<R>) {
    let Some(popover) = app.get_webview_window(POPOVER) else {
        return;
    };
    if !popover.is_visible().unwrap_or(false) {
        return;
    }
    if hand_back_focus() {
        hide_once_focus_moves(popover, Duration::from_millis(300));
    } else {
        hide_popover(app);
    }
}

/// When the popover last hid. A click on the icon while it is open first blurs the popover
/// (hiding it), then arrives as a click; without this the click would reopen it at once.
static LAST_HIDE: Mutex<Option<Instant>> = Mutex::new(None);

/// Menu bar icon: a template image, so macOS tints it for light and dark menu bars.
pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("Pitwall")
        .on_tray_icon_event(|tray, event| {
            // The positioner needs every tray event to know where the icon is.
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_popover(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn toggle_popover<R: Runtime>(app: &AppHandle<R>) {
    let Some(popover) = app.get_webview_window(POPOVER) else {
        return;
    };

    if popover.is_visible().unwrap_or(false) {
        dismiss_popover(app);
        return;
    }

    let just_hidden = LAST_HIDE.lock().ok().and_then(|at| *at).is_some_and(|at| at.elapsed() < Duration::from_millis(250));

    if just_hidden {
        return;
    }

    // Under the icon; before the icon was ever clicked (shortcut), the top-right corner.
    if popover.move_window(Position::TrayBottomCenter).is_err() {
        let _ = popover.move_window(Position::TopRight);
    }

    note_front_app(app);
    let _ = windows::show(&popover);
    let _ = popover.set_focus();

    if let Some(state) = app.try_state::<AppState>() {
        state.core.popover_opened();
    }
}

pub fn hide_popover<R: Runtime>(app: &AppHandle<R>) {
    if let Some(popover) = app.get_webview_window(POPOVER) {
        if popover.is_visible().unwrap_or(false) {
            let _ = windows::hide(&popover);
            if let Ok(mut at) = LAST_HIDE.lock() {
                *at = Some(Instant::now());
            }
        }
    }
}

/// The quick switcher: a centred palette for jumping to a project from the keyboard.
pub fn toggle_switcher<R: Runtime>(app: &AppHandle<R>) {
    // The shortcut toggles: pressed again, it closes and hands focus back.
    if app.get_webview_window(SWITCHER).is_some_and(|switcher| switcher.is_visible().unwrap_or(false)) {
        dismiss_switcher(app);
        return;
    }

    // Coming from the popover's search button, the app to return to is the popover's. Otherwise
    // there is none: the switcher never takes the front from the app you are in (`make_panel`).
    let from_popover = app.get_webview_window(POPOVER).and_then(|p| p.is_visible().ok()).unwrap_or(false);
    if !from_popover {
        PREVIOUS_APP.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    // Made on first use; it comes up once its page listens for `switcher-opened`.
    windows::open(app, SWITCHER);
}

/// Puts the switcher up on the screen the pointer is on, with the keyboard.
pub fn present_switcher<R: Runtime>(app: &AppHandle<R>) {
    let Some(switcher) = app.get_webview_window(SWITCHER) else {
        return;
    };

    // The popover, if open, steps aside by itself once the switcher has the keyboard (its blur
    // hides it); hiding it first would hand the keyboard to Pitwall's window for a moment.
    if !center_on_pointer(app, &switcher) {
        let _ = switcher.move_window(Position::Center);
    }
    let shown = windows::show(&switcher);
    focus_alone(&switcher);
    #[cfg(debug_assertions)]
    eprintln!("[pitwall] switcher show={shown:?}");
    let _ = shown;
    let _ = switcher.emit("switcher-opened", ());
}

/// Gives the switcher the keyboard. It is a non-activating panel (`make_panel`), so Pitwall is
/// not brought to the front and none of its other windows come along.
#[cfg(target_os = "macos")]
fn focus_alone<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(ns_window) = window.ns_window() else {
        let _ = window.set_focus();
        return;
    };
    let ns_window = ns_window as usize;

    let _ = window.run_on_main_thread(move || {
        // SAFETY: the pointer is this window's NSWindow, alive while the window is, and AppKit
        // is only touched here, on the main thread.
        let ns_window = unsafe { &*(ns_window as *const objc2_app_kit::NSWindow) };
        ns_window.makeKeyAndOrderFront(None);
    });
}

/// Turns the switcher into a non-activating panel, as Spotlight's is: it takes the keyboard while
/// the app you are in stays in front. Activating Pitwall for it raised Pitwall's main window too,
/// and now and then that window took the keyboard and the search went away. Once, at start.
///
/// The window's class becomes a subclass of NSPanel. NSPanel adds no fields to NSWindow, and the
/// one field of tao's window class, `focusable`, is declared again in the same place, so tao's
/// window object stays valid as it is.
#[cfg(target_os = "macos")]
pub fn make_panel<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;

    let _ = window.run_on_main_thread(move || {
        use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
        use objc2::{msg_send, sel, ClassType};
        use objc2_app_kit::{NSPanel, NSWindow, NSWindowStyleMask};

        extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool {
            Bool::YES
        }
        extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
            Bool::NO
        }

        let class = AnyClass::get(c"PitwallPanel").or_else(|| {
            let mut builder = ClassBuilder::new(c"PitwallPanel", NSPanel::class())?;
            builder.add_ivar::<Bool>(c"focusable");
            // SAFETY: both methods match their selectors' signatures (no arguments, BOOL).
            unsafe {
                builder.add_method(sel!(canBecomeKeyWindow), yes as extern "C-unwind" fn(_, _) -> _);
                builder.add_method(sel!(canBecomeMainWindow), no as extern "C-unwind" fn(_, _) -> _);
            }
            Some(builder.register())
        });
        let Some(class) = class else {
            return;
        };

        // SAFETY: this window's NSWindow, on the main thread; the new class has the same layout
        // (see above).
        unsafe { objc2::ffi::object_setClass(ns_window as *mut AnyObject, class) };
        let ns_window = unsafe { &*(ns_window as *const NSWindow) };
        ns_window.setStyleMask(ns_window.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        // Hiding is the blur handler's job, not AppKit's.
        ns_window.setHidesOnDeactivate(false);
        let _: () = unsafe { msg_send![ns_window, setBecomesKeyOnlyIfNeeded: false] };
    });
}

#[cfg(not(target_os = "macos"))]
fn focus_alone<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.set_focus();
}

/// The popover and the switcher show on the Space you are on, a full-screen app's included.
/// Otherwise macOS takes you to the Space they were last shown on, where Pitwall's window may be.
#[cfg(target_os = "macos")]
pub fn float_over_spaces<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;

    let _ = window.run_on_main_thread(move || {
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        // SAFETY: as in `focus_alone`: this window's NSWindow, on the main thread.
        let ns_window = unsafe { &*(ns_window as *const NSWindow) };
        ns_window.setCollectionBehavior(ns_window.collectionBehavior() | NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
    });
}

/// Centres a window on the screen the pointer is on, so the switcher opens where you are working.
fn center_on_pointer<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) -> bool {
    let (Ok(cursor), Ok(monitors), Ok(size), Ok(window_scale)) = (app.cursor_position(), app.available_monitors(), window.outer_size(), window.scale_factor()) else {
        return false;
    };

    // macOS lays every screen out in points, but reports each screen's geometry scaled by its own
    // factor and the pointer by the main screen's; dividing back puts them in one space. Windows
    // and Linux already share physical pixels.
    let points = cfg!(target_os = "macos");
    let unit = |scale: f64| if points { scale } else { 1.0 };
    let main = app.primary_monitor().ok().flatten().map_or(1.0, |m| m.scale_factor());
    let (px, py) = (cursor.x / unit(main), cursor.y / unit(main));

    let bounds = |m: &Monitor| {
        let u = unit(m.scale_factor());
        (f64::from(m.position().x) / u, f64::from(m.position().y) / u, f64::from(m.size().width) / u, f64::from(m.size().height) / u)
    };
    let Some((x, y, w, h)) = monitors.iter().map(bounds).find(|&(x, y, w, h)| px >= x && px < x + w && py >= y && py < y + h) else {
        return false;
    };

    let u = unit(window_scale);
    let left = x + (w - f64::from(size.width) / u) / 2.0;
    let top = y + (h - f64::from(size.height) / u) / 2.0;

    let placed = if points { window.set_position(LogicalPosition::new(left, top)) } else { window.set_position(PhysicalPosition::new(left.round() as i32, top.round() as i32)) };
    placed.is_ok()
}

/// Shows the main window. The Dock icon appears while it is open, like any other app window.
/// Made on first use, it comes up once its page is ready; the Dock icon comes at once.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    hide_popover(app);

    if windows::get_or_create(app, MAIN).is_some() {
        set_dock_visible(app, true);
        windows::open(app, MAIN);
    }
}

/// Puts the main window up, in front, with the keyboard.
pub fn present_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = windows::show(&main);
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

#[allow(unused_variables)]
pub fn set_dock_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(if visible {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    });
}

/// Count next to the icon and the coloured dot. Waiting projects win over the running count;
/// a crash shows a red dot until the popover is opened.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, snapshot: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let count = if snapshot.waiting > 0 { snapshot.waiting } else { snapshot.running };
    let _ = tray.set_title(if count > 0 { Some(count.to_string()) } else { None::<String> });

    let dark = app.get_webview_window(POPOVER).and_then(|w| w.theme().ok()).is_none_or(|theme| theme == Theme::Dark);
    let variant: Option<&[u8]> = match (snapshot.waiting > 0, snapshot.crash_unseen, dark) {
        (true, _, true) => Some(include_bytes!("../icons/tray-claude-dark.png")),
        (true, _, false) => Some(include_bytes!("../icons/tray-claude-light.png")),
        (false, true, true) => Some(include_bytes!("../icons/tray-crash-dark.png")),
        (false, true, false) => Some(include_bytes!("../icons/tray-crash-light.png")),
        _ => None,
    };

    match variant.and_then(|bytes| Image::from_bytes(bytes).ok()) {
        Some(image) => {
            let _ = tray.set_icon(Some(image));
            let _ = tray.set_icon_as_template(false);
        }
        None => {
            if let Ok(image) = Image::from_bytes(include_bytes!("../icons/tray.png")) {
                let _ = tray.set_icon(Some(image));
                let _ = tray.set_icon_as_template(true);
            }
        }
    }

    let tooltip = t!("core.tray.tooltip", running = snapshot.running, waiting = snapshot.waiting);
    let _ = tray.set_tooltip(Some(tooltip));
}
