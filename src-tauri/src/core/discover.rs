//! The projects folder looked through: which of its subfolders could be projects (the first
//! run's list), and which appeared since (offered under the project list). Of a subfolder, its
//! `package.json` is read, for the dev script and for what it is built with; besides that only
//! whether it has an `artisan` file or a `src-tauri` folder, and when it was last written to
//! (its `package.json`, Git's `index` and `HEAD`, the folder itself).

use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;

use super::*;
use crate::files::{read_regular, MANIFEST_BYTES};

/// As many subfolders as the quick switcher's search takes.
const FOLDERS: usize = 5000;
/// Among the folders known: the projects folder was looked at once.
const LOOKED: &str = ".";

/// A subfolder of the projects folder that has the dev script.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    /// What it is built with, a proper name: `Laravel + Vite`, `Next.js`, `Vite + Vue`…
    pub kind: String,
    /// When it was last worked on (ms); None when that can't be told.
    pub changed_at: Option<u64>,
    /// Already in the list.
    pub listed: bool,
}

/// The projects folder as the first run shows it. `dir` None: no projects folder is set.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub dir: Option<String>,
    /// How many subfolders it has in all.
    pub folders: usize,
    pub candidates: Vec<Candidate>,
}

/// The folder's subfolders that aren't hidden: (name, path). None when it can't be read.
fn subfolders(dir: &str) -> Option<Vec<(String, PathBuf)>> {
    let entries = fs::read_dir(dir).ok()?;
    let folders = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            (!name.starts_with('.') && path.is_dir()).then_some((name, path))
        })
        .take(FOLDERS)
        .collect();

    Some(folders)
}

