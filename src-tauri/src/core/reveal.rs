//! A click on a Claude session (the day view, the Sessions page) brings it up where it runs. In
//! one of Pitwall's own terminals, the main window shows that terminal. Elsewhere the project's
//! window comes to the front, and that window's extension (a `reveal-claude` command) shows the
//! Claude Code tab or the terminal the session runs in; a finished session opens again in a
//! Claude Code tab. Anything that can't be proven safe only brings the window up: a session
//! opened a second time would have two processes writing one log.
//!
//! Which sessions run, and under which processes, comes from Claude Code's
//! `~/.claude/sessions/<pid>.json` (see `sessions.rs` for what is read of it).

use super::*;

/// How long a finished session waits for its project's window to open and take the command.
const WINDOW_WAIT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Liveness {
    /// Its process runs: how it was started (`claude-vscode`, `cli`…) and the process with its
    /// parents, nearest first.
    Running { entrypoint: Option<String>, lineage: Vec<u32> },
    Ended,
    /// No way to tell: Claude Code keeps no sessions folder, the process table can't be read,
    /// or the log changed a moment ago without a running process (an older Claude Code).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reveal {
    /// Bring `raise` to the front and ask `target` to show the session; `terminal` is the shell
    /// pid of the terminal it runs in.
    Command { target: String, raise: String, terminal: Option<u32> },
    /// Only bring the project's window to the front.
    Window,
}

