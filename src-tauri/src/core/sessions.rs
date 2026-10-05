//! The Sessions page: one row per Claude session Pitwall knows of, in a listed project or not,
//! with where it stands, where it runs, its time today and its tokens. Built on request from
//! what is already at hand: the last scan (`ClaudeWatch::evaluate`), the day's file (no git),
//! today's token counts (`spend.rs`), and Claude Code's running sessions.
//!
//! Claude Code keeps `~/.claude/sessions/<pid>.json` while a session runs. Only `pid`,
//! `sessionId`, `entrypoint`, `cwd`, `startedAt` and `status` are read from it: `SessionFile`
//! declares nothing else, so every other field (`name`, which Claude Code derives from the
//! conversation, the socket and key fields) is skipped unread. The process table tells which of
//! them still run and in what; a process's arguments are only tested, never kept or shown, and a
//! terminal app is named only from a fixed list.

use serde::Deserialize;

use super::activity::{DayCache, SessionDay, Sighting};
use super::reveal::is_session_id;
use super::snapshot::build_snapshot;
use super::*;
use crate::claude::{inside, Peek, SessionState, TurnKind, WORKING_WINDOW_MS};
use crate::process::ProcessInfo;
use crate::spend::SessionSpend;

/// Running sessions are read again at most this often, unless their folder changed.
const READ_EVERY: Duration = Duration::from_secs(30);
/// The longest chain of parent processes followed.
const LINEAGE_MAX: usize = 32;

/// Terminal apps a session can run in, by the lower-case name of one of its parent processes:
/// the executable, or on macOS the app bundle it lives in. Nothing else names one.
const TERMINAL_APPS: &[(&str, &str)] = &[
    ("iterm2", "iTerm2"),
    ("iterm", "iTerm2"),
    ("terminal", "Terminal"),
    ("ghostty", "Ghostty"),
    ("wezterm", "WezTerm"),
    ("wezterm-gui", "WezTerm"),
    ("wezterm-mux-server", "WezTerm"),
    ("alacritty", "Alacritty"),
    ("kitty", "kitty"),
    ("warp", "Warp"),
    ("hyper", "Hyper"),
    ("tabby", "Tabby"),
    ("rio", "Rio"),
    ("wave", "Wave"),
    ("windowsterminal", "Windows Terminal"),
    ("openconsole", "Windows Terminal"),
    ("tmux", "tmux"),
    ("zellij", "Zellij"),
    ("screen", "screen"),
    ("konsole", "Konsole"),
    ("gnome-terminal-server", "GNOME Terminal"),
    ("kgx", "Console"),
    ("ptyxis", "Ptyxis"),
    ("xfce4-terminal", "Xfce Terminal"),
    ("tilix", "Tilix"),
    ("terminator", "Terminator"),
    ("foot", "foot"),
    ("xterm", "xterm"),
    ("urxvt", "urxvt"),
    ("visual studio code", "VS Code"),
    ("code", "VS Code"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("zed", "Zed"),
];

/// iTerm2's shells run under a server named for its version (`iTermServer-3.5.4`).
const TERMINAL_PREFIXES: &[(&str, &str)] = &[("itermserver", "iTerm2")];

/// A file in `~/.claude/sessions`, as far as it is read. Every other field is skipped unread.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    pid: u32,
    session_id: String,
    #[serde(default)]
    entrypoint: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    started_at: Option<f64>,
    /// `busy` | `idle` | `waiting` | `shell`
    #[serde(default)]
    status: Option<String>,
}

/// A session whose process runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Running {
    pub pid: u32,
    /// How it was started: `cli`, `claude-vscode`, `sdk-ts`…
    pub entrypoint: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<u64>,
    /// Claude Code's own word for where it stands: `busy` (at work, its background subagents
    /// included) | `idle` | `waiting` | `shell` (idle, a background command still runs).
    pub status: Option<String>,
    /// The process and its parents, nearest first.
    pub lineage: Vec<u32>,
    /// The terminal app among its parents, from `TERMINAL_APPS`.
    pub app: Option<&'static str>,
}

