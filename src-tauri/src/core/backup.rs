//! Backup and import of what Pitwall keeps, and the projects whose folder moved.
//!
//! A backup is one JSON file, written where the user says: the general preferences, the
//! projects (the list, its order, each project's settings, the teams), the board (every card
//! and its images) and the timeline's days, each part only when chosen. It holds what the app's
//! own files hold and nothing else.
//!
//! An import puts the chosen parts of such a file back. The file may come from anywhere, so it
//! is read and checked whole before anything changes (`backup_checked` says what is refused and
//! what is skipped), and nothing in it names a place to write: a card's images go where the
//! board puts them, a day where the timeline does. What is here now is copied to
//! `backups/before-import-<ms>/` beside the settings first, and an import adds to what is here:
//! only a card, a project's settings or a team the file names again is replaced, and a day that
//! has a file is never written over.
//!
//! A backup of another machine names that machine's home folder; its paths under that folder
//! are put under this one's.
//!
//! A project whose folder is gone (moved, renamed, on a disk that isn't there) keeps its
//! settings, its place in the list, its team and its cards under the old path:
//! `missing_projects` lists those, `rehome_project` hands what is kept to the folder the
//! project is in now, and `forget_project` drops it. Both copy what is here aside first, as an
//! import does (`backups/before-move-<ms>/`, `backups/before-forget-<ms>/`).

use std::collections::BTreeMap;
use std::io::{ErrorKind, Write as _};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::board::{taken, CardImage, Column, ImageKind, IMAGES_MAX};
use super::reveal::is_session_id;
use super::*;
use crate::registry::{base36, write_atomic};
use crate::settings::{CustomCommand, FlagSettings, OpenSettings, Sounds, Team};

const FORMAT: &str = "pitwall-backup";
const VERSION: u64 = 1;
/// A backup is this large at most, read or written: it is held in memory whole. One that would
/// be larger isn't written, so no backup is made that can't be read back.
const FILE_BYTES_MAX: u64 = 512 * 1024 * 1024;
/// What an import, a move or a Forget may change is copied into a folder of its own under this
/// one, beside the settings.
const KEPT_DIR: &str = "backups";

/// What a backup takes, or an import puts back.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct BackupParts {
    pub settings: bool,
    pub projects: bool,
    pub board: bool,
    pub activity: bool,
}

/// What a backup file holds, as far as an import takes it. A part the file doesn't have is 0 or
/// false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file: String,
    pub created_at: u64,
    /// The version that wrote it.
    pub app: String,
    pub settings: bool,
    /// The listed projects.
    pub projects: usize,
    /// The commands of all its projects, and their links.
    pub commands: usize,
    pub links: usize,
    pub teams: usize,
    pub cards: usize,
    pub images: usize,
    /// Days of the timeline.
    pub days: usize,
}

/// What an import put back: the projects it listed, the cards it wrote, the days it added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub projects: usize,
    pub cards: usize,
    pub days: usize,
    pub settings: bool,
}

/// A project whose folder is gone, and what is still kept for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingProject {
    pub path: String,
    /// The folder's name.
    pub name: String,
    pub commands: usize,
    pub links: usize,
    /// The board's cards that carry its path.
    pub cards: usize,
    /// It is among the listed projects.
    pub listed: bool,
}

/// The general preferences: the settings part of a backup. One that isn't set is `null` in the
/// file, apart from one the file doesn't name at all.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct General {
    language: String,
    script: String,
    package_manager: String,
    editor: String,
    open_url_on_start: bool,
    sounds: Sounds,
    shortcut: bool,
    shortcut_keys: String,
    projects_dir: Option<String>,
    fuel_in_bar: bool,
    fuel_in_popover: bool,
    fuel_every: u32,
    servers_in_bar: bool,
    claudes_in_bar: bool,
    auto_update: bool,
    board_view: Option<String>,
    files_preview: bool,
    favourites_only: bool,
    track: Option<TrackSettings>,
    theme: String,
    resume: String,
    suggest_projects: bool,
    flag: FlagSettings,
    open: OpenSettings,
}

impl General {
    fn of(settings: &Settings) -> Self {
        // Every setting is named here, so a new one doesn't compile until it is put into
        // backups or kept out of them on purpose.
        let Settings {
            language, script, package_manager, editor, open_url_on_start, sounds, shortcut, shortcut_keys, projects_dir,
            fuel_in_bar, fuel_in_popover, fuel_every, servers_in_bar, claudes_in_bar, auto_update, board_view, files_preview, favourites_only, track,
            theme, resume, suggest_projects, flag, open,
            // This Mac's own: its registration with the system, and what it has shown so far.
            launch_at_login: _, seen_version: _, onboarded: _, seen_folders: _, board_scope: _,
            // The projects part.
            projects: _, order: _, added: _, teams: _,
            // Kept out: what is told and when is this Mac's own, every event's switch with the
            // quiet period under way and the muted projects' paths (Settings › Notifications:
            // Claude waiting and the fuel alert are two of its switches).
            notifications: _, notify: _, fuel_alert: _,
            // This Mac's own too: Claude Code's configuration here is what names this app.
            radio: _,
            // This Mac's own as well: where its small window stands, the command installed here.
            tower: _, cli: _,
        } = settings.clone();

        Self {
            language, script, package_manager, editor, open_url_on_start, sounds, shortcut, shortcut_keys, projects_dir,
            fuel_in_bar, fuel_in_popover, fuel_every, servers_in_bar, claudes_in_bar, auto_update, board_view, files_preview, favourites_only, track,
            theme, resume, suggest_projects, flag, open,
        }
    }

    fn fields(&self) -> Map<String, Value> {
        match serde_json::to_value(self) {
            Ok(Value::Object(fields)) => fields,
            _ => Map::new(),
        }
    }

    /// `settings` with the general preferences a backup holds. A preference the file doesn't
    /// name (a backup of an older version) stays as it is, and nothing but these preferences is
    /// read from the file: whatever else it says, the rest of `settings` is left alone.
    fn onto(settings: &Settings, file: &Map<String, Value>) -> Result<Settings, serde_json::Error> {
        let mut fields = Self::of(settings).fields();
        for (name, value) in fields.iter_mut() {
            if let Some(theirs) = file.get(name) {
                value.clone_from(theirs);
            }
        }

        let Self {
            language, script, package_manager, editor, open_url_on_start, sounds, shortcut, shortcut_keys, projects_dir,
            fuel_in_bar, fuel_in_popover, fuel_every, servers_in_bar, claudes_in_bar, auto_update, board_view, files_preview, favourites_only, track,
            theme, resume, suggest_projects, flag, open,
        } = serde_json::from_value(Value::Object(fields))?;

        Ok(Settings {
            language, script, package_manager, editor, open_url_on_start, sounds, shortcut, shortcut_keys, projects_dir,
            fuel_in_bar, fuel_in_popover, fuel_every, servers_in_bar, claudes_in_bar, auto_update, board_view, files_preview, favourites_only, track,
            theme, resume, suggest_projects, flag, open,
            ..settings.clone()
        })
    }
}

/// The projects part: the listed projects (the favourites and the others added), the list's
/// order, each project's settings, the teams.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct ProjectsPart {
    favourites: Vec<Favourite>,
    /// A backup of an older version has none: its favourites are all it listed.
    added: Vec<String>,
    order: Vec<String>,
    settings: BTreeMap<String, ProjectSettings>,
    teams: Vec<Team>,
}

