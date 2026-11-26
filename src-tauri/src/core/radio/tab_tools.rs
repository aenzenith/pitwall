//! The terminal panel over the radio: a session lists the tabs of its project's panel, the
//! user's too, opens Claude, Codex and chat tabs in it, closes them, sends their agents a message,
//! starts a session that ended in its tab again, reads what an agent last said, renames and pins
//! a tab, and keeps a chat from falling asleep. The user's word, 2026-11-26: any tab of the
//! project, the user's own included ("ben bir oturumdan şu şu oturumları kapat komutu vermek
//! isterim"); no shell tab is opened over the radio and no shell is written to; a tab opened over
//! the radio may open more, bounded by how many stand at once and how many a session opens a
//! minute; a tab's name and its session's id may be told.
//!
//! What is never done: a dialog answered (a permission, a question: `send.rs` writes nothing to
//! an agent that shows one), an administrator's tab or one of the laps' touched, a tab of another
//! project reached, Pitwall brought to the front or the panel opened (a tab opened here is
//! `unasked`). A pinned tab and the caller's own are never closed. A message or a prompt is passed
//! on and dropped, never kept or logged; the list of calls keeps a tab's name and counts alone.

use serde_json::Value;

use super::super::board::{tab_name, Column};
use super::super::chat::{chat_program, ChatOpen, UNTRUSTED};
use super::super::reveal::Liveness;
use super::super::send::SendSkip;
use super::super::terminal::{first_prompt, tab_sessions, Launch};
use super::team_tools::{cut, one_line, state_of};
use super::tools::{fitted, refused, text, Outcome};
use super::*;
use crate::chat::wire::ChatOutgoing;
use crate::chat::ChatState;
use crate::claude::TurnKind;
use crate::start::{ChatMode, ChatStart, CodexMode, CodexModel, Mode, Model};

/// Tabs opened over the radio that stand in one project at once, and tabs one session opens
/// within `OPENS_SPAN`: a session that opens sessions that open sessions stops there.
const RADIO_TABS_MAX: usize = 8;
const OPENS_MAX: usize = 4;
const OPENS_SPAN: Duration = Duration::from_secs(60);
/// The tabs `tab_list` names at most, and how much of a name or a card's title it says.
const TABS_LISTED_MAX: usize = 40;
const NAME_CHARS: usize = 120;
/// How long a session started again (`tab_wake`) is waited for before its message goes.
const WAKE_WAIT: Duration = Duration::from_secs(30);

const NO_TAB: &str = "There is no such tab in this project's terminal panel. tab_list gives the tabs' ids.";
const ADMIN: &str = "That is an administrator's shell: nothing is done to it over the radio.";
/// The acting tools need a session Pitwall can tell, and not one of the laps'.
const UNTOLD: &str = "Pitwall can't tell which session this call comes from, or it is one of the laps' own: it acts on no tab for it.";

/// A tab of the project's panel, as the radio tells of it.
struct Seen {
    view: TerminalView,
    /// The agent's session that runs in it now: a chat's own, else the one under its shell.
    session: Option<(Agent, String)>,
    /// Where it stands, in plain words.
    state: String,
    /// Its agent is at work, starts, or shows a dialog: closed only with `force`.
    busy: bool,
}

/// What a tab is, in a word or two.
fn kind_of(view: &TerminalView) -> &'static str {
    match view.kind.as_str() {
        "chat" => "Claude chat",
        "claude" => "Claude terminal",
        "codex" => "Codex terminal",
        _ if view.elevated => "administrator's shell",
        _ => "shell",
    }
}

/// A tab's name as an answer says it: on one line, cut.
fn named(view: &TerminalView) -> String {
    cut(&one_line(&view.name), NAME_CHARS)
}

/// Tab ids as the call gives them: a list, or one alone.
fn ids(args: &Value, name: &str) -> Vec<u64> {
    match args.get(name) {
        Some(Value::Array(ids)) => ids.iter().filter_map(Value::as_u64).collect(),
        Some(id) => id.as_u64().into_iter().collect(),
        None => Vec::new(),
    }
}

