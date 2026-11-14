//! Chat tabs (docs/SOHBET.md, chatbuild/CONTRACT.md): the user's `claude` run with no terminal,
//! over stream-json both ways (`crate::chat`), drawn by the window instead of xterm. A chat is a
//! `TerminalView` of kind `chat` among the project's tabs, with a terminal id from the same
//! counter, so panes, layout, rename and close treat it as any tab; its Claude's pid stands where
//! a terminal's shell does (`terminal_shells`, `record_pids`, `terminal_pids`,
//! `agent_may_run_in`), so its session is followed, named, counted and told of as any Pitwall
//! Claude session, its own state given to the watch over what the hook and the log say
//! (`chat_truths`).
//!
//! What the process says is held for its tab in memory only (`Shared::frames`, bounded) and sent
//! to the main window as `chat` events batched per 16 ms: never written down, logged, in a
//! snapshot, a record or a backup. The quit's and the update's records name a chat by its session
//! id, its name and its account (`kind: chat`), no frame of it.

use std::io::{Read, Write};
use std::process::ExitStatus;
use std::sync::mpsc;
use std::sync::Condvar;

use serde_json::{json, Value};

use super::board::tab_name;
use super::reveal::{Liveness, Opened};
use super::terminal::Launch;
use super::*;
use crate::chat::control::{self, ChatAnswer, ChatControl, Known};
use crate::chat::reader::{self, End};
use crate::chat::wire::{self, ChatInfo, ChatOutgoing, ShellCarry};
use crate::chat::{history, meta, process as cli, ChatExitReason, ChatState, ChatView};
use crate::claude::TurnKind;
use crate::start::{chat_args, is_uuid, new_uuid, worktree_name, ChatMode, ChatSession, ChatStart, Mode, Start};

/// A chat tab's kind (`TerminalView::kind`).
pub(super) const CHAT: &str = "chat";
/// What `open_chat` fails with when the project's first chat wasn't allowed yet: the window asks
/// (`chat_trust`) and opens again.
pub(super) const UNTRUSTED: &str = "untrusted";
/// What it fails with when the session runs elsewhere (a terminal, VS Code): one writer per log.
pub(super) const ELSEWHERE: &str = "elsewhere";

/// The most a tab holds of what its process said, for a window that comes back (`chat_buffer`).
const BUFFER_BYTES: usize = 16 * 1024 * 1024;
const BUFFER_FRAMES: usize = 5_000;
/// How long frames gather before they go to the window as one event.
const BATCH: Duration = Duration::from_millis(16);
/// How long a request of the core's waits for its reply; a later one is ignored.
const REPLY_WAIT: Duration = Duration::from_secs(60);
/// Each step of the close ladder: after the end of stdin, after SIGTERM.
const EXIT_WAIT: Duration = Duration::from_secs(5);
/// How long a quit waits for its chats to end by themselves before they are stopped.
const QUIT_WAIT: Duration = Duration::from_secs(2);
/// The end of stderr kept in memory, and the part of it the window is told of an error with.
const STDERR_KEEP: usize = 64 * 1024;
const STDERR_TOLD: usize = 4 * 1024;
/// What a `!` line's output carries to Claude: its last lines, in bytes at most.
const SHELL_LINES: usize = 200;
const SHELL_BYTES: usize = 16 * 1024;
/// The most `!` runs waiting to go with the next message.
const CARRY_MAX: usize = 10;
/// The longest `!` line.
const SHELL_LINE_MAX: usize = 10_000;
/// What the core answers a request of the CLI's it serves nothing for.
const UNSERVED: &str = "Pitwall serves no such request";
/// The most of a message the CLI is given to name its session from: Claude Code reads no more
/// of one.
const TITLE_FROM: usize = 2_000;

/// A chat tab's process and what it said.
pub(super) struct ChatTab {
    id: u64,
    path: String,
    /// The account it runs with, its id and folder; none: the main one.
    account: Option<(String, PathBuf)>,
    start: ChatStart,
    /// The session it was opened on: a fork's stays one until its copy has a log.
    origin: ChatSession,
    /// The worktree Pitwall named for it (O12).
    worktree: Option<String>,
    program: PathBuf,
    shared: Mutex<Shared>,
    /// Told whenever its process ends.
    changed: Condvar,
    /// To the thread that sends its frames to the window.
    out: mpsc::Sender<Out>,
}

/// What goes to the window, in order.
enum Out {
    Frame(u64, Value),
    Exit { code: Option<i32>, reason: ChatExitReason, stderr: Option<String> },
}

/// A frame held for `chat_buffer`.
struct Held {
    n: u64,
    frame: Value,
    weight: usize,
    /// A tool's output: the first to go when the buffer is full.
    tool: bool,
    /// A partial message's, by its thread and message (`stream_of`): they go once its stream
    /// ended, as its `assistant` frames hold it whole.
    stream: Option<String>,
    /// The session's earlier turns: the last to go.
    history: bool,
}

/// A request of the CLI's a card answers, and when it came.
struct Pending {
    id: String,
    request: Value,
    at: u64,
}

/// A message of the user's held until the process answered `initialize`: its line, and what is
/// handed back should it never go (its text, the `!` runs it carried).
struct Waiting {
    uuid: String,
    line: String,
    text: String,
    carry: Vec<ShellCarry>,
}

struct Shared {
    seq: u64,
    frames: VecDeque<Held>,
    weight: usize,
    state: ChatState,
    /// When `state` began.
    since: u64,
    /// When the turn under way began.
    turn_began: u64,
    /// When the last turn ended by itself; none while one runs or after an interrupt.
    finished: Option<u64>,
    /// The user stopped the turn (`chat_interrupt`): its end is no finished turn.
    interrupted: bool,
    info: Option<ChatInfo>,
    pending: Vec<Pending>,
    /// The core's own requests waiting for their replies.
    replies: HashMap<String, mpsc::Sender<Result<Value, String>>>,
    /// The lines to the process's stdin; none: its stdin is closed.
    writer: Option<mpsc::Sender<String>>,
    pid: Option<u32>,
    /// Which process of the tab it is (`chat_restart` starts another).
    generation: u64,
    /// A process runs and wasn't reaped yet.
    running: bool,
    /// It answered `initialize`: messages go to it, not into `held`.
    ready: bool,
    held: Vec<Waiting>,
    /// It is being ended: its end is no failure.
    closing: bool,
    /// What it wrote couldn't be read.
    wire_lost: bool,
    /// The tab was closed: it goes once its process has.
    gone: bool,
    session: String,
    /// A message went to the process (not only held): its session has a log, or will have once
    /// the process read it, and a record names it.
    sent: bool,
    /// The conversation was reset (`/clear`): the next session id the process names is the
    /// tab's.
    reset: bool,
    /// The message each thread streams now (`stream_of`).
    streams: HashMap<String, String>,
    title: Option<String>,
    /// The text of the last message sent, until its turn began: what an untitled session is
    /// named from (`chat_name`).
    title_from: Option<String>,
    /// The CLI was asked for a title and hasn't answered.
    naming: bool,
    /// The messages a dry run of `rewind_files` was asked for.
    dry_runs: HashSet<String>,
    /// The `!` runs going, by run id: their process groups.
    shells: HashMap<String, u32>,
    carry: Vec<ShellCarry>,
}

impl Shared {
    fn new(session: String, sent: bool) -> Self {
        Self {
            seq: 0,
            frames: VecDeque::new(),
            weight: 0,
            state: ChatState::Starting,
            since: now_ms(),
            turn_began: 0,
            finished: None,
            interrupted: false,
            info: None,
            pending: Vec::new(),
            replies: HashMap::new(),
            writer: None,
            pid: None,
            generation: 0,
            running: false,
            ready: false,
            held: Vec::new(),
            closing: false,
            wire_lost: false,
            gone: false,
            session,
            sent,
            reset: false,
            streams: HashMap::new(),
            title: None,
            title_from: None,
            naming: false,
            dry_runs: HashSet::new(),
            shells: HashMap::new(),
            carry: Vec::new(),
        }
    }

    /// Writes the user's `message` to the process, or holds it until the process answered
    /// `initialize`.
    fn send(&mut self, message: Waiting) -> Result<(), String> {
        if !self.running {
            return Err(not_running());
        }
        if !self.ready {
            self.held.push(message);
            return Ok(());
        }
        self.writer.as_ref().ok_or_else(not_running)?.send(message.line).map_err(|_| not_running())?;
        self.sent = true;
        Ok(())
    }

    /// Stops the tab's `!` runs: their groups get SIGKILL (their threads tell their ends).
    fn stop_shells(&self) {
        for pid in self.shells.values() {
            process::kill_tree(*pid, true);
        }
    }
}

impl ChatTab {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Hands `frame` to the window and holds it for `chat_buffer`, the buffer kept in bounds.
    fn push(&self, shared: &mut Shared, frame: Value) {
        self.push_as(shared, frame.clone(), frame);
    }

    /// `push`, the window given `live` now and the buffer holding `frame`, a lighter copy of it.
    /// Once a message's stream ended, its partial frames go: its `assistant` frames hold it whole,
    /// and a window that comes in while it streams still gets every word so far.
    fn push_as(&self, shared: &mut Shared, live: Value, frame: Value) {
        shared.seq += 1;
        let kind = frame.get("type").and_then(Value::as_str).unwrap_or_default();
        let tool = kind == "user" && tool_output(&frame);
        let (stream, ended) = if kind == "stream_event" { stream_of(shared, &frame) } else { (None, false) };
        let history = kind == "pitwall" && frame.get("kind").and_then(Value::as_str) == Some("history");
        let weight = weight(&frame);
        let _ = self.out.send(Out::Frame(shared.seq, live));
        shared.weight += weight;
        shared.frames.push_back(Held { n: shared.seq, frame, weight, tool, stream: stream.clone(), history });
        if let Some(key) = stream.filter(|_| ended) {
            let mut freed = 0;
            shared.frames.retain(|held| {
                let ours = held.stream.as_ref() == Some(&key);
                if ours {
                    freed += held.weight;
                }
                !ours
            });
            shared.weight -= freed;
        }
        while shared.frames.len() > BUFFER_FRAMES || (shared.weight > BUFFER_BYTES && shared.frames.len() > 1) {
            let at = shared.frames.iter().position(|held| held.tool).or_else(|| shared.frames.iter().position(|held| !held.history)).unwrap_or(0);
            if let Some(held) = shared.frames.remove(at) {
                shared.weight -= held.weight;
            }
        }
    }

