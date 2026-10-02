//! System notifications on Windows: toasts, silent, since Pitwall plays its own sound
//! (`sound.rs`). A toast names the app it comes from by an AppUserModelID that Windows knows from
//! a Start menu shortcut: the NSIS and MSI installers give Pitwall's shortcut the bundle
//! identifier. A dev binary has no shortcut, so it posts on behalf of PowerShell, as Tauri's own
//! notification plugin does. A click on the toast opens the project while Pitwall runs.

use std::path::Path;
use std::sync::OnceLock;
use std::thread;

use tauri_winrt_notification::{IconCrop, Toast};

/// `identifier` in `tauri.conf.json`: the AppUserModelID of the installed app's shortcut.
const APP_ID: &str = "com.aenzenith.pitwall";

type OnClick = Box<dyn Fn(String) + Send + Sync>;

/// What a click on a notification does with its project.
static ON_CLICK: OnceLock<OnClick> = OnceLock::new();
static BUNDLED: OnceLock<bool> = OnceLock::new();

/// Once, at start: takes the clicks and picks the sender (the installed app, or PowerShell).
pub fn start(bundled: bool, on_click: impl Fn(String) + Send + Sync + 'static) {
    let _ = BUNDLED.set(bundled);
    let _ = ON_CLICK.set(Box::new(on_click));
}

/// A notification about `path`, with `image` (a PNG) as the toast's logo.
pub fn post(path: String, title: String, body: String, image: Option<&Path>) {
    let sender = if BUNDLED.get().copied().unwrap_or(false) { APP_ID } else { Toast::POWERSHELL_APP_ID };
    let image = image.map(Path::to_path_buf);

    // Showing goes through WinRT, off the thread that asked.
    thread::spawn(move || {
        let mut toast = Toast::new(sender).title(&title).text1(&body).sound(None);
        if let Some(image) = image.as_deref() {
            toast = toast.icon(image, IconCrop::Square, "");
        }
        let _ = toast
            .on_activated(move |_action| {
                if let Some(on_click) = ON_CLICK.get() {
                    on_click(path.clone());
                }
                Ok(())
            })
            .show();
    });
}
