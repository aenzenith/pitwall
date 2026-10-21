//! App settings, kept in the app's config folder (not shared with the extension).

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::registry::{now_ms, write_atomic};
use crate::start::{Mode, Model, Start};

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
    /// The model a Claude session started here takes (a board card given, "New Claude"); none:
    /// the one Claude Code picks itself. One of a fixed list, whatever the file says (`start.rs`).
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "crate::start::model_or_none")]
    pub claude_model: Option<Model>,
    /// The mode such a session starts in.
    #[serde(skip_serializing_if = "Mode::is_normal", deserialize_with = "crate::start::mode_or_normal")]
    pub claude_mode: Mode,
    /// The three letters the Track page and the mini tower write for the project, in place of
    /// those told from its name (`codes` in `src/lib/track.ts`).
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "code_or_none")]
    pub code: Option<String>,
    /// What a double click (or Return) on the project's row in the list opens: `editor`, or,
    /// left out, its Files window.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "editor_or_none")]
    pub double_click: Option<String>,
}

/// A project's code as a file has it: its letters and digits in capitals, three at most; none
/// when nothing is left.
fn code_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let code: String = value.as_str().unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_uppercase).take(3).collect();
    Ok((!code.is_empty()).then_some(code))
}

/// A row's double click as a file has it: `editor`, or none for anything else (the Files window).
fn editor_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok((value.as_str() == Some("editor")).then(|| "editor".to_string()))
}

impl ProjectSettings {
    /// How a Claude session of the project starts when nothing else was chosen.
    pub fn start(&self) -> Start {
        Start { model: self.claude_model, mode: self.claude_mode }
    }
}

/// A team: projects listed together, started and stopped together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Team {
    pub id: String,
    pub name: String,
    /// Its projects' paths, in the order they are listed and started. A project is in one team
    /// at most.
    pub projects: Vec<String>,
    /// Folded in the list: its head alone.
    pub collapsed: bool,
    /// Its servers start when Pitwall does.
    pub start_at_launch: bool,
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

/// The quiet hours: every day from `from` to `to`, local time as `HH:MM`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuietHours {
    pub on: bool,
    pub from: String,
    pub to: String,
    /// Switched off by hand while they were under way: they don't count before this moment (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_until: Option<u64>,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self { on: false, from: "22:00".into(), to: "08:00".into(), off_until: None }
    }
}

/// A quiet period the user started. Without `until` (ms) it lasts until it is switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct QuietPeriod {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
}

/// What Pitwall tells the user about, and when it holds its tongue (`core/signals.rs`). Claude
/// waiting has its own switch (`Settings::notify`), and so has the fuel alert
/// (`Settings::fuel_alert`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notifications {
    /// A notification when a dev server crashed.
    pub server_crashed: bool,
    /// … when a command ended, well or not.
    pub command: bool,
    /// … when Claude has been in one turn for 20 minutes.
    pub long_turn: bool,
    /// … when the Garage page finds something to install, or a migration that waits.
    pub deps: bool,
    /// Projects nothing is told about, by path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub muted: Vec<String>,
    pub quiet_hours: QuietHours,
    /// While quiet, Claude's questions and permission prompts still come through.
    pub questions_through: bool,
    /// While quiet, the sounds are held too.
    pub sounds_too: bool,
    /// The quiet period under way, or the last one: it survives a restart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet: Option<QuietPeriod>,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            server_crashed: true,
            command: false,
            long_turn: true,
            deps: false,
            muted: Vec::new(),
            quiet_hours: QuietHours::default(),
            questions_through: true,
            sounds_too: true,
            quiet: None,
        }
    }
}

/// The team radio (`core/radio`): what a Claude Code session may ask Pitwall, and do through it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioSettings {
    /// Its endpoint is open, and Pitwall is one of Claude Code's MCP servers.
    pub on: bool,
    /// Each tool's switch, by the tool's name (`mcp::TOOLS`). One not named here is off.
    pub tools: BTreeMap<String, bool>,
}

impl Default for RadioSettings {
    fn default() -> Self {
        Self { on: false, tools: crate::mcp::TOOLS.iter().map(|tool| (tool.name.to_string(), tool.default_on)).collect() }
    }
}

impl RadioSettings {
    pub fn allows(&self, tool: &str) -> bool {
        self.tools.get(tool).copied().unwrap_or(false)
    }
}

/// The chequered flag (`core/flag.rs`): what is still open, shown before a quit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FlagSettings {
    /// A quit from one of Pitwall's own menus shows what is still open first.
    pub at_quit: bool,
}

impl Default for FlagSettings {
    fn default() -> Self {
        Self { at_quit: true }
    }
}

