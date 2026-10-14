//! The mini tower: a small window over every other, for a glance at which projects run and where
//! Claude works or waits (`src/tower/Tower.vue`). It never takes the keyboard and never brings
//! Pitwall to the front: on macOS a panel as the popover's (`tray::make_panel`), one that can't
//! become the key window. It is as large as its page's content, stands where it was left, and is
//! shown or hidden as `Settings::tower` says (`sync`).
//!
//! Its place is worked out in one space on every system: left and top, the top counted
//! downwards. On macOS that is AppKit's points (never Tauri's pixels, which mix the scales of a
//! Retina screen and a plain one: see `tray::place_under_icon`), turned over at the main
//! screen's top; elsewhere physical pixels, as those systems lay their screens out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::{windows, AppState};

pub const LABEL: &str = "tower";

/// Kept between the tower and its screen's edges where it goes by itself: the first time, and
/// when the place it was left at is on no screen any more.
const MARGIN: f64 = 16.0;
/// How much of the tower must lie on a screen for it to be there: enough to take hold of.
const HOLD: f64 = 24.0;
/// The largest content a page may ask for, whatever it measured.
const MAX_SIDE: f64 = 4000.0;
/// A place is written down once the tower has stood still this long: a drag is one write.
const STILL: Duration = Duration::from_millis(400);

/// A rectangle on the desktop: left, top, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn of([x, y, w, h]: [i32; 4]) -> Self {
        Self { x: f64::from(x), y: f64::from(y), w: f64::from(w), h: f64::from(h) }
    }

    fn rounded(self) -> [i32; 4] {
        [self.x.round() as i32, self.y.round() as i32, self.w.round() as i32, self.h.round() as i32]
    }

    /// Moved as little as it takes to lie inside `area`; one larger than it starts at its top left.
    fn within(self, area: Rect) -> Self {
        let x = self.x.min(area.x + area.w - self.w).max(area.x);
        let y = self.y.min(area.y + area.h - self.h).max(area.y);
        Self { x, y, ..self }
    }

    /// How wide and how tall the part it shares with `other` is.
    fn shared(self, other: Rect) -> (f64, f64) {
        let wide = (self.x + self.w).min(other.x + other.w) - self.x.max(other.x);
        let tall = (self.y + self.h).min(other.y + other.h) - self.y.max(other.y);
        (wide.max(0.0), tall.max(0.0))
    }

    /// It and `other` cover some of the same ground (touching edges don't).
    fn overlaps(self, other: Rect) -> bool {
        let (wide, tall) = self.shared(other);
        wide > 0.0 && tall > 0.0
    }
}

/// A screen: all of it, what its menu bar, Dock or taskbar leave free, and the Dock itself when
/// it stands on this screen and its own rectangle is known. Then `area` has the Dock's whole
/// band back (the room beside the Dock is free) and only `dock` is kept clear; else `area` is
/// without the band, as the system leaves it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub frame: Rect,
    pub area: Rect,
    pub dock: Option<Rect>,
}

/// What `area` leaves free beside `dock`: its parts above, below, left and right of it, each as
/// wide (or as tall) as `area`. A rectangle inside `area` keeps off `dock` exactly when it lies
/// wholly in one of them: wherever it stands, it is either above, below, left or right of it.
fn rooms(area: Rect, dock: Rect) -> [Rect; 4] {
    let (left, top, right, bottom) = (area.x, area.y, area.x + area.w, area.y + area.h);
    let (dock_left, dock_top) = (dock.x.max(left), dock.y.max(top));
    let (dock_right, dock_bottom) = ((dock.x + dock.w).min(right), (dock.y + dock.h).min(bottom));
    [
        Rect { h: dock_top - top, ..area },
        Rect { y: dock_bottom, h: bottom - dock_bottom, ..area },
        Rect { w: dock_left - left, ..area },
        Rect { x: dock_right, w: right - dock_right, ..area },
    ]
}

/// `rect`, inside `area`, kept off `dock`: where it stands when it doesn't cover it; else moved
/// into the nearest of the rooms beside it (`rooms`) that it fits in, as little as it takes. One
/// that fits in none (a list as tall as the screen over a Dock at its bottom) goes into the room
/// that keeps most of it, cut to that room's size.
fn clear(rect: Rect, area: Rect, dock: Option<Rect>) -> Rect {
    let Some(dock) = dock.filter(|dock| rect.overlaps(*dock)) else {
        return rect;
    };
    let rooms = rooms(area, dock);
    let moved = |to: &Rect| (to.x - rect.x).hypot(to.y - rect.y);
    // Of two as near, or keeping as much, the first.
    let nearest = rooms.iter().filter(|room| room.w >= rect.w && room.h >= rect.h).map(|room| rect.within(*room)).min_by(|a, b| moved(a).total_cmp(&moved(b)));
    nearest.unwrap_or_else(|| {
        let kept = |room: &&Rect| rect.w.min(room.w) * rect.h.min(room.h);
        // A Dock that leaves no room at all is no Dock this screen has: it stays as it is.
        let room = rooms.iter().filter(|room| room.w > 0.0 && room.h > 0.0).min_by(|a, b| kept(b).total_cmp(&kept(a)));
        room.map_or(rect, |room| Rect { w: rect.w.min(room.w), h: rect.h.min(room.h), ..rect }.within(*room))
    })
}