    /// The cards still open close: at the end of a turn those of the turn, a background
    /// subagent's (`agent_id`) staying owed; at the process's end (`all`) every one. Whether any
    /// closed.
    fn close_pending(&self, shared: &mut Shared, all: bool) -> bool {
        let (closed, owed): (Vec<Pending>, Vec<Pending>) = shared.pending.drain(..).partition(|pending| all || !of_subagent(&pending.request));
        shared.pending = owed;
        for pending in &closed {
            self.push(shared, own("answered", json!({ "requestId": pending.id })));
        }
        !closed.is_empty()
    }
}

/// A request a subagent asks (its `agent_id`): a background one's outlives its turn.
fn of_subagent(request: &Value) -> bool {
    request.get("agent_id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
}

/// The message a stream frame is part of, by its thread (`parent_tool_use_id`) and the id its
/// `message_start` gave, and whether it ends that message's stream (`message_stop`).
fn stream_of(shared: &mut Shared, frame: &Value) -> (Option<String>, bool) {
    let thread = frame.get("parent_tool_use_id").and_then(Value::as_str).unwrap_or_default().to_string();
    let event = frame.pointer("/event/type").and_then(Value::as_str);
    if event == Some("message_start") {
        let id = frame.pointer("/event/message/id").and_then(Value::as_str).map_or_else(|| format!("#{}", shared.seq), str::to_string);
        shared.streams.insert(thread.clone(), id);
    }
    let key = shared.streams.get(&thread).map(|id| format!("{thread}/{id}"));
    if event == Some("message_stop") {
        shared.streams.remove(&thread);
        return (key, true);
    }
    (key, false)
}

/// A frame the core makes itself.
fn own(kind: &str, fields: Value) -> Value {
    let mut frame = json!({ "type": "pitwall", "kind": kind });
    if let (Some(frame), Value::Object(fields)) = (frame.as_object_mut(), fields) {
        frame.extend(fields);
    }
    frame
}

/// The `sent` frame that echoes message `uuid`: what goes to the window now, all of it but its
/// documents' data (a PDF may be larger than the whole buffer; their names and kinds stay), and
/// what the buffer holds of it, its images' data out too (`"data": null, "omitted": true`, as a
/// log's are), so that one message's pictures can't push the conversation out of the buffer.
fn echoed(uuid: &str, message: &ChatOutgoing) -> (Value, Value) {
    let mut echo = message.clone();
    for document in &mut echo.documents {
        document.data.clear();
    }
    let at = now_iso();
    let images = std::mem::take(&mut echo.images);
    let mut held = own("sent", json!({ "uuid": uuid, "message": &echo, "timestamp": at }));
    held["message"]["images"] = images.iter().map(|image| json!({ "mediaType": image.media_type, "data": null, "omitted": true })).collect();
    echo.images = images;
    (own("sent", json!({ "uuid": uuid, "message": echo, "timestamp": at })), held)
}

/// A session's title as a tab may be named from it: no control characters, trimmed, cut to a tab
/// name's length.
fn clean_title(text: &str) -> String {
    let title: String = text.chars().filter(|c| !c.is_control()).take(super::terminal::TAB_NAME_MAX).collect();
    title.trim().to_string()
}

/// What a session is named from of a message's `text`: none when it says nothing, else its first
/// `TITLE_FROM` characters.
fn title_source(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(TITLE_FROM).collect())
}

/// Now, as the CLI writes a frame's `timestamp`.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A user or assistant frame the CLI wrote without its time gets the moment the core read it:
/// the window measures a turn from these.
fn stamp(frame: &mut Value) {
    if let Some(fields) = frame.as_object_mut() {
        if !fields.get("timestamp").is_some_and(Value::is_string) {
            fields.insert("timestamp".into(), Value::String(now_iso()));
        }
    }
}

/// About how many bytes a frame holds: its text and a little for each value.
fn weight(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len() + 2,
        Value::Array(items) => items.iter().map(weight).sum::<usize>() + 2,
        Value::Object(fields) => fields.iter().map(|(key, value)| key.len() + 3 + weight(value)).sum::<usize>() + 2,
        _ => 8,
    }
}

/// A user frame that carries a tool's result.
fn tool_output(frame: &Value) -> bool {
    frame.pointer("/message/content").and_then(Value::as_array).is_some_and(|blocks| blocks.iter().any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result")))
}

fn not_running() -> String {
    t!("chatCore.error.notRunning")
}

/// How many ready prompts the empty chat keeps (`EXAMPLES_MOST` in `src/lib/chat/examples.ts`).
const CHAT_EXAMPLES_MOST: usize = 30;
/// How long a ready prompt may be, in characters.
const CHAT_EXAMPLE_CHARS: usize = 1_000;

fn refused() -> String {
    t!("chatCore.error.refused")
}

fn gone() -> String {
    t!("chatCore.error.gone")
}

/// What a card waits with, for the watch: a question or a plan (`AskUserQuestion`,
/// `ExitPlanMode`, a form, a dialog) asks; any other tool waits for its permission, named.
fn card_kind(request: &Value) -> (TurnKind, Option<String>) {
    match request.get("subtype").and_then(Value::as_str) {
        Some("can_use_tool") => match request.get("tool_name").and_then(Value::as_str) {
            Some("AskUserQuestion" | "ExitPlanMode") => (TurnKind::Asking, None),
            tool => (TurnKind::Permission, tool.map(|tool| tool.chars().filter(|c| !c.is_control()).take(100).collect())),
        },
        _ => (TurnKind::Asking, None),
    }
}

/// The state the CLI says, its turn's start and end kept: the moment it ran from idle, the moment
/// it went idle from a turn of its own. Whether it changed.
fn set_state(shared: &mut Shared, state: ChatState) -> bool {
    let was = shared.state;
    if was == state {
        return false;
    }
    let now = now_ms();
    match state {
        ChatState::Running if was != ChatState::RequiresAction => {
            shared.turn_began = now;
            shared.interrupted = false;
            shared.finished = None;
            // A turn may change the files a dry run of a rewind listed: it is asked again.
            shared.dry_runs.clear();
        }
        ChatState::Idle if matches!(was, ChatState::Running | ChatState::RequiresAction) && !shared.interrupted => shared.finished = Some(now),
        _ => {}
    }
    shared.state = state;
    shared.since = now;
    true
}

/// A reply of the process to one of the core's requests, to whoever waits for it; a reply nobody
/// waits for any more (too late, or never asked) is dropped.
fn take_reply(shared: &mut Shared, frame: &Value) {
    let Some(response) = frame.get("response") else {
        return;
    };
    let Some(waiting) = response.get("request_id").and_then(Value::as_str).and_then(|id| shared.replies.remove(id)) else {
        return;
    };
    let result = match response.get("subtype").and_then(Value::as_str) {
        Some("success") => Ok(response.get("response").cloned().unwrap_or(Value::Null)),
        _ => Err(response.get("error").and_then(Value::as_str).map(wire::short_error).unwrap_or_else(|| t!("chatCore.error.refusedByCli"))),
    };
    let _ = waiting.send(result);
}

/// The last `max` bytes of `text`, on a character's edge.
fn last_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The last `lines` lines of `text`, `bytes` at most.
fn last_lines(text: &str, lines: usize, bytes: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let kept = all[all.len().saturating_sub(lines)..].join("\n");
    last_bytes(&kept, bytes).to_string()
}

/// The `claude` a chat runs: the one on the terminal's `PATH`; in a test, the stand-in it set.
fn chat_program() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(program) = tests::PROGRAM.with(|program| program.borrow().clone()) {
        return Some(program);
    }
    cli::program()
}

/// The mode Settings › Claude › Sohbet starts a chat in: `auto`, else Claude Code's own default.
/// Nothing but these two is ever read from the settings for it.
fn chat_start_mode(word: &str) -> ChatMode {
    if word == "auto" {
        ChatMode::Auto
    } else {
        ChatMode::Default
    }
}

/// `start`, its mode the settings' where it names none (`default`: a project's or a card's
/// Normal, a start with no choice); a mode it names (plan, edits, auto) stays.
fn with_mode(start: ChatStart, mode: ChatMode) -> ChatStart {
    if start.mode == ChatMode::Default {
        ChatStart { mode, ..start }
    } else {
        start
    }
}

/// What the window asks a chat to open with (`open_chat`).
#[derive(Debug, Clone, Default)]
pub struct ChatOpen {
    /// The model, the mode and the effort; none: the project's own model and mode.
    pub start: Option<ChatStart>,
    /// The Claude account by id (`accounts::MAIN` for the main one); none: the one in use.
    pub account: Option<String>,
    /// A session to go on with.
    pub session: Option<String>,
    /// A session to fork, and the entry it is cut after.
    pub fork: Option<(String, Option<String>)>,
    /// The first message, sent once the process is ready.
    pub prompt: Option<ChatOutgoing>,
    /// In a git worktree of Pitwall's naming.
    pub worktree: bool,
}

/// A tab about to be made: everything checked.
struct Made<'a> {
    path: &'a str,
    program: PathBuf,
    account: Option<(String, PathBuf)>,
    start: ChatStart,
    worktree: Option<String>,
    prompt: Option<ChatOutgoing>,
    name: Option<String>,
    renamed: bool,
    unasked: bool,
}

/// What `chat_buffer` gives a window that comes back (`wire.ts` `ChatBuffer`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBuffer {
    pub seq: u64,
    pub frames: Vec<Value>,
    pub state: ChatState,
    pub info: Option<ChatInfo>,
    pub pending: Vec<ChatPending>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPending {
    pub request_id: String,
    pub request: Value,
}

/// What an interrupt left (`wire.ts` `ChatInterruptReceipt`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatInterruptReceipt {
    pub still_queued: Vec<String>,
    pub cancelled: Vec<String>,
}

