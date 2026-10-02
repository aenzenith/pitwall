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
use crate::{AppState, MAIN, POPOVER, SWITCHER};

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
fn hand_back_focus() {
    let pid = PREVIOUS_APP.swap(0, std::sync::atomic::Ordering::SeqCst);

    #[cfg(target_os = "macos")]
    if pid != 0 {
        if let Some(previous) = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            previous.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = pid;
}

/// Closes the switcher on purpose and returns to whatever was in front before.
pub fn dismiss_switcher<R: Runtime>(app: &AppHandle<R>) {
    if let Some(switcher) = app.get_webview_window(SWITCHER) {
        if switcher.is_visible().unwrap_or(false) {
            let _ = switcher.hide();
            hand_back_focus();
        }
    }
}

/// Closes the popover on purpose (Esc) and returns to whatever was in front before.
pub fn dismiss_popover<R: Runtime>(app: &AppHandle<R>) {
    let visible = app.get_webview_window(POPOVER).and_then(|p| p.is_visible().ok()).unwrap_or(false);
    hide_popover(app);
    if visible {
        hand_back_focus();
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
    let _ = popover.show();
    let _ = popover.set_focus();

    if let Some(state) = app.try_state::<AppState>() {
        state.core.popover_opened();
    }
}

pub fn hide_popover<R: Runtime>(app: &AppHandle<R>) {
    if let Some(popover) = app.get_webview_window(POPOVER) {
        if popover.is_visible().unwrap_or(false) {
            let _ = popover.hide();
            if let Ok(mut at) = LAST_HIDE.lock() {
                *at = Some(Instant::now());
            }
        }
    }
}

/// The quick switcher: a centred palette for jumping to a project from the keyboard.
pub fn toggle_switcher<R: Runtime>(app: &AppHandle<R>) {
    let Some(switcher) = app.get_webview_window(SWITCHER) else {
        return;
    };

    // The shortcut toggles: pressed again, it closes and hands focus back.
    if switcher.is_visible().unwrap_or(false) {
        dismiss_switcher(app);
        return;
    }

    // Coming from the popover's search button, the app to return to is the popover's.
    let from_popover = app.get_webview_window(POPOVER).and_then(|p| p.is_visible().ok()).unwrap_or(false);
    if !from_popover {
        note_front_app(app);
    }

    hide_popover(app);
    if !center_on_pointer(app, &switcher) {
        let _ = switcher.move_window(Position::Center);
    }
    let shown = switcher.show();
    let focused = switcher.set_focus();
    #[cfg(debug_assertions)]
    eprintln!("[pitwall] switcher show={shown:?} focus={focused:?}");
    let _ = (shown, focused);
    let _ = switcher.emit("switcher-opened", ());
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
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    hide_popover(app);

    if let Some(main) = app.get_webview_window(MAIN) {
        set_dock_visible(app, true);
        let _ = main.show();
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
