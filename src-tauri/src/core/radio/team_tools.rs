//! What else a session may ask over the radio: who else works in its project and how much of its
//! agent's limits are used, and a saved command stopped. And what an output read again with
//! `since_read` needs: where each caller last read it, kept in memory only and gone at quit.

use serde_json::Value;

use super::super::board::Column;
use super::super::output::MARKER;
use super::super::sessions::Running;
use super::super::snapshot::build_snapshot;
use super::tools::{fitted, no_such_command, text};
use super::*;
use crate::claude::TurnKind;
use crate::usage::{Status, UsageState};

/// The marks kept at once, every output's together: the one read longest ago goes for a new one.
const MARKS_MAX: usize = 256;
/// The sessions a `team_status` answer lists at most, and how much of a card's title and of a
/// tool's name it says.
const MATES_MAX: usize = 20;
const TITLE_CHARS: usize = 120;
const TOOL_CHARS: usize = 80;
/// `server_output`'s most lines at once (`mcp.rs`), as a cut answer says.
const LINES_MAX: usize = 200;

/// How the radio is given an agent's limits as last read, for the account in use (`fuel.rs`,
/// which holds them; the app gives it at its start).
type FuelRead = Arc<dyn Fn(Agent) -> Option<UsageState> + Send + Sync>;

/// What these tools keep, in memory only: where each caller last read an output, and how fuel
/// readings are asked for.
#[derive(Default)]
pub(in crate::core) struct Team {
    /// Each output read over the radio with a caller that can be told, by its key (`output.rs`).
    outputs: HashMap<String, Track>,
    /// Reads counted, for the mark read longest ago.
    reads: u64,
    fuel: Option<FuelRead>,
}

/// One output as its readers know it. A line's place counts from the first line it held when it
/// was first read, so it stays the same while older lines go: those that went are counted.
struct Track {
    /// The project it is an output of: its marks go once it isn't listed.
    path: String,
    /// Lines that went off its front since (trimmed, or the output emptied), and of those the
    /// output's own, not the core's markers.
    gone: u64,
    gone_own: u64,
    /// Where it was last emptied: a run of its own began there (`Place`).
    floor: Place,
    /// Each reader's mark: after the last line it was handed.
    marks: HashMap<String, Mark>,
}

/// A place in an output, and the output's own lines before it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Place {
    at: u64,
    own: u64,
}

#[derive(Debug, Clone, Copy)]
struct Mark {
    place: Place,
    /// The read that set it.
    read: u64,
    /// The output was emptied since it was set: it is a place in the run that went with it, even
    /// where it stands level with where the next one began (a reader that read to the end and
    /// left no marker after it, as Codex's reading leaves none).
    emptied: bool,
}

/// What a reader is handed of an output with `since_read`.
pub(super) struct Unread {
    /// The output's own lines it wasn't handed before, the oldest first.
    pub(super) lines: Vec<String>,
    /// It read this output before, and its mark was kept.
    pub(super) before: bool,
    /// A run began since its last read: `lines` are that run's.
    pub(super) new_run: bool,
    /// The output's own lines of this run that went before it could read them.
    pub(super) missed: u64,
}

impl Team {
    /// `gone` lines went off the front of the output under `key`, `own` of them not the core's
    /// markers (`output.rs`): only an output someone reads keeps count.
    pub(in crate::core) fn went(&mut self, key: &str, gone: u64, own: u64) {
        if let Some(track) = self.outputs.get_mut(key) {
            track.gone += gone;
            track.gone_own += own;
        }
    }

    /// The output under `key` was emptied with all it held (`output.rs`: `forget_output`): a run of
    /// its own begins where it was.
    pub(in crate::core) fn emptied(&mut self, key: &str, gone: u64, own: u64) {
        self.went(key, gone, own);
        if let Some(track) = self.outputs.get_mut(key) {
            track.floor = Place { at: track.gone, own: track.gone_own };
            track.marks.values_mut().for_each(|mark| mark.emptied = true);
        }
    }

