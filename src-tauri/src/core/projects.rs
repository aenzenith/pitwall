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

    pub fn set_settings(&self, settings: Settings) {
        settings.save(&self.cfg.settings_file);
        i18n::set(i18n::resolve(&settings.language));
        self.lock().settings = settings;
        self.notify();
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// The list order after a drag; paths not in it fall back to name order after it.
    pub fn reorder(&self, paths: Vec<String>) {
        let settings = {
            let mut inner = self.lock();
            inner.settings.order = merge_order(&inner.settings.order, paths);
            inner.settings.clone()
        };

        settings.save(&self.cfg.settings_file);
        self.notify();
    }

    pub fn set_project_settings(&self, path: &str, project: ProjectSettings) {
        let settings = {
            let mut inner = self.lock();
            if project == ProjectSettings::default() {
                inner.settings.projects.remove(path);
            } else {
                inner.settings.projects.insert(path.to_string(), project);
            }
            inner.settings.clone()
        };

        settings.save(&self.cfg.settings_file);
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