/// Which of the tower's corners stays put while its size changes, and where that corner stands:
/// taken from where the user put it, and kept while the page makes the tower larger or smaller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    pub right: bool,
    pub bottom: bool,
    /// The held corner: its x (the right edge when `right`) and its y (the bottom when `bottom`).
    pub x: f64,
    pub y: f64,
}

impl Hold {
    /// The corner of `rect` nearest the corner of `area` it stands in.
    fn of(rect: Rect, area: Rect) -> Self {
        let right = rect.x + rect.w / 2.0 > area.x + area.w / 2.0;
        let bottom = rect.y + rect.h / 2.0 > area.y + area.h / 2.0;
        Self { right, bottom, x: if right { rect.x + rect.w } else { rect.x }, y: if bottom { rect.y + rect.h } else { rect.y } }
    }
}

/// Where a tower of `size` stands, given where it stood (`from`: where it is, or where it was
/// left before the last quit) and the screens there are now, the main one first. Always wholly
/// inside one screen's free area and off its Dock, beside which it may stand (`clear`):
///
/// - on the screen that holds most of `from`, by the corner it is held at (`held`: the one it
///   was last arranged by, while it hasn't been moved since; else the corner of `from` nearest
///   that screen's own corner), so a tower in a screen's bottom right grows up and to the left,
///   and one the screen's edge pulled in when it grew goes back to its corner when it shrinks;
/// - when it has just changed its form (`turned`: a list become a strip, or back), with its top
///   where the old form's top was (its head, where it was looked at), on the side it was held,
///   so a tall list in a screen's lower half doesn't turn into a strip at the screen's bottom;
/// - at the top right of the main screen when there is no `from`, or it lies on no screen any
///   more (a display taken away, the layout changed).
///
/// With the corner it is held at from now on. None without a screen.
pub fn settle(from: Option<Rect>, size: (f64, f64), screens: &[Screen], margin: f64, turned: bool, held: Option<Hold>) -> Option<(Rect, Hold)> {
    let main = screens.first()?;
    // Whole points (or pixels), and never larger than the screen it stands on.
    let sized = |area: Rect| (size.0.round().max(1.0).min(area.w), size.1.round().max(1.0).min(area.h));
    let whole = |rect: Rect| Rect { x: rect.x.round(), y: rect.y.round(), ..rect };

    let home = from.and_then(|from| {
        let held = |screen: &&Screen| {
            let (wide, tall) = from.shared(screen.frame);
            wide >= HOLD.min(from.w) && tall >= HOLD.min(from.h) && wide > 0.0 && tall > 0.0
        };
        let most = |screen: &&Screen| {
            let (wide, tall) = from.shared(screen.frame);
            (wide * tall) as i64
        };
        // Of two that hold as much, the first: `max_by_key` would take the last.
        screens.iter().filter(held).rev().max_by_key(most).map(|screen| (*screen, from))
    });

    Some(match home {
        Some((Screen { area, dock, .. }, from)) => {
            let (w, h) = sized(area);
            let hold = held.unwrap_or_else(|| Hold::of(from, area));
            // A new form takes the old one's head: its top, on the side it was held.
            let hold = if turned { Hold { bottom: false, y: from.y, ..hold } } else { hold };
            let x = if hold.right { hold.x - w } else { hold.x };
            let y = if hold.bottom { hold.y - h } else { hold.y };
            (clear(whole(Rect { x, y, w, h }).within(area), area, dock), hold)
        }
        None => {
            let (w, h) = sized(main.area);
            let at = whole(Rect { x: main.area.x + main.area.w - w - margin, y: main.area.y + margin, w, h }).within(main.area);
            // As far from the Dock as from the screen's edges.
            let dock = main.dock.map(|dock| Rect { x: dock.x - margin, y: dock.y - margin, w: dock.w + 2.0 * margin, h: dock.h + 2.0 * margin });
            let at = clear(at, main.area, dock);
            (at, Hold { right: true, bottom: false, x: at.x + at.w, y: at.y })
        }
    })
}

/// The tower has been put in its place once since the app started: from then on it is where it
/// stands that counts, not where it was left before the last quit.
static PLACED: AtomicBool = AtomicBool::new(false);

