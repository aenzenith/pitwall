//! Laps (Turlar): jobs that run on a schedule while Pitwall runs. A job is bound to a listed
//! project or to a folder the user picked, and is an agent's session, a script and then an
//! agent, or a script alone (`types.rs`). See `docs/TURLAR.md`.
//!
//! - `when.rs`, `cron.rs`: when a job's moments are, on this Mac's clock.
//! - `store.rs`: `laps.json`, `lap-runs/` (records and script output) beside the settings.
//! - `due.rs`: the loop's second: due moments, missed ones, overlap, the lock.
//! - `run.rs`: one run: checks, script, gate, prompt, agent (a tab or the background), its end.
//!
//! Nothing of an agent's words is kept: they are read from the session's own log when asked
//! (`lap_result`), and once at a run's end only to hash them, write them into the file the user
//! named, or hand them to the next run's prompt.

mod cron;
mod due;
mod run;
mod store;
mod types;
mod when;

pub use types::*;

use std::collections::HashMap;

use super::reveal::{is_session_id, Liveness, Opened};
use super::signals::Signal;
use super::terminal::Launch;
use super::*;
use crate::awake::Awake;
use store::Store;

/// What a run is stopped for, set by whoever stops it and read by the run's own thread, which
/// ends what it started (`run.rs`).
type StopFlag = Arc<Mutex<Option<Reason>>>;

/// A run that goes, by its job's id.
struct Going {
    run: String,
    /// `script` | `agent` | `waiting` | `retry`.
    phase: &'static str,
    started_at: u64,
    /// The script's or the background agent's process group while it runs.
    pgid: Option<u32>,
    /// The tab its agent runs in.
    tab: Option<u64>,
    session: Option<String>,
    agent: Option<Agent>,
    /// Held while it goes, when the job keeps the computer awake.
    awake: Option<Awake>,
    stop: StopFlag,
    /// What the schedule's `begin` moved, put back should the run not start (`due.rs`).
    undo: Option<Undo>,
}

/// A job as it was before the tick began a run of it: its switch, next moment, runs done and the
/// moment held for it.
#[derive(Debug, Clone, Copy)]
struct Undo {
    on: bool,
    next_at: Option<u64>,
    runs_done: u32,
    held: Option<Held>,
}

/// A moment that came while the job's previous run still went (`Overlap::Wait`, `Stop`): it
/// starts once that run has ended, if still within the grace.
#[derive(Debug, Clone, Copy)]
struct Held {
    at: u64,
}

/// An account's fuel as its last usage reading had it (`laps_fuel`).
#[derive(Debug, Clone, Default, PartialEq)]
struct Fuel {
    week: Option<f64>,
    refills: Vec<u64>,
}

/// What `fuel.rs` reads of an account's usage for the laps: the week's share used (none on a plan
/// without one) and the refills still ahead, the 5-hour session's first.
pub type FuelReading = (Option<f64>, Vec<u64>);

/// An agent's accounts as its last usage readings had them (`laps_fuel`).
#[derive(Debug, Clone, Default, PartialEq)]
struct AgentFuel {
    /// The account a job that names none runs with.
    in_use: String,
    /// Every account by its id, with its reading; none before one was read.
    accounts: HashMap<String, Option<Fuel>>,
}

/// One refill as two readings may give it: the API's moment moves a little between them
/// (`fuel.rs` › `HOUR_MS`).
const SAME_REFILL_MS: u64 = 60 * 60 * 1000;

/// The tab a job's last run in a tab had, so the next one closes it once its turn is over.
#[derive(Debug, Clone)]
struct LastTab {
    tab: u64,
    session: Option<String>,
    agent: Agent,
}

/// What a run asked of its agent, for the tests' stand-in (`Laps::agent_hook`).
#[derive(Debug, Clone)]
#[allow(dead_code)] // what a stand-in may look at
pub(super) struct Asked {
    pub lap: String,
    pub agent: Agent,
    pub prompt: String,
    pub background: bool,
    /// The background command's arguments (`Start::background_args`).
    pub args: Option<Vec<String>>,
    pub session: Option<String>,
}

/// What the stand-in says: the exit code, the session and what the agent "said".
#[derive(Debug, Clone, Default)]
pub(super) struct Said {
    pub code: i32,
    pub session: Option<String>,
    pub message: Option<String>,
}

type AgentHook = Arc<dyn Fn(&Asked) -> Said + Send + Sync>;

/// A run the tick began (its record written): its thread runs it (`run.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Planned {
    pub lap: String,
    pub run: String,
}

/// The laps' state in memory: what `laps.json` holds, the kept runs, what runs now.
#[derive(Default)]
pub struct Laps {
    pub(super) laps: Vec<Lap>,
    /// Each job's kept runs, oldest first.
    pub(super) runs: HashMap<String, Vec<LapRun>>,
    /// The file couldn't be written as it is now.
    pub(super) unsaved: bool,
    store: Store,
    going: HashMap<String, Going>,
    held: HashMap<String, Held>,
    /// The last second the loop ran; a gap of more than 90 s since is sleep or a suspended
    /// process (`due.rs`).
    last_tick: u64,
    /// When the laps last woke: the lock taken (the files read again), or the loop back after a
    /// gap (sleep, a suspended process). A moment at or before it came while this Pitwall wasn't
    /// running the laps, whichever second looks at it (a job left for the next second by the
    /// starts' cap too).
    pub(super) woke_at: Option<u64>,
    last_date: String,
    fuel: HashMap<Agent, AgentFuel>,
    /// A permission or question each run's session was last told with (`lap_waits`), by session.
    waits_told: HashMap<String, u64>,
    /// `laps.lock`, while this Pitwall holds it; taken at the first tick (or change), not at
    /// the start, and tried again every minute while another one holds it.
    lock: Option<fs::File>,
    lock_tried: Option<u64>,
    elsewhere: bool,
    /// Pitwall quits: nothing more is started or written.
    quit: bool,
    tabs: HashMap<String, LastTab>,
    /// The laps' tabs that are neither a going run's nor a job's last: a session opened again
    /// from a dialog, a last tab a new run found shown or at work, a deleted job's. Each is closed
    /// once no dialog shows it and its agent is done (`lap_sweep_tabs`): an agent's session stays
    /// open after its turn, and no one would see it in a tab the panel never shows.
    loose: HashMap<u64, LastTab>,
    /// The laps' tabs a run's dialog shows now (`lap_terminal`, let go by `lap_terminal_left`).
    shown: HashSet<u64>,
    counter: u64,
    /// Tests: an agent's run is asked of this instead of a tab or a process.
    agent_hook: Option<AgentHook>,
    /// Tests: a minute, in ms (0: a real one), for retries.
    minute_ms: u64,
}

impl Laps {
    /// Read from beside `settings_file` (`store.rs`). Nothing is written or moved here: what the
    /// last run left going is cut, and a file that doesn't fit set aside, once this Pitwall holds
    /// the lock (`due.rs`).
    pub fn load(settings_file: &Path, now: u64) -> Self {
        let mut store = Store::beside(settings_file);
        let (laps, _) = store.load_laps(false);
        let runs = store.load_runs(&laps);
        Self { laps, runs, store, last_tick: now, ..Default::default() }
    }

    fn minute(&self) -> u64 {
        if self.minute_ms == 0 { 60_000 } else { self.minute_ms }
    }

    fn new_id(&mut self, prefix: &str, now: u64) -> String {
        loop {
            self.counter += 1;
            let id = format!("{prefix}-{}-{}", crate::registry::base36(now), crate::registry::base36(self.counter));
            let taken = self.laps.iter().any(|lap| lap.id == id) || self.runs.values().flatten().any(|run| run.id == id);
            if !taken {
                return id;
            }
        }
    }

