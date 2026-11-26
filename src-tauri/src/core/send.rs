//! One message to several tabs: what the terminal panel's send box (`TerminalSend.vue`) typed
//! once, written into each of the project's tabs picked. A message is never an answer: each tab is
//! judged right before it is written to (`Core::judge`), from what the core holds of it then, and
//! nothing reaches one whose agent shows a dialog (a permission prompt, a question, Claude Code's
//! own `waiting`), whose agent's state can't be read (Codex's hook unheard, an agent starting),
//! whose agent ended (a shell now, for words meant for the agent), whose shell runs a program, or
//! that is an administrator's or the laps' own. A Return that lands in a dialog approves what it
//! asks, silently and for good.
//!
//! The agent a tab runs is told by the session that runs in it (its process under the tab's
//! shell), not by the tab's kind: Claude's from `~/.claude/sessions` read afresh, Codex's as its
//! hook tied it. An agent takes the text as a paste and its Return a moment later, judged again
//! first; a shell takes it as lines, each run as the empty box runs one; a chat as its next
//! message. The text is held by nothing here: written and dropped, never kept or logged, and what
//! comes back names each tab and why it was left out, never the text.

use std::thread;

use super::sessions::Running;
use super::*;
use crate::chat::wire::ChatOutgoing;
use crate::claude::TurnKind;

/// How long after its paste an agent is given its Return: in the same read as the paste it would
/// be taken as part of the paste (and no message sent).
const ENTER_AFTER: Duration = Duration::from_millis(200);
/// The most of a message that is sent, as of the empty box's first prompt.
const SEND_BYTES: usize = 100_000;

/// Why a tab was given nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SendSkip {
    /// The tab closed.
    Gone,
    /// One of the laps' own (`TerminalView::apart`).
    Laps,
    /// An administrator's shell.
    Admin,
    /// Its agent shows a dialog: a permission prompt, a question, a plan to approve.
    Dialog,
    /// Its agent's state can't be read: Codex's hook didn't tie it, Claude Code tells no status
    /// and no hook does, or the agent is only starting.
    Unknown,
    /// The agent it was opened for ended: its shell is at its prompt, or runs something else.
    Ended,
    /// Its shell runs a program.
    Busy,
    /// It couldn't take it: a chat whose process doesn't run.
    Failed,
}

/// What became of one tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub id: u64,
    /// Why it was given nothing; none: it was written to.
    pub skipped: Option<SendSkip>,
}

/// How a tab takes the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    /// An agent at its prompt, or at work (it queues what comes): a paste, then a Return.
    Paste,
    /// A shell at its prompt: each line, then a Return.
    Lines,
    /// A chat: its next message.
    Chat,
}

/// What a tab is judged by: the running sessions as just read.
pub(super) struct Reads {
    /// Claude's running sessions; none when they can't be told (Claude off).
    claude: Option<HashMap<String, Running>>,
    /// Codex's that its hook tied to a process; none while Codex is off.
    codex: Option<HashMap<String, Running>>,
    /// The process table, read once for the tabs that need it.
    pass: process::PassTable,
}

/// The message as it is written: line ends as `\n`, no control character but a tab, trimmed and
/// cut to `SEND_BYTES` on a character's boundary; none when nothing is left.
fn message(text: &str) -> Option<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text: String = text.chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect();
    let mut text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    if text.len() > SEND_BYTES {
        let mut end = SEND_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Some(text)
}

/// A paste as a terminal app's own is: its lines in brackets (the text holds no escape that could
/// end them early: `message` took every control character out).
fn pasted(text: &str) -> String {
    format!("\x1b[200~{}\x1b[201~", text.replace('\n', "\r"))
}

/// Lines a shell runs, as the empty box gives it one: a tab character would complete there.
fn lines(text: &str) -> String {
    format!("{}\r", text.replace('\t', " ").replace('\n', "\r"))
}

/// A session waits on the user with a dialog open: a question or a permission prompt.
fn asks(state: &SessionState) -> bool {
    state.phase == SessionPhase::Waiting && state.turn.is_some_and(|turn| matches!(turn.kind, TurnKind::Asking | TurnKind::Permission))
}

