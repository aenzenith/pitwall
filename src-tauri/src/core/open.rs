//! Opening things for the user: a project's address in the browser (a tab that already shows it
//! comes forward), a project's links, and the project's editor window.

use std::process::Stdio;

use super::*;
use crate::resolve::{herd_fallback_url, parse_app_url};

impl Core {
    /// Setting > `APP_URL` in `.env` > `<folder>.test` for Laravel > the server's own address.
    pub fn resolve_url(&self, path: &str, local: Option<String>) -> Option<String> {
        let configured = self.lock().settings.project(path).url.filter(|u| !u.trim().is_empty());

        if configured.is_some() {
            return configured;
        }

        if let Some(app_url) = fs::read_to_string(Path::new(path).join(".env")).ok().and_then(|env| parse_app_url(&env)) {
            return Some(app_url);
        }

        if Path::new(path).join("artisan").exists() {
            return Some(herd_fallback_url(&folder_name(path)));
        }

        local
    }

    /// Opens one of a project's links: the tab that already shows it comes forward, else it opens.
    /// Off the calling thread, as asking the browsers can take a moment. Only web and mail
    /// addresses open (see `links::normalize`); anything else is an error.
    pub fn open_url(self: &Arc<Self>, url: &str) -> Result<(), String> {
        if url.trim().is_empty() {
            return Ok(());
        }
        let url = crate::links::normalize(url).ok_or_else(|| t!("core.error.linkScheme", url = url.trim()))?;
        let core = Arc::clone(self);

        thread::spawn(move || {
            if !crate::browser::focus_page(&url) {
                core.emit(CoreEvent::Open(url));
            }
        });
        Ok(())
    }

    pub fn open_browser(self: &Arc<Self>, path: &str) {
        let local = self.snapshot().projects.into_iter().find(|p| p.path == path).and_then(|p| p.url);
        self.show_in_browser(path, local, false);
    }

    /// Shows the project in the browser: a tab that already has it (its address or the dev
    /// server's) comes forward, reloaded with `reload`; without one, the address opens. Off the
    /// calling thread, as asking the browsers can take a moment. Only a web address opens, so a
    /// project's setting or `.env` can't open a local file or app.
    pub(super) fn show_in_browser(self: &Arc<Self>, path: &str, local: Option<String>, reload: bool) {
        let Some(url) = self.resolve_url(path, local.clone()).and_then(|url| crate::links::normalize(&url)) else {
            return;
        };
        let urls: Vec<String> = std::iter::once(url.clone()).chain(local.filter(|l| *l != url)).collect();
        let core = Arc::clone(self);

        thread::spawn(move || {
            if !crate::browser::focus_tab(&urls, reload) {
                core.emit(CoreEvent::Open(url));
            }
        });
    }

    /// Opening the project's window is looking at it: its Claude turn counts as seen.
    pub fn open_editor(&self, path: &str) {
        self.launch_editor(path);
        self.mark_seen(path);
    }

    /// Brings the project's editor window to the front, or opens the folder in a new window.
    pub(super) fn launch_editor(&self, path: &str) {
        let settings = self.lock().settings.clone();

        // `open -a <Editor> <folder>` focuses the window that has the folder open and opens a
        // new one otherwise. The URL scheme below is the fallback.
        #[cfg(target_os = "macos")]
        {
            let opened = Command::new("open")
                .args(["-a", settings.editor_app(), path])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());

            if opened {
                return;
            }
        }

        let scheme = settings.editor_scheme().to_string();
        let mut url_path = path.replace('\\', "/");

        if !url_path.starts_with('/') {
            url_path = format!("/{url_path}");
        }

        self.emit(CoreEvent::Open(format!("{scheme}://file{url_path}")));
    }
}
