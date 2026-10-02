//! The app's core: dev servers, the shared registry and Claude sessions, without any Tauri
//! types so it can be driven end to end from tests. The Tauri layer listens to `CoreEvent`s.
//!
//! Lifecycle rules follow the extension's `src/runner.ts`: login shell, whole process tree,
//! silent free port, crash restart after 3 s, give up after 3 failures within 60 s, health
//! probe every 30 s.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::claude::{ClaudeWatch, SessionPhase, Turn, TurnKind};
use crate::git::{self, GitInfo};
use crate::hooks::ClaudeHook;
use crate::i18n::{self, t};
use crate::ports::{is_port_served, Reservations};
use crate::process::{self, kill_tree};
use crate::registry::{now_ms, Favourite, Issue, PidEntry, ProjectState, Registry, RemoteCommand, WindowRecord};
use crate::resolve::{
    build_command, detect_error_line, detect_package_manager, detect_port_conflict, extract_local_url, has_script,
    herd_fallback_url, is_forbidden_script, parse_app_url, parse_vite_port, port_from_url, PackageManager,
};
use crate::settings::{ProjectSettings, Settings};

#[path = "activity.rs"]
mod activity;
#[path = "jobs.rs"]
mod jobs;
#[path = "reveal.rs"]
mod reveal;
#[path = "supervise.rs"]
mod supervise;
#[path = "terminal.rs"]
mod terminal;
pub use activity::DaySummary;
pub use terminal::{TerminalBuffer, TerminalView};
pub use jobs::CommandView;
use jobs::{job_key, Job, JobResult};
use supervise::{Supervisor, Unit};

const RESTART_DELAY: Duration = Duration::from_secs(3);
const MAX_RESTARTS: u32 = 3;
const STABLE: Duration = Duration::from_secs(60);
const OUTPUT_LINES: usize = 500;
/// Output reaches the UI in batches, one per project and command at most this often.
const OUTPUT_BATCH: Duration = Duration::from_millis(50);
/// Most bytes read from a followed output file at once; a bigger burst skips to its newest part.
const TAIL_CHUNK: u64 = 256 * 1024;
/// A "line" that never ends is cut here.
const TAIL_LINE_MAX: usize = 64 * 1024;
const HEARTBEAT_EVERY: u64 = 5;
const HEALTH_EVERY: u64 = 30;
const SWEEP_EVERY: u64 = 60;
const GIT_EVERY: u64 = 10;
/// A waiting turn is announced only if it is still waiting this long after it was first seen:
/// a focused VS Code window marks it seen in the meantime.
const NOTIFY_AFTER: Duration = Duration::from_secs(4);
/// A start counts as done once the server printed its address, or after this long.
const SETTLE_LIMIT: Duration = Duration::from_secs(15);
/// The spinner never outlives this, whatever happens.
const PENDING_LIMIT: Duration = Duration::from_secs(30);

pub enum CoreEvent {
    /// The snapshot changed; read it with `Core::snapshot`.
    State,
    /// Lines of output, batched: the dev server's (`job` empty) or a custom command's.
    Output { path: String, job: Option<String>, lines: Vec<String> },
    /// The core wants a URL opened (browser or editor).
    Open(String),
    /// A system notification: Claude waits in this project.
    Notify { path: String, title: String, body: String },
    /// Play one of Pitwall's sounds (`sound.rs`), by id.
    Sound(String),
    /// Output of a project terminal.
    Terminal { id: u64, seq: u64, data: String },
    /// A project terminal ended.
    TerminalExit { id: u64 },
}

pub type Sink = Arc<dyn Fn(CoreEvent) + Send + Sync>;

pub struct CoreConfig {
    pub registry_dir: PathBuf,
    pub claude_dir: PathBuf,
    pub settings_file: PathBuf,
    /// `~/.claude/settings.json`, where the Notification hook goes.
    pub claude_settings: PathBuf,
    /// Registry folders of older extension versions, read for windows not yet reloaded.
    pub legacy_registries: Vec<PathBuf>,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub id: String,
    pub title: String,
}

/// A Claude session that is working, or waits on you (its notification not read yet).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveSession {
    pub id: String,
    /// `working` | `waiting`
    pub phase: SessionPhase,
    /// While it waits: the turn it waits with.
    pub turn: Option<Turn>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub path: String,
    pub name: String,
    pub favourite: bool,
    /// `running` | `stopped` | `crashed` | `busy`
    pub status: &'static str,
    /// While busy: `starting…` | `stopping…` | `restarting…`
    pub phase: Option<&'static str>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub started_at: Option<u64>,
    pub issue: Option<Issue>,
    /// Who runs it when it isn't this app (a VS Code window).
    pub owner: Option<Owner>,
    /// The editor window that has it open as a root folder.
    pub open_in: Option<String>,
    pub claude: Option<Turn>,
    /// Claude is mid-turn in this project right now.
    pub claude_working: bool,
    /// Its Claude sessions that are working or wait on you.
    pub claude_sessions: Vec<LiveSession>,
    pub git: Option<GitInfo>,
    pub script: String,
    /// The command its dev server runs here, or would: `pnpm run dev`, `npm run dev -- --port 5174`.
    pub run_command: String,
    pub settings: ProjectSettings,
    /// The project's custom commands and their state.
    pub commands: Vec<CommandView>,
    /// The project's open terminals.
    pub terminals: Vec<TerminalView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub projects: Vec<ProjectView>,
    pub running: usize,
    pub waiting: usize,
    pub crashed: usize,
    /// A server crashed since the popover was last opened.
    pub crash_unseen: bool,
    /// The Claude Code Notification hook is installed (permission prompts are caught).
    pub claude_hook: bool,
    pub settings: Settings,
    /// The language the UI speaks: Settings' choice, else the system's.
    pub language: &'static str,
    /// The system's language, for the "System" choice in Settings.
    pub system_language: &'static str,
}

/// Whether an issue ends a start the user waits on: a refused or failed start, or a final
/// give-up, does; a crash or a hang on its way back doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueEnd {
    Final,
    Retrying,
}

/// A start/stop/restart the user asked for and hasn't seen finish yet; drives the spinner.
struct Pending {
    action: Action,
    at: Instant,
    at_ms: u64,
}