/// Where a chat's session stands by the chat itself, for the watch (`apply_chats`).
pub(super) struct ChatTruth {
    pub(super) session: String,
    pub(super) path: String,
    pub(super) phase: TruthPhase,
    pub(super) title: Option<String>,
    pub(super) interrupted: bool,
}

pub(super) enum TruthPhase {
    Idle,
    Working { since: u64 },
    /// A card is open: a permission (its tool), a question, a plan.
    Waiting { turn: Turn, tool: Option<String> },
    /// The last turn ended then by itself.
    Finished { at: u64 },
}

impl Core {
    pub(super) fn chats(&self) -> MutexGuard<'_, HashMap<u64, Arc<ChatTab>>> {
        self.chat_tabs.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Chat tab `id`, while it is open.
    fn chat(&self, id: u64) -> Option<Arc<ChatTab>> {
        self.chats().get(&id).filter(|tab| !tab.lock().gone).cloned()
    }

    /// Whether tab `id` is a chat, and if so whether its process runs (a closed one's too, until
    /// it has ended): its Claude may still write its log (`agent_may_run_in`).
    pub(super) fn chat_alive(&self, id: u64) -> Option<bool> {
        self.chats().get(&id).map(|tab| tab.lock().running)
    }

    /// A tab is open: a terminal's shell or a chat (the board's cards).
    pub(super) fn tab_open(&self, id: u64) -> bool {
        self.terminal_sessions().contains_key(&id) || self.chat(id).is_some()
    }

    /// Every open chat's running Claude, as `terminal_shells` gives a shell: its pid, the tab and
    /// its project.
    pub(super) fn chat_shells(&self) -> Vec<(u32, u64, String)> {
        let tabs: Vec<Arc<ChatTab>> = self.chats().values().cloned().collect();
        tabs.iter()
            .filter_map(|tab| {
                let shared = tab.lock();
                let pid = shared.pid.filter(|_| shared.running && !shared.gone)?;
                Some((pid, tab.id, tab.path.clone()))
            })
            .collect()
    }

    /// Every process group a chat started that may still run: its Claude (a closed tab's too,
    /// until it has ended) and its `!` runs; for the pids file and the quit.
    pub(super) fn chat_groups(&self) -> Vec<PidEntry> {
        let tabs: Vec<Arc<ChatTab>> = self.chats().values().cloned().collect();
        let mut entries = Vec::new();
        for tab in tabs {
            let shared = tab.lock();
            if let Some(pid) = shared.pid.filter(|_| shared.running) {
                entries.push(PidEntry { path: tab.path.clone(), pid });
            }
            entries.extend(shared.shells.values().map(|&pid| PidEntry { path: tab.path.clone(), pid }));
        }
        entries
    }

    /// The view of chat tab `id`, while it is open.
    fn chat_view(&self, id: u64) -> Option<TerminalView> {
        self.lock().terminal_list.iter().find(|view| view.id == id && view.kind == CHAT).cloned()
    }

    /// The open chat that holds `session`, running or not: a session is never in two chats.
    fn chat_holding(&self, session: &str) -> Option<TerminalView> {
        let id = self.chats().values().find(|tab| {
            let shared = tab.lock();
            !shared.gone && shared.session == session
        })?.id;
        self.chat_view(id)
    }

    /// The window's `open_chat`: a chat in the project at `path`, a new session or one going on
    /// (`session`) or forked (`fork`), as `open` says.
    pub fn open_chat(self: &Arc<Self>, path: &str, open: ChatOpen) -> Result<TerminalView, String> {
        let program = chat_program().ok_or_else(|| t!("chatCore.error.noClaude"))?;
        self.open_chat_as(path, open, program, None, false, false)
    }

    /// `open_chat` with the tab's name (`renamed`: the user gave it) and whether the core opens it
    /// for a board card's moment (`unasked`). A session another process runs is never opened: a
    /// chat that holds it comes up instead, anything else fails with `ELSEWHERE`; one Claude
    /// Code can't tell of fails too. Opened again once only, whoever asks (`open_once`).
    pub(super) fn open_chat_as(self: &Arc<Self>, path: &str, open: ChatOpen, program: PathBuf, name: Option<String>, renamed: bool, unasked: bool) -> Result<TerminalView, String> {
        let ChatOpen { start, account, session, fork, prompt, worktree } = open;
        let made = self.chat_made(path, start, account.as_deref(), program, prompt, worktree, name, renamed, unasked)?;

        match (session, fork) {
            (_, Some((from, at))) => {
                if !is_uuid(&from) || at.as_deref().is_some_and(|at| !is_uuid(at)) {
                    return Err(refused());
                }
                let (log, _) = self.session_log(&from).ok_or_else(|| t!("chatCore.error.noLog"))?;
                let frames = history::read_to(&log, at.as_deref());
                self.make_chat(made, ChatSession::Fork { from, at, id: new_uuid() }, frames)
            }
            (Some(session), None) => {
                if !is_uuid(&session) {
                    return Err(refused());
                }
                if let Some(view) = self.chat_holding(&session) {
                    return Ok(view);
                }
                match self.liveness(&session) {
                    Liveness::Running { lineage, .. } => self.pitwall_terminal(&lineage).and_then(|(tab, _)| self.chat_view(tab)).ok_or_else(|| ELSEWHERE.to_string()),
                    Liveness::Unknown if self.session_log(&session).is_some() => Err(t!("chatCore.error.unsure")),
                    Liveness::Unknown => Err(t!("chatCore.error.noLog")),
                    Liveness::Ended | Liveness::Stopped => {
                        let (log, _) = self.session_log(&session).ok_or_else(|| t!("chatCore.error.noLog"))?;
                        let frames = history::read(&log);
                        let resumed = session.clone();
                        match self.open_once(&session, || self.make_chat(made, ChatSession::Resume(resumed), frames)) {
                            Opened::New(view) => Ok(view),
                            Opened::Held(tab) => self.chat_view(tab).ok_or_else(|| ELSEWHERE.to_string()),
                            Opened::Opening => Err(ELSEWHERE.to_string()),
                            Opened::Failed(error) => Err(error),
                        }
                    }
                }
            }
            (None, None) => self.make_chat(made, ChatSession::New(new_uuid()), Vec::new()),
        }
    }

    /// Whether `path` is a listed project's: a chat runs in no other folder.
    fn is_listed(&self, path: &str) -> bool {
        self.listed_paths().iter().any(|listed| same_path(listed, path))
    }