impl Core {
    /// Writes `text` into each of tabs `ids` that may take it, and says of each what became of it,
    /// in the order asked. Off the main thread: an agent's Return waits a moment.
    pub fn send_to_tabs(self: &Arc<Self>, ids: &[u64], text: &str) -> Result<Vec<SendResult>, String> {
        let text = message(text).ok_or_else(|| t!("terminal.send.nothing"))?;
        Ok(self.send_with(ids, &text, || self.reads_now()))
    }

    /// The running sessions now: Claude's read afresh, Codex's as its hook tied them, read again.
    fn reads_now(&self) -> Reads {
        let claude = self.read_running();
        self.sessions_cache().codex_stale = true;
        Reads { claude, codex: self.codex_running(), pass: None }
    }

    /// `send_to_tabs` with the readings `read` gives: once before the tabs are written to, and once
    /// more before the agents' Return.
    pub(super) fn send_with(self: &Arc<Self>, ids: &[u64], text: &str, read: impl Fn() -> Reads) -> Vec<SendResult> {
        let mut seen = HashSet::new();
        let ids: Vec<u64> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
        let mut results = Vec::with_capacity(ids.len());
        let mut reads = read();
        let mut waiting = Vec::new();

        for id in ids {
            let skipped = match self.judge(id, &mut reads) {
                Ok(Way::Paste) => {
                    self.write_terminal(id, &pasted(text));
                    waiting.push(id);
                    None
                }
                Ok(Way::Lines) => {
                    self.write_terminal(id, &lines(text));
                    None
                }
                Ok(Way::Chat) => {
                    let message = ChatOutgoing { text: text.to_string(), images: Vec::new(), documents: Vec::new(), priority: "next".into(), pasted: false };
                    self.chat_send(id, message).err().map(|_| SendSkip::Failed)
                }
                Err(skip) => Some(skip),
            };
            results.push(SendResult { id, skipped });
        }

        // The agents' Return, once the paste was read apart; judged again, so that a dialog that
        // opened meanwhile never takes it. The paste then waits in its input.
        if !waiting.is_empty() {
            thread::sleep(ENTER_AFTER);
            let mut reads = read();
            for id in waiting {
                match self.judge(id, &mut reads) {
                    Ok(Way::Paste) => self.write_terminal(id, "\r"),
                    other => {
                        if let Some(result) = results.iter_mut().find(|result| result.id == id) {
                            result.skipped = Some(other.err().unwrap_or(SendSkip::Unknown));
                        }
                    }
                }
            }
        }
        results
    }

    /// How tab `id` takes a message now, or why it takes none.
    fn judge(&self, id: u64, reads: &mut Reads) -> Result<Way, SendSkip> {
        let (kind, apart, elevated) = {
            let inner = self.lock();
            let view = inner.terminal_list.iter().find(|view| view.id == id).ok_or(SendSkip::Gone)?;
            (view.kind.clone(), view.apart, view.elevated)
        };
        if apart {
            return Err(SendSkip::Laps);
        }
        if elevated {
            return Err(SendSkip::Admin);
        }
        if kind == "chat" {
            return match self.chat_asks(id) {
                None => Err(SendSkip::Gone),
                Some(true) => Err(SendSkip::Dialog),
                Some(false) => Ok(Way::Chat),
            };
        }
        let shell = {
            let sessions = self.terminal_sessions();
            let session = sessions.get(&id).ok_or(SendSkip::Gone)?;
            session.pid_entry().map(|entry| entry.pid)
        };

        // The sessions that run under the tab's shell, whatever the tab was opened for.
        if let Some(shell) = shell {
            let scanned: Vec<SessionState> = self.lock().scanned.clone();
            let state = |session: &str, agent: Agent| scanned.iter().find(|state| state.id == session && state.agent == agent);
            let mut found = false;
            let mut unknown = false;
            for (session, run) in reads.claude.iter().flatten().filter(|(_, run)| run.lineage.contains(&shell)) {
                found = true;
                let state = state(session, Agent::Claude);
                if run.status.as_deref() == Some("waiting") || state.is_some_and(asks) {
                    return Err(SendSkip::Dialog);
                }
                unknown |= run.status.is_none() && !state.is_some_and(|state| state.hooked);
            }
            for (session, _) in reads.codex.iter().flatten().filter(|(_, run)| run.lineage.contains(&shell)) {
                found = true;
                match state(session, Agent::Codex) {
                    Some(state) if state.hooked && asks(state) => return Err(SendSkip::Dialog),
                    Some(state) if state.hooked => {}
                    _ => unknown = true,
                }
            }
            if found {
                return if unknown { Err(SendSkip::Unknown) } else { Ok(Way::Paste) };
            }
        }

        // No session of its: an agent's tab whose agent is only starting, can't be told, or ended.
        if Agent::of_tab(&kind).is_some() {
            return Err(if self.agent_may_run_with(id, &mut reads.pass) { SendSkip::Unknown } else { SendSkip::Ended });
        }
        if self.terminal_busy(id, &mut reads.pass) {
            return Err(SendSkip::Busy);
        }
        Ok(Way::Lines)
    }
}