/// The mini tower (`tower.rs`): a small window over every other, the projects that run or wait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TowerSettings {
    /// It is shown.
    pub on: bool,
    /// `list` (a row per project) or `strip` (one row of codes).
    pub form: String,
    /// Projects that neither run nor wait on you are left out.
    pub hide_stopped: bool,
    /// It shows on every desktop (Space).
    pub all_spaces: bool,
    /// A global shortcut of its own shows and hides it.
    pub shortcut: bool,
    /// That shortcut's keys, in global-hotkey form.
    pub shortcut_keys: String,
    /// Where it was left and how large it was there: left, top, width, height, the top counted
    /// downwards from the main screen's (`tower.rs`: points on macOS, pixels elsewhere). None
    /// until it was shown once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<[i32; 4]>,
}

impl Default for TowerSettings {
    fn default() -> Self {
        Self { on: false, form: "list".into(), hide_stopped: true, all_spaces: true, shortcut: false, shortcut_keys: "Ctrl+Alt+KeyK".into(), place: None }
    }
}

/// The command line (`cli.rs`, `core/cli`): the `pitwall` command and `pitwall://` links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CliSettings {
    /// The command line's line is open (`core/cli`): the `pitwall` command is installed. A dev
    /// build installs nothing, and has its line only when this is set by hand.
    pub on: bool,
    /// `pitwall://` links are answered.
    pub links: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for CliSettings {
    fn default() -> Self {
        Self { on: false, links: false }
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
    /// The session's fuel left beside the icon in the menu bar (`tray.rs`).
    pub fuel_in_bar: bool,
    /// The dials in the popover, in its foot.
    pub fuel_in_popover: bool,
    /// Minutes between two requests for Claude's limits: 1, 5 or 15 (`fuel_every_ms`).
    pub fuel_every: u32,
    /// The count of running dev servers beside the icon in the menu bar, in green (`tray.rs`).
    pub servers_in_bar: bool,
    /// The count of Claude sessions at work beside it, in orange.
    pub claudes_in_bar: bool,
    /// Asks for new versions of Pitwall by itself and downloads them (`update.rs`). Off, only
    /// "Check now" in Settings asks.
    pub auto_update: bool,
    /// The board the Board page showed last: `all`, or a project's path. None until one is picked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_scope: Option<String>,
    /// How the Board page draws a project's board: `columns` or `list`. None until one is picked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_view: Option<String>,
    /// The Files window shows a Markdown or SVG file as its preview rather than its text, from
    /// the moment its eye is pressed until it is pressed again.
    pub files_preview: bool,
    /// The Track page's own settings. None until one is changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSettings>,
    /// How the app is drawn: `system` (as the system is), `dark` or `light`.
    pub theme: String,
    /// The list's teams, in the order they are drawn (`core/teams.rs`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<Team>,
    /// What was open at the last quit, at the next start (`core/resume.rs`): `ask`, `always` or
    /// `never`.
    pub resume: String,
    /// The newest version whose notes were shown (`whatsnew.rs`). None until the first start
    /// that knows of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen_version: Option<String>,
    /// The first run's steps were finished or put off: they don't come up by themselves again.
    pub onboarded: bool,
    /// A project that appears in the projects folder is offered under the list
    /// (`core/discover.rs`).
    pub suggest_projects: bool,
    /// The projects folder's subfolders known so far, by name: none of them is offered.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub seen_folders: Vec<String>,
    /// What is told, the quiet hours and the muted projects (`core/signals.rs`).
    #[serde(default)]
    pub notifications: Notifications,
    /// The team radio: whether it is on, and what Claude may do over it.
    #[serde(default)]
    pub radio: RadioSettings,
    /// The chequered flag; changed through its own command only.
    #[serde(default)]
    pub flag: FlagSettings,
    /// The mini tower; changed through its own commands only.
    #[serde(default)]
    pub tower: TowerSettings,
    /// The command line and `pitwall://` links; changed through their own commands only.
    #[serde(default)]
    pub cli: CliSettings,
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
            fuel_in_bar: true,
            fuel_in_popover: true,
            fuel_every: 15,
            servers_in_bar: true,
            claudes_in_bar: true,
            auto_update: true,
            board_scope: None,
            board_view: None,
            files_preview: false,
            track: None,
            theme: "system".into(),
            teams: Vec::new(),
            resume: "ask".into(),
            seen_version: None,
            onboarded: false,
            suggest_projects: false,
            seen_folders: Vec::new(),
            notifications: Notifications::default(),
            radio: RadioSettings::default(),
            flag: FlagSettings::default(),
            tower: TowerSettings::default(),
            cli: CliSettings::default(),
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

    /// How long Claude's limits rest between two background requests, in ms. A value that isn't
    /// one of the three Settings offers counts as the longest: the endpoint punishes polling.
    pub fn fuel_every_ms(&self) -> i64 {
        match self.fuel_every {
            1 => 60_000,
            5 => 5 * 60_000,
            _ => 15 * 60_000,
        }
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
