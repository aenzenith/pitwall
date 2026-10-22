//! The projects the app lists and how they are set up: projects added and made favourite, the
//! app's and each project's settings, the order dragged into place, and the folders of the
//! projects folder for the quick switcher.

use super::*;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: String,
}

impl Core {
    pub fn add_project(&self, path: &str) -> Result<(), String> {
        let dir = Path::new(path);

        if !dir.is_dir() {
            return Err(t!("core.error.notFolder", path = path));
        }

        self.registry.set_favourite(Favourite { path: path.to_string(), name: folder_name(path) }, true);
        self.lock().favourites = self.registry.read_favourites();
        self.notify();
        Ok(())
    }

    pub fn set_favourite(&self, path: &str, on: bool) {
        self.registry.set_favourite(Favourite { path: path.to_string(), name: folder_name(path) }, on);
        self.lock().favourites = self.registry.read_favourites();
        self.notify();
    }

    /// The Settings page's form takes hold. Project overrides, the list order, the shortcut
    /// keys, the projects folder, the board shown last, how it is drawn, the Track page's
    /// settings, the teams, what is told and when (the notifications, quiet, the muted
    /// projects), the team radio and what the core notes for itself (the version seen, the
    /// first run, the folders known) have their own commands: those stay as they are here,
    /// whatever the form holds of them, which may be older. Taken and put back under one lock,
    /// so nothing changes in between.
    pub fn set_settings_form(self: &Arc<Self>, form: Settings) {
        let language = form.language.clone();
        let unnamed = {
            let mut inner = self.lock();
            let kept = std::mem::take(&mut inner.settings);
            let unnamed = PackageManager::parse(&kept.package_manager).is_some() && PackageManager::parse(&form.package_manager).is_none();
            // Turned on: its first look only notes what is there (`discover.rs`).
            let seen_folders = if form.suggest_projects && !kept.suggest_projects { Vec::new() } else { kept.seen_folders };
            inner.settings = Settings {
                projects: kept.projects,
                order: kept.order,
                shortcut_keys: kept.shortcut_keys,
                projects_dir: kept.projects_dir,
                board_scope: kept.board_scope,
                board_view: kept.board_view,
                files_preview: kept.files_preview,
                track: kept.track,
                teams: kept.teams,
                seen_version: kept.seen_version,
                onboarded: kept.onboarded,
                seen_folders,
                // Settings › Notifications' own, each by its command (`signals.rs`).
                notify: kept.notify,
                notifications: kept.notifications,
                fuel_alert: kept.fuel_alert,
                radio: kept.radio,
                // The chequered flag's, the mini tower's and the command line's: each by its own commands.
                flag: kept.flag,
                tower: kept.tower,
                cli: kept.cli,
                ..form
            };
            unnamed
        };

        self.save_settings();
        i18n::set(i18n::resolve(&language));
        self.retell_issues();
        self.notify();
        if unnamed {
            self.managers_unnamed();
        }
    }