    /// The marks of outputs of projects that aren't `listed` go.
    fn keep(&mut self, listed: &HashSet<String>) {
        self.outputs.retain(|_, track| listed.contains(&track.path));
    }

    /// Room for one more mark: past `MARKS_MAX`, the one read longest ago goes, and an output that
    /// has none left.
    fn make_room(&mut self) {
        if self.outputs.values().map(|track| track.marks.len()).sum::<usize>() < MARKS_MAX {
            return;
        }
        let oldest = self.outputs.iter().flat_map(|(key, track)| track.marks.iter().map(move |(reader, mark)| (mark.read, key, reader))).min();
        let Some((_, key, reader)) = oldest.map(|(read, key, reader)| (read, key.clone(), reader.clone())) else {
            return;
        };
        if let Some(track) = self.outputs.get_mut(&key) {
            track.marks.remove(&reader);
            if track.marks.is_empty() {
                self.outputs.remove(&key);
            }
        }
    }

    /// `reader`'s mark on the output under `key` (`path`'s), which holds `lines` now, moved after
    /// its last line; and what was new to it there: the current run's own lines after its last
    /// read, or a new run's from its beginning. At a first read, the current run's lines.
    fn read(&mut self, path: &str, key: &str, lines: &VecDeque<String>, reader: &str) -> Unread {
        if !self.outputs.get(key).is_some_and(|track| track.marks.contains_key(reader)) {
            self.make_room();
        }
        self.reads += 1;
        let read = self.reads;
        let track = self.outputs.entry(key.to_string()).or_insert_with(|| Track { path: path.to_string(), gone: 0, gone_own: 0, floor: Place::default(), marks: HashMap::new() });

        let own = |line: &&String| !line.starts_with(MARKER);
        let start = Place { at: track.gone, own: track.gone_own };
        let place_of = |count: usize| Place { at: start.at + count as u64, own: start.own + lines.iter().take(count).filter(own).count() as u64 };
        // Where the current run began: after its `started` marker (`output.rs`: `mark_run`), else
        // where the output was emptied.
        let started = format!("{MARKER}started\t");
        let run = lines.iter().rposition(|line| line.starts_with(&started)).map_or(track.floor, |at| place_of(at + 1));
        let end = place_of(lines.len());

        // A mark past the end is none this output could have set.
        let mark = track.marks.get(reader).copied().filter(|mark| mark.place.at <= end.at);
        let (from, new_run) = match mark {
            Some(mark) if mark.place.at >= run.at && !mark.emptied => (mark.place, false),
            Some(_) => (run, true),
            None => (run, false),
        };
        // Of the lines this reader would have been handed, those that went before this read.
        let missed = if mark.is_some() && from.at < start.at { start.own.saturating_sub(from.own) } else { 0 };
        let skip = usize::try_from(from.at.saturating_sub(start.at)).unwrap_or(usize::MAX);
        let unread = lines.iter().skip(skip).filter(own).cloned().collect();

        track.marks.insert(reader.to_string(), Mark { place: end, read, emptied: false });
        Unread { lines: unread, before: mark.is_some(), new_run, missed }
    }
}

/// Who reads an output, for `server_output`'s `since_read`: the calling session when it is
/// known, else the agent's own process the radio's server runs under (its parent), which is one
/// session at a time; none under Codex's shared server, which runs several, nor without a parent.
pub(super) fn reader(session: Option<&str>, ppid: u32, shared: bool) -> Option<String> {
    match session {
        Some(session) => Some(format!("session {session}")),
        None if ppid != 0 && !shared => Some(format!("process {ppid}")),
        None => None,
    }
}

