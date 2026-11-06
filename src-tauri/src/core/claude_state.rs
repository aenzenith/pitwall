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
    /// The project looked at: what Claude waits on you with there counts as seen, up to a
    /// permission prompt or a question still open in a session that runs, which waits until it is
    /// answered (`ClaudeWatch::mark_seen`), and so does whatever came after it.
    pub fn mark_seen(&self, path: &str) {
        // Before the watch is locked: it may read the process table.
        let running: Option<HashSet<String>> = self.running_cached().map(|running| running.into_keys().collect());
        // Codex's permission requests still open there wait as Claude's do; the seen book is
        // shared, so a look clears both agents' finished turns.
        let codex_open = self.codex_open_prompt(path);
        let seen = match self.claude.lock() {
            Ok(mut watch) => watch.mark_seen_before(path, running.as_ref(), codex_open),
            Err(_) => now_ms(),
        };
        let unseen = |turn: Option<Turn>| turn.is_some_and(|turn| turn.at > seen);
        {
            let mut inner = self.lock();
            if unseen(inner.waiting.get(path).copied()) {
                // What still waits keeps its line; a long turn listed of the project was seen.
                inner.missed.retain(|entry| !(entry.kind == SignalKind::ClaudeLongTurn && same_path(&entry.path, path)));
            } else {
                inner.waiting.remove(path);
                turn_seen(&mut inner, path);
            }
            if let Some(sessions) = inner.claude_sessions.get_mut(path) {
                sessions.retain(|session| session.phase != SessionPhase::Waiting || unseen(session.turn));
            }
            // The Sessions page's too, a folder that isn't listed included, until the next scan.
            for session in inner.scanned.iter_mut().filter(|session| session.phase == SessionPhase::Waiting && !unseen(session.turn)) {
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

    /// Claude's watch set to the listed projects while Claude is on, read once more and its hook's
    /// events taken (its hook's state read again as it comes back on); while it is off, let go:
    /// nothing of it is read (its logs, its running sessions, its hook's file), and what its hook
    /// still writes into `claude-events/` is deleted unread once a day old.
    pub(super) fn follow_claude(&self, paths: &[String]) {
        let on = self.agent_on(Agent::Claude);
        let was_off = {
            let mut cache = self.sessions_cache();
            let was_off = std::mem::replace(&mut cache.claude_off, !on);
            if !on {
                cache.forget_claude();
            }
            was_off
        };
        if on {
            if let Ok(mut claude) = self.claude.lock() {
                claude.set_listed(paths);
                claude.rescan();
            }
            crate::claude::take_events(&self.claude);
            if was_off {
                self.update_hook_status();
            }
        } else {
            let events = match self.claude.lock() {
                Ok(mut claude) => {
                    claude.let_go();
                    claude.events_dir().to_path_buf()
                }
                Err(_) => return,
            };
            crate::codex::sweep_events(&events);
        }
    }

    /// Where Claude stands, from what was read so far (`ClaudeWatch::evaluate`): working and
    /// waiting projects, their notifications once due, sounds and the day's timeline. Nothing
    /// before the first full read; false then.
    pub(super) fn refresh_claude(&self) -> bool {
        // Before the watch is locked: it may read the process table.
        let busy = self.busy_sessions();
        let idle = self.idle_sessions();
        // Taken before the watch is locked: the laps take their lock alone.
        let apart = self.lap_sessions();
        self.codex_set_running();
        let announce = {
            let Ok(mut watch) = self.claude.lock() else {
                return false;
            };
            if !watch.ready() {
                return false;
            }

            watch.set_busy(busy);
            watch.set_idle(idle);
            let mut scan = watch.evaluate();
            // Every other agent that is on, as if Claude had its sessions too.
            let merged = self.merge_agents(&mut scan);
            // The laps' sessions are no project's: none of what follows hears of them.
            let laps_own = set_apart(&mut scan, &apart);
            let (announce, projects) = {
                let mut inner = self.lock();
                let first = !inner.claude_scanned;
                let fresh = first_scans(&mut inner, &merged, &scan);
                let mut announce = due_notifications(&mut inner, &scan.waiting, &scan.sessions);
                announce.extend(long_turns(&mut inner, &scan.sessions, first, &fresh));
                turns_seen(&mut inner, &scan.waiting);
                inner.waiting = scan.waiting;
                inner.working = scan.working;
                inner.claude_sessions = live_sessions(&scan.sessions);
                inner.scanned = scan.sessions.iter().chain(&scan.others).cloned().collect();
                inner.lap_scanned = laps_own.clone();
                (announce, build_snapshot(&inner).projects)
            };
            // Still under the watch's lock: two refreshes never write the timeline out of order.
            self.record_activity(&scan.sessions, &projects);
            (announce, laps_own)
        };

        // A lap's session ends as the lap tells it (`LapDone`, `LapFailed`); what it waits on is
        // told by the laps (`lap_waits`), asked with no lock held: they take theirs alone.
        let (mut announce, laps_own) = announce;
        announce.extend(self.lap_waits(&laps_own));

        // Banner, sound, quiet and the phone are the door's to decide (`signals.rs`).
        self.signals(announce);

        // Codex's hook is at work once its events come.
        self.update_codex_hook_status();
        self.notify();
        // Cards given to Claude follow their sessions from the state just read.
        self.follow_board();
        // And a Claude tab takes its session's name.
        self.name_terminals();
        true
    }

    /// Installs the hook, or brings an older one up to date; in every other Claude account's own
    /// settings too (`account_hook`).
    pub fn install_claude_hook(&self) -> Result<(), String> {
        let result = self.hook.install();
        if result.is_ok() {
            self.account_hooks(true);
        }
        self.update_hook_status();
        result
    }

    pub fn uninstall_claude_hook(&self) -> Result<(), String> {
        let result = self.hook.uninstall();
        self.account_hooks(false);
        // Taken out while Claude is off: its settings aren't read again for it.
        if result.is_ok() && self.sessions_cache().claude_off {
            let mut inner = self.lock();
            inner.claude_hook = false;
            inner.claude_hook_outdated = false;
        }
        self.update_hook_status();
        result
    }

    pub(super) fn update_hook_status(&self) {
        // Claude's own settings file is opened for nothing while it is off: its last state stays.
        if self.sessions_cache().claude_off {
            return;
        }
        let status = self.hook.status();
        {
            let mut inner = self.lock();
            inner.claude_hook = status.installed;
            inner.claude_hook_outdated = status.outdated;
        }
        self.notify();
    }
}

/// The sessions the laps' runs hold (`apart`, `Core::lap_sessions`) taken out of `scan` and
/// given back: they are the laps' own layer, in no project's state, count or list (Needs
/// attention, the Sessions page, the day's timeline, the tower). A project one of them was in
/// waits and works by its other sessions alone, reckoned as `evaluate` does: with the newest turn
/// one of them waits with, else at work while one of them works.
fn set_apart(scan: &mut crate::claude::ScanResult, apart: &HashSet<String>) -> Vec<SessionState> {
    if apart.is_empty() {
        return Vec::new();
    }
    let (own, sessions): (Vec<SessionState>, Vec<SessionState>) = std::mem::take(&mut scan.sessions).into_iter().partition(|session| apart.contains(&session.id));
    scan.sessions = sessions;
    let (others, kept): (Vec<SessionState>, Vec<SessionState>) = std::mem::take(&mut scan.others).into_iter().partition(|session| apart.contains(&session.id));
    scan.others = kept;

    let touched: HashSet<&str> = own.iter().map(|session| session.path.as_str()).collect();
    for path in touched {
        scan.waiting.remove(path);
        scan.working.remove(path);
        let left = || scan.sessions.iter().filter(|session| session.path == path);
        match left().filter(|session| session.phase == SessionPhase::Waiting).filter_map(|session| session.turn).max_by_key(|turn| turn.at) {
            Some(turn) => {
                scan.waiting.insert(path.to_string(), turn);
            }
            None if left().any(|session| session.phase == SessionPhase::Working) => {
                scan.working.insert(path.to_string());
            }
            None => {}
        }
    }
    own.into_iter().chain(others).collect()
}

/// The sessions working or waiting on you, by project.
fn live_sessions(sessions: &[crate::claude::SessionState]) -> HashMap<String, Vec<LiveSession>> {
    let mut found: HashMap<String, Vec<LiveSession>> = HashMap::new();
    for session in sessions.iter().filter(|s| s.phase != SessionPhase::Idle) {
        // Only the hook tells when the work began; a log's last line doesn't.
        let since = session.since.filter(|_| session.hooked && session.phase == SessionPhase::Working);
        let live = LiveSession { id: session.id.clone(), agent: session.agent, phase: session.phase, turn: session.turn, since, tool: session.tool.clone() };
        found.entry(session.path.clone()).or_default().push(live);
    }
    found
}

/// The agents `merged` into this scan for the first time since they came on (found, switched on);
/// one no longer merged is forgotten, and is new again when it comes back. What they wait with now
/// waited before: it is taken note of as announced, as Claude's first scan after launch does, so
/// an agent coming on tells of nothing that was there already.
fn first_scans(inner: &mut Inner, merged: &[Agent], scan: &crate::claude::ScanResult) -> Vec<Agent> {
    inner.agents_scanned.retain(|agent| merged.contains(agent));
    let fresh: Vec<Agent> = merged.iter().copied().filter(|agent| inner.agents_scanned.insert(*agent)).collect();
    for (path, turn) in &scan.waiting {
        if scan.sessions.iter().any(|session| &session.path == path && session.turn == Some(*turn) && fresh.contains(&session.agent)) {
            let noted = inner.notified.entry(path.clone()).or_insert(0);
            *noted = (*noted).max(turn.at);
        }
    }
    fresh
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
                // The session that waits with the turn: its agent, and a permission's tool.
                let asker = sessions.iter().find(|session| &session.path == path && session.turn == Some(*turn));
                let agent = asker.map_or(Agent::Claude, |session| session.agent);
                let (kind, detail) = match turn.kind {
                    TurnKind::Finished => (SignalKind::ClaudeFinished, None),
                    TurnKind::Asking => (SignalKind::ClaudeAsking, None),
                    TurnKind::Permission => (SignalKind::ClaudePermission, asker.and_then(|session| session.tool.clone())),
                };
                due.push(Signal { kind, path: path.clone(), name, detail, agent });
            }
            Some((at, _)) if *at == turn.at => {}
            _ => {
                inner.candidates.insert(path.clone(), (turn.at, Instant::now()));
            }
        }
    }

    due
}