/// One of a card's images in a backup: the card's id, the image's number in its note, its kind
/// and its bytes in base64.
#[derive(Serialize, Deserialize)]
struct ImageEntry {
    card: String,
    n: u32,
    kind: ImageKind,
    data: String,
}

/// The board part: every card as the board's file has it, and their images.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct BoardPart {
    cards: Vec<Card>,
    images: Vec<ImageEntry>,
}

/// The file. A part is in it only when it was chosen.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Document {
    format: String,
    v: u64,
    created_at: u64,
    /// The version that wrote it.
    app: String,
    /// The home folder of the machine that wrote it.
    home: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projects: Option<ProjectsPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    board: Option<BoardPart>,
    /// Each day's file as it is, by the day's name (`YYYY-MM-DD`).
    #[serde(skip_serializing_if = "Option::is_none")]
    activity: Option<BTreeMap<String, String>>,
}

/// A card of a backup and the images it takes: each one's number and kind, and its bytes.
struct Taken {
    card: Card,
    images: Vec<(CardImage, Vec<u8>)>,
}

/// A backup read and checked: what an import takes from it.
struct Backup {
    created_at: u64,
    app: String,
    home: String,
    settings: Option<Map<String, Value>>,
    projects: Option<ProjectsPart>,
    cards: Option<Vec<Taken>>,
    days: Option<Vec<(String, String)>>,
}

impl Backup {
    fn info(&self, file: &Path) -> BackupInfo {
        let projects = self.projects.as_ref();
        let kept = || projects.into_iter().flat_map(|part| part.settings.values());

        BackupInfo {
            file: file.to_string_lossy().into_owned(),
            created_at: self.created_at,
            app: self.app.clone(),
            settings: self.settings.is_some(),
            projects: projects.map_or(0, |part| part.favourites.len()),
            commands: kept().map(|project| project.commands.len()).sum(),
            links: kept().map(|project| project.links.len()).sum(),
            teams: projects.map_or(0, |part| part.teams.len()),
            cards: self.cards.as_ref().map_or(0, Vec::len),
            images: self.cards.iter().flatten().map(|taken| taken.images.len()).sum(),
            days: self.days.as_ref().map_or(0, Vec::len),
        }
    }

    /// Puts the paths of the machine that wrote the backup where they are on this one, whose
    /// home folder is `home`: the listed projects, the order, the projects' settings, the
    /// teams' members, the cards, the projects folder and the projects the days' events name.
    fn move_home(&mut self, home: &str) {
        let ends = ['/', '\\'];
        let homes = Homes { from: self.home.trim_end_matches(ends).to_string(), to: home.trim_end_matches(ends).to_string() };
        if homes.from.is_empty() || homes.to.is_empty() || homes.from == homes.to {
            return;
        }

        if let Some(Value::String(dir)) = self.settings.as_mut().and_then(|general| general.get_mut("projectsDir")) {
            *dir = homes.here(dir);
        }
        if let Some(part) = &mut self.projects {
            for favourite in &mut part.favourites {
                favourite.path = homes.here(&favourite.path);
            }
            for path in part.added.iter_mut().chain(part.order.iter_mut()).chain(part.teams.iter_mut().flat_map(|team| team.projects.iter_mut())) {
                *path = homes.here(path);
            }
            part.settings = std::mem::take(&mut part.settings).into_iter().map(|(path, project)| (homes.here(&path), project)).collect();
        }
        for taken in self.cards.iter_mut().flatten() {
            taken.card.path = homes.here(&taken.card.path);
        }
        for (_, text) in self.days.iter_mut().flatten() {
            *text = homes.day(text);
        }
    }
}

/// The home folder of the machine a backup was written on, and this one's; neither ends in a
/// separator.
struct Homes {
    from: String,
    to: String,
}

impl Homes {
    /// `path` as it is here: under this home when it was under that one, else as it was. A
    /// folder beside the home whose name only starts like it (`/Users/me-old`) isn't under it.
    fn here(&self, path: &str) -> String {
        match path.strip_prefix(self.from.as_str()) {
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => format!("{}{rest}", self.to),
            _ => path.to_string(),
        }
    }

    /// A day's file with the project of each event where it is here. A line that names no
    /// project under the other home stays as it is, one that isn't an event too.
    fn day(&self, text: &str) -> String {
        let mut moved = String::with_capacity(text.len());

        for line in text.lines() {
            let event = serde_json::from_str::<Value>(line).ok().and_then(|mut event| {
                let path = event.get_mut("path")?;
                let here = self.here(path.as_str()?);
                if path.as_str() == Some(here.as_str()) {
                    return None;
                }
                *path = Value::String(here);
                Some(event.to_string())
            });
            moved.push_str(event.as_deref().unwrap_or(line));
            moved.push('\n');
        }

        moved
    }
}

