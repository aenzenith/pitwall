use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    image::Image,
    Emitter,
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, LogicalPosition, Manager, Monitor, PhysicalPosition, Runtime, WebviewWindow,
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

/// Menu bar icon: a template image, so macOS tints it for light and dark menu bars. On Windows
/// and Linux the tray also has a menu (`menu`): Linux delivers no clicks on the icon, so the menu
/// is all there is; Windows opens it with the right button.
pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(plain_icon(bar_is_dark(app))?)
        .icon_as_template(true)
        .tooltip("Pitwall")
        .on_tray_icon_event(|tray, event| {
            // The positioner needs every tray event to know where the icon is.
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_popover(tray.app_handle());
            }
        });

    #[cfg(not(target_os = "macos"))]
    let builder = builder
        .menu(&menu::build(app, None)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| menu::clicked(app, event.id().as_ref()));

    builder.build(app)?;

    // The count and the icon now, not at the first change.
    if let Some(state) = app.try_state::<AppState>() {
        refresh(app, &state.core.snapshot());
    }
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

    place_popover(app, &popover);

    note_front_app(app);
    let _ = windows::show(&popover);
    let _ = popover.set_focus();

    if let Some(state) = app.try_state::<AppState>() {
        state.core.popover_opened();
    }
}

/// Under the icon in the menu bar; before the icon was ever clicked, the top-right corner.
#[cfg(target_os = "macos")]
fn place_popover<R: Runtime>(_app: &AppHandle<R>, popover: &WebviewWindow<R>) {
    if popover.move_window(Position::TrayBottomCenter).is_err() {
        let _ = popover.move_window(Position::TopRight);
    }
}

/// By the icon, inside the work area: above it on a taskbar at the bottom (the usual place),
/// below it on one at the top. Without the icon's place, the bottom-right corner.
#[cfg(not(target_os = "macos"))]
fn place_popover<R: Runtime>(app: &AppHandle<R>, popover: &WebviewWindow<R>) {
    if !place_by_icon(app, popover) {
        let _ = popover.move_window(Position::BottomRight);
    }
}

#[cfg(not(target_os = "macos"))]
fn place_by_icon<R: Runtime>(app: &AppHandle<R>, popover: &WebviewWindow<R>) -> bool {
    let Some(icon) = app.tray_by_id(TRAY_ID).and_then(|tray| tray.rect().ok().flatten()) else {
        return false;
    };
    let (Ok(scale), Ok(size)) = (popover.scale_factor(), popover.outer_size()) else {
        return false;
    };
    let (at, extent) = (icon.position.to_physical::<f64>(scale), icon.size.to_physical::<f64>(scale));
    let Some(monitor) = app.monitor_from_point(at.x, at.y).ok().flatten() else {
        return false;
    };

    // Physical pixels throughout, as Windows reports them.
    let area = monitor.work_area();
    let (left, top) = (f64::from(area.position.x), f64::from(area.position.y));
    let (right, bottom) = (left + f64::from(area.size.width), top + f64::from(area.size.height));
    let (width, height) = (f64::from(size.width), f64::from(size.height));
    let middle = f64::from(monitor.position().y) + f64::from(monitor.size().height) / 2.0;

    let x = at.x + extent.width / 2.0 - width / 2.0;
    let y = if at.y > middle { at.y - height } else { at.y + extent.height };
    let x = x.min(right - width).max(left);
    let y = y.min(bottom - height).max(top);

    popover.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32)).is_ok()
}

/// The popover sizes itself to its content after it is placed. Above a taskbar it has to grow
/// upwards, so its edge stays by the icon.
#[cfg(windows)]
pub fn keep_popover_by_icon<R: Runtime>(app: &AppHandle<R>) {
    if let Some(popover) = app.get_webview_window(POPOVER) {
        if popover.is_visible().unwrap_or(false) {
            let _ = place_by_icon(app, &popover);
        }
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

/// The icon in the bar now (waiting, crashed, dark bar), so a refresh that changes none of them
/// leaves it alone.
static ICON: Mutex<Option<(bool, bool, bool)>> = Mutex::new(None);

/// Count next to the icon and the coloured dot. Waiting projects win over the running count;
/// a crash shows a red dot until the popover is opened. Windows shows no text beside a tray icon,
/// so there the tooltip carries the counts.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, snapshot: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let count = if snapshot.waiting > 0 { snapshot.waiting } else { snapshot.running };
    let _ = tray.set_title(if count > 0 { Some(count.to_string()) } else { None::<String> });

    let icon = (snapshot.waiting > 0, snapshot.crash_unseen, bar_is_dark(app));
    let changed = ICON.lock().map_or(true, |mut shown| shown.replace(icon) != Some(icon));
    if changed {
        set_icon(&tray, icon);
    }

    let tooltip = t!("core.tray.tooltip", running = snapshot.running, waiting = snapshot.waiting);
    let _ = tray.set_tooltip(Some(tooltip));

    #[cfg(not(target_os = "macos"))]
    menu::sync(app, &tray, snapshot);
}

fn set_icon<R: Runtime>(tray: &TrayIcon<R>, (waiting, crashed, dark): (bool, bool, bool)) {
    let variant: Option<&[u8]> = match (waiting, crashed, dark) {
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
            if let Ok(image) = plain_icon(dark) {
                let _ = tray.set_icon(Some(image));
                let _ = tray.set_icon_as_template(true);
            }
        }
    }
}

