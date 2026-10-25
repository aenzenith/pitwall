//! What the UI shows: every project with its server, Claude and Git state, built from `Inner`
//! and announced (`CoreEvent::State`) only when it changed.

use super::*;
use super::routing::prune_pending;
use super::servers::run_command;
use super::signals::Signal;

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
    /// While it works: since when, as the hook told it (none without the hook).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// While it waits on a permission prompt: the tool's name, when the hook told it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub path: String,
    pub name: String,
    pub favourite: bool,
    /// Among the projects added to Pitwall (`Settings::added`): listed while no window has it open
    /// and nothing of it runs, and "Remove from Pitwall" takes it out.
    pub added: bool,
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
    /// How the panel lays them out, while not one at a time (`layout.rs`).
    pub layout: Option<super::TerminalLayout>,
    /// Nothing is told about it (`signals.rs`).
    pub muted: bool,
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
    /// Pitwall's Claude Code hook is installed (turns, permission prompts and questions are
    /// reported the moment they happen).
    pub claude_hook: bool,
    /// The hook is installed but older than this version's; installing it again updates it.
    pub claude_hook_outdated: bool,
    /// The board could not be written to disk: what changed on it lives only while the app runs.
    pub board_unsaved: bool,
    pub settings: Settings,
    /// The language the UI speaks: Settings' choice, else the system's.
    pub language: &'static str,
    /// The system's language, for the "System" choice in Settings.
    pub system_language: &'static str,
    /// What was open at the last quit, while the user hasn't answered (`resume.rs`).
    pub resume: Option<ResumeOffer>,
    /// This version's notes are still to be shown (`whatsnew.rs`).
    pub whats_new: bool,
    /// Administrator's tabs the last quit or update had open, which no record puts back, until
    /// the terminal panel has said so.
    pub admin_not_restored: usize,
    /// Projects that appeared in the projects folder (`discover.rs`).
    pub suggestions: Vec<Candidate>,
    /// It is quiet now, and until when (`signals.rs`).
    pub quiet: Option<QuietView>,
    /// What the user wasn't told meanwhile, newest first.
    pub missed: Vec<Missed>,
    /// How many calls came over the team radio: when it moves, a session's calls are asked for
    /// again (`radio`). The calls themselves are no part of the snapshot.
    pub radio_calls: u64,
    /// Its place among the snapshots taken for the windows (`Core::snapshot`), counted under the
    /// core's lock: two sent from different threads can reach a window in either order, and a
    /// window keeps the one taken last. 0 in the one `notify` compares, so it never counts as a
    /// change.
    pub seq: u64,
}

/// The last `Snapshot::seq` given.
static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Core {
    pub fn snapshot(&self) -> Snapshot {
        let inner = self.lock();
        // Under the lock: a later number is a later state.
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        Snapshot { seq, ..build_snapshot(&inner) }
    }

    /// Emits `State` only when the snapshot actually changed.
    pub fn notify(&self) {
        let (changed, turned) = {
            let mut inner = self.lock();
            prune_pending(&mut inner);
            let snapshot = build_snapshot(&inner);
            let turned = server_turns(&mut inner, &snapshot.projects);
            let json = serde_json::to_string(&snapshot).unwrap_or_default();

            if json == inner.last_state {
                (false, turned)
            } else {
                inner.last_state = json;
                (true, turned)
            }
        };

        // Crashes are told, servers that came up are heard (`signals.rs`), the lock let go first:
        // however many turned at once, each sound once.
        let (crashed, up): (Vec<_>, Vec<_>) = turned.into_iter().partition(|(_, _, crashed)| *crashed);
        self.signals(crashed.into_iter().map(|(path, name, _)| Signal { kind: SignalKind::ServerCrashed, path, name, detail: None }).collect());
        if !up.is_empty() {
            let up: Vec<&str> = up.iter().map(|(path, ..)| path.as_str()).collect();
            self.sound(&up, |sounds| &sounds.server_ready);
        }
        if changed {
            self.emit(CoreEvent::State);
        }
    }

    pub fn popover_opened(&self) {
        self.lock().crash_unseen = false;
        self.notify();
    }
}

