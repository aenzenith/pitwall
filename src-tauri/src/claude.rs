//! Claude Code session watcher. Port of the extension's `src/claude.ts`.
//! Reads only the last lines of each session log, and only to see whether the turn ended;
//! message text is never kept. One exception: what Claude last said in a session, read when
//! that session's details ask for it (`last_message`: the Sessions page, a board card) and
//! handed over, never kept. Where the opt-in hook is installed (hooks.rs), its events decide
//! instead: a prompt sent, the turn over, a permission prompt or question open, the session
//! gone. Logs stay the fallback for sessions the hook hasn't reported. "Seen" state lives in the
//! shared `claude-seen.json`.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::registry::{base36, now_ms, path_key, same_path, write_atomic};

const ASKING_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];
const TAIL_START: u64 = 64 * 1024;
const TAIL_MAX: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnKind {
    /// The turn ended; your move.
    Finished,
    /// Claude asked a question (AskUserQuestion, plan approval).
    Asking,
    /// Claude waits for a permission prompt. Only the hook reports this; the session log
    /// doesn't record it.
    Permission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Turn {
    pub kind: TurnKind,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Verdict {
    pub turn: Option<Turn>,
    pub cwd: Option<String>,
    /// The session's name, as Claude Code shows it in `/resume`: a title set by hand, else the
    /// one Claude Code wrote itself. Only these title lines are read, never message text.
    pub title: Option<String>,
}

/// Where one session stands, for the day's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionPhase {
    Working,
    Waiting,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    /// The session log's file name, without `.jsonl`.
    pub id: String,
    /// The listed project it belongs to; empty for a session elsewhere (`ScanResult::others`).
    pub path: String,
    /// The folder it runs in, as its log or the hook says.
    pub folder: Option<String>,
    pub title: Option<String>,
    pub phase: SessionPhase,
    /// While it waits on you: the turn it waits with.
    pub turn: Option<Turn>,
    /// When its phase began: the hook's event, else the turn it waits with.
    pub since: Option<u64>,
    /// Its hook events decide where it stands (else its log does).
    pub hooked: bool,
    /// The hook heard it end (`SessionEnd`).
    pub ended: bool,
    /// While it waits on a permission prompt: the tool's name, when the hook told it.
    pub tool: Option<String>,
    /// Subagents that ran tools in its current turn, as the hook heard them.
    pub subagents: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenBook {
    /// The first run; older turns count as seen.
    pub since: u64,
    pub paths: HashMap<String, u64>,
}

impl SeenBook {
    /// When the project was last looked at, under whichever spelling of its path (on Windows
    /// the extension writes `c:\…`, the app `C:\…`); the latest look counts.
    fn seen(&self, folder_path: &str) -> Option<u64> {
        self.paths.iter().filter(|(path, _)| same_path(path, folder_path)).map(|(_, at)| *at).max()
    }
}

/// Both sides write the book: keep the larger timestamp per project and the earlier start, so
/// nobody's "I looked" is lost.
pub fn merge_seen(disk: Option<SeenBook>, mine: SeenBook) -> SeenBook {
    let Some(mut merged) = disk else {
        return mine;
    };

    for (path, at) in mine.paths {
        let slot = merged.paths.entry(path).or_insert(0);
        *slot = (*slot).max(at);
    }

    merged.since = merged.since.min(mine.since);
    merged
}

/// Milliseconds since the epoch for `2026-09-24T17:33:19.947Z`.
fn parse_iso_ms(value: &str) -> Option<u64> {
    let (date, time) = value.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|part| part.parse::<i64>());
    let (year, month, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':');
    let (hour, minute) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let seconds: f64 = t.next()?.parse().ok()?;

    // Days from civil (Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    let ms = ((days * 86_400 + hour * 3_600 + minute * 60) as f64 + seconds) * 1000.0;

    (ms >= 0.0).then_some(ms as u64)
}

/// What the last decisive line of a session log says. Side-chain (sub-agent) and meta entries
/// are skipped. `None` if no line decides.
pub fn read_tail(lines: &[&str]) -> Option<Verdict> {
    for line in lines.iter().rev() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let kind = entry.get("type").and_then(Value::as_str);
        let flag = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);

        if !matches!(kind, Some("user") | Some("assistant")) || flag("isSidechain") || flag("isMeta") {
            continue;
        }

        let cwd = entry.get("cwd").and_then(Value::as_str).map(str::to_string);

        if kind == Some("user") {
            return Some(Verdict { turn: None, cwd, title: None });
        }

        let message = entry.get("message");
        let at = entry.get("timestamp").and_then(Value::as_str).and_then(parse_iso_ms).unwrap_or(0);
        let asking = message
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    block.get("type").and_then(Value::as_str) == Some("tool_use")
                        && block.get("name").and_then(Value::as_str).is_some_and(|name| ASKING_TOOLS.contains(&name))
                })
            });

        if asking {
            return Some(Verdict { turn: Some(Turn { kind: TurnKind::Asking, at }), cwd, title: None });
        }

        let stop = message.and_then(|m| m.get("stop_reason")).and_then(Value::as_str);

        if stop.is_some_and(|reason| reason != "tool_use") {
            return Some(Verdict { turn: Some(Turn { kind: TurnKind::Finished, at }), cwd, title: None });
        }

        return Some(Verdict { turn: None, cwd, title: None });
    }

    None
}

/// The session's latest name in these lines: one set by hand (`custom-title`) wins over Claude
/// Code's own (`ai-title`). Both are rewritten often, so the last lines have them.
pub fn read_title(lines: &[&str]) -> Option<String> {
    let mut generated: Option<String> = None;

    for line in lines.iter().rev() {
        let custom = line.contains("\"custom-title\"");
        if !custom && (generated.is_some() || !line.contains("\"ai-title\"")) {
            continue;
        }

        let Ok(entry) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let key = if custom { "customTitle" } else { "aiTitle" };
        let Some(title) = entry.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) else {
            continue;
        };

        if custom {
            return Some(title.to_string());
        }
        generated = Some(title.to_string());
    }

    generated
}

/// What Claude last said in these lines: the text blocks of the last assistant line that has
/// any, nothing of its thinking or of a tool's input. Side-chain (sub-agent) and meta entries
/// are skipped, and nothing the user wrote is read. For a session's details only
/// (`last_message`).
pub fn read_last_message(lines: &[&str]) -> Option<String> {
    for line in lines.iter().rev() {
        if !line.contains("\"assistant\"") {
            continue;
        }

        let Ok(entry) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let flag = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);

        if entry.get("type").and_then(Value::as_str) != Some("assistant") || flag("isSidechain") || flag("isMeta") {
            continue;
        }

        let Some(blocks) = entry.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) else {
            continue;
        };
        let said: Vec<&str> = blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect();

        if !said.is_empty() {
            return Some(said.join("\n\n"));
        }
    }

    None
}