/// Where a session runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionOrigin {
    /// One of Pitwall's own terminals, by its id.
    Pitwall { terminal: u64 },
    /// A Claude Code tab of a VS Code window (by its title) that runs Pitwall for VS Code.
    VscodeTab { window: String },
    /// A terminal of such a window.
    VscodeTerminal { window: String },
    /// A terminal app, named when it is a known one.
    Terminal { app: Option<String> },
    /// Started some other way (`sdk-ts`, the desktop app…).
    Other { entrypoint: Option<String> },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RowPhase {
    Working,
    Waiting,
    Idle,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpan {
    pub start: u64,
    pub end: u64,
    /// `work` | `wait`
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionToday {
    /// Milliseconds Claude worked, and waited on you, today.
    pub work: u64,
    pub wait: u64,
    pub turns: u32,
    pub spans: Vec<SessionSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    /// The listed project it belongs to; `None` for a folder that isn't listed.
    pub path: Option<String>,
    /// The folder it runs in.
    pub folder: String,
    /// The project's name, or the folder's.
    pub project: String,
    /// Its name as Claude Code shows it in `/resume`.
    pub title: Option<String>,
    pub phase: RowPhase,
    /// While it waits on you: the turn it waits with.
    pub turn: Option<Turn>,
    /// While it waits on a permission prompt: the tool's name, when the hook told it.
    pub tool: Option<String>,
    /// When its phase began (when it ended, once ended), when known.
    pub since: Option<u64>,
    /// Its process runs; `None` when that can't be told.
    pub running: Option<bool>,
    pub origin: SessionOrigin,
    /// Its time today, for sessions of listed projects.
    pub today: Option<SessionToday>,
    /// Its tokens today, subagents included.
    pub spend: Option<SessionSpend>,
    pub subagents: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsView {
    pub now: u64,
    /// Pitwall's Claude Code hook is installed.
    pub hook: bool,
    /// Waiting first (the longest wait first), then working, idle and ended (the latest first).
    pub sessions: Vec<SessionRow>,
    /// How many of them run in folders that aren't listed (`path` is `None`).
    pub unlisted: usize,
}

/// What the page keeps between two requests.
#[derive(Default)]
pub(super) struct SessionsCache {
    /// Running sessions as last read (`None`: can't be told) and when.
    running: Option<HashMap<String, Running>>,
    read_at: Option<Instant>,
    /// The sessions folder changed since.
    stale: bool,
    /// The origin each session had while it ran, kept for after it ends.
    origins: HashMap<String, SessionOrigin>,
    day: DayCache,
    /// When each session was first seen in its phase, where nothing better tells.
    first: HashMap<String, (RowPhase, u64)>,
    /// The names and folders read from the logs of earlier days' sessions (`history.rs`).
    pub(super) briefs: HashMap<String, super::history::Brief>,
}

/// The process and its parents, nearest first.
pub(super) fn lineage(table: &HashMap<u32, ProcessInfo>, pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];

    while let Some(parent) = chain.last().and_then(|pid| table.get(pid)).map(|process| process.parent) {
        if parent <= 1 || chain.contains(&parent) || chain.len() >= LINEAGE_MAX {
            break;
        }
        chain.push(parent);
    }

    chain
}

/// The terminal app a process is, by its name or its macOS app bundle; only from the list.
fn app_of(process: &ProcessInfo) -> Option<&'static str> {
    let parts: Vec<&str> = process.program.split(['/', '\\']).filter(|part| !part.is_empty()).collect();
    let bundle = parts.iter().find_map(|part| part.strip_suffix(".app"));
    let base = parts.last().copied();
    let names = [bundle, base, Some(process.name.as_str())];

    names.into_iter().flatten().find_map(|name| {
        let name = name.to_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        TERMINAL_APPS
            .iter()
            .find(|(key, _)| *key == name)
            .or_else(|| TERMINAL_PREFIXES.iter().find(|(prefix, _)| name.starts_with(prefix)))
            .map(|(_, app)| *app)
    })
}

/// The nearest of the session's parents that is a known terminal app.
fn terminal_app(table: &HashMap<u32, ProcessInfo>, lineage: &[u32]) -> Option<&'static str> {
    lineage.iter().skip(1).filter_map(|pid| table.get(pid)).find_map(app_of)
}