    fn lap(&self, id: &str) -> Option<&Lap> {
        self.laps.iter().find(|lap| lap.id == id)
    }

    /// The sessions its runs hold, going or kept.
    fn sessions(&self) -> HashSet<String> {
        self.going.values().filter_map(|going| going.session.clone()).chain(self.runs.values().flatten().filter_map(|run| run.session.clone())).collect()
    }

    /// Where the job runs whose run holds `session`, going or kept.
    fn session_place(&self, session: &str) -> Option<Place> {
        let held = |run: &LapRun| run.session.as_deref() == Some(session);
        let lap = self.going.iter().find(|(_, going)| going.session.as_deref() == Some(session)).map(|(lap, _)| lap).or_else(|| self.runs.iter().find(|(_, runs)| runs.iter().any(held)).map(|(lap, _)| lap))?;
        Some(self.lap(lap)?.place.clone())
    }

    fn lap_mut(&mut self, id: &str) -> Option<&mut Lap> {
        self.laps.iter_mut().find(|lap| lap.id == id)
    }

    fn record_mut(&mut self, lap: &str, run: &str) -> Option<&mut LapRun> {
        self.runs.get_mut(lap)?.iter_mut().find(|record| record.id == run)
    }

    /// The going run of `lap`, if it is `run`.
    fn going_of(&mut self, planned: &Planned) -> Option<&mut Going> {
        self.going.get_mut(&planned.lap).filter(|going| going.run == planned.run)
    }

    /// The run of id `run`, whichever job's.
    fn find_run(&self, run: &str) -> Option<(&Lap, &LapRun)> {
        self.runs.iter().find_map(|(lap, runs)| Some((self.lap(lap)?, runs.iter().find(|record| record.id == run)?)))
    }

    /// The next moment of `lap` after `after`, on a day that isn't off (its weekdays off are out
    /// of `when` already): none while it is off, paused or ended; a refill job's is its agent's
    /// next refill, none until a reading told it.
    fn next_moment(&self, lap: &Lap, after: u64) -> Option<u64> {
        if !lap.on || lap.paused.is_some() || lap.ends.after.is_some_and(|after| lap.runs_done >= after) {
            return None;
        }
        let next = match lap.when {
            // A refill a run of the schedule already took is that refill still, however the
            // reading moved it: only one more than an hour past that run's moment is the next. One
            // on a day off is no moment.
            When::Refill => {
                let taken = self.runs.get(&lap.id).into_iter().flatten().filter(|run| !run.by_hand).map(|run| run.due + SAME_REFILL_MS).max().unwrap_or(0);
                self.fuel_of(lap)?.refills.iter().copied().filter(|&at| at > after.max(taken) && !lap.days_off.contains(&when::date_of(at).0)).min()
            }
            _ => when::next_after(&lap.when, after, &lap.days_off),
        }?;
        (!lap.ends.at.is_some_and(|end| next > end)).then_some(next)
    }

    /// The reading of the account `lap`'s agent runs with: its own (the main one for one no longer
    /// listed), else the one in use. None before one was read for it.
    fn fuel_of(&self, lap: &Lap) -> Option<&Fuel> {
        let known = self.fuel.get(&lap.start.agent)?;
        let id = match lap.account.as_deref() {
            Some(id) if known.accounts.contains_key(id) => id,
            Some(_) => crate::accounts::MAIN,
            None => &known.in_use,
        };
        known.accounts.get(id)?.as_ref()
    }

    /// Writes `laps.json`; whether it could be.
    fn save(&mut self) -> bool {
        let saved = self.store.save_laps(&self.laps);
        self.unsaved = !saved;
        saved
    }

    /// Writes `lap`'s runs, pruned to its `keep`.
    fn save_runs(&mut self, lap: &str) -> bool {
        let keep = self.lap(lap).map_or(1, |lap| lap.keep);
        let Self { store, runs, .. } = self;
        store.save_runs(lap, runs.entry(lap.to_string()).or_default(), keep)
    }

    /// May this Pitwall change the laps now: it holds the lock (taken now if need be) and isn't
    /// quitting.
    fn writable(&mut self, now: u64) -> Result<(), String> {
        if self.quit {
            return Err(t!("core.laps.error.unsaved"));
        }
        match self.ensure_lock(now) {
            Some(_) => Ok(()),
            None => Err(t!("core.laps.error.elsewhere")),
        }
    }

    /// The previous kept run of `lap` before `run` that has a hash, a head…: as `pick` says.
    fn previous<T>(&self, lap: &str, run: &str, pick: impl Fn(&LapRun) -> Option<T>) -> Option<T> {
        self.runs.get(lap)?.iter().rev().filter(|record| record.id != run).find_map(pick)
    }
}

fn not_found() -> String {
    t!("core.laps.error.notFound")
}

/// `~` and `~/…` under the home folder.
fn expand_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') || rest.starts_with('\\') => home.join(&rest[1..]),
        _ => PathBuf::from(path),
    }
}

/// `path` as shown: under the home folder as `~/…`.
fn shown(path: &str) -> String {
    match dirs::home_dir().and_then(|home| Path::new(path).strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.to_string(),
    }
}

/// `path` (resolved) is in one of `roots` (Claude's, Codex's, Pitwall's own folders), or in a
/// folder of the home folder named as theirs (`.claude*`, `.codex*`, `.pitwall`).
fn refused_place(path: &Path, home: &Path, roots: &[PathBuf]) -> bool {
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let theirs = path.strip_prefix(&home).ok().and_then(|rest| rest.components().next()).is_some_and(|first| {
        let name = first.as_os_str().to_string_lossy().to_lowercase();
        name.starts_with(".claude") || name.starts_with(".codex") || name == ".pitwall"
    });
    theirs || roots.iter().any(|root| path.starts_with(root.canonicalize().unwrap_or_else(|_| root.clone())))
}

/// How much of a job its check looks at (`Core::lap_refused`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    /// Saved from the dialog, switched on, resumed: all of it, as the dialog checks it.
    Save,
    /// From a backup: all of it but what moving to this Mac changes (a folder, or the result
    /// file's, that isn't here yet is taken as written, refused only by its name) and the
    /// schedule's own time (a moment past runs nothing).
    Import,
    /// A run's start: all of it but the schedule's own time (its moment came).
    Run,
}

/// What a job is checked against besides itself: the projects listed and their saved commands,
/// as they are now, or as an import is about to leave them (`backup.rs`).
#[derive(Debug, Clone, Default)]
pub(crate) struct Known {
    pub listed: Vec<String>,
    /// The saved commands of each listed project, by its path.
    pub commands: Vec<(String, Vec<crate::settings::CustomCommand>)>,
}

impl Known {
    /// The project at `path`'s saved commands.
    fn commands_of(&self, path: &str) -> &[crate::settings::CustomCommand] {
        self.commands.iter().find(|(of, _)| same_path(of, path)).map_or(&[], |(_, commands)| commands.as_slice())
    }

    /// `path`'s saved commands are `commands` from now on (an import's settings for it).
    pub fn set_commands(&mut self, path: &str, commands: Vec<crate::settings::CustomCommand>) {
        self.commands.retain(|(of, _)| !same_path(of, path));
        self.commands.push((path.to_string(), commands));
    }
}

/// `path` has a `..` in it: what it names can only be told once it is resolved.
fn climbs(path: &Path) -> bool {
    path.components().any(|part| part == std::path::Component::ParentDir)
}

