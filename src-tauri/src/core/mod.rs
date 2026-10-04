//! The app's core: dev servers, the shared registry and Claude sessions, without any Tauri
//! types so it can be driven end to end from tests. The Tauri layer listens to `CoreEvent`s.
//!
//! Lifecycle rules follow the extension's `src/runner.ts`: login shell, whole process tree,
//! silent free port, crash restart after 3 s, give up after 3 failures within 60 s, health
//! probe every 30 s.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::claude::{ClaudeWatch, SessionPhase, SessionState, Turn};
use crate::git::GitInfo;
use crate::hooks::ClaudeHook;
use crate::i18n::{self, t};
use crate::ports::Reservations;
use crate::process::{self, kill_tree};
use crate::registry::{now_ms, same_path, Favourite, Issue, PidEntry, Registry, WindowRecord};
use crate::resolve::PackageManager;
use crate::settings::{ProjectSettings, Settings, TrackSettings};

mod activity;
mod board;
mod claude_state;
mod dependencies;
mod jobs;
mod open;
mod output;
mod peers;
mod projects;
mod reveal;
mod routing;
mod servers;
mod sessions;
mod snapshot;
mod supervise;
mod terminal;
mod watch;
pub use activity::DaySummary;
pub use board::{Card, Give, NewImage};
pub use terminal::{TerminalBuffer, TerminalView};
pub use jobs::CommandView;
pub use projects::Folder;
pub use routing::Action;
pub use sessions::SessionsView;
pub use snapshot::Snapshot;
use jobs::{job_key, Job, JobResult};
use output::Outbox;
use peers::Tail;
use routing::Pending;
use servers::Run;
use snapshot::{LiveSession, ProjectView};
use supervise::{Supervisor, Unit};

const RESTART_DELAY: Duration = Duration::from_secs(3);
const MAX_RESTARTS: u32 = 3;
const STABLE: Duration = Duration::from_secs(60);
const OUTPUT_LINES: usize = 500;
const HEARTBEAT_EVERY: u64 = 5;
const HEALTH_EVERY: u64 = 30;
const SWEEP_EVERY: u64 = 60;
/// Claude's logs, the hook's events and Git are read again in full this often, in case a
/// file-system event was missed; otherwise they are read when they change.
const POLL_EVERY: u64 = 60;

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
    /// Show this terminal in the main window: a Claude session runs in it.
    RevealTerminal { path: String, id: u64 },
    /// The board changed: every card, for the main window.
    Board(Vec<Card>),
    /// The Dependencies page's reports changed; every listed project's.
    Deps(Vec<crate::deps::DepReport>),
}

pub type Sink = Arc<dyn Fn(CoreEvent) + Send + Sync>;

pub struct CoreConfig {
    pub registry_dir: PathBuf,
    pub claude_dir: PathBuf,
    pub settings_file: PathBuf,
    /// `~/.claude/settings.json`, where the Claude Code hook goes.
    pub claude_settings: PathBuf,
    /// Registry folders of older extension versions, read for windows not yet reloaded.
    pub legacy_registries: Vec<PathBuf>,
    pub title: String,
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
    /// Every session of the last scan, listed projects' and the hook's others, for the
    /// Sessions page.
    scanned: Vec<SessionState>,
    git: HashMap<String, GitInfo>,
    claude_hook: bool,
    claude_hook_outdated: bool,
    /// The board isn't on disk as it is now (`board.rs`).
    board_unsaved: bool,
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

pub struct Core {
    pub registry: Registry,
    cfg: CoreConfig,
    inner: Mutex<Inner>,
    /// Project terminals' processes; apart from `inner` so typing never waits on it.
    terminals: Mutex<HashMap<u64, terminal::Session>>,
    /// What the day's timeline has written down last.
    activity: Mutex<activity::Recorder>,
    /// What the Sessions page keeps between two requests.
    sessions: Mutex<sessions::SessionsCache>,
    reservations: Mutex<Reservations>,
    claude: Mutex<ClaudeWatch>,
    hook: ClaudeHook,
    /// File-system events for Claude's logs, the hook's events and Git; started by the first
    /// poll.
    watch: Mutex<Option<watch::Watch>>,
    git_busy: AtomicBool,
    /// Held while the pids file is built and written, so it is never stale or torn.
    pids_lock: Mutex<()>,
    outbox: Arc<Mutex<Outbox>>,
    /// Projects whose Git state is to be read again.
    git_queue: Mutex<HashSet<String>>,
    /// The board's cards (`board.rs`); never held together with `inner`.
    board: Mutex<board::Board>,
    /// Held while the board follows its sessions, one pass at a time.
    board_follow: Mutex<()>,
    /// The Dependencies page's checks.
    deps: dependencies::Dependencies,
    sink: Sink,
}

fn folder_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

impl Core {
    pub fn new(cfg: CoreConfig, sink: Sink) -> Arc<Self> {
        let registry = Registry::new(cfg.registry_dir.clone(), &cfg.title).with_legacy(cfg.legacy_registries.clone());
        let legacy_seen = cfg.legacy_registries.iter().map(|dir| dir.join("claude-seen.json")).collect();
        let claude = ClaudeWatch::new(cfg.claude_dir.clone(), &cfg.registry_dir).with_legacy_seen(legacy_seen);
        let hook = ClaudeHook::new(cfg.claude_settings.clone(), &cfg.registry_dir);
        let hook_status = hook.status();
        let settings = Settings::load(&cfg.settings_file);
        i18n::set(i18n::resolve(&settings.language));
        let favourites = registry.read_favourites();
        let peers = registry.read_peers();
        // Beside the settings, on this Mac only.
        let board = board::Board::load(cfg.settings_file.with_file_name("board.json"), now_ms());

        Arc::new(Self {
            registry,
            cfg,
            inner: Mutex::new(Inner {
                settings,
                favourites,
                peers,
                claude_hook: hook_status.installed,
                claude_hook_outdated: hook_status.outdated,
                board_unsaved: board.read_only(),
                ..Inner::default()
            }),
            terminals: Mutex::new(HashMap::new()),
            activity: Mutex::new(activity::Recorder::default()),
            sessions: Mutex::new(sessions::SessionsCache::default()),
            reservations: Mutex::new(Reservations::default()),
            claude: Mutex::new(claude),
            hook,
            watch: Mutex::new(None),
            git_busy: AtomicBool::new(false),
            pids_lock: Mutex::new(()),
            outbox: Arc::new(Mutex::new(Outbox::default())),
            git_queue: Mutex::new(HashSet::new()),
            board: Mutex::new(board),
            board_follow: Mutex::new(()),
            deps: dependencies::Dependencies::default(),
            sink,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn emit(&self, event: CoreEvent) {
        (self.sink)(event);
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

        if tick.is_multiple_of(POLL_EVERY) {
            self.poll_files();
        }

        if tick.is_multiple_of(HEARTBEAT_EVERY) {
            self.heartbeat();
        } else if self.claude_due() {
            self.refresh_claude();
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

        self.deps_tick(tick);
    }
}

#[cfg(test)]
mod tests;