    /// The settings as a whole, as a test sets a core up; the app changes them part by part.
    #[cfg(test)]
    pub fn set_settings(&self, settings: Settings) {
        i18n::set(i18n::resolve(&settings.language));
        self.lock().settings = settings;
        self.save_settings();
        self.notify();
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Writes the settings as they are now. One writer at a time, and each takes what it writes
    /// once its turn has come: the file never ends up older than what the core holds.
    pub(super) fn save_settings(&self) {
        let _writing = self.settings_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let settings = self.lock().settings.clone();
        settings.save(&self.cfg.settings_file);
    }

    /// New keys for the quick switcher, which is on from then.
    pub fn set_shortcut_keys(&self, keys: String) {
        {
            let mut inner = self.lock();
            inner.settings.shortcut = true;
            inner.settings.shortcut_keys = keys;
        }

        self.save_settings();
        self.notify();
    }

    /// The list order after a drag; paths not in it fall back to name order after it.
    pub fn reorder(&self, paths: Vec<String>) {
        {
            let mut inner = self.lock();
            inner.settings.order = merge_order(&inner.settings.order, paths);
        }

        self.save_settings();
        self.notify();
    }

    /// The board the Board page shows (`all`, or a project's path), kept for its next opening.
    pub fn set_board_scope(&self, scope: String) {
        {
            let mut inner = self.lock();
            if inner.settings.board_scope.as_deref() == Some(scope.as_str()) {
                return;
            }
            inner.settings.board_scope = Some(scope);
        }

        self.save_settings();
        self.notify();
    }

    /// How the Board page draws a project's board (`columns` or `list`), kept for its next opening.
    pub fn set_board_view(&self, view: String) {
        {
            let mut inner = self.lock();
            if inner.settings.board_view.as_deref() == Some(view.as_str()) {
                return;
            }
            inner.settings.board_view = Some(view);
        }

        self.save_settings();
        self.notify();
    }

    /// Whether the Files window shows a Markdown or SVG file as its preview, kept until it is turned off.
    pub fn set_files_preview(&self, on: bool) {
        {
            let mut inner = self.lock();
            if inner.settings.files_preview == on {
                return;
            }
            inner.settings.files_preview = on;
        }

        self.save_settings();
        self.notify();
    }

    /// The Track page's own settings (its circuit, what the cars carry, how much moves), kept for
    /// its next opening.
    pub fn set_track(&self, track: TrackSettings) {
        {
            let mut inner = self.lock();
            if inner.settings.track.as_ref() == Some(&track) {
                return;
            }
            inner.settings.track = Some(track);
        }

        self.save_settings();
        self.notify();
    }

    /// Where the project at `path` runs (`ProjectSettings::work_dir`): its settings read under
    /// the lock, its folder looked at after it.
    pub fn work_dir(&self, path: &str) -> Option<PathBuf> {
        let project = self.lock().settings.project(path);
        project.work_dir(path)
    }

    pub fn set_project_settings(self: &Arc<Self>, path: &str, project: ProjectSettings) {
        let moved = {
            let mut inner = self.lock();
            // Its lock file is looked for again where it runs now (`detect_managers`).
            let moved = inner.settings.project(path).folder != project.folder;
            if moved {
                inner.managers.remove(path);
            }
            if project == ProjectSettings::default() {
                inner.settings.projects.remove(path);
            } else {
                inner.settings.projects.insert(path.to_string(), project);
            }
            moved
        };

        self.save_settings();
        self.notify();
        if moved {
            self.refresh_git(vec![path.to_string()]);
        }
    }

    /// This version's notes, in the language spoken.
    pub fn whats_new(&self) -> crate::whatsnew::WhatsNew {
        let inner = self.lock();
        crate::whatsnew::view(inner.whats_new, i18n::current())
    }

    /// The notes were shown: they don't come up by themselves again.
    pub fn whats_new_seen(&self) {
        let noted = {
            let mut inner = self.lock();
            inner.whats_new = false;
            let noted = inner.settings.seen_version.as_deref() != Some(crate::whatsnew::CURRENT);
            if noted {
                inner.settings.seen_version = Some(crate::whatsnew::CURRENT.to_string());
            }
            noted
        };

        if noted {
            self.save_settings();
        }
        self.notify();
    }

    /// Subfolders of the projects folder (see `Settings::projects_dir`), for the quick switcher's
    /// search when no project matches. Hidden folders and files are left out.
    pub fn project_folders(&self) -> Vec<Folder> {
        let Some(dir) = self.lock().settings.projects_dir.clone() else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut folders: Vec<Folder> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                (!name.starts_with('.') && path.is_dir()).then(|| Folder { name, path: path.to_string_lossy().into_owned() })
            })
            .take(5000)
            .collect();

        folders.sort_by_key(|folder| folder.name.to_lowercase());
        folders
    }
}

/// The new order of the listed projects, keeping the place of projects that aren't listed right
/// now (a closed window that isn't a favourite): each stays right after the project it followed,
/// so it comes back where it was.
pub(super) fn merge_order(old: &[String], listed: Vec<String>) -> Vec<String> {
    let mut merged = listed;
    let mut anchor: Option<String> = None;

    for path in old {
        if merged.contains(path) {
            anchor = Some(path.clone());
            continue;
        }

        let at = anchor.as_ref().and_then(|a| merged.iter().position(|p| p == a)).map_or(0, |i| i + 1);
        merged.insert(at, path.clone());
        anchor = Some(path.clone());
    }

    merged
}