/// What Claude last said in the session logged in `file` (`read_last_message`): read from the
/// file's end when asked and handed over, never kept or logged.
pub fn last_message(file: &Path) -> Option<String> {
    let size = fs::metadata(file).ok()?.len();
    scan_tail(file, size, read_last_message)
}

/// Claude Code's folder name for a project, as Claude Code makes it: every UTF-16 unit that isn't
/// an ASCII letter or digit becomes `-` (`C:\Users\me\proj` is `C--Users-me-proj`). A name
/// longer than 200 is cut there and gets the path's hash, in base 36, after a `-`.
pub fn encode_project_path(folder_path: &str) -> String {
    const MAX: usize = 200;
    let name: String = folder_path
        .encode_utf16()
        .map(|unit| char::from_u32(u32::from(unit)).filter(char::is_ascii_alphanumeric).unwrap_or('-'))
        .collect();

    if name.len() <= MAX {
        return name;
    }
    format!("{}-{}", &name[..MAX], base36(path_hash(folder_path)))
}

/// Claude Code's hash of a path: `hash * 31 + unit` over its UTF-16 units in 32 bits, made
/// positive.
fn path_hash(folder_path: &str) -> u64 {
    let hash = folder_path.encode_utf16().fold(0i32, |hash, unit| (hash << 5).wrapping_sub(hash).wrapping_add(i32::from(unit)));
    i64::from(hash).unsigned_abs()
}

/// Reads from the end of the file, growing the window until `read` finds what it looks for. A
/// single tool output line can be hundreds of KB, so a fixed tail is not enough.
fn scan_tail<T>(file: &Path, size: u64, read: impl Fn(&[&str]) -> Option<T>) -> Option<T> {
    let mut handle = File::open(file).ok()?;
    let mut span = TAIL_START;

    loop {
        let length = span.min(size);
        let mut buffer = vec![0u8; length as usize];

        handle.seek(SeekFrom::Start(size - length)).ok()?;
        handle.read_exact(&mut buffer).ok()?;

        let text = String::from_utf8_lossy(&buffer);
        let mut lines: Vec<&str> = text.split('\n').collect();

        if length < size && !lines.is_empty() {
            lines.remove(0);
        }

        let found = read(&lines);

        if found.is_some() || length >= size || span >= TAIL_MAX {
            return found;
        }

        span *= 4;
    }
}

/// What the log's last decisive line says, with the session's name from the same lines.
fn read_file_tail(file: &Path, size: u64) -> Option<Verdict> {
    scan_tail(file, size, |lines| read_tail(lines).map(|verdict| Verdict { title: read_title(lines), ..verdict }))
}

/// All that is read of an earlier day's session, from its log's last lines: its name, the folder
/// it runs in, and when it last said or was told something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogBrief {
    pub title: Option<String>,
    pub cwd: Option<String>,
    /// The time on the conversation's last line. The file's own time is no stand-in: a session
    /// only opened again writes to its log too.
    pub at: Option<u64>,
}

/// The brief of the session logged in `file`: its last line of the conversation itself
/// (side-chain and meta entries are skipped) tells the folder and the time, the same lines the
/// name. Nothing of what was said is read. `None` when no line tells.
pub fn log_brief(file: &Path, size: u64) -> Option<LogBrief> {
    scan_tail(file, size, |lines| {
        let (cwd, at) = lines.iter().rev().find_map(|line| {
            let entry = serde_json::from_str::<Value>(line.trim()).ok()?;
            let flag = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);
            if !matches!(entry.get("type").and_then(Value::as_str), Some("user") | Some("assistant")) || flag("isSidechain") || flag("isMeta") {
                return None;
            }
            let text = |key: &str| entry.get(key).and_then(Value::as_str);
            Some((text("cwd").map(str::to_string), text("timestamp").and_then(parse_iso_ms)))
        })?;
        Some(LogBrief { title: read_title(lines), cwd, at })
    })
}

/// A session log as last read.
struct Log {
    modified: SystemTime,
    modified_ms: u64,
    size: u64,
    verdict: Option<Verdict>,
}

/// A session counts as working while its log changed this recently and the turn isn't over.
pub(crate) const WORKING_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Hook events, and sessions known only from them, are dropped after a day.
const EVENT_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
/// Claude Code reports a permission prompt only once it has been open this long.
const PROMPT_NOTICE_MS: u64 = 6_000;
/// Log writes this soon after a prompt was reported are the turn catching up, not an answer.
const ANSWER_SLACK_MS: u64 = 1_500;

#[derive(Debug, Default)]
pub struct ScanResult {
    /// Projects whose newest turn ended (or that wait on a prompt) after the user last looked.
    pub waiting: HashMap<String, Turn>,
    /// Projects where Claude is busy right now.
    pub working: HashSet<String>,
    /// Every session changed lately, with its name and where it stands.
    pub sessions: Vec<SessionState>,
    /// Sessions the hook reported in folders that aren't listed (`path` empty).
    pub others: Vec<SessionState>,
}

/// One event from the hook script (see hooks.rs). Only these fields are ever written; never
/// the prompt, a message or a tool's input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HookEvent {
    /// Claude Code's event: `UserPromptSubmit`, `Stop`, `Notification`…
    event: String,
    session_id: String,
    /// The subagent it came from; none for the session's own thread.
    agent_id: Option<String>,
    cwd: Option<String>,
    transcript_path: Option<String>,
    notification_type: Option<String>,
    /// Why a session started: `startup` | `resume` | `clear` | `compact`.
    source: Option<String>,
    /// The tool a permission dialog is for, or that just ran, by name only.
    tool_name: Option<String>,
    /// When it happened: the file's modification time.
    at: u64,
}

/// A file of the first hook script: Claude Code's whole Notification input. Only these fields
/// are read.
#[derive(Debug, Deserialize)]
struct LegacyInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    notification_type: Option<String>,
}

impl HookEvent {
    /// The script's `key=value` lines; unknown keys are skipped.
    fn parse(text: &str, at: u64) -> Option<Self> {
        let mut event = HookEvent { at, ..HookEvent::default() };

        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = Some(value.to_string()).filter(|value| !value.is_empty());

            match key {
                "event" => event.event = value.unwrap_or_default(),
                "session_id" => event.session_id = value.unwrap_or_default(),
                "agent_id" => event.agent_id = value,
                "cwd" => event.cwd = value,
                "transcript_path" => event.transcript_path = value,
                "notification_type" => event.notification_type = value,
                "source" => event.source = value,
                "tool_name" => event.tool_name = value.filter(|name| is_tool_name(name)),
                _ => {}
            }
        }