/// Session ids are the log's file name, a UUID; anything else never reaches a path or a command.
pub(super) fn is_session_id(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// Where a session is shown, from what is known about its process and the live participants.
pub(super) fn plan_reveal(path: &str, liveness: &Liveness, peers: &[WindowRecord]) -> Reveal {
    let capable = || peers.iter().filter(|peer| peer.takes("reveal-claude"));
    let raise = |peer: &WindowRecord| {
        let roots = peer.roots.as_deref().unwrap_or_default();
        if peer.has_root(path) { path.to_string() } else { roots.first().cloned().unwrap_or_else(|| path.to_string()) }
    };

    match liveness {
        Liveness::Running { entrypoint, lineage } => {
            // A terminal of a window: the session's shell is one of the process's parents.
            for peer in capable() {
                let terminals = peer.terminals.as_deref().unwrap_or_default();

                if let Some(&shell) = lineage.iter().find(|pid| terminals.contains(pid)) {
                    return Reveal::Command { target: peer.window_id.clone(), raise: raise(peer), terminal: Some(shell) };
                }
            }

            // A Claude Code tab: its process descends from that window's extension host, which is
            // the process that writes the window's record. There Claude Code brings the open tab
            // up instead of starting a second process.
            if entrypoint.as_deref() == Some("claude-vscode") {
                for peer in capable() {
                    if peer.pid().is_some_and(|host| lineage.contains(&host)) {
                        return Reveal::Command { target: peer.window_id.clone(), raise: raise(peer), terminal: None };
                    }
                }
            }

            Reveal::Window
        }
        Liveness::Ended => match capable().find(|peer| peer.has_root(path)) {
            Some(peer) => Reveal::Command { target: peer.window_id.clone(), raise: path.to_string(), terminal: None },
            None => Reveal::Window,
        },
        Liveness::Unknown => Reveal::Window,
    }
}

impl Core {
    /// Brings a Claude session up where it runs (see the top of this file). `path` is its
    /// project, or the folder it runs in when that isn't listed.
    pub fn reveal_claude(self: &Arc<Self>, path: &str, session: &str) {
        self.mark_seen(path);

        let core = Arc::clone(self);
        let (path, session) = (path.to_string(), session.to_string());

        thread::spawn(move || core.reveal_now(&path, &session));
    }

    fn reveal_now(&self, path: &str, session: &str) {
        let liveness = if is_session_id(session) { self.liveness(session) } else { Liveness::Unknown };

        // One of our own terminals: the window shows it.
        if let Liveness::Running { lineage, .. } = &liveness {
            if let Some((id, path)) = self.pitwall_terminal(lineage) {
                self.emit(CoreEvent::RevealTerminal { path, id });
                return;
            }
        }

        match plan_reveal(path, &liveness, &self.registry.read_peers()) {
            Reveal::Command { target, raise, terminal } => {
                self.launch_editor(&raise);
                self.registry.send_reveal(&target, path, session, terminal);
            }
            Reveal::Window => {
                self.launch_editor(path);

                // A finished session can wait for the window that is opening now.
                if liveness == Liveness::Ended {
                    self.reveal_when_open(path, session);
                }
            }
        }
    }

    fn reveal_when_open(&self, path: &str, session: &str) {
        let deadline = Instant::now() + WINDOW_WAIT;

        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(500));

            if let Reveal::Command { target, .. } = plan_reveal(path, &Liveness::Ended, &self.registry.read_peers()) {
                self.registry.send_reveal(&target, path, session, None);
                return;
            }
        }
    }

    fn liveness(&self, session: &str) -> Liveness {
        let Some(running) = self.read_running() else {
            return Liveness::Unknown;
        };

        if let Some(run) = running.get(session) {
            return Liveness::Running { entrypoint: run.entrypoint.clone(), lineage: run.lineage.clone() };
        }

        match self.log_modified(session) {
            Some(modified) if now_ms().saturating_sub(modified) > crate::claude::WORKING_WINDOW_MS => Liveness::Ended,
            _ => Liveness::Unknown,
        }
    }

    /// The session's log (`<session>.jsonl` in one of Claude Code's project folders; the last
    /// written, should several have one) and when it last changed; `None` when there's no such
    /// log. `session` must be a session id: it becomes a file name.
    pub(super) fn session_log(&self, session: &str) -> Option<(PathBuf, u64)> {
        let file = format!("{session}.jsonl");

        fs::read_dir(&self.cfg.claude_dir)
            .ok()?
            .flatten()
            .filter_map(|dir| {
                let log = dir.path().join(&file);
                let modified = fs::metadata(&log).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
                Some((log, modified.as_millis() as u64))
            })
            .max_by_key(|(_, modified)| *modified)
    }

    /// When the session's log last changed; `None` when there's no such log.
    fn log_modified(&self, session: &str) -> Option<u64> {
        self.session_log(session).map(|(_, modified)| modified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "/Users/me/projects/paddock";

    fn window(host: u32, takes: bool, terminals: &[u32]) -> WindowRecord {
        WindowRecord {
            window_id: format!("{}-lq8x", crate::registry::base36(u64::from(host))),
            title: "paddock".into(),
            updated_at: now_ms(),
            projects: Vec::new(),
            roots: Some(vec![PROJECT.into()]),
            features: takes.then(|| vec!["reveal-claude".into()]),
            terminals: Some(terminals.to_vec()),
        }
    }

    fn running(entrypoint: &str, lineage: &[u32]) -> Liveness {
        Liveness::Running { entrypoint: Some(entrypoint.into()), lineage: lineage.to_vec() }
    }

    fn command(host: u32, terminal: Option<u32>) -> Reveal {
        Reveal::Command { target: window(host, true, &[]).window_id, raise: PROJECT.into(), terminal }
    }

    /// A running session is never opened a second time (two processes would write one log), and
    /// an extension that doesn't list `reveal-claude` never gets it (old ones start the server).
    #[test]
    fn a_session_is_only_brought_up_where_that_is_safe() {
        let host = 500;
        let other = 600;
        let shell = 700;
        let cases = [
            // A terminal of the window: its terminal, never a tab.
            (running("cli", &[900, shell, 650]), vec![window(host, true, &[shell])], command(host, Some(shell))),
            (running("claude-vscode", &[900, shell]), vec![window(host, true, &[shell])], command(host, Some(shell))),
            // A terminal no window knows (iTerm…): only the window, although one has the project.
            (running("cli", &[900, 800]), vec![window(host, true, &[shell])], Reveal::Window),
            // A Claude Code tab: only the window whose extension host runs it.
            (running("claude-vscode", &[900, host]), vec![window(other, true, &[]), window(host, true, &[])], command(host, None)),
            (running("claude-vscode", &[900, 650]), vec![window(host, true, &[])], Reveal::Window),
            (running("cli", &[900, host]), vec![window(host, true, &[])], Reveal::Window),
            // Old extension versions get nothing.
            (running("claude-vscode", &[900, host]), vec![window(host, false, &[])], Reveal::Window),
            (running("cli", &[900, shell]), vec![window(host, false, &[shell])], Reveal::Window),
            (Liveness::Ended, vec![window(host, false, &[])], Reveal::Window),
            // A finished session opens again in a window that has the project.
            (Liveness::Ended, vec![window(host, true, &[])], command(host, None)),
            // Unsure whether it runs: only the window.
            (Liveness::Unknown, vec![window(host, true, &[])], Reveal::Window),
        ];

        for (liveness, peers, expected) in cases {
            assert_eq!(plan_reveal(PROJECT, &liveness, &peers), expected, "{liveness:?}");
        }

        // The id from the UI reaches a file path and a command only as a UUID.
        assert!(is_session_id("9bb85e86-f618-4861-9858-03ec8fc36c28"));
        for bad in ["", "../../etc/passwd", "9bb85e86-f618-4861-9858-03ec8fc36c2/", "9bb85e86f61848619858103ec8fc36c28aa"] {
            assert!(!is_session_id(bad), "{bad}");
        }
    }
}