impl Core {
    fn laps(&self) -> MutexGuard<'_, Laps> {
        self.laps.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Claude's, Codex's and Pitwall's own folders: no lap runs or writes in them. The accounts'
    /// folders are the settings' list, whether their agent is on or not.
    fn laps_refused_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.cfg.registry_dir.clone(), self.cfg.codex_home.clone()];
        roots.extend(self.cfg.settings_file.parent().map(Path::to_path_buf));
        roots.extend(self.cfg.claude_dir.parent().map(Path::to_path_buf));
        let inner = self.lock();
        let accounts = inner.settings.accounts.others.iter().chain(&inner.settings.agents.codex.accounts.others);
        roots.extend(accounts.map(|account| PathBuf::from(&account.dir)));
        roots
    }

    /// The projects listed now and their saved commands, for a job's check.
    pub(crate) fn laps_known(&self) -> Known {
        let listed = self.listed_paths();
        let inner = self.lock();
        let commands = listed.iter().map(|path| (path.clone(), inner.settings.project(path).commands)).collect();
        Known { listed, commands }
    }

    /// The page's state changed: sent with no guard of the laps held.
    fn laps_changed(&self) {
        let state = self.laps_state();
        self.emit(CoreEvent::Laps(state));
    }

    /// Every job as the page draws it.
    pub fn laps_state(&self) -> LapsState {
        struct Row {
            lap: Lap,
            last: Vec<RunMark>,
            running: Option<(LapRunning, Option<String>)>,
            failing: bool,
            no_reading: bool,
            unread: usize,
        }
        let (rows, elsewhere) = {
            let laps = self.laps();
            let rows: Vec<Row> = laps
                .laps
                .iter()
                .map(|lap| {
                    let runs = laps.runs.get(&lap.id).map(Vec::as_slice).unwrap_or_default();
                    let last = runs[runs.len().saturating_sub(10)..].iter().map(|run| RunMark { id: run.id.clone(), due: run.due, result: run.result, unread: run.unread }).collect();
                    let running = laps.going.get(&lap.id).map(|going| {
                        (LapRunning { run: going.run.clone(), started_at: going.started_at, phase: going.phase, tab: going.tab, permission: None }, going.session.clone())
                    });
                    let last_failed = runs.iter().rev().find(|run| run.ended_at.is_some()).is_some_and(|run| run.result == Some(RunResult::Failed));
                    let no_reading = lap.kind.has_agent() && laps.fuel_of(lap).is_none();
                    let unread = runs.iter().filter(|run| run.unread).count();
                    Row { lap: lap.clone(), last, running, failing: last_failed || lap.paused == Some(PausedBy::Failures), no_reading, unread }
                })
                .collect();
            (rows, laps.elsewhere)
        };

        // A permission a run's session waits on: its tool's name, as the hook told it.
        let waits: HashMap<String, String> = {
            let sessions: Vec<&String> = rows.iter().filter_map(|row| row.running.as_ref()?.1.as_ref()).collect();
            if sessions.is_empty() {
                HashMap::new()
            } else {
                let inner = self.lock();
                inner
                    .lap_scanned
                    .iter()
                    .chain(&inner.scanned)
                    .filter(|state| sessions.contains(&&state.id) && state.phase == SessionPhase::Waiting)
                    .filter(|state| state.turn.is_some_and(|turn| turn.kind == crate::claude::TurnKind::Permission))
                    .filter_map(|state| Some((state.id.clone(), state.tool.clone()?)))
                    .collect()
            }
        };

        let failing = rows.iter().filter(|row| row.failing).count();
        let unread = rows.iter().map(|row| row.unread).sum();
        let laps = rows
            .into_iter()
            .map(|row| {
                let path = row.lap.place.path().to_string();
                let project = matches!(row.lap.place, Place::Project { .. }).then(|| self.project_name(&path));
                let running = row.running.map(|(mut running, session)| {
                    running.permission = session.and_then(|session| waits.get(&session).cloned());
                    running
                });
                LapView { git: Path::new(&path).join(".git").exists(), folder: shown(&path), project, last: row.last, running, no_reading: row.no_reading, unread: row.unread, lap: row.lap }
            })
            .collect();
        LapsState { laps, failing, unread, elsewhere }
    }

    /// Checks `lap` whole, as the window sent it: the fields it may set, resolved (its place,
    /// its file's template); the core's own are not looked at.
    fn lap_checked(&self, mut lap: Lap, now: u64) -> Result<Lap, String> {
        match self.lap_refused(&mut lap, now, &self.laps_known(), Check::Save) {
            Ok(()) => Ok(lap),
            Err((key, _)) => Err(t!(key)),
        }
    }

    /// Whatever brought `lap` (the dialog, a backup, a hand-written `laps.json`), it is kept and
    /// run only as `lap_save` takes it (`check` says how much of it is looked at): its fields
    /// resolved here (its place, its name, what its kind doesn't use); else why not, as the
    /// catalog's key and a run's reason. Takes the core's lock, never with the laps' held.
    pub(crate) fn lap_refused(&self, lap: &mut Lap, now: u64, known: &Known, check: Check) -> Result<(), (&'static str, Reason)> {
        let home = dirs::home_dir().unwrap_or_default();
        let roots = self.laps_refused_roots();
        let unfit = |key: &'static str| Err((key, Reason::NotStarted));
        // A folder that isn't on this Mac yet, which an import keeps as written: refused by what
        // its name says alone.
        let elsewhere = |path: &Path| check == Check::Import && path.is_absolute() && !climbs(path) && !path.exists();

        lap.name = lap.name.trim().to_string();
        if lap.name.is_empty() || lap.name.chars().count() > 200 {
            return unfit("core.laps.error.name");
        }
        lap.place = match &lap.place {
            Place::Project { path } => {
                let listed = known.listed.iter().find(|listed| same_path(listed, path)).cloned().ok_or(("core.laps.error.place", Reason::NotListed))?;
                // Its folder, when it is there, is none of the agents' or Pitwall's own either.
                if Path::new(&listed).canonicalize().is_ok_and(|folder| refused_place(&folder, &home, &roots)) {
                    return Err(("core.laps.error.placeRefused", Reason::NotFolder));
                }
                Place::Project { path: listed }
            }
            Place::Folder { path } => {
                let folder = expand_home(path.trim(), &home);
                if elsewhere(&folder) {
                    if refused_place(&folder, &home, &roots) {
                        return Err(("core.laps.error.placeRefused", Reason::NotFolder));
                    }
                    Place::Folder { path: folder.to_string_lossy().into_owned() }
                } else {
                    if !folder.is_absolute() || !folder.is_dir() {
                        return Err(("core.laps.error.place", Reason::NotFolder));
                    }
                    let folder = folder.canonicalize().map_err(|_| ("core.laps.error.place", Reason::NotFolder))?;
                    if refused_place(&folder, &home, &roots) {
                        return Err(("core.laps.error.placeRefused", Reason::NotFolder));
                    }
                    Place::Folder { path: folder.to_string_lossy().into_owned() }
                }
            }
        };
        if lap.kind.has_agent() && lap.prompt.trim().is_empty() {
            return unfit("core.laps.error.prompt");
        }
        if !lap.kind.has_agent() {
            lap.prompt = String::new();
        }
        // A tab's agent gets no prompt on Windows (see `lap_agent`): the job says where it runs.
        if cfg!(windows) {
            lap.place_run = Where::Background;
        }
        if !lap.kind.has_script() {
            lap.script = None;
        }
        match (&lap.script, &lap.place) {
            (None, _) if lap.kind.has_script() => return unfit("core.laps.error.script"),
            (Some(Script::Text { text }), _) if text.trim().is_empty() => return unfit("core.laps.error.script"),
            (Some(Script::Command { .. }), Place::Folder { .. }) => return Err(("core.laps.error.commandFolder", Reason::AskFirst)),
            (Some(Script::Command { id }), Place::Project { path }) if !known.commands_of(path).iter().any(|command| command.id == *id && !command.confirm) => {
                return Err(("core.laps.error.command", Reason::AskFirst));
            }
            _ => {}
        }
        // A run's moment came, and an import's past moment runs nothing: only a save looks at
        // whether the schedule's time is still to come.
        if let Some(key) = when::refused(&lap.when, lap.kind, now).filter(|key| check == Check::Save || !when::about_time(key)) {
            return unfit(key);
        }
        if !(1..=100).contains(&lap.keep) {
            return unfit("core.laps.error.keep");
        }
        if lap.timeout_min.is_some_and(|minutes| !(1..=1440).contains(&minutes)) {
            return unfit("core.laps.error.timeout");
        }
        if lap.fuel.is_some_and(|fuel| !(1..=100).contains(&fuel.over)) {
            return unfit("core.laps.error.fuel");
        }
        if lap.retry.is_some_and(|retry| retry.times > 5 || !(1..=120).contains(&retry.every_min)) {
            return unfit("core.laps.error.retry");
        }
        if lap.pause_after.is_some_and(|after| !(1..=20).contains(&after)) {
            return unfit("core.laps.error.pauseAfter");
        }
        lap.file = lap.file.take().map(|file| file.trim().to_string()).filter(|file| !file.is_empty());
        if let Some(template) = &lap.file {
            if run::result_file(template, &lap.name, now, &home, &roots).is_none() {
                // An import's file whose folder isn't here yet: by its name, under the home folder.
                let taken = run::result_path(template, &lap.name, now, &home)
                    .and_then(|file| file.parent().map(Path::to_path_buf))
                    .is_some_and(|folder| elsewhere(&folder) && folder.starts_with(&home) && !refused_place(&folder, &home, &roots));
                if !taken {
                    return unfit("core.laps.error.file");
                }
            }
        }
        if !lap.kind.has_agent() {
            lap.fuel = None;
            lap.gate.script = ScriptGate::None;
        }
        Ok(())
    }

    /// A folder picked for a job of no listed project, checked as `lap_save` checks it: resolved,
    /// and refused in Claude's, Codex's or Pitwall's own folders.
    pub fn lap_folder(&self, path: &str) -> Result<PickedFolder, String> {
        let home = dirs::home_dir().unwrap_or_default();
        let folder = expand_home(path.trim(), &home);
        if !folder.is_absolute() || !folder.is_dir() {
            return Err(t!("core.laps.error.place"));
        }
        let folder = folder.canonicalize().map_err(|_| t!("core.laps.error.place"))?;
        if refused_place(&folder, &home, &self.laps_refused_roots()) {
            return Err(t!("core.laps.error.placeRefused"));
        }
        let path = folder.to_string_lossy().into_owned();
        Ok(PickedFolder { shown: shown(&path), path })
    }

    /// Saves `lap` (a new one when its id is empty), checked whole first; the saved job, or why not.
    pub fn lap_save(&self, lap: Lap) -> Result<Lap, String> {
        let now = now_ms();
        let mut lap = self.lap_checked(lap, now)?;
        let saved = {
            let mut laps = self.laps();
            laps.writable(now)?;
            let stored = if lap.id.is_empty() { None } else { Some(laps.lap(&lap.id).cloned().ok_or_else(not_found)?) };
            match &stored {
                Some(stored) => {
                    lap.days_off = stored.days_off.clone();
                    lap.weekdays_off = stored.weekdays_off.iter().copied().filter(|&day| !when::has_weekday(&lap.when, day)).collect();
                    lap.paused = stored.paused;
                    lap.streak = stored.streak;
                    lap.runs_done = stored.runs_done;
                    lap.created_at = stored.created_at;
                }
                None => {
                    lap.id = laps.new_id("l", now);
                    lap.days_off = Vec::new();
                    lap.weekdays_off = Vec::new();
                    lap.paused = None;
                    lap.streak = 0;
                    lap.runs_done = 0;
                    lap.created_at = now;
                }
            }
            lap.next_at = laps.next_moment(&lap, now);
            let before = laps.laps.clone();
            match laps.laps.iter_mut().find(|kept| kept.id == lap.id) {
                Some(kept) => *kept = lap.clone(),
                None => laps.laps.push(lap.clone()),
            }
            laps.held.remove(&lap.id);
            if !laps.save() {
                laps.laps = before;
                return Err(t!("core.laps.error.unsaved"));
            }
            lap
        };
        self.laps_changed();
        Ok(saved)
    }

    /// Changes the job `id` through `change`, gives it its next moment again and writes
    /// `laps.json`; as it was when that can't be written.
    fn lap_change(&self, id: &str, change: impl FnOnce(&mut Lap) -> Result<(), String>) -> Result<(), String> {
        let now = now_ms();
        {
            let mut laps = self.laps();
            laps.writable(now)?;
            let before = laps.lap(id).cloned().ok_or_else(not_found)?;
            let mut lap = before.clone();
            change(&mut lap)?;
            lap.next_at = laps.next_moment(&lap, now);
            if lap == before {
                return Ok(());
            }
            *laps.lap_mut(id).ok_or_else(not_found)? = lap;
            if !laps.save() {
                *laps.lap_mut(id).ok_or_else(not_found)? = before;
                return Err(t!("core.laps.error.unsaved"));
            }
        }
        self.laps_changed();
        Ok(())
    }

    /// The job goes, with its runs and their output. A run of it that goes is stopped.
    pub fn lap_delete(&self, id: &str) -> Result<(), String> {
        let now = now_ms();
        {
            let mut laps = self.laps();
            laps.writable(now)?;
            let index = laps.laps.iter().position(|lap| lap.id == id).ok_or_else(not_found)?;
            let lap = laps.laps.remove(index);
            if !laps.save() {
                laps.laps.insert(index, lap);
                return Err(t!("core.laps.error.unsaved"));
            }
            if let Some(going) = laps.going.remove(id) {
                *going.stop.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Reason::Stopped);
            }
            laps.held.remove(id);
            if let Some(last) = laps.tabs.remove(id) {
                laps.loose.insert(last.tab, last);
            }
            laps.runs.remove(id);
            laps.store.forget(id);
        }
        self.laps_changed();
        Ok(())
    }

    /// The jobs in the user's order (the tower dragged), which is the order of `laps.json` and of
    /// the laps' state: those `ids` names first, as it names them, then those it doesn't name, in
    /// the order they had. Written as every change is; as it was when that can't be.
    pub fn lap_reorder(&self, ids: &[String]) -> Result<(), String> {
        let now = now_ms();
        {
            let mut laps = self.laps();
            laps.writable(now)?;
            let before = laps.laps.clone();
            laps.laps = in_order(before.clone(), ids);
            if laps.laps != before && !laps.save() {
                laps.laps = before;
                return Err(t!("core.laps.error.unsaved"));
            }
        }
        self.laps_changed();
        Ok(())
    }

    /// The switch. A job is switched on only as `lap_save` would take it.
    pub fn lap_switch(&self, id: &str, on: bool) -> Result<(), String> {
        if on {
            self.lap_fits(id)?;
        }
        self.lap_change(id, |lap| {
            lap.on = on;
            Ok(())
        })
    }

    /// Duraklat / Sürdür. Sürdür starts the failures in a row anew, for a job `lap_save` would
    /// take as it is.
    pub fn lap_pause(&self, id: &str, paused: bool) -> Result<(), String> {
        if !paused {
            self.lap_fits(id)?;
        }
        self.lap_change(id, |lap| {
            if paused {
                lap.paused = Some(PausedBy::User);
            } else {
                lap.paused = None;
                lap.streak = 0;
            }
            Ok(())
        })
    }

    /// The job `id` as it is kept passes the checks of `lap_save` (one from a backup or a
    /// hand-written `laps.json` may not), or why not.
    fn lap_fits(&self, id: &str) -> Result<(), String> {
        let lap = self.laps().lap(id).cloned().ok_or_else(not_found)?;
        self.lap_checked(lap, now_ms()).map(drop)
    }

    /// Şimdi çalıştır (and Şimdi dene, Yeniden çalıştır): a run now, off the schedule. Its
    /// record is written before anything starts; the checks hold, the day off doesn't.
    pub fn lap_run_now(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let planned = self.lap_begin_now(id, now_ms())?;
        let core = Arc::clone(self);
        thread::spawn(move || core.run_lap(planned));
        Ok(())
    }

    /// `lap_run_now`'s record: written, or why not.
    fn lap_begin_now(&self, id: &str, now: u64) -> Result<Planned, String> {
        let planned = {
            let mut laps = self.laps();
            laps.writable(now)?;
            let index = laps.laps.iter().position(|lap| lap.id == id).ok_or_else(not_found)?;
            if laps.going.contains_key(id) {
                return Err(t!("core.laps.error.running"));
            }
            let planned = laps.begin(index, now, true, now);
            if !laps.save_runs(id) {
                laps.going.remove(id);
                if let Some(runs) = laps.runs.get_mut(id) {
                    runs.retain(|run| run.id != planned.run);
                }
                return Err(t!("core.laps.error.unsaved"));
            }
            planned
        };
        self.laps_changed();
        Ok(planned)
    }

    /// Bu seferi atla: the next moment becomes a skipped run.
    pub fn lap_skip_next(&self, id: &str) -> Result<(), String> {
        let now = now_ms();
        {
            let mut laps = self.laps();
            laps.writable(now)?;
            let lap = laps.lap(id).cloned().ok_or_else(not_found)?;
            let Some(at) = lap.next_at else {
                return Err(t!("core.laps.error.noRun"));
            };
            let next = laps.next_moment(&lap, at.max(now));
            laps.lap_mut(id).ok_or_else(not_found)?.next_at = next;
            if !laps.save() {
                laps.lap_mut(id).ok_or_else(not_found)?.next_at = Some(at);
                return Err(t!("core.laps.error.unsaved"));
            }
            laps.skipped(id, at, Reason::UserSkip, now);
            laps.save_runs(id);
        }
        self.laps_changed();
        Ok(())
    }

    /// Stops the job's run in progress: its own thread ends what it started (`run.rs`).
    pub fn lap_stop(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut laps = self.laps();
        let going = laps.going.get(id).ok_or_else(|| t!("core.laps.error.noRun"))?;
        *going.stop.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Reason::Stopped);
        laps.held.remove(id);
        Ok(())
    }

    /// A day off: `date` alone, or (`every`) its weekday from now on, taken out of `when`.
    pub fn lap_day_off(&self, id: &str, date: &str, every: bool) -> Result<(), String> {
        let day = weekday_of(date).ok_or_else(|| t!("core.laps.error.date"))?;
        self.lap_change(id, |lap| {
            if every {
                lap.when = when::without_weekday(&lap.when, day).ok_or_else(|| t!("core.laps.error.weekday"))?;
                if !lap.weekdays_off.contains(&day) {
                    lap.weekdays_off.push(day);
                }
            } else if !lap.days_off.iter().any(|off| off == date) {
                lap.days_off.push(date.to_string());
            }
            Ok(())
        })
    }

    /// "geri aç": `date`'s day off, or its weekday's, is taken back.
    pub fn lap_day_on(&self, id: &str, date: &str) -> Result<(), String> {
        let day = weekday_of(date).ok_or_else(|| t!("core.laps.error.date"))?;
        self.lap_change(id, |lap| {
            if lap.days_off.iter().any(|off| off == date) {
                lap.days_off.retain(|off| off != date);
            } else if lap.weekdays_off.contains(&day) {
                lap.when = when::with_weekday(&lap.when, day);
                lap.weekdays_off.retain(|&off| off != day);
            }
            Ok(())
        })
    }

    /// The next four moments of `when` for a job of `kind`, or why it is refused. A refill's
    /// are Claude's (`lap_preview_for` names the agent).
    pub fn lap_preview(&self, when: &When, kind: LapKind) -> Result<Vec<u64>, String> {
        self.lap_preview_for(when, kind, Agent::Claude, None)
    }

    /// `lap_preview` for a job of `agent` with `account` (none: the one in use): a refill's
    /// moments are its refills as its account's last usage reading had them.
    pub fn lap_preview_for(&self, when: &When, kind: LapKind, agent: Agent, account: Option<String>) -> Result<Vec<u64>, String> {
        let now = now_ms();
        if let Some(key) = when::refused(when, kind, now) {
            return Err(t!(key));
        }
        Ok(match when {
            When::Refill => {
                let laps = self.laps();
                let lap = Lap { start: crate::start::Start { agent, ..Default::default() }, account, ..Default::default() };
                let mut refills: Vec<u64> = laps.fuel_of(&lap).map(|fuel| fuel.refills.iter().copied().filter(|&at| at > now).collect()).unwrap_or_default();
                refills.sort_unstable();
                refills.dedup();
                refills.truncate(4);
                refills
            }
            _ => when::upcoming(when, now, &[], 4),
        })
    }

    /// The month's days for the calendar popover.
    pub fn lap_calendar(&self, year: i32, month: u32) -> Vec<CalendarDay> {
        let now = now_ms();
        let prefix = format!("{year:04}-{month:02}-");
        let days: Vec<(String, u8)> = (1..=31).filter_map(|day| crate::clock::local_midnight(year, month, day)).map(when::date_of).filter(|(date, _)| date.starts_with(&prefix)).collect();
        let laps = self.laps();
        days.into_iter()
            .map(|(date, weekday)| {
                let mut of_day: Vec<(u64, RunResult)> = laps.runs.values().flatten().filter(|run| when::date_of(run.due).0 == date).filter_map(|run| Some((run.due, run.result?))).collect();
                of_day.sort_by_key(|(due, _)| *due);
                let results = of_day.into_iter().map(|(_, result)| result).collect();
                let planned = laps.laps.iter().map(|lap| laps.planned_on(lap, &date, now).len() as u32).sum();
                let off = laps.laps.iter().any(|lap| lap.days_off.contains(&date) || lap.weekdays_off.contains(&weekday));
                CalendarDay { date, results, planned, off }
            })
            .collect()
    }

    /// The jobs of one day (`YYYY-MM-DD`): those with moments, runs or a day off on it.
    pub fn lap_day(&self, date: &str) -> Vec<DayRow> {
        let Some(weekday) = weekday_of(date) else {
            return Vec::new();
        };
        let now = now_ms();
        let laps = self.laps();
        laps.laps
            .iter()
            .filter_map(|lap| {
                // Today's still to come; another day's all (none of a day past).
                let moments: Vec<u64> = if lap.on && lap.paused.is_none() {
                    match lap.when {
                        When::Refill => laps.fuel_of(lap).map(|fuel| fuel.refills.iter().copied().filter(|&at| when::date_of(at).0 == date).collect()).unwrap_or_default(),
                        _ => when::on_day(&lap.when, date),
                    }
                } else {
                    Vec::new()
                };
                let moments: Vec<u64> = moments.into_iter().filter(|&at| at > now).collect();
                let off = if lap.days_off.iter().any(|off| off == date) {
                    Some("day")
                } else if lap.weekdays_off.contains(&weekday) {
                    Some("weekday")
                } else {
                    None
                };
                let runs: Vec<RunMark> = laps
                    .runs
                    .get(&lap.id)
                    .into_iter()
                    .flatten()
                    .filter(|run| when::date_of(run.due).0 == date)
                    .map(|run| RunMark { id: run.id.clone(), due: run.due, result: run.result, unread: run.unread })
                    .collect();
                if moments.is_empty() && runs.is_empty() && off.is_none() {
                    return None;
                }
                let count = moments.len() as u32;
                let last = moments.last().copied();
                Some(DayRow { lap: lap.id.clone(), name: lap.name.clone(), at: moments.into_iter().take(12).collect(), count, last, off, runs })
            })
            .collect()
    }

    /// The job's kept runs, newest first.
    pub fn lap_runs(&self, id: &str) -> Vec<LapRun> {
        self.laps().runs.get(id).map(|runs| runs.iter().rev().cloned().collect()).unwrap_or_default()
    }

    /// What the agent of a run's session last said, read now (never kept), and whether its log
    /// is there at all. Nothing of an agent that is off is read.
    fn lap_message(&self, agent: Agent, session: &str) -> (Option<String>, bool) {
        if !is_session_id(session) || !self.agent_on(agent) {
            return (None, true);
        }
        match agent {
            Agent::Claude => match self.session_log(session) {
                Some((log, _)) => (crate::claude::last_message(&log), true),
                None => (None, false),
            },
            Agent::Codex => match self.codex_rollout(session) {
                Some((_, file, _)) => (crate::codex::message_in(&file), true),
                None => (None, false),
            },
        }
    }

    /// The first line of what the agent last said in each of `runs`, read now; for Son koşular.
    pub fn lap_summaries(&self, runs: &[String]) -> HashMap<String, String> {
        let sessions: Vec<(String, Agent, String)> = {
            let laps = self.laps();
            runs.iter().filter_map(|run| {
                let (_, record) = laps.find_run(run)?;
                Some((run.clone(), record.agent.unwrap_or(Agent::Claude), record.session.clone()?))
            })
            .collect()
        };
        sessions
            .into_iter()
            .filter_map(|(run, agent, session)| {
                let message = self.lap_message(agent, &session).0?;
                let line = message.lines().map(str::trim).find(|line| !line.is_empty())?;
                Some((run, line.chars().take(120).collect()))
            })
            .collect()
    }

    /// A run's result: its kept script output and what its agent last said, read now.
    pub fn lap_result(&self, run: &str) -> Result<LapResult, String> {
        let (record, command, script) = {
            let laps = self.laps();
            let (lap, record) = laps.find_run(run).ok_or_else(|| t!("core.laps.error.noRun"))?;
            let (command, script) = match record.script.then(|| laps.store.read_kept(&lap.id, run)).flatten() {
                Some((command, lines)) => (command, Some(lines)),
                None => (None, None),
            };
            (record.clone(), command, script)
        };
        let (message, found) = match &record.session {
            Some(session) => self.lap_message(record.agent.unwrap_or(Agent::Claude), session),
            None => (None, true),
        };
        Ok(LapResult { run: record, command, script, message, gone: !found })
    }

    /// A run's result was opened: it is read from now on, its record written and the page told.
    /// A run read already, or no longer kept, is no error. Should the record not be written, it
    /// stays read in memory: the user saw it.
    pub fn lap_read(&self, run: &str) -> Result<(), String> {
        let now = now_ms();
        {
            let mut laps = self.laps();
            let lap = match laps.find_run(run) {
                Some((lap, record)) if record.unread => lap.id.clone(),
                _ => return Ok(()),
            };
            laps.writable(now)?;
            let Some(record) = laps.record_mut(&lap, run).filter(|record| record.unread) else {
                return Ok(());
            };
            record.unread = false;
            laps.save_runs(&lap);
        }
        self.laps_changed();
        Ok(())
    }

    /// What a run's dialog shows in its terminal (Oturumu aç, or a run that goes, opened): the tab
    /// of the laps' own its agent runs in, as `LapTerminal` says. A run that goes: its tab, once
    /// its agent has one. A run that has ended: the job's last tab while its agent is still there,
    /// else the laps' tab its session runs in; a session that has ended opened again in a new tab
    /// of the laps' (`claude --resume`, `codex resume --no-daemon`), with the job's account and
    /// sized `size`, held as every opening is (`open_once`); one that runs anywhere else brought up
    /// there, as the Sessions page brings it. A Claude session is opened again only once it is
    /// known to have ended. A tab given is shown until the dialog lets go of it.
    pub fn lap_terminal(self: &Arc<Self>, run: &str, size: Option<(u16, u16)>) -> Result<LapTerminal, String> {
        let shown = self.lap_terminal_of(run, size)?;
        if let LapTerminal::Tab { id } = shown {
            self.laps().shown.insert(id);
        }
        Ok(shown)
    }

    fn lap_terminal_of(self: &Arc<Self>, run: &str, size: Option<(u16, u16)>) -> Result<LapTerminal, String> {
        let (record, folder, name, going, last) = {
            let laps = self.laps();
            let (lap, record) = laps.find_run(run).ok_or_else(|| t!("core.laps.error.noRun"))?;
            // On Windows a lap's agent always runs in the background (`run.rs`).
            let background = lap.place_run == Where::Background || cfg!(windows);
            let going = laps.going.get(&lap.id).filter(|going| going.run == run).map(|going| (going.tab, going.phase == "agent" && background));
            let last = laps.tabs.get(&lap.id).filter(|last| last.session.is_some() && last.session == record.session).map(|last| last.tab);
            (record.clone(), lap.place.path().to_string(), lap.name.clone(), going, last)
        };
        if let Some((tab, background)) = going {
            return Ok(match tab.filter(|&tab| self.terminal_sessions().contains_key(&tab)) {
                Some(id) => LapTerminal::Tab { id },
                None if background => LapTerminal::Background,
                None => LapTerminal::NotYet,
            });
        }

        let session = record.session.clone().filter(|session| is_session_id(session)).ok_or_else(|| t!("core.laps.error.noSession"))?;
        let agent = record.agent.unwrap_or(Agent::Claude);
        if !self.agent_on(agent) {
            return Err(t!(match agent {
                Agent::Claude => "board.agentOff.claude",
                Agent::Codex => "board.agentOff.codex",
            }));
        }
        // The run's own tab, its agent still there: the session goes on in it.
        if let Some(id) = last.filter(|&tab| self.terminal_sessions().contains_key(&tab) && self.lap_tab_holds(tab)) {
            return Ok(LapTerminal::Tab { id });
        }
        let liveness = match agent {
            Agent::Claude => self.liveness(&session),
            Agent::Codex => self.codex_liveness(&session),
        };
        if let Liveness::Running { lineage, .. } = &liveness {
            if let Some((id, path)) = self.pitwall_terminal(lineage) {
                if self.terminal_apart(id) {
                    return Ok(LapTerminal::Tab { id });
                }
                self.reveal_terminal(path, id);
                return Ok(LapTerminal::Elsewhere);
            }
            if agent == Agent::Claude {
                self.reveal_claude(&folder, &session);
                return Ok(LapTerminal::Elsewhere);
            }
        }
        let (launch, account) = match agent {
            Agent::Claude => {
                if !matches!(liveness, Liveness::Ended | Liveness::Stopped) {
                    return Err(t!(if self.session_log(&session).is_some() { "core.laps.error.sessionUnsure" } else { "core.laps.error.noSession" }));
                }
                (Launch::Resume { session: session.clone() }, record.account.clone().unwrap_or_else(|| crate::accounts::MAIN.to_string()))
            }
            Agent::Codex => {
                let (account, _, _) = self.codex_rollout(&session).ok_or_else(|| t!("core.laps.error.noSession"))?;
                (Launch::ResumeCodex { session: session.clone() }, account)
            }
        };
        // The run's own tab, its agent gone (its shell alone): the session goes on in a new one.
        if let Some(tab) = last {
            self.lap_terminal_left(tab);
        }
        let open = || self.open_terminal_apart(&folder, &launch, super::board::tab_name(&name), size, Some(&account));
        match self.open_once(&session, open) {
            // No run's and no job's last: closed once the dialog lets go and its agent is done.
            Opened::New(view) => {
                self.laps().loose.insert(view.id, LastTab { tab: view.id, session: Some(session.clone()), agent });
                Ok(LapTerminal::Tab { id: view.id })
            }
            Opened::Held(id) if self.terminal_apart(id) => Ok(LapTerminal::Tab { id }),
            Opened::Held(id) => {
                self.reveal_terminal(folder, id);
                Ok(LapTerminal::Elsewhere)
            }
            // Another opening of it is on its way.
            Opened::Opening => Err(t!("core.laps.error.sessionUnsure")),
            Opened::Failed(error) => Err(error),
        }
    }

    /// A run's dialog let go of tab `id` (it closed, or its terminal went from it). A run that
    /// goes keeps its tab (the run follows it), and so does a job's last tab while its agent is
    /// still there, for the dialog to come back to (`lap_close_last_tab` lets it go before the
    /// job's next run); closed once its agent has left, its shell alone. Any other of the laps'
    /// (`Laps::loose`) is closed once its agent is done, its turn over; one at work goes on and is
    /// closed when it is done (`lap_sweep_tabs`). A tab not of the laps' is none of this.
    pub fn lap_terminal_left(&self, id: u64) {
        let loose = {
            let mut laps = self.laps();
            laps.shown.remove(&id);
            if laps.going.values().any(|going| going.tab == Some(id)) {
                return;
            }
            laps.loose.get(&id).cloned()
        };
        if !self.terminal_apart(id) {
            return;
        }
        match loose {
            Some(last) => {
                if self.lap_tab_done(&last) {
                    self.laps().loose.remove(&id);
                    self.close_terminal(id);
                }
            }
            None => {
                if !self.lap_tab_holds(id) {
                    self.close_terminal(id);
                }
            }
        }
    }

    /// Something runs in the laps' tab `id` beyond its shell: what holds its foreground, or its
    /// agent, which may still be on its way there (`agent_may_run_in`).
    fn lap_tab_holds(&self, id: u64) -> bool {
        self.terminal_busy(id) || self.agent_may_run_in(id)
    }

    /// The questions and permissions the laps' sessions wait with (`held`: what the scan set
    /// apart, `set_apart` in `claude_state.rs`): no project's state has them, so they are told
    /// here as a project's would be, under the job's project or folder as the laps' banners are;
    /// each turn once. A permission names its tool alone; a finished turn is the lap's to tell
    /// (`LapDone`, `LapFailed`).
    pub(super) fn lap_waits(&self, held: &[crate::claude::SessionState]) -> Vec<Signal> {
        use crate::claude::TurnKind;

        let fresh: Vec<(SignalKind, Place, &crate::claude::SessionState)> = {
            let mut laps = self.laps();
            let placed: Vec<(SignalKind, Place, &crate::claude::SessionState, u64)> = held
                .iter()
                .filter(|state| state.phase == SessionPhase::Waiting)
                .filter_map(|state| {
                    let turn = state.turn?;
                    let kind = match turn.kind {
                        TurnKind::Asking => SignalKind::ClaudeAsking,
                        TurnKind::Permission => SignalKind::ClaudePermission,
                        TurnKind::Finished => return None,
                    };
                    Some((kind, laps.session_place(&state.id)?, state, turn.at))
                })
                .collect();
            let kept = laps.sessions();
            laps.waits_told.retain(|session, _| kept.contains(session));
            placed.into_iter().filter(|(_, _, state, at)| laps.waits_told.insert(state.id.clone(), *at) != Some(*at)).map(|(kind, place, state, _)| (kind, place, state)).collect()
        };
        fresh
            .into_iter()
            .map(|(kind, place, state)| {
                let path = place.path().to_string();
                let name = match place {
                    Place::Project { .. } => self.project_name(&path),
                    Place::Folder { .. } => Path::new(&path).file_name().map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned()),
                };
                let detail = state.tool.clone().filter(|_| kind == SignalKind::ClaudePermission);
                Signal { kind, path, name, detail, agent: state.agent }
            })
            .collect()
    }

    /// A run that goes in the folder `path`, one with a tab first: what its wait's banner
    /// (`lap_waits`) brings up (its dialog, `CoreEvent::RevealLap`) and how the phone names it, a
    /// folder no project's list has.
    pub fn lap_going_in(&self, path: &str) -> Option<String> {
        let laps = self.laps();
        let going: Vec<&Going> = laps.going.iter().filter(|(lap, _)| laps.lap(lap).is_some_and(|lap| same_path(lap.place.path(), path))).map(|(_, going)| going).collect();
        going.iter().find(|going| going.tab.is_some()).or(going.first()).map(|going| going.run.clone())
    }

    /// The process groups the laps' runs lead (a script, a background agent), each by its job's
    /// folder, for the cleanup after a crash (`record_pids`).
    pub(super) fn lap_groups(&self) -> Vec<crate::registry::PidEntry> {
        let laps = self.laps();
        laps.going.iter().filter_map(|(id, going)| Some(crate::registry::PidEntry { path: laps.lap(id)?.place.path().to_string(), pid: going.pgid? })).collect()
    }

    /// The sessions the laps' runs hold, going or kept: a layer of their own, which no project's
    /// state, count or list takes in (`set_apart` in `claude_state.rs`) and no card takes
    /// (`codex_adopt`).
    pub(super) fn lap_sessions(&self) -> HashSet<String> {
        self.laps().sessions()
    }

    /// A session as the last scan has it: the laps' own first (`lap_scanned`), else any other
    /// (one a run took on after that scan).
    fn lap_state(&self, session: &str) -> Option<crate::claude::SessionState> {
        let inner = self.lock();
        inner.lap_scanned.iter().chain(&inner.scanned).find(|state| state.id == session).cloned()
    }

    /// The job named `name` in the folder `path` paused itself after failures and runs nothing
    /// now: its `LapFailed` says it paused.
    pub fn lap_paused_now(&self, path: &str, name: &str) -> bool {
        let laps = self.laps();
        laps.laps.iter().any(|lap| same_path(lap.place.path(), path) && lap.name == name && lap.paused == Some(PausedBy::Failures) && !laps.going.contains_key(&lap.id))
    }

    /// The agent's accounts and each one's reading (`fuel.rs`): the week share used (0–100) and
    /// the next refills (ms), none for an account not read yet; `in_use`, the one a job that names
    /// none runs with. The same readings twice change nothing; refills already past are dropped. A
    /// refill job that isn't running takes its next refill as these readings have it: a sooner
    /// one, the same one moved, none once its account has no reading. A moment already due is
    /// left to the loop.
    pub fn laps_fuel(&self, agent: Agent, in_use: &str, readings: Vec<(String, Option<FuelReading>)>) {
        let now = now_ms();
        let accounts = readings
            .into_iter()
            .map(|(id, reading)| {
                let fuel = reading.map(|(week, refills)| {
                    let mut refills: Vec<u64> = refills.into_iter().filter(|&at| at > now).collect();
                    refills.sort_unstable();
                    refills.dedup();
                    Fuel { week, refills }
                });
                (id, fuel)
            })
            .collect();
        let fuel = AgentFuel { in_use: in_use.to_string(), accounts };
        let changed = {
            let mut laps = self.laps();
            if laps.fuel.get(&agent) == Some(&fuel) {
                return;
            }
            laps.fuel.insert(agent, fuel);
            if laps.lock.is_none() || laps.quit {
                false
            } else {
                let mut changed = false;
                for index in 0..laps.laps.len() {
                    let lap = &laps.laps[index];
                    if lap.when != When::Refill || lap.start.agent != agent || laps.going.contains_key(&lap.id) || lap.next_at.is_some_and(|at| at <= now) {
                        continue;
                    }
                    let next = laps.next_moment(lap, now);
                    if next != lap.next_at {
                        laps.laps[index].next_at = next;
                        changed = true;
                    }
                }
                if changed {
                    laps.save();
                }
                changed
            }
        };
        if changed {
            self.laps_changed();
        }
    }

    /// Every job as a backup holds it: without its account, days off, times and counts
    /// (`backup.rs`).
    pub(crate) fn laps_export(&self) -> Vec<Lap> {
        self.laps()
            .laps
            .iter()
            .map(|lap| Lap { account: None, days_off: Vec::new(), weekdays_off: Vec::new(), next_at: None, streak: 0, runs_done: 0, ..lap.clone() })
            .collect()
    }

    /// Jobs from a backup (`backup.rs`), each checked as `lap_save` checks one (`Check::Import`,
    /// against `known`: the projects and commands as the import leaves them), a refused one
    /// skipped: each one taken replaces a job of the same id or is added, comes in paused by the
    /// user with no next moment, and is written. How many were.
    pub(crate) fn laps_import(&self, imported: Vec<Lap>, known: &Known) -> Result<usize, String> {
        let now = now_ms();
        let imported: Vec<Lap> = imported.into_iter().filter_map(|mut lap| self.lap_refused(&mut lap, now, known, Check::Import).ok().map(|()| lap)).collect();
        if imported.is_empty() {
            return Ok(0);
        }
        let count = {
            let mut laps = self.laps();
            laps.writable(now)?;
            let before = laps.laps.clone();
            let count = imported.len();
            for mut lap in imported {
                if !store::own_id(&lap.id) {
                    lap.id = laps.new_id("l", now);
                }
                lap.paused = Some(PausedBy::User);
                lap.next_at = None;
                if lap.created_at == 0 {
                    lap.created_at = now;
                }
                laps.held.remove(&lap.id);
                match laps.lap_mut(&lap.id) {
                    Some(kept) => *kept = lap,
                    None => laps.laps.push(lap),
                }
            }
            if !laps.save() {
                laps.laps = before;
                return Err(t!("core.laps.error.unsaved"));
            }
            count
        };
        self.laps_changed();
        Ok(count)
    }

    /// `laps.json` as it is now, for the copy an import makes first; none when there is none.
    pub(crate) fn laps_file(&self) -> Option<PathBuf> {
        Some(self.laps().store.laps_file()).filter(|file| file.is_file())
    }

    /// A project's folder moved (`rehome_project`): its jobs, and those of folders inside it,
    /// go with it.
    pub(crate) fn laps_rehome(&self, from: &str, to: &str) {
        self.laps_move(|place| match place {
            Place::Project { path } if same_path(path, from) => Some(Place::Project { path: to.to_string() }),
            Place::Folder { path } => {
                let rest = Path::new(path.as_str()).strip_prefix(from).ok()?;
                Some(Place::Folder { path: Path::new(to).join(rest).to_string_lossy().into_owned() })
            }
            Place::Project { .. } => None,
        });
    }

    /// A project forgotten (`forget_project`): its jobs stay, paused, as jobs of its folder.
    pub(crate) fn laps_forget(&self, path: &str) {
        self.laps_move(|place| match place {
            Place::Project { path: of } if same_path(of, path) => Some(Place::Folder { path: of.clone() }),
            _ => None,
        });
    }

    fn laps_move(&self, moved: impl Fn(&Place) -> Option<Place>) {
        let now = now_ms();
        let changed = {
            let mut laps = self.laps();
            if laps.writable(now).is_err() {
                return;
            }
            let mut changed = false;
            for lap in &mut laps.laps {
                if let Some(place) = moved(&lap.place) {
                    if matches!((&lap.place, &place), (Place::Project { .. }, Place::Folder { .. })) {
                        lap.paused = Some(PausedBy::User);
                        lap.next_at = None;
                    }
                    lap.place = place;
                    changed = true;
                }
            }
            if changed {
                laps.save();
            }
            changed
        };
        if changed {
            self.laps_changed();
        }
    }

    /// The loop's second (`Core::tick`): what is due starts, each run on a thread of its own.
    pub(super) fn laps_due(self: &Arc<Self>) {
        self.laps_due_at(now_ms(), &mut |core, planned| {
            let core = Arc::clone(core);
            thread::spawn(move || core.run_lap(planned));
        });
    }

    /// At a quit: what runs is let go of, its records written `cut / quit`, its processes
    /// stopped, the awake holds and the lock let go.
    pub fn laps_quit(&self) {
        let now = now_ms();
        let groups: Vec<u32> = {
            let mut laps = self.laps();
            if laps.quit {
                return;
            }
            laps.quit = true;
            let going: Vec<(String, Going)> = laps.going.drain().collect();
            let mut groups = Vec::new();
            for (lap, going) in going {
                *going.stop.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Reason::Quit);
                groups.extend(going.pgid);
                if let Some(record) = laps.record_mut(&lap, &going.run) {
                    record.ended_at = Some(now);
                    record.result = Some(RunResult::Cut);
                    record.reason = Some(Reason::Quit);
                    record.tab = None;
                }
                if laps.lock.is_some() {
                    laps.save_runs(&lap);
                }
                drop(going.awake);
            }
            laps.held.clear();
            laps.lock = None;
            groups
        };
        if !groups.is_empty() {
            process::stop_groups(&groups, Duration::from_millis(600), Duration::from_millis(300));
        }
    }
}

/// `laps` in the order `ids` names them, then those it doesn't name as they stood: each job once,
/// none lost, an id it doesn't know skipped and one named twice taken at its first place.
fn in_order(laps: Vec<Lap>, ids: &[String]) -> Vec<Lap> {
    let mut left: Vec<Option<Lap>> = laps.into_iter().map(Some).collect();
    let mut order = Vec::with_capacity(left.len());
    for id in ids {
        if let Some(lap) = left.iter_mut().find(|lap| lap.as_ref().is_some_and(|lap| &lap.id == id)).and_then(Option::take) {
            order.push(lap);
        }
    }
    order.extend(left.into_iter().flatten());
    order
}

/// The weekday (1–7) of a date `YYYY-MM-DD`; none for what isn't one.
fn weekday_of(date: &str) -> Option<u8> {
    let mut parts = date.split('-');
    let (year, month, day) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    if parts.next().is_some() || date.len() != 10 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (of, weekday) = when::date_of(crate::clock::local_midnight(year, month, day)?);
    (of == date).then_some(weekday)
}

#[cfg(test)]
mod tests;

