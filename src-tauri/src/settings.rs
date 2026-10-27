//! App settings, kept in the app's config folder (not shared with the extension).

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::registry::{now_ms, same_path, write_atomic};
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
    /// The folder inside the project its dev server and commands run in, and whose files are
    /// read for them (`ProjectSettings::work_dir`): written from the project's folder, its
    /// parts joined by `/` (`project`, `apps/web`); none: the project's own folder. Whatever a
    /// file says that isn't such a path (absolute, with `..`) is none.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "folder_or_none")]
    pub folder: Option<String>,
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
    /// Where the project opens from each place, in place of Pitwall's own choice
    /// (`Settings::open`); a place left out takes Pitwall's.
    #[serde(skip_serializing_if = "ProjectOpen::is_empty")]
    pub open: ProjectOpen,
    /// What a double click on its row opened before `open` (`editor`, or none for its Files
    /// window): read from an older file into `open` (`ProjectSettings::upgraded`), never written.
    #[serde(skip_serializing, deserialize_with = "editor_or_none")]
    pub double_click: Option<String>,
}

/// A project's own choice of where it opens from each place (`OpenSettings`'s places and
/// values); none: Pitwall's. The window reads them leniently (`src/lib/open.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectOpen {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside: Option<String>,
}

impl ProjectOpen {
    pub fn is_empty(&self) -> bool {
        self.list.is_none() && self.pages.is_none() && self.outside.is_none()
    }

    /// Each place this one has no choice for takes `other`'s.
    pub fn or(self, other: Self) -> Self {
        Self { list: self.list.or(other.list), pages: self.pages.or(other.pages), outside: self.outside.or(other.outside) }
    }
}

/// A project's code as a file has it: its letters and digits in capitals, three at most; none
/// when nothing is left.
fn code_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let code: String = value.as_str().unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_uppercase).take(3).collect();
    Ok((!code.is_empty()).then_some(code))
}

/// A project's folder as a file has it: a path under the project's folder, its parts joined by
/// `/`; none for one that is empty, absolute or climbs out with `..`.
fn folder_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(value.as_str().and_then(plain_folder))
}

fn plain_folder(text: &str) -> Option<String> {
    let text = text.trim();
    if text.starts_with(['/', '\\']) || text.contains('\0') {
        return None;
    }
    let parts: Vec<&str> = text.split(['/', '\\']).filter(|part| !part.is_empty() && *part != ".").collect();
    // `..`, or a drive (`C:`) or a stream of Windows'.
    if parts.iter().any(|part| *part == ".." || part.contains(':')) {
        return None;
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The folder `picked` (absolute, or written from the project's folder) as `ProjectSettings::
/// folder` keeps it: from the project's folder, once every link and `..` is followed; `Ok(None)`
/// for the project's own folder. Refused when it isn't a folder there or ends outside the
/// project's.
pub fn folder_within(root: &Path, picked: &str) -> Result<Option<String>, FolderRefused> {
    let picked = picked.trim();
    if picked.is_empty() {
        return Ok(None);
    }
    let real_root = fs::canonicalize(root).map_err(|_| FolderRefused::Missing)?;
    let real = fs::canonicalize(root.join(picked)).map_err(|_| FolderRefused::Missing)?;
    let within = real.strip_prefix(&real_root).map_err(|_| FolderRefused::Outside)?;
    if !real.is_dir() {
        return Err(FolderRefused::Missing);
    }
    let parts: Vec<String> = within.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    Ok(plain_folder(&parts.join("/")))
}

/// Why a folder can't be a project's own (`folder_within`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRefused {
    Missing,
    Outside,
}

/// A row's double click as an older file has it: `editor`, or none for anything else (the Files
/// window).
fn editor_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok((value.as_str() == Some("editor")).then(|| "editor".to_string()))
}

impl ProjectSettings {
    /// How a Claude session of the project starts when nothing else was chosen.
    pub fn start(&self) -> Start {
        Start { model: self.claude_model, mode: self.claude_mode }
    }

    /// Where the project at `path` runs its dev server and commands, and whose files are read
    /// for them (`package.json`, its lock file, the Vite config, `.env`, what the Dependencies
    /// page checks): its `folder`, else its own. None when that folder isn't there or, with its
    /// links followed, ends outside the project's. It asks the file system: never under the
    /// core's lock.
    pub fn work_dir(&self, path: &str) -> Option<PathBuf> {
        let root = Path::new(path);
        let Some(folder) = self.folder.as_deref() else {
            return Some(root.to_path_buf());
        };
        let dir = root.join(folder);
        let real = fs::canonicalize(&dir).ok()?;
        (real.is_dir() && real.starts_with(fs::canonicalize(root).ok()?)).then_some(dir)
    }

    /// An older file's double click as the places it reached then: the list's row and the Track
    /// page's tower; a place already chosen keeps its choice.
    pub fn upgraded(mut self) -> Self {
        if let Some(target) = self.double_click.take() {
            self.open = ProjectOpen { list: self.open.list.or(Some(target.clone())), pages: self.open.pages.or(Some(target)), ..self.open };
        }
        self
    }
}

/// Where a project opens from each place (Settings › Projects): `list`, a double click or Return
/// on its row in the list (`terminal`, its terminal in the terminal panel, the keyboard in it;
/// `files`, its Files window, or `editor`); `pages`, a double click on it on the window's other
/// pages (the Track page, the Today page), and `outside`, a click in the menu bar's popover or the
/// tray's menu (`tray.rs`), Return in the quick switcher or a double click in the mini tower
/// (`terminal`, `list`, the project in the list, `editor` or `files`). `terminal` is every place's
/// default. A project may choose its own (`ProjectOpen`). The window and the tray's menu read them
/// leniently (`src/lib/open.ts`, `Settings::open_outside`): a value not known is the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenSettings {
    pub list: String,
    pub pages: String,
    pub outside: String,
}

