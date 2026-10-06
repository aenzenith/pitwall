//! A click on a Claude session (the day view, the Sessions and Track pages; the popover, the
//! switcher and a notification, which name only its project: `open_claude`) brings it up where
//! it runs. In one of Pitwall's own terminals, the main window shows that terminal (the board
//! card it was given from, else the project's terminal panel). Elsewhere the project's
//! window comes to the front, and that window's extension (a `reveal-claude` command) shows the
//! Claude Code tab or the terminal the session runs in; a finished session opens again in a
//! Claude Code tab. One that has just stopped (no process of Claude Code's runs it, its log
//! written a moment ago) opens again in a Pitwall terminal, wherever it ran. Anything that can't
//! be proven safe only brings the window up: a session opened a second time would have two
//! processes writing one log.
//!
//! Which sessions run, and under which processes, comes from Claude Code's
//! `~/.claude/sessions/<pid>.json` (see `sessions.rs` for what is read of it).

use super::terminal::Launch;
use super::*;

/// How long a finished session waits for its project's window to open and take the command.
const WINDOW_WAIT: Duration = Duration::from_secs(15);

/// In `SessionsCache::reopened`, a session whose tab is being opened right now.
const OPENING: u64 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Liveness {
    /// Its process runs: how it was started (`claude-vscode`, `cli`…) and the process with its
    /// parents, nearest first.
    Running { entrypoint: Option<String>, lineage: Vec<u32> },
    Ended,
    /// Claude Code lists its running sessions and this one isn't among them, though its log
    /// changed a moment ago: it has just stopped, wherever it ran.
    Stopped,
    /// No way to tell: Claude Code keeps no sessions folder, the process table can't be read,
    /// or the session has no log.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reveal {
    /// Bring `raise` to the front and ask `target` to show the session; `terminal` is the shell
    /// pid of the terminal it runs in.
    Command { target: String, raise: String, terminal: Option<u32> },
    /// Open it again in one of Pitwall's own terminals.
    Reopen,
    /// Only bring the project's window to the front.
    Window,
}

/// What bringing up a session that has just stopped does about its tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    /// Nobody holds it: the caller opens its tab, and says which once it is open.
    Open,
    /// It was opened again in this tab, which is still there.
    Tab(u64),
    /// Another click is opening its tab right now.
    Opening,
}