/// Shows or hides the tower as Settings says, on every desktop or on one: at start, and after
/// each change of its settings. Shown, the window is made if need be; it comes up once its page
/// has drawn and said how large it is (`fit`), so it never shows empty or at another size.
pub fn sync<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let tower = state.core.settings().tower;

    if !tower.on {
        if let Some(window) = app.get_webview_window(LABEL) {
            if window.is_visible().unwrap_or(false) {
                let _ = windows::hide(&window);
            }
        }
        return;
    }

    let Some(_window) = windows::get_or_create(app, LABEL) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        spaces(&_window, tower.all_spaces);
        pointer::follow(&_window);
    }
}

/// The tower's shortcut: shown becomes hidden, hidden shown, and stays so over a restart.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<AppState>() {
        state.core.toggle_tower();
        sync(app);
    }
}

/// The page has drawn: its content is `width` by `height`. The window takes that size where it
/// stands (`settle`) and, when it is to be shown and isn't, comes up: without the keyboard, and
/// without Pitwall coming to the front.
pub fn fit<R: Runtime>(window: &WebviewWindow<R>, width: f64, height: f64) {
    let Some(state) = window.app_handle().try_state::<AppState>() else {
        return;
    };
    let tower = state.core.settings().tower;
    let sized = |side: f64| side.is_finite() && (1.0..=MAX_SIDE).contains(&side);
    if !tower.on || !sized(width) || !sized(height) {
        return;
    }

    // The form it was last fitted in: a change of it keeps the head where it was.
    let turned = {
        let mut last = FORM.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let turned = last.as_deref().is_some_and(|form| form != tower.form);
        *last = Some(tower.form.clone());
        turned
    };
    arrange(window, (width.ceil(), height.ceil()), tower.place, turned);
}

/// The form (`list`, `strip`) the tower was last fitted in, since the app started.
static FORM: Mutex<Option<String>> = Mutex::new(None);
/// The corner the tower was last arranged by, and where that put it.
static HELD: Mutex<Option<(Hold, Rect)>> = Mutex::new(None);

/// That corner, while the tower still stands where it was put: one moved since (by hand) is held
/// anew, by where it is now.
fn kept(now: Rect) -> Option<Hold> {
    let near = |a: Rect, b: Rect| (a.x - b.x).abs() <= 1.0 && (a.y - b.y).abs() <= 1.0 && (a.w - b.w).abs() <= 1.0 && (a.h - b.h).abs() <= 1.0;
    let held = *HELD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    held.and_then(|(hold, at)| near(at, now).then_some(hold))
}

fn keep(hold: Hold, at: Rect) {
    *HELD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((hold, at));
}