// Real tabs (a login shell each, as the panel opens them) and stand-ins for what runs in them.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::claude::Turn;

    fn core_in(tmp: &Path) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude").join("projects"),
                codex_home: tmp.join("codex"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        )
    }

    fn wait_for(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// A session of `agent` running under `shell`, as the readings give one.
    fn running(shell: u32, status: Option<&str>) -> Running {
        Running { pid: 1, entrypoint: Some("cli".into()), cwd: None, started_at: None, status: status.map(str::to_string), status_at: None, lineage: vec![1, shell], app: None }
    }

    /// What the scan holds of a session: waiting with `turn`, or at work.
    fn scanned(id: &str, agent: Agent, turn: Option<TurnKind>, hooked: bool) -> SessionState {
        SessionState {
            id: id.into(),
            agent,
            path: String::new(),
            folder: None,
            title: None,
            phase: if turn.is_some() { SessionPhase::Waiting } else { SessionPhase::Working },
            turn: turn.map(|kind| Turn { kind, at: 1 }),
            since: None,
            hooked,
            ended: false,
            tool: None,
            subagents: 0,
            helping: 0,
            interrupted: false,
        }
    }

    /// A message never reaches a tab whose agent could take it for an answer, or whose shell runs
    /// something: a Return in a dialog approves it for good, and nobody sees it happen. Each case
    /// is a real tab that echoes whatever comes to it, so "nothing written" is what its screen
    /// shows; an idle shell and an agent at its prompt (or at work) do take it, the agent's paste
    /// followed by its Return.
    #[test]
    fn a_message_reaches_only_tabs_that_can_take_it() {
        // (case, the tab's kind, Claude's session in it, Codex's, what the scan says, what it does
        // first, what must come of it)
        type Case = (&'static str, &'static str, Option<Option<&'static str>>, bool, Option<(Agent, Option<TurnKind>, bool)>, &'static str, Option<SendSkip>);
        let cases: [Case; 13] = [
            ("permission", "claude", Some(Some("busy")), false, Some((Agent::Claude, Some(TurnKind::Permission), true)), "", Some(SendSkip::Dialog)),
            ("question", "claude", Some(Some("idle")), false, Some((Agent::Claude, Some(TurnKind::Asking), true)), "", Some(SendSkip::Dialog)),
            ("status waiting", "claude", Some(Some("waiting")), false, None, "", Some(SendSkip::Dialog)),
            ("claude unread", "claude", Some(None), false, None, "", Some(SendSkip::Unknown)),
            ("codex asking", "codex", None, true, Some((Agent::Codex, Some(TurnKind::Permission), true)), "", Some(SendSkip::Dialog)),
            ("codex unhooked", "codex", None, true, None, "", Some(SendSkip::Unknown)),
            ("codex untied", "codex", None, false, None, "codex", Some(SendSkip::Unknown)),
            ("agent ended", "claude", None, false, None, "", Some(SendSkip::Ended)),
            ("shell busy", "shell", None, false, None, "sleep", Some(SendSkip::Busy)),
            ("admin", "shell", None, false, None, "admin", Some(SendSkip::Admin)),
            ("laps", "shell", None, false, None, "apart", Some(SendSkip::Laps)),
            ("claude idle", "claude", Some(Some("idle")), false, Some((Agent::Claude, Some(TurnKind::Finished), true)), "agent", None),
            ("claude at work", "claude", Some(Some("busy")), false, None, "agent", None),
        ];
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy().into_owned();

        let core = core_in(tmp.path());
        core.add_project(&project).unwrap();
        let mut tabs = Vec::new();
        for (case, kind, ..) in &cases {
            let tab = core.open_terminal(&project, false, Some((120, 24))).unwrap();
            tabs.push((case, tab.id, core.terminal_sessions().get(&tab.id).and_then(|session| session.pid_entry()).unwrap().pid));
            core.lock().terminal_list.iter_mut().find(|view| view.id == tab.id).unwrap().kind = kind.to_string();
        }
        let shell = core.open_terminal(&project, false, Some((120, 24))).unwrap().id;
        let opened = Instant::now();
        let all: Vec<u64> = tabs.iter().map(|(_, id, _)| *id).chain([shell]).collect();
        // Every shell at its prompt, and what each case does first.
        assert!(wait_for(|| all.iter().all(|id| !core.terminal_buffer(*id).data.is_empty() && !core.terminal_busy(*id, &mut None))), "the shells never came to rest");
        for ((_, _, _, _, _, first, _), (_, id, _)) in cases.iter().zip(&tabs) {
            match *first {
                "sleep" => core.write_terminal(*id, "sleep 60\r"),
                // An agent's stand-in, which takes its input as Claude Code does: a paste in its
                // brackets, sent by the Return after it. The shell itself can't stand in: bash 3.2
                // (the login shell of GitHub's macOS runners) knows no bracketed paste.
                "agent" => core.write_terminal(*id, "sh -c 'stty -echo; while IFS= read -r line; do case \"$line\" in *\"[200~\"*\"[201~\"*) echo \"cg-$((40+2))\";; esac; done'\r"),
                // A stand-in whose command line names Codex, for a Codex tab its hook never tied.
                "codex" => core.write_terminal(*id, "sh -c 'sleep 60; :' codex-stand-in\r"),
                "admin" => core.mark_elevated(*id),
                "apart" => core.lock().terminal_list.iter_mut().find(|view| view.id == *id).unwrap().apart = true,
                _ => {}
            }
        }
        assert!(wait_for(|| cases.iter().zip(&tabs).filter(|(case, _)| matches!(case.5, "sleep" | "codex" | "agent")).all(|(_, (_, id, _))| core.terminal_busy(*id, &mut None))), "the programs never started");
        // A tab is taken for one whose agent is still starting for a moment after it opens
        // (`STARTING`); only after that is an agent that ended told apart. A shell that comes up
        // fast must not beat it.
        thread::sleep(crate::core::terminal::STARTING.saturating_sub(opened.elapsed()) + Duration::from_millis(50));

        let mut claude = HashMap::new();
        let mut codex_running = HashMap::new();
        let mut states = Vec::new();
        for ((_, _, in_claude, in_codex, state, ..), (_, _, pid)) in cases.iter().zip(&tabs) {
            let session = format!("00000000-0000-4000-8000-{:012}", pid);
            if let Some(status) = in_claude {
                claude.insert(session.clone(), running(*pid, *status));
            }
            if *in_codex {
                codex_running.insert(session.clone(), running(*pid, None));
            }
            if let Some((agent, turn, hooked)) = state {
                states.push(scanned(&session, *agent, *turn, *hooked));
            }
        }
        core.lock().scanned = states;
        let read = || Reads { claude: Some(claude.clone()), codex: Some(codex_running.clone()), pass: None };

        // Each line a command that leaves its mark when it runs.
        let results = core.send_with(&all, "echo cg-$((6*7))", read);

        let ran = |id: u64| core.terminal_buffer(id).data.contains("cg-42");
        assert!(wait_for(|| ran(shell)), "the idle shell never ran it");
        for ((case, .., want), (_, id, _)) in cases.iter().zip(&tabs) {
            let got = results.iter().find(|result| result.id == *id).unwrap().skipped;
            assert_eq!(got, *want, "{case}");
            if want.is_none() {
                assert!(wait_for(|| ran(*id)), "{case}: the paste and its Return never ran");
            }
        }
        assert_eq!(results.iter().find(|result| result.id == shell).unwrap().skipped, None);
        // What was skipped shows nothing of it, a while after the others ran it.
        thread::sleep(Duration::from_millis(500));
        for ((case, .., want), (_, id, _)) in cases.iter().zip(&tabs) {
            if want.is_some() {
                assert!(!core.terminal_buffer(*id).data.contains("cg-"), "{case}: written to");
            }
        }

        core.dispose();
    }
}
