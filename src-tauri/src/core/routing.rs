//! Start, stop and restart, routed between participants: a server another participant runs is
//! handled by that participant, a project open in a VS Code window is started there, and work
//! orders from them run here. The spinner shows each action until it is visibly done.

use super::*;
use crate::registry::RemoteCommand;

/// A start counts as done once the server printed its address, or after this long.
const SETTLE_LIMIT: Duration = Duration::from_secs(15);
/// The spinner never outlives this, whatever happens.
const PENDING_LIMIT: Duration = Duration::from_secs(30);

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

/// A start/stop/restart the user asked for and hasn't seen finish yet; drives the spinner.
pub(super) struct Pending {
    pub(super) action: Action,
    at: Instant,
    at_ms: u64,
}

impl Core {
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

    pub(super) fn has_pending(&self) -> bool {
        !self.lock().pending.is_empty()
    }

    pub(super) fn act_now(self: &Arc<Self>, path: &str, action: Action) {
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
}

/// Drops spinners whose action is visibly done (or that ran out of time).
pub(super) fn prune_pending(inner: &mut Inner) {
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