    /// Everything a chat in `path` needs checked before it opens: the folder, a listed project's,
    /// Claude on, the project's chats allowed (`UNTRUSTED`), the account, the first message.
    #[allow(clippy::too_many_arguments)]
    fn chat_made<'a>(
        &self,
        path: &'a str,
        start: Option<ChatStart>,
        account: Option<&str>,
        program: PathBuf,
        prompt: Option<ChatOutgoing>,
        worktree: bool,
        name: Option<String>,
        renamed: bool,
        unasked: bool,
    ) -> Result<Made<'a>, String> {
        if !Path::new(path).is_dir() {
            return Err(t!("core.error.notFolder", path = path));
        }
        if !self.is_listed(path) {
            return Err(t!("core.error.depsNotListed", path = path));
        }
        if !self.agent_on(Agent::Claude) {
            return Err(t!("terminal.claudeOff"));
        }
        let (trusted, account, own, mode) = {
            let inner = self.lock();
            let project = inner.settings.project(path);
            let account = crate::accounts::start_with(&inner.settings.accounts, account)?.map(|account| (account.id.clone(), PathBuf::from(&account.dir)));
            (project.chat_trusted, account, project.start_with(Agent::Claude), chat_start_mode(&inner.settings.chat.mode))
        };
        if !trusted {
            return Err(UNTRUSTED.into());
        }
        if let Some(prompt) = &prompt {
            prompt.checked(false)?;
        }
        Ok(Made {
            path,
            program,
            account,
            start: with_mode(start.unwrap_or_else(|| ChatStart::from_start(own)), mode),
            worktree: worktree.then(worktree_name),
            prompt,
            name,
            renamed,
            unasked,
        })
    }

    /// Makes the tab and starts its process on `session`, the earlier turns (`history`) its first
    /// frame and the first message held until the process is ready. Nothing stays of a tab whose
    /// process didn't start.
    fn make_chat(self: &Arc<Self>, made: Made, session: ChatSession, history: Vec<Value>) -> Result<TerminalView, String> {
        let session_id = session.id().to_string();
        let view = {
            let mut inner = self.lock();
            inner.next_run += 1;
            let id = inner.next_run;
            let same = inner.terminal_list.iter().filter(|view| view.path == made.path && view.kind == CHAT && !view.apart).count();
            let base = t!("chatCore.tabName");
            let name = made.name.clone().unwrap_or_else(|| if same == 0 { base.clone() } else { format!("{base} {}", same + 1) });
            let view = TerminalView {
                id,
                path: made.path.to_string(),
                name,
                kind: CHAT.into(),
                chat: Some(ChatView { session: Some(session_id.clone()), state: ChatState::Starting, title: None }),
                shell: None,
                elevated: false,
                renamed: made.renamed,
                unasked: made.unasked,
                account: made.account.as_ref().map(|(id, _)| id.clone()),
                apart: false,
            };
            inner.terminal_list.push(view.clone());
            view
        };

        let (out, events) = mpsc::channel();
        let sent = matches!(session, ChatSession::Resume(_));
        let tab = Arc::new(ChatTab {
            id: view.id,
            path: made.path.to_string(),
            account: made.account,
            start: made.start,
            origin: session.clone(),
            worktree: made.worktree,
            program: made.program,
            shared: Mutex::new(Shared::new(session_id.clone(), sent)),
            changed: Condvar::new(),
            out,
        });
        self.send_chat_events(view.id, events);
        {
            let mut shared = tab.lock();
            if !history.is_empty() {
                tab.push(&mut shared, own("history", json!({ "frames": history })));
            }
            if let Some(prompt) = &made.prompt {
                let uuid = new_uuid();
                let line = wire::user_message(prompt, &uuid, &[]).to_string();
                let (live, held) = echoed(&uuid, prompt);
                shared.held.push(Waiting { uuid, line, text: prompt.text.clone(), carry: Vec::new() });
                shared.title_from = title_source(&prompt.text);
                tab.push_as(&mut shared, live, held);
            }
        }
        self.chats().insert(view.id, Arc::clone(&tab));

        if let Err(error) = self.start_chat(&tab, session) {
            self.chats().remove(&view.id);
            self.lock().terminal_list.retain(|known| known.id != view.id);
            self.notify();
            return Err(error);
        }
        // Its session is this tab's: nothing opens it again while its Claude may run here.
        self.sessions_cache().reopened.insert(session_id, view.id);
        self.notify();
        Ok(view)
    }

    /// Starts the tab's process on `session`: its writer, reader, stderr drain and waiter, and the
    /// `initialize` request.
    fn start_chat(self: &Arc<Self>, tab: &Arc<ChatTab>, session: ChatSession) -> Result<(), String> {
        let (suggestions, hook_events, continued) = {
            let inner = self.lock();
            let chat = &inner.settings.chat;
            (chat.suggestions, chat.hook_rows, chat.continue_interrupted)
        };
        let args = chat_args(tab.start, &session, suggestions, hook_events, tab.worktree.as_deref()).ok_or_else(refused)?;
        let spawn = cli::Spawn { program: tab.program.clone(), args, cwd: PathBuf::from(&tab.path), account: tab.account.as_ref().map(|(_, dir)| dir.clone()), continue_interrupted: continued };
        let mut child = cli::spawn(&spawn).map_err(|error| t!("chatCore.error.start", error = error))?;
        let pid = child.id();
        let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            cli::stop(pid, true);
            let _ = child.wait();
            return Err(t!("chatCore.error.start", error = "no pipes"));
        };

        let (writer, lines) = mpsc::channel::<String>();
        let generation = {
            let mut shared = tab.lock();
            shared.generation += 1;
            shared.pid = Some(pid);
            shared.writer = Some(writer);
            shared.running = true;
            shared.ready = false;
            shared.closing = false;
            shared.wire_lost = false;
            shared.reset = false;
            shared.streams.clear();
            // A new process: a dry run of the last one is no check for this one, and a title the
            // last was asked for is not waited for.
            shared.dry_runs.clear();
            shared.naming = false;
            shared.state = ChatState::Starting;
            shared.since = now_ms();
            tab.push(&mut shared, own("state", json!({ "state": ChatState::Starting })));
            shared.generation
        };

        // One writer: each line and its break, in the order sent; stdin ends with the channel.
        thread::spawn(move || {
            for line in lines {
                if stdin.write_all(line.as_bytes()).and_then(|()| stdin.write_all(b"\n")).and_then(|()| stdin.flush()).is_err() {
                    break;
                }
            }
        });

        let tail: Arc<Mutex<VecDeque<u8>>> = Arc::new(Mutex::new(VecDeque::new()));
        {
            let tail = Arc::clone(&tail);
            thread::spawn(move || drain(stderr, &tail));
        }

        let (read, how) = mpsc::channel::<End>();
        {
            let core = Arc::clone(self);
            let tab = Arc::clone(tab);
            thread::spawn(move || {
                let end = reader::read(stdout, reader::LINE_CAP, |frame| core.chat_frame(&tab, generation, frame));
                if end != End::Eof {
                    core.chat_wire_lost(&tab, generation, end);
                }
                let _ = read.send(end);
            });
        }
        {
            let core = Arc::clone(self);
            let tab = Arc::clone(tab);
            thread::spawn(move || {
                let status = child.wait().ok();
                // What it wrote last is read before its end is told; a pipe a child of its still
                // holds open is waited for a moment only.
                let end = how.recv_timeout(Duration::from_secs(2)).unwrap_or(End::Eof);
                core.chat_ended(&tab, generation, status, end, &tail);
            });
        }
        {
            let core = Arc::clone(self);
            let tab = Arc::clone(tab);
            thread::spawn(move || core.chat_initialize(&tab, generation, suggestions));
        }

        self.record_pids();
        self.chat_changed(tab, false);
        Ok(())
    }

    /// Sends `initialize` and waits for its reply: then the tab knows the process (`info`), and
    /// the messages held go to it. One that doesn't answer is ended.
    fn chat_initialize(&self, tab: &Arc<ChatTab>, generation: u64, suggestions: bool) {
        match self.chat_request(tab, generation, control::initialize(suggestions)) {
            Ok(reply) => {
                // Of the account only its kind and provider are read (`of_initialize`).
                let info = ChatInfo::of_initialize(&reply);
                {
                    let mut shared = tab.lock();
                    if shared.generation != generation || !shared.running {
                        return;
                    }
                    shared.ready = true;
                    shared.info = Some(info.clone());
                    tab.push(&mut shared, own("info", json!({ "info": info })));
                    if shared.state == ChatState::Starting {
                        shared.state = ChatState::Idle;
                        shared.since = now_ms();
                        tab.push(&mut shared, own("state", json!({ "state": ChatState::Idle })));
                    }
                    let held = std::mem::take(&mut shared.held);
                    match &shared.writer {
                        Some(writer) => {
                            let mut went = false;
                            for message in held {
                                went |= writer.send(message.line).is_ok();
                            }
                            shared.sent |= went;
                        }
                        // It is being closed: what was held is told of at its end.
                        None => shared.held = held,
                    }
                }
                self.chat_changed(tab, true);
            }
            Err(error) => {
                {
                    let mut shared = tab.lock();
                    if shared.generation != generation || !shared.running {
                        return;
                    }
                    tab.push(&mut shared, own("error", json!({ "reason": "initialize", "text": error })));
                }
                // No close: its end says it never started (`StartFailed`).
                end_process(tab, false);
            }
        }
    }

    /// A request of the core's to the process (`body`), its reply waited for `REPLY_WAIT`: the
    /// CLI's `response`, or its error. A reply that comes later is dropped.
    fn chat_request(&self, tab: &ChatTab, generation: u64, body: Value) -> Result<Value, String> {
        let id = control::request_id();
        let (reply, replied) = mpsc::channel();
        {
            let mut shared = tab.lock();
            if shared.generation != generation || !shared.running {
                return Err(not_running());
            }
            let line = control::request_line(&id, body).to_string();
            shared.writer.as_ref().ok_or_else(not_running)?.send(line).map_err(|_| not_running())?;
            shared.replies.insert(id.clone(), reply);
        }
        match replied.recv_timeout(REPLY_WAIT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tab.lock().replies.remove(&id);
                Err(t!("chatCore.error.noAnswer"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(not_running()),
        }
    }

    /// One frame of the process, read: replies to the core's requests taken, cards held,
    /// `keep_alive` dropped, the state, the title and what it tells of itself kept, the rest
    /// passed on as it came, made fit for the window (`wire::sanitize`); an untitled session named
    /// once a message's turn began (`chat_name`). Always reads on.
    fn chat_frame(self: &Arc<Self>, tab: &Arc<ChatTab>, generation: u64, mut frame: Value) -> bool {
        let (mut state, mut title, mut adopted, mut name) = (false, false, None, None);
        {
            let mut shared = tab.lock();
            if shared.generation != generation || !shared.running {
                return true;
            }
            // After a reset the process goes on under a new session: the tab's from then on, with
            // no log until a message goes to it, and no dry run or name of the last.
            if shared.reset {
                let next = frame.get("session_id").and_then(Value::as_str).filter(|id| is_uuid(id) && *id != shared.session.as_str()).map(str::to_string);
                if let Some(next) = next {
                    adopted = Some((std::mem::replace(&mut shared.session, next.clone()), next));
                    shared.reset = false;
                    shared.sent = false;
                    shared.dry_runs.clear();
                    shared.title = None;
                    shared.naming = false;
                    state = true;
                }
            }
            let kind = frame.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
            let subtype = frame.get("subtype").and_then(Value::as_str).unwrap_or_default().to_string();
            if matches!(kind.as_str(), "user" | "assistant") {
                stamp(&mut frame);
            }
            match (kind.as_str(), subtype.as_str()) {
                ("control_response", _) => {
                    take_reply(&mut shared, &frame);
                    return true;
                }
                ("control_request", _) => {
                    let Some(id) = frame.get("request_id").and_then(Value::as_str).map(str::to_string) else {
                        return true;
                    };
                    let asked = frame.pointer("/request/subtype").and_then(Value::as_str).unwrap_or_default();
                    // `hook_callback`, `mcp_message` and whatever else: Pitwall registered none.
                    if !control::is_card(asked) {
                        if let Some(writer) = &shared.writer {
                            let _ = writer.send(control::error_line(&id, UNSERVED).to_string());
                        }
                        return true;
                    }
                    wire::sanitize(&mut frame);
                    let request = frame.get("request").cloned().unwrap_or(Value::Null);
                    shared.pending.push(Pending { id, request, at: now_ms() });
                    tab.push(&mut shared, frame);
                    state = true;
                }
                ("control_cancel_request", _) => {
                    let id = frame.get("request_id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let before = shared.pending.len();
                    shared.pending.retain(|pending| pending.id != id);
                    state = shared.pending.len() != before;
                    tab.push(&mut shared, frame);
                }
                ("keep_alive", _) => return true,
                ("system", "session_state_changed") => {
                    if let Some(now) = frame.get("state").and_then(Value::as_str).and_then(ChatState::parse) {
                        state = set_state(&mut shared, now);
                        if state {
                            tab.push(&mut shared, own("state", json!({ "state": now })));
                        }
                        // The message's turn began: the session has it, and a log to keep a title.
                        if now == ChatState::Running {
                            let from = shared.title_from.take();
                            if let Some(from) = from.filter(|_| shared.title.is_none() && !shared.naming) {
                                shared.naming = true;
                                name = Some((shared.session.clone(), from));
                            }
                        }
                    }
                }
                ("system", "session_title_changed") => {
                    let named = frame.get("title").and_then(Value::as_str).map(clean_title);
                    if let Some(named) = named.filter(|named| !named.is_empty()) {
                        title = shared.title.as_deref() != Some(named.as_str());
                        shared.title = Some(named);
                    }
                    tab.push(&mut shared, frame);
                }
                ("system", "init" | "status" | "commands_changed") => {
                    let changed = shared.info.as_mut().is_some_and(|info| info.take(&frame));
                    tab.push(&mut shared, frame);
                    if changed {
                        let info = shared.info.clone();
                        tab.push(&mut shared, own("info", json!({ "info": info })));
                    }
                }
                ("conversation_reset", _) => {
                    shared.reset = true;
                    tab.push(&mut shared, frame);
                    if let Some(info) = shared.info.clone() {
                        tab.push(&mut shared, own("info", json!({ "info": info })));
                    }
                }
                ("result", _) => {
                    wire::sanitize(&mut frame);
                    tab.push(&mut shared, frame);
                    // The turn is over: no card of it is still open, but a background subagent's.
                    state |= tab.close_pending(&mut shared, false);
                }
                _ => {
                    wire::sanitize(&mut frame);
                    tab.push(&mut shared, frame);
                }
            }
        }
        if let Some((was, now)) = adopted {
            // Nothing opens the new session again while its Claude runs here; the last is free.
            let mut cache = self.sessions_cache();
            if cache.reopened.get(&was) == Some(&tab.id) {
                cache.reopened.remove(&was);
            }
            cache.reopened.insert(now, tab.id);
        }
        if state || title {
            self.chat_changed(tab, state);
        }
        if let Some((session, from)) = name {
            self.chat_name(tab, generation, session, from);
        }
        true
    }

    /// Names `session`, which has no title, from `from`, the message its turn began with, as
    /// Claude Code names a session in its terminal: it names none itself in a `-p` run that no
    /// SDK started (`CLAUDE_CODE_ENTRYPOINT` `sdk-cli`, a chat's). The CLI writes the title into
    /// the session's log as its `ai-title`, and it becomes the tab's name unless the user named
    /// the tab. A session whose log has a title already (one opened again) is not asked about;
    /// one the CLI finds nothing to name from waits for the next message.
    fn chat_name(self: &Arc<Self>, tab: &Arc<ChatTab>, generation: u64, session: String, from: String) {
        let core = Arc::clone(self);
        let tab = Arc::clone(tab);
        thread::spawn(move || {
            let titled = match core.claude.lock() {
                Ok(mut watch) => watch.peek(&session, &tab.path).is_some_and(|known| known.title.is_some()),
                Err(_) => true,
            };
            let named = if titled {
                None
            } else {
                core.chat_request(&tab, generation, control::generate_title(&from)).ok().and_then(|reply| reply.get("title").and_then(Value::as_str).map(clean_title)).filter(|title| !title.is_empty())
            };
            let changed = {
                let mut shared = tab.lock();
                if shared.generation != generation {
                    return;
                }
                shared.naming = false;
                match named {
                    Some(named) if shared.session == session && shared.title.is_none() => {
                        shared.title = Some(named);
                        true
                    }
                    _ => false,
                }
            };
            if changed {
                core.chat_changed(&tab, false);
            }
        });
    }

    /// The process wrote what can't be read (`End`): the tab says so, and the process is ended.
    fn chat_wire_lost(&self, tab: &Arc<ChatTab>, generation: u64, end: End) {
        {
            let mut shared = tab.lock();
            if shared.generation != generation || !shared.running {
                return;
            }
            shared.wire_lost = true;
            let text = if end == End::TooLong { t!("chatCore.error.lineTooLong") } else { t!("chatCore.error.wire") };
            tab.push(&mut shared, own("error", json!({ "reason": "wire", "text": text })));
        }
        let tab = Arc::clone(tab);
        thread::spawn(move || end_process(&tab, false));
    }

    /// The process ended (reaped): why, as the window is told (`chat-exit`), every card closed,
    /// the tab `exited` until it is closed or started again; a closed tab goes now.
    fn chat_ended(&self, tab: &Arc<ChatTab>, generation: u64, status: Option<ExitStatus>, end: End, tail: &Mutex<VecDeque<u8>>) {
        let code = status.and_then(|status| status.code());
        let stderr = {
            let mut tail = tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            String::from_utf8_lossy(tail.make_contiguous()).into_owned()
        };
        let (pid, gone) = {
            let mut shared = tab.lock();
            if shared.generation != generation {
                return;
            }
            let reason = if end != End::Eof || shared.wire_lost {
                ChatExitReason::Error
            } else if shared.closing {
                ChatExitReason::Closed
            } else if stderr.contains("No conversation found") {
                ChatExitReason::NotFound
            } else if !shared.ready {
                ChatExitReason::StartFailed
            } else if code == Some(0) {
                ChatExitReason::Closed
            } else {
                ChatExitReason::Error
            };
            shared.running = false;
            shared.ready = false;
            shared.writer = None;
            shared.replies.clear();
            tab.close_pending(&mut shared, true);
            // What was held for it never went: the tab says so and hands it back (`unsent`: the
            // drawn messages' uuids, their text for the composer), the `!` runs it carried left
            // for the next message.
            let unsent = std::mem::take(&mut shared.held);
            if !unsent.is_empty() {
                let uuids: Vec<&str> = unsent.iter().map(|message| message.uuid.as_str()).collect();
                let text = unsent.iter().map(|message| message.text.as_str()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n\n");
                tab.push(&mut shared, own("error", json!({ "reason": "unsent", "text": t!("chatCore.error.unsent") })));
                tab.push(&mut shared, own("unsent", json!({ "uuids": uuids, "text": text })));
                let mut carry: Vec<ShellCarry> = unsent.into_iter().flat_map(|message| message.carry).collect();
                carry.append(&mut shared.carry);
                let over = carry.len().saturating_sub(CARRY_MAX);
                carry.drain(..over);
                shared.carry = carry;
            }
            shared.state = ChatState::Exited;
            shared.since = now_ms();
            tab.push(&mut shared, own("state", json!({ "state": ChatState::Exited })));
            let told = (reason != ChatExitReason::Closed).then(|| last_bytes(&stderr, STDERR_TOLD).to_string()).filter(|text| !text.trim().is_empty());
            let _ = tab.out.send(Out::Exit { code, reason, stderr: told });
            tab.changed.notify_all();
            (shared.pid.take(), shared.gone)
        };
        // What it started and left in its group (a server, a background command, an MCP server)
        // goes with it, whether it ended by itself or was closed.
        if let Some(pid) = pid.filter(|&pid| process::group_alive(pid)) {
            process::kill_tree(pid, true);
        }
        if gone {
            let mut chats = self.chats();
            if chats.get(&tab.id).is_some_and(|known| Arc::ptr_eq(known, tab)) {
                chats.remove(&tab.id);
            }
        }
        self.tab_gone(pid);
        self.record_pids();
        if gone {
            self.refresh_claude();
        } else {
            self.chat_changed(tab, true);
        }
    }

    /// The tab's view brought level with its chat (its state and title, its name from the title
    /// unless the user named it); the watch told when its state changed (`refresh`).
    fn chat_changed(&self, tab: &ChatTab, refresh: bool) {
        let (state, title, session) = {
            let shared = tab.lock();
            if shared.gone {
                return;
            }
            (shared.state, shared.title.clone(), shared.session.clone())
        };
        let changed = {
            let mut inner = self.lock();
            match inner.terminal_list.iter_mut().find(|view| view.id == tab.id) {
                Some(view) => {
                    let chat = ChatView { session: Some(session), state, title: title.clone() };
                    let mut changed = view.chat.as_ref() != Some(&chat);
                    view.chat = Some(chat);
                    if let Some(name) = title.filter(|_| !view.renamed).map(|title| tab_name(&title)).filter(|name| !name.is_empty() && *name != view.name) {
                        view.name = name;
                        changed = true;
                    }
                    changed
                }
                None => false,
            }
        };
        if changed {
            self.notify();
        }
        if refresh {
            self.refresh_claude();
        }
    }

    /// The thread that sends tab `id`'s frames to the window: gathered for `BATCH` after the first,
    /// as one event, and the process's end after them. It ends with the tab.
    fn send_chat_events(&self, id: u64, events: mpsc::Receiver<Out>) {
        let sink = Arc::clone(&self.sink);
        thread::spawn(move || {
            while let Ok(first) = events.recv() {
                let deadline = Instant::now() + BATCH;
                let (mut frames, mut seq, mut exit) = (Vec::new(), 0, None);
                let mut next = Some(first);
                while let Some(out) = next.take() {
                    match out {
                        Out::Frame(n, frame) => {
                            seq = n;
                            frames.push(frame);
                        }
                        Out::Exit { code, reason, stderr } => {
                            exit = Some(CoreEvent::ChatExit { id, code, reason, stderr });
                            break;
                        }
                    }
                    next = events.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok();
                }
                if !frames.is_empty() {
                    sink(CoreEvent::Chat { id, seq, frames });
                }
                if let Some(exit) = exit {
                    sink(exit);
                }
            }
        });
    }

    /* ---------- the window's commands ---------- */

    /// Sends the user's message: the uuid the core mints for it (the window's to rewind to), with
    /// the `!` runs since the last message before its text.
    pub fn chat_send(&self, id: u64, message: ChatOutgoing) -> Result<String, String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let mut shared = tab.lock();
        if !shared.running {
            return Err(not_running());
        }
        message.checked(!shared.carry.is_empty())?;
        let uuid = new_uuid();
        let line = wire::user_message(&message, &uuid, &shared.carry).to_string();
        let carry = shared.carry.clone();
        shared.send(Waiting { uuid: uuid.clone(), line, text: message.text.clone(), carry })?;
        shared.title_from = title_source(&message.text);
        shared.carry.clear();
        // The turn it starts may change the files a dry run listed: it is asked again.
        shared.dry_runs.clear();
        let (live, held) = echoed(&uuid, &message);
        tab.push_as(&mut shared, live, held);
        Ok(uuid)
    }

    /// The user's answer to card `request`: only a card the CLI asked and still holds open, and
    /// only as `answer_body` builds it on that card's own request.
    pub fn chat_answer(&self, id: u64, request: &str, answer: &ChatAnswer) -> Result<(), String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        {
            let mut shared = tab.lock();
            let at = shared.pending.iter().position(|pending| pending.id == request).ok_or_else(|| t!("chatCore.error.notPending"))?;
            let body = control::answer_body(&shared.pending[at].request, answer)?;
            let writer = shared.writer.as_ref().ok_or_else(not_running)?;
            writer.send(control::answer_line(request, body).to_string()).map_err(|_| not_running())?;
            shared.pending.remove(at);
            tab.push(&mut shared, own("answered", json!({ "requestId": request })));
        }
        self.chat_changed(&tab, true);
        Ok(())
    }

    /// Stops the turn under way (`cancel_queued`: and the messages queued behind it).
    pub fn chat_interrupt(&self, id: u64, cancel_queued: bool) -> Result<ChatInterruptReceipt, String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let generation = {
            let mut shared = tab.lock();
            if !shared.running {
                return Err(not_running());
            }
            shared.interrupted = true;
            shared.generation
        };
        let mut body = json!({ "subtype": "interrupt" });
        if cancel_queued {
            body["cancel_queued"] = Value::Bool(true);
        }
        let reply = self.chat_request(&tab, generation, body)?;
        let ids = |key: &str| -> Vec<String> { reply.get(key).and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_str).filter(|id| is_uuid(id)).map(str::to_string).collect()).unwrap_or_default() };
        Ok(ChatInterruptReceipt { still_queued: ids("still_queued"), cancelled: ids("cancelled") })
    }

    /// One of the window's controls (`ChatControl`), checked against what the process said of
    /// itself, and the CLI's reply with what names the account taken out (`scrub_answer`).
    pub fn chat_control(&self, id: u64, control: &ChatControl) -> Result<Value, String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let (generation, models, agents, styles, dry_runs) = {
            let shared = tab.lock();
            if !shared.running || !shared.ready {
                return Err(not_running());
            }
            let info = shared.info.clone().unwrap_or_default();
            (shared.generation, info.model_values(), info.agents.iter().map(|agent| agent.name.clone()).collect::<Vec<_>>(), info.output_styles.clone(), shared.dry_runs.clone())
        };
        let body = control::request_body(control, &Known { models: &models, agents: &agents, styles: &styles }, &dry_runs)?;
        let mut reply = self.chat_request(&tab, generation, body)?;
        if let ChatControl::RewindFiles { message, dry_run } = control {
            let mut shared = tab.lock();
            if !dry_run {
                shared.dry_runs.remove(message);
            } else if reply.get("canRewind").and_then(Value::as_bool) == Some(true) {
                shared.dry_runs.insert(message.clone());
            }
        }
        wire::scrub_answer(&mut reply);
        if !control.carries_content() {
            wire::scrub_emails(&mut reply);
        }
        Ok(reply)
    }

    /// What the tab holds after frame `since`, its state, what it knows of the process and the
    /// cards still open: for a window that draws it again.
    pub fn chat_buffer(&self, id: u64, since: u64) -> Result<ChatBuffer, String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let shared = tab.lock();
        Ok(ChatBuffer {
            seq: shared.seq,
            frames: shared.frames.iter().filter(|held| held.n > since).map(|held| held.frame.clone()).collect(),
            state: shared.state,
            info: shared.info.clone(),
            pending: shared.pending.iter().map(|pending| ChatPending { request_id: pending.id.clone(), request: pending.request.clone() }).collect(),
        })
    }

    /// A folder the user picked for Claude to work in too (`/add-dir`): `register_repo_root`.
    pub fn chat_add_dir(&self, id: u64, folder: &Path) -> Result<(), String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        if !folder.is_dir() {
            return Err(refused());
        }
        let generation = {
            let shared = tab.lock();
            if !shared.ready {
                return Err(not_running());
            }
            shared.generation
        };
        self.chat_request(&tab, generation, json!({ "subtype": "register_repo_root", "directory": folder.to_string_lossy() })).map(|_| ())
    }

    /// The chat that exited starts again on its session (`session_now`). Never while it runs
    /// elsewhere, nor while another tab Claude may run it in holds it: it goes through
    /// `open_once`, as every opening of a session does.
    pub fn chat_restart(self: &Arc<Self>, id: u64) -> Result<(), String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let (running, session) = {
            let shared = tab.lock();
            (shared.running, shared.session.clone())
        };
        if running {
            return Ok(());
        }
        if !self.agent_on(Agent::Claude) {
            return Err(t!("terminal.claudeOff"));
        }
        let elsewhere = || {
            let _ = tab.out.send(Out::Exit { code: None, reason: ChatExitReason::Elsewhere, stderr: None });
            Err(ELSEWHERE.to_string())
        };
        if let Liveness::Running { .. } = self.liveness(&session) {
            return elsewhere();
        }
        let start = || {
            self.start_chat(&tab, self.session_now(&tab, &session))?;
            self.chat_view(id).ok_or_else(gone)
        };
        match self.open_once(&session, start) {
            Opened::New(_) => Ok(()),
            // Its own process, started since by another restart.
            Opened::Held(held) if held == id => Ok(()),
            Opened::Held(_) | Opened::Opening => elsewhere(),
            Opened::Failed(error) => Err(error),
        }
    }

    /// The session a chat's next process runs on: going on with it once it has a log (a message
    /// that was only held left none), else the fork the tab was opened as (its copy is made with
    /// its first message), else a new one under its id.
    fn session_now(&self, tab: &ChatTab, session: &str) -> ChatSession {
        if self.session_log(session).is_some() {
            return ChatSession::Resume(session.to_string());
        }
        match &tab.origin {
            ChatSession::Fork { from, at, id } if id == session => ChatSession::Fork { from: from.clone(), at: at.clone(), id: id.clone() },
            _ => ChatSession::New(session.to_string()),
        }
    }

    /// Ends the chat and opens its session in the Claude Code terminal instead, in the tab's place,
    /// with its account: going on with it once it has turns, else a new session under its id. The
    /// chat's process has ended before the terminal's starts.
    pub fn chat_to_terminal(self: &Arc<Self>, id: u64) -> Result<TerminalView, String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        // A fork no message went to has no session yet that the terminal could open from a log
        // (its cut is print mode's alone): it stays a chat, its process untouched.
        let (session, sent) = {
            let shared = tab.lock();
            (shared.session.clone(), shared.sent)
        };
        if !sent && matches!(self.session_now(&tab, &session), ChatSession::Fork { .. }) {
            return Err(t!("chatCore.error.noLog"));
        }
        end_process(&tab, true);
        let (session, running) = {
            let shared = tab.lock();
            (shared.session.clone(), shared.running)
        };
        if running {
            return Err(t!("chatCore.error.stillRunning"));
        }
        let (name, renamed) = self.chat_view(id).map(|view| (view.name, view.renamed)).unwrap_or_default();
        let account = tab.account.as_ref().map_or(crate::accounts::MAIN, |(id, _)| id.as_str()).to_string();
        let launch = match self.session_now(&tab, &session) {
            ChatSession::Resume(session) => Launch::Resume { session },
            ChatSession::New(session) => {
                let mode = match tab.start.mode {
                    ChatMode::Plan => Mode::Plan,
                    ChatMode::AcceptEdits => Mode::AcceptEdits,
                    _ => Mode::Normal,
                };
                Launch::Claude { prompt: None, start: Start { model: tab.start.model, mode, ..Start::default() }, images: None, session: Some(session) }
            }
            // Its message went but left no log: a restart makes the fork again.
            ChatSession::Fork { .. } => return Err(t!("chatCore.error.noLog")),
        };
        let open = || self.open_terminal_with(&tab.path, &launch, Some(name.clone()), renamed, None, false, Some(&account));
        let view = match self.open_once(&session, open) {
            Opened::New(view) => view,
            Opened::Held(_) | Opened::Opening => return Err(ELSEWHERE.into()),
            Opened::Failed(error) => return Err(error),
        };

        // The terminal takes the chat's place: in the tabs' order, in the panes, on its cards. Its
        // `!` runs end with the chat.
        {
            let mut shared = tab.lock();
            shared.gone = true;
            shared.stop_shells();
        }
        self.chats().remove(&id);
        {
            let mut inner = self.lock();
            if let (Some(at), Some(new)) = (inner.terminal_list.iter().position(|known| known.id == id), inner.terminal_list.iter().position(|known| known.id == view.id)) {
                let moved = inner.terminal_list.remove(new);
                let at = if new < at { at - 1 } else { at };
                inner.terminal_list[at] = moved;
            }
            if let Some(layout) = inner.terminal_layouts.get_mut(&tab.path) {
                layout.swap(id, view.id, now_ms());
            }
        }
        self.board_terminal_moved(id, view.id);
        self.board_resumed(&session, view.id);
        self.emit(CoreEvent::TerminalExit { id });
        self.notify();
        Ok(view)
    }

    /// Closes chat tab `id`: the tab goes now, its `!` runs are stopped and its process runs the
    /// close ladder (`end_process`) off the caller's thread. False when `id` is no chat.
    pub(super) fn close_chat(&self, id: u64) -> bool {
        let Some(tab) = self.chat(id) else {
            return false;
        };
        let running = {
            let mut shared = tab.lock();
            shared.gone = true;
            shared.stop_shells();
            shared.running
        };
        if !running {
            self.chats().remove(&id);
        } else {
            let tab = Arc::clone(&tab);
            thread::spawn(move || end_process(&tab, true));
        }
        let removed = {
            let mut inner = self.lock();
            let before = inner.terminal_list.len();
            inner.terminal_list.retain(|view| view.id != id);
            before != inner.terminal_list.len()
        };
        if removed {
            self.emit(CoreEvent::TerminalExit { id });
            self.notify();
        }
        self.board_terminal_gone(id);
        true
    }

    /// Quit: every chat's turn under way is stopped and its stdin closed at once, its `!` runs
    /// stopped, and the chats get `QUIT_WAIT` together to end by themselves (the CLI writes its
    /// transcript after the end of stdin); what still runs is stopped with the rest
    /// (`terminal_pids`).
    pub(super) fn quit_chats(&self) {
        let tabs: Vec<Arc<ChatTab>> = self.chats().values().cloned().collect();
        let mut ending = Vec::new();
        for tab in tabs {
            let mut shared = tab.lock();
            shared.stop_shells();
            if let Some(generation) = close_input(&tab, &mut shared) {
                drop(shared);
                ending.push((tab, generation));
            }
        }
        let deadline = Instant::now() + QUIT_WAIT;
        for (tab, generation) in &ending {
            wait_gone(tab, *generation, deadline.saturating_duration_since(Instant::now()));
        }
    }

    /// Runs `line` as the `!` mode does, as a saved command runs (the user's shell in the project's
    /// folder, a process group of its own): its output comes as `shell` frames, and its last lines
    /// go to Claude with the next message. The run's id.
    pub fn chat_shell(self: &Arc<Self>, id: u64, line: &str) -> Result<String, String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let line = line.trim().to_string();
        if line.is_empty() || line.chars().count() > SHELL_LINE_MAX || line.contains('\0') {
            return Err(refused());
        }
        let mut child = process::spawn_shell(&line, &tab.path).map_err(|error| t!("chatCore.error.shell", error = error))?;
        let pid = child.id();
        let run = format!("sh_{}", &new_uuid()[..8]);
        {
            let mut shared = tab.lock();
            shared.shells.insert(run.clone(), pid);
            tab.push(&mut shared, own("shell", json!({ "run": run, "line": line })));
        }
        self.record_pids();

        let readers: Vec<thread::JoinHandle<String>> = [child.stdout.take().map(|out| Box::new(out) as Box<dyn Read + Send>), child.stderr.take().map(|err| Box::new(err) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
            .map(|source| {
                let tab = Arc::clone(&tab);
                let run = run.clone();
                thread::spawn(move || shell_output(&tab, &run, source))
            })
            .collect();
        let core = Arc::clone(self);
        let ran = run.clone();
        thread::spawn(move || {
            let mut tails = readers.into_iter().map(|reader| reader.join().unwrap_or_default());
            let (stdout, stderr) = (tails.next().unwrap_or_default(), tails.next().unwrap_or_default());
            let code = child.wait().ok().and_then(|status| status.code());
            {
                let mut shared = tab.lock();
                shared.shells.remove(&ran);
                let stderr = last_lines(&stderr, SHELL_LINES / 4, SHELL_BYTES / 4);
                let stdout = last_lines(&stdout, SHELL_LINES - stderr.lines().count(), SHELL_BYTES - stderr.len());
                shared.carry.push(ShellCarry { line, stdout, stderr });
                let over = shared.carry.len().saturating_sub(CARRY_MAX);
                shared.carry.drain(..over);
                tab.push(&mut shared, own("shell", json!({ "run": ran, "done": true, "code": code })));
            }
            core.record_pids();
        });
        Ok(run)
    }

    /// Stops `!` run `run` of the tab: its group gets SIGTERM, then SIGKILL.
    pub fn chat_shell_stop(&self, id: u64, run: &str) -> Result<(), String> {
        let tab = self.chat(id).ok_or_else(gone)?;
        let Some(pid) = tab.lock().shells.get(run).copied() else {
            return Ok(());
        };
        process::kill_tree(pid, false);
        let run = run.to_string();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(600));
            if tab.lock().shells.get(&run) == Some(&pid) {
                process::kill_tree(pid, true);
            }
        });
        Ok(())
    }

    /// The project's chats are allowed (O6): its hooks and MCP servers run in them. This Mac's own;
    /// a listed project's only.
    pub fn chat_trust(&self, path: &str) -> Result<(), String> {
        if !self.is_listed(path) {
            return Err(t!("core.error.depsNotListed", path = path));
        }
        {
            let mut inner = self.lock();
            let mut project = inner.settings.project(path);
            if project.chat_trusted {
                return Ok(());
            }
            project.chat_trusted = true;
            inner.settings.projects.insert(path.to_string(), project);
        }
        self.save_settings();
        self.notify();
        Ok(())
    }

    /// The chat's preferences changed (`patch`: some of `ChatSettings`, in its JSON names): a value
    /// of the wrong kind, or a word not of its list, changes nothing.
    pub fn set_chat_settings(&self, patch: &serde_json::Map<String, Value>) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let mut merged = serde_json::to_value(&inner.settings.chat).map_err(|error| error.to_string())?;
            if let Some(fields) = merged.as_object_mut() {
                // The ready prompts have a way of their own (`set_chat_examples`), which trims and bounds them.
                for (key, value) in patch.iter().filter(|(key, _)| key.as_str() != "examples") {
                    if fields.get(key).is_some_and(|known| std::mem::discriminant(known) == std::mem::discriminant(value)) {
                        fields.insert(key.clone(), value.clone());
                    }
                }
            }
            let next: crate::settings::ChatSettings = serde_json::from_value(merged).map_err(|_| refused())?;
            if !matches!(next.open.as_str(), "terminal" | "chat") || !matches!(next.send_key.as_str(), "enter" | "mod-enter") || !matches!(next.mode.as_str(), "default" | "auto") {
                return Err(refused());
            }
            if inner.settings.chat == next {
                return Ok(());
            }
            inner.settings.chat = next;
        }
        self.save_settings();
        self.notify();
        Ok(())
    }

    /// The empty chat's ready prompts, written whole as the window left them: each one trimmed, an
    /// empty one dropped, a control character made a space, at most `CHAT_EXAMPLES_MOST` of them
    /// and `CHAT_EXAMPLE_CHARS` characters each.
    pub fn set_chat_examples(&self, examples: Vec<String>) {
        let examples: Vec<String> = examples
            .iter()
            .map(|text| text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>())
            .map(|text| text.trim().chars().take(CHAT_EXAMPLE_CHARS).collect::<String>())
            .filter(|text| !text.is_empty())
            .take(CHAT_EXAMPLES_MOST)
            .collect();
        {
            let mut inner = self.lock();
            if inner.settings.chat.examples.as_ref() == Some(&examples) {
                return;
            }
            inner.settings.chat.examples = Some(examples);
        }
        self.save_settings();
        self.notify();
    }

    /// A board card is given to a chat in `path` rather than to the terminal: Settings › Claude
    /// says chats, and the project's chats were allowed (else it opens in the terminal, as before).
    pub(super) fn cards_go_to_chats(&self, path: &str) -> bool {
        let inner = self.lock();
        inner.settings.chat.opens_chat() && inner.settings.project(path).chat_trusted
    }

    /// A board card as a chat's first message: its text, its images as images (those that can't
    /// be sent so named by their files, as a terminal's prompt names them), and the line that
    /// asks for a summary.
    pub(super) fn card_message(&self, card: &super::board::Card) -> ChatOutgoing {
        let mut text = if card.note.is_empty() { card.title.clone() } else { format!("{}\n\n{}", card.title, card.note) };
        let mut message = ChatOutgoing::text(String::new());
        let mut named: Vec<(u32, PathBuf)> = Vec::new();
        for (n, file) in self.card_images(card).map(|(_, files)| files).unwrap_or_default() {
            let kind = match file.extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("png") => Some("image/png"),
                Some("jpg" | "jpeg") => Some("image/jpeg"),
                Some("gif") => Some("image/gif"),
                Some("webp") => Some("image/webp"),
                _ => None,
            };
            let data = kind.and_then(|_| fs::read(&file).ok()).map(|bytes| {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            });
            match (kind, data) {
                (Some(kind), Some(data)) if data.len() <= wire::IMAGE_BASE64_MAX && message.images.len() < 20 => message.images.push(wire::OutImage { media_type: kind.into(), data }),
                _ => named.push((n, file)),
            }
        }
        if !named.is_empty() {
            text.push_str(&format!("\n\n{}", t!("board.imagesPrompt")));
            for (n, file) in named {
                text.push_str(&format!("\n[Image #{n}] {}", file.display()));
            }
        }
        message.text = format!("{text}\n\n{}", t!("board.summaryPrompt")).chars().take(wire::TEXT_MAX).collect();
        message
    }

    /// A board card given to a chat (`cards_go_to_chats`): a new session with the card's start
    /// and its text as the first message, the tab named after it.
    pub(super) fn card_chat(self: &Arc<Self>, card: &super::board::Card, start: Start, unasked: bool) -> Result<TerminalView, String> {
        let program = chat_program().ok_or_else(|| t!("chatCore.error.noClaude"))?;
        let open = ChatOpen { start: Some(ChatStart::from_start(start)), prompt: Some(self.card_message(card)), ..ChatOpen::default() };
        self.open_chat_as(&card.path, open, program, Some(tab_name(&card.title)), false, unasked)
    }

    /// A chat a record put back (a quit's, an update's): its session going on in a chat, with the
    /// name and the account it had; in the terminal instead while the project's chats aren't
    /// allowed, `claude` isn't found or the session has no log. The caller has made sure nothing
    /// runs it (`open_once`).
    pub(super) fn chat_back(self: &Arc<Self>, path: &str, session: &str, name: Option<String>, renamed: bool, account: &str) -> Result<TerminalView, String> {
        let in_terminal = || self.open_terminal_with(path, &Launch::Resume { session: session.to_string() }, name.clone(), renamed, None, false, Some(account));
        let trusted = self.lock().settings.project(path).chat_trusted;
        let (Some(program), Some((log, _)), true) = (chat_program(), self.session_log(session), trusted) else {
            return in_terminal();
        };
        let made = self.chat_made(path, None, Some(account), program, None, false, name.clone(), renamed, false)?;
        self.make_chat(made, ChatSession::Resume(session.to_string()), history::read(&log))
    }

    /// The chats a record names: by tab, the session and whether a turn of it is at work. One
    /// whose session has no log, and was sent nothing that will write one, isn't named.
    pub(super) fn chat_records(&self) -> HashMap<u64, (String, bool)> {
        let tabs: Vec<Arc<ChatTab>> = self.chats().values().cloned().collect();
        let chats: Vec<(u64, String, bool, bool)> = tabs
            .iter()
            .filter_map(|tab| {
                let shared = tab.lock();
                (!shared.gone).then(|| (tab.id, shared.session.clone(), shared.sent, matches!(shared.state, ChatState::Running | ChatState::RequiresAction)))
            })
            .collect();
        chats.into_iter().filter(|(_, session, sent, _)| *sent || self.session_log(session).is_some()).map(|(id, session, _, working)| (id, (session, working))).collect()
    }

    /// Where each running chat's session stands by the chat itself (its cards, its state), for the
    /// watch to take over its own reading (`apply_chats`).
    pub(super) fn chat_truths(&self) -> Vec<ChatTruth> {
        let tabs: Vec<Arc<ChatTab>> = self.chats().values().cloned().collect();
        tabs.iter()
            .filter_map(|tab| {
                let shared = tab.lock();
                if !shared.running || shared.gone {
                    return None;
                }
                let phase = match shared.pending.first() {
                    Some(card) => {
                        let (kind, tool) = card_kind(&card.request);
                        TruthPhase::Waiting { turn: Turn { kind, at: card.at }, tool }
                    }
                    None => match (shared.state, shared.finished) {
                        (ChatState::Running | ChatState::RequiresAction, _) => TruthPhase::Working { since: shared.turn_began.max(1) },
                        (_, Some(at)) => TruthPhase::Finished { at },
                        _ => TruthPhase::Idle,
                    },
                };
                Some(ChatTruth { session: shared.session.clone(), path: tab.path.clone(), phase, title: shared.title.clone(), interrupted: shared.interrupted })
            })
            .collect()
    }

    /// Each chat tab the user didn't name takes its session's name: the one the chat was told,
    /// else the log's (`/rename`), as `name_terminals` names a Claude tab.
    pub(super) fn name_chats(&self) {
        let open: Vec<(u64, String, String)> = self
            .lock()
            .terminal_list
            .iter()
            .filter(|view| view.kind == CHAT && !view.renamed)
            .filter_map(|view| Some((view.id, view.path.clone(), view.chat.as_ref()?.session.clone()?)))
            .collect();
        if open.is_empty() {
            return;
        }
        let told: HashMap<u64, Option<String>> = self.chats().iter().map(|(id, tab)| (*id, tab.lock().title.clone())).collect();
        let names: Vec<(u64, String)> = match self.claude.lock() {
            Ok(mut watch) => open
                .into_iter()
                .filter_map(|(id, path, session)| {
                    let title = told.get(&id).cloned().flatten().or_else(|| watch.peek(&session, &path)?.title)?;
                    Some((id, tab_name(&title)))
                })
                .filter(|(_, name)| !name.is_empty())
                .collect(),
            Err(_) => return,
        };
        self.give_names(names);
    }

    /// The last lines of terminal tab `id` (not a chat's), escapes taken out: for a chat to take as
    /// context.
    pub fn chat_terminal_text(&self, id: u64) -> String {
        let text = wire::strip_ansi(&self.terminal_buffer(id).data);
        last_lines(&text, SHELL_LINES, 256 * 1024)
    }

    fn bookmarks_file(&self) -> PathBuf {
        self.cfg.settings_file.with_file_name(meta::BOOKMARKS)
    }

    pub(super) fn sessions_meta_file(&self) -> PathBuf {
        self.cfg.settings_file.with_file_name(meta::SESSIONS_META)
    }

    pub fn chat_bookmarks(&self, session: &str) -> Vec<String> {
        meta::bookmarks(&self.bookmarks_file(), session)
    }

    pub fn chat_bookmark(&self, session: &str, message: &str, on: bool) -> Result<(), String> {
        meta::set_bookmark(&self.bookmarks_file(), session, message, on)
    }

    pub fn sessions_meta(&self) -> meta::SessionsMeta {
        meta::sessions_meta(&self.sessions_meta_file())
    }

    pub fn set_sessions_meta(&self, patch: meta::SessionsMetaPatch) -> Result<meta::SessionsMeta, String> {
        meta::change_sessions_meta(&self.sessions_meta_file(), patch)
    }

    /// Whether chat tab `id` exists, open: the window's export names one.
    pub fn chat_exists(&self, id: u64) -> bool {
        self.chat(id).is_some()
    }
}