/// The sessions of `dir` whose process runs, by session id. A file left behind by a process that
/// died is skipped: its pid is gone or belongs to something else now (a Claude process that took
/// the pid over writes the same file name).
fn read_running(dir: &Path, table: &HashMap<u32, ProcessInfo>) -> HashMap<String, Running> {
    let files: Vec<PathBuf> = fs::read_dir(dir).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    let mut found = HashMap::new();

    for file in files.iter().filter(|file| file.extension().is_some_and(|ext| ext == "json")) {
        let Some(record) = fs::read_to_string(file).ok().and_then(|text| serde_json::from_str::<SessionFile>(&text).ok()) else {
            continue;
        };
        let alive = table.get(&record.pid).is_some_and(|process| process.line.to_lowercase().contains("claude"));
        if !alive || !is_session_id(&record.session_id) {
            continue;
        }

        let lineage = lineage(table, record.pid);
        let running = Running {
            pid: record.pid,
            entrypoint: record.entrypoint.filter(|entry| is_word(entry)),
            cwd: record.cwd.filter(|cwd| !cwd.is_empty()),
            started_at: record.started_at.filter(|at| at.is_finite() && *at > 0.0).map(|at| at as u64),
            status: record.status.filter(|status| is_word(status)),
            app: terminal_app(table, &lineage),
            lineage,
        };
        found.insert(record.session_id, running);
    }

    found
}

/// A short identifier (`cli`, `sdk-ts`, `busy`); anything else is dropped.
fn is_word(text: &str) -> bool {
    !text.is_empty() && text.len() <= 40 && text.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// Where a running session runs: one of Pitwall's terminals (`(shell pid, terminal id)`), a
/// VS Code window's terminal or Claude Code tab, a terminal app, or something else.
fn origin_of(running: &Running, pitwall: &[(u32, u64)], peers: &[WindowRecord]) -> SessionOrigin {
    if let Some((_, id)) = pitwall.iter().find(|(shell, _)| running.lineage.contains(shell)) {
        return SessionOrigin::Pitwall { terminal: *id };
    }

    for peer in peers {
        let terminals = peer.terminals.as_deref().unwrap_or_default();
        if running.lineage.iter().any(|pid| terminals.contains(pid)) {
            return SessionOrigin::VscodeTerminal { window: peer.title.clone() };
        }
    }

    match running.entrypoint.as_deref() {
        Some("claude-vscode") => peers
            .iter()
            .find(|peer| peer.pid().is_some_and(|host| running.lineage.contains(&host)))
            .map(|peer| SessionOrigin::VscodeTab { window: peer.title.clone() })
            .unwrap_or(SessionOrigin::Other { entrypoint: Some("claude-vscode".into()) }),
        None | Some("cli") => SessionOrigin::Terminal { app: running.app.map(str::to_string) },
        Some(other) => SessionOrigin::Other { entrypoint: Some(other.to_string()) },
    }
}

/// Where a session stands by Claude Code's own `status`, for a session whose phase the hook
/// doesn't decide: busy is working, waiting is a prompt open; idle at the prompt (`shell`: with
/// a background command of its still running) keeps a finished turn that still waits on you.
pub(super) fn status_phase(status: Option<&str>, scanned: Option<(RowPhase, Option<Turn>)>) -> Option<(RowPhase, Option<Turn>)> {
    let waiting_turn = scanned.and_then(|(phase, turn)| turn.filter(|_| phase == RowPhase::Waiting));

    match status? {
        "busy" => Some((RowPhase::Working, None)),
        "waiting" => Some((RowPhase::Waiting, waiting_turn.filter(|turn| turn.kind != TurnKind::Finished))),
        "idle" | "shell" => Some(match waiting_turn {
            Some(turn) if turn.kind == TurnKind::Finished => (RowPhase::Waiting, Some(turn)),
            _ => (RowPhase::Idle, None),
        }),
        _ => None,
    }
}

pub(super) fn row_phase(phase: SessionPhase) -> RowPhase {
    match phase {
        SessionPhase::Working => RowPhase::Working,
        SessionPhase::Waiting => RowPhase::Waiting,
        SessionPhase::Idle => RowPhase::Idle,
    }
}

/// One session while its row is put together.
#[derive(Default)]
struct Draft {
    scanned: Option<SessionState>,
    running: Option<Running>,
    day: Option<SessionDay>,
}

/// The draft of session `id`, new ones in the order met.
fn draft<'a>(order: &mut Vec<String>, drafts: &'a mut HashMap<String, Draft>, id: &str) -> &'a mut Draft {
    if !drafts.contains_key(id) {
        order.push(id.to_string());
    }
    drafts.entry(id.to_string()).or_default()
}