        (!event.event.is_empty() && !event.session_id.is_empty()).then_some(event)
    }

    fn parse_legacy(raw: &str, at: u64) -> Option<Self> {
        let input: LegacyInput = serde_json::from_str(raw).ok()?;

        Some(HookEvent {
            event: "Notification".into(),
            session_id: input.session_id?,
            cwd: input.cwd,
            transcript_path: input.transcript_path,
            notification_type: input.notification_type,
            at,
            ..HookEvent::default()
        })
    }
}

/// A tool's name as Claude Code gives it (`Bash`, `mcp__github__create_issue`); anything else is
/// dropped, so nothing but a name ever reaches the screen.
fn is_tool_name(name: &str) -> bool {
    name.len() <= 128 && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
}

/// Where a session stands by its hook events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookPhase {
    /// Started, or back at the prompt; nothing to wait on.
    Idle,
    Working,
    /// Its turn ended, or it waits on a permission prompt or a question.
    Waiting(TurnKind),
    /// Nothing decisive heard yet, or a prompt answered without an event saying so (a denied
    /// tool ends the turn silently): the log decides until the next event.
    Log,
    Ended,
}

struct HookSession {
    /// Where it was first seen running.
    cwd: Option<String>,
    transcript: Option<PathBuf>,
    phase: HookPhase,
    /// When the phase began.
    since: u64,
    /// The latest event.
    last: u64,
    /// Each thread's latest finished tool call: `""` for the session's own, else the subagent.
    threads: HashMap<String, u64>,
    /// Threads that kept running while the open prompt waited: their tool calls don't answer it.
    busy: HashSet<String>,
    /// The thread whose permission dialog is open, when `PermissionRequest` said so: only its
    /// tool call answers it.
    asker: Option<String>,
    /// The open permission dialog's tool.
    tool: Option<String>,
}

impl HookSession {
    fn new(at: u64) -> Self {
        Self {
            cwd: None,
            transcript: None,
            phase: HookPhase::Log,
            since: at,
            last: at,
            threads: HashMap::new(),
            busy: HashSet::new(),
            asker: None,
            tool: None,
        }
    }

    /// Subagents that ran tools in the current turn.
    fn subagents(&self) -> u32 {
        self.threads.keys().filter(|thread| !thread.is_empty()).count() as u32
    }

    fn apply(&mut self, event: &HookEvent) {
        if self.cwd.is_none() {
            self.cwd = event.cwd.clone();
        }
        if let Some(path) = &event.transcript_path {
            self.transcript = Some(PathBuf::from(path));
        }
        self.last = self.last.max(event.at);

        let thread = event.agent_id.clone().unwrap_or_default();
        let prompt_open = matches!(self.phase, HookPhase::Waiting(TurnKind::Permission | TurnKind::Asking));

        let next = match event.event.as_str() {
            "SessionStart" if event.source.as_deref() == Some("compact") => None,
            "SessionStart" => Some(HookPhase::Idle),
            "UserPromptSubmit" => Some(HookPhase::Working),
            // Installed only for the tools that ask: AskUserQuestion, ExitPlanMode.
            "PreToolUse" => Some(HookPhase::Waiting(TurnKind::Asking)),
            // The dialog opened this moment, for this thread.
            "PermissionRequest" => {
                self.asker = Some(thread.clone());
                self.tool = event.tool_name.clone();
                Some(HookPhase::Waiting(TurnKind::Permission))
            }
            "PostToolUse" => {
                self.threads.insert(thread.clone(), event.at);
                let asker = self.asker.as_ref().filter(|_| self.phase == HookPhase::Waiting(TurnKind::Permission));
                match self.phase {
                    // The thread that asked ran its tool: answered. Another thread's tool call,
                    // or another tool the same message ran beside it, ran beside the open dialog.
                    _ if asker.is_some() => {
                        let same_tool = event.tool_name.is_none() || self.tool.is_none() || event.tool_name == self.tool;
                        (asker == Some(&thread) && same_tool).then_some(HookPhase::Working)
                    }
                    // A finished tool call answers the prompt, unless its thread kept running
                    // while the prompt was open (a parallel subagent).
                    _ if prompt_open => (!self.busy.contains(&thread)).then_some(HookPhase::Working),
                    // Another tool's Stop hook can keep the turn going; a subagent left running
                    // in the background doesn't.
                    HookPhase::Waiting(TurnKind::Finished) => thread.is_empty().then_some(HookPhase::Working),
                    HookPhase::Ended => None,
                    _ => Some(HookPhase::Working),
                }
            }
            "Notification" => match event.notification_type.as_deref() {
                // The dialog `PermissionRequest` already reported, 6 s on.
                Some("permission_prompt") if self.asker.is_some() && self.phase == HookPhase::Waiting(TurnKind::Permission) => None,
                Some("permission_prompt") => {
                    // The waiting thread can't finish a tool call while its prompt is open.
                    let opened = event.at.saturating_sub(PROMPT_NOTICE_MS);
                    self.busy = self.threads.iter().filter(|(_, at)| **at > opened).map(|(thread, _)| thread.clone()).collect();
                    Some(HookPhase::Waiting(TurnKind::Permission))
                }
                Some("elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input") => Some(HookPhase::Waiting(TurnKind::Asking)),
                Some("elicitation_complete" | "elicitation_response") if prompt_open => Some(HookPhase::Working),
                _ => None,
            },
            "Stop" => Some(HookPhase::Waiting(TurnKind::Finished)),
            "SessionEnd" => Some(HookPhase::Ended),
            _ => None,
        };

        if let Some(phase) = next {
            if phase != HookPhase::Waiting(TurnKind::Permission) {
                self.busy.clear();
                self.asker = None;
                self.tool = None;
            } else if event.event != "PermissionRequest" {
                // Reported by `Notification`: which thread asked, and for what, is unknown.
                self.asker = None;
                self.tool = None;
            }
            if matches!(phase, HookPhase::Idle | HookPhase::Ended | HookPhase::Waiting(TurnKind::Finished)) {
                self.threads.clear();
            }
            self.phase = phase;
            self.since = event.at;
        }
    }

