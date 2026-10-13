//! Where Claude stands in each project, from its logs and the hook's events: the sessions that
//! work or wait on you, a waiting turn's notification and sound once it is due, turns seen, and
//! the Claude Code hook itself.

use super::*;
use super::signals::{long_turns, name_of, turn_seen, turns_seen, Signal};
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
            turn_seen(&mut inner, path);
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
                let first = !inner.claude_scanned;
                let mut announce = due_notifications(&mut inner, &scan.waiting, &scan.sessions);
                announce.extend(long_turns(&mut inner, &scan.sessions, first));
                turns_seen(&mut inner, &scan.waiting);
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

        // Banner, sound, quiet and the phone are the door's to decide (`signals.rs`).
        self.signals(announce);

        self.notify();
        // Cards given to Claude follow their sessions from the state just read.
        self.follow_board();
        // And a Claude tab takes its session's name.
        self.name_terminals();
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

/// Waiting turns to announce now, a permission's with the tool the hook named (`sessions`: the
/// scan's). The first scan after launch only takes note, so turns that were already waiting
/// don't arrive as a burst.
fn due_notifications(inner: &mut Inner, waiting: &HashMap<String, Turn>, sessions: &[SessionState]) -> Vec<Signal> {
    let first = !inner.claude_scanned;
    inner.claude_scanned = true;
    inner.candidates.retain(|path, _| waiting.contains_key(path));

    let mut due = Vec::new();

    for (path, turn) in waiting {
        // Not newer than the turn announced last: another session's, which waited all along, or
        // the same turn as its log dates it (the hook's event comes after the log's line).
        if inner.notified.get(path).is_some_and(|announced| turn.at <= *announced) {
            // A newer turn that was about to be announced went back to work meanwhile.
            inner.candidates.remove(path);
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

                let name = name_of(inner, path);
                let (kind, detail) = match turn.kind {
                    TurnKind::Finished => (SignalKind::ClaudeFinished, None),
                    TurnKind::Asking => (SignalKind::ClaudeAsking, None),
                    TurnKind::Permission => {
                        let asker = sessions.iter().find(|session| &session.path == path && session.turn == Some(*turn));
                        (SignalKind::ClaudePermission, asker.and_then(|session| session.tool.clone()))
                    }
                };
                due.push(Signal { kind, path: path.clone(), name, detail });
            }
            Some((at, _)) if *at == turn.at => {}
            _ => {
                inner.candidates.insert(path.clone(), (turn.at, Instant::now()));
            }
        }
    }

    due
}
