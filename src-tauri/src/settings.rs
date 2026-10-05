//! App settings, kept in the app's config folder (not shared with the extension).

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::registry::{now_ms, write_atomic};

/// A project's own command (queue worker, migrations, tests…), run from the detail panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomCommand {
    pub id: String,
    pub name: String,
    pub command: String,
    /// A worker or watcher: restarted if it crashes, like the dev server.
    pub keep_running: bool,
    /// Ask before running (migrations and other hard-to-undo commands).
    pub confirm: bool,
    /// Started and stopped together with the dev server.
    pub with_server: bool,
}

/// One of a project's other addresses: staging, production, the admin panel, the issue tracker…
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectLink {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CustomCommand>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<ProjectLink>,
}

/// Which of Pitwall's sounds (`sounds/`) each event plays; empty for none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sounds {
    pub claude_finished: String,
    /// Claude asks a question or needs a permission.
    pub claude_asking: String,
    pub server_crashed: String,
    pub server_ready: String,
    pub command_done: String,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            claude_finished: "boxbox".into(),
            claude_asking: "limiter".into(),
            server_crashed: "yellowflag".into(),
            server_ready: String::new(),
            command_done: String::new(),
        }
    }
}

/// The Track page's own settings. The page reads them leniently (src/lib/track.ts:
/// `trackSettings`): a value it doesn't know falls back to its default there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackSettings {
    /// A circuit (`street`, `night`, `oval`, `eight`, `straight`), or `shuffle`: another one each
    /// time the page opens.
    pub circuit: String,
    /// What a car carries: `code` (its project's three letters), `name`, or `hover`.
    pub labels: String,
    /// `full`, `calm` (half speed, short trails) or `still`.
    pub motion: String,
    /// The lights' soft glow.
    pub glow: bool,
}

impl Default for TrackSettings {
    fn default() -> Self {
        Self { circuit: "street".into(), labels: "code".into(), motion: "full".into(), glow: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `system` or a language code from `i18n::LANGUAGES`.
    pub language: String,
    /// npm script to run unless a project overrides it.
    pub script: String,
    /// `auto` | `npm` | `pnpm` | `yarn` | `bun`
    pub package_manager: String,
    /// `vscode` | `cursor` | `vscode-insiders` | `windsurf`
    pub editor: String,
    pub open_url_on_start: bool,
    /// A system notification when Claude waits on you.
    pub notify: bool,
    /// The sound each event plays.
    pub sounds: Sounds,
    pub launch_at_login: bool,
    /// A global shortcut opens the quick switcher from anywhere.
    pub shortcut: bool,
    /// The keys, in global-hotkey form: `Ctrl+Alt+KeyP`, `Alt+Space`, `Super+Shift+KeyK`.
    pub shortcut_keys: String,
    pub projects: BTreeMap<String, ProjectSettings>,
    /// The project list order the user dragged into place.
    pub order: Vec<String>,
    /// The folder whose subfolders the quick switcher searches when no project matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects_dir: Option<String>,
    /// A notification when Claude's session or weekly limit passes 90 %.
    pub fuel_alert: bool,
    /// Asks for new versions of Pitwall by itself and downloads them (`update.rs`). Off, only
    /// "Check now" in Settings asks.
    pub auto_update: bool,
    /// The board the Board page showed last: `all`, or a project's path. None until one is picked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_scope: Option<String>,
    /// How the Board page draws a project's board: `columns` or `list`. None until one is picked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_view: Option<String>,
    /// The Track page's own settings. None until one is changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSettings>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            script: "dev".into(),
            package_manager: "auto".into(),
            editor: "vscode".into(),
            open_url_on_start: false,
            notify: true,
            sounds: Sounds::default(),
            launch_at_login: false,
            shortcut: true,
            shortcut_keys: "Ctrl+Alt+KeyP".into(),
            projects: BTreeMap::new(),
            order: Vec::new(),
            projects_dir: None,
            fuel_alert: true,
            auto_update: true,
            board_scope: None,
            board_view: None,
            track: None,
        }
    }
}

impl Settings {
    /// The saved settings; defaults when there is no file. A file that can't be read or doesn't
    /// parse is moved aside first (`settings.json.broken-<ms>`), so no save ever overwrites what
    /// is in it. Of JSON that doesn't fit as a whole, every field that fits on its own is kept
    /// (each project, command and link separately) and written back at once.
    pub fn load(file: &Path) -> Self {
        let raw = match fs::read(file) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                set_aside(file, &error.to_string());
                return Self::default();
            }
        };

        let value = match serde_json::from_slice::<Value>(&raw) {
            Ok(value) => value,
            Err(error) => {
                set_aside(file, &error.to_string());
                return Self::default();
            }
        };

        match serde_json::from_value::<Self>(value.clone()) {
            Ok(settings) => settings,
            Err(error) => {
                set_aside(file, &error.to_string());
                let settings = salvage(value);
                if settings != Self::default() {
                    settings.save(file);
                }
                settings
            }
        }
    }

    pub fn save(&self, file: &Path) {
        if let Some(parent) = file.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = write_atomic(file, &json);
        }
    }

    pub fn project(&self, path: &str) -> ProjectSettings {
        self.projects.get(path).cloned().unwrap_or_default()
    }

    pub fn script_for(&self, path: &str) -> String {
        self.project(path).script.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| self.script.clone())
    }

    /// The macOS application name of the chosen editor.
    pub fn editor_app(&self) -> &str {
        match self.editor.as_str() {
            "cursor" => "Cursor",
            "vscode-insiders" => "Visual Studio Code - Insiders",
            "windsurf" => "Windsurf",
            _ => "Visual Studio Code",
        }
    }

    /// The URL scheme that focuses (or opens) a folder in the chosen editor.
    pub fn editor_scheme(&self) -> &str {
        match self.editor.as_str() {
            "cursor" => "cursor",
            "vscode-insiders" => "vscode-insiders",
            "windsurf" => "windsurf",
            _ => "vscode",
        }
    }
}