    /// Its phase and the turn it waits with, given when its log was last written; `None` when
    /// the log decides. A prompt counts as answered once the log is written again.
    fn state(&mut self, logged: u64, baseline: u64, recent: u64) -> Option<(SessionPhase, Option<Turn>)> {
        let alive = if self.last.max(logged) > recent { SessionPhase::Working } else { SessionPhase::Idle };

        match self.phase {
            HookPhase::Log => None,
            HookPhase::Idle | HookPhase::Ended => Some((SessionPhase::Idle, None)),
            HookPhase::Working => Some((alive, None)),
            HookPhase::Waiting(TurnKind::Finished) if self.since > baseline => {
                Some((SessionPhase::Waiting, Some(Turn { kind: TurnKind::Finished, at: self.since })))
            }
            HookPhase::Waiting(TurnKind::Finished) => Some((SessionPhase::Idle, None)),
            HookPhase::Waiting(kind) => {
                let written = self.transcript.as_deref().and_then(modified_ms).unwrap_or(0).max(logged);

                if written > self.since + ANSWER_SLACK_MS {
                    self.phase = HookPhase::Log;
                    self.busy.clear();
                    self.asker = None;
                    self.tool = None;
                    return None;
                }

                if self.since > baseline {
                    Some((SessionPhase::Waiting, Some(Turn { kind, at: self.since })))
                } else {
                    Some((alive, None))
                }
            }
        }
    }
}