/// Reads a `!` run's output to its end as `shell` frames, escapes taken out; what it printed last
/// (`SHELL_BYTES` a few times over) for the carry.
fn shell_output(tab: &ChatTab, run: &str, mut source: Box<dyn Read + Send>) -> String {
    let mut tail = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let read = match source.read(&mut buffer) {
            Ok(0) | Err(_) => 0,
            Ok(read) => read,
        };
        if read == 0 && pending.is_empty() {
            break;
        }
        pending.extend_from_slice(&buffer[..read]);
        // A character cut at the chunk's end waits for the rest of it.
        let whole = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(error) if read > 0 && error.error_len().is_none() => error.valid_up_to(),
            Err(_) => pending.len(),
        };
        let text = wire::strip_ansi(&String::from_utf8_lossy(&pending[..whole]));
        pending.drain(..whole);
        if !text.is_empty() {
            tail.push_str(&text);
            if tail.len() > SHELL_BYTES * 4 {
                tail = last_bytes(&tail, SHELL_BYTES * 2).to_string();
            }
            let mut shared = tab.lock();
            tab.push(&mut shared, own("shell", json!({ "run": run, "text": text })));
        }
        if read == 0 {
            break;
        }
    }
    tail
}

/// Reads stderr to its end, its last `STDERR_KEEP` bytes kept in memory.
fn drain(mut stderr: impl Read, tail: &Mutex<VecDeque<u8>>) {
    let mut buffer = [0u8; 4096];
    while let Ok(read) = stderr.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let mut tail = tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        tail.extend(&buffer[..read]);
        let over = tail.len().saturating_sub(STDERR_KEEP);
        tail.drain(..over);
    }
}