/// The plain glyph. macOS tints the template itself; elsewhere a dark bar gets it in white.
fn plain_icon(dark: bool) -> tauri::Result<Image<'static>> {
    let image = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    if cfg!(target_os = "macos") || !dark {
        return Ok(image);
    }

    let mut rgba = image.rgba().to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[..3].fill(255);
    }
    Ok(Image::new_owned(rgba, image.width(), image.height()))
}

/// Whether the menu bar is dark, for the coloured icons: it follows the system's appearance.
#[cfg(target_os = "macos")]
fn bar_is_dark<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(POPOVER).and_then(|w| w.theme().ok()).is_none_or(|theme| theme == tauri::Theme::Dark)
}

/// Whether the taskbar is dark. It has a mode of its own (Settings › Personalisation › Colours,
/// "Windows mode"), apart from the apps'.
#[cfg(windows)]
fn bar_is_dark<R: Runtime>(_app: &AppHandle<R>) -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .map_or(true, |light| light == 0)
}

/// Whether the panel is dark. GNOME's top bar is dark in either mode; elsewhere the panel follows
/// the desktop's theme, as Pitwall's windows do.
#[cfg(target_os = "linux")]
fn bar_is_dark<R: Runtime>(app: &AppHandle<R>) -> bool {
    let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| desktop.split(':').any(|name| matches!(name, "GNOME" | "Unity" | "ubuntu")));
    gnome || app.webview_windows().values().next().and_then(|w| w.theme().ok()).is_none_or(|theme| theme == tauri::Theme::Dark)
}

/// The tray's menu on Windows and Linux: Pitwall's window, the quick switcher, the projects that
/// wait on you and those that run (a few of each; a click opens one in its editor, as a
/// notification does), and Quit. Rebuilt only when what it lists changes.
#[cfg(not(target_os = "macos"))]
mod menu {
    use std::sync::Mutex;

    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIcon;
    use tauri::{AppHandle, Manager, Runtime};

    use crate::core::Snapshot;
    use crate::i18n::t;
    use crate::AppState;

    const OPEN: &str = "pitwall:open";
    const SWITCHER: &str = "pitwall:switcher";
    const QUIT: &str = "pitwall:quit";
    /// Followed by the project's path.
    const PROJECT: &str = "project:";
    /// Projects listed under each heading.
    const MAX_PROJECTS: usize = 5;

    /// The headings and projects the menu shows now, to tell when it needs building again.
    static SHOWN: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

    /// (id, label) for each line between the fixed ones; a heading has no id.
    fn lines(snapshot: &Snapshot) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        let waiting: Vec<_> = snapshot.projects.iter().filter(|p| p.claude.is_some()).take(MAX_PROJECTS).collect();
        let running: Vec<_> = snapshot.projects.iter().filter(|p| p.claude.is_none() && p.status == "running").take(MAX_PROJECTS).collect();

        for (heading, projects) in [(t!("window.filter.waiting"), waiting), (t!("window.filter.running"), running)] {
            if projects.is_empty() {
                continue;
            }
            lines.push((String::new(), heading));
            lines.extend(projects.into_iter().map(|p| (format!("{PROJECT}{}", p.path), p.name.clone())));
        }
        lines
    }

    pub fn build<R: Runtime>(app: &AppHandle<R>, snapshot: Option<&Snapshot>) -> tauri::Result<Menu<R>> {
        let menu = Menu::new(app)?;
        menu.append(&MenuItem::with_id(app, OPEN, t!("switcher.openPitwall"), true, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, SWITCHER, t!("switcher.placeholder"), true, None::<&str>)?)?;

        let lines = snapshot.map(lines).unwrap_or_default();
        if !lines.is_empty() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        for (id, label) in lines {
            // Headings are greyed out; their projects follow.
            let enabled = !id.is_empty();
            let id = if enabled { id } else { format!("heading:{label}") };
            menu.append(&MenuItem::with_id(app, id, label, enabled, None::<&str>)?)?;
        }

        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::with_id(app, QUIT, t!("common.quit"), true, None::<&str>)?)?;
        Ok(menu)
    }

    /// Builds the menu again when its projects (or the language) changed.
    pub fn sync<R: Runtime>(app: &AppHandle<R>, tray: &TrayIcon<R>, snapshot: &Snapshot) {
        let mut now = lines(snapshot);
        // The fixed lines change with the language only.
        now.push((String::new(), t!("switcher.openPitwall")));

        // Not held while building: menus are made on the main thread, which may be waiting here.
        {
            let mut shown = SHOWN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if shown.as_ref() == Some(&now) {
                return;
            }
            *shown = Some(now);
        }

        let set = build(app, Some(snapshot)).and_then(|menu| tray.set_menu(Some(menu)));
        if set.is_err() {
            *SHOWN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }
    }

    pub fn clicked<R: Runtime>(app: &AppHandle<R>, id: &str) {
        match id {
            OPEN => super::show_main(app),
            SWITCHER => super::toggle_switcher(app),
            QUIT => app.exit(0),
            _ => {
                if let (Some(path), Some(state)) = (id.strip_prefix(PROJECT), app.try_state::<AppState>()) {
                    state.core.open_editor(path);
                }
            }
        }
    }
}