fn system_ms(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn modified_ms(path: &Path) -> Option<u64> {
    Some(system_ms(fs::metadata(path).ok()?.modified().ok()?))
}

/// `cwd` is the project's folder or under it; on Windows in any letter case and with either
/// separator (see `path_key`).
pub(crate) fn inside(cwd: &str, folder_path: &str) -> bool {
    let cwd = path_key(cwd);
    let folder = path_key(folder_path);
    *cwd == *folder || cwd.starts_with(&format!("{folder}{MAIN_SEPARATOR}"))
}

/// Whether a folder under `~/.claude/projects` holds a project's sessions: `Some(true)` for its
/// own folder, `Some(false)` for a subfolder's (`<project>-sub`, which a neighbour such as
/// `pitwall-docs` also looks like; the session's cwd tells them apart).
fn dir_match(dir: &str, encoded: &str) -> Option<bool> {
    let name = path_key(dir);

    if *name == *encoded {
        Some(true)
    } else if name.starts_with(&format!("{encoded}-")) {
        Some(false)
    } else {
        None
    }
}

/// A session decided by its log's last lines.
fn log_state(verdict: &Verdict, modified: u64, baseline: u64, recent: u64) -> (SessionPhase, Option<Turn>) {
    let phase = match verdict.turn {
        Some(turn) if turn.at > baseline => SessionPhase::Waiting,
        None if modified > recent => SessionPhase::Working,
        _ => SessionPhase::Idle,
    };

    (phase, verdict.turn.filter(|_| phase == SessionPhase::Waiting))
}

pub struct ClaudeWatch {
    root: PathBuf,
    seen_file: PathBuf,
    /// Seen books of older extension versions. VS Code windows not reloaded since the move
    /// to `~/.pitwall/` still record "I looked" there; read, never written.
    legacy_seen: Vec<PathBuf>,
    events_dir: PathBuf,
    /// Session logs read so far: by folder under `root`, then by session id.
    logs: HashMap<String, HashMap<String, Log>>,
    /// Sessions the hook reported, by session id.
    hooked: HashMap<String, HookSession>,
    /// Logs of sessions outside the listed projects, read only for their state and name when
    /// asked (`peek`): by session id, with when they were last asked for.
    extra: HashMap<String, (u64, Log)>,
    /// The projects listed now.
    listed: Vec<String>,
    /// A full scan ran; nothing is known before.
    ready: bool,
}

impl ClaudeWatch {
    pub fn new(claude_projects: PathBuf, registry_dir: &Path) -> Self {
        Self {
            root: claude_projects,
            seen_file: registry_dir.join("claude-seen.json"),
            legacy_seen: Vec::new(),
            events_dir: registry_dir.join("claude-events"),
            logs: HashMap::new(),
            hooked: HashMap::new(),
            extra: HashMap::new(),
            listed: Vec::new(),
            ready: false,
        }
    }

    pub fn with_legacy_seen(mut self, files: Vec<PathBuf>) -> Self {
        self.legacy_seen = files;
        self
    }

    /// The projects folder of Claude Code (`~/.claude/projects`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the hook drops its events.
    pub fn events_dir(&self) -> &Path {
        &self.events_dir
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn listed(&self) -> &[String] {
        &self.listed
    }

    /// The projects to follow; true when the list changed.
    pub fn set_listed(&mut self, paths: &[String]) -> bool {
        if self.listed == paths {
            return false;
        }
        self.listed = paths.to_vec();
        true
    }

    /// The shared book plus every "I looked" from the legacy books (latest wins). The start
    /// time stays the shared one, so older legacy books can't bring back ancient turns.
    fn read_seen_everywhere(&self) -> SeenBook {
        let mut book = self.read_seen();

        for file in &self.legacy_seen {
            let Some(legacy) = fs::read_to_string(file).ok().and_then(|raw| serde_json::from_str::<SeenBook>(&raw).ok()) else {
                continue;
            };

            for (path, at) in legacy.paths {
                let slot = book.paths.entry(path).or_insert(0);
                *slot = (*slot).max(at);
            }
        }

        book
    }

    fn read_seen_file(&self) -> Option<SeenBook> {
        serde_json::from_str(&fs::read_to_string(&self.seen_file).ok()?).ok()
    }

    fn read_seen(&self) -> SeenBook {
        if let Some(book) = self.read_seen_file() {
            return book;
        }

        let fresh = SeenBook { since: now_ms(), paths: HashMap::new() };
        self.write_seen(fresh.clone());
        fresh
    }

    /// Merged with what is on disk, so a write from the extension in between isn't lost.
    fn write_seen(&self, book: SeenBook) {
        let merged = merge_seen(self.read_seen_file(), book);

        if let Ok(json) = serde_json::to_string(&merged) {
            let _ = write_atomic(&self.seen_file, &json);
        }
    }

    pub fn mark_seen(&self, folder_path: &str) {
        let mut paths = HashMap::new();
        paths.insert(folder_path.to_string(), now_ms());
        self.write_seen(SeenBook { since: self.read_seen().since, paths });
    }

    /// A full pass: every log of the listed projects, the hook's events, then the verdict.
    #[cfg(test)]
    pub fn scan(&mut self, folder_paths: &[String]) -> ScanResult {
        self.set_listed(folder_paths);
        self.rescan();
        self.apply_events();
        self.evaluate()
    }

    /// Reads the session logs of the listed projects changed since their turns were last seen
    /// (or lately), and forgets those of projects no longer listed.
    pub fn rescan(&mut self) {
        let book = self.read_seen_everywhere();
        let recent = now_ms().saturating_sub(WORKING_WINDOW_MS);
        let dirs: Vec<String> = fs::read_dir(&self.root)
            .map(|entries| entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        let mut wanted: HashMap<String, u64> = HashMap::new();

        for folder_path in &self.listed {
            let since = book.seen(folder_path).unwrap_or(book.since).min(recent);
            let encoded = path_key(&encode_project_path(folder_path)).into_owned();

            for dir in dirs.iter().filter(|dir| dir_match(dir, &encoded).is_some()) {
                let slot = wanted.entry(dir.clone()).or_insert(since);
                *slot = (*slot).min(since);
            }
        }

        self.logs.retain(|dir, _| wanted.contains_key(dir));
        for (dir, since) in wanted {
            self.rescan_dir(&dir, since);
        }
        self.ready = true;
    }

    fn rescan_dir(&mut self, dir: &str, since: u64) {
        let Ok(entries) = fs::read_dir(self.root.join(dir)) else {
            self.logs.remove(dir);
            return;
        };

        let mut present = HashSet::new();

        for entry in entries.flatten() {
            let file = entry.path();

            if file.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }

            let Some(id) = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };

            present.insert(id.clone());

            if meta.modified().map(system_ms).unwrap_or(0) > since {
                self.read_log(dir, id, &file, &meta);
            }
        }

        if let Some(logs) = self.logs.get_mut(dir) {
            logs.retain(|id, _| present.contains(id));
        }
    }

    /// The log's last lines, unless it is unchanged since they were read.
    fn read_log(&mut self, dir: &str, id: String, file: &Path, meta: &fs::Metadata) {
        let Ok(modified) = meta.modified() else {
            return;
        };
        let logs = self.logs.entry(dir.to_string()).or_default();

        if logs.get(&id).is_some_and(|log| log.modified == modified && log.size == meta.len()) {
            return;
        }

        let verdict = read_file_tail(file, meta.len());
        logs.insert(id, Log { modified, modified_ms: system_ms(modified), size: meta.len(), verdict });
    }

    /// One session log changed (`<folder>/<session>.jsonl` under the projects folder).
    pub fn refresh_log(&mut self, dir: &str, file_name: &str) {
        let Some(id) = file_name.strip_suffix(".jsonl") else {
            return;
        };
        if !self.listed.iter().any(|folder| dir_match(dir, &path_key(&encode_project_path(folder))).is_some()) {
            return;
        }

        let file = self.root.join(dir).join(file_name);

        match fs::metadata(&file) {
            Ok(meta) => self.read_log(dir, id.to_string(), &file, &meta),
            Err(_) => {
                if let Some(logs) = self.logs.get_mut(dir) {
                    logs.remove(id);
                }
            }
        }
    }

    /// Applies the hook's event files in the order they were written, then deletes them; one
    /// older than a day is only deleted. Returns the listed projects where a turn just ended.
    pub fn apply_events(&mut self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.events_dir) else {
            return Vec::new();
        };

        let now = now_ms();
        let mut files: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "event" || ext == "json"))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        files.sort();

        let mut ended = Vec::new();

        for (modified, file) in files {
            let at = system_ms(modified);
            let raw = fs::read_to_string(&file).unwrap_or_default();
            let _ = fs::remove_file(&file);

            if now.saturating_sub(at) >= EVENT_MAX_AGE_MS {
                continue;
            }

            let legacy = file.extension().is_some_and(|ext| ext == "json");
            let Some(event) = (if legacy { HookEvent::parse_legacy(&raw, at) } else { HookEvent::parse(&raw, at) }) else {
                continue;
            };

            let session = self.hooked.entry(event.session_id.clone()).or_insert_with(|| HookSession::new(at));
            session.apply(&event);

            if event.event == "Stop" {
                if let Some(cwd) = session.cwd.as_deref() {
                    ended.extend(self.listed.iter().filter(|folder| inside(cwd, folder)).cloned());
                }
            }
        }

        ended.sort();
        ended.dedup();
        ended
    }

    /// Where every session of the listed projects stands, from what was read so far: the
    /// hook's events where it reported, else the log's last lines; and the sessions the hook
    /// reported elsewhere. Only the seen book is read.
    pub fn evaluate(&mut self) -> ScanResult {
        let book = self.read_seen_everywhere();
        let now = now_ms();
        let recent = now.saturating_sub(WORKING_WINDOW_MS);
        let mut result = ScanResult::default();

        self.hooked.retain(|_, session| now.saturating_sub(session.last) < EVENT_MAX_AGE_MS);
        self.extra.retain(|_, (asked, _)| now.saturating_sub(*asked) < EXTRA_MAX_AGE_MS);
        let Self { logs, hooked, listed, extra, .. } = self;

        for folder_path in listed.iter() {
            let baseline = book.seen(folder_path).unwrap_or(book.since);
            let since = baseline.min(recent);
            let encoded = path_key(&encode_project_path(folder_path)).into_owned();
            let mut states: Vec<SessionState> = Vec::new();

            for (dir, sessions) in logs.iter() {
                let Some(exact) = dir_match(dir, &encoded) else {
                    continue;
                };

                for (id, log) in sessions {
                    if !exact {
                        let cwd = log.verdict.as_ref().and_then(|v| v.cwd.clone()).or_else(|| hooked.get(id).and_then(|h| h.cwd.clone()));
                        if !cwd.is_some_and(|cwd| inside(&cwd, folder_path)) {
                            continue;
                        }
                    }

                    if hooked.get(id).map_or(0, |h| h.last).max(log.modified_ms) <= since {
                        continue;
                    }

                    let (phase, turn) = match hooked.get_mut(id).and_then(|h| h.state(log.modified_ms, baseline, recent)) {
                        Some(state) => state,
                        None => match &log.verdict {
                            Some(verdict) if log.modified_ms > since => log_state(verdict, log.modified_ms, baseline, recent),
                            _ => continue,
                        },
                    };
                    states.push(session_state(id, folder_path, Some(log), hooked.get(id), phase, turn));
                }
            }

            // Sessions the hook reported whose log hasn't been read: not written yet, or older.
            for (id, session) in hooked.iter_mut() {
                if session.last <= since || states.iter().any(|state| &state.id == id) {
                    continue;
                }

                let log_dir = log_folder(session);
                let by_cwd = session.cwd.as_deref().is_some_and(|cwd| inside(cwd, folder_path));
                if !by_cwd && log_dir.as_deref().is_none_or(|dir| dir_match(dir, &encoded) != Some(true)) {
                    continue;
                }

                let log = log_dir.as_deref().and_then(|dir| logs.get(dir)).and_then(|sessions| sessions.get(id));

                if let Some((phase, turn)) = session.state(log.map_or(0, |log| log.modified_ms), baseline, recent) {
                    states.push(session_state(id, folder_path, log, Some(&*session), phase, turn));
                }
            }

            // A stable order, so an unchanged state never looks changed.
            states.sort_by(|a, b| a.id.cmp(&b.id));

            let newest = states.iter().filter(|s| s.phase == SessionPhase::Waiting).filter_map(|s| s.turn).max_by_key(|turn| turn.at);
            match newest {
                Some(turn) => {
                    result.waiting.insert(folder_path.clone(), turn);
                }
                None if states.iter().any(|s| s.phase == SessionPhase::Working) => {
                    result.working.insert(folder_path.clone());
                }
                None => {}
            }
            result.sessions.extend(states);
        }

        // Sessions in folders that aren't listed: the hook tells where they run; their own log,
        // as last asked for (`peek`, by the Sessions page), what the hook leaves open. Only its
        // modification time is read here.
        for (id, session) in hooked.iter_mut() {
            let Some(cwd) = session.cwd.clone() else {
                continue;
            };
            let log_dir = log_folder(session);
            let listed_here = listed.iter().any(|folder| {
                inside(&cwd, folder) || log_dir.as_deref().is_some_and(|dir| dir_match(dir, &path_key(&encode_project_path(folder))) == Some(true))
            });
            if listed_here || result.sessions.iter().any(|state| &state.id == id) {
                continue;
            }

            let baseline = book.seen(&cwd).unwrap_or(book.since);
            let log = extra.get(id).map(|(_, log)| log);
            let logged = session.transcript.as_deref().and_then(modified_ms).unwrap_or(0);
            let (phase, turn) = match session.state(logged, baseline, recent) {
                Some(state) => state,
                None => match log.and_then(|log| log.verdict.as_ref()) {
                    Some(verdict) => log_state(verdict, logged, baseline, recent),
                    None => (SessionPhase::Idle, None),
                },
            };
            result.others.push(session_state(id, "", log, Some(&*session), phase, turn));
        }
        result.others.sort_by(|a, b| a.id.cmp(&b.id));

        result
    }

    /// A session's log as last read, for its name and when it was last written: a listed
    /// project's from the scan, else its own file, read again only once it changed. That file is
    /// the one the hook named (under the projects folder only), else
    /// `<projects>/<folder, encoded>/<id>.jsonl`. `id` must be a session id (a UUID); it becomes
    /// a file name.
    pub fn peek(&mut self, id: &str, folder: &str) -> Option<Peek> {
        if let Some(log) = self.logs.values().find_map(|sessions| sessions.get(id)) {
            return Some(Peek::of(log));
        }

        let named = self.hooked.get(id).and_then(|session| session.transcript.clone());
        let file = named
            .filter(|file| {
                let plain = file.components().all(|part| !matches!(part, std::path::Component::ParentDir));
                plain && file.starts_with(&self.root) && file.extension().is_some_and(|ext| ext == "jsonl")
            })
            .unwrap_or_else(|| self.root.join(encode_project_path(folder)).join(format!("{id}.jsonl")));
        peek_file(&mut self.extra, id, &file, now_ms()).map(Peek::of)
    }
}