/// Stops the turn under way and closes the process's stdin: what a close begins with. The
/// process's generation, or none when none runs.
fn close_input(tab: &ChatTab, shared: &mut Shared) -> Option<u64> {
    if !shared.running {
        return None;
    }
    shared.closing = true;
    if matches!(shared.state, ChatState::Running | ChatState::RequiresAction) {
        if let Some(writer) = &shared.writer {
            let _ = writer.send(control::request_line(&control::request_id(), json!({ "subtype": "interrupt" })).to_string());
        }
    }
    // The writer's thread writes what it has, then stdin ends.
    shared.writer = None;
    tab.changed.notify_all();
    Some(shared.generation)
}

/// The close ladder: the turn under way interrupted, stdin closed, `EXIT_WAIT` for the process to
/// end (it writes its transcript after the end of its input), SIGTERM to its group, `EXIT_WAIT`,
/// SIGKILL. `closing`: its end is a close, no failure. Returns once it ended or the ladder ran out.
fn end_process(tab: &ChatTab, closing: bool) {
    let generation = {
        let mut shared = tab.lock();
        let wire_lost = shared.wire_lost;
        let Some(generation) = close_input(tab, &mut shared) else {
            return;
        };
        shared.closing = closing && !wire_lost;
        generation
    };
    for force in [false, true] {
        if wait_gone(tab, generation, EXIT_WAIT) {
            return;
        }
        let shared = tab.lock();
        if shared.generation != generation || !shared.running {
            return;
        }
        if let Some(pid) = shared.pid {
            cli::stop(pid, force);
        }
    }
    wait_gone(tab, generation, Duration::from_secs(1));
}