/// Why a message reached no tab, in plain words.
fn skipped(skip: SendSkip) -> &'static str {
    match skip {
        SendSkip::Gone => "it closed",
        SendSkip::Laps => "it is one of the laps' own",
        SendSkip::Admin => "it is an administrator's",
        SendSkip::Dialog => "its agent shows a dialog the user answers",
        SendSkip::Unknown => "where its agent stands can't be read, or it is only starting",
        SendSkip::Ended => "its agent ended (tab_wake starts its session again)",
        SendSkip::Busy => "its shell runs a program",
        SendSkip::Failed => "its chat couldn't take it",
    }
}

/// A chat's state in plain words.
fn chat_words(state: ChatState, asleep: bool) -> &'static str {
    match state {
        _ if asleep => "idle, asleep: it wakes as soon as it is sent something",
        ChatState::Starting => "starting",
        ChatState::Idle => "idle",
        ChatState::Running => "working",
        ChatState::RequiresAction => "waiting for the user to answer a dialog",
        ChatState::Exited => "its process ended (tab_wake starts it again)",
    }
}

impl Core {
    /// One of the tab tools, its gates of the radio passed: on the caller's project alone.
    pub(super) fn radio_tab(self: &Arc<Self>, agent: Agent, project: &ProjectView, name: &str, session: Option<&str>, args: &Value) -> Outcome {
        let acts = TOOLS.iter().any(|tool| tool.name == name && tool.acts);
        let told = session.filter(|session| !self.radio_lap_layer(agent, Some(session)));
        if acts && told.is_none() {
            return refused(UNTOLD);
        }
        match name {
            "tab_list" => self.radio_tab_list(agent, project, session),
            "tab_open" => self.radio_tab_open(agent, project, told.unwrap_or_default(), args),
            "tab_close" => self.radio_tab_close(agent, project, session, args),
            "tab_send" => self.radio_tab_send(agent, project, session, args),
            "tab_wake" => self.radio_tab_wake(project, args),
            "tab_result" => self.radio_tab_result(project, args),
            "tab_rename" => self.radio_tab_rename(project, args),
            "tab_pin" => self.radio_tab_pin(project, args),
            "tab_keep_awake" => self.radio_tab_awake(agent, project, session, args),
            _ => refused(format!("Unknown tool: {name}")),
        }
    }

    /// The project's tabs, the laps' own left out, in the panel's order.
    fn radio_tabs(&self, project: &ProjectView) -> Vec<TerminalView> {
        self.lock().terminal_list.iter().filter(|view| view.path == project.path && !view.apart).cloned().collect()
    }

    /// One tab of the project, by its id.
    fn radio_tab_of(&self, project: &ProjectView, id: u64) -> Option<TerminalView> {
        self.radio_tabs(project).into_iter().find(|view| view.id == id)
    }

    /// The tab the calling session runs in, one of Pitwall's.
    fn radio_own_tab(&self, agent: Agent, session: Option<&str>) -> Option<u64> {
        let session = session?;
        let running = match agent {
            Agent::Claude => self.running_cached(),
            Agent::Codex => self.codex_running(),
        }?;
        let lineage = running.get(session)?.lineage.clone();
        self.pitwall_terminal(&lineage).map(|(tab, _)| tab)
    }