/// Servers, ours or a peer's, that just crashed (`true`) or came up, by path and name. A project
/// first seen is neither.
fn server_turns(inner: &mut Inner, projects: &[ProjectView]) -> Vec<(String, String, bool)> {
    let mut turned = Vec::new();

    for project in projects {
        let before = inner.statuses.insert(project.path.clone(), project.status);
        let crashed = match (before, project.status) {
            (Some(was), "crashed") if was != "crashed" => true,
            (Some(was), "running") if was != "running" => false,
            _ => continue,
        };
        turned.push((project.path.clone(), project.name.clone(), crashed));
    }

    turned
}

/// The spelling a project is listed under (see `build_snapshot`), for a path another
/// participant gave: on Windows one folder can come as `c:\…` and as `C:\…`. A path not
/// listed stays as given.
pub(super) fn listed_path(inner: &Inner, path: &str) -> String {
    inner
        .favourites
        .iter()
        .map(|favourite| favourite.path.as_str())
        .chain(inner.settings.added.iter().flatten().map(String::as_str))
        .chain(inner.peers.iter().flat_map(|peer| peer.owned().map(|project| project.folder_path.as_str())))
        .chain(inner.runs.keys().map(String::as_str))
        .find(|known| same_path(known, path))
        .unwrap_or(path)
        .to_string()
}

/// Every project the app shows: favourites, the other projects added to it, projects of open
/// editor windows, and whatever runs.
pub(super) fn build_snapshot(inner: &Inner) -> Snapshot {
    let mut order: Vec<String> = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    // One row per folder: on Windows a peer's `c:\…` is the app's `C:\…`.
    let mut add = |path: &str, name: &str| {
        if !order.iter().any(|known| same_path(known, path)) {
            order.push(path.to_string());
            names.insert(path.to_string(), name.to_string());
        }
    };

    for favourite in &inner.favourites {
        add(&favourite.path, &favourite.name);
    }
    for path in inner.settings.added.iter().flatten() {
        add(path, &folder_name(path));
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
                .find_map(|peer| peer.projects.iter().find(|p| same_path(&p.folder_path, path) && p.running).map(|p| (peer, p)));
            let open_in = inner.peers.iter().find(|peer| peer.has_root(path)).map(|peer| peer.title.clone());
            let run = inner.runs.get(path);
            let issue = inner.issues.get(path).cloned().or_else(|| {
                inner.peers.iter().find_map(|peer| peer.projects.iter().find(|p| same_path(&p.folder_path, path)).and_then(|p| p.issue.clone()))
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
                favourite: inner.favourites.iter().any(|f| same_path(&f.path, path)),
                added: inner.settings.added.iter().flatten().any(|known| same_path(known, path)),
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
                layout: inner.terminal_layouts.get(path).cloned(),
                muted: super::signals::muted(inner, path),
            }
        })
        .collect();

    // A fixed order: the teams' projects first (the teams in their order, each one's projects in
    // its own), then the order the user dragged into place, then everything else by name. State
    // changes (Claude, servers) never move a row.
    let teams = super::teams::places(&inner.settings.teams);
    let position = |p: &ProjectView| inner.settings.order.iter().position(|path| path == &p.path);
    let loose = |a: &ProjectView, b: &ProjectView| match (position(a), position(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    };
    projects.sort_by(|a, b| match (teams.get(a.path.as_str()), teams.get(b.path.as_str())) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => loose(a, b),
    });

    Snapshot {
        running: projects.iter().filter(|p| p.status == "running").count(),
        waiting: projects.iter().filter(|p| p.claude.is_some()).count(),
        crashed: projects.iter().filter(|p| p.status == "crashed").count(),
        crash_unseen: inner.crash_unseen,
        claude_hook: inner.claude_hook,
        claude_hook_outdated: inner.claude_hook_outdated,
        board_unsaved: inner.board_unsaved,
        // Without the folders known: up to thousands of names only the core reads.
        settings: Settings { seen_folders: Vec::new(), ..inner.settings.clone() },
        language: i18n::resolve(&inner.settings.language),
        system_language: i18n::system(),
        resume: super::resume::offer(inner),
        whats_new: inner.whats_new,
        admin_not_restored: inner.admin_not_restored,
        suggestions: inner.suggestions.clone(),
        quiet: super::signals::quiet_at(&inner.settings.notifications, now_ms()),
        missed: super::signals::listed(&inner.missed, &projects),
        projects,
        radio_calls: inner.radio.noted,
        seq: 0,
    }
}