impl Default for OpenSettings {
    fn default() -> Self {
        Self { list: "terminal".into(), pages: "terminal".into(), outside: "terminal".into() }
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
            server_ready: "lightsout".into(),
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

/// Claude's accounts (`accounts/`): the main one, Claude Code's own folder, always there, and the
/// others the user added, each a folder of its own (`CLAUDE_CONFIG_DIR`). Changed through their
/// own commands only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountSettings {
    /// The name the user gave the main account; none: the catalog's (`accounts.main`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_name: Option<String>,
    /// The accounts added, in the order they were added.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<ClaudeAccount>,
    /// The account the Claude sessions Pitwall starts use, by id; none: the main one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_use: Option<String>,
}

/// An account added beside the main one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ClaudeAccount {
    /// Lower-case hex, made when it was added; never `main`.
    pub id: String,
    pub name: String,
    /// Its folder, absolute, exactly as Claude Code is given it: the name of the Keychain item
    /// that holds its sign-in is made from these very characters (`accounts::keychain_service`).
    pub dir: String,
    /// Its folder links to the main account's settings (`accounts::SHARED`).
    pub shared: bool,
    /// Its MCP servers are brought level with the main account's when it is switched to.
    pub sync: bool,
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
    /// The projects added to Pitwall, starred or not: taking a star off leaves the project here,
    /// "Remove from Pitwall" takes it out (`core/projects.rs`). None until it is first made from
    /// what the settings knew of projects (`known_projects`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<Vec<String>>,
    /// The folder whose subfolders the quick switcher searches when no project matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects_dir: Option<String>,
    /// A notification when Claude's session or weekly limit passes 90 %.
    pub fuel_alert: bool,
    /// The session's fuel left beside the icon in the menu bar (`tray.rs`).
    pub fuel_in_bar: bool,
    /// The dials in the popover, in its foot.
    pub fuel_in_popover: bool,
    /// Which projects the menu bar's window lists (Settings › Projects): `all`, or `working`, the
    /// favourites and the projects at work (anything else reads as `working`).
    pub bar_projects: String,
    /// Which projects the terminal panel's picker lists (Settings › Projects): `all`, or `open`,
    /// the favourites and the projects open or at work, and the panel's own (anything else reads
    /// as `open`).
    pub terminal_projects: String,
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
    /// The list shows the favourites and the projects at work only ("Only favourites and open").
    pub favourites_only: bool,
    /// A press on the page folds the open terminal panel to its tab row (Settings › General).
    pub panel_fold_outside: bool,
    /// The terminal panel's height while it shows panes (`src/lib/panes.ts`), as it was last
    /// dragged or grown to; none until it first changes. This Mac's own: it goes with its screens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panes_height: Option<u32>,
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
    /// Where a project opens from each place, unless it chose its own (Settings › Projects).
    #[serde(default)]
    pub open: OpenSettings,
    /// Claude's accounts; changed through their own commands only.
    #[serde(default)]
    pub accounts: AccountSettings,
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
            shortcut_keys: if cfg!(target_os = "macos") { "Alt+Shift+KeyP" } else { "Ctrl+Alt+KeyP" }.into(),
            projects: BTreeMap::new(),
            order: Vec::new(),
            added: None,
            projects_dir: None,
            fuel_alert: true,
            fuel_in_bar: true,
            fuel_in_popover: true,
            bar_projects: "working".into(),
            terminal_projects: "open".into(),
            fuel_every: 15,
            servers_in_bar: true,
            claudes_in_bar: true,
            auto_update: true,
            board_scope: None,
            board_view: None,
            files_preview: false,
            favourites_only: false,
            panel_fold_outside: true,
            panes_height: None,
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
            open: OpenSettings::default(),
            accounts: AccountSettings::default(),
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
            Ok(settings) => settings.upgraded(),
            Err(error) => {
                set_aside(file, &error.to_string());
                let settings = salvage(value).upgraded();
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

    /// What an older file says in words this version no longer writes, in those it does
    /// (`ProjectSettings::upgraded`).
    pub fn upgraded(mut self) -> Self {
        self.projects = self.projects.into_iter().map(|(path, project)| (path, project.upgraded())).collect();
        self
    }

    pub fn project(&self, path: &str) -> ProjectSettings {
        self.projects.get(path).cloned().unwrap_or_default()
    }

    /// The projects these settings knew of before Pitwall kept a list of its own (`added`): those
    /// in the order, in a team or with settings of their own, whose folder is there; the projects
    /// folder itself is none.
    pub fn known_projects(&self) -> Vec<String> {
        let mut known: Vec<String> = Vec::new();
        for path in self.order.iter().chain(self.teams.iter().flat_map(|team| &team.projects)).chain(self.projects.keys()) {
            let projects_dir = self.projects_dir.as_deref().is_some_and(|dir| same_path(dir, path));
            if !projects_dir && Path::new(path).is_dir() && !known.iter().any(|seen| same_path(seen, path)) {
                known.push(path.clone());
            }
        }
        known
    }

    /// Where the project at `path` opens from outside the window (`OpenSettings::outside`): its
    /// own choice, else Pitwall's, a value not known being the default, as the window reads it.
    pub fn open_outside(&self, path: &str) -> &'static str {
        let known = |value: &str| ["terminal", "list", "editor", "files"].into_iter().find(|target| *target == value);
        self.project(path).open.outside.as_deref().and_then(known).or_else(|| known(&self.open.outside)).unwrap_or("terminal")
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
    use serde_json::json;

    use super::*;

    #[test]
    fn an_older_double_click_still_opens_the_places_it_did() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let older = json!({ "projects": {
            "/a": { "doubleClick": "editor" },
            "/b": { "doubleClick": "editor", "open": { "list": "files" } },
            "/c": { "doubleClick": "files", "script": "serve" },
        } });
        fs::write(&file, older.to_string()).unwrap();

        let settings = Settings::load(&file);
        let open = |path: &str| settings.project(path).open;
        let place = |value: &str| Some(value.to_string());
        assert_eq!(open("/a"), ProjectOpen { list: place("editor"), pages: place("editor"), outside: None });
        assert_eq!(open("/b"), ProjectOpen { list: place("files"), pages: place("editor"), outside: None }, "a place chosen keeps its choice");
        assert!(open("/c").is_empty(), "the Files window was no choice of its own");

        settings.save(&file);
        assert!(!fs::read_to_string(&file).unwrap().contains("doubleClick"));
        assert_eq!(Settings::load(&file), settings);
    }

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