    /// Each tab of `tabs` as the radio tells of it: what runs in it now and where it stands. An
    /// agent that is off is read for nothing.
    fn radio_seen(&self, tabs: Vec<TerminalView>) -> Vec<Seen> {
        let terminals: Vec<u64> = tabs.iter().filter(|view| view.kind != "chat").map(|view| view.id).collect();
        let shells = self.terminal_shells();
        // The session nearest each terminal's shell, of the tab's own agent first.
        let mut runs: HashMap<u64, (Agent, String)> = HashMap::new();
        for agent in Agent::ALL {
            if terminals.is_empty() || !self.agent_on(agent) {
                continue;
            }
            let running = match agent {
                Agent::Claude => self.running_cached(),
                Agent::Codex => self.codex_running(),
            };
            let Some(running) = running else {
                continue;
            };
            for (tab, (session, _)) in tab_sessions(&running, &shells, &terminals) {
                let own = tabs.iter().any(|view| view.id == tab && Agent::of_tab(&view.kind) == Some(agent));
                if own || !runs.contains_key(&tab) {
                    runs.insert(tab, (agent, session.clone()));
                }
            }
        }
        let scanned = self.lock().scanned.clone();
        let mut pass = None;

        tabs.into_iter()
            .map(|view| {
                let state_now = |agent: Agent, id: &str| scanned.iter().find(|state| state.agent == agent && state.id == id).cloned();
                if view.kind == "chat" {
                    let session = view.chat.as_ref().and_then(|chat| chat.session.clone()).map(|id| (Agent::Claude, id));
                    let (state, asleep, awake) = self.chat_rest_of(view.id).unwrap_or((ChatState::Exited, false, false));
                    let read = session.as_ref().and_then(|(agent, id)| state_now(*agent, id)).filter(|_| !asleep && state != ChatState::Exited);
                    let mut words = match &read {
                        Some(read) if read.phase != SessionPhase::Idle => state_of(read.phase, read.turn, read.since, read.tool.as_deref()),
                        _ => chat_words(state, asleep).to_string(),
                    };
                    if awake {
                        words.push_str(", kept awake");
                    }
                    let busy = matches!(state, ChatState::Starting | ChatState::Running | ChatState::RequiresAction) && !asleep;
                    return Seen { view, session, state: words, busy };
                }
                if view.elevated {
                    return Seen { view, session: None, state: "an administrator's shell".into(), busy: false };
                }
                if let Some((agent, id)) = runs.get(&view.id).cloned() {
                    let (state, busy) = match state_now(agent, &id) {
                        Some(read) => {
                            let asks = read.phase == SessionPhase::Waiting && read.turn.is_some_and(|turn| matches!(turn.kind, TurnKind::Asking | TurnKind::Permission));
                            (state_of(read.phase, read.turn, read.since, read.tool.as_deref()), read.phase == SessionPhase::Working || asks)
                        }
                        None => ("running".to_string(), true),
                    };
                    let state = if Agent::of_tab(&view.kind) == Some(agent) { state } else { format!("{} runs in it: {state}", agent.name()) };
                    return Seen { view, session: Some((agent, id)), state, busy };
                }
                let program = self.terminal_busy(view.id, &mut pass);
                if Agent::of_tab(&view.kind).is_some() {
                    if self.agent_may_run_with(view.id, &mut pass) {
                        return Seen { view, session: None, state: "its agent is starting".into(), busy: true };
                    }
                    let seen = self.tab_seen(view.id);
                    let state = match (program, &seen) {
                        (true, _) => "its agent ended; its shell runs a program",
                        (false, Some(_)) => "its agent ended; the shell is at its prompt (tab_wake starts its session again)",
                        (false, None) => "its agent ended; the shell is at its prompt",
                    };
                    return Seen { view, session: seen, state: state.into(), busy: program };
                }
                let state = if program { "runs a program" } else { "at its prompt" };
                Seen { view, session: None, state: state.into(), busy: program }
            })
            .collect()
    }