struct Run {
    id: u64,
    pid: u32,
    name: String,
    /// The command line it runs.
    command: String,
    started: Instant,
    started_at: u64,
    port: Option<u16>,
    url: Option<String>,
    buffer: String,
    open_url: bool,
    settled: bool,
}

#[derive(Default)]
struct Inner {
    settings: Settings,
    runs: HashMap<String, Run>,
    issues: HashMap<String, Issue>,
    retried: HashSet<String>,
    busy: HashSet<String>,
    /// Servers and commands being stopped, by `Unit::key`: their exit is no crash.
    stopping: HashSet<String>,
    supervisor: Supervisor,
    /// Each project's package manager as its lock file tells, read once and again at each start.
    managers: RefCell<HashMap<String, PackageManager>>,
    output: HashMap<String, VecDeque<String>>,
    peers: Vec<WindowRecord>,
    favourites: Vec<Favourite>,
    waiting: HashMap<String, Turn>,
    working: HashSet<String>,
    /// Sessions working or waiting on you, per project.
    claude_sessions: HashMap<String, Vec<LiveSession>>,
    git: HashMap<String, GitInfo>,
    claude_hook: bool,
    claude_scanned: bool,
    /// Turn timestamps already announced, per project.
    notified: HashMap<String, u64>,
    /// Waiting turns seen once, not announced yet: (turn time, first seen).
    candidates: HashMap<String, (u64, Instant)>,
    pending: HashMap<String, Pending>,
    /// Running custom commands, by `job_key(path, id)`.
    jobs: HashMap<String, Job>,
    job_results: HashMap<String, JobResult>,
    /// Servers another participant runs, whose mirrored output we follow; by project path.
    tails: HashMap<String, Tail>,
    /// Open project terminals, in the order they were opened.
    terminal_list: Vec<TerminalView>,
    next_run: u64,
    disposed: bool,
    crash_unseen: bool,
    last_state: String,
    /// Each project's server status as last published, to hear it crash or come up.
    statuses: HashMap<String, &'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: String,
}

/// Where we are in another participant's output file.
struct Tail {
    file: PathBuf,
    offset: u64,
    /// Bytes after the last complete line.
    rest: Vec<u8>,
}

pub struct Core {
    pub registry: Registry,
    cfg: CoreConfig,
    inner: Mutex<Inner>,
    /// Project terminals' processes; apart from `inner` so typing never waits on it.
    terminals: Mutex<HashMap<u64, terminal::Session>>,
    /// What the day's timeline has written down last.
    activity: Mutex<activity::Recorder>,
    reservations: Mutex<Reservations>,
    claude: Mutex<ClaudeWatch>,
    hook: ClaudeHook,
    git_busy: AtomicBool,
    /// Held while the pids file is built and written, so it is never stale or torn.
    pids_lock: Mutex<()>,
    outbox: Arc<Mutex<Outbox>>,
    sink: Sink,
}

fn folder_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

/// Output lines on their way to the UI, gathered per project and command.
#[derive(Default)]
struct Outbox {
    batches: Vec<(String, Option<String>, Vec<String>)>,
    /// A flush is on its way.
    due: bool,
}

/// Sends the gathered output, in the order it came.
fn flush_outbox(outbox: &Mutex<Outbox>, sink: &Sink) {
    let mut outbox = outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Sent under the lock, so a batch never overtakes the one before it.
    for (path, job, lines) in outbox.batches.drain(..) {
        sink(CoreEvent::Output { path, job, lines });
    }
}

