//! System notifications on Linux, through the desktop's notification server (D-Bus,
//! `org.freedesktop.Notifications`), silent: Pitwall plays its own sound (`sound.rs`). A click
//! opens the project on servers that take actions (GNOME, KDE, most others), and a newer
//! notification of a project replaces the older one, as on macOS.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;

use notify_rust::{Hint, Notification};

use crate::i18n::t;

type OnClick = Box<dyn Fn(String) + Send + Sync>;

/// What a click on a notification does with its project.
static ON_CLICK: OnceLock<OnClick> = OnceLock::new();

/// Each project's notification on screen: the server's id for it, and which post it was.
static SHOWN: Mutex<BTreeMap<String, (u32, u64)>> = Mutex::new(BTreeMap::new());
static POSTS: AtomicU64 = AtomicU64::new(0);

fn shown() -> MutexGuard<'static, BTreeMap<String, (u32, u64)>> {
    SHOWN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Once, at start: takes the clicks. Nothing to ask for; the server shows what it is sent.
pub fn start(_bundled: bool, on_click: impl Fn(String) + Send + Sync + 'static) {
    let _ = ON_CLICK.set(Box::new(on_click));
}

/// A notification about `path`, with `image` (a PNG) beside the text.
pub fn post(path: String, title: String, body: String, image: Option<&Path>) {
    let image = image.map(|image| image.to_string_lossy().into_owned());
    let open = t!("common.open");

    // Waiting for the click blocks, so each notification gets its own thread; it ends once the
    // notification is clicked, closed or replaced and then clicked.
    thread::spawn(move || {
        let post = POSTS.fetch_add(1, Ordering::SeqCst);
        let replaces = shown().get(&path).map(|(id, _)| *id);

        let mut notification = Notification::new();
        notification
            .appname("Pitwall")
            .summary(&title)
            .body(&body)
            .hint(Hint::SuppressSound(true))
            .action("default", &open);
        if let Some(image) = image.as_deref() {
            notification.image_path(image);
        }
        if let Some(id) = replaces {
            notification.id(id);
        }

        let Ok(handle) = notification.show() else {
            return;
        };
        shown().insert(path.clone(), (handle.id(), post));

        handle.wait_for_action(|action| {
            // A replaced notification keeps its id, so its thread hears the newer one's click
            // too; only the project's latest post answers.
            let latest = {
                let mut shown = shown();
                let latest = shown.get(&path).is_some_and(|(_, at)| *at == post);
                if latest {
                    shown.remove(&path);
                }
                latest
            };
            if latest && action == "default" {
                if let Some(on_click) = ON_CLICK.get() {
                    on_click(path);
                }
            }
        });
    });
}