    /// The project's tabs, each on a line: its id, kind, name and state, its session's id, who
    /// opened it, whether it is pinned, the caller's own and the board card it holds.
    fn radio_tab_list(&self, agent: Agent, project: &ProjectView, session: Option<&str>) -> Outcome {
        let own = self.radio_own_tab(agent, session);
        let seen = self.radio_seen(self.radio_tabs(project));
        let cards: HashMap<u64, String> = {
            let board = self.board();
            board
                .cards
                .iter()
                .filter(|card| card.path == project.path && matches!(card.column, Column::Claude | Column::Review))
                .filter_map(|card| Some((card.terminal?, cut(&one_line(&card.title), NAME_CHARS))))
                .collect()
        };

        let count = seen.len();
        let mut lines = vec![match count {
            0 => "This project's terminal panel has no tabs open.".to_string(),
            1 => "One tab in this project's terminal panel:".to_string(),
            _ => format!("{count} tabs in this project's terminal panel, in their order:"),
        }];
        for tab in seen.iter().take(TABS_LISTED_MAX) {
            let view = &tab.view;
            let mut line = format!("- tab {}: {} \"{}\", {}", view.id, kind_of(view), named(view), tab.state);
            if let Some((_, id)) = &tab.session {
                line.push_str(&format!(", session {id}"));
            }
            line.push_str(&match (view.by, view.unasked) {
                (Some(by), _) => format!(", opened by {} over the radio", by.name()),
                (None, true) => ", opened by Pitwall at a board card's moment".to_string(),
                (None, false) => ", opened by the user".to_string(),
            });
            if view.pinned {
                line.push_str(", pinned");
            }
            if let Some(title) = cards.get(&view.id) {
                line.push_str(&format!(", on the board card \"{title}\""));
            }
            if Some(view.id) == own {
                line.push_str(" (this session's own tab)");
            }
            lines.push(line);
        }
        if count > TABS_LISTED_MAX {
            lines.push(format!("- and {} more", count - TABS_LISTED_MAX));
        }
        (Reply::ok(lines.join("\n")), CallResult::Tabs { count })
    }