impl Core {
    pub fn new(cfg: CoreConfig, sink: Sink) -> Arc<Self> {
        let registry = Registry::new(cfg.registry_dir.clone(), &cfg.title).with_legacy(cfg.legacy_registries.clone());
        let legacy_seen = cfg.legacy_registries.iter().map(|dir| dir.join("claude-seen.json")).collect();
        let claude = ClaudeWatch::new(cfg.claude_dir.clone(), &cfg.registry_dir).with_legacy_seen(legacy_seen);
        let hook = ClaudeHook::new(cfg.claude_settings.clone(), &cfg.registry_dir);
        let claude_hook = hook.installed();
        let settings = Settings::load(&cfg.settings_file);
        i18n::set(i18n::resolve(&settings.language));
        let favourites = registry.read_favourites();
        let peers = registry.read_peers();

        Arc::new(Self {
            registry,
            cfg,
            inner: Mutex::new(Inner { settings, favourites, peers, claude_hook, ..Inner::default() }),
            terminals: Mutex::new(HashMap::new()),
            activity: Mutex::new(activity::Recorder::default()),
            reservations: Mutex::new(Reservations::default()),
            claude: Mutex::new(claude),
            hook,
            git_busy: AtomicBool::new(false),
            pids_lock: Mutex::new(()),
            outbox: Arc::new(Mutex::new(Outbox::default())),
            sink,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn emit(&self, event: CoreEvent) {
        (self.sink)(event);
    }

    /* ---------- snapshot ---------- */

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.lock();
        build_snapshot(&inner)
    }

    /// Emits `State` only when the snapshot actually changed.
    pub fn notify(&self) {
        let (changed, sounds) = {
            let mut inner = self.lock();
            prune_pending(&mut inner);
            let snapshot = build_snapshot(&inner);
            let sounds = server_sounds(&mut inner, &snapshot.projects);
            let json = serde_json::to_string(&snapshot).unwrap_or_default();

            if json == inner.last_state {
                (false, sounds)
            } else {
                inner.last_state = json;
                (true, sounds)
            }
        };

        for sound in sounds {
            self.emit(CoreEvent::Sound(sound));
        }
        if changed {
            self.emit(CoreEvent::State);
        }
    }

    pub fn output(&self, path: &str) -> Vec<String> {
        self.lock().output.get(path).map(|lines| lines.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn popover_opened(&self) {
        self.lock().crash_unseen = false;
        self.notify();
    }

    /* ---------- projects ---------- */

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

    /* ---------- actions, routed between participants ---------- */

    /// Start, stop or restart a project wherever it belongs. A server another participant runs
    /// is handled by that participant; a project open in a VS Code window is started there.
    pub fn act(self: &Arc<Self>, path: &str, action: Action) {
        self.mark_pending(path, action);

        let core = Arc::clone(self);
        let path = path.to_string();

        thread::spawn(move || core.act_now(&path, action));
    }

    fn mark_pending(&self, path: &str, action: Action) {
        self.lock().pending.insert(path.to_string(), Pending { action, at: Instant::now(), at_ms: now_ms() });
        self.notify();
    }

    fn has_pending(&self) -> bool {
        !self.lock().pending.is_empty()
    }

    fn act_now(self: &Arc<Self>, path: &str, action: Action) {
        let (running_here, stoppable, runner, root_owner) = {
            let inner = self.lock();
            let runner = inner
                .peers
                .iter()
                .find(|peer| peer.projects.iter().any(|p| p.folder_path == path && p.running))
                .map(|peer| peer.window_id.clone());
            let root_owner = inner.peers.iter().find(|peer| peer.has_root(path)).map(|peer| peer.window_id.clone());

            // A start under way or a crash restart waiting is ours to stop as well.
            let running_here = inner.runs.contains_key(path);
            (running_here, running_here || inner.supervisor.pending(path), runner, root_owner)
        };

        match action {
            Action::Start if running_here || runner.is_some() => {}
            Action::Start => match root_owner {
                Some(owner) => self.registry.send(&owner, "start", path),
                None => self.start(path, true),
            },
            Action::Stop if stoppable => self.stop(path),
            Action::Restart if stoppable => self.restart(path),
            Action::Stop | Action::Restart => {
                if let Some(owner) = runner {
                    self.registry.send(&owner, action.as_str(), path);
                } else if action == Action::Restart {
                    self.act_now(path, Action::Start);
                }
            }
        }
    }

    pub fn start_all(self: &Arc<Self>) {
        let idle: Vec<String> =
            self.snapshot().projects.into_iter().filter(|p| p.status != "running" && p.status != "busy").map(|p| p.path).collect();
        for path in &idle {
            self.mark_pending(path, Action::Start);
        }

        let core = Arc::clone(self);

        thread::spawn(move || {
            for (index, path) in idle.iter().enumerate() {
                if index > 0 {
                    thread::sleep(Duration::from_secs(1));
                }
                core.act_now(path, Action::Start);
            }
        });
    }

    pub fn stop_all(self: &Arc<Self>) {
        let mut paths: Vec<String> = self.snapshot().projects.into_iter().filter(|p| p.status == "running").map(|p| p.path).collect();
        for path in self.lock().supervisor.pending_servers() {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }

        for path in paths {
            self.act(&path, Action::Stop);
        }
    }

    /// Work orders from other participants addressed to us.
    pub fn handle_command(self: &Arc<Self>, command: RemoteCommand) {
        let Some(action) = Action::parse(&command.action) else {
            return;
        };

        let core = Arc::clone(self);

        thread::spawn(move || match action {
            Action::Start => core.start(&command.folder_path, false),
            Action::Stop => core.stop(&command.folder_path),
            Action::Restart => core.restart(&command.folder_path),
        });
    }

    /* ---------- dev servers ---------- */

    /// A start by hand resets the restart counter.
    pub fn start(self: &Arc<Self>, path: &str, open_url: bool) {
        self.lock().supervisor.reset_attempts(path);
        let open = open_url && self.lock().settings.open_url_on_start;
        self.launch(path, open, Vec::new());

        if self.lock().runs.contains_key(path) {
            self.start_server_companions(path);
        }
    }

    pub fn restart(self: &Arc<Self>, path: &str) {
        self.stop(path);
        self.start(path, false);
    }

    /// `end` says whether the spinner of a start the user waits on stops here.
    fn set_issue(&self, path: &str, kind: &str, text: String, end: IssueEnd) {
        {
            let mut inner = self.lock();
            inner.issues.insert(path.to_string(), Issue { kind: kind.into(), text });
            if end == IssueEnd::Final {
                inner.pending.remove(path);
            }
        }
        self.notify();
    }

    fn push_line(&self, path: &str, line: String) {
        {
            let mut inner = self.lock();
            let lines = inner.output.entry(path.to_string()).or_default();
            lines.push_back(line.clone());
            while lines.len() > OUTPUT_LINES {
                lines.pop_front();
            }
        }

        self.send_output(path, None, line);
    }

    /// Queues a line for the UI. The first line of a batch sends the batch `OUTPUT_BATCH` later,
    /// with whatever else came for any project or command meanwhile.
    fn send_output(&self, path: &str, job: Option<&str>, line: String) {
        let mut outbox = self.outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        match outbox.batches.iter_mut().find(|(p, j, _)| p == path && j.as_deref() == job) {
            Some((_, _, lines)) => lines.push(line),
            None => outbox.batches.push((path.to_string(), job.map(str::to_string), vec![line])),
        }

        if !outbox.due {
            outbox.due = true;
            let (pending, sink) = (Arc::clone(&self.outbox), Arc::clone(&self.sink));

            thread::spawn(move || {
                thread::sleep(OUTPUT_BATCH);
                pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).due = false;
                flush_outbox(&pending, &sink);
            });
        }
    }

    /// Sends the queued output at once: a process ended, its last lines shouldn't wait.
    fn flush_output(&self) {
        flush_outbox(&self.outbox, &self.sink);
    }

    fn launch(self: &Arc<Self>, path: &str, open_url: bool, extra_args: Vec<String>) {
        // Claimed before the slow part, so a second start meanwhile does nothing.
        let Some(start) = self.begin_start(&Unit::Server(path.to_string())) else {
            return;
        };
        let (settings, project) = {
            let inner = self.lock();
            (inner.settings.clone(), inner.settings.project(path))
        };

        let script = settings.script_for(path);

        if is_forbidden_script(&script) {
            return self.set_issue(path, "error", t!("core.error.forbiddenScript", script = script), IssueEnd::Final);
        }

        let package_json = fs::read_to_string(Path::new(path).join("package.json")).unwrap_or_default();

        if !has_script(&package_json, &script) {
            return self.set_issue(path, "error", t!("core.error.noScript", script = script), IssueEnd::Final);
        }

        let manager = PackageManager::parse(&settings.package_manager).unwrap_or_else(|| {
            let detected = detect_manager(path);
            self.lock().managers.borrow_mut().insert(path.to_string(), detected);
            detected
        });

        let (args, port) = if extra_args.is_empty() {
            let base = project.port.filter(|p| *p > 0).unwrap_or_else(|| vite_port(path));
            let free = self.reservations.lock().ok().and_then(|mut r| r.reserve_free(base));

            match free {
                Some(free) if free != base => (vec!["--port".to_string(), free.to_string()], Some(free)),
                _ => (Vec::new(), Some(base)),
            }
        } else {
            let port = extra_args.iter().skip_while(|a| *a != "--port").nth(1).and_then(|p| p.parse().ok());
            (extra_args, port)
        };

        let command = match build_command(manager, &script, &args) {
            Ok(command) => command,
            Err(message) => return self.set_issue(path, "error", message, IssueEnd::Final),
        };

        let child = match process::spawn_shell(&command, path) {
            Ok(child) => child,
            Err(error) => return self.set_issue(path, "crashed", t!("core.issue.couldNotStart", error = error), IssueEnd::Final),
        };

        let adopted = self.adopt(start, child, |inner, id, pid| {
            inner.output.insert(path.to_string(), VecDeque::new());
            inner.issues.remove(path);
            inner.runs.insert(
                path.to_string(),
                Run {
                    id,
                    pid,
                    name: folder_name(path),
                    command: command.clone(),
                    started: Instant::now(),
                    started_at: now_ms(),
                    port,
                    url: None,
                    buffer: String::new(),
                    open_url,
                    settled: false,
                },
            );
        });
        let Some((run_id, child)) = adopted else {
            return;
        };

        self.push_line(path, format!("$ {command}"));
        self.push_line(path, format!("  {path}"));
        self.record_pids();
        self.notify();
        self.watch(Unit::Server(path.to_string()), run_id, child);
    }

    fn on_line(self: &Arc<Self>, path: &str, run_id: u64, line: String) {
        let clean = crate::resolve::strip_ansi(&line);
        let mut open: Option<Option<String>> = None;
        let mut conflict_port: Option<u16> = None;
        let mut changed = false;

        {
            let mut inner = self.lock();
            let Some(run) = inner.runs.get_mut(path).filter(|run| run.id == run_id) else {
                return;
            };

            run.buffer.push_str(&clean);
            run.buffer.push('\n');
            if run.buffer.len() > 4096 {
                let cut = run.buffer.len() - 4096;
                let cut = (cut..run.buffer.len()).find(|i| run.buffer.is_char_boundary(*i)).unwrap_or(0);
                run.buffer.drain(..cut);
            }

            if let Some(conflict) = detect_port_conflict(&run.buffer) {
                run.buffer.clear();
                if conflict.fatal && conflict.port > 0 && !inner.retried.contains(path) {
                    inner.retried.insert(path.to_string());
                    conflict_port = Some(conflict.port);
                }
            } else if let Some(url) = extract_local_url(&run.buffer) {
                run.buffer.clear();
                if !run.settled {
                    run.settled = true;
                    run.port = port_from_url(&url).or(run.port);
                    run.url = Some(url.clone());
                    changed = true;
                    if run.open_url {
                        open = Some(Some(url));
                    }
                }
            }

            if let Some(error) = detect_error_line(&clean, 90) {
                let current = inner.issues.get(path).map(|i| i.kind.clone());
                if current.as_deref() != Some("crashed") {
                    inner.issues.insert(path.to_string(), Issue { kind: "error".into(), text: error });
                    changed = true;
                }
            }
        }

        // Shown as plain text: colour codes from tools that ignore FORCE_COLOR are dropped.
        self.push_line(path, clean);

        if changed {
            self.notify();
        }

        if let Some(Some(local)) = open {
            self.show_in_browser(path, Some(local), true);
        }

        if let Some(port) = conflict_port {
            // The server died on a busy port: move it to a free one, once per project.
            let core = Arc::clone(self);
            let path = path.to_string();

            thread::spawn(move || {
                let free = core.reservations.lock().ok().and_then(|mut r| r.reserve_free(port.saturating_add(1)));
                if let Some(free) = free {
                    core.stop_server(&path);
                    core.launch(&path, false, vec!["--port".into(), free.to_string()]);
                }
            });
        }
    }

    /// The server's shell exited (taken out of `runs` already); `planned` when a stop did it.
    fn on_exit(self: &Arc<Self>, path: &str, started: Instant, planned: bool, disposed: bool, status: Option<std::process::ExitStatus>) {
        let (code, signal) = describe_exit(status);

        self.push_line(path, format!("[pitwall] {}", t!("core.log.processEnded", code = code, signal = signal)));
        self.flush_output();
        self.record_pids();
        self.notify();

        if planned || disposed {
            return;
        }

        self.handle_crash(path, started, &code, &signal);
    }

    fn handle_crash(self: &Arc<Self>, path: &str, started: Instant, code: &str, signal: &str) {
        let detail = {
            let inner = self.lock();
            match inner.issues.get(path) {
                Some(issue) if issue.kind == "error" => issue.text.clone(),
                _ if code != "-" => t!("core.issue.exitCode", code = code),
                _ => t!("core.issue.exitSignal", signal = signal),
            }
        };

        self.lock().crash_unseen = true;
        self.record_crash(path);

        let unit = Unit::Server(path.to_string());
        let Some(attempt) = self.claim_restart(&unit, started) else {
            self.set_issue(path, "crashed", t!("core.issue.crashedGaveUp", detail = detail, max = MAX_RESTARTS), IssueEnd::Final);
            self.push_line(path, format!("[pitwall] {}", t!("core.log.gaveUpServer", max = MAX_RESTARTS)));
            return;
        };

        let text = t!("core.issue.crashedRestarting", detail = detail, attempt = attempt, max = MAX_RESTARTS);
        self.set_issue(path, "crashed", text, IssueEnd::Retrying);
        self.push_line(path, format!("[pitwall] {}", t!("core.log.crashedRestarting", attempt = attempt, max = MAX_RESTARTS)));
        self.restart_later(&unit);
    }

    /// Stops the dev server and the commands that run along with it.
    pub fn stop(self: &Arc<Self>, path: &str) {
        self.stop_server(path);
        self.stop_server_companions(path);
    }

    /// The dev server only; recovery paths restart it without touching its companions.
    fn stop_server(self: &Arc<Self>, path: &str) {
        self.stop_unit(&Unit::Server(path.to_string()));
    }

    /// Quit: every server goes, nothing is left behind.
    pub fn dispose(&self) {
        self.record_app("stop");

        let mut pids: Vec<u32> = {
            let mut inner = self.lock();
            inner.disposed = true;
            let lingering: Vec<u32> = inner.supervisor.lingering_groups().into_iter().map(|(pgid, _)| pgid).collect();
            inner.runs.values().map(|run| run.pid).chain(inner.jobs.values().map(|job| job.pid())).chain(lingering).collect()
        };
        pids.extend(self.terminal_pids());
        pids.sort_unstable();
        pids.dedup();

        if !pids.is_empty() {
            process::stop_groups(&pids, Duration::from_millis(600), Duration::ZERO);
        }

        self.registry.dispose();
    }

    /// Process groups left by a participant that died without cleaning up: SIGTERM now, SIGKILL
    /// for whatever is left a moment later. The number of groups found.
    pub fn reap_orphans(&self) -> usize {
        if cfg!(windows) {
            // Windows reuses pids quickly; without a check on the image name it is not safe.
            return 0;
        }

        let groups: Vec<u32> = self.registry.take_orphans().into_iter().map(|orphan| orphan.pid).filter(|pid| process::group_alive(*pid)).collect();

        if !groups.is_empty() {
            let reaped = groups.clone();
            thread::spawn(move || process::stop_groups(&reaped, Duration::from_millis(600), Duration::from_secs(3)));
        }

        groups.len()
    }

    /// Writes down every process group we started and that may still have processes, for the
    /// orphan cleanup after a crash. One writer at a time, so the file is never stale or torn.
    fn record_pids(&self) {
        let _writing = self.pids_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries: Vec<PidEntry> = {
            let mut inner = self.lock();
            let lingering = inner.supervisor.lingering_groups();
            let mut entries: Vec<PidEntry> = inner
                .runs
                .iter()
                .map(|(path, run)| PidEntry { path: path.clone(), pid: run.pid })
                .chain(inner.jobs.iter().map(|(key, job)| PidEntry { path: key.clone(), pid: job.pid() }))
                .collect();
            for (pid, key) in lingering {
                if !entries.iter().any(|entry| entry.pid == pid) {
                    entries.push(PidEntry { path: key, pid });
                }
            }
            entries
        };
        entries.extend(self.sessions().values().filter_map(|session| session.pid_entry()));
        self.registry.record_pids(&entries);
    }

    /* ---------- health ---------- */

    pub fn check_health(self: &Arc<Self>) {
        let targets: Vec<(String, u64, u16)> = {
            let inner = self.lock();
            inner
                .runs
                .iter()
                .filter(|(path, _)| !inner.busy.contains(*path))
                .filter_map(|(path, run)| run.port.filter(|_| run.settled).map(|port| (path.clone(), run.id, port)))
                .collect()
        };

        for (path, run_id, port) in targets {
            let alive = is_port_served(port);
            let current = self.lock().issues.get(&path).map(|issue| issue.kind.clone());

            if !alive && current.as_deref() != Some("unresponsive") {
                self.set_issue(&path, "unresponsive", t!("core.issue.notResponding", port = port), IssueEnd::Retrying);
                let core = Arc::clone(self);
                thread::spawn(move || core.recover_unresponsive(&path, run_id, port));
            } else if alive && current.as_deref() == Some("unresponsive") {
                self.lock().issues.remove(&path);
                self.notify();
            }
        }
    }

    fn recover_unresponsive(self: &Arc<Self>, path: &str, run_id: u64, port: u16) {
        thread::sleep(RESTART_DELAY);

        let started = {
            let inner = self.lock();
            match inner.runs.get(path) {
                Some(run) if run.id == run_id && !inner.busy.contains(path) && !inner.disposed => run.started,
                _ => return,
            }
        };

        if is_port_served(port) {
            let mut inner = self.lock();
            if inner.issues.get(path).is_some_and(|issue| issue.kind == "unresponsive") {
                inner.issues.remove(path);
            }
            drop(inner);
            self.notify();
            return;
        }

        let Some(attempt) = self.claim_restart(&Unit::Server(path.to_string()), started) else {
            self.set_issue(path, "unresponsive", t!("core.issue.notRespondingGaveUp", port = port, max = MAX_RESTARTS), IssueEnd::Final);
            return;
        };

        self.push_line(path, format!("[pitwall] {}", t!("core.log.notRespondingRestarting", port = port, attempt = attempt, max = MAX_RESTARTS)));
        self.stop_server(path);
        self.launch(path, false, Vec::new());
    }

    /* ---------- addresses ---------- */

    /// Setting > `APP_URL` in `.env` > `<folder>.test` for Laravel > the server's own address.
    pub fn resolve_url(&self, path: &str, local: Option<String>) -> Option<String> {
        let configured = self.lock().settings.project(path).url.filter(|u| !u.trim().is_empty());

        if configured.is_some() {
            return configured;
        }

        if let Some(app_url) = fs::read_to_string(Path::new(path).join(".env")).ok().and_then(|env| parse_app_url(&env)) {
            return Some(app_url);
        }

        if Path::new(path).join("artisan").exists() {
            return Some(herd_fallback_url(&folder_name(path)));
        }

        local
    }

    /// Opens one of a project's links: the tab that already shows it comes forward, else it opens.
    /// Off the calling thread, as asking the browsers can take a moment. Only web and mail
    /// addresses open (see `links::normalize`); anything else is an error.
    pub fn open_url(self: &Arc<Self>, url: &str) -> Result<(), String> {
        if url.trim().is_empty() {
            return Ok(());
        }
        let url = crate::links::normalize(url).ok_or_else(|| t!("core.error.linkScheme", url = url.trim()))?;
        let core = Arc::clone(self);

        thread::spawn(move || {
            if !crate::browser::focus_page(&url) {
                core.emit(CoreEvent::Open(url));
            }
        });
        Ok(())
    }

    pub fn open_browser(self: &Arc<Self>, path: &str) {
        let local = self.snapshot().projects.into_iter().find(|p| p.path == path).and_then(|p| p.url);
        self.show_in_browser(path, local, false);
    }

    /// Shows the project in the browser: a tab that already has it (its address or the dev
    /// server's) comes forward, reloaded with `reload`; without one, the address opens. Off the
    /// calling thread, as asking the browsers can take a moment. Only a web address opens, so a
    /// project's setting or `.env` can't open a local file or app.
    fn show_in_browser(self: &Arc<Self>, path: &str, local: Option<String>, reload: bool) {
        let Some(url) = self.resolve_url(path, local.clone()).and_then(|url| crate::links::normalize(&url)) else {
            return;
        };
        let urls: Vec<String> = std::iter::once(url.clone()).chain(local.filter(|l| *l != url)).collect();
        let core = Arc::clone(self);

        thread::spawn(move || {
            if !crate::browser::focus_tab(&urls, reload) {
                core.emit(CoreEvent::Open(url));
            }
        });
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

    /// Opening the project's window is looking at it: its Claude turn counts as seen.
    pub fn open_editor(&self, path: &str) {
        self.launch_editor(path);
        self.mark_seen(path);
    }

    /// Brings the project's editor window to the front, or opens the folder in a new window.
    fn launch_editor(&self, path: &str) {
        let settings = self.lock().settings.clone();

        // `open -a <Editor> <folder>` focuses the window that has the folder open and opens a
        // new one otherwise. The URL scheme below is the fallback.
        #[cfg(target_os = "macos")]
        {
            let opened = Command::new("open")
                .args(["-a", settings.editor_app(), path])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());

            if opened {
                return;
            }
        }

        let scheme = settings.editor_scheme().to_string();
        let mut url_path = path.replace('\\', "/");

        if !url_path.starts_with('/') {
            url_path = format!("/{url_path}");
        }

        self.emit(CoreEvent::Open(format!("{scheme}://file{url_path}")));
    }

    /* ---------- Claude ---------- */

    pub fn mark_seen(&self, path: &str) {
        if let Ok(watch) = self.claude.lock() {
            watch.mark_seen(path);
        }
        {
            let mut inner = self.lock();
            inner.waiting.remove(path);
            if let Some(sessions) = inner.claude_sessions.get_mut(path) {
                sessions.retain(|session| session.phase != SessionPhase::Waiting);
            }
        }
        self.notify();
    }

    /// Opens the project in the editor; that counts the Claude turn as seen.
    pub fn open_claude(&self, path: &str) {
        self.open_editor(path);
    }

    /* ---------- the loop ---------- */

    /// One pass of the background loop; `tick` counts seconds.
    pub fn tick(self: &Arc<Self>, tick: u64) {
        for command in self.registry.drain_commands() {
            self.handle_command(command);
        }

        self.follow_peer_output();

        // While a spinner waits on another participant, read its state every second.
        if self.has_pending() && !tick.is_multiple_of(HEARTBEAT_EVERY) {
            let peers = self.registry.read_peers();
            self.lock().peers = peers;
            self.notify();
        }

        if tick.is_multiple_of(HEARTBEAT_EVERY) {
            self.heartbeat();
        }

        if tick.is_multiple_of(SWEEP_EVERY) {
            self.registry.sweep();
            // Not only at start: a participant that died moments before we started still looked
            // alive then, and its servers would be left running.
            self.reap_orphans();
        }

        if tick > 0 && tick.is_multiple_of(HEALTH_EVERY) {
            self.check_health();
        }

        if tick.is_multiple_of(GIT_EVERY) {
            self.refresh_git();
        }
    }

    /// Publish our state, read everybody else's, rescan Claude sessions.
    pub fn heartbeat(&self) {
        let own: Vec<ProjectState> = {
            let inner = self.lock();
            inner
                .runs
                .iter()
                .map(|(path, run)| ProjectState {
                    folder_path: path.clone(),
                    name: run.name.clone(),
                    running: true,
                    port: run.port,
                    url: run.url.clone(),
                    started_at: Some(run.started_at),
                    issue: inner.issues.get(path).cloned(),
                    output: None,
                })
                .collect()
        };

        self.registry.publish(own);

        let peers = self.registry.read_peers();
        let favourites = self.registry.read_favourites();
        let paths: Vec<String> = {
            let mut inner = self.lock();
            inner.peers = peers;
            inner.favourites = favourites;
            build_snapshot(&inner).projects.into_iter().map(|p| p.path).collect()
        };

        let scan = self.claude.lock().map(|mut watch| watch.scan(&paths)).unwrap_or_default();
        let hook_installed = self.hook.installed();
        let (announce, projects) = {
            let mut inner = self.lock();
            let announce = due_notifications(&mut inner, &scan.waiting);
            inner.waiting = scan.waiting;
            inner.working = scan.working;
            inner.claude_sessions = live_sessions(&scan.sessions);
            inner.claude_hook = hook_installed;
            (announce, build_snapshot(&inner).projects)
        };
        self.record_activity(&scan.sessions, &projects);

        let sounds = self.settings().sounds;
        let mut sound = None;
        for (path, name, kind) in announce {
            let body = match kind {
                TurnKind::Finished => t!("core.notify.finished"),
                TurnKind::Asking => t!("core.notify.asking"),
                TurnKind::Permission => t!("core.notify.permission"),
            };
            self.emit(CoreEvent::Notify { path, title: name, body });
            // One sound for the lot; a question outranks a finished turn.
            if kind != TurnKind::Finished || sound.is_none() {
                sound = Some(if kind == TurnKind::Finished { &sounds.claude_finished } else { &sounds.claude_asking });
            }
        }
        if let Some(sound) = sound.filter(|s| !s.is_empty()) {
            self.emit(CoreEvent::Sound(sound.clone()));
        }

        self.notify();
    }

    /// Servers another participant runs show their output here too: each mirrors it to a file
    /// under `output/` and names it in its record. The last lines are loaded once, then the file
    /// is followed every tick. A file that shrank was started again (or trimmed): read it anew.
    fn follow_peer_output(&self) {
        let wanted: Vec<(String, PathBuf)> = {
            let inner = self.lock();
            let mut wanted: Vec<(String, PathBuf)> = Vec::new();

            for project in inner.peers.iter().flat_map(|peer| peer.projects.iter()) {
                if !project.running || inner.runs.contains_key(&project.folder_path) || wanted.iter().any(|(path, _)| path == &project.folder_path) {
                    continue;
                }
                if let Some(file) = project.output.as_deref().and_then(|relative| self.registry.output_file(relative)) {
                    wanted.push((project.folder_path.clone(), file));
                }
            }

            wanted
        };

        // Stopped or moved elsewhere: stop following. Its lines stay until the next run.
        let mut tails = std::mem::take(&mut self.lock().tails);
        tails.retain(|path, tail| wanted.iter().any(|(p, file)| p == path && file == &tail.file));

        for (path, file) in wanted {
            let Ok(len) = fs::metadata(&file).map(|meta| meta.len()) else {
                continue;
            };

            let fresh = !tails.contains_key(&path);
            let tail = tails.entry(path.clone()).or_insert_with(|| Tail { file: file.clone(), offset: 0, rest: Vec::new() });

            if len < tail.offset {
                tail.offset = 0;
                tail.rest.clear();
            }
            if len == tail.offset && !fresh {
                continue;
            }

            let from = tail.offset.max(len.saturating_sub(TAIL_CHUNK));
            let Some(mut bytes) = read_range(&file, from, len) else {
                continue;
            };

            // Started in the middle of the file: the first line is partial.
            if from > tail.offset {
                tail.rest.clear();
                let start = bytes.iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| i + 1);
                bytes.drain(..start);
            }

            tail.offset = len;
            tail.rest.extend_from_slice(&bytes);
            let mut lines = take_lines(&mut tail.rest);

            if fresh {
                self.lock().output.insert(path.clone(), VecDeque::new());
                let skip = lines.len().saturating_sub(OUTPUT_LINES);
                lines.drain(..skip);
            }

            for line in lines {
                self.push_line(&path, line);
            }
        }

        self.lock().tails = tails;
    }

    /// Branch and changes of every listed project, off the loop thread; one pass at a time.
    fn refresh_git(self: &Arc<Self>) {
        if self.git_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let paths: Vec<String> = self.snapshot().projects.into_iter().map(|p| p.path).collect();
        let core = Arc::clone(self);

        thread::spawn(move || {
            let states: HashMap<String, GitInfo> =
                paths.iter().filter_map(|path| git::status(path).map(|info| (path.clone(), info))).collect();

            core.lock().git = states;
            core.git_busy.store(false, Ordering::SeqCst);
            core.notify();
        });
    }

    pub fn install_claude_hook(&self) -> Result<(), String> {
        let result = self.hook.install();
        self.lock().claude_hook = self.hook.installed();
        self.notify();
        result
    }

    pub fn uninstall_claude_hook(&self) -> Result<(), String> {
        let result = self.hook.uninstall();
        self.lock().claude_hook = self.hook.installed();
        self.notify();
        result
    }

    pub fn registry_dir(&self) -> &Path {
        &self.cfg.registry_dir
    }
}

fn read_range(file: &Path, from: u64, to: u64) -> Option<Vec<u8>> {
    let mut handle = fs::File::open(file).ok()?;
    handle.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    handle.take(to.saturating_sub(from)).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Complete lines out of `rest`, cleaned like our own output; the unfinished end stays.
fn take_lines(rest: &mut Vec<u8>) -> Vec<String> {
    let end = match rest.iter().rposition(|&b| b == b'\n') {
        Some(at) => at + 1,
        None if rest.len() > TAIL_LINE_MAX => rest.len(),
        None => return Vec::new(),
    };

    let complete: Vec<u8> = rest.drain(..end).collect();
    String::from_utf8_lossy(&complete)
        .split_terminator('\n')
        .map(|line| crate::resolve::strip_ansi(line.trim_end_matches('\r')))
        .collect()
}

/// Servers, ours or a peer's, that just crashed or came up: their sounds, once each. A project
/// first seen makes none.
fn server_sounds(inner: &mut Inner, projects: &[ProjectView]) -> Vec<String> {
    let mut sounds: Vec<String> = Vec::new();

    for project in projects {
        let before = inner.statuses.insert(project.path.clone(), project.status);
        let sound = match (before, project.status) {
            (Some(was), "crashed") if was != "crashed" => &inner.settings.sounds.server_crashed,
            (Some(was), "running") if was != "running" => &inner.settings.sounds.server_ready,
            _ => continue,
        };
        if !sound.is_empty() && !sounds.contains(sound) {
            sounds.push(sound.clone());
        }
    }

    sounds
}

/// The sessions working or waiting on you, by project.
fn live_sessions(sessions: &[crate::claude::SessionState]) -> HashMap<String, Vec<LiveSession>> {
    let mut found: HashMap<String, Vec<LiveSession>> = HashMap::new();
    for session in sessions.iter().filter(|s| s.phase != SessionPhase::Idle) {
        let live = LiveSession { id: session.id.clone(), phase: session.phase, turn: session.turn };
        found.entry(session.path.clone()).or_default().push(live);
    }
    found
}

fn describe_exit(status: Option<std::process::ExitStatus>) -> (String, String) {
    let Some(status) = status else {
        return ("-".into(), "-".into());
    };

    let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "-".into());

    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|s| s.to_string()).unwrap_or_else(|| "-".into())
    };
    #[cfg(not(unix))]
    let signal = "-".to_string();

