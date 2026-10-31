use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    image::Image,
    Emitter,
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, LogicalPosition, Manager, Monitor, PhysicalPosition, Runtime, WebviewWindow,
};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::claude::SessionPhase;
use crate::core::Snapshot;
use crate::fuel::BarFuel;
use crate::i18n::t;
use crate::{windows, AppState, MAIN, POPOVER, SWITCHER};

const TRAY_ID: &str = "pitwall";

/// Whether Pitwall is the active app. The popover and the switcher (non-activating panels) leave
/// it as it was: active only while its window is the one you are in.
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

/// Closes the switcher on purpose (its shortcut again, Esc, a pick). It never took the front from
/// the app you are in (`make_panel`), so the keyboard is back there as soon as it goes.
pub fn dismiss_switcher<R: Runtime>(app: &AppHandle<R>) {
    if let Some(switcher) = app.get_webview_window(SWITCHER) {
        if switcher.is_visible().unwrap_or(false) {
            let _ = windows::hide(&switcher);
        }
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
        hide_popover(app);
        return;
    }

    let just_hidden = LAST_HIDE.lock().ok().and_then(|at| *at).is_some_and(|at| at.elapsed() < Duration::from_millis(250));

    if just_hidden {
        return;
    }

    place_popover(app, &popover);

    // It comes clear and fades in: AppKit would bring it up at once (`make_panel`).
    #[cfg(target_os = "macos")]
    fade(&popover, 0.0, Duration::ZERO);
    // Not `set_focus`: on macOS it brings Pitwall to the front, and Pitwall's window with it.
    let _ = windows::show(&popover);
    focus_alone(&popover);
    #[cfg(target_os = "macos")]
    fade(&popover, 1.0, FADE);

    if let Some(state) = app.try_state::<AppState>() {
        state.core.popover_opened();
    }
}

/// Under the icon in the menu bar; without an item to ask, the top-right corner.
#[cfg(target_os = "macos")]
fn place_popover<R: Runtime>(app: &AppHandle<R>, popover: &WebviewWindow<R>) {
    if !place_under_icon(app, popover) {
        let _ = popover.move_window(Position::TopRight);
    }
}

