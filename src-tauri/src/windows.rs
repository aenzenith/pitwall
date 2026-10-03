//! The main window and the quick switcher are made the first time they are wanted, not at launch,
//! so a Pitwall started at login keeps only the popover's page. Their settings stay in
//! `tauri.conf.json` (`"create": false`) and are built from there, exactly as at launch.
//!
//! A window made on first use comes up once its page says it is ready (`window_ready`), so the
//! first opening is a painted page whose listeners are in place, not a blank window.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

use crate::{tray, MAIN, SWITCHER};

/// Should a page never say it is ready, its window comes up after this anyway.
const FALLBACK: Duration = Duration::from_secs(3);

/// Events the switcher (or the core: a notification, a Claude session in one of Pitwall's
/// terminals) sends the main window just before opening it. If the window's page isn't listening
/// yet they would be lost, so the last one waits here and is sent again once it is.
const FOR_MAIN: [&str; 5] = ["reveal-project", "reveal-output", "reveal-fuel", "reveal-terminal", "reveal-page"];

struct Pages {
    /// Windows whose page is up and listening.
    ready: Vec<String>,
    /// Windows to bring up as soon as their page is.
    waiting: Vec<String>,
    /// One of `FOR_MAIN` with its payload, sent before the main window's page was listening.
    held: Option<(&'static str, String)>,
}

static PAGES: Mutex<Pages> = Mutex::new(Pages { ready: Vec::new(), waiting: Vec::new(), held: None });

fn pages() -> std::sync::MutexGuard<'static, Pages> {
    PAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn is_ready(pages: &Pages, label: &str) -> bool {
    pages.ready.iter().any(|l| l == label)
}

/// Keeps what the switcher sends the main window while that window's page isn't there yet.
pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    for event in FOR_MAIN {
        app.listen_any(event, move |sent| {
            let mut pages = pages();
            if !is_ready(&pages, MAIN) {
                pages.held = Some((event, sent.payload().to_string()));
            }
        });
    }
}

/// The window `label`, made now from its `tauri.conf.json` entry if it doesn't exist yet. Only
/// for showing it: code that merely hides or looks at a window uses `get_webview_window`.
pub fn get_or_create<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(label) {
        return Some(window);
    }

    let config = app.config().app.windows.iter().find(|config| config.label == label)?.clone();
    // Linux composites a see-through window only on some desktops (black corners elsewhere), so
    // the switcher is opaque there and its page paints the whole window.
    #[cfg(target_os = "linux")]
    let config = tauri::utils::config::WindowConfig { transparent: false, ..config };
    // A new page, should an earlier window of this label have gone: not listening yet.
    pages().ready.retain(|l| l != label);
    let build = || {
        WebviewWindowBuilder::from_config(app, &config).and_then(|builder| {
            builder
                // A reload (or a crashed page coming back) is not listening until it says so again.
                .on_page_load(|window, load| {
                    if load.event() == PageLoadEvent::Started {
                        pages().ready.retain(|l| l != window.label());
                    }
                })
                .build()
        })
    };
    // The popover and the switcher are panels over the app you are in: Pitwall stays behind it
    // while they are made, too.
    #[cfg(target_os = "macos")]
    let panel = label == SWITCHER || label == crate::POPOVER;
    #[cfg(target_os = "macos")]
    let built = if panel { tray::without_activating(build) } else { build() };
    #[cfg(not(target_os = "macos"))]
    let built = build();

    let window = match built {
        Ok(window) => window,
        Err(_error) => {
            #[cfg(debug_assertions)]
            eprintln!("[pitwall] create {label}: {_error}");
            // Made meanwhile by another caller.
            return app.get_webview_window(label);
        }
    };

    #[cfg(target_os = "macos")]
    if panel {
        tray::make_panel(&window);
        tray::float_over_spaces(&window);
    }

    Some(window)
}

/// Brings `label` up: made if need be, and shown (`present`) once its page is ready.
pub fn open<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if get_or_create(app, label).is_none() {
        return;
    }

    {
        let mut pages = pages();
        if !is_ready(&pages, label) {
            if !pages.waiting.iter().any(|l| l == label) {
                pages.waiting.push(label.to_string());
            }
            drop(pages);

            let (app, label) = (app.clone(), label.to_string());
            thread::spawn(move || {
                thread::sleep(FALLBACK);
                if take_waiting(&label) {
                    present(&app, &label);
                }
            });
            return;
        }
    }

    present(app, label);
}

fn take_waiting(label: &str) -> bool {
    let mut pages = pages();
    let before = pages.waiting.len();
    pages.waiting.retain(|l| l != label);
    pages.waiting.len() != before
}

/// A page is up and listening: what it missed is sent again, and its window comes up if it was
/// asked for meanwhile.
pub fn ready<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let held = {
        let mut pages = pages();
        if !is_ready(&pages, label) {
            pages.ready.push(label.to_string());
        }
        if label == MAIN {
            pages.held.take()
        } else {
            None
        }
    };

    if let Some((event, payload)) = held {
        let _ = app.emit_str_to(MAIN, event, payload);
    }
    if take_waiting(label) {
        present(app, label);
    }
}

fn present<R: Runtime>(app: &AppHandle<R>, label: &str) {
    match label {
        MAIN => tray::present_main(app),
        SWITCHER => tray::present_switcher(app),
        _ => {}
    }
}

#[derive(Clone, serde::Serialize)]
struct Visibility {
    visible: bool,
}

/// Tells a window's page whether Pitwall has it on screen, so it can pause animations and timers
/// while hidden.
pub fn tell<R: Runtime>(app: &AppHandle<R>, label: &str, visible: bool) {
    let _ = app.emit_to(label, "visibility", Visibility { visible });
}

/// Shows a window and tells its page.
pub fn show<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    tell(window.app_handle(), window.label(), true);
    window.show()
}

/// Hides a window and tells its page.
pub fn hide<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let hidden = window.hide();
    tell(window.app_handle(), window.label(), false);
    hidden
}
