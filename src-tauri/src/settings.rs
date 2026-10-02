//! App settings, kept in the app's config folder (not shared with the extension).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::registry::write_atomic;

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
            launch_at_login: false,
            shortcut: true,
            shortcut_keys: "Ctrl+Alt+KeyP".into(),
            projects: BTreeMap::new(),
            order: Vec::new(),
            projects_dir: None,
        }
    }
}

impl Settings {
    pub fn load(file: &Path) -> Self {
        fs::read_to_string(file).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
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
