//! Where Claude stands in each project, from its logs and the hook's events: the sessions that
//! work or wait on you, a waiting turn's notification and sound once it is due, turns seen, and
//! the Claude Code hook itself.

use super::*;
use super::snapshot::build_snapshot;
use crate::claude::TurnKind;

/// A waiting turn is announced only if it is still waiting this long after it was first seen:
/// a focused VS Code window marks it seen in the meantime.
const NOTIFY_AFTER: Duration = Duration::from_secs(4);

impl Core {
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
            // The Sessions page's too, a folder that isn't listed included, until the next scan.
            for session in inner.scanned.iter_mut().filter(|session| session.phase == SessionPhase::Waiting) {
                let here = if session.path.is_empty() { session.folder.as_deref().is_some_and(|folder| same_path(folder, path)) } else { session.path == path };
                if here {
                    session.phase = SessionPhase::Idle;
                    session.turn = None;
                }
            }
        }
        self.notify();
    }

    /// A waiting turn's notification falls due before the next heartbeat.
    pub(super) fn claude_due(&self) -> bool {
        self.lock().candidates.values().any(|(_, since)| since.elapsed() >= NOTIFY_AFTER)
    }

    /// Where Claude stands, from what was read so far (`ClaudeWatch::evaluate`): working and
    /// waiting projects, their notifications once due, sounds and the day's timeline. Nothing
    /// before the first full read; false then.
    pub(super) fn refresh_claude(&self) -> bool {
        // Before the watch is locked: it may read the process table.
        let busy = self.busy_sessions();
        let announce = {
            let Ok(mut watch) = self.claude.lock() else {
                return false;
            };
            if !watch.ready() {
                return false;
            }

            watch.set_busy(busy);
            let scan = watch.evaluate();
            let (announce, projects) = {
                let mut inner = self.lock();
                let announce = due_notifications(&mut inner, &scan.waiting);
                inner.waiting = scan.waiting;
                inner.working = scan.working;
                inner.claude_sessions = live_sessions(&scan.sessions);
                inner.scanned = scan.sessions.iter().chain(&scan.others).cloned().collect();
                (announce, build_snapshot(&inner).projects)
            };
            // Still under the watch's lock: two refreshes never write the timeline out of order.
            self.record_activity(&scan.sessions, &projects);
            announce
        };

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
        // Cards given to Claude follow their sessions from the state just read.
        self.follow_board();
        true
    }

    /// Installs the hook, or brings an older one up to date.
    pub fn install_claude_hook(&self) -> Result<(), String> {
        let result = self.hook.install();
        self.update_hook_status();
        result
    }

    pub fn uninstall_claude_hook(&self) -> Result<(), String> {
        let result = self.hook.uninstall();
        self.update_hook_status();
        result
    }

    fn update_hook_status(&self) {
        let status = self.hook.status();
        {
            let mut inner = self.lock();
            inner.claude_hook = status.installed;
            inner.claude_hook_outdated = status.outdated;
        }
        self.notify();
    }
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