    (code, signal)
}

/// The package manager a project's lock file names (npm without one).
fn detect_manager(path: &str) -> PackageManager {
    let files: Vec<String> =
        fs::read_dir(path).map(|entries| entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    detect_package_manager(&files)
}

/// The command a project's dev server would run, as the launcher builds it: Settings' package
/// manager or the lock file's, and the project's script. The lock file is read once per
/// project (and again at each start).
fn run_command(inner: &Inner, path: &str) -> String {
    let script = inner.settings.script_for(path);
    let manager = PackageManager::parse(&inner.settings.package_manager)
        .unwrap_or_else(|| *inner.managers.borrow_mut().entry(path.to_string()).or_insert_with(|| detect_manager(path)));

    build_command(manager, &script, &[]).unwrap_or_else(|_| format!("{manager} run {}", script.trim()))
}

/// The port a project wants: `server.port` in its Vite config, else 5173.
fn vite_port(path: &str) -> u16 {
    ["vite.config.ts", "vite.config.js", "vite.config.mts", "vite.config.mjs"]
        .iter()
        .filter_map(|name| fs::read_to_string(Path::new(path).join(name)).ok())
        .find_map(|source| parse_vite_port(&source))
        .unwrap_or(5173)
}

/// Waiting turns to announce now. The first scan after launch only takes note, so turns that
/// were already waiting don't arrive as a burst.
fn due_notifications(inner: &mut Inner, waiting: &HashMap<String, Turn>) -> Vec<(String, String, TurnKind)> {
    let first = !inner.claude_scanned;
    inner.claude_scanned = true;
    inner.candidates.retain(|path, _| waiting.contains_key(path));

    let mut due = Vec::new();

    for (path, turn) in waiting {
        if inner.notified.get(path) == Some(&turn.at) {
            continue;
        }

        if first {
            inner.notified.insert(path.clone(), turn.at);
            continue;
        }

        match inner.candidates.get(path) {
            Some((at, since)) if *at == turn.at && since.elapsed() >= NOTIFY_AFTER => {
                inner.candidates.remove(path);
                inner.notified.insert(path.clone(), turn.at);

                if inner.settings.notify {
                    let name = inner
                        .favourites
                        .iter()
                        .find(|f| &f.path == path)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| folder_name(path));
                    due.push((path.clone(), name, turn.kind));
                }
            }
            Some((at, _)) if *at == turn.at => {}
            _ => {
                inner.candidates.insert(path.clone(), (turn.at, Instant::now()));
            }
        }
    }

    due
}