fn modified(file: &Path) -> Option<u64> {
    let time = fs::metadata(file).ok()?.modified().ok()?;
    Some(time.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// When the folder was last worked on: Git's index and head, else its `package.json`, else the
/// folder itself.
fn changed_at(dir: &Path) -> Option<u64> {
    [dir.join(".git").join("index"), dir.join(".git").join("HEAD"), dir.join("package.json")]
        .iter()
        .filter_map(|file| modified(file))
        .max()
        .or_else(|| modified(dir))
}

/// What a project is built with, from its dependencies' names and two files.
fn kind(dir: &Path, manifest: &serde_json::Value) -> String {
    let has = |name: &str| ["dependencies", "devDependencies"].iter().any(|section| manifest.get(section).and_then(|deps| deps.get(name)).is_some());
    let vite = has("vite");
    let with_vite = |name: &str| if vite { format!("{name} + Vite") } else { name.to_string() };

    if dir.join("artisan").is_file() || has("laravel-vite-plugin") {
        return with_vite("Laravel");
    }
    if dir.join("src-tauri").is_dir() || has("@tauri-apps/cli") {
        return with_vite("Tauri");
    }

    let named = [
        ("next", "Next.js"),
        ("nuxt", "Nuxt"),
        ("astro", "Astro"),
        ("@sveltejs/kit", "SvelteKit"),
        ("@remix-run/dev", "Remix"),
        ("@react-router/dev", "React Router"),
        ("@angular/core", "Angular"),
        ("gatsby", "Gatsby"),
        ("@vue/cli-service", "Vue CLI"),
        ("react-scripts", "Create React App"),
        ("expo", "Expo"),
        ("electron", "Electron"),
    ];
    if let Some((_, name)) = named.iter().find(|(package, _)| has(package)) {
        return (*name).to_string();
    }

    if vite {
        return match [("vue", "Vue"), ("react", "React"), ("svelte", "Svelte"), ("solid-js", "Solid"), ("preact", "Preact")].iter().find(|(package, _)| has(package)) {
            Some((_, with)) => format!("Vite + {with}"),
            None => "Vite".to_string(),
        };
    }

    "Node".to_string()
}

/// The folder as a candidate, when its `package.json` is a plain file that has `script`.
fn candidate(name: &str, dir: &Path, script: &str, listed: bool) -> Option<Candidate> {
    let bytes = read_regular(&dir.join("package.json"), MANIFEST_BYTES)?;
    // A byte-order mark before it is no part of it (`resolve::has_script`).
    let manifest: serde_json::Value = serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).ok()?;
    manifest.get("scripts")?.get(script)?.as_str()?;

    Some(Candidate { path: dir.to_string_lossy().into_owned(), name: name.to_string(), kind: kind(dir, &manifest), changed_at: changed_at(dir), listed })
}

impl Core {
    /// The projects folder's subfolders that have the dev script, the one worked on last first.
    pub fn discover_projects(&self) -> Discovery {
        let (dir, script) = {
            let inner = self.lock();
            (inner.settings.projects_dir.clone(), inner.settings.script.clone())
        };
        let Some(dir) = dir else {
            return Discovery::default();
        };

        let listed = self.listed_paths();
        let folders = subfolders(&dir).unwrap_or_default();
        let mut candidates: Vec<Candidate> = folders
            .iter()
            .filter_map(|(name, path)| {
                let is_listed = listed.iter().any(|known| same_path(known, &path.to_string_lossy()));
                candidate(name, path, &script, is_listed)
            })
            .collect();
        candidates.sort_by(|a, b| b.changed_at.cmp(&a.changed_at).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

        Discovery { dir: Some(dir), folders: folders.len(), candidates }
    }

    /// Lists every folder of `paths` that is one; the others are left out without a word.
    pub fn add_projects(&self, paths: &[String]) {
        for path in paths.iter().filter(|path| Path::new(path).is_dir()) {
            self.registry.set_favourite(Favourite { path: path.clone(), name: folder_name(path) }, true);
        }
        self.reload_favourites();
        self.notify();
        self.refresh_suggestions();
    }

    /// A project offered under the list isn't offered again.
    pub fn dismiss_suggestion(&self, path: &str) {
        let name = folder_name(path);
        {
            let mut inner = self.lock();
            if !inner.settings.seen_folders.contains(&name) {
                inner.settings.seen_folders.push(name);
            }
            inner.suggestions.retain(|candidate| candidate.path != path);
        }

        self.save_settings();
        self.notify();
    }

    /// The first run's steps are over: finished or put off.
    pub fn set_onboarded(&self) {
        {
            let mut inner = self.lock();
            if inner.settings.onboarded {
                return;
            }
            inner.settings.onboarded = true;
        }

        self.save_settings();
        self.notify();
    }

    /// The projects folder, set, changed or cleared (None).
    pub fn set_projects_dir(&self, dir: Option<String>) {
        self.lock().settings.projects_dir = dir;
        self.projects_dir_changed();
    }

    /// The projects folder was set, changed or cleared: what was known of the old one is
    /// forgotten, and the new one's subfolders become the ones known.
    pub fn projects_dir_changed(&self) {
        {
            let mut inner = self.lock();
            inner.settings.seen_folders.clear();
            inner.suggestions.clear();
        }

        self.save_settings();
        self.refresh_suggestions();
        self.notify();
    }

    /// `refresh_suggestions` on a thread of its own, one look at a time: a projects folder on a
    /// volume that doesn't answer holds up nothing but the next look.
    pub(super) fn look_for_projects(self: &Arc<Self>) {
        if self.discover_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let core = Arc::clone(self);
        thread::spawn(move || {
            core.refresh_suggestions();
            core.discover_busy.store(false, Ordering::SeqCst);
        });
    }

    /// Looks for projects that appeared in the projects folder (every minute, and after a
    /// change): a subfolder not known so far that has the dev script and isn't listed. The first
    /// look after the setting is turned on only notes what is there: nothing old is offered.
    pub fn refresh_suggestions(&self) {
        let (on, dir, script, mut seen) = {
            let inner = self.lock();
            (inner.settings.suggest_projects, inner.settings.projects_dir.clone(), inner.settings.script.clone(), inner.settings.seen_folders.clone())
        };

        let Some(dir) = dir.filter(|_| on) else {
            let mut inner = self.lock();
            if !inner.suggestions.is_empty() {
                inner.suggestions.clear();
                drop(inner);
                self.notify();
            }
            return;
        };

        let folders = subfolders(&dir);
        let first = seen.is_empty();
        let listed = self.listed_paths();
        let mut offered: Vec<Candidate> = Vec::new();
        let before = seen.len();
        // A folder that can't be read (its volume isn't there yet) wasn't looked at.
        if first && folders.is_some() {
            // The folder was looked at once, whatever it held (no subfolder is named like this).
            seen.push(LOOKED.to_string());
        }

        for (name, path) in folders.iter().flatten() {
            if seen.contains(name) {
                continue;
            }
            let is_listed = listed.iter().any(|known| same_path(known, &path.to_string_lossy()));
            // What is here at the first look, and what got listed some other way, is known.
            if first || is_listed {
                seen.push(name.clone());
            } else if let Some(found) = candidate(name, path, &script, false) {
                offered.push(found);
            }
        }
        offered.sort_by(|a, b| b.changed_at.cmp(&a.changed_at).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

        let (changed, grown) = {
            let mut inner = self.lock();
            let changed = inner.suggestions != offered;
            inner.suggestions = offered;
            let grown = seen.len() != before;
            if grown {
                // What was noted meanwhile (a project put aside) stays.
                for name in seen {
                    if !inner.settings.seen_folders.contains(&name) {
                        inner.settings.seen_folders.push(name);
                    }
                }
            }
            (changed, grown)
        };

        if grown {
            self.save_settings();
        }
        if changed {
            self.notify();
        }
    }
}