/// Centred under the menu bar item, inside the item's screen. In AppKit's points throughout,
/// never in pixels: the item's pixels are its own screen's, and a position in pixels is read by
/// the scale of the screen the popover was last on. Between a Retina screen and a plain one the
/// two differ, and the popover came up on another screen, wherever the halved or doubled numbers
/// fell.
#[cfg(target_os = "macos")]
fn place_under_icon<R: Runtime>(app: &AppHandle<R>, popover: &WebviewWindow<R>) -> bool {
    let (Some(tray), Ok(ns_window)) = (app.tray_by_id(TRAY_ID), popover.ns_window()) else {
        return false;
    };
    let ns_window = ns_window as usize;

    let placed = tray.with_inner_tray_icon(move |inner| {
        // The item's window is on the screen whose menu bar was clicked.
        let item = inner.ns_status_item()?.button(objc2::MainThreadMarker::new()?)?.window()?;
        // That screen without its menu bar and Dock.
        let (icon, area) = (item.frame(), item.screen()?.visibleFrame());
        // SAFETY: as in `focus_alone`: this window's NSWindow, on the main thread.
        let popover = unsafe { &*(ns_window as *const objc2_app_kit::NSWindow) };
        let width = popover.frame().size.width;

        let left = (icon.origin.x + (icon.size.width - width) / 2.0).min(area.origin.x + area.size.width - width).max(area.origin.x);
        let top = icon.origin.y.min(area.origin.y + area.size.height);
        popover.setFrameTopLeftPoint(objc2_foundation::NSPoint::new(left.round(), top));
        Some(())
    });
    placed.ok().flatten().is_some()
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

/// A click anywhere outside Pitwall closes the popover, as it does a menu. Losing the keyboard
/// already hides it (`Focused(false)`), but a click on the menu bar, on another app's icon there
/// or on the Dock takes the keyboard from no one. Only that a button went down is used; clicks in
/// Pitwall's own windows don't come this way. Once, at start: the monitor stays for good.
#[cfg(target_os = "macos")]
pub fn hide_popover_on_outside_click<R: Runtime>(app: &AppHandle<R>) {
    use objc2_app_kit::{NSEvent, NSEventMask};

    let app = app.clone();
    let hide = block2::RcBlock::new(move |_: std::ptr::NonNull<NSEvent>| hide_popover(&app));
    let buttons = NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let _ = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(buttons, &hide);
}

/// The quick switcher: a centred palette for jumping to a project from the keyboard.
pub fn toggle_switcher<R: Runtime>(app: &AppHandle<R>) {
    // The shortcut toggles: pressed again, it closes and hands focus back.
    if app.get_webview_window(SWITCHER).is_some_and(|switcher| switcher.is_visible().unwrap_or(false)) {
        dismiss_switcher(app);
        return;
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

/// Gives the popover or the switcher the keyboard. They are non-activating panels (`make_panel`),
/// so Pitwall is not brought to the front and none of its other windows come along.
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

/// How long the popover takes to fade in: as long as the switcher's panel does (`.panel.shown`).
#[cfg(target_os = "macos")]
const FADE: Duration = Duration::from_millis(160);

/// Takes a window to `alpha` over `time`, easing out as the switcher's panel does, or at once for
/// no time.
#[cfg(target_os = "macos")]
fn fade<R: Runtime>(window: &WebviewWindow<R>, alpha: f64, time: Duration) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;

    let _ = window.run_on_main_thread(move || {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;

        // SAFETY: as in `focus_alone`: this window's NSWindow, on the main thread.
        let ns_window = unsafe { &*(ns_window as *const objc2_app_kit::NSWindow) };
        if time.is_zero() {
            ns_window.setAlphaValue(alpha);
            return;
        }
        // SAFETY: AppKit's and Core Animation's own classes, each message with the types it
        // declares.
        unsafe {
            let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
            let context: *mut AnyObject = msg_send![class!(NSAnimationContext), currentContext];
            let _: () = msg_send![context, setDuration: time.as_secs_f64()];
            let ease: *mut AnyObject = msg_send![class!(CAMediaTimingFunction), functionWithName: &*NSString::from_str("easeOut")];
            let _: () = msg_send![context, setTimingFunction: ease];
            let animator: *mut AnyObject = msg_send![ns_window, animator];
            let _: () = msg_send![animator, setAlphaValue: alpha];
            let _: () = msg_send![class!(NSAnimationContext), endGrouping];
        }
    });
}

/// Turns the popover or the switcher into a non-activating panel, as a menu bar extra's and
/// Spotlight's are: it takes the keyboard while the app you are in stays in front. Activating
/// Pitwall for it raised Pitwall's main window too, and now and then that window took the keyboard:
/// the search went away, and the popover, never having had the keyboard, stayed up when you
/// clicked elsewhere. Once, when the window is made.
///
/// The window's class becomes a subclass of NSPanel. NSPanel adds no fields to NSWindow, and the
/// one field of tao's window class, `focusable`, is declared again in the same place, so tao's
/// window object stays valid as it is.
///
/// The mini tower (`tower.rs`) is such a panel too, of a class that never takes the keyboard: a
/// click in it leaves the keyboard, and the front, with the app you are in.
#[cfg(target_os = "macos")]
pub fn make_panel<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let popover = window.label() == POPOVER;
    let quiet = window.label() == crate::tower::LABEL;

    let _ = window.run_on_main_thread(move || {
        use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObjectProtocol, Sel};
        use objc2::{msg_send, sel, ClassType};
        use objc2_app_kit::{NSPanel, NSWindow, NSWindowAnimationBehavior, NSWindowStyleMask};

        extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool {
            Bool::YES
        }
        extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
            Bool::NO
        }

        let name = if quiet { c"PitwallQuietPanel" } else { c"PitwallPanel" };
        let keyed = if quiet { no as extern "C-unwind" fn(_, _) -> _ } else { yes as extern "C-unwind" fn(_, _) -> _ };
        let class = AnyClass::get(name).or_else(|| {
            let mut builder = ClassBuilder::new(name, NSPanel::class())?;
            builder.add_ivar::<Bool>(c"focusable");
            // SAFETY: both methods match their selectors' signatures (no arguments, BOOL).
            unsafe {
                builder.add_method(sel!(canBecomeKeyWindow), keyed);
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
        // AppKit passes that flag on to the window server only when a window is made. Set later,
        // the panel takes the keyboard without activating Pitwall, but a click in it still
        // activates. This tells the window server; it is private, hence the check.
        if ns_window.respondsToSelector(sel!(_setPreventsActivation:)) {
            let _: () = unsafe { msg_send![ns_window, _setPreventsActivation: true] };
        }
        // Hiding is the blur handler's job, not AppKit's.
        ns_window.setHidesOnDeactivate(false);
        let _: () = unsafe { msg_send![ns_window, setBecomesKeyOnlyIfNeeded: false] };
        // AppKit animates a window by its class, and a panel pops in with a bounce. As a utility
        // window the popover comes at once and fades out; `toggle_popover` fades it in.
        if popover {
            ns_window.setAnimationBehavior(NSWindowAnimationBehavior::UtilityWindow);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn focus_alone<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.set_focus();
}

/// Set while a panel's window is being made (`without_activating`).
#[cfg(target_os = "macos")]
static STAY_BEHIND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Makes a panel's window (`make`) without Pitwall coming to the front for it. wry activates the
/// app for every webview it makes; for the switcher, made on first use over the app you are in,
/// that brought Pitwall forward, and its window with it once the switcher went.
#[cfg(target_os = "macos")]
pub fn without_activating<T>(make: impl FnOnce() -> T) -> T {
    use std::sync::atomic::Ordering;

    STAY_BEHIND.store(true, Ordering::SeqCst);
    let made = make();
    STAY_BEHIND.store(false, Ordering::SeqCst);
    made
}

/// Lets `without_activating` hold Pitwall back: AppKit's `activate` (and the older
/// `activateIgnoringOtherApps:`, which wry calls before macOS 14) get a wrapper that does nothing
/// while a panel is being made and is AppKit's own method otherwise. Once, at start.
#[cfg(target_os = "macos")]
pub fn guard_activation() {
    use std::mem::transmute;
    use std::sync::atomic::Ordering;
    use std::sync::OnceLock;

    use objc2::runtime::{AnyObject, Bool, Imp, Sel};
    use objc2::{sel, ClassType};

    type Activate = unsafe extern "C-unwind" fn(&AnyObject, Sel);
    type ActivateIgnoring = unsafe extern "C-unwind" fn(&AnyObject, Sel, Bool);

    /// AppKit's own methods, to pass the call on to.
    static ACTIVATE: OnceLock<Activate> = OnceLock::new();
    static ACTIVATE_IGNORING: OnceLock<ActivateIgnoring> = OnceLock::new();

    unsafe extern "C-unwind" fn activate(this: &AnyObject, cmd: Sel) {
        if let (false, Some(appkit)) = (STAY_BEHIND.load(Ordering::SeqCst), ACTIVATE.get()) {
            unsafe { appkit(this, cmd) };
        }
    }
    unsafe extern "C-unwind" fn activate_ignoring(this: &AnyObject, cmd: Sel, others: Bool) {
        if let (false, Some(appkit)) = (STAY_BEHIND.load(Ordering::SeqCst), ACTIVATE_IGNORING.get()) {
            unsafe { appkit(this, cmd, others) };
        }
    }

    let class = objc2_app_kit::NSApplication::class();
    // SAFETY: each wrapper has the signature of the method it stands in for (`activate`: no
    // arguments; `activateIgnoringOtherApps:`: a BOOL; neither returns anything), and AppKit's
    // method is kept before the wrapper goes in.
    unsafe {
        if let Some(method) = class.instance_method(sel!(activate)) {
            let _ = ACTIVATE.set(transmute::<Imp, Activate>(method.implementation()));
            method.set_implementation(transmute::<Activate, Imp>(activate));
        }
        if let Some(method) = class.instance_method(sel!(activateIgnoringOtherApps:)) {
            let _ = ACTIVATE_IGNORING.set(transmute::<Imp, ActivateIgnoring>(method.implementation()));
            method.set_implementation(transmute::<ActivateIgnoring, Imp>(activate_ignoring));
        }
    }
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

/// The text beside the icon, each part there only while Settings shows it and there is something
/// to count: the running dev servers, the Claude sessions at work, and Claude's fuel with its
/// share as written (`74%`).
#[derive(Clone, PartialEq, Eq)]
struct Title {
    servers: Option<String>,
    claudes: Option<String>,
    fuel: Option<(BarFuel, String)>,
}

/// The one in the bar now, so a refresh that changes none of it leaves it alone.
static TITLE: Mutex<Option<Title>> = Mutex::new(None);

/// The counts next to the icon and its coloured dot: the running servers in green, the Claude
/// sessions at work in orange. An orange dot says Claude waits on you; a crash shows a red one
/// until the popover is opened. With Claude's fuel shown, the session's share left follows the
/// counts. Windows shows no text beside a tray icon, so there the tooltip carries all of it.
///
/// On the main thread, and never waited for. The tray's handle taken there may be its last one
/// when the app quits, and AppKit's status item goes with it, which macOS allows on the main
/// thread alone (anywhere else it stops the app). And whoever asks goes on at once: the state
/// it sends next isn't held back while the main thread is busy (a menu is open) and then sent
/// after a newer one.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, snapshot: &Snapshot) {
    let (handle, snapshot) = (app.clone(), snapshot.clone());
    let _ = app.run_on_main_thread(move || draw(&handle, &snapshot));
}

/// The latest `Snapshot::seq` drawn: refreshes asked for on two threads can reach the main thread
/// in either order, and one taken before the state in the bar is not drawn over it.
static DRAWN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn draw<R: Runtime>(app: &AppHandle<R>, snapshot: &Snapshot) {
    if DRAWN.fetch_max(snapshot.seq, std::sync::atomic::Ordering::SeqCst) > snapshot.seq {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let settings = &snapshot.settings;
    let working = snapshot.projects.iter().flat_map(|project| &project.claude_sessions).filter(|session| matches!(session.phase, SessionPhase::Working)).count();
    let count = |shown: bool, count: usize| (shown && count > 0).then(|| count.to_string());
    let fuel = app.try_state::<AppState>().and_then(|state| state.fuel.bar(settings));
    let fuel = fuel.map(|fuel| (fuel, t!("core.tray.percent", value = fuel.left)));

    let title = Title { servers: count(settings.servers_in_bar, snapshot.running), claudes: count(settings.claudes_in_bar, working), fuel };
    if TITLE.lock().map_or(true, |mut shown| shown.replace(title.clone()).as_ref() != Some(&title)) {
        set_title(&tray, title.clone());
    }

    let icon = (snapshot.waiting > 0, snapshot.crash_unseen, bar_is_dark(app));
    let changed = ICON.lock().map_or(true, |mut shown| shown.replace(icon) != Some(icon));
    if changed {
        set_icon(&tray, icon);
    }

    let mut tooltip = match &title.fuel {
        Some((fuel, left)) if fuel.week => t!("core.tray.tooltipFuelWeek", running = snapshot.running, waiting = snapshot.waiting, left = left),
        Some((_, left)) => t!("core.tray.tooltipFuel", running = snapshot.running, waiting = snapshot.waiting, left = left),
        None => t!("core.tray.tooltip", running = snapshot.running, waiting = snapshot.waiting),
    };
    if working > 0 {
        tooltip = format!("{tooltip} · {}", t!("core.tray.working", count = working));
    }
    // After the title: setting the tooltip also fits the icon's click area to the item's new width.
    let _ = tray.set_tooltip(Some(tooltip));

    #[cfg(not(target_os = "macos"))]
    menu::sync(app, &tray, snapshot);
}

/// Puts the text beside the icon. macOS colours the counts and draws the fuel's dial into the
/// title; elsewhere the parts follow one another as plain text.
fn set_title<R: Runtime>(tray: &TrayIcon<R>, title: Title) {
    if title.servers.is_none() && title.claudes.is_none() && title.fuel.is_none() {
        let _ = tray.set_title(None::<String>);
        return;
    }

    #[cfg(target_os = "macos")]
    let _ = tray.with_inner_tray_icon(move |inner| {
        let (Some(item), Some(mtm)) = (inner.ns_status_item(), objc2::MainThreadMarker::new()) else {
            return;
        };
        if let Some(button) = item.button(mtm) {
            button.setAttributedTitle(&bar::title(&title));
        }
    });

    #[cfg(not(target_os = "macos"))]
    {
        let parts: Vec<String> = [title.servers, title.claudes, title.fuel.map(|(_, percent)| percent)].into_iter().flatten().collect();
        let _ = tray.set_title(Some(parts.join(" · ")));
    }
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

/// Whether the menu bar is dark, for the coloured icons: as the menu bar item itself is drawn
/// there. Never as Pitwall's windows are: Settings can give those a theme of their own, which
/// the bar doesn't take. Dark while there is no item to ask.
#[cfg(target_os = "macos")]
fn bar_is_dark<R: Runtime>(app: &AppHandle<R>) -> bool {
    use objc2_app_kit::NSAppearanceCustomization as _;

    let item = app.tray_by_id(TRAY_ID).and_then(|tray| {
        let dark = tray.with_inner_tray_icon(|inner| {
            let button = inner.ns_status_item()?.button(objc2::MainThreadMarker::new()?)?;
            Some(bar::is_dark(&button.effectiveAppearance()))
        });
        dark.ok().flatten()
    });
    item.unwrap_or(true)
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

/// The menu bar item's text as one attributed title (a plain title holds neither colour nor
/// picture): the running servers' count in green, the working Claude sessions' in orange, then a
/// small dial and the session's share left. The dial is the Fuel page's, at the bar's size: a 240°
/// arc from empty at the lower left to full at the lower right, lit as far as the share left, and
/// a needle.
#[cfg(target_os = "macos")]
mod bar {
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage};
    use objc2_foundation::{NSArray, NSAttributedString, NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString};

    use super::Title;
    use crate::fuel::BarFuel;

    /// The dial's box in points, and how far it sits under the text's baseline to stand level
    /// with the digits.
    const SIDE: f64 = 16.0;
    const DROP: f64 = -3.5;
    /// The arc: its centre in the box (y up), radius and stroke; empty at 210°, full 240° on.
    const CENTRE: (f64, f64) = (8.0, 6.9);
    const RADIUS: f64 = 5.6;
    const STROKE: f64 = 1.7;
    const EMPTY: f64 = 210.0;
    const SWEEP: f64 = 240.0;
    /// The needle's length and its hub's radius.
    const NEEDLE: f64 = 3.6;
    const HUB: f64 = 1.0;
    /// `NSLineCapStyleRound`.
    const ROUND: usize = 1;
    /// An older reading: the dial and its share at this strength.
    const DIMMED: f64 = 0.45;
    /// Pitwall's state colours (tokens.css: --run, --claude, --crash) as a dark bar takes them,
    /// and each a step darker for a light one.
    const RUN: [[f64; 3]; 2] = [[115.0, 201.0, 145.0], [31.0, 138.0, 76.0]];
    const CLAUDE: [[f64; 3]; 2] = [[240.0, 136.0, 62.0], [199.0, 90.0, 18.0]];
    const LOW: [[f64; 3]; 2] = [[241.0, 76.0, 76.0], [198.0, 47.0, 47.0]];

    pub fn title(title: &Title) -> Retained<NSAttributedString> {
        let text = NSMutableAttributedString::new();

        if let Some(servers) = &title.servers {
            append(&text, servers, Some(&tint(RUN)));
        }
        if let Some(claudes) = &title.claudes {
            // A space and a hair space: about as near the servers' count as that is to the icon.
            if text.length() > 0 {
                append(&text, " \u{200A}", None);
            }
            append(&text, claudes, Some(&tint(CLAUDE)));
        }
        if let Some((fuel, percent)) = &title.fuel {
            if text.length() > 0 {
                append(&text, "  ", None);
            }
            text.appendAttributedString(&dial(*fuel));
            // An older reading's share in the system's own dimmer ink, which follows the bar.
            let dimmed = fuel.stale.then(NSColor::secondaryLabelColor);
            append(&text, &format!(" {percent}"), dimmed.as_deref());
        }

        let font = NSFont::menuBarFontOfSize(0.0);
        // SAFETY: a font for the font attribute, over the whole string.
        unsafe { text.addAttribute_value_range(NSFontAttributeName, &font, NSRange::new(0, text.length())) };
        Retained::into_super(text)
    }

    /// Whether `appearance` is a dark one: the nearer of AppKit's two.
    pub fn is_dark(appearance: &NSAppearance) -> bool {
        // SAFETY: the names are AppKit's constants.
        unsafe {
            let names = NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]);
            appearance.bestMatchFromAppearancesWithNames(&names).is_some_and(|name| *name == *NSAppearanceNameDarkAqua)
        }
    }

    /// One of Pitwall's colours for the bar: the first of the pair on a dark bar, the second on a
    /// light one. Which it is, is worked out each time it is drawn, so it follows the bar.
    fn tint([dark, light]: [[f64; 3]; 2]) -> Retained<NSColor> {
        let colour = |[red, green, blue]: [f64; 3]| NSColor::colorWithSRGBRed_green_blue_alpha(red / 255.0, green / 255.0, blue / 255.0, 1.0);
        let (dark, light) = (colour(dark), colour(light));
        let pick = block2::RcBlock::new(move |appearance: NonNull<NSAppearance>| -> NonNull<NSColor> {
            // SAFETY: AppKit hands the block a live appearance.
            let is_dark = is_dark(unsafe { appearance.as_ref() });
            NonNull::from(if is_dark { &*dark } else { &*light })
        });
        // SAFETY: `colorWithName:dynamicProvider:` with no name and a block of the signature it
        // declares; the block answers with one of the two colours it keeps, and AppKit keeps the
        // block for as long as the colour it makes lives.
        unsafe { msg_send![class!(NSColor), colorWithName: std::ptr::null::<NSString>(), dynamicProvider: &*pick] }
    }

    /// Adds `piece` to the title, in `tint` or (none) in the bar's own ink.
    fn append(title: &NSMutableAttributedString, piece: &str, tint: Option<&NSColor>) {
        let start = title.length();
        title.appendAttributedString(&NSAttributedString::from_nsstring(&NSString::from_str(piece)));
        if let Some(tint) = tint {
            // SAFETY: a colour for the colour attribute, over the piece just added.
            unsafe { title.addAttribute_value_range(NSForegroundColorAttributeName, tint, NSRange::new(start, title.length() - start)) };
        }
    }

    /// The dial as a piece of text: an attachment whose picture is drawn each time it shows, so
    /// it takes the bar's ink, light or dark.
    fn dial(fuel: BarFuel) -> Retained<NSAttributedString> {
        let draw = block2::RcBlock::new(move |_: NSRect| {
            draw(fuel);
            Bool::YES
        });
        let image = NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(SIDE, SIDE), false, &draw);

        // SAFETY: AppKit's own classes, each message with the types it declares.
        unsafe {
            let attachment: Retained<AnyObject> = msg_send![class!(NSTextAttachment), new];
            let _: () = msg_send![&*attachment, setImage: &*image];
            let _: () = msg_send![&*attachment, setBounds: NSRect::new(NSPoint::new(0.0, DROP), NSSize::new(SIDE, SIDE))];
            msg_send![class!(NSAttributedString), attributedStringWithAttachment: &*attachment]
        }
    }

    fn draw(fuel: BarFuel) {
        // The bar's ink, dimmed for an older reading; the arc in red instead while little is left.
        let strength = if fuel.stale { DIMMED } else { 1.0 };
        let ink = NSColor::labelColor().colorWithAlphaComponent(strength);
        let arc_colour = if fuel.low && !fuel.stale { tint(LOW) } else { NSColor::labelColor() };
        let lit = arc_colour.colorWithAlphaComponent(strength);
        let centre = NSPoint::new(CENTRE.0, CENTRE.1);
        let at = EMPTY - f64::from(fuel.left) / 100.0 * SWEEP;

        // SAFETY: NSBezierPath's own messages, each with the types it declares; drawn inside the
        // picture's drawing handler, where AppKit has a context set.
        unsafe {
            let arc = |to: f64| -> Retained<AnyObject> {
                let path: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPath];
                let _: () = msg_send![&*path, appendBezierPathWithArcWithCenter: centre, radius: RADIUS, startAngle: EMPTY, endAngle: to, clockwise: true];
                let _: () = msg_send![&*path, setLineWidth: STROKE];
                let _: () = msg_send![&*path, setLineCapStyle: ROUND];
                path
            };

            // The whole arc faint, then the share left over it.
            arc_colour.colorWithAlphaComponent(strength * 0.3).setStroke();
            let _: () = msg_send![&*arc(EMPTY - SWEEP), stroke];
            if fuel.left > 0 {
                lit.setStroke();
                let _: () = msg_send![&*arc(at), stroke];
            }

            // The needle, from the centre to where the share left ends, and its hub.
            let (sin, cos) = at.to_radians().sin_cos();
            let needle: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPath];
            let _: () = msg_send![&*needle, moveToPoint: centre];
            let _: () = msg_send![&*needle, lineToPoint: NSPoint::new(centre.x + NEEDLE * cos, centre.y + NEEDLE * sin)];
            let _: () = msg_send![&*needle, setLineWidth: 1.15f64];
            let _: () = msg_send![&*needle, setLineCapStyle: ROUND];
            ink.setStroke();
            let _: () = msg_send![&*needle, stroke];

            let hub: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: NSRect::new(NSPoint::new(centre.x - HUB, centre.y - HUB), NSSize::new(HUB * 2.0, HUB * 2.0))];
            ink.setFill();
            let _: () = msg_send![&*hub, fill];
        }
    }
}

/// The tray's menu on Windows and Linux: Pitwall's window, the quick switcher, the projects that
/// wait on you and those that run (a few of each; a click opens one as Settings › Projects says
/// for the menu bar), and Quit. Rebuilt only when what it lists changes.
#[cfg(not(target_os = "macos"))]
mod menu {
    use std::sync::Mutex;

    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIcon;
    use tauri::{AppHandle, Emitter, Manager, Runtime};

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
            // With something still open, the chequered flag's dialog first (`flag.rs`).
            QUIT => crate::flag::request_quit(app),
            _ => {
                if let (Some(path), Some(state)) = (id.strip_prefix(PROJECT), app.try_state::<AppState>()) {
                    match state.core.settings().open_outside(path) {
                        "editor" => state.core.open_editor(path),
                        target => {
                            super::show_main(app);
                            let event = match target {
                                "files" => "reveal-files",
                                "terminal" => "focus-terminal",
                                _ => "reveal-project",
                            };
                            let _ = app.emit(event, serde_json::json!({ "path": path }));
                        }
                    }
                }
            }
        }
    }
}