/// The new order of the listed projects, keeping the place of projects that aren't listed right
/// now (a closed window that isn't a favourite): each stays right after the project it followed,
/// so it comes back where it was.
pub fn merge_order(old: &[String], listed: Vec<String>) -> Vec<String> {
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

/// Drops spinners whose action is visibly done (or that ran out of time).
fn prune_pending(inner: &mut Inner) {
    let done: Vec<String> = inner
        .pending
        .iter()
        .filter(|(path, pending)| pending_done(inner, path, pending))
        .map(|(path, _)| path.clone())
        .collect();

    for path in done {
        inner.pending.remove(&path);
    }
}

fn pending_done(inner: &Inner, path: &str, pending: &Pending) -> bool {
    if pending.at.elapsed() > PENDING_LIMIT {
        return true;
    }

    let run = inner.runs.get(path);
    let peer = inner.peers.iter().find_map(|peer| peer.projects.iter().find(|p| p.folder_path == path && p.running));
    let ready = |run: &Run| run.settled || run.started.elapsed() > SETTLE_LIMIT;

    match pending.action {
        Action::Start => run.is_some_and(ready) || peer.is_some(),
        Action::Stop => run.is_none() && peer.is_none() && !inner.busy.contains(path),
        Action::Restart => {
            run.is_some_and(|run| run.started > pending.at && ready(run))
                || peer.is_some_and(|p| p.started_at.unwrap_or(0) + 2_000 >= pending.at_ms)
        }
    }
}

/// Every project the app shows: favourites, projects of open editor windows, and whatever runs.
fn build_snapshot(inner: &Inner) -> Snapshot {
    let mut order: Vec<String> = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut add = |path: &str, name: &str| {
        if !names.contains_key(path) {
            order.push(path.to_string());
            names.insert(path.to_string(), name.to_string());
        }
    };

    for favourite in &inner.favourites {
        add(&favourite.path, &favourite.name);
    }
    for peer in &inner.peers {
        for project in peer.owned() {
            add(&project.folder_path, &project.name);
        }
    }
    for (path, run) in &inner.runs {
        add(path, &run.name);
    }

    let mut projects: Vec<ProjectView> = order
        .iter()
        .map(|path| {
            let peer_run = inner
                .peers
                .iter()
                .find_map(|peer| peer.projects.iter().find(|p| &p.folder_path == path && p.running).map(|p| (peer, p)));
            let open_in = inner.peers.iter().find(|peer| peer.has_root(path)).map(|peer| peer.title.clone());
            let run = inner.runs.get(path);
            let issue = inner.issues.get(path).cloned().or_else(|| {
                inner.peers.iter().find_map(|peer| peer.projects.iter().find(|p| &p.folder_path == path).and_then(|p| p.issue.clone()))
            });

            let pending = inner.pending.get(path);
            let phase = match pending.map(|p| p.action) {
                Some(Action::Start) => Some("starting…"),
                Some(Action::Stop) => Some("stopping…"),
                Some(Action::Restart) => Some("restarting…"),
                None if inner.busy.contains(path) => Some("stopping…"),
                None => None,
            };

            let status = if phase.is_some() {
                "busy"
            } else if run.is_some() || peer_run.is_some() {
                "running"
            } else if issue.as_ref().is_some_and(|i| i.kind == "crashed") {
                "crashed"
            } else {
                "stopped"
            };

            let (port, url, started_at) = match (run, peer_run) {
                (Some(run), _) => (run.port, run.url.clone(), Some(run.started_at)),
                (None, Some((_, p))) => (p.port, p.url.clone(), p.started_at),
                _ => (None, None, None),
            };

            ProjectView {
                path: path.clone(),
                name: names.get(path).cloned().unwrap_or_else(|| folder_name(path)),
                favourite: inner.favourites.iter().any(|f| &f.path == path),
                status,
                phase,
                port,
                url,
                started_at,
                issue,
                owner: if run.is_none() {
                    peer_run.map(|(peer, _)| Owner { id: peer.window_id.clone(), title: peer.title.clone() })
                } else {
                    None
                },
                open_in,
                claude: inner.waiting.get(path).copied(),
                claude_working: inner.working.contains(path),
                claude_sessions: inner.claude_sessions.get(path).cloned().unwrap_or_default(),
                git: inner.git.get(path).cloned(),
                script: inner.settings.script_for(path),
                run_command: run.map(|run| run.command.clone()).unwrap_or_else(|| run_command(inner, path)),
                settings: inner.settings.project(path),
                commands: jobs::command_views(inner, path),
                terminals: inner.terminal_list.iter().filter(|t| &t.path == path).cloned().collect(),
            }
        })
        .collect();

    // A fixed order: the one the user dragged into place, then everything else by name. State
    // changes (Claude, servers) never move a row.
    let position = |p: &ProjectView| inner.settings.order.iter().position(|path| path == &p.path);
    projects.sort_by(|a, b| match (position(a), position(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Snapshot {
        running: projects.iter().filter(|p| p.status == "running").count(),
        waiting: projects.iter().filter(|p| p.claude.is_some()).count(),
        crashed: projects.iter().filter(|p| p.status == "crashed").count(),
        crash_unseen: inner.crash_unseen,
        claude_hook: inner.claude_hook,
        settings: inner.settings.clone(),
        language: i18n::resolve(&inner.settings.language),
        system_language: i18n::system(),
        projects,
    }
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