#[cfg(target_os = "macos")]
fn arrange<R: Runtime>(window: &WebviewWindow<R>, size: (f64, f64), left_at: Option<[i32; 4]>, turned: bool) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let app = window.app_handle().clone();

    let _ = window.run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        // SAFETY: as in `tray::focus_alone`: this window's NSWindow, on the main thread.
        let ns_window = unsafe { &*(ns_window as *const objc2_app_kit::NSWindow) };
        let (screens, top) = mac::screens(mtm);
        let now = mac::frame(ns_window, top);
        let (from, held) = if PLACED.swap(true, Ordering::SeqCst) { (Some(now), kept(now)) } else { (left_at.map(Rect::of), None) };

        if let Some((to, hold)) = settle(from, size, &screens, MARGIN, turned, held) {
            keep(hold, to);
            if to != now {
                mac::set_frame(ns_window, to, top);
            }
        }
        if !ns_window.isVisible() {
            // Not `show`: that asks for the keyboard, which this window never takes. In front
            // of its level whichever app is active.
            ns_window.orderFrontRegardless();
            windows::tell(&app, LABEL, true);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn arrange<R: Runtime>(window: &WebviewWindow<R>, size: (f64, f64), left_at: Option<[i32; 4]>, turned: bool) {
    use tauri::{PhysicalPosition, PhysicalSize};

    let scale = window.scale_factor().unwrap_or(1.0);
    let now = frame(window);
    let (from, held) = if PLACED.swap(true, Ordering::SeqCst) { (now, now.and_then(kept)) } else { (left_at.map(Rect::of), None) };

    if let Some((to, hold)) = settle(from, (size.0 * scale, size.1 * scale), &screens(window.app_handle()), MARGIN * scale, turned, held) {
        keep(hold, to);
        if Some(to) != now {
            let _ = window.set_size(PhysicalSize::new(to.w.round() as u32, to.h.round() as u32));
            let _ = window.set_position(PhysicalPosition::new(to.x.round() as i32, to.y.round() as i32));
        }
    }
    if !window.is_visible().unwrap_or(false) {
        let _ = windows::show(window);
    }
}

/// Every screen in physical pixels, the main one first. The taskbar's whole band stays out of
/// the free area: its own rectangle isn't asked for.
#[cfg(not(target_os = "macos"))]
fn screens<R: Runtime>(app: &AppHandle<R>) -> Vec<Screen> {
    let rect = |x: i32, y: i32, w: u32, h: u32| Rect { x: f64::from(x), y: f64::from(y), w: f64::from(w), h: f64::from(h) };
    let screen = |monitor: &tauri::Monitor| {
        let (at, size, area) = (monitor.position(), monitor.size(), monitor.work_area());
        Screen { frame: rect(at.x, at.y, size.width, size.height), area: rect(area.position.x, area.position.y, area.size.width, area.size.height), dock: None }
    };

    let main = app.primary_monitor().ok().flatten().map(|monitor| screen(&monitor));
    let mut all: Vec<Screen> = app.available_monitors().unwrap_or_default().iter().map(screen).collect();
    if let Some(main) = main {
        all.retain(|screen| *screen != main);
        all.insert(0, main);
    }
    all
}

/// Where the tower stands now.
#[cfg(target_os = "macos")]
fn frame<R: Runtime>(window: &WebviewWindow<R>) -> Option<Rect> {
    let mtm = objc2::MainThreadMarker::new()?;
    let ns_window = window.ns_window().ok()? as *const objc2_app_kit::NSWindow;
    // SAFETY: as in `tray::focus_alone`: this window's NSWindow, on the main thread (the marker).
    let ns_window = unsafe { &*ns_window };
    Some(mac::frame(ns_window, mac::top(mtm)))
}

#[cfg(not(target_os = "macos"))]
fn frame<R: Runtime>(window: &WebviewWindow<R>) -> Option<Rect> {
    let (at, size) = (window.outer_position().ok()?, window.outer_size().ok()?);
    Some(Rect { x: f64::from(at.x), y: f64::from(at.y), w: f64::from(size.width), h: f64::from(size.height) })
}

/// Where the tower was last seen standing, and when: written down once it has stood still.
static LEFT: Mutex<Option<([i32; 4], Instant)>> = Mutex::new(None);
/// A thread is waiting for it to stand still.
static WRITING: AtomicBool = AtomicBool::new(false);

/// The tower moved or changed its size (a drag, its content, `arrange`): where it stands now is
/// where it comes back, after a hide and after a restart. Only while it shows, and once it has
/// been put in its place: a window just made is wherever the system made it.
pub fn frame_changed<R: Runtime>(app: &AppHandle<R>) {
    if !PLACED.load(Ordering::SeqCst) {
        return;
    }
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let Some(place) = frame(&window).map(Rect::rounded) else {
        return;
    };

    *LEFT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((place, Instant::now()));
    if WRITING.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    thread::spawn(move || loop {
        thread::sleep(STILL);
        let mut left = LEFT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match *left {
            Some((_, at)) if at.elapsed() < STILL => {}
            Some((place, _)) => {
                *left = None;
                drop(left);
                if let Some(state) = app.try_state::<AppState>() {
                    state.core.set_tower_place(place);
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

/// The app is on its way out: a place still waiting to stand still (`frame_changed`) is written
/// now, or the tower would come back where it stood before it was last moved.
pub fn flush(core: &crate::core::Core) {
    let waiting = LEFT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some((place, _)) = waiting {
        core.set_tower_place(place);
    }
}

/// On every desktop (Space), or on the one it was shown on; over a full-screen app's too, and
/// out of the window cycle either way.
#[cfg(target_os = "macos")]
fn spaces<R: Runtime>(window: &WebviewWindow<R>, all: bool) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;

    let _ = window.run_on_main_thread(move || {
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        // SAFETY: as in `tray::focus_alone`: this window's NSWindow, on the main thread.
        let ns_window = unsafe { &*(ns_window as *const NSWindow) };
        let every = NSWindowCollectionBehavior::CanJoinAllSpaces;
        let kept = ns_window.collectionBehavior() | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::IgnoresCycle;
        let behaviour = if all { kept | every } else { NSWindowCollectionBehavior(kept.0 & !every.0) };
        if behaviour != ns_window.collectionBehavior() {
            ns_window.setCollectionBehavior(behaviour);
        }
    });
}

/// AppKit's screens and frames in the tower's own space: points, the top counted downwards
/// from the main screen's top (AppKit counts upwards from its bottom).
#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CStr;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};

    use super::{Rect, Screen};

    fn turned(rect: NSRect, top: f64) -> Rect {
        Rect { x: rect.origin.x, y: top - (rect.origin.y + rect.size.height), w: rect.size.width, h: rect.size.height }
    }

    fn top_of(all: &NSArray<NSScreen>) -> f64 {
        all.iter().next().map_or(0.0, |main| main.frame().size.height)
    }

    /// The main screen's height (the menu bar's, where AppKit counts from): what turns AppKit's
    /// frames over.
    pub fn top(mtm: MainThreadMarker) -> f64 {
        top_of(&NSScreen::screens(mtm))
    }

    /// Every screen, the main one first, and the main one's height (`top`). The Dock's own
    /// rectangle is asked for here, as the screens are read, and nowhere else.
    pub fn screens(mtm: MainThreadMarker) -> (Vec<Screen>, f64) {
        let all = NSScreen::screens(mtm);
        let top = top_of(&all);
        let dock = dock();
        let screens = all
            .iter()
            .map(|screen| {
                let (frame, area) = (turned(screen.frame(), top), turned(screen.visibleFrame(), top));
                match dock.and_then(|dock| beside(frame, area, dock)) {
                    Some((area, dock)) => Screen { frame, area, dock: Some(dock) },
                    None => Screen { frame, area, dock: None },
                }
            })
            .collect();
        (screens, top)
    }

    /// The Dock's rectangle as the window server has it at this moment, in CoreGraphics' space,
    /// which is the tower's (points from the main screen's top left, downwards); hidden, it has
    /// no thickness. Only its length is taken (`beside`). The Dock's own window is no help: on
    /// macOS 26 it is as large as its screen. The function is private (SkyLight's, under its two
    /// names), so it is looked up by name: without it, or when it fails, there is none.
    fn dock() -> Option<Rect> {
        type Connection = unsafe extern "C" fn() -> i32;
        type DockRect = unsafe extern "C" fn(i32, *mut NSRect, *mut i32) -> i32;
        // SAFETY: looks a name up in the images loaded already; nothing is called.
        let find = |names: [&CStr; 2]| names.into_iter().map(|name| unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }).find(|found| !found.is_null());
        let connection = find([c"SLSMainConnectionID", c"CGSMainConnectionID"])?;
        let dock_rect = find([c"SLSGetDockRectWithReason", c"CGSGetDockRectWithReason"])?;
        // SAFETY: both are functions of these types, as SkyLight declares them: the app's own
        // connection to the window server; and, given that connection, a CGRect and a reason
        // code to fill, a CGError back.
        let connection = unsafe { std::mem::transmute::<*mut libc::c_void, Connection>(connection) };
        // SAFETY: as above.
        let dock_rect = unsafe { std::mem::transmute::<*mut libc::c_void, DockRect>(dock_rect) };
        let (mut rect, mut reason) = (NSRect::ZERO, 0);
        // SAFETY: as above; both pointers to our own values.
        let error = unsafe { dock_rect(connection(), &mut rect, &mut reason) };
        let rect = Rect { x: rect.origin.x, y: rect.origin.y, w: rect.size.width, h: rect.size.height };
        let sane = [rect.x, rect.y, rect.w, rect.h].iter().all(|side| side.is_finite()) && rect.w >= 0.0 && rect.h >= 0.0 && rect.w + rect.h > 0.0;
        (error == 0 && sane).then_some(rect)
    }

    /// The Dock on this screen: when AppKit took a band off the screen's bottom, left or right
    /// (`area`, its visible frame) and the window server's Dock (`dock`) lies in that band, the
    /// screen's area with the band given back and the Dock as thick as the band, as long as the
    /// window server has it. Else none: the band stays out (a Dock that hides takes none, and
    /// one on another screen none here).
    fn beside(frame: Rect, area: Rect, dock: Rect) -> Option<(Rect, Rect)> {
        let (right, bottom) = (frame.x + frame.w, frame.y + frame.h);
        let (free_right, free_bottom) = (area.x + area.w, area.y + area.h);
        // Each band, whether it runs across the screen, and the area with it given back.
        let bands = [
            (Rect { x: frame.x, y: free_bottom, w: frame.w, h: bottom - free_bottom }, true, Rect { h: bottom - area.y, ..area }),
            (Rect { x: frame.x, y: area.y, w: area.x - frame.x, h: area.h }, false, Rect { x: frame.x, w: free_right - frame.x, ..area }),
            (Rect { x: free_right, y: area.y, w: right - free_right, h: area.h }, false, Rect { w: right - area.x, ..area }),
        ];
        let (middle_x, middle_y) = (dock.x + dock.w / 2.0, dock.y + dock.h / 2.0);
        let holds = |band: &Rect| {
            band.w > 0.0 && band.h > 0.0 && (band.x..=band.x + band.w).contains(&middle_x) && (band.y..=band.y + band.h).contains(&middle_y)
        };
        let (band, across, area) = bands.into_iter().find(|(band, _, _)| holds(band))?;
        // Whole points, rounded outwards.
        let dock = if across {
            let (from, to) = (dock.x.max(band.x).floor(), (dock.x + dock.w).min(band.x + band.w).ceil());
            Rect { x: from, w: to - from, ..band }
        } else {
            let (from, to) = (dock.y.max(band.y).floor(), (dock.y + dock.h).min(band.y + band.h).ceil());
            Rect { y: from, h: to - from, ..band }
        };
        (dock.w > 0.0 && dock.h > 0.0).then_some((area, dock))
    }

    pub fn frame(window: &NSWindow, top: f64) -> Rect {
        turned(window.frame(), top)
    }

    pub fn set_frame(window: &NSWindow, to: Rect, top: f64) {
        window.setFrame_display(NSRect::new(NSPoint::new(to.x, top - (to.y + to.h)), NSSize::new(to.w, to.h)), true);
    }
}

/// Where the pointer is over the tower, told to its page (`tower-pointer`). WebKit lights
/// nothing under the pointer in a window that hasn't the keyboard, and the tower never has it;
/// so a tracking area of the tower's own, active whichever app is in front, hears the pointer
/// and the page lights the row itself. Nothing is heard while the window is hidden.
#[cfg(target_os = "macos")]
mod pointer {
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::OnceLock;

    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
    use objc2::{class, msg_send, sel, ClassType, MainThreadMarker};
    use objc2_app_kit::{NSEvent, NSWindow};
    use objc2_foundation::NSRect;
    use tauri::{Emitter, Manager, Runtime, WebviewWindow};

    /// The pointer's place in the page, from its top left.
    #[derive(Clone, serde::Serialize)]
    struct At {
        x: f64,
        y: f64,
    }

    /// Entered and left, moved, whichever app is active, over the whole view as it is resized
    /// (`NSTrackingMouseEnteredAndExited | NSTrackingMouseMoved | NSTrackingActiveAlways |
    /// NSTrackingInVisibleRect`).
    const OPTIONS: usize = 0x01 | 0x02 | 0x80 | 0x200;
    /// No place told yet, or the pointer left.
    const NOWHERE: i64 = i64::MIN;

    /// Sends the page the pointer's place (none: it left).
    static TELL: OnceLock<Box<dyn Fn(Option<At>) + Send + Sync>> = OnceLock::new();
    /// The place told last, in whole points: one that hasn't changed isn't told again.
    static TOLD: AtomicI64 = AtomicI64::new(NOWHERE);
    /// The view followed now: it is followed once.
    static FOLLOWED: AtomicUsize = AtomicUsize::new(0);

    fn tell(at: Option<(f64, f64)>) {
        let key = at.map_or(NOWHERE, |(x, y)| ((x.round() as i64) << 32) | (y.round() as i64 & 0xffff_ffff));
        if TOLD.swap(key, Ordering::SeqCst) == key {
            return;
        }
        if let Some(tell) = TELL.get() {
            tell(at.map(|(x, y)| At { x, y }));
        }
    }

    extern "C-unwind" fn moved(_: &AnyObject, _: Sel, event: &NSEvent) {
        let Some(window) = MainThreadMarker::new().and_then(|mtm| event.window(mtm)) else {
            return;
        };
        // AppKit counts from the window's bottom, the page from its top.
        let (at, height) = (event.locationInWindow(), window.frame().size.height);
        tell(Some((at.x, height - at.y)));
    }

    extern "C-unwind" fn left(_: &AnyObject, _: Sel, _: &NSEvent) {
        tell(None);
    }

    /// The tracking area's owner: an object that takes the pointer's three messages.
    fn hearer() -> Option<&'static AnyClass> {
        AnyClass::get(c"PitwallTowerPointer").or_else(|| {
            let mut builder = ClassBuilder::new(c"PitwallTowerPointer", NSObject::class())?;
            // SAFETY: each method matches its selector's signature (an NSEvent, nothing back).
            unsafe {
                builder.add_method(sel!(mouseEntered:), moved as extern "C-unwind" fn(_, _, _));
                builder.add_method(sel!(mouseMoved:), moved as extern "C-unwind" fn(_, _, _));
                builder.add_method(sel!(mouseExited:), left as extern "C-unwind" fn(_, _, _));
            }
            Some(builder.register())
        })
    }

    /// Starts hearing the pointer over `window`. Once for each view the window has had.
    pub fn follow<R: Runtime>(window: &WebviewWindow<R>) {
        if TELL.get().is_none() {
            let app = window.app_handle().clone();
            let _ = TELL.set(Box::new(move |at| {
                let _ = app.emit_to(super::LABEL, "tower-pointer", at);
            }));
        }

        let Ok(ns_window) = window.ns_window() else {
            return;
        };
        let ns_window = ns_window as usize;

        let _ = window.run_on_main_thread(move || {
            // SAFETY: as in `tray::focus_alone`: this window's NSWindow, on the main thread.
            let ns_window = unsafe { &*(ns_window as *const NSWindow) };
            let Some(view) = ns_window.contentView() else {
                return;
            };
            let address = Retained::as_ptr(&view) as usize;
            if FOLLOWED.swap(address, Ordering::SeqCst) == address {
                return;
            }
            let Some(class) = hearer() else {
                return;
            };

            // SAFETY: AppKit's own classes, each message with the types it declares. The area
            // doesn't keep its owner alive, so the owner is never let go: one for each view.
            unsafe {
                let owner: Retained<AnyObject> = msg_send![class, new];
                let area: Allocated<AnyObject> = msg_send![class!(NSTrackingArea), alloc];
                let area: Retained<AnyObject> = msg_send![area, initWithRect: NSRect::ZERO, options: OPTIONS, owner: &*owner, userInfo: None::<&AnyObject>];
                let _: () = msg_send![&*view, addTrackingArea: &*area];
                std::mem::forget(owner);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// A screen with a 25-high bar at its top.
    fn screen(x: f64, y: f64, w: f64, h: f64) -> Screen {
        Screen { frame: rect(x, y, w, h), area: rect(x, y + 25.0, w, h - 25.0), dock: None }
    }

    /// The same, its Dock's own rectangle known: the band beside the Dock is free.
    fn docked(screen: Screen, dock: Rect) -> Screen {
        Screen { dock: Some(dock), ..screen }
    }

    /// Three screens side by side, as the tower's user has them: the main one, one to its
    /// right standing higher, and one past that standing lower.
    fn desk() -> Vec<Screen> {
        vec![screen(0.0, 0.0, 1512.0, 982.0), screen(1512.0, -98.0, 1920.0, 1080.0), screen(3432.0, 85.0, 1920.0, 1080.0)]
    }

    /// Where it stands, by its own corner (not held from before).
    fn at(from: Option<Rect>, size: (f64, f64), screens: &[Screen], turned: bool) -> Option<Rect> {
        settle(from, size, screens, 16.0, turned, None).map(|(rect, _)| rect)
    }

    fn inside(rect: Rect, area: Rect) -> bool {
        rect.x >= area.x && rect.y >= area.y && rect.x + rect.w <= area.x + area.w && rect.y + rect.h <= area.y + area.h
    }

    /// Wherever the tower was left, it comes back wholly inside one screen's free area: a place
    /// on a screen that is gone, one hanging over an edge, one in a corner whose content grew.
    #[test]
    fn a_place_left_never_puts_the_tower_off_every_screen() {
        let size = (248.0, 190.0);
        let desk = desk();
        let alone = &desk[..1];
        let top_right = rect(1512.0 - 248.0 - 16.0, 25.0 + 16.0, 248.0, 190.0);

        // Never shown before: the main screen's top right.
        assert_eq!(at(None, size, &desk, false), Some(top_right));

        // Left on the third screen, which is no longer there, and on none at all.
        for gone in [rect(5000.0, 400.0, 248.0, 120.0), rect(-900.0, -900.0, 248.0, 120.0), rect(1500.0, 2000.0, 248.0, 120.0)] {
            assert_eq!(at(Some(gone), size, alone, false), Some(top_right), "{gone:?}");
        }
        // The same place while its screen is there: it stays on that screen.
        let third = at(Some(rect(5000.0, 400.0, 248.0, 120.0)), size, &desk, false).unwrap();
        assert!(inside(third, desk[2].area), "{third:?}");

        // Hanging over an edge, a sliver left on the screen: not enough to take hold of.
        assert_eq!(at(Some(rect(-240.0, 300.0, 248.0, 120.0)), size, alone, false), Some(top_right));
        // Enough of it left: pulled wholly in, on that screen.
        assert_eq!(at(Some(rect(-200.0, 300.0, 248.0, 190.0)), size, alone, false), Some(rect(0.0, 300.0, 248.0, 190.0)));

        // Near the bottom right of the screen to the right, its content grew: that corner stays
        // where it was, 40 from the edges, and the tower grows up and to the left.
        let corner = rect(3432.0 - 40.0 - 300.0, 982.0 - 40.0 - 60.0, 300.0, 60.0);
        assert_eq!(at(Some(corner), (360.0, 240.0), &desk, false), Some(rect(3432.0 - 40.0 - 360.0, 982.0 - 40.0 - 240.0, 360.0, 240.0)));
        // In a top left, the other way: the top left stays.
        assert_eq!(at(Some(rect(1600.0, 0.0, 300.0, 60.0)), (360.0, 240.0), &desk, false), Some(rect(1600.0, 0.0, 360.0, 240.0)));

        // A tall list at the main screen's right edge, its middle in the lower half, turned into a
        // strip: the strip stands where the list's head was, against the same edge, not at the
        // screen's bottom. A strip near the bottom turned into a list too tall to hang from it:
        // pulled up, wholly on the screen.
        let list = rect(1264.0, 44.0, 248.0, 938.0);
        assert_eq!(at(Some(list), (1429.0, 32.0), alone, true), Some(rect(83.0, 44.0, 1429.0, 32.0)));
        assert_eq!(at(Some(list), (1429.0, 32.0), alone, false), Some(rect(83.0, 950.0, 1429.0, 32.0)), "a change of size keeps the nearest corner");
        assert_eq!(at(Some(rect(83.0, 900.0, 1429.0, 32.0)), (248.0, 400.0), alone, true), Some(rect(1264.0, 582.0, 248.0, 400.0)));

        // A small strip put by the second screen's right edge grows wider than the room to its left
        // (the stopped projects shown): pulled in to the screen's left edge. Shown fewer again, it
        // goes back to the corner it was put by, not the one the pull left it near.
        let small = rect(3077.0, 300.0, 43.0, 32.0);
        let (wide, hold) = settle(Some(small), (1699.0, 32.0), &desk, 16.0, false, None).unwrap();
        assert_eq!(wide, rect(1512.0, 300.0, 1699.0, 32.0));
        assert_eq!(settle(Some(wide), (43.0, 32.0), &desk, 16.0, false, Some(hold)).map(|(rect, _)| rect), Some(small));

        // The Dock at the main screen's bottom, 900 long, and at the second screen's left, 400
        // long: the room beside a Dock is free. A strip put left or right of the bottom one, in
        // its band, stays there; so does a tower above the left one, in its band.
        let docks = [
            docked(desk[0], rect(306.0, 912.0, 900.0, 70.0)),
            docked(desk[1], rect(1512.0, 300.0, 70.0, 400.0)),
            desk[2],
        ];
        let beside = rect(16.0, 940.0, 280.0, 32.0);
        for place in [beside, rect(1220.0, 930.0, 280.0, 32.0), rect(1520.0, 100.0, 248.0, 190.0)] {
            assert_eq!(at(Some(place), (place.w, place.h), &docks, false), Some(place), "{place:?}");
        }
        // One on a Dock is moved off it as little as it takes: above the bottom one, to the
        // right of the left one.
        assert_eq!(at(Some(rect(250.0, 900.0, 248.0, 60.0)), (248.0, 60.0), &docks, false), Some(rect(250.0, 852.0, 248.0, 60.0)));
        assert_eq!(at(Some(rect(1540.0, 400.0, 248.0, 190.0)), (248.0, 190.0), &docks, false), Some(rect(1582.0, 400.0, 248.0, 190.0)));
        // Grown from beside the Dock into it: above it; shown fewer again, back beside it.
        let (grown, hold) = settle(Some(beside), (400.0, 32.0), &docks, 16.0, false, None).unwrap();
        assert_eq!(grown, rect(16.0, 880.0, 400.0, 32.0));
        assert_eq!(settle(Some(grown), (280.0, 32.0), &docks, 16.0, false, Some(hold)).map(|(rect, _)| rect), Some(beside));
        // Never shown before, a Dock down the main screen's right: as far from it as from the top.
        let right = docked(desk[0], rect(1442.0, 60.0, 70.0, 900.0));
        assert_eq!(at(None, size, &[right], false), Some(rect(1442.0 - 16.0 - 248.0, 25.0 + 16.0, 248.0, 190.0)));

        // Whatever the place and the size, the tower ends inside a screen's free area, off its
        // Dock.
        let mut seed = 7u64;
        let mut next = |span: f64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) * span
        };
        for _ in 0..2000 {
            let from = rect(next(8000.0) - 1500.0, next(3000.0) - 1000.0, 40.0 + next(900.0), 30.0 + next(900.0));
            let size = (40.0 + next(2400.0), 30.0 + next(1400.0));
            for screens in [&desk[..], alone, &docks[..], &[right][..]] {
                for turned in [false, true] {
                    let to = at(Some(from), size, screens, turned).unwrap();
                    let free = |screen: &Screen| inside(to, screen.area) && screen.dock.is_none_or(|dock| !to.overlaps(dock));
                    assert!(screens.iter().any(free), "{from:?} {size:?} {turned} -> {to:?}");
                }
            }
        }

        // Without a screen there is nowhere to stand.
        assert_eq!(at(None, size, &[], false), None);
    }
}