/// Waits up to `wait` for the process of `generation` to have ended. Whether it has.
fn wait_gone(tab: &ChatTab, generation: u64, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    let mut shared = tab.lock();
    loop {
        if shared.generation != generation || !shared.running {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        shared = tab.changed.wait_timeout(shared, left).map(|(shared, _)| shared).unwrap_or_else(|poisoned| poisoned.into_inner().0);
    }
}

/// The chats' truths laid over the watch's scan (`ClaudeWatch::evaluate`): each running chat's
/// session stands where the chat says, in its listed project, whatever the hook or the log said
/// of it, and the projects it touched wait and work by their sessions again (`reckon`). A turn
/// that finished counts as waiting only after the project's last look (`baseline`).
pub(super) fn apply_chats(scan: &mut crate::claude::ScanResult, truths: Vec<ChatTruth>, listed: &[String], baseline: impl Fn(&str) -> u64, reckon: impl Fn(&mut crate::claude::ScanResult, &str)) {
    let mut touched: HashSet<String> = HashSet::new();
    for truth in truths {
        let Some(path) = listed.iter().find(|listed| same_path(listed, &truth.path)).cloned() else {
            continue;
        };
        let old = match scan.sessions.iter().position(|state| state.id == truth.session) {
            Some(at) => Some(scan.sessions.remove(at)),
            None => scan.others.iter().position(|state| state.id == truth.session).map(|at| scan.others.remove(at)),
        };
        if let Some(old) = &old {
            touched.insert(old.path.clone());
        }
        let (phase, turn, since, tool) = match truth.phase {
            TruthPhase::Idle => (SessionPhase::Idle, None, None, None),
            TruthPhase::Working { since } => (SessionPhase::Working, None, Some(since), None),
            TruthPhase::Waiting { turn, tool } => (SessionPhase::Waiting, Some(turn), Some(turn.at), tool),
            TruthPhase::Finished { at } if at > baseline(&path) => (SessionPhase::Waiting, Some(Turn { kind: TurnKind::Finished, at }), Some(at), None),
            TruthPhase::Finished { .. } => (SessionPhase::Idle, None, None, None),
        };
        scan.sessions.push(SessionState {
            id: truth.session,
            agent: Agent::Claude,
            path: path.clone(),
            folder: old.as_ref().and_then(|old| old.folder.clone()).or_else(|| Some(path.clone())),
            title: old.as_ref().and_then(|old| old.title.clone()).or(truth.title),
            phase,
            turn,
            since,
            hooked: true,
            ended: false,
            tool,
            subagents: old.as_ref().map_or(0, |old| old.subagents),
            interrupted: phase == SessionPhase::Idle && truth.interrupted,
        });
        touched.insert(path);
    }
    for path in touched.iter().filter(|path| !path.is_empty()) {
        reckon(scan, path);
    }
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, not(unix)))]
mod tests {
    thread_local! {
        pub(super) static PROGRAM: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
    }
}