/// What `peek` tells of a session's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peek {
    pub title: Option<String>,
    /// When the log was last written.
    pub modified: u64,
}

impl Peek {
    fn of(log: &Log) -> Self {
        Self { title: log.verdict.as_ref().and_then(|v| v.title.clone()), modified: log.modified_ms }
    }
}

/// Logs of other sessions are forgotten once nobody asked for them this long.
const EXTRA_MAX_AGE_MS: u64 = 5 * 60 * 1000;

/// The folder under the projects folder a hooked session logs into, from its transcript path.
fn log_folder(session: &HookSession) -> Option<String> {
    session.transcript.as_deref().and_then(Path::parent).and_then(Path::file_name).map(|dir| dir.to_string_lossy().into_owned())
}

/// A log outside the scan, read again only when it changed.
fn peek_file<'a>(extra: &'a mut HashMap<String, (u64, Log)>, id: &str, file: &Path, now: u64) -> Option<&'a Log> {
    let Ok(meta) = fs::metadata(file) else {
        extra.remove(id);
        return None;
    };
    let modified = meta.modified().ok()?;
    let fresh = extra.get(id).is_some_and(|(_, log)| log.modified == modified && log.size == meta.len());

    if !fresh {
        let verdict = read_file_tail(file, meta.len());
        extra.insert(id.to_string(), (now, Log { modified, modified_ms: system_ms(modified), size: meta.len(), verdict }));
    }

    let entry = extra.get_mut(id)?;
    entry.0 = now;
    Some(&entry.1)
}

