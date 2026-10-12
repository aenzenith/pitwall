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
    /// settings and the teams have their own commands: those stay as they are here, whatever
    /// the form holds of them, which may be older. Taken and put back under one lock, so
    /// nothing changes in between.
    pub fn set_settings_form(&self, form: Settings) {
        let language = form.language.clone();
        {
            let mut inner = self.lock();
            let kept = std::mem::take(&mut inner.settings);
            inner.settings = Settings {
                projects: kept.projects,
                order: kept.order,
                shortcut_keys: kept.shortcut_keys,
                projects_dir: kept.projects_dir,
                board_scope: kept.board_scope,
                board_view: kept.board_view,
                track: kept.track,
                teams: kept.teams,
                ..form
            };
        }

        self.save_settings();
        i18n::set(i18n::resolve(&language));
        self.notify();
    }

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

    pub fn set_project_settings(&self, path: &str, project: ProjectSettings) {
        {
            let mut inner = self.lock();
            if project == ProjectSettings::default() {
                inner.settings.projects.remove(path);
            } else {
                inner.settings.projects.insert(path.to_string(), project);
            }
        }

        self.save_settings();
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
