//! The projects the app lists and how they are set up: projects added and made favourite, the
//! app's and each project's settings, the order dragged into place, and the folders of the
//! projects folder for the quick switcher.
//!
//! A project added to Pitwall stays in its list (`Settings::added`) until it is removed from it;
//! its star (`favorites.json`, shared with the VS Code extension) is a mark on top. Adding one
//! stars it too, but from the quick switcher (`add_unstarred`).

use super::*;

/// Every favourite among the added projects, so that taking its star off later, here or in a VS
/// Code window, leaves it in the list. Whether one was missing. Only where Pitwall reads the
/// favourites for itself (at its start, after its own writes): a heartbeat's read may be older
/// than a removal and would put the project back.
pub(super) fn keep_favourites(settings: &mut Settings, favourites: &[Favourite]) -> bool {
    let added = settings.added.get_or_insert_with(Vec::new);
    let before = added.len();
    for favourite in favourites {
        if !added.iter().any(|path| same_path(path, &favourite.path)) {
            added.push(favourite.path.clone());
        }
    }
    added.len() != before
}

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
        self.reload_favourites();
        self.notify();
        Ok(())
    }

    /// A folder added from the quick switcher: one of Pitwall's projects, with no star.
    pub fn add_unstarred(&self, path: &str) -> Result<(), String> {
        if !Path::new(path).is_dir() {
            return Err(t!("core.error.notFolder", path = path));
        }

        let favourite = Favourite { path: path.to_string(), name: folder_name(path) };
        if keep_favourites(&mut self.lock().settings, std::slice::from_ref(&favourite)) {
            self.save_settings();
        }
        self.notify();
        Ok(())
    }

    /// A star put on or taken off. Either way the project is one of the added ones from then: the
    /// star taken off leaves it in the list (`remove_project` takes it out).
    pub fn set_favourite(&self, path: &str, on: bool) {
        let favourite = Favourite { path: path.to_string(), name: folder_name(path) };
        if keep_favourites(&mut self.lock().settings, std::slice::from_ref(&favourite)) {
            self.save_settings();
        }
        self.registry.set_favourite(favourite, on);
        self.reload_favourites();
        self.notify();
    }

    /// "Remove from Pitwall": the project is no added one and no favourite any more. A VS Code
    /// window that has it open, or a server of it that runs, still shows it, as for any project.
    pub fn remove_project(&self, path: &str) {
        self.registry.set_favourite(Favourite { path: path.to_string(), name: folder_name(path) }, false);
        self.reload_favourites();
        {
            let mut inner = self.lock();
            if let Some(added) = &mut inner.settings.added {
                added.retain(|known| !same_path(known, path));
            }
        }
        self.save_settings();
        self.notify();
    }

    /// The favourites read again after a write of Pitwall's own, each kept among the added
    /// projects (`keep_favourites`).
    pub(super) fn reload_favourites(&self) {
        let favourites = self.registry.read_favourites();
        let grew = {
            let mut inner = self.lock();
            let grew = keep_favourites(&mut inner.settings, &favourites);
            inner.favourites = favourites;
            grew
        };
        if grew {
            self.save_settings();
        }
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
                favourites_only: kept.favourites_only,
                added: kept.added,
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
                // Claude's accounts: by their own commands (`core/accounts.rs`).
                accounts: kept.accounts,
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

    /// Whether the list shows only the favourites and the projects at work, kept until it is
    /// turned off.
    pub fn set_favourites_only(&self, on: bool) {
        {
            let mut inner = self.lock();
            if inner.settings.favourites_only == on {
                return;
            }
            inner.settings.favourites_only = on;
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