    /// A new Claude or Codex terminal, or a Claude chat, in the project's panel, in the background
    /// (`unasked`: it takes neither the view nor the keyboard), with the caller's account for its
    /// own agent. Never a shell.
    fn radio_tab_open(self: &Arc<Self>, agent: Agent, project: &ProjectView, session: &str, args: &Value) -> Outcome {
        let kind = text(args, "kind").unwrap_or_default();
        let (tab_agent, chat) = match kind {
            "claude" => (Agent::Claude, false),
            "codex" => (Agent::Codex, false),
            "chat" => (Agent::Claude, true),
            _ => return refused("`kind` is one of: claude, codex, chat. No shell tab is opened over the radio."),
        };
        if !self.agent_on(tab_agent) {
            return refused(format!("{} is off in Pitwall: no tab of it is opened.", tab_agent.name()));
        }
        let path = project.path.as_str();

        // The bounds: tabs of the radio's standing in the project, and this session's opens.
        let standing = self.radio_tabs(project).iter().filter(|view| view.by.is_some()).count();
        if standing >= RADIO_TABS_MAX {
            return refused(format!("{RADIO_TABS_MAX} tabs opened over the radio stand in this project already: close one first (tab_close)."));
        }
        let recent = {
            let mut inner = self.lock();
            // Only a session that opened tabs within the span is kept.
            inner.radio.opens.retain(|_, opens| opens.back().is_some_and(|at| at.elapsed() < OPENS_SPAN));
            let opens = inner.radio.opens.entry(session.to_string()).or_default();
            opens.retain(|at| at.elapsed() < OPENS_SPAN);
            opens.len()
        };
        if recent >= OPENS_MAX {
            return refused(format!("This session opened {OPENS_MAX} tabs within the last minute: wait a moment before opening another."));
        }

        // The model and the mode, each one of its agent's list; the project's own where left out.
        let model = text(args, "model");
        let mode = text(args, "mode");
        let prompt = text(args, "prompt").map(str::to_string);
        let name = text(args, "name").map(tab_name).filter(|name| !name.is_empty());
        let renamed = name.is_some();
        // The caller's account for its own agent, as its tab runs with it; the one in use else.
        let account = match tab_agent == agent {
            true => self.radio_own_tab(agent, Some(session)).and_then(|own| self.lock().terminal_list.iter().find(|view| view.id == own).and_then(|view| view.account.clone())),
            false => None,
        };

        let opened = if chat {
            let model = match model.map(|model| Model::parse(model).ok_or(model)) {
                Some(Err(_)) => return refused("A chat's `model` is one of: fable, opus, sonnet, haiku."),
                other => other.and_then(Result::ok),
            };
            let mode = match mode.map(|mode| ChatMode::parse(mode).filter(|mode| mode.at_start()).ok_or(mode)) {
                Some(Err(_)) => return refused("A chat's `mode` is one of: normal, plan, acceptEdits, auto."),
                other => other.and_then(Result::ok),
            };
            let start = (model.is_some() || mode.is_some()).then(|| ChatStart { model, mode: mode.unwrap_or_default(), effort: None });
            let prompt = prompt.map(|text| ChatOutgoing { text, images: Vec::new(), documents: Vec::new(), priority: "next".into(), pasted: false });
            let open = ChatOpen { start, account, prompt, exact: mode.is_some(), ..ChatOpen::default() };
            match chat_program() {
                Some(program) => self.open_chat_as(path, open, program, name, renamed, true),
                None => Err(t!("chatCore.error.noClaude")),
            }
        } else {
            let mut start = self.lock().settings.project(path).start_with(tab_agent);
            match tab_agent {
                Agent::Claude => {
                    if let Some(model) = model {
                        let Some(model) = Model::parse(model) else {
                            return refused("Claude's `model` is one of: fable, opus, sonnet, haiku.");
                        };
                        start.model = Some(model);
                    }
                    if let Some(mode) = mode {
                        let Some(mode) = Mode::parse(mode) else {
                            return refused("Claude's `mode` is one of: normal, plan, acceptEdits.");
                        };
                        start.mode = mode;
                    }
                }
                Agent::Codex => {
                    if let Some(model) = model {
                        let Some(model) = CodexModel::parse(model) else {
                            return refused("Codex's `model` is one of: gpt-6.1-sol, gpt-6.1-terra, gpt-6-astra, gpt-6-luna.");
                        };
                        start.codex.model = Some(model);
                    }
                    if let Some(mode) = mode {
                        let Some(mode) = CodexMode::parse(mode) else {
                            return refused("Codex's `mode` is one of: normal, readOnly.");
                        };
                        start.codex.mode = mode;
                    }
                }
            }
            let prompt = first_prompt(prompt);
            let launch = match tab_agent {
                Agent::Claude => Launch::Claude { prompt, start, images: None, session: None },
                Agent::Codex => Launch::Codex { prompt, start, images: None },
            };
            self.open_terminal_with(path, &launch, name, renamed, None, true, account.as_deref())
        };

        let view = match opened {
            Ok(view) => view,
            // The window asks the user once per project before its first chat; the radio doesn't.
            Err(error) if error == UNTRUSTED => {
                return refused("The user hasn't allowed chats in this project yet: Pitwall asks them as they open its first chat. Open a claude tab instead, or ask them to.")
            }
            Err(error) => return refused(error),
        };
        {
            let mut inner = self.lock();
            if let Some(tab) = inner.terminal_list.iter_mut().find(|tab| tab.id == view.id) {
                tab.by = Some(agent);
            }
            inner.radio.opens.entry(session.to_string()).or_default().push_back(Instant::now());
        }
        self.notify();

        let mut said = format!(
            "Opened tab {}: a {} \"{}\" in this project's terminal panel, in the background. Follow it with tab_list, and read its answer with tab_result once its turn is over.",
            view.id,
            kind_of(&view),
            named(&view)
        );
        // On Windows a terminal's agent starts without its first prompt (`claude_command`).
        if cfg!(windows) && !chat && text(args, "prompt").is_some() {
            said.push_str(" On Windows its prompt isn't given to it as it starts: send it with tab_send once the agent is up.");
        }
        (Reply::ok(said), CallResult::TabOpened { name: view.name })
    }