/// A session as the scan reports it, from its log and its hook events.
fn session_state(id: &str, path: &str, log: Option<&Log>, hook: Option<&HookSession>, phase: SessionPhase, turn: Option<Turn>) -> SessionState {
    let verdict = log.and_then(|log| log.verdict.as_ref());
    let decided = hook.filter(|hook| hook.phase != HookPhase::Log);
    let permission = turn.is_some_and(|turn| turn.kind == TurnKind::Permission);

    SessionState {
        id: id.to_string(),
        path: path.to_string(),
        folder: verdict.and_then(|v| v.cwd.clone()).or_else(|| hook.and_then(|h| h.cwd.clone())),
        title: verdict.and_then(|v| v.title.clone()),
        phase,
        turn,
        since: decided.map(|hook| hook.since).or(turn.map(|turn| turn.at)),
        hooked: decided.is_some(),
        ended: hook.is_some_and(|hook| hook.phase == HookPhase::Ended),
        tool: hook.filter(|_| permission).and_then(|hook| hook.tool.clone()),
        subagents: hook.map_or(0, HookSession::subagents),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_state_from_another_writer_is_never_lost() {
        let disk = SeenBook { since: 100, paths: HashMap::from([("/a".into(), 500), ("/b".into(), 300)]) };
        let mine = SeenBook { since: 200, paths: HashMap::from([("/a".into(), 400), ("/c".into(), 600)]) };
        let merged = merge_seen(Some(disk), mine);

        assert_eq!(merged.since, 100);
        assert_eq!(merged.paths, HashMap::from([("/a".into(), 500), ("/b".into(), 300), ("/c".into(), 600)]));
    }

    /// The folder Claude Code keeps a project's logs in, as its own code names it (expected
    /// values from Claude Code 2.1.287's function): a wrong name loses every session silently.
    #[test]
    fn project_folders_are_named_as_claude_code_names_them() {
        assert_eq!(encode_project_path(r"C:\Users\me\proj"), "C--Users-me-proj");
        assert_eq!(encode_project_path("/Users/me/🚀 app"), "-Users-me----app");

        let long = format!(r"C:\Users\me\{}proj", "çok-uzun-klasör-".repeat(14));
        let expected = format!("C--Users-me-{}-ok-uzun-kla-yvlsze", "-ok-uzun-klas-r-".repeat(11));
        assert_eq!(encode_project_path(&long), expected);
    }

    #[test]
    fn a_look_recorded_by_an_old_extension_window_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let registry = tmp.path().join("registry");
        let legacy = tmp.path().join("old-claude-seen.json");
        let project = "/Users/me/projects/paddock";
        let session_dir = claude.join(encode_project_path(project));

        fs::create_dir_all(&session_dir).unwrap();
        fs::create_dir_all(&registry).unwrap();
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        fs::write(
            session_dir.join("s.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-09-24T17:33:19.947Z\",\"message\":{\"stop_reason\":\"end_turn\"}}\n",
        )
        .unwrap();
        // An old window looked after the turn ended; its book still says since=0 elsewhere.
        fs::write(&legacy, format!(r#"{{"since":0,"paths":{{"{project}":1790271299947}}}}"#)).unwrap();

        let mut watch = ClaudeWatch::new(claude.clone(), &registry);
        assert_eq!(watch.scan(&[project.to_string()]).waiting.len(), 1, "without the legacy book it waits");

        let mut watch = ClaudeWatch::new(claude, &registry).with_legacy_seen(vec![legacy, tmp.path().join("missing.json")]);
        assert!(watch.scan(&[project.to_string()]).waiting.is_empty());
        assert!(!fs::read_to_string(registry.join("claude-seen.json")).unwrap().contains("paddock"), "legacy looks are read, not copied");
    }

    #[test]
    fn tail_verdicts() {
        let finished = r#"{"type":"assistant","timestamp":"2026-09-24T17:33:19.947Z","cwd":"/p","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"x"}]}}"#;
        let asking = r#"{"type":"assistant","timestamp":"2026-09-24T17:33:19.000Z","message":{"stop_reason":"tool_use","content":[{"type":"tool_use","name":"AskUserQuestion"}]}}"#;
        let working = r#"{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"tool_use","name":"Bash"}]}}"#;
        let sidechain = r#"{"type":"assistant","isSidechain":true,"message":{"stop_reason":"end_turn"}}"#;
        let noise = r#"{"type":"cost-state"}"#;

        let verdict = read_tail(&[finished, sidechain, noise]).unwrap();
        assert_eq!(verdict.turn, Some(Turn { kind: TurnKind::Finished, at: 1_790_271_199_947 }));
        assert_eq!(verdict.cwd.as_deref(), Some("/p"));
        assert_eq!(read_tail(&[asking]).unwrap().turn.unwrap().kind, TurnKind::Asking);
        assert_eq!(read_tail(&[finished, working]).unwrap().turn, None);
        assert_eq!(read_tail(&[noise]), None);
    }

    /// A session's details are the one place message text is read: only what Claude last said,
    /// never the user's words, Claude's thinking, a tool's input or a sub-agent's lines.
    #[test]
    fn the_last_message_is_only_what_claude_said() {
        let prompt = r#"{"type":"user","message":{"role":"user","content":"my secret prompt"}}"#;
        let earlier = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Looking into it."}]}}"#;
        let tool = r#"{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"thinking","thinking":"private thought"},{"type":"tool_use","name":"Bash","input":{"command":"cat .env"}}]}}"#;
        let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"\"type\":\"assistant\" TOKEN=abc"}]}}"#;
        let answer = r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"thinking","thinking":"private thought"},{"type":"text","text":" Moved the port check. "},{"type":"text","text":"Tests pass."}]}}"#;
        let sidechain = r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"sub-agent report"}]}}"#;
        let meta = r#"{"type":"assistant","isMeta":true,"message":{"content":[{"type":"text","text":"meta line"}]}}"#;

        assert_eq!(read_last_message(&[prompt, earlier, tool, result, answer, sidechain, meta]).as_deref(), Some("Moved the port check.\n\nTests pass."));
        // Still at work: the last thing said, nothing of the tool call after it.
        assert_eq!(read_last_message(&[prompt, earlier, tool, result]).as_deref(), Some("Looking into it."));
        assert_eq!(read_last_message(&[prompt, tool, result, sidechain, meta]), None);
    }

    #[test]
    fn hook_events_drive_the_session_and_a_running_subagent_never_answers_a_prompt() {
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let registry = tmp.path().join("registry");
        let events = registry.join("claude-events");
        let project = "/Users/me/projects/paddock";
        let session_dir = claude.join(encode_project_path(project));
        let transcript = session_dir.join("s1.jsonl");
        let base = SystemTime::now() - Duration::from_secs(60);
        let at = |seconds: u64| base + Duration::from_secs(seconds);

        fs::create_dir_all(&session_dir).unwrap();
        fs::create_dir_all(&events).unwrap();
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        // Its log, last written before all of this, ends with an older finished turn.
        fs::write(&transcript, "{\"type\":\"assistant\",\"timestamp\":\"2026-09-24T17:33:19.947Z\",\"message\":{\"stop_reason\":\"end_turn\"}}\n").unwrap();
        File::options().write(true).open(&transcript).unwrap().set_modified(base - Duration::from_secs(60)).unwrap();

        let mut watch = ClaudeWatch::new(claude, &registry);
        let paths = vec![project.to_string()];
        let common = format!("session_id=s1\ncwd={project}/app\ntranscript_path={}\n", transcript.display());
        let mut step = |name: &str, seconds: u64, lines: String| {
            let file = events.join(format!("{name}.event"));
            fs::write(&file, lines).unwrap();
            File::options().write(true).open(&file).unwrap().set_modified(at(seconds)).unwrap();
            let result = watch.scan(&paths);
            let session = result.sessions.iter().find(|s| s.id == "s1").expect("the session is listed");
            (session.phase, session.turn.map(|turn| turn.kind), result)
        };

        let (phase, _, result) = step("1", 0, format!("event=UserPromptSubmit\n{common}"));
        assert_eq!(phase, SessionPhase::Working);
        assert!(result.working.contains(project) && result.waiting.is_empty());

        // A subagent runs tools; a prompt opens (reported 6 s later) for another thread.
        step("2", 3, "event=PostToolUse\nsession_id=s1\nagent_id=a1\n".into());
        let (phase, kind, result) = step("3", 8, format!("event=Notification\nnotification_type=permission_prompt\n{common}"));
        assert_eq!((phase, kind), (SessionPhase::Waiting, Some(TurnKind::Permission)));
        assert_eq!(result.waiting[project].kind, TurnKind::Permission);
        assert!(!result.working.contains(project));

        let (_, kind, _) = step("4", 9, "event=PostToolUse\nsession_id=s1\nagent_id=a1\n".into());
        assert_eq!(kind, Some(TurnKind::Permission), "the subagent that kept running didn't answer it");

        let (phase, kind, _) = step("5", 12, "event=PostToolUse\nsession_id=s1\n".into());
        assert_eq!((phase, kind), (SessionPhase::Working, None), "the waiting thread's tool ran: answered");

        let (_, kind, _) = step("6", 14, format!("event=PreToolUse\n{common}"));
        assert_eq!(kind, Some(TurnKind::Asking));
        let (phase, _, _) = step("7", 16, "event=PostToolUse\nsession_id=s1\n".into());
        assert_eq!(phase, SessionPhase::Working);

        let (phase, _, result) = step("8", 20, format!("event=Stop\n{common}"));
        assert_eq!(phase, SessionPhase::Waiting);
        assert_eq!(result.waiting[project], Turn { kind: TurnKind::Finished, at: system_ms(at(20)) });

        let (phase, _, result) = step("9", 25, "event=SessionEnd\nsession_id=s1\n".into());
        assert_eq!(phase, SessionPhase::Idle, "gone, and its older turn in the log doesn't come back");
        assert!(result.waiting.is_empty() && result.working.is_empty());
        assert_eq!(fs::read_dir(&events).unwrap().count(), 0, "applied events are deleted");
    }

    /// A permission dialog waits from the moment it opens, with its tool's name; only the
    /// asking thread's tool call answers it, and the late `Notification` for it changes nothing.
    #[test]
    fn a_permission_request_waits_at_once_until_its_tool_runs() {
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let registry = tmp.path().join("registry");
        let events = registry.join("claude-events");
        let project = "/Users/me/projects/paddock";
        let transcript = claude.join(encode_project_path(project)).join("s1.jsonl");
        let base = SystemTime::now() - Duration::from_secs(60);
        let at = |seconds: u64| base + Duration::from_secs(seconds);

        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::create_dir_all(&events).unwrap();
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        fs::write(&transcript, "{}\n").unwrap();
        File::options().write(true).open(&transcript).unwrap().set_modified(base).unwrap();

        let mut watch = ClaudeWatch::new(claude, &registry);
        let paths = vec![project.to_string()];
        let common = format!("session_id=s1\ncwd={project}\ntranscript_path={}\n", transcript.display());
        let mut step = |name: &str, seconds: u64, lines: String| {
            let file = events.join(format!("{name}.event"));
            fs::write(&file, lines).unwrap();
            File::options().write(true).open(&file).unwrap().set_modified(at(seconds)).unwrap();
            let result = watch.scan(&paths);
            result.sessions.into_iter().find(|s| s.id == "s1").expect("the session is listed")
        };

        let session = step("1", 1, format!("event=UserPromptSubmit\n{common}"));
        assert_eq!((session.phase, session.since), (SessionPhase::Working, Some(system_ms(at(1)))));

        let session = step("2", 5, format!("event=PermissionRequest\ntool_name=Bash\n{common}"));
        assert_eq!(session.phase, SessionPhase::Waiting);
        assert_eq!(session.turn, Some(Turn { kind: TurnKind::Permission, at: system_ms(at(5)) }), "no 6 s wait for the notification");
        assert_eq!(session.tool.as_deref(), Some("Bash"));

        // A subagent's tool call, and another tool of the same message, ran beside the dialog;
        // the notification comes 6 s on.
        let session = step("3", 7, "event=PostToolUse\nsession_id=s1\nagent_id=a1\ntool_name=Bash\n".into());
        assert_eq!((session.phase, session.tool.as_deref()), (SessionPhase::Waiting, Some("Bash")));
        let session = step("3b", 8, "event=PostToolUse\nsession_id=s1\ntool_name=Read\n".into());
        assert_eq!((session.phase, session.tool.as_deref()), (SessionPhase::Waiting, Some("Bash")));
        let session = step("4", 11, format!("event=Notification\nnotification_type=permission_prompt\n{common}"));
        assert_eq!(session.turn.map(|turn| turn.at), Some(system_ms(at(5))), "still waiting since the dialog opened");
        assert_eq!(session.tool.as_deref(), Some("Bash"));
        assert_eq!(session.subagents, 1);

        // Allowed: the tool ran.
        let session = step("5", 14, "event=PostToolUse\nsession_id=s1\ntool_name=Bash\n".into());
        assert_eq!((session.phase, session.turn, session.tool), (SessionPhase::Working, None, None));

        let session = step("6", 20, format!("event=Stop\n{common}"));
        assert_eq!(session.turn, Some(Turn { kind: TurnKind::Finished, at: system_ms(at(20)) }));
        assert_eq!((session.tool, session.subagents), (None, 0));

        // A name that isn't a tool's never gets through.
        let session = step("7", 25, format!("event=PermissionRequest\ntool_name=rm -rf /\n{common}"));
        assert_eq!((session.turn.map(|turn| turn.kind), session.tool), (Some(TurnKind::Permission), None));
    }

    #[test]
    fn a_finished_turn_shows_until_marked_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let registry = tmp.path().join("registry");
        let project = "/Users/me/projects/paddock";
        let session_dir = claude.join(encode_project_path(project));

        fs::create_dir_all(&session_dir).unwrap();
        fs::create_dir_all(&registry).unwrap();
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        fs::write(
            session_dir.join("s.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-09-24T17:33:19.947Z\",\"message\":{\"stop_reason\":\"end_turn\"}}\n",
        )
        .unwrap();

        let mut watch = ClaudeWatch::new(claude, &registry);
        let paths = vec![project.to_string(), "/Users/me/projects/paddock-docs".to_string()];

        let pending = watch.scan(&paths).waiting;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[project].kind, TurnKind::Finished);

        watch.mark_seen(project);
        assert!(watch.scan(&paths).waiting.is_empty());
    }
}