/// Takes the opening of `session`'s tab, unless it has one (`open`: that tab is still there) or
/// one is on its way: asked twice before its process is listed, it is opened once.
fn claim(reopened: &mut HashMap<String, u64>, session: &str, open: impl Fn(u64) -> bool) -> Claim {
    match reopened.get(session).copied() {
        Some(OPENING) => Claim::Opening,
        Some(tab) if open(tab) => Claim::Tab(tab),
        _ => {
            reopened.insert(session.to_string(), OPENING);
            Claim::Open
        }
    }
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
        Liveness::Stopped => Reveal::Reopen,
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

    /// A project whose Claude waits on you, opened where no one session is named (the popover,
    /// the switcher, a notification's click): the session that began to wait last comes up
    /// where it runs, as `reveal_claude` brings it. With none to name (its turn was read from a
    /// log no running session owns), the project's editor window.
    pub fn open_claude(self: &Arc<Self>, path: &str) {
        let waiting = self.lock().claude_sessions.get(path).and_then(|sessions| {
            let waiting = sessions.iter().filter(|session| session.phase == SessionPhase::Waiting);
            waiting.max_by_key(|session| session.turn.map(|turn| turn.at)).map(|session| session.id.clone())
        });

        match waiting {
            Some(session) => self.reveal_claude(path, &session),
            None => self.open_editor(path),
        }
    }

    fn reveal_now(self: &Arc<Self>, path: &str, session: &str) {
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
            Reveal::Reopen if self.reopen(path, session) => {}
            // Without a terminal to open it in, the window as well.
            Reveal::Reopen | Reveal::Window => {
                self.launch_editor(path);

                // A finished session can wait for the window that is opening now.
                if liveness == Liveness::Ended {
                    self.reveal_when_open(path, session);
                }
            }
        }
    }

    /// Opens a session that has just stopped again (`claude --resume`) in a new tab of its
    /// project's terminals and shows it; a card that holds it follows it there. `false` when
    /// there is no such tab to open: a folder that isn't listed has no terminals, and Claude Code
    /// finds a session only from the folder it ran in. Asked again before its process is listed,
    /// the tab it was opened in comes up instead: never a second process on one log.
    fn reopen(self: &Arc<Self>, path: &str, session: &str) -> bool {
        let Some(path) = self.listed_paths().into_iter().find(|listed| same_path(listed, path)) else {
            return false;
        };
        let log = self.cfg.claude_dir.join(crate::claude::encode_project_path(&path)).join(format!("{session}.jsonl"));
        if !log.is_file() {
            return false;
        }

        let claimed = {
            let mut cache = self.sessions_cache();
            claim(&mut cache.reopened, session, |tab| self.terminal_sessions().contains_key(&tab))
        };
        let tab = match claimed {
            Claim::Opening => return true,
            Claim::Tab(tab) => tab,
            Claim::Open => {
                let opened = self.open_terminal_as(&path, &Launch::Resume { session: session.to_string() }, None, false, None);
                let mut cache = self.sessions_cache();
                let Ok(view) = opened else {
                    cache.reopened.remove(session);
                    return false;
                };
                cache.reopened.insert(session.to_string(), view.id);
                drop(cache);

                self.board_resumed(session, view.id);
                view.id
            }
        };

        self.emit(CoreEvent::RevealTerminal { path, id: tab });
        true
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

    pub(super) fn liveness(&self, session: &str) -> Liveness {
        let Some(running) = self.read_running() else {
            return Liveness::Unknown;
        };

        if let Some(run) = running.get(session) {
            return Liveness::Running { entrypoint: run.entrypoint.clone(), lineage: run.lineage.clone() };
        }

        match self.log_modified(session) {
            Some(modified) if now_ms().saturating_sub(modified) > crate::claude::WORKING_WINDOW_MS => Liveness::Ended,
            Some(_) => Liveness::Stopped,
            None => Liveness::Unknown,
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
            // Just stopped: one of Pitwall's own terminals, although a window has the project.
            (Liveness::Stopped, vec![window(host, true, &[])], Reveal::Reopen),
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

    /// A session opens again only when Claude Code's own list says no process runs it: without
    /// that list a fresh log may be a running session's. And asked twice before the new process
    /// is listed, it is opened once: two would write one log.
    #[test]
    fn a_session_opens_again_only_once_and_only_when_it_is_known_to_have_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let id = "9bb85e86-f618-4861-9858-03ec8fc36c28";
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

        let folder = claude.join("projects").join(crate::claude::encode_project_path(PROJECT));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{id}.jsonl")), "{}\n").unwrap();

        // No sessions folder (an older Claude Code): nothing tells that it stopped.
        assert_eq!(core.liveness(id), Liveness::Unknown);
        fs::create_dir_all(claude.join("sessions")).unwrap();
        assert_eq!(core.liveness(id), Liveness::Stopped);
        // Without a log there is nothing to open again.
        assert_eq!(core.liveness("1a2b3c4d-f618-4861-9858-03ec8fc36c28"), Liveness::Unknown);

        let mut reopened = HashMap::new();
        let open = |tab: u64| tab == 7;
        assert_eq!(claim(&mut reopened, id, open), Claim::Open);
        assert_eq!(claim(&mut reopened, id, open), Claim::Opening);
        reopened.insert(id.to_string(), 7);
        assert_eq!(claim(&mut reopened, id, open), Claim::Tab(7));
        // That tab closed: the next one is opened once again.
        reopened.insert(id.to_string(), 8);
        assert_eq!(claim(&mut reopened, id, open), Claim::Open);
        assert_eq!(claim(&mut reopened, id, open), Claim::Opening);
    }
}