    /// Tabs of the project closed, what runs in them ended, as their × closes them: never a pinned
    /// one, an administrator's or the caller's own; one at work only with `force`.
    fn radio_tab_close(&self, agent: Agent, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let asked = ids(args, "tabs");
        if asked.is_empty() {
            return refused("`tabs` lists the ids of the tabs to close, as tab_list gives them.");
        }
        let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
        let own = self.radio_own_tab(agent, session);
        let seen = self.radio_seen(self.radio_tabs(project));

        let mut lines = Vec::new();
        let mut closed = 0;
        for id in asked {
            let Some(tab) = seen.iter().find(|tab| tab.view.id == id) else {
                lines.push(format!("- tab {id}: no such tab in this project"));
                continue;
            };
            let name = named(&tab.view);
            let left = if tab.view.pinned {
                Some("it is pinned (tab_pin unpins it)")
            } else if tab.view.elevated {
                Some("it is an administrator's")
            } else if Some(id) == own {
                Some("it is this session's own")
            } else if tab.busy && !force {
                Some("its agent is at work, starting or waiting on a dialog, or its shell runs a program; `force` closes it all the same")
            } else {
                None
            };
            match left {
                Some(why) => lines.push(format!("- tab {id} \"{name}\": left open, {why}")),
                None => {
                    self.close_terminal(id);
                    closed += 1;
                    lines.push(format!("- tab {id} \"{name}\": closed"));
                }
            }
        }
        (Reply::ok(lines.join("\n")), CallResult::TabsClosed { count: closed })
    }

    /// A message to the agents of tabs of the project, as the send window gives one
    /// (`send_to_tabs`, which writes nothing to an agent that shows a dialog): never to a shell or
    /// to the caller's own tab.
    fn radio_tab_send(self: &Arc<Self>, agent: Agent, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let asked = ids(args, "tabs");
        if asked.is_empty() {
            return refused("`tabs` lists the ids of the tabs to send to, as tab_list gives them.");
        }
        let Some(message) = args.get("text").and_then(Value::as_str).filter(|message| !message.trim().is_empty()) else {
            return refused("`text` is the message to send.");
        };
        let own = self.radio_own_tab(agent, session);
        let tabs = self.radio_tabs(project);

        let mut lines = Vec::new();
        let mut sendable = Vec::new();
        for id in asked {
            match tabs.iter().find(|view| view.id == id) {
                None => lines.push((id, format!("- tab {id}: no such tab in this project"))),
                Some(view) if view.kind == "shell" => lines.push((id, format!("- tab {id} \"{}\": nothing sent, a shell takes no message over the radio", named(view)))),
                Some(view) if Some(id) == own => lines.push((id, format!("- tab {id} \"{}\": nothing sent, it is this session's own", named(view)))),
                Some(_) => sendable.push(id),
            }
        }

        let mut sent = 0;
        if !sendable.is_empty() {
            let results = match self.send_to_tabs(&sendable, message) {
                Ok(results) => results,
                Err(error) => return refused(error),
            };
            for result in results {
                let name = tabs.iter().find(|view| view.id == result.id).map(named).unwrap_or_default();
                let line = match result.skipped {
                    None => {
                        sent += 1;
                        format!("- tab {} \"{name}\": sent", result.id)
                    }
                    Some(skip) => format!("- tab {} \"{name}\": nothing sent, {}", result.id, skipped(skip)),
                };
                lines.push((result.id, line));
            }
        }
        let lines: Vec<String> = lines.into_iter().map(|(_, line)| line).collect();
        (Reply::ok(lines.join("\n")), CallResult::TabsSent { count: sent })
    }