/// Moves a settings file that can't be used to `<file>.broken-<ms>` (with `-1`, `-2`… when that
/// is taken: an earlier backup is never overwritten) and says so on stderr. False when it could
/// not be moved. The board (`core/board.rs`) keeps its file the same way.
pub(crate) fn set_aside(file: &Path, reason: &str) -> bool {
    let stamp = now_ms();

    for attempt in 0..100 {
        let suffix = if attempt == 0 { String::new() } else { format!("-{attempt}") };
        let backup = PathBuf::from(format!("{}.broken-{stamp}{suffix}", file.display()));

        // A hard link fails rather than replace an existing file; then the original goes.
        let kept = match fs::hard_link(file, &backup) {
            Ok(()) => fs::remove_file(file).is_ok(),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => !backup.exists() && fs::rename(file, &backup).is_ok(),
        };

        if kept {
            eprintln!("[pitwall] {} is unusable ({reason}); kept as {}", file.display(), backup.display());
        } else {
            eprintln!("[pitwall] {} is unusable ({reason}) and could not be set aside", file.display());
        }
        return kept;
    }

    false
}

/// Settings out of JSON that doesn't fit as a whole: field by field, so a value of the wrong
/// type costs only itself. Projects, their commands and links, and the sounds are each kept or
/// trimmed on their own.
fn salvage(mut value: Value) -> Settings {
    if let Some(Value::Object(projects)) = value.get_mut("projects") {
        for project in projects.values_mut() {
            if let Value::Object(fields) = project {
                if let Some(Value::Array(commands)) = fields.get_mut("commands") {
                    for command in commands.iter_mut() {
                        *command = fitting::<CustomCommand>(command.take());
                    }
                }
                if let Some(Value::Array(links)) = fields.get_mut("links") {
                    for link in links.iter_mut() {
                        *link = fitting::<ProjectLink>(link.take());
                    }
                }
            }
            *project = fitting::<ProjectSettings>(project.take());
        }
    }
    if let Some(sounds) = value.get_mut("sounds") {
        *sounds = fitting::<Sounds>(sounds.take());
    }

    serde_json::from_value(fitting::<Settings>(value)).unwrap_or_default()
}

/// `value` with only the fields that fit `T`: each is tried on top of the defaults and dropped
/// (with a word on stderr) when it doesn't parse.
fn fitting<T: DeserializeOwned + Serialize + Default>(value: Value) -> Value {
    let mut kept = match serde_json::to_value(T::default()) {
        Ok(Value::Object(defaults)) => defaults,
        _ => Map::new(),
    };
    let Value::Object(fields) = value else {
        return Value::Object(kept);
    };

    for (name, field) in fields {
        let before = kept.insert(name.clone(), field);
        if serde_json::from_value::<T>(Value::Object(kept.clone())).is_err() {
            eprintln!("[pitwall] settings: dropped `{name}`, which doesn't parse");
            match before {
                Some(before) => kept.insert(name, before),
                None => kept.remove(&name),
            };
        }
    }

    Value::Object(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_settings_file_is_kept_and_what_parses_survives() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let backups = || {
            let mut found: Vec<String> = fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("settings.json.broken-"))
                .collect();
            found.sort();
            found
        };

        // Not JSON at all: nothing to recover, but the file is kept and no save touches it.
        fs::write(&file, "{\"projects\": {\"/a\": {\"script\": \"dev\"").unwrap();
        assert_eq!(Settings::load(&file), Settings::default());
        Settings::default().save(&file);

        // JSON with one value of the wrong type: the rest stays.
        let mixed = r#"{"script":"serve","notify":"yes","projects":{"/a":{"port":"x","url":"https://a.test","commands":[{"id":"w","name":"Worker","command":"php artisan queue:work","keepRunning":"yes"}],"links":[{"name":"Staging","url":"https://s.a.test"}]}}}"#;
        fs::write(&file, mixed).unwrap();
        let loaded = Settings::load(&file);

        assert_eq!(loaded.script, "serve");
        assert!(loaded.notify);
        let project = loaded.project("/a");
        assert_eq!(project.port, None);
        assert_eq!(project.url.as_deref(), Some("https://a.test"));
        assert_eq!(project.commands.len(), 1);
        assert_eq!(project.commands[0].command, "php artisan queue:work");
        assert_eq!(project.links.len(), 1);
        assert_eq!(Settings::load(&file), loaded, "what was recovered is written back");

        // Two backups, neither overwritten, each with what it had.
        let found = backups();
        assert_eq!(found.len(), 2, "{found:?}");
        let contents: Vec<String> = found.iter().map(|name| fs::read_to_string(dir.path().join(name)).unwrap()).collect();
        assert!(contents.iter().any(|c| c.starts_with("{\"projects\"")));
        assert!(contents.iter().any(|c| c == mixed));
    }
}