/// The answer to `server_output` with `since_read`: what was new to the reader, `count` lines at
/// most, with what it says of the rest. Without a reader nothing could be kept: the last lines,
/// and that they are.
pub(super) fn unread_answer(unread: Option<Unread>, lines: &[String], count: usize) -> (Reply, CallResult) {
    let Some(unread) = unread else {
        let (text, count) = fitted(&last_lines(lines, count));
        let note = "[Pitwall can't tell this session from the others here, so it keeps no place for it: these are the output's last lines, not only the new ones]";
        let text = if count == 0 { format!("{note}\n(no output)") } else { format!("{note}\n{text}") };
        return (Reply::ok(text), CallResult::Lines { count });
    };

    let mut notes: Vec<String> = Vec::new();
    if !unread.before {
        notes.push("[your first read of this output: the current run's lines, from the end]".into());
    }
    if unread.new_run {
        let said = if unread.lines.is_empty() { "it has printed nothing yet" } else { "these lines are that run's" };
        notes.push(format!("[a new run began since your last read, and {said}]"));
    }
    if unread.missed > 0 {
        let lines = if unread.missed == 1 { "line" } else { "lines" };
        notes.push(format!("[{} {lines} came and went before this read: Pitwall keeps an output's last {OUTPUT_LINES} lines]", unread.missed));
    }
    let left = unread.lines.len().saturating_sub(count);
    if left > 0 {
        let lines = if left == 1 { "line" } else { "lines" };
        notes.push(format!("[left out: {left} earlier new {lines}; `lines` asks for up to {LINES_MAX}]"));
    }

    let shown: Vec<&str> = unread.lines[left..].iter().map(String::as_str).collect();
    let (text, count) = fitted(&shown);
    let body = match count {
        0 if unread.new_run => None,
        0 if unread.before => Some("(nothing new since your last read)".to_string()),
        0 => Some("(no output)".to_string()),
        _ => Some(text),
    };
    let reply = notes.into_iter().chain(body).collect::<Vec<_>>().join("\n");
    (Reply::ok(reply), CallResult::Lines { count })
}

/// A session of the project as `team_status` tells it.
struct Mate {
    id: String,
    agent: Agent,
    state: String,
}

/// Where a session stands, in plain words: its tool's name the only thing of it said.
fn state_of(phase: SessionPhase, turn: Option<Turn>, since: Option<u64>, tool: Option<&str>) -> String {
    match phase {
        SessionPhase::Working => since.map_or_else(|| "working".to_string(), |since| format!("working since {}", crate::clock::clock(since as i64))),
        SessionPhase::Waiting => match turn.map(|turn| turn.kind) {
            Some(TurnKind::Permission) => match tool.map(one_line).filter(|tool| !tool.is_empty()) {
                Some(tool) => format!("waiting on a permission for {}", cut(&tool, TOOL_CHARS)),
                None => "waiting on a permission".to_string(),
            },
            Some(TurnKind::Asking) => "waiting for an answer to its question".to_string(),
            Some(TurnKind::Finished) => "its turn is over, waiting for the user".to_string(),
            None => "waiting for the user".to_string(),
        },
        SessionPhase::Idle => "idle".to_string(),
    }
}

/// Where a running session runs, as far as its processes tell it cheaply: one of Pitwall's tabs
/// (`shells`, their shells), VS Code, or a terminal (named from the fixed list, `sessions.rs`).
fn place_of(run: &Running, shells: &[u32]) -> Option<String> {
    if run.lineage.iter().any(|pid| shells.contains(pid)) {
        return Some("in a Pitwall tab".into());
    }
    match run.entrypoint.as_deref() {
        Some("claude-vscode" | "codex-vscode") => Some("in VS Code".into()),
        None | Some("cli") => Some(run.app.map_or_else(|| "in a terminal".to_string(), |app| format!("in a terminal ({app})"))),
        Some(_) => None,
    }
}