    /// The session of a tab whose agent ended there, the tab still open, started again in it: a
    /// terminal's with `claude --resume` (`codex resume`) at its shell's prompt, a chat whose
    /// process ended as its Restart does. Never a session that runs elsewhere, nor into a shell
    /// that runs a program. With `text`, that message once the agent is up.
    fn radio_tab_wake(self: &Arc<Self>, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = args.get("tab").and_then(Value::as_u64) else {
            return refused("`tab` is the tab's id, as tab_list gives it.");
        };
        let Some(view) = self.radio_tab_of(project, id) else {
            return refused(NO_TAB);
        };
        if view.elevated {
            return refused(ADMIN);
        }
        let message = args.get("text").and_then(Value::as_str).filter(|message| !message.trim().is_empty());
        let name = named(&view);

        if view.kind == "chat" {
            let (state, asleep, _) = self.chat_rest_of(id).unwrap_or((ChatState::Exited, false, false));
            if state != ChatState::Exited || asleep {
                let why = if asleep { "it is only asleep, and wakes as soon as it is sent something (tab_send)" } else { "its process runs" };
                return refused(format!("Tab {id} \"{name}\" needs no waking: {why}."));
            }
            if let Err(error) = self.chat_restart(id) {
                return refused(error);
            }
        } else {
            let Some(agent) = Agent::of_tab(&view.kind) else {
                return refused(format!("Tab {id} \"{name}\" is a shell: there is no agent's session in it to start again."));
            };
            if !self.agent_on(agent) {
                return refused(format!("{} is off in Pitwall: none of its sessions is started.", agent.name()));
            }
            if self.agent_may_run_in(id) {
                return refused(format!("Tab {id} \"{name}\" needs no waking: its agent runs in it, or is starting."));
            }
            if self.terminal_busy(id, &mut None) {
                return refused(format!("Tab {id} \"{name}\": its shell runs a program, so nothing is typed into it."));
            }
            let Some((_, session)) = self.tab_seen(id).filter(|(seen, _)| *seen == agent) else {
                return refused(format!("Pitwall saw no session in tab {id} \"{name}\", so there is none to start again in it."));
            };
            let elsewhere = match agent {
                Agent::Claude => self.liveness(&session),
                Agent::Codex => self.codex_liveness(&session),
            };
            if let Liveness::Running { .. } = elsewhere {
                return refused(format!("The session of tab {id} \"{name}\" runs elsewhere: it isn't started a second time."));
            }
            let held = {
                let cache = self.sessions_cache();
                cache.reopened.get(&session).copied().filter(|tab| *tab != id)
            };
            if held.is_some_and(|tab| self.agent_may_run_in(tab)) {
                return refused(format!("The session of tab {id} \"{name}\" was opened again in another tab: it isn't started a second time."));
            }
            if !self.wake_terminal(id, agent, &session) {
                return refused(NO_TAB);
            }
            self.sessions_cache().reopened.insert(session.clone(), id);
            self.board_resumed(&session, id);
        }

        let Some(message) = message else {
            return (Reply::ok(format!("Tab {id} \"{name}\": its session is starting again.")), CallResult::TabWoken { name: view.name });
        };
        // The message goes once the agent takes one; nothing is written while it can't.
        let deadline = Instant::now() + WAKE_WAIT;
        loop {
            thread::sleep(Duration::from_millis(500));
            let result = match self.send_to_tabs(&[id], message) {
                Ok(results) => results.into_iter().next().and_then(|result| result.skipped),
                Err(error) => return refused(error),
            };
            match result {
                None => return (Reply::ok(format!("Tab {id} \"{name}\": its session started again, and was sent the message.")), CallResult::TabWoken { name: view.name }),
                Some(SendSkip::Dialog) => {
                    return (Reply::ok(format!("Tab {id} \"{name}\": its session started again, but shows a dialog the user answers; the message wasn't sent.")), CallResult::TabWoken { name: view.name })
                }
                Some(SendSkip::Gone) => return refused(NO_TAB),
                Some(_) if Instant::now() < deadline => continue,
                Some(skip) => {
                    return (
                        Reply::ok(format!("Tab {id} \"{name}\": its session is starting again; the message wasn't sent, as {}. Send it with tab_send.", skipped(skip))),
                        CallResult::TabWoken { name: view.name },
                    )
                }
            }
        }
    }