    /// The folder a project runs in never leads out of it: not as the settings file writes it,
    /// not as it is typed or picked, not through a link inside the project.
    #[cfg(unix)]
    #[test]
    fn the_folder_a_project_runs_in_stays_inside_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("hotel");
        fs::create_dir_all(root.join("project/public")).unwrap();
        fs::create_dir_all(tmp.path().join("hotel-admin")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("hotel-admin"), root.join("out")).unwrap();
        std::os::unix::fs::symlink(root.join("project"), root.join("app")).unwrap();
        let path = root.to_string_lossy().into_owned();

        // As the file has it.
        let file = tmp.path().join("settings.json");
        let folders = ["project", "./project/", "/etc", "../hotel-admin", "project/../../hotel-admin", "C:/Windows", ""];
        let projects: Map<String, Value> = folders.iter().enumerate().map(|(i, folder)| (format!("/p{i}"), serde_json::json!({ "folder": folder }))).collect();
        fs::write(&file, serde_json::json!({ "projects": projects }).to_string()).unwrap();
        let read: Vec<Option<String>> = (0..folders.len()).map(|i| Settings::load(&file).project(&format!("/p{i}")).folder).collect();
        assert_eq!(read, [Some("project".into()), Some("project".into()), None, None, None, None, None]);

        // As it is typed or picked.
        let real = |relative: &str| fs::canonicalize(&root).unwrap().join(relative).to_string_lossy().into_owned();
        assert_eq!(folder_within(&root, "project"), Ok(Some("project".into())));
        assert_eq!(folder_within(&root, &real("project/public")), Ok(Some("project/public".into())), "absolute, inside");
        assert_eq!(folder_within(&root, "app"), Ok(Some("project".into())), "a link inside, as where it leads");
        assert_eq!(folder_within(&root, &path), Ok(None), "the project's own");
        assert_eq!(folder_within(&root, "../hotel-admin"), Err(FolderRefused::Outside));
        assert_eq!(folder_within(&root, &tmp.path().join("hotel-admin").to_string_lossy()), Err(FolderRefused::Outside), "a neighbour whose name begins as the project's");
        assert_eq!(folder_within(&root, "out"), Err(FolderRefused::Outside), "a link that leads out");
        assert_eq!(folder_within(&root, "nowhere"), Err(FolderRefused::Missing));

        // Where it runs.
        let at = |folder: &str| ProjectSettings { folder: Some(folder.into()), ..Default::default() }.work_dir(&path);
        assert_eq!(at("project"), Some(root.join("project")));
        assert_eq!(ProjectSettings::default().work_dir(&path), Some(root.clone()));
        assert_eq!(at("out"), None, "a link made after it was set, leading out");
        assert_eq!(at("nowhere"), None);
    }
}