/// This machine's home folder.
fn home() -> String {
    dirs::home_dir().map(|home| home.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `name` is a day as the timeline names its files: `YYYY-MM-DD`, of a date there is. Nothing
/// else ever becomes a file name there.
fn is_day(name: &str) -> bool {
    let shaped = name.len() == 10 && name.bytes().enumerate().all(|(at, byte)| if matches!(at, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() });
    shaped && chrono::NaiveDate::parse_from_str(name, "%Y-%m-%d").is_ok()
}

/// `paths` without the empty ones, each folder once.
fn unique(paths: Vec<String>) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for path in paths {
        if !path.is_empty() && !found.iter().any(|known| same_path(known, &path)) {
            found.push(path);
        }
    }
    found
}

/// The projects part of a backup put into `settings`. A project's settings in the file take the
/// place of those kept for the same folder, the others stay (an entry that holds nothing takes
/// no place). The order is the file's, then the paths here it doesn't name; the projects added
/// are those added here and the file's. A team in the file
/// takes the place of the team with its id, the others stay, and a project is in one team at
/// most: where the file puts it. Whether a team starts with Pitwall is never the file's to say:
/// one it brings or replaces doesn't, until that is switched on here.
fn put_projects(settings: &mut Settings, part: &ProjectsPart) {
    for (path, project) in &part.settings {
        let project = project.clone().upgraded();
        if !path.is_empty() && project != ProjectSettings::default() {
            settings.projects.insert(path.clone(), project);
        }
    }

    settings.order = unique(part.order.iter().chain(&settings.order).cloned().collect());
    let added = settings.added.take().unwrap_or_default();
    settings.added = Some(unique(added.into_iter().chain(part.added.iter().cloned()).collect()));

    for team in &part.teams {
        for other in settings.teams.iter_mut().filter(|other| other.id != team.id) {
            other.projects.retain(|path| !team.projects.iter().any(|theirs| same_path(theirs, path)));
        }
        let team = Team { start_at_launch: false, ..team.clone() };
        match settings.teams.iter_mut().find(|known| known.id == team.id) {
            Some(known) => *known = team,
            None => settings.teams.push(team),
        }
    }
}

/// The settings kept for the folder `path`, taken out.
fn take_project(settings: &mut Settings, path: &str) -> Option<ProjectSettings> {
    let key = settings.projects.keys().find(|known| same_path(known, path))?.clone();
    settings.projects.remove(&key)
}

/// Everything `settings` keeps for the folder `from` goes to the folder `to`. What `to` has of
/// its own stays (its script, port and address, its place in the order, its team) and what it
/// lacks comes from `from`. Commands and links are joined: each of `from`'s is added unless
/// `to` has the same one, so none is dropped.
fn move_project(settings: &mut Settings, from: &str, to: &str) {
    if let Some(theirs) = take_project(settings, from) {
        let mut ours = take_project(settings, to).unwrap_or_default();
        ours.folder = ours.folder.or(theirs.folder);
        ours.script = ours.script.or(theirs.script);
        ours.port = ours.port.or(theirs.port);
        ours.url = ours.url.or(theirs.url);
        ours.claude_model = ours.claude_model.or(theirs.claude_model);
        ours.code = ours.code.or(theirs.code);
        ours.open = ours.open.or(theirs.open);
        if ours.claude_mode.is_normal() {
            ours.claude_mode = theirs.claude_mode;
        }

        for mut command in theirs.commands {
            // The same command but for its id: `to` has it.
            if ours.commands.iter().any(|known| same_command(known, &command)) {
                continue;
            }
            // A project's commands are run and told apart by their ids: no two share one.
            let mut n = 0;
            while ours.commands.iter().any(|known| known.id == command.id) {
                command.id = format!("{}{}", base36(now_ms()), base36(n));
                n += 1;
            }
            ours.commands.push(command);
        }
        for link in theirs.links {
            if !ours.links.contains(&link) {
                ours.links.push(link);
            }
        }

        if ours != ProjectSettings::default() {
            settings.projects.insert(to.to_string(), ours);
        }
    }

    for list in std::iter::once(&mut settings.order).chain(settings.added.as_mut()) {
        if let Some(at) = list.iter().position(|path| same_path(path, from)) {
            if list.iter().any(|path| same_path(path, to)) {
                list.remove(at);
            } else {
                list[at] = to.to_string();
            }
        }
    }

    let teamed = settings.teams.iter().any(|team| team.projects.iter().any(|path| same_path(path, to)));
    for team in &mut settings.teams {
        if teamed {
            team.projects.retain(|path| !same_path(path, from));
        } else if let Some(path) = team.projects.iter_mut().find(|path| same_path(path, from)) {
            *path = to.to_string();
        }
    }

    // The Board page's last board: its cards are `to`'s now.
    if settings.board_scope.as_deref().is_some_and(|scope| same_path(scope, from)) {
        settings.board_scope = Some(to.to_string());
    }

    // Muted where it was, muted where it is.
    let muted = &mut settings.notifications.muted;
    if let Some(at) = muted.iter().position(|path| same_path(path, from)) {
        if muted.iter().any(|path| same_path(path, to)) {
            muted.remove(at);
        } else {
            muted[at] = to.to_string();
        }
    }
}

/// The two are one command: all of it the same but its id.
fn same_command(a: &CustomCommand, b: &CustomCommand) -> bool {
    CustomCommand { id: String::new(), ..a.clone() } == CustomCommand { id: String::new(), ..b.clone() }
}

impl Core {
    /// Writes a backup of the chosen parts to `file`.
    pub fn backup_write(&self, file: &Path, parts: BackupParts) -> Result<(), String> {
        self.backup_write_as(file, parts, &home())
    }

    /// `backup_write` on a machine whose home folder is `home`.
    fn backup_write_as(&self, file: &Path, parts: BackupParts, home: &str) -> Result<(), String> {
        let settings = self.settings();
        let document = Document {
            format: FORMAT.into(),
            v: VERSION,
            created_at: now_ms(),
            app: env!("CARGO_PKG_VERSION").into(),
            home: home.into(),
            settings: parts.settings.then(|| General::of(&settings).fields()),
            projects: parts.projects.then(|| ProjectsPart {
                favourites: self.registry.read_favourites(),
                added: settings.added.clone().unwrap_or_default(),
                order: settings.order.clone(),
                settings: settings.projects.clone(),
                teams: settings.teams.clone(),
            }),
            board: parts.board.then(|| self.backup_board()),
            activity: parts.activity.then(|| self.backup_days()),
        };

        let failed = |error: &dyn std::fmt::Display| t!("core.error.backupWrite", file = file.display(), error = error);
        let json = serde_json::to_string_pretty(&document).map_err(|error| failed(&error))?;
        if json.len() as u64 > FILE_BYTES_MAX {
            return Err(t!("core.error.backupWriteTooLarge", limit = FILE_BYTES_MAX / (1024 * 1024)));
        }
        write_atomic(file, &json).map_err(|error| failed(&error))
    }

    /// Every card as the board's file has it, and each image file that is there. The moment a
    /// card was told to wait for is this Mac's own, and stays out.
    fn backup_board(&self) -> BoardPart {
        let mut cards = self.board_state();
        for card in &mut cards {
            card.schedule = None;
        }
        let mut images: Vec<ImageEntry> = Vec::new();

        for card in &cards {
            let Some((_, files)) = self.card_images(card) else {
                continue;
            };
            for (n, file) in files {
                let kind = card.images.iter().find(|image| image.n == n).map(|image| image.kind);
                if let (Some(kind), Ok(bytes)) = (kind, fs::read(&file)) {
                    images.push(ImageEntry { card: card.id.clone(), n, kind, data: BASE64.encode(bytes) });
                }
            }
        }

        BoardPart { cards, images }
    }

    /// Every day the timeline has a file of, as it is.
    fn backup_days(&self) -> BTreeMap<String, String> {
        let Some(entries) = self.activity_dir().and_then(|dir| fs::read_dir(dir).ok()) else {
            return BTreeMap::new();
        };

        entries
            .flatten()
            .filter_map(|entry| {
                let day = entry.file_name().to_str()?.strip_suffix(".jsonl").filter(|day| is_day(day))?.to_string();
                Some((day, fs::read_to_string(entry.path()).ok()?))
            })
            .collect()
    }

    /// Reads `file` and says what it holds; an error when it isn't a backup this version reads.
    pub fn backup_read(&self, file: &Path) -> Result<BackupInfo, String> {
        Ok(self.backup_checked(file)?.info(file))
    }

    /// Reads a backup and checks all of it; nothing is changed here.
    ///
    /// Refused whole: a file over `FILE_BYTES_MAX`, one that isn't a backup (not JSON, another
    /// `format`), one of a version this one doesn't read, and one with a part that doesn't
    /// parse (a card without its fields, an image of a kind that isn't one of the board's four,
    /// a day that isn't text, a preference of the wrong type).
    ///
    /// Skipped, the rest taken: a card whose id isn't one the board gives (`Board::image_dir`:
    /// letters, digits and dashes, so it names no folder but its own) with its images, and an
    /// id's second card; an image of no card in the file, one its card's note doesn't name, a
    /// number's second image, bytes that aren't the image its kind says or are larger than the
    /// board takes (`taken`), and those past a card's `IMAGES_MAX`; a day whose name isn't a
    /// date (`is_day`); a listed project without a path, a team without an id.
    fn backup_checked(&self, file: &Path) -> Result<Backup, String> {
        let mut raw = Vec::new();
        fs::File::open(file)
            .and_then(|opened| opened.take(FILE_BYTES_MAX + 1).read_to_end(&mut raw))
            .map_err(|error| t!("core.error.backupRead", file = file.display(), error = error))?;
        if raw.len() as u64 > FILE_BYTES_MAX {
            return Err(t!("core.error.backupReadTooLarge", file = file.display(), limit = FILE_BYTES_MAX / (1024 * 1024)));
        }

        let not_backup = || t!("core.error.backupNotBackup", file = file.display());
        let damaged = |error: serde_json::Error| t!("core.error.backupDamaged", file = file.display(), error = error);

        let value: Value = serde_json::from_slice(&raw).map_err(|_| not_backup())?;
        drop(raw);
        if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(not_backup());
        }
        match value.get("v").and_then(Value::as_u64) {
            Some(VERSION) => {}
            Some(newer) if newer > VERSION => {
                let app = value.get("app").and_then(Value::as_str).unwrap_or("?");
                return Err(t!("core.error.backupNewer", file = file.display(), app = app));
            }
            _ => return Err(not_backup()),
        }
        let document: Document = serde_json::from_value(value).map_err(damaged)?;

        if let Some(general) = &document.settings {
            General::onto(&Settings::default(), general).map_err(damaged)?;
        }

        let projects = document.projects.map(|mut part| {
            part.favourites.retain(|favourite| !favourite.path.is_empty());
            for favourite in part.favourites.iter_mut().filter(|favourite| favourite.name.is_empty()) {
                favourite.name = folder_name(&favourite.path);
            }
            part.teams.retain(|team| !team.id.is_empty());
            for team in &mut part.teams {
                team.projects = unique(std::mem::take(&mut team.projects));
            }
            part
        });

        Ok(Backup {
            created_at: document.created_at,
            app: document.app,
            home: document.home,
            settings: document.settings,
            projects,
            cards: document.board.map(|part| self.backup_cards(part)),
            days: document.activity.map(|days| days.into_iter().filter(|(day, _)| is_day(day)).collect()),
        })
    }

    /// The cards an import takes from a backup's board, each with the images it takes (see
    /// `backup_checked`). A session that isn't a session id is dropped, as when the board's own
    /// file is read.
    fn backup_cards(&self, part: BoardPart) -> Vec<Taken> {
        let BoardPart { cards, images } = part;
        let own: Vec<bool> = {
            let board = self.board();
            cards.iter().map(|card| board.image_dir(&card.id).is_some()).collect()
        };

        let mut found: Vec<Taken> = Vec::new();
        for (mut card, own) in cards.into_iter().zip(own) {
            if !own || found.iter().any(|known| known.card.id == card.id) {
                continue;
            }

            let mut held: Vec<(CardImage, Vec<u8>)> = Vec::new();
            for entry in images.iter().filter(|entry| entry.card == card.id) {
                if held.len() == IMAGES_MAX {
                    break;
                }
                if held.iter().any(|(image, _)| image.n == entry.n) {
                    continue;
                }
                // The board's own rule, as for an image pasted into a note.
                let pasted = NewImage { n: entry.n, data: format!("data:;base64,{}", entry.data) };
                let image = taken(&card.note, &[pasted]).ok().and_then(|mut one| one.pop());
                held.extend(image.filter(|(image, _)| image.kind == entry.kind));
            }
            held.sort_by_key(|(image, _)| image.n);
            card.images = held.iter().map(|(image, _)| *image).collect();

            card.session = card.session.filter(|session| is_session_id(session));
            // A card told to wait was told so where the backup was made: here it arrives as any
            // other card up next, with the model and the mode it was last started with.
            card.schedule = None;
            found.push(Taken { card, images: held });
        }

        found
    }

    /// Puts the chosen parts of `file` back.
    pub fn backup_import(self: &Arc<Self>, file: &Path, parts: BackupParts) -> Result<ImportReport, String> {
        self.backup_import_as(file, parts, &home())
    }

    /// `backup_import` on a machine whose home folder is `home`. The file is checked whole
    /// (`backup_checked`) and what is here is copied aside (`keep_before`) before anything
    /// changes; either failing, nothing does.
    fn backup_import_as(self: &Arc<Self>, file: &Path, parts: BackupParts, home: &str) -> Result<ImportReport, String> {
        let mut backup = self.backup_checked(file)?;
        backup.move_home(home);

        let general = backup.settings.filter(|_| parts.settings);
        let projects = backup.projects.filter(|_| parts.projects);
        let cards = backup.cards.filter(|_| parts.board);
        let days = backup.days.filter(|_| parts.activity);
        let incoming: Vec<&str> = cards.iter().flatten().map(|taken| taken.card.id.as_str()).collect();
        self.keep_before("import", &incoming).map_err(|error| t!("core.error.backupKeep", error = error))?;

        let mut report = ImportReport::default();

        if general.is_some() || projects.is_some() {
            let (language, unnamed) = {
                let mut inner = self.lock();
                let named = PackageManager::parse(&inner.settings.package_manager).is_some();
                // Checked when the file was read: it fits.
                if let Some(Ok(next)) = general.as_ref().map(|general| General::onto(&inner.settings, general)) {
                    inner.settings = next;
                    report.settings = true;
                }
                if let Some(part) = &projects {
                    put_projects(&mut inner.settings, part);
                }
                (inner.settings.language.clone(), named && PackageManager::parse(&inner.settings.package_manager).is_none())
            };
            self.save_settings();
            i18n::set(i18n::resolve(&language));
            self.retell_issues();
            if unnamed {
                self.managers_unnamed();
            }
        }

        // Listed whether or not its folder is here: one that isn't shows among the moved ones.
        if let Some(part) = &projects {
            for favourite in &part.favourites {
                self.registry.set_favourite(favourite.clone(), true);
            }
            self.reload_favourites();
            let theirs = unique(part.favourites.iter().map(|favourite| favourite.path.clone()).chain(part.added.iter().cloned()).collect());
            let added = self.lock().settings.added.clone().unwrap_or_default();
            report.projects = theirs.iter().filter(|path| added.iter().any(|known| same_path(known, path))).count();
        }

        if let Some(cards) = cards {
            report.cards = self.import_cards(cards);
        }
        if let Some(days) = days {
            report.days = self.import_days(&days);
        }

        self.notify();
        Ok(report)
    }

    /// Copies what `change` (an `import`, a `move`, a `forget`) is about to write over into
    /// `backups/before-<change>-<ms>/` beside the settings, as it is on disk now: the settings,
    /// the board, the list of projects and, under `board-images/`, the images of the board's
    /// cards among `incoming` (ids), whose place an import's cards take. A file that isn't
    /// there yet has nothing to keep; any other failure is the caller's to stop at.
    fn keep_before(&self, change: &str, incoming: &[&str]) -> std::io::Result<()> {
        let settings = &self.cfg.settings_file;
        let dir = settings.with_file_name(KEPT_DIR).join(format!("before-{change}-{}", now_ms()));
        fs::create_dir_all(&dir)?;

        for file in [settings.clone(), settings.with_file_name("board.json"), self.cfg.registry_dir.join("favorites.json")] {
            let Some(name) = file.file_name() else {
                continue;
            };
            match fs::copy(&file, dir.join(name)) {
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        for card in self.board_state().iter().filter(|card| incoming.contains(&card.id.as_str())) {
            // Only a card with a folder of its own has files (`Board::image_dir`).
            let Some((_, files)) = self.card_images(card) else {
                continue;
            };
            let kept = dir.join("board-images").join(&card.id);
            fs::create_dir_all(&kept)?;
            for (_, file) in &files {
                if let Some(name) = file.file_name() {
                    fs::copy(file, kept.join(name))?;
                }
            }
        }
        Ok(())
    }

    /// Puts a backup's cards on the board, through the board's own save: a card whose id the
    /// board has takes that card's place and its images go, the others come after the cards
    /// that are here. A card whose images can't be written arrives without them. Returns how
    /// many were put.
    ///
    /// The terminal a card ran in was its run's, as when the board's own file is read: a card
    /// the file left in `claude` with one goes to `review`. A tab open here for the card it
    /// takes the place of is this run's, and stays the card's.
    fn import_cards(&self, cards: Vec<Taken>) -> usize {
        if cards.is_empty() {
            return 0;
        }
        let count = cards.len();
        let now = now_ms();
        let mut board = self.board();

        for Taken { mut card, images } in cards {
            let at = board.cards.iter().position(|known| known.id == card.id);
            let before = at.map(|at| board.cards[at].clone());
            let open = before.as_ref().and_then(|before| before.terminal);
            if card.terminal.take().is_some() && open.is_none() && card.column == Column::Claude {
                card.column = Column::Review;
                card.moved_at = now;
            }
            card.terminal = open;

            if let Some(before) = &before {
                board.remove_images(&before.id, &before.images);
            }
            if board.write_images(&card.id, &images).is_err() {
                board.remove_images(&card.id, &card.images);
                card.images.clear();
            }
            match at {
                Some(at) => board.cards[at] = card,
                None => board.cards.push(card),
            }
        }

        self.board_changed(board);
        count
    }

    /// Writes the days of a backup that have no file here; a day that has one is left as it
    /// is. Returns how many were written.
    fn import_days(&self, days: &[(String, String)]) -> usize {
        let Some(dir) = self.activity_dir().filter(|_| !days.is_empty()) else {
            return 0;
        };
        let _ = fs::create_dir_all(&dir);

        days.iter()
            .filter(|(day, text)| {
                let file = dir.join(format!("{day}.jsonl"));
                // Opening fails when the day has a file: nothing is ever written over one.
                let Ok(mut opened) = fs::OpenOptions::new().append(true).create_new(true).open(&file) else {
                    return false;
                };
                let written = opened.write_all(text.as_bytes()).is_ok();
                if !written {
                    let _ = fs::remove_file(&file);
                }
                written
            })
            .count()
    }

    /// The projects whose folder isn't there: every path among the listed projects, the
    /// projects' settings and the teams' members that is no folder now, with what is kept for
    /// it, by name. A path nothing is kept for (an empty settings entry and nothing else) isn't
    /// one of them.
    pub fn missing_projects(&self) -> Vec<MissingProject> {
        let (favourites, settings) = {
            let inner = self.lock();
            (inner.favourites.clone(), inner.settings.clone())
        };
        let cards = self.board_state();

        let named = favourites
            .iter()
            .map(|favourite| &favourite.path)
            .chain(settings.added.iter().flatten())
            .chain(settings.projects.keys())
            .chain(settings.teams.iter().flat_map(|team| &team.projects));
        let mut missing: Vec<MissingProject> = unique(named.cloned().collect())
            .into_iter()
            .filter(|path| !Path::new(path).is_dir())
            .filter_map(|path| {
                let kept = settings.projects.iter().find(|(known, _)| same_path(known, &path)).map(|(_, project)| project.clone()).unwrap_or_default();
                let listed = favourites.iter().map(|favourite| &favourite.path).chain(settings.added.iter().flatten()).any(|known| same_path(known, &path));
                let teamed = settings.teams.iter().any(|team| team.projects.iter().any(|member| same_path(member, &path)));
                let cards = cards.iter().filter(|card| same_path(&card.path, &path)).count();

                (listed || teamed || cards > 0 || kept != ProjectSettings::default()).then(|| MissingProject {
                    name: folder_name(&path),
                    path,
                    commands: kept.commands.len(),
                    links: kept.links.len(),
                    cards,
                    listed,
                })
            })
            .collect();

        missing.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.path.cmp(&b.path)));
        missing
    }

    /// Everything kept for the folder `from` goes to the folder `to`, which must be there and
    /// be another one: the project's settings, its place in the order and its team
    /// (`move_project`), its place among the listed projects when it had one, and its cards,
    /// after `to`'s own. `to` needn't be listed for its cards, as `board_rehome` asks: what is
    /// kept for a folder that was never listed moves all the same. What is here is copied aside
    /// first (`keep_before`); that failing, nothing moves.
    pub fn rehome_project(&self, from: &str, to: &str) -> Result<(), String> {
        if !Path::new(to).is_dir() {
            return Err(t!("core.error.notFolder", path = to));
        }
        if same_path(from, to) {
            return Err(t!("core.error.rehomeSame", path = to));
        }
        self.keep_before("move", &[]).map_err(|error| t!("core.error.keepFirst", error = error))?;

        move_project(&mut self.lock().settings, from, to);
        self.save_settings();

        // `to` first: listed twice for a moment rather than not at all.
        if self.registry.read_favourites().iter().any(|favourite| same_path(&favourite.path, from)) {
            self.registry.set_favourite(Favourite { path: to.to_string(), name: folder_name(to) }, true);
            self.registry.set_favourite(Favourite { path: from.to_string(), name: folder_name(from) }, false);
        }
        self.reload_favourites();

        {
            let mut board = self.board();
            let (mut moved, kept): (Vec<Card>, Vec<Card>) = std::mem::take(&mut board.cards).into_iter().partition(|card| same_path(&card.path, from));
            let any = !moved.is_empty();
            for card in &mut moved {
                card.path = to.to_string();
            }
            board.cards = kept;
            board.cards.append(&mut moved);
            if any {
                self.board_changed(board);
            }
        }

        self.notify();
        Ok(())
    }

    /// What is kept for a project whose folder is gone is dropped: its settings, its place in
    /// the order, its team, its mute and its place among the listed projects. Its cards stay.
    /// A folder that is there is no such project: nothing of it is dropped. What is here is
    /// copied aside first (`keep_before`); that failing, nothing is dropped.
    pub fn forget_project(&self, path: &str) -> Result<(), String> {
        if Path::new(path).is_dir() {
            return Ok(());
        }
        self.keep_before("forget", &[]).map_err(|error| t!("core.error.keepFirst", error = error))?;

        {
            let mut inner = self.lock();
            take_project(&mut inner.settings, path);
            inner.settings.order.retain(|known| !same_path(known, path));
            if let Some(added) = &mut inner.settings.added {
                added.retain(|known| !same_path(known, path));
            }
            for team in &mut inner.settings.teams {
                team.projects.retain(|known| !same_path(known, path));
            }
            inner.settings.notifications.muted.retain(|known| !same_path(known, path));
            inner.held.remove(path);
        }
        self.save_settings();

        self.registry.set_favourite(Favourite { path: path.to_string(), name: folder_name(path) }, false);
        self.reload_favourites();
        self.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings::ProjectLink;

    const ALL: BackupParts = BackupParts { settings: true, projects: true, board: true, activity: true };

    fn core_in(tmp: &Path) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude").join("projects"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        )
    }

    /// How a PNG starts, and a little more.
    fn png(fill: u8) -> Vec<u8> {
        [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[fill; 16][..]].concat()
    }

    /// An image as the window hands one over.
    fn pasted(n: u32, fill: u8) -> NewImage {
        NewImage { n, data: format!("data:image/png;base64,{}", BASE64.encode(png(fill))) }
    }

    /// Every file under `dir`, by its path from there, its parts joined by `/` on every system.
    fn files(dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, found: &mut Vec<String>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, found);
                } else {
                    let parts: Vec<_> = path.strip_prefix(root).unwrap().iter().map(|part| part.to_string_lossy()).collect();
                    found.push(parts.join("/"));
                }
            }
        }

        let mut found = Vec::new();
        walk(dir, dir, &mut found);
        found.sort();
        found
    }

    /// A backup file of these parts, of a machine that names no home.
    fn backup(parts: Value) -> String {
        let mut file = json!({ "format": "pitwall-backup", "v": 1, "createdAt": 1_790_000_000_000u64, "app": "9.9.9", "home": "" });
        file.as_object_mut().unwrap().extend(parts.as_object().unwrap().clone());
        file.to_string()
    }

    fn card(id: &str, path: &str, title: &str, note: &str) -> Value {
        json!({ "id": id, "path": path, "title": title, "note": note, "column": "queued", "createdAt": 1_790_000_000_000u64, "movedAt": 1_790_000_000_000u64 })
    }

    fn command(id: &str, name: &str) -> CustomCommand {
        CustomCommand { id: id.into(), name: name.into(), command: format!("npm run {name}"), ..Default::default() }
    }

    fn listed(core: &Core) -> Vec<String> {
        let mut paths: Vec<String> = core.registry.read_favourites().into_iter().map(|favourite| favourite.path).collect();
        paths.sort();
        paths
    }

    /// A backup is a file anyone can hand the user. A card's id, an image or a day in it that
    /// names a place outside the board's and the timeline's own folders must never be written
    /// there: an import would write over the user's files, and nothing would show it. Such an
    /// entry is skipped and the rest of the file arrives; a file with a part that doesn't parse
    /// changes nothing at all. Nor does a team of the file arrive set to start with Pitwall: its
    /// servers would come up at the next start, unasked.
    #[test]
    fn an_import_writes_nothing_outside_the_apps_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let core = core_in(tmp.path());

        // Where a forged id or day would land: `board-images/../../kept`, `activity/../../kept/day`.
        let kept = tmp.path().join("kept");
        fs::create_dir_all(&kept).unwrap();
        fs::write(kept.join("1.png"), b"mine").unwrap();
        let absolute = tmp.path().join("absolute").to_string_lossy().into_owned();
        let absolute_day = format!("{absolute}-day");

        let project = "/Users/me/projects/paddock";
        let image = |card: &str, n: u32, kind: &str, bytes: &[u8]| json!({ "card": card, "n": n, "kind": kind, "data": BASE64.encode(bytes) });
        let forged = |kind: &str| {
            backup(json!({
                "board": {
                    "cards": [
                        card("../../kept", project, "Up and out", "[Image #1]"),
                        card(&absolute, project, "From the root", "[Image #1]"),
                        card("cut\u{0}short", project, "A NUL", "[Image #1]"),
                        card("", project, "No id", "[Image #1]"),
                        card("b2-1", project, "Fine", "[Image #1] [Image #2]"),
                    ],
                    "images": [
                        image("../../kept", 1, "png", &png(1)),
                        image(&absolute, 1, "png", &png(1)),
                        image("cut\u{0}short", 1, "png", &png(1)),
                        image("", 1, "png", &png(1)),
                        image("nobody", 1, "png", &png(1)),
                        image("b2-1", 1, kind, &png(7)),
                        image("b2-1", 2, "png", b"#!/bin/sh\nrm -rf ~\n"),
                    ],
                },
                "activity": {
                    "../../kept/day": "forged\n",
                    (absolute_day.as_str()): "forged\n",
                    "2026-10-1\u{0}": "forged\n",
                    "2026-02-30": "forged\n",
                    "2026-10-11": "{\"t\":1,\"kind\":\"app\",\"state\":\"start\"}\n",
                },
                "projects": { "teams": [
                    { "id": "t8", "name": "Up by itself", "projects": [project], "startAtLaunch": true },
                    { "id": "t9", "name": "Here already", "projects": [project], "startAtLaunch": true },
                ] },
            }))
        };
        let file = tmp.path().join("forged.json");
        let everything = || files(tmp.path());

        // A kind that isn't one of the board's doesn't parse: the file is refused whole, and not
        // even the copy of what is here is made.
        fs::write(&file, forged("png/../../kept/1")).unwrap();
        let before = everything();
        assert!(core.backup_read(&file).is_err());
        assert!(core.backup_import(&file, ALL).is_err());
        assert_eq!(everything(), before);
        assert!(core.board_state().is_empty());

        // A team that starts with Pitwall here; the file names it again, with projects of its own.
        core.set_settings(Settings { teams: vec![Team { id: "t9".into(), start_at_launch: true, ..Default::default() }], ..core.settings() });
        fs::write(&file, forged("png")).unwrap();
        let info = core.backup_read(&file).unwrap();
        let report = core.backup_import(&file, ALL).unwrap();

        // Nothing outside the app's own folder appeared or changed.
        let outside = |all: Vec<String>| all.into_iter().filter(|file| !file.starts_with("config/")).collect::<Vec<_>>();
        assert_eq!(outside(everything()), outside(before));
        assert_eq!(fs::read(kept.join("1.png")).unwrap(), b"mine");

        // Inside it: the one card that is the board's, its one image, the one day.
        assert_eq!((info.cards, info.images, info.days), (1, 1, 1));
        assert_eq!((report.cards, report.days), (1, 1));
        // (Beside the settings a first start writes, and their copy under `backups/`.)
        let own: Vec<String> = files(&config).into_iter().filter(|file| !file.starts_with("backups/")).collect();
        assert_eq!(own, ["activity/2026-10-11.jsonl", "board-images/b2-1/1.png", "board.json", "settings.json"]);
        let cards = core.board_state();
        assert_eq!(cards.iter().map(|card| card.id.as_str()).collect::<Vec<_>>(), ["b2-1"]);
        assert_eq!(cards[0].images, [CardImage { n: 1, kind: ImageKind::Png }]);
        assert_eq!(fs::read(config.join("board-images/b2-1/1.png")).unwrap(), png(7));

        // Nor does the file say what starts by itself: its teams arrive with that switched off.
        assert_eq!(core.settings().teams.iter().map(|team| (team.id.as_str(), team.start_at_launch)).collect::<Vec<_>>(), [("t9", false), ("t8", false)]);
    }

    /// An import adds to what is here. A card, a project's settings, a team or a day the file
    /// doesn't name would otherwise be gone for good, and a day written over would be too, with
    /// no word of it; the settings that are this Mac's own stay whatever the file says; and
    /// what the import did replace is still there to take back, in `backups/before-import-*`.
    #[test]
    fn an_import_keeps_what_was_here() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let core = core_in(tmp.path());
        let (here, shared, theirs) = ("/Users/me/projects/here", "/Users/me/projects/shared", "/Users/me/projects/theirs");
        let unstarred = "/Users/me/projects/unstarred";
        let team = |id: &str, projects: &[&str]| Team { id: id.into(), name: id.to_uppercase(), projects: projects.iter().map(|path| path.to_string()).collect(), ..Default::default() };

        core.set_favourite(here, true);
        core.set_settings(Settings { script: "serve".into(), onboarded: true, order: vec![here.into()], teams: vec![team("t1", &[here, shared])], ..core.settings() });
        core.set_project_settings(here, ProjectSettings { commands: vec![command("c1", "worker")], ..Default::default() });
        let kept = core.board_add(here, "Kept", "see [Image #1]", &[pasted(1, 1)]).unwrap();
        let replaced = core.board_add(here, "As it was", "see [Image #1]", &[pasted(1, 2)]).unwrap();
        fs::create_dir_all(config.join("activity")).unwrap();
        fs::write(config.join("activity/2026-10-10.jsonl"), "here\n").unwrap();
        let as_it_was = [config.join("settings.json"), config.join("board.json"), tmp.path().join("registry/favorites.json")].map(|file| fs::read_to_string(file).unwrap());

        let file = tmp.path().join("backup.json");
        let parts = json!({
            "settings": { "script": "dev", "launchAtLogin": true, "onboarded": false, "seenVersion": "0.0.1", "notify": false, "fuelAlert": false },
            "projects": {
                "favourites": [{ "path": theirs, "name": "theirs" }],
                "added": [theirs, unstarred],
                "order": [theirs],
                "settings": { (theirs): { "commands": [command("c2", "tests")] } },
                "teams": [team("t2", &[theirs, shared])],
            },
            "board": { "cards": [card(&replaced.id, here, "From the file", ""), card("b9-1", theirs, "New", "")] },
            "activity": { "2026-10-09": "older\n", "2026-10-10": "theirs\n" },
        });
        fs::write(&file, backup(parts)).unwrap();

        let report = core.backup_import(&file, ALL).unwrap();
        assert_eq!(report, ImportReport { projects: 2, cards: 2, days: 1, settings: true });

        // The file's preference came; what is this Mac's own stayed.
        let settings = core.settings();
        let seen = Some(crate::whatsnew::CURRENT.to_string());
        assert_eq!((settings.script.as_str(), settings.launch_at_login, settings.onboarded, &settings.seen_version), ("dev", false, true, &seen));
        // What is told is this Mac's own too, Claude waiting and the fuel alert as every other event.
        assert_eq!((settings.notify, settings.fuel_alert), (true, true));
        // Projects, their settings and teams: the file's beside those that were here, and the
        // project both teams named is in the file's alone.
        assert_eq!(listed(&core), [here, theirs]);
        assert_eq!(settings.added.as_deref(), Some(&[here, theirs, unstarred].map(String::from)[..]), "a project without its star arrives too");
        assert_eq!(settings.project(here).commands, [command("c1", "worker")]);
        assert_eq!(settings.project(theirs).commands, [command("c2", "tests")]);
        assert_eq!(settings.order, [theirs, here]);
        assert_eq!(settings.teams, [team("t1", &[here]), team("t2", &[theirs, shared])]);

        // The card the file names again is the file's; the other is as it was, its image too.
        let cards = core.board_state();
        assert_eq!(cards.iter().map(|card| (card.id.as_str(), card.title.as_str())).collect::<Vec<_>>(), [(kept.id.as_str(), "Kept"), (replaced.id.as_str(), "From the file"), ("b9-1", "New")]);
        assert_eq!(cards[0], kept);
        assert_eq!(fs::read(config.join("board-images").join(&kept.id).join("1.png")).unwrap(), png(1));

        // A day that had a file is as it was; the other one arrived.
        assert_eq!(fs::read_to_string(config.join("activity/2026-10-10.jsonl")).unwrap(), "here\n");
        assert_eq!(fs::read_to_string(config.join("activity/2026-10-09.jsonl")).unwrap(), "older\n");

        // What was here before, to take back: the files as they were, and the image of the card
        // the file's took the place of, which the board no longer has.
        let copies: Vec<PathBuf> = fs::read_dir(config.join("backups")).unwrap().flatten().map(|entry| entry.path()).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert!(copies[0].file_name().unwrap().to_string_lossy().starts_with("before-import-"));
        assert_eq!(["settings.json", "board.json", "favorites.json"].map(|name| fs::read_to_string(copies[0].join(name)).unwrap()), as_it_was);
        assert!(!config.join("board-images").join(&replaced.id).exists());
        assert_eq!(fs::read(copies[0].join("board-images").join(&replaced.id).join("1.png")).unwrap(), png(2));
    }

    /// A backup is how what Pitwall keeps gets to another Mac, where the home folder has another
    /// name. Were its paths left as they were, every project, its commands, its team and its
    /// cards would arrive under folders that aren't there, with nothing to say so; a path that
    /// isn't under the old home (one whose name only starts like it, too) must stay where it is.
    #[test]
    fn a_backup_arrives_under_another_home() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (home_a, home_b) = (a.path().join("home"), b.path().join("home"));
        let under = |home: &Path, rest: &str| home.join(rest).to_string_lossy().into_owned();
        let project = under(&home_a, "projects/paddock");
        fs::create_dir_all(&project).unwrap();
        let elsewhere = "/opt/sites/depot".to_string();
        let neighbour = format!("{}-old/site", home_a.display());

        let from = core_in(a.path());
        from.add_project(&project).unwrap();
        from.set_favourite(&elsewhere, true);
        from.set_favourite(&neighbour, true);
        let kept = ProjectSettings { port: Some(5180), commands: vec![CustomCommand { keep_running: true, ..command("c1", "worker") }], ..Default::default() };
        let team = |paths: [&String; 2]| Team { id: "t1".into(), name: "Race".into(), projects: paths.map(String::clone).to_vec(), ..Default::default() };
        from.set_settings(Settings { projects_dir: Some(under(&home_a, "projects")), order: vec![elsewhere.clone(), project.clone()], teams: vec![team([&project, &neighbour])], ..from.settings() });
        from.set_project_settings(&project, kept.clone());
        let card = from.board_add(&project, "Card", "see [Image #1]", &[pasted(1, 3)]).unwrap();
        let event = |path: &str| json!({ "t": 1_790_000_000_000u64, "path": path, "kind": "server", "state": "running" }).to_string();
        fs::create_dir_all(a.path().join("config/activity")).unwrap();
        fs::write(a.path().join("config/activity/2026-10-11.jsonl"), format!("{}\n{}\n", event(&project), event(&elsewhere))).unwrap();

        let file = a.path().join("pitwall-backup.json");
        from.backup_write_as(&file, ALL, &home_a.to_string_lossy()).unwrap();

        let to = core_in(b.path());
        let report = to.backup_import_as(&file, ALL, &home_b.to_string_lossy()).unwrap();
        assert_eq!(report, ImportReport { projects: 3, cards: 1, days: 1, settings: true });

        // A core started on what the import wrote: all of it is on disk, under the new home.
        let again = core_in(b.path());
        let moved = under(&home_b, "projects/paddock");
        let mut expected = vec![moved.clone(), elsewhere.clone(), neighbour.clone()];
        expected.sort();
        assert_eq!(listed(&again), expected);

        let settings = again.settings();
        assert_eq!(settings.projects.iter().collect::<Vec<_>>(), [(&moved, &kept)]);
        assert_eq!(settings.order, [elsewhere.clone(), moved.clone()]);
        assert_eq!(settings.teams, [team([&moved, &neighbour])]);
        assert_eq!(settings.projects_dir, Some(under(&home_b, "projects")));

        assert_eq!(again.board_state(), [Card { path: moved.clone(), ..card.clone() }]);
        assert_eq!(again.board_image(&card.id, 1).unwrap(), pasted(1, 3).data);
        assert_eq!(fs::read_to_string(b.path().join("config/activity/2026-10-11.jsonl")).unwrap(), format!("{}\n{}\n", event(&moved), event(&elsewhere)));
    }

    /// A card told to wait was told so on the Mac the backup was made on, for a session that is
    /// that Mac's. Imported, it waits for nothing: the core here would give it to Claude at that
    /// moment, with nobody having asked. The model and the mode it was last started with are the
    /// card's own, and come along.
    #[test]
    fn an_imported_card_waits_for_nothing() {
        use crate::start::{Mode, Model, Start};
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (home_a, home_b) = (a.path().join("home"), b.path().join("home"));
        let project = home_a.join("projects/paddock").to_string_lossy().into_owned();
        fs::create_dir_all(&project).unwrap();

        let from = core_in(a.path());
        from.add_project(&project).unwrap();
        let chosen = Start { model: Some(Model::Sonnet), mode: Mode::Plan };
        let waits = from.board_add(&project, "Waits", "", &[]).unwrap();
        from.board_schedule(&waits.id, crate::registry::now_ms() + 3_600_000, true, chosen).unwrap();
        let started = from.board_add(&project, "Started before", "", &[]).unwrap();
        from.board().cards.iter_mut().find(|card| card.id == started.id).unwrap().start = Some(chosen);

        let file = a.path().join("pitwall-backup.json");
        from.backup_write_as(&file, ALL, &home_a.to_string_lossy()).unwrap();
        core_in(b.path()).backup_import_as(&file, ALL, &home_b.to_string_lossy()).unwrap();

        // As a core started on what the import wrote has them.
        let cards = core_in(b.path()).board_state();
        assert_eq!(cards.iter().map(|card| (card.title.as_str(), card.schedule, card.start)).collect::<Vec<_>>(), [("Waits", None, None), ("Started before", None, Some(chosen))]);
    }

    /// What is kept for a project is typed in by hand: its commands, its links, its team, its
    /// place in the list, its cards, its mute. A move to the project's new folder that left any
    /// of it behind, or wrote over what the new folder already had, would lose it without a word.
    #[test]
    fn a_moved_project_takes_everything_along() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());
        let folder = |name: &str| tmp.path().join(name).to_string_lossy().into_owned();
        let (old, mid, new, other) = (folder("old/paddock"), folder("mid/paddock"), folder("new/paddock"), folder("other"));
        fs::create_dir_all(&mid).unwrap();
        fs::create_dir_all(&new).unwrap();
        let link = |name: &str| ProjectLink { name: name.into(), url: format!("https://{name}.paddock.test") };
        let titles = |path: &str| -> Vec<String> { core.board_state().into_iter().filter(|card| card.path == path).map(|card| card.title).collect() };
        let sorted = |mut paths: Vec<String>| {
            paths.sort();
            paths
        };

        core.set_favourite(&old, true);
        core.set_favourite(&other, true);
        core.set_settings(Settings {
            order: vec![other.clone(), old.clone()],
            teams: vec![Team { id: "t1".into(), name: "Race".into(), projects: vec![old.clone(), other.clone()], ..Default::default() }],
            ..core.settings()
        });
        let kept = ProjectSettings {
            folder: Some("web".into()),
            script: Some("serve".into()),
            port: None,
            url: Some("https://paddock.test".into()),
            commands: vec![command("c1", "worker"), command("c2", "tests")],
            links: vec![link("staging")],
            claude_model: Some(crate::start::Model::Sonnet),
            claude_mode: crate::start::Mode::Plan,
            code: Some("PDK".into()),
            open: crate::settings::ProjectOpen { list: Some("editor".into()), outside: Some("list".into()), pages: None },
            double_click: None,
        };
        core.set_project_settings(&old, kept.clone());
        core.mute_project(&old, true);
        for title in ["One", "Two"] {
            core.board_add(&old, title, "", &[]).unwrap();
        }
        core.board_add(&other, "Theirs", "", &[]).unwrap();

        // To no folder, or to the folder it is: nothing moves.
        assert!(core.rehome_project(&old, &folder("nowhere")).is_err());
        assert!(core.rehome_project(&mid, &mid).is_err());
        assert_eq!((core.settings().project(&old), titles(&old).len()), (kept.clone(), 2));

        // To a folder nothing is kept for: all of it, as it was.
        core.rehome_project(&old, &mid).unwrap();
        let settings = core.settings();
        assert_eq!(settings.projects.iter().collect::<Vec<_>>(), [(&mid, &kept)]);
        assert_eq!(settings.order, [other.clone(), mid.clone()]);
        assert_eq!(settings.teams[0].projects, [mid.clone(), other.clone()]);
        assert_eq!(settings.notifications.muted, [mid.as_str()], "muted where it was, muted where it is");
        assert_eq!(listed(&core), sorted(vec![mid.clone(), other.clone()]));
        assert_eq!((titles(&old).len(), titles(&mid), titles(&other)), (0, vec!["One".to_string(), "Two".into()], vec!["Theirs".to_string()]));

        // To a folder with settings of its own: those stay, and what it lacks arrives. Its
        // `worker` is the same command under another id; its `deploy` has the id of `tests`.
        core.set_project_settings(&new, ProjectSettings { port: Some(4000), commands: vec![command("x9", "worker"), command("c2", "deploy")], links: vec![link("docs")], ..Default::default() });
        core.rehome_project(&mid, &new).unwrap();
        let settings = core.settings();
        assert_eq!(settings.projects.keys().collect::<Vec<_>>(), [&new]);
        let merged = settings.project(&new);
        assert_eq!((merged.script.as_deref(), merged.port, merged.url.as_deref()), (Some("serve"), Some(4000), Some("https://paddock.test")));
        assert_eq!(merged.commands.iter().map(|command| command.name.as_str()).collect::<Vec<_>>(), ["worker", "deploy", "tests"]);
        assert_eq!(merged.commands.iter().map(|command| command.id.as_str()).collect::<HashSet<_>>().len(), 3, "{:?}", merged.commands);
        assert_eq!(merged.links, [link("docs"), link("staging")]);
        assert_eq!((merged.claude_model, merged.claude_mode, merged.code.as_deref()), (kept.claude_model, kept.claude_mode, Some("PDK")));
        assert_eq!(settings.teams[0].projects, [new.clone(), other.clone()]);
        assert_eq!(settings.notifications.muted, [new.as_str()]);
        assert_eq!(listed(&core), sorted(vec![new.clone(), other.clone()]));
        assert_eq!((titles(&mid).len(), titles(&new).len(), core.board_state().len()), (0, 2, 3));
    }
}