/// `text` on one line, without control characters.
fn one_line(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` cut to `max` characters, an ellipsis where it was.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// A span of time in a few words: `3 min`, `2 h 5 min`, `2 days 3 h`.
fn span(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => "under a minute".into(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h {} min", minutes / 60, minutes % 60),
        _ => {
            let days = minutes / 1440;
            format!("{days} {} {} h", if days == 1 { "day" } else { "days" }, (minutes % 1440) / 60)
        }
    }
}

/// A moment ahead or behind, as the clock says it: the time today, else the day and the time.
fn moment(at: u64, now: u64) -> String {
    let day = crate::clock::day_name(at);
    let time = crate::clock::clock(at as i64);
    if day == crate::clock::day_name(now) {
        time
    } else {
        format!("{day} {time}")
    }
}

/// One window of a reading (the 5-hour session's, the week's) in a line, and its share used as
/// the call's result keeps it: none for a window the reading hasn't, or that refilled since.
fn window_line(label: &str, window: Option<&crate::usage::Window>, now: u64) -> (String, Option<u32>) {
    let Some(window) = window else {
        return (format!("- {label}: none in this reading (the plan has no such limit now)"), None);
    };
    let used = window.used.max(0.0).round() as u32;
    match window.resets_at.and_then(|at| u64::try_from(at).ok()) {
        None => (format!("- {label}: {used} % used, not started (its time begins with the next request)"), Some(used)),
        Some(at) if at <= now => (format!("- {label}: refilled at {}, after this reading; nothing is known of it since", moment(at, now)), None),
        Some(at) => (format!("- {label}: {used} % used, refills at {} (in {})", moment(at, now), span(at - now)), Some(used)),
    }
}

/// `fuel_status`'s answer for `agent` from `state`, its account in use's limits as last read.
fn fuel_answer(agent: Agent, state: Option<UsageState>, now: u64) -> (Reply, CallResult) {
    let none = CallResult::Fuel { session: None, week: None };
    let name = agent.name();
    let Some(state) = state else {
        return (Reply::ok(format!("Pitwall has no reading of {name}'s usage limits yet.")), none);
    };
    let Some(usage) = state.usage.as_ref() else {
        let why = match (agent, state.status) {
            (Agent::Claude, Status::SignedOut) => "Claude Code isn't signed in for the account Pitwall uses now, so there is no reading of its limits.",
            (Agent::Claude, Status::Expired) => "Claude Code's sign-in has expired, so Pitwall can't read its limits. Opening Claude Code renews it.",
            (Agent::Claude, Status::RateLimited) => "Anthropic asked Pitwall to wait before reading the limits again, and there is no earlier reading.",
            (Agent::Claude, Status::Error) => "Pitwall couldn't read Claude's limits. It tries again by itself.",
            (Agent::Claude, Status::Loading | Status::Ready) => "Pitwall is reading Claude's limits for the first time. Ask again in a minute.",
            (Agent::Codex, Status::SignedOut) => "Codex isn't signed in for the account Pitwall uses now, so there is no reading of its limits.",
            (Agent::Codex, _) => {
                "Codex has made no request in the last 8 days, so there is no reading of its limits: Pitwall reads them from Codex's own logs, after its next request."
            }
        };
        return (Reply::ok(why), none);
    };

    let window = |id: &str| usage.windows.iter().find(|window| window.id == id);
    if window("five_hour").is_none() && window("seven_day").is_none() {
        return (Reply::ok(format!("{name}'s last reading has neither a 5-hour nor a weekly limit for the account in use.")), none);
    }
    let age = state.fetched_at.and_then(|at| u64::try_from(at).ok()).map(|at| match now.saturating_sub(at) {
        0..=59_999 => "just now".to_string(),
        ago => format!("{} ago", span(ago)),
    });
    let head = match (agent, age) {
        (Agent::Claude, Some(age)) => format!("Claude's usage limits for the account in use, as Pitwall last read them ({age}):"),
        (Agent::Codex, Some(age)) => format!("Codex's usage limits for the account in use, as its last request told them ({age}):"),
        (_, None) => format!("{name}'s usage limits for the account in use, as last read:"),
    };
    let (session_line, session) = window_line("5-hour session", window("five_hour"), now);
    let (week_line, week) = window_line("week", window("seven_day"), now);
    let mut lines = vec![head, session_line, week_line];
    let after = match state.status {
        Status::Expired => Some("Claude Code's sign-in has expired since, so this reading isn't renewed. Opening Claude Code renews it."),
        Status::RateLimited => Some("Anthropic asked Pitwall to wait before reading them again."),
        Status::Error => Some("Pitwall couldn't read them the last time it tried: this is the reading before."),
        Status::Loading => Some("Pitwall is reading them again now."),
        Status::Ready | Status::SignedOut => None,
    };
    lines.extend(after.map(str::to_string));
    (Reply::ok(lines.join("\n")), CallResult::Fuel { session, week })
}

impl Core {
    /// How `fuel_status` is given the limits as last read: the app's, at its start (`fuel.rs`).
    pub fn radio_fuel_from(&self, read: Arc<dyn Fn(Agent) -> Option<UsageState> + Send + Sync>) {
        self.lock().radio.team.fuel = Some(read);
    }

    /// The output under `key` (`path`'s server's, or one of its commands') as it is now and, with
    /// a `reader`, that reader's mark moved after its last line and what was new to it. In one hold
    /// of the core's lock, which a line is kept under: one that comes meanwhile is handed now or
    /// at the next read, never neither. The marks of a project no longer listed go first.
    pub(super) fn radio_read(&self, path: &str, key: &str, reader: Option<&str>) -> (Vec<String>, Option<Unread>) {
        let mut inner = self.lock();
        let inner = &mut *inner;
        let lines = inner.output.get(key).cloned().unwrap_or_default();
        let unread = reader.map(|reader| {
            let listed: HashSet<String> = build_snapshot(inner).projects.into_iter().map(|project| project.path).collect();
            inner.radio.team.keep(&listed);
            inner.radio.team.read(path, key, &lines, reader)
        });
        (lines.into(), unread)
    }

    /// One of the project's saved commands stopped, by its name, as its Stop button stops it: its
    /// whole process tree, and a restart it waits for called off. Answered once it is down. The
    /// name is looked for among this project's commands alone, and the dev server is none of them.
    pub(super) fn radio_stop(self: &Arc<Self>, project: &ProjectView, args: &Value) -> (Reply, CallResult) {
        let Some(command) = text(args, "name").and_then(|name| saved(project, name)) else {
            let (reply, result) = no_such_command(project);
            return (Reply::refused(format!("{} To stop the dev server, use server_restart.", reply.text)), result);
        };
        let (path, name) = (project.path.as_str(), command.name.clone());
        let key = job_key(path, &command.id);
        let runs = |inner: &Inner| inner.jobs.contains_key(&key) || inner.supervisor.pending(&key);

        if !runs(&self.lock()) {
            return (Reply::ok(format!("\"{name}\" isn't running: there is nothing to stop.")), CallResult::Stopped { name, running: false });
        }
        self.stop_command(path, &command.id);
        (Reply::ok(format!("Stopped \"{name}\".")), CallResult::Stopped { name, running: true })
    }

    /// Who works in the project now: every Claude and Codex session its row counts (at work, or
    /// waiting on the user) and those of the last scan that are idle and still run; none of the
    /// laps' (`lap_sessions`), a layer of their own. Each by its agent, where it stands, where it
    /// runs when its processes tell it, and the card it was given; never its name, its folder or
    /// anything it or the user wrote but a card's title.
    pub(super) fn radio_team(&self, project: &ProjectView, session: Option<&str>) -> (Reply, CallResult) {
        let apart = self.lap_sessions();
        let mut mates: Vec<Mate> = project
            .claude_sessions
            .iter()
            .filter(|live| !apart.contains(&live.id))
            .map(|live| Mate { id: live.id.clone(), agent: live.agent, state: state_of(live.phase, live.turn, live.since, live.tool.as_deref()) })
            .collect();
        let idle: Vec<(String, Agent)> = {
            let inner = self.lock();
            inner
                .scanned
                .iter()
                .filter(|state| state.path == project.path && state.phase == SessionPhase::Idle && !state.ended && !apart.contains(&state.id))
                .filter(|state| !mates.iter().any(|mate| mate.id == state.id))
                .map(|state| (state.id.clone(), state.agent))
                .collect()
        };

        // Which run, and where: what was read last, read again only once it is old (an agent
        // that is off is read for nothing).
        let has = |agent: Agent| mates.iter().any(|mate| mate.agent == agent) || idle.iter().any(|(_, of)| *of == agent);
        let claude = if has(Agent::Claude) { self.running_cached() } else { None };
        let codex = if has(Agent::Codex) { self.codex_running() } else { None };
        let running = |id: &str, agent: Agent| match agent {
            Agent::Claude => claude.as_ref().and_then(|running| running.get(id)),
            Agent::Codex => codex.as_ref().and_then(|running| running.get(id)),
        };
        for (id, agent) in idle {
            if running(&id, agent).is_some() {
                mates.push(Mate { id, agent, state: "idle".into() });
            }
        }
        // This session first.
        mates.sort_by_key(|mate| Some(mate.id.as_str()) != session);

        let shells: Vec<u32> = self.terminal_shells().into_iter().map(|(shell, _, _)| shell).collect();
        let cards: HashMap<String, String> = {
            let board = self.board();
            let mut held: Vec<(&String, u64, &String)> = board
                .cards
                .iter()
                .filter(|card| card.path == project.path && card.column == Column::Claude)
                .filter_map(|card| Some((card.session.as_ref()?, card.given_at.unwrap_or(card.moved_at), &card.title)))
                .collect();
            // The one given last, of several.
            held.sort_by_key(|(_, at, _)| *at);
            held.into_iter().map(|(session, _, title)| (session.clone(), cut(&one_line(title), TITLE_CHARS))).collect()
        };

        let count = mates.len();
        let mine = session.is_some_and(|session| mates.iter().any(|mate| mate.id == session));
        let head = match count {
            0 => "Pitwall counts no Claude or Codex session in this project right now.".to_string(),
            1 if mine => "Only this session works in this project right now, as Pitwall counts them:".to_string(),
            1 => "One session in this project right now, as Pitwall counts them:".to_string(),
            _ => format!("{count} sessions in this project right now, as Pitwall counts them{}:", if mine { ", this one among them" } else { "" }),
        };
        let mut lines = vec![head];
        for mate in mates.iter().take(MATES_MAX) {
            let who = if Some(mate.id.as_str()) == session { format!("{} (this session)", mate.agent.name()) } else { mate.agent.name().to_string() };
            let mut line = format!("- {who}: {}", mate.state);
            if let Some(place) = running(&mate.id, mate.agent).and_then(|run| place_of(run, &shells)) {
                line.push_str(&format!(", {place}"));
            }
            if let Some(title) = cards.get(&mate.id) {
                line.push_str(&format!(", on the board card \"{title}\""));
            }
            lines.push(line);
        }
        if count > MATES_MAX {
            lines.push(format!("- and {} more", count - MATES_MAX));
        }
        (Reply::ok(lines.join("\n")), CallResult::Team { count })
    }

    /// The calling agent's limits as Pitwall last read them, for the account in use: nothing is
    /// asked of Anthropic or OpenAI for it, and nothing written down.
    pub(super) fn radio_fuel(&self, agent: Agent) -> (Reply, CallResult) {
        if !self.agent_on(agent) {
            let name = agent.name();
            let text = format!("Pitwall reads none of {name}'s limits while it is turned off in Pitwall's Settings › Agents.");
            return (Reply::ok(text), CallResult::Fuel { session: None, week: None });
        }
        let read = self.lock().radio.team.fuel.clone();
        // Asked with no lock held: the app's side asks the core for the account in use.
        fuel_answer(agent, read.and_then(|read| read(agent)), now_ms())
    }
}