impl Core {
    pub(super) fn sessions_cache(&self) -> MutexGuard<'_, SessionsCache> {
        self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What Claude last said in a session, for its details (the Sessions page's, a board
    /// card's): read whole from the session's log when asked, never kept or logged. `None` for
    /// what isn't a session id (it becomes a file name) and for a session without a log.
    pub fn last_message(&self, session: &str) -> Option<String> {
        if !is_session_id(session) {
            return None;
        }
        let (log, _) = self.session_log(session)?;
        crate::claude::last_message(&log)
    }

    /// `~/.claude/sessions`, beside the projects folder.
    pub(super) fn sessions_dir(&self) -> Option<PathBuf> {
        self.cfg.claude_dir.parent().map(|claude| claude.join("sessions"))
    }

    /// The sessions folder changed: read it again on the next request.
    pub(super) fn sessions_changed(&self) {
        self.sessions_cache().stale = true;
    }

    /// The running sessions now (`None` when that can't be told), read afresh.
    pub(super) fn read_running(&self) -> Option<HashMap<String, Running>> {
        let dir = self.sessions_dir().filter(|dir| dir.is_dir())?;
        let table = process::process_table()?;
        let running = read_running(&dir, &table);

        let mut cache = self.sessions_cache();
        cache.running = Some(running.clone());
        cache.read_at = Some(Instant::now());
        cache.stale = false;
        Some(running)
    }

    /// The sessions Claude Code itself calls busy now, by session id: a turn of theirs that ended
    /// isn't over (`ClaudeWatch::set_busy`). Empty when the running sessions can't be told.
    pub(super) fn busy_sessions(&self) -> HashSet<String> {
        let running = self.running_cached().unwrap_or_default();
        running.into_iter().filter(|(_, run)| run.status.as_deref() == Some("busy")).map(|(id, _)| id).collect()
    }

    /// The running sessions as last read; again when the folder changed, or 30 s on.
    pub(super) fn running_cached(&self) -> Option<HashMap<String, Running>> {
        let due = {
            let cache = self.sessions_cache();
            cache.stale || cache.read_at.is_none_or(|at| at.elapsed() >= READ_EVERY)
        };
        if !due {
            return self.sessions_cache().running.clone();
        }

        let running = self.read_running();
        if running.is_none() {
            let mut cache = self.sessions_cache();
            cache.running = None;
            cache.read_at = Some(Instant::now());
            cache.stale = false;
        }
        running
    }

    /// Every Claude session Pitwall knows of, one row each (see the top of this file). `spend`
    /// is today's tokens per session.
    pub fn sessions_view(&self, spend: &HashMap<String, SessionSpend>) -> SessionsView {
        let now = now_ms();
        let (midnight, _, _) = crate::clock::local_day(now);

        let (scanned, hook, projects, peers) = {
            let inner = self.lock();
            let projects: Vec<(String, String)> = build_snapshot(&inner).projects.into_iter().map(|p| (p.path, p.name)).collect();
            (inner.scanned.clone(), inner.claude_hook, projects, inner.peers.clone())
        };
        let pitwall: Vec<(u32, u64)> = self.terminal_shells().into_iter().map(|(shell, id, _)| (shell, id)).collect();
        let sightings: HashMap<String, Sighting> = self.sightings();
        let running = self.running_cached();
        let days = {
            let mut cache = self.sessions_cache();
            self.day_sessions(&mut cache.day)
        };

        // Every session once: the scan's, the running ones, then the day's.
        let mut order: Vec<String> = Vec::new();
        let mut drafts: HashMap<String, Draft> = HashMap::new();
        for state in scanned {
            let id = state.id.clone();
            draft(&mut order, &mut drafts, &id).scanned = Some(state);
        }
        for (id, run) in running.iter().flatten() {
            draft(&mut order, &mut drafts, id).running = Some(run.clone());
        }
        for (id, day) in days {
            draft(&mut order, &mut drafts, &id).day = Some(day);
        }

        let listed_for = |folder: &str| -> Option<&(String, String)> {
            projects.iter().filter(|(path, _)| inside(folder, path)).max_by_key(|(path, _)| path.len())
        };

        // Where each one runs, and its log's name and last write; read once, outside the cache.
        let located: Vec<(String, Option<String>, String)> = order
            .iter()
            .map(|id| {
                let draft = &drafts[id];
                let scanned = draft.scanned.as_ref();
                let listed = scanned.map(|s| s.path.clone()).filter(|path| !path.is_empty());
                let folder = scanned
                    .and_then(|s| s.folder.clone())
                    .or_else(|| draft.running.as_ref().and_then(|r| r.cwd.clone()))
                    .or_else(|| listed.clone())
                    .or_else(|| draft.day.as_ref().map(|d| d.path.clone()))
                    .unwrap_or_default();
                let path = listed
                    .or_else(|| draft.day.as_ref().and_then(|d| projects.iter().find(|(p, _)| same_path(p, &d.path))).map(|(p, _)| p.clone()))
                    .or_else(|| listed_for(&folder).map(|(p, _)| p.clone()));
                (id.clone(), path, folder)
            })
            .collect();
        let peeks: HashMap<String, Peek> = match self.claude.lock() {
            Ok(mut watch) => located
                .iter()
                .filter(|(id, _, folder)| is_session_id(id) && !folder.is_empty())
                .filter_map(|(id, _, folder)| Some((id.clone(), watch.peek(id, folder)?)))
                .collect(),
            Err(_) => HashMap::new(),
        };

        let mut cache = self.sessions_cache();
        let mut rows: Vec<SessionRow> = Vec::new();

        for (id, path, folder) in located {
            let Some(draft) = drafts.remove(&id) else {
                continue;
            };
            let peek = peeks.get(&id);
            let scanned = draft.scanned.as_ref().map(|s| (row_phase(s.phase), s.turn.filter(|_| s.phase == SessionPhase::Waiting)));
            let decided = draft.scanned.as_ref().is_some_and(|s| s.hooked);
            let status = draft.running.as_ref().and_then(|r| r.status.as_deref());

            let (mut phase, mut turn) = match (decided, status_phase(status, scanned)) {
                (false, Some(by_status)) => by_status,
                _ => scanned.unwrap_or((RowPhase::Idle, None)),
            };

            // Not running: ended, once its log has been quiet a while (an older Claude Code keeps
            // no sessions folder, so a session that writes now may well run).
            let last = peek.map(|p| p.modified).or(draft.day.as_ref().map(|d| d.end)).unwrap_or(0);
            let running_now = match (&draft.running, &running) {
                (Some(_), _) => Some(true),
                (None, None) => None,
                (None, Some(_)) if now.saturating_sub(last) > WORKING_WINDOW_MS => Some(false),
                (None, Some(_)) => None,
            };
            let ended_by_hook = draft.scanned.as_ref().is_some_and(|s| s.ended);
            let running_now = if ended_by_hook && draft.running.is_none() { Some(false) } else { running_now };
            if ended_by_hook || running_now == Some(false) {
                phase = RowPhase::Ended;
            }
            if phase != RowPhase::Waiting {
                turn = None;
            }

            let sighting = sightings.get(&id);
            let first = match cache.first.get(&id) {
                Some((seen, at)) if *seen == phase => *at,
                _ => now,
            };
            cache.first.insert(id.clone(), (phase, first));
            let since = match phase {
                RowPhase::Ended => draft.scanned.as_ref().filter(|s| s.ended).and_then(|s| s.since).or(Some(last).filter(|at| *at > 0)),
                RowPhase::Idle => draft.scanned.as_ref().and_then(|s| s.since).or(Some(last).filter(|at| *at > 0)),
                _ => draft
                    .scanned
                    .as_ref()
                    .filter(|s| row_phase(s.phase) == phase)
                    .and_then(|s| s.since)
                    .or(sighting.filter(|seen| row_phase(seen.phase) == phase).map(|seen| seen.since))
                    .or(Some(first)),
            };
            // Claude Code says a prompt is open, the hook didn't say which: a permission prompt
            // is by far the likeliest.
            if phase == RowPhase::Waiting && turn.is_none() {
                turn = Some(Turn { kind: TurnKind::Permission, at: since.unwrap_or(first) });
            }
            let tool = draft
                .scanned
                .as_ref()
                .filter(|_| turn.is_some_and(|turn| turn.kind == TurnKind::Permission))
                .and_then(|s| s.tool.clone());

            let title = peek
                .and_then(|p| p.title.clone())
                .or_else(|| draft.scanned.as_ref().and_then(|s| s.title.clone()))
                .or_else(|| sighting.and_then(|seen| seen.title.clone()))
                .or_else(|| draft.day.as_ref().and_then(|d| d.title.clone()));

            let origin = match &draft.running {
                Some(run) => {
                    let origin = origin_of(run, &pitwall, &peers);
                    cache.origins.insert(id.clone(), origin.clone());
                    origin
                }
                None => cache.origins.get(&id).cloned().unwrap_or(SessionOrigin::Unknown),
            };

            let today = draft.day.map(|day| SessionToday {
                work: day.work,
                wait: day.wait,
                turns: day.turns,
                spans: day.spans.into_iter().map(|(start, end, working)| SessionSpan { start, end, kind: if working { "work" } else { "wait" } }).collect(),
            });
            let spend = spend.get(&id).cloned();

            // Ended or quiet sessions show only when they did something today.
            let lively = running_now == Some(true) || matches!(phase, RowPhase::Working | RowPhase::Waiting);
            let today_too = today.is_some() || spend.is_some() || since.is_some_and(|at| at >= midnight) || last >= midnight;
            if !lively && !today_too {
                continue;
            }

            let project = match path.as_deref().and_then(|path| projects.iter().find(|(p, _)| p == path)) {
                Some((_, name)) => name.clone(),
                None => folder_name(&folder),
            };
            rows.push(SessionRow {
                id: id.clone(),
                path,
                folder,
                project,
                title,
                phase,
                turn,
                tool,
                since,
                running: running_now,
                origin,
                today,
                spend,
                subagents: draft.scanned.as_ref().map_or(0, |s| s.subagents),
            });
        }

        cache.first.retain(|id, _| rows.iter().any(|row| &row.id == id));
        cache.origins.retain(|id, _| rows.iter().any(|row| &row.id == id));
        drop(cache);

        let rank = |phase: RowPhase| match phase {
            RowPhase::Waiting => 0,
            RowPhase::Working => 1,
            RowPhase::Idle => 2,
            RowPhase::Ended => 3,
        };
        rows.sort_by(|a, b| {
            let by_since = match a.phase {
                // The longest wait, and the longest run, first; the latest of the rest first.
                RowPhase::Waiting | RowPhase::Working => a.since.cmp(&b.since),
                RowPhase::Idle | RowPhase::Ended => b.since.cmp(&a.since),
            };
            rank(a.phase).cmp(&rank(b.phase)).then(by_since).then(a.id.cmp(&b.id))
        });

        SessionsView { now, hook, unlisted: rows.iter().filter(|row| row.path.is_none()).count(), sessions: rows }
    }

    /// The Pitwall terminal a session runs in, by its parents: its id and project.
    pub(super) fn pitwall_terminal(&self, lineage: &[u32]) -> Option<(u64, String)> {
        self.terminal_shells().into_iter().find(|(shell, _, _)| lineage.contains(shell)).map(|(_, id, path)| (id, path))
    }
}

// A Claude process in the process table, `sh` named `claude`, needs a Unix shell.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Claude Code's sessions file carries the session's name, which it derives from the
    /// conversation, and its log the conversation itself: neither reaches the page, whatever
    /// else of them does.
    #[test]
    fn a_running_session_shows_without_its_name_or_its_words() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let folder = tmp.path().join("elsewhere").join("app").to_string_lossy().into_owned();
        let id = "9bb85e86-f618-4861-9858-03ec8fc36c28";
        let secret = "SECRET fix the login bug for ACME";