    /// What the agent of a tab last said: its last answer, read from its session's log now and
    /// kept nowhere (`Core::last_message`).
    fn radio_tab_result(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = args.get("tab").and_then(Value::as_u64) else {
            return refused("`tab` is the tab's id, as tab_list gives it.");
        };
        let Some(tab) = self.radio_tab_of(project, id).map(|view| self.radio_seen(vec![view])).and_then(|mut seen| seen.pop()) else {
            return refused(NO_TAB);
        };
        let name = named(&tab.view);
        let Some((_, session)) = tab.session else {
            return refused(format!("No agent's session runs in tab {id} \"{name}\" or ran there while Pitwall watched, so there is no answer to read."));
        };
        let head = format!("Tab {id} \"{name}\", {}.", tab.state);
        let reply = match self.last_message(&session) {
            Some(said) => {
                let lines: Vec<&str> = said.lines().collect();
                format!("{head} Its last answer:\n{}", fitted(&lines).0)
            }
            None => format!("{head} Its session has no answer yet."),
        };
        (Reply::ok(reply), CallResult::TabRead { name: tab.view.name })
    }

    /// A tab renamed, as the user renames one: it keeps that name from then on.
    fn radio_tab_rename(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = args.get("tab").and_then(Value::as_u64) else {
            return refused("`tab` is the tab's id, as tab_list gives it.");
        };
        let Some(name) = text(args, "name").map(tab_name).filter(|name| !name.is_empty()) else {
            return refused("`name` is the tab's new name.");
        };
        let Some(view) = self.radio_tab_of(project, id) else {
            return refused(NO_TAB);
        };
        if view.elevated {
            return refused(ADMIN);
        }
        self.rename_terminal(id, &name);
        let name = self.radio_tab_of(project, id).map_or(name, |view| view.name);
        (Reply::ok(format!("Tab {id} is \"{}\" now.", cut(&one_line(&name), NAME_CHARS))), CallResult::TabRenamed { name })
    }

    /// A tab pinned at the start of its row, or unpinned, as its menu does.
    fn radio_tab_pin(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = args.get("tab").and_then(Value::as_u64) else {
            return refused("`tab` is the tab's id, as tab_list gives it.");
        };
        let pinned = args.get("pinned").and_then(Value::as_bool).unwrap_or(true);
        let Some(view) = self.radio_tab_of(project, id) else {
            return refused(NO_TAB);
        };
        if view.elevated {
            return refused(ADMIN);
        }
        self.pin_terminal(id, pinned);
        let said = if pinned { "pinned" } else { "unpinned" };
        (Reply::ok(format!("Tab {id} \"{}\" is {said}.", named(&view))), CallResult::TabPinned { name: view.name, pinned })
    }

    /// A chat kept from falling asleep, or let sleep again: the caller's own tab when none is
    /// named. A terminal never sleeps.
    fn radio_tab_awake(self: &Arc<Self>, agent: Agent, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let awake = args.get("awake").and_then(Value::as_bool).unwrap_or(true);
        let id = match args.get("tab").and_then(Value::as_u64) {
            Some(id) => id,
            None => match self.radio_own_tab(agent, session) {
                Some(id) => id,
                None => return refused("This session runs in none of Pitwall's tabs: name the tab with `tab`, as tab_list gives it."),
            },
        };
        let Some(view) = self.radio_tab_of(project, id) else {
            return refused(NO_TAB);
        };
        let name = named(&view);
        let Some(done) = self.chat_keep_awake(id, awake) else {
            return (Reply::ok(format!("Tab {id} \"{name}\" is a terminal, and a terminal never sleeps: nothing needed doing.")), CallResult::TabAwake { name: view.name, awake });
        };
        let said = match (awake, done) {
            (true, Ok(true)) => format!("Tab {id} \"{name}\" was asleep: it is awake now, and is kept awake until it is let sleep again."),
            (true, Ok(false)) => format!("Tab {id} \"{name}\" is kept awake until it is let sleep again (tab_keep_awake with awake false)."),
            (false, _) => format!("Tab {id} \"{name}\" may fall asleep again once nothing happens in it for a while."),
            (true, Err(error)) => return refused(format!("Tab {id} \"{name}\" is kept awake, but couldn't be woken now: {error}")),
        };
        (Reply::ok(said), CallResult::TabAwake { name: view.name, awake })
    }
}