        // A Claude process, as far as the process table tells (`claude` in its command line).
        let mut child = std::process::Command::new("sh").args(["-c", "sleep 30; true", "claude"]).spawn().unwrap();
        let pid = child.id();

        fs::create_dir_all(claude.join("sessions")).unwrap();
        fs::write(
            claude.join("sessions").join(format!("{pid}.json")),
            serde_json::json!({
                "pid": pid, "sessionId": id, "cwd": folder, "startedAt": 1_790_855_479_000_u64, "kind": "interactive",
                "entrypoint": "cli", "status": "busy", "statusUpdatedAt": 1_790_855_479_500_u64, "name": secret,
                "nameSource": "derived", "messagingSocketPath": format!("/tmp/{secret}.sock"),
            })
            .to_string(),
        )
        .unwrap();
        fs::write(claude.join("sessions").join(format!("{secret}.key")), secret).unwrap();

        let log = claude.join("projects").join(crate::claude::encode_project_path(&folder)).join(format!("{id}.jsonl"));
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        let lines = [
            serde_json::json!({ "type": "user", "cwd": folder, "message": { "role": "user", "content": secret } }),
            serde_json::json!({ "type": "assistant", "cwd": folder, "timestamp": "2026-10-02T10:00:00.000Z",
                "message": { "stop_reason": "end_turn", "content": [{ "type": "text", "text": secret }] } }),
            serde_json::json!({ "type": "ai-title", "aiTitle": "Login page", "sessionId": id }),
        ];
        fs::write(&log, lines.iter().map(|line| format!("{line}\n")).collect::<String>()).unwrap();

        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: claude.join("projects"),
                settings_file: tmp.path().join("config").join("settings.json"),
                claude_settings: claude.join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        let view = core.sessions_view(&HashMap::new());
        let _ = child.kill();
        let _ = child.wait();

        let row = view.sessions.iter().find(|row| row.id == id).expect("the running session has a row");
        assert_eq!((row.path.as_deref(), row.folder.as_str(), row.project.as_str()), (None, folder.as_str(), "app"));
        assert_eq!((row.phase, row.running, row.title.as_deref()), (RowPhase::Working, Some(true), Some("Login page")));
        assert!(matches!(row.origin, SessionOrigin::Terminal { .. }), "{:?}", row.origin);
        assert_eq!(view.unlisted, 1);

        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("SECRET"), "{json}");
    }
}
