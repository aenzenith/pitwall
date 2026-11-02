//! Codex's sessions, as `claude.rs` follows Claude Code's: its rollouts
//! (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`: today's and yesterday's folders, and the
//! older one a session the hook tells of writes into again), the events Pitwall's hook drops into
//! `~/.pitwall/codex-events/` (`codex_hooks.rs`) and whether a session's process runs (the core's
//! `codex_running`). Of a rollout only the fields this file declares are read: of its first line
//! (`session_meta`) the session's id, folder, source and start, of its last lines a turn's start
//! and end (`event_msg` `task_started`, `task_complete`, `turn_aborted`), each line's kind and
//! time. Every other value (`response_item`, `compacted`, `base_instructions`, a prompt, a message,
//! reasoning, a tool's input or output) is skipped as it streams past, never held. What Codex last
//! said (`last_message`) is read only when asked, never kept or logged; the sessions' names come
//! from `session_index.jsonl`, names only.
//!
//! Where a session stands is decided in this order (`docs/CODEX.md`, "Codex's live state"): the
//! hook's events (a prompt sent, a permission request with its tool's name, that tool run, the
//! turn over, Esc, subagents at work, the session started or gone), then the rollout's last turn
//! line. No permission wait ever comes from a rollout: Codex writes none there.
//!
//! Nothing of it runs while Codex is off (`Core::agent_on`): no folder of Codex's is looked into.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use crate::agent::{Agent, AgentWatch, HookState};
use crate::claude::{
    is_tool_name, owner, parse_iso_ms, read_seen_books, scan_tail, system_ms, Peek, ScanResult, SeenBook, SessionPhase, SessionState, Turn, TurnKind, ANSWER_SLACK_MS,
    EVENT_MAX_AGE_MS, WORKING_WINDOW_MS,
};
use crate::registry::now_ms;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// A rollout's first line is read this far at most; its `base_instructions` alone run to tens of
/// KB, skipped as they pass.
const META_LIMIT: u64 = 16 << 20;
/// The rollout of a session the hook tells of, outside today's and yesterday's folders (a session
/// opened again), is looked for again after this long.
const LOCATE_EVERY_MS: u64 = 5 * 60 * 1000;
/// A session that began after Pitwall's hook was put in (or its file changed) tells within this
/// long whether Codex runs the hook for it: `SessionStart` comes the moment it begins.
const HOOK_GRACE_MS: u64 = 15_000;
/// A rollout that grew by no more than this is read from where it ended; one that grew by more is
/// read from its end, as a new one is.
const GROWN_MAX: u64 = 4 << 20;
/// How far before where a grown rollout ended its new part is read from: the line being written
/// then.
const GROWN_OVERLAP: u64 = 64 * 1024;

/// Every look into a Codex folder (a file opened, a folder listed, a file's times asked), for the
/// tests that hold that an agent that is off is read for nothing.
#[cfg(test)]
pub(crate) static LOOKS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn look(path: &Path) {
    #[cfg(test)]
    LOOKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(path.to_path_buf());
    #[cfg(not(test))]
    let _ = path;
}

fn open(path: &Path) -> std::io::Result<File> {
    look(path);
    File::open(path)
}

fn list(path: &Path) -> std::io::Result<fs::ReadDir> {
    look(path);
    fs::read_dir(path)
}

fn stat(path: &Path) -> std::io::Result<fs::Metadata> {
    look(path);
    fs::metadata(path)
}

/* ---------- reading only what is declared ---------- */

/// A string as it is; any other value (a number, an object, a list) is skipped as it is read,
/// never held.
fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    struct Text;

    impl<'de> Visitor<'de> for Text {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("any value")
        }
        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }
        fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
            Ok(Some(value))
        }
        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(Text)
}

/// A number; anything else is skipped as it is read.
fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    struct Number;

    impl<'de> Visitor<'de> for Number {
        type Value = Option<f64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("any value")
        }
        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
            Ok(Some(value as f64))
        }
        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value as f64))
        }
        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
            Ok(Some(value).filter(|value| value.is_finite()))
        }
        fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(Number)
}

/// Where a session runs, as its `session_meta` says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Source {
    /// None written: Codex's own default, `vscode`.
    #[default]
    Missing,
    /// `cli`, `vscode`, `exec`, `mcp`…
    Named(String),
    /// An object, by its one key: `{"subagent": …}` is a subagent's thread, never a session of
    /// its own. Nothing inside it is read.
    Nested(Option<String>),
}

fn source<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Source, D::Error> {
    struct Of;

    impl<'de> Visitor<'de> for Of {
        type Value = Source;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("any value")
        }
        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(if is_word(value) { Source::Named(value.to_string()) } else { Source::Nested(None) })
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let key = map.next_key::<String>()?;
            if key.is_some() {
                map.next_value::<IgnoredAny>()?;
            }
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(Source::Nested(key.filter(|key| is_word(key))))
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(Source::Nested(None))
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(Source::Missing)
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(Source::Missing)
        }
        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }
        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok(Source::Nested(None))
        }
        fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
            Ok(Source::Nested(None))
        }
        fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
            Ok(Source::Nested(None))
        }
        fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
            Ok(Source::Nested(None))
        }
    }

    deserializer.deserialize_any(Of)
}

/// A short identifier (`cli`, `vscode`, `subagent`, `startup`); anything else is dropped.
fn is_word(text: &str) -> bool {
    !text.is_empty() && text.len() <= 40 && text.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// A thread's or a session's id as Codex writes them (UUIDs); it becomes part of a file name
/// looked for, so nothing else passes.
fn is_id(text: &str) -> bool {
    !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A line's kind and time; its payload is skipped as it is read.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Head {
    #[serde(rename = "type", deserialize_with = "text")]
    kind: Option<String>,
    #[serde(deserialize_with = "text")]
    timestamp: Option<String>,
}

/// A line's payload, of a kind `Head` told, read as `P` declares it.
#[derive(Deserialize)]
struct WithPayload<P> {
    payload: P,
}

/// What a payload is (`task_started`, `message`), and nothing else of it.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Kind {
    #[serde(rename = "type", deserialize_with = "text")]
    kind: Option<String>,
}

/// A rollout's first line, read as it streams in: the payload's other fields
/// (`base_instructions`, `git`, …) pass unread.
#[derive(Deserialize, Default)]
#[serde(default)]
struct MetaLine {
    #[serde(rename = "type", deserialize_with = "text")]
    kind: Option<String>,
    #[serde(deserialize_with = "text")]
    timestamp: Option<String>,
    payload: Option<MetaPayload>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MetaPayload {
    #[serde(deserialize_with = "text")]
    id: Option<String>,
    #[serde(deserialize_with = "text")]
    session_id: Option<String>,
    #[serde(deserialize_with = "text")]
    cwd: Option<String>,
    #[serde(deserialize_with = "source")]
    source: Source,
    #[serde(deserialize_with = "text")]
    timestamp: Option<String>,
}

/// A turn's start or end (`event_msg`): its kind and times, nothing else.
#[derive(Deserialize, Default)]
#[serde(default)]
struct TurnPayload {
    #[serde(rename = "type", deserialize_with = "text")]
    kind: Option<String>,
    #[serde(deserialize_with = "number")]
    started_at: Option<f64>,
    #[serde(deserialize_with = "number")]
    completed_at: Option<f64>,
}

/// The one text a `task_complete` gives: what Codex said at the turn's end.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Done {
    #[serde(deserialize_with = "text")]
    last_agent_message: Option<String>,
}

/// An `agent_message` event (an older history mode): what Codex said.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Spoken {
    #[serde(deserialize_with = "text")]
    message: Option<String>,
}

/// A `response_item` message: whose it is, before anything of what it says is read.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ItemHead {
    #[serde(rename = "type", deserialize_with = "text")]
    kind: Option<String>,
    #[serde(deserialize_with = "text")]
    role: Option<String>,
}

/// An assistant message's text blocks, read only once `ItemHead` said it is Codex's own.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ItemContent {
    content: Vec<Block>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Block {
    #[serde(rename = "type", deserialize_with = "text")]
    kind: Option<String>,
    #[serde(deserialize_with = "text")]
    text: Option<String>,
}

/// A line of `session_index.jsonl`: a thread's name, nothing else.
#[derive(Deserialize, Default)]
#[serde(default)]
struct IndexLine {
    #[serde(deserialize_with = "text")]
    id: Option<String>,
    #[serde(deserialize_with = "text")]
    thread_name: Option<String>,
}

/* ---------- rollouts ---------- */

/// What Pitwall knows of a rollout from its first line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Meta {
    /// The thread's id: the rollout's, `codex resume`'s.
    id: String,
    /// The session it belongs to: the thread's own for a session's main thread, the parent's for
    /// a subagent's.
    session_id: Option<String>,
    cwd: Option<String>,
    source: Source,
    started: Option<u64>,
}

impl Meta {
    /// A session of its own, in Codex's TUI or VS Code's (`cli`, `vscode`, or none written: Codex's
    /// default is `vscode`). `exec` runs, MCP clients, the desktop app's threads and subagents'
    /// threads are not.
    fn interactive(&self) -> bool {
        match &self.source {
            Source::Missing => true,
            Source::Named(source) => source == "cli" || source == "vscode",
            Source::Nested(_) => false,
        }
    }

    /// The session a subagent's thread works for.
    fn subagent_of(&self) -> Option<&str> {
        match &self.source {
            Source::Nested(Some(key)) if key == "subagent" => self.session_id.as_deref().filter(|parent| *parent != self.id),
            _ => None,
        }
    }
}

/// The `session_meta` of the rollout at `file` (its first line), read as it streams in.
fn read_meta(file: &Path) -> Option<Meta> {
    let reader = BufReader::new(open(file).ok()?).take(META_LIMIT);
    let mut stream = serde_json::Deserializer::from_reader(reader);
    let line = MetaLine::deserialize(&mut stream).ok()?;
    if line.kind.as_deref() != Some("session_meta") {
        return None;
    }
    let payload = line.payload?;
    let started = payload.timestamp.as_deref().and_then(parse_iso_ms).or_else(|| line.timestamp.as_deref().and_then(parse_iso_ms));

    Some(Meta {
        id: payload.id.filter(|id| is_id(id))?,
        session_id: payload.session_id.filter(|id| is_id(id)),
        cwd: payload.cwd.filter(|cwd| !cwd.is_empty()),
        source: payload.source,
        started,
    })
}

/// A rollout's `session_meta`: the fields Pitwall reads of it, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RolloutMeta {
    /// The thread's id (`payload.id`).
    pub id: String,
    pub cwd: Option<String>,
    /// `cli`, `vscode`, `exec`, …: where the session runs. A subagent's thread is `subagent` (its
    /// `source` is an object), never a session of its own. Codex writes none for `vscode`.
    pub source: Option<String>,
    /// When it began, in ms.
    pub started: Option<u64>,
}

/// The `session_meta` of the rollout at `path` (its first line), or none.
pub fn rollout_meta(path: &Path) -> Option<RolloutMeta> {
    let meta = read_meta(path)?;
    let source = match meta.source {
        Source::Missing => None,
        Source::Named(source) => Some(source),
        Source::Nested(key) => key,
    };
    Some(RolloutMeta { id: meta.id, cwd: meta.cwd, source, started: meta.started })
}

/// A turn line of a rollout's tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// `task_started`: a turn under way.
    Started,
    /// `task_complete`: the turn is over; Codex's move is done.
    Complete,
    /// `turn_aborted`: interrupted (Esc), over with nothing to tell.
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TurnLine {
    mark: Mark,
    /// The line's time.
    at: u64,
}

/// The last turn line in these lines. Every other line is passed over; of a turn line only its
/// kind and times are read.
fn read_turn(lines: &[&str]) -> Option<TurnLine> {
    for line in lines.iter().rev() {
        let line = line.trim();
        if !(line.contains("\"task_started\"") || line.contains("\"task_complete\"") || line.contains("\"turn_aborted\"")) {
            continue;
        }
        let Ok(head) = serde_json::from_str::<Head>(line) else {
            continue;
        };
        if head.kind.as_deref() != Some("event_msg") {
            continue;
        }
        let Ok(WithPayload { payload }) = serde_json::from_str::<WithPayload<TurnPayload>>(line) else {
            continue;
        };
        let (mark, seconds) = match payload.kind.as_deref() {
            Some("task_started") => (Mark::Started, payload.started_at),
            Some("task_complete") => (Mark::Complete, payload.completed_at),
            Some("turn_aborted") => (Mark::Aborted, payload.completed_at),
            _ => continue,
        };
        let at = head.timestamp.as_deref().and_then(parse_iso_ms).or_else(|| seconds.filter(|s| *s > 0.0).map(|s| (s * 1000.0) as u64)).unwrap_or(0);
        return Some(TurnLine { mark, at });
    }
    None
}

/// The last turn line in `file`'s bytes from `from` to `to` (a line cut at `from` left out).
fn scan_from(file: &Path, from: u64, to: u64) -> Option<TurnLine> {
    use std::io::{Seek, SeekFrom};

    let mut handle = File::open(file).ok()?;
    let mut buffer = vec![0u8; to.saturating_sub(from) as usize];
    handle.seek(SeekFrom::Start(from)).ok()?;
    handle.read_exact(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    read_turn(&lines)
}

/// What Codex last said in these lines: its last `task_complete`'s `last_agent_message`, else the
/// text of its last own message (`agent_message`, an assistant `response_item`). Nothing the user
/// wrote, nothing of Codex's reasoning or of a tool's input or output is read: a line's kind and
/// role are read before any of its text.
pub(crate) fn read_last_message(lines: &[&str]) -> Option<String> {
    let said = |text: Option<String>| text.map(|text| text.trim().to_string()).filter(|text| !text.is_empty());

    for line in lines.iter().rev() {
        let line = line.trim();
        if !(line.contains("\"task_complete\"") || line.contains("\"agent_message\"") || line.contains("\"assistant\"")) {
            continue;
        }
        let Ok(head) = serde_json::from_str::<Head>(line) else {
            continue;
        };

        match head.kind.as_deref() {
            Some("event_msg") => {
                let Ok(WithPayload { payload: kind }) = serde_json::from_str::<WithPayload<Kind>>(line) else {
                    continue;
                };
                let text = match kind.kind.as_deref() {
                    Some("task_complete") => serde_json::from_str::<WithPayload<Done>>(line).ok().and_then(|line| said(line.payload.last_agent_message)),
                    Some("agent_message") => serde_json::from_str::<WithPayload<Spoken>>(line).ok().and_then(|line| said(line.payload.message)),
                    _ => None,
                };
                if text.is_some() {
                    return text;
                }
            }
            Some("response_item") => {
                let Ok(WithPayload { payload: item }) = serde_json::from_str::<WithPayload<ItemHead>>(line) else {
                    continue;
                };
                if item.kind.as_deref() != Some("message") || item.role.as_deref() != Some("assistant") {
                    continue;
                }
                let Ok(WithPayload { payload }) = serde_json::from_str::<WithPayload<ItemContent>>(line) else {
                    continue;
                };
                let blocks: Vec<String> =
                    payload.content.into_iter().filter(|block| block.kind.as_deref() == Some("output_text")).filter_map(|block| said(block.text)).collect();
                if !blocks.is_empty() {
                    return Some(blocks.join("\n\n"));
                }
            }
            _ => {}
        }
    }
    None
}

/// `home`'s folder of the rollouts begun on the local day of `ms`, as Codex names it.
fn day_folder(home: &Path, ms: u64) -> PathBuf {
    let (year, month, day) = crate::clock::local_date(ms);
    home.join("sessions").join(year.to_string()).join(format!("{month:02}")).join(format!("{day:02}"))
}

/// A rollout's file name (`rollout-<time>-<thread>.jsonl`); a compressed one, a week idle at
/// least, isn't read.
fn is_rollout(name: &str) -> bool {
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

/// Every day folder of `home`'s rollouts, the latest first; names only.
fn day_folders(home: &Path) -> Vec<PathBuf> {
    let numbered = |dir: &Path| -> Vec<PathBuf> {
        let mut found: Vec<(String, PathBuf)> = list(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                (!name.is_empty() && name.chars().all(|c| c.is_ascii_digit())).then(|| (name, entry.path()))
            })
            .collect();
        // By number: the year isn't zero-padded.
        found.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| b.0.cmp(&a.0)));
        found.into_iter().map(|(_, path)| path).collect()
    };

    let mut days = Vec::new();
    for year in numbered(&home.join("sessions")) {
        for month in numbered(&year) {
            days.extend(numbered(&month));
        }
    }
    days
}

/// The rollout of thread `id` under `home`: the latest written of its files (a revert adds one)
/// in the latest day folder that has one. Names are compared; nothing is read.
pub(crate) fn find_rollout(home: &Path, id: &str) -> Option<PathBuf> {
    if !is_id(id) {
        return None;
    }
    let plain = format!("-{id}.jsonl");
    let reverted = format!("-{id}_");

    day_folders(home).into_iter().find_map(|day| {
        list(&day)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_str().is_some_and(|name| is_rollout(name) && (name.ends_with(&plain) || name.contains(&reverted))))
            .filter_map(|entry| {
                let file = entry.path();
                let modified = stat(&file).ok()?.modified().ok()?;
                Some((modified, file))
            })
            .max()
            .map(|(_, file)| file)
    })
}

/// The rollout files written since `since_ms` (by their modification time), newest first: plain
/// `.jsonl` only, a compressed one being at least a week old.
pub fn rollouts_since(home: &Path, since_ms: u64) -> Vec<PathBuf> {
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    for day in day_folders(home) {
        for entry in list(&day).into_iter().flatten().flatten() {
            if !entry.file_name().to_str().is_some_and(is_rollout) {
                continue;
            }
            let file = entry.path();
            let Some(modified) = stat(&file).ok().filter(fs::Metadata::is_file).and_then(|meta| meta.modified().ok()).map(system_ms) else {
                continue;
            };
            if modified >= since_ms {
                found.push((modified, file));
            }
        }
    }
    found.sort_by(|a, b| b.cmp(a));
    found.into_iter().map(|(_, file)| file).collect()
}

/// What Codex last said in session `id` (`read_last_message`): read from the end of its rollout
/// when asked and handed over, never kept or logged. None when it can't be told.
pub fn last_message(home: &Path, id: &str) -> Option<String> {
    message_in(&find_rollout(home, id)?)
}

/// What Codex last said in the rollout at `file`.
pub(crate) fn message_in(file: &Path) -> Option<String> {
    let size = stat(file).ok()?.len();
    look(file);
    scan_tail(file, size, read_last_message)
}

/// The sessions' names as `/rename` gave them (`$CODEX_HOME/session_index.jsonl`, the latest line
/// of each id winning), by session id: names only.
pub fn session_names(home: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(file) = open(&home.join("session_index.jsonl")) else {
        return names;
    };

    for line in BufReader::new(file).split(b'\n').map_while(Result::ok) {
        let Ok(entry) = serde_json::from_slice::<IndexLine>(&line) else {
            continue;
        };
        let Some(id) = entry.id.filter(|id| is_id(id)) else {
            continue;
        };
        match entry.thread_name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => names.insert(id, name.to_string()),
            None => names.remove(&id),
        };
    }
    names
}

/// A home's session names, with the stamp (modification time, size) of its index when read.
type Names = (Option<(SystemTime, u64)>, HashMap<String, String>);

/// A rollout as last read.
struct Rollout {
    modified: SystemTime,
    modified_ms: u64,
    size: u64,
    meta: Option<Meta>,
    /// Its last turn line; none when its last lines hold none.
    turn: Option<TurnLine>,
}

/// Where a session stands by its rollout's last turn line (`logged`: when it or its subagents'
/// rollouts were last written).
fn rollout_state(turn: Option<TurnLine>, logged: u64, baseline: u64, recent: u64) -> (SessionPhase, Option<Turn>) {
    match turn {
        Some(TurnLine { mark: Mark::Complete, at }) if at > baseline => (SessionPhase::Waiting, Some(Turn { kind: TurnKind::Finished, at })),
        Some(TurnLine { mark: Mark::Started, .. }) | None if logged > recent => (SessionPhase::Working, None),
        _ => (SessionPhase::Idle, None),
    }
}

/* ---------- the hook's events ---------- */

/// One event from Pitwall's Codex hook (`codex_hooks.rs`). Only these fields are ever written;
/// never the prompt, a message or a tool's input or output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HookEvent {
    /// Codex's event: `UserPromptSubmit`, `PermissionRequest`, `Stop`…
    event: String,
    session_id: String,
    /// The subagent it came from (`SubagentStart`, `SubagentStop`); none for the session's own
    /// thread.
    agent_id: Option<String>,
    cwd: Option<String>,
    /// Why a session started: `startup` | `resume` | `clear` | `compact` | `fork`.
    source: Option<String>,
    /// The tool a permission request is for, or that just ran, by name only.
    tool_name: Option<String>,
    /// The hook's parent: Codex's TUI (`--no-daemon`), else the shared daemon.
    ppid: Option<u32>,
    /// When it happened: the file's modification time.
    at: u64,
}

impl HookEvent {
    /// The hook's `key=value` lines; unknown keys are skipped.
    fn parse(text: &str, at: u64) -> Option<Self> {
        let mut event = HookEvent { at, ..HookEvent::default() };

        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = Some(value.to_string()).filter(|value| !value.is_empty());

            match key {
                "event" => event.event = value.filter(|value| is_word(value)).unwrap_or_default(),
                "session_id" => event.session_id = value.filter(|value| is_id(value)).unwrap_or_default(),
                "agent_id" => event.agent_id = value.filter(|value| value.len() <= 256),
                "cwd" => event.cwd = value,
                "source" => event.source = value.filter(|value| is_word(value)),
                "tool_name" => event.tool_name = value.filter(|name| is_tool_name(name)),
                "ppid" => event.ppid = value.and_then(|value| value.parse().ok()).filter(|pid| *pid > 1),
                _ => {}
            }
        }

        (!event.event.is_empty() && !event.session_id.is_empty()).then_some(event)
    }
}

/// The hook's event files in `dir`, read in the order they were written, and deleted; one older
/// than a day is deleted unread.
fn read_events(dir: &Path) -> Vec<HookEvent> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let now = now_ms();
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "event"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    files.sort();

    let mut events = Vec::new();
    for (modified, file) in files {
        let at = system_ms(modified);
        if now.saturating_sub(at) >= EVENT_MAX_AGE_MS {
            let _ = fs::remove_file(&file);
            continue;
        }
        let raw = fs::read_to_string(&file).unwrap_or_default();
        let _ = fs::remove_file(&file);
        events.extend(HookEvent::parse(&raw, at));
    }
    events
}

/// While Codex is off its hook may still write: what it left a day ago and more goes, unread,
/// as the registry's dead records do.
pub fn sweep_events(dir: &Path) {
    let now = now_ms();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let old = entry.metadata().ok().and_then(|meta| meta.modified().ok()).is_some_and(|at| now.saturating_sub(system_ms(at)) >= EVENT_MAX_AGE_MS);
        if old && entry.path().extension().is_some_and(|ext| ext == "event" || ext == "tmp") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Where a session stands by its hook events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookPhase {
    /// Started; nothing to wait on.
    Idle,
    Working,
    /// Its turn ended, or it waits on a permission request.
    Waiting(TurnKind),
    /// The user pressed Esc (`Interrupt`): over, with nothing to tell.
    Interrupted,
    /// Nothing decisive heard yet, or a permission request answered with no event saying so (a
    /// denied tool: Codex went on): the rollout decides until the next event.
    Log,
    Ended,
}

struct HookSession {
    cwd: Option<String>,
    /// The hook's parent, as last told.
    ppid: Option<u32>,
    phase: HookPhase,
    /// When the phase began.
    since: u64,
    /// The latest event.
    last: u64,
    /// When it started (`SessionStart`).
    started: Option<u64>,
    /// It started anew (not `resume`): its rollout is a new one, in today's folder.
    fresh: bool,
    /// Subagents started and not stopped yet: they keep the turn at work.
    helping: HashSet<String>,
    /// The subagents heard of in the current turn.
    helpers: HashSet<String>,
    /// The thread whose permission request is open (`""`: the session's own).
    asker: Option<String>,
    /// The open permission request's tool.
    tool: Option<String>,
}

impl HookSession {
    fn new(at: u64) -> Self {
        Self {
            cwd: None,
            ppid: None,
            phase: HookPhase::Log,
            since: at,
            last: at,
            started: None,
            fresh: false,
            helping: HashSet::new(),
            helpers: HashSet::new(),
            asker: None,
            tool: None,
        }
    }

    fn apply(&mut self, event: &HookEvent) {
        if self.cwd.is_none() {
            self.cwd = event.cwd.clone();
        }
        if event.ppid.is_some() {
            self.ppid = event.ppid;
        }
        self.last = self.last.max(event.at);

        let thread = event.agent_id.clone().unwrap_or_default();
        let next = match event.event.as_str() {
            "SessionStart" if event.source.as_deref() == Some("compact") => None,
            "SessionStart" => {
                self.started = Some(event.at);
                self.fresh = event.source.as_deref() != Some("resume");
                self.helping.clear();
                Some(HookPhase::Idle)
            }
            "UserPromptSubmit" => {
                self.helpers.clear();
                Some(HookPhase::Working)
            }
            // The request opened this moment, for this thread.
            "PermissionRequest" => {
                self.asker = Some(thread.clone());
                self.tool = event.tool_name.clone();
                Some(HookPhase::Waiting(TurnKind::Permission))
            }
            "PostToolUse" => match self.phase {
                // The thread that asked ran its tool: allowed. Another tool, or another thread's,
                // ran beside the open request.
                HookPhase::Waiting(TurnKind::Permission) => {
                    let same_tool = event.tool_name.is_none() || self.tool.is_none() || event.tool_name == self.tool;
                    (self.asker.as_deref() == Some(thread.as_str()) && same_tool).then_some(HookPhase::Working)
                }
                // A Stop hook of another tool can keep the turn going.
                HookPhase::Waiting(TurnKind::Finished) => thread.is_empty().then_some(HookPhase::Working),
                HookPhase::Waiting(_) | HookPhase::Ended | HookPhase::Interrupted => None,
                _ => Some(HookPhase::Working),
            },
            "SubagentStart" => {
                if !thread.is_empty() {
                    self.helping.insert(thread.clone());
                    self.helpers.insert(thread);
                }
                None
            }
            "SubagentStop" => {
                self.helping.remove(&thread);
                None
            }
            "Stop" => Some(HookPhase::Waiting(TurnKind::Finished)),
            "Interrupt" => Some(HookPhase::Interrupted),
            "SessionEnd" => Some(HookPhase::Ended),
            _ => None,
        };

        if let Some(phase) = next {
            if phase != HookPhase::Waiting(TurnKind::Permission) {
                self.asker = None;
                self.tool = None;
            }
            if matches!(phase, HookPhase::Interrupted | HookPhase::Ended) {
                self.helping.clear();
            }
            // A tool call that finishes mid-turn begins nothing: the work began with its prompt.
            if !(phase == HookPhase::Working && self.phase == HookPhase::Working) {
                self.since = event.at;
            }
            self.phase = phase;
        }
    }

    /// Its phase and the turn it waits with; `None` when the rollout decides. `own`: when its
    /// rollout was last written, which answers an open permission request (denied: Codex went on);
    /// `any`: that or one of its subagents' rollouts, which keeps it alive.
    fn state(&mut self, own: u64, any: u64, baseline: u64, recent: u64) -> Option<(SessionPhase, Option<Turn>)> {
        let alive = self.last.max(any) > recent;
        let helped = !self.helping.is_empty() && alive;

        match self.phase {
            HookPhase::Log => None,
            HookPhase::Idle | HookPhase::Ended | HookPhase::Interrupted => Some((SessionPhase::Idle, None)),
            HookPhase::Working => Some((if alive { SessionPhase::Working } else { SessionPhase::Idle }, None)),
            HookPhase::Waiting(TurnKind::Finished) if helped => Some((SessionPhase::Working, None)),
            HookPhase::Waiting(TurnKind::Finished) if self.since > baseline => {
                Some((SessionPhase::Waiting, Some(Turn { kind: TurnKind::Finished, at: self.since })))
            }
            HookPhase::Waiting(TurnKind::Finished) => Some((SessionPhase::Idle, None)),
            HookPhase::Waiting(kind) => {
                if own > self.since + ANSWER_SLACK_MS {
                    self.phase = HookPhase::Log;
                    self.asker = None;
                    self.tool = None;
                    return None;
                }
                if self.since > baseline {
                    Some((SessionPhase::Waiting, Some(Turn { kind, at: self.since })))
                } else {
                    Some((if alive { SessionPhase::Working } else { SessionPhase::Idle }, None))
                }
            }
        }
    }
}

/// One take of the hook's events at a time (`take_events`).
static TAKING: Mutex<()> = Mutex::new(());

/// Takes the hook's events in: their files read and deleted without the lock on `codex`, then
/// applied under it. Returns the listed projects where a turn just ended.
pub fn take_events(codex: &Mutex<CodexWatch>) -> Vec<String> {
    let _taking = TAKING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Ok(dir) = codex.lock().map(|codex| codex.events_dir.clone()) else {
        return Vec::new();
    };
    let events = read_events(&dir);
    match codex.lock() {
        Ok(mut codex) => codex.apply_events(events),
        Err(_) => Vec::new(),
    }
}

/// A session the hook told of, for telling whether its process runs (`Core::codex_running`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tied {
    /// The thread's id.
    pub id: String,
    /// The hook's parent: Codex's TUI under `--no-daemon`, else the daemon.
    pub ppid: u32,
    pub cwd: Option<String>,
    pub started: Option<u64>,
    /// It runs in VS Code (its rollout's source).
    pub vscode: bool,
}

pub struct CodexWatch {
    /// Codex's home folder (`CoreConfig::codex_home`).
    home: PathBuf,
    /// The homes followed now, the main one first (`Core::codex_homes`); none while Codex is off.
    homes: Vec<PathBuf>,
    /// Where Pitwall's hook drops its events.
    events_dir: PathBuf,
    /// The shared seen book, read only: a look clears both agents' turns.
    seen_file: PathBuf,
    legacy_seen: Vec<PathBuf>,
    listed: Vec<String>,
    /// Rollouts read so far, by file.
    rollouts: HashMap<PathBuf, Rollout>,
    /// Sessions the hook told of, by session id.
    hooked: HashMap<String, HookSession>,
    /// The rollouts of hooked sessions in an older day folder, by session id, with when they were
    /// looked for.
    located: HashMap<String, (Option<PathBuf>, u64)>,
    /// Each home's names as last read, with the index file's stamp then.
    names: HashMap<PathBuf, Names>,
    /// The sessions whose process runs, as the core last told (`set_running`); none when that
    /// can't be told.
    running: Option<HashSet<String>>,
    /// When Codex began to be followed.
    since: u64,
    /// When the latest hook event happened.
    heard: u64,
    /// A session started or ended, or one was heard of anew, since `take_moved`.
    moved: bool,
    /// A full read ran; nothing is known before.
    ready: bool,
}

impl CodexWatch {
    pub fn new(codex_home: PathBuf, registry_dir: &Path) -> Self {
        Self {
            home: codex_home,
            homes: Vec::new(),
            events_dir: registry_dir.join("codex-events"),
            seen_file: registry_dir.join("claude-seen.json"),
            legacy_seen: Vec::new(),
            listed: Vec::new(),
            rollouts: HashMap::new(),
            hooked: HashMap::new(),
            located: HashMap::new(),
            names: HashMap::new(),
            running: None,
            since: 0,
            heard: 0,
            moved: false,
            ready: false,
        }
    }

    /// Whether a session started or ended, or one was heard of anew, since last asked: its
    /// process is to be looked for again (`Core::codex_running`).
    pub fn take_moved(&mut self) -> bool {
        std::mem::take(&mut self.moved)
    }

    /// When the latest event of the hook came, to the minute (Settings' "Last event"); none
    /// before the first.
    pub fn heard(&self) -> Option<u64> {
        (self.heard > 0).then(|| self.heard - self.heard % 60_000)
    }

    /// Where the hook drops its events.
    pub fn events_dir(&self) -> &Path {
        &self.events_dir
    }

    /// The Codex homes to follow (`Core::codex_homes`): none while Codex is off, and then
    /// everything read of it is forgotten.
    pub fn set_homes(&mut self, homes: Vec<PathBuf>) {
        if self.homes == homes {
            return;
        }
        if self.homes.is_empty() {
            self.since = now_ms();
        }
        if homes.is_empty() {
            self.rollouts.clear();
            self.hooked.clear();
            self.located.clear();
            self.names.clear();
            self.running = None;
            self.ready = false;
        } else {
            self.rollouts.retain(|file, _| homes.iter().any(|home| file.starts_with(home)));
            self.names.retain(|home, _| homes.contains(home));
        }
        self.homes = homes;
    }

    /// The seen books of older extension versions (`ClaudeWatch::legacy_seen`).
    pub fn set_legacy_seen(&mut self, files: Vec<PathBuf>) {
        self.legacy_seen = files;
    }

    /// The sessions whose process runs now (`Core::codex_running`); none when that can't be told.
    /// A hooked session whose process is known gone waits on nothing but a finished turn.
    pub fn set_running(&mut self, running: Option<HashSet<String>>) {
        self.running = running;
    }

    /// The folders whose rollouts are followed: each home's today's and yesterday's, and those
    /// of the hooked sessions' older rollouts.
    pub fn folders(&self) -> Vec<PathBuf> {
        let now = now_ms();
        let mut folders: Vec<PathBuf> = Vec::new();
        for home in &self.homes {
            for at in [now, now.saturating_sub(DAY_MS)] {
                folders.push(day_folder(home, at));
            }
        }
        folders.extend(self.located.values().filter_map(|(file, _)| file.as_deref()?.parent().map(Path::to_path_buf)));
        folders.sort();
        folders.dedup();
        folders
    }

    fn book(&self) -> SeenBook {
        read_seen_books(&self.seen_file, &self.legacy_seen).unwrap_or_else(|| SeenBook { since: now_ms(), paths: HashMap::new() })
    }

    /// The rollouts of today's and yesterday's folders changed since the earliest of the listed
    /// projects' last looks (or since midnight, for the sessions elsewhere), and those of the
    /// sessions the hook told of; rollouts no longer there are forgotten.
    fn rescan_rollouts(&mut self) {
        if self.homes.is_empty() {
            self.ready = false;
            return;
        }

        let now = now_ms();
        let book = self.book();
        let recent = now.saturating_sub(WORKING_WINDOW_MS);
        let (midnight, _, _) = crate::clock::local_day(now);
        let floor = self.listed.iter().map(|path| book.seen(path).unwrap_or(book.since)).min().unwrap_or(recent).min(recent).min(midnight);
        let mut present: HashSet<PathBuf> = HashSet::new();

        for home in self.homes.clone() {
            let mut days = vec![day_folder(&home, now), day_folder(&home, now.saturating_sub(DAY_MS))];
            days.dedup();
            for day in days {
                for entry in list(&day).into_iter().flatten().flatten() {
                    if !entry.file_name().to_str().is_some_and(is_rollout) {
                        continue;
                    }
                    let file = entry.path();
                    let Ok(meta) = stat(&file) else {
                        continue;
                    };
                    if !meta.is_file() {
                        continue;
                    }
                    present.insert(file.clone());
                    let modified = meta.modified().map(system_ms).unwrap_or(0);
                    if modified > floor || self.rollouts.contains_key(&file) {
                        self.read_rollout(file, &meta);
                    }
                }
            }
        }

        self.locate(now);
        present.extend(self.located.values().filter_map(|(file, _)| file.clone()));
        self.rollouts.retain(|file, _| present.contains(file));
        self.ready = true;
    }

    /// The rollouts of the hooked sessions whose rollout isn't one of today's or yesterday's: a
    /// session opened again writes into its old one. Each is looked for by name again only after
    /// a while.
    fn locate(&mut self, now: u64) {
        let known: HashSet<String> = self
            .rollouts
            .values()
            .filter_map(|rollout| rollout.meta.as_ref())
            .filter(|meta| meta.subagent_of().is_none())
            .flat_map(|meta| [Some(meta.id.clone()), meta.session_id.clone()])
            .flatten()
            .collect();
        // A session begun anew writes into today's folder, once its first prompt is sent.
        let wanted: Vec<String> = self.hooked.iter().filter(|(id, session)| !known.contains(*id) && !session.fresh).map(|(id, _)| id.clone()).collect();
        self.located.retain(|id, _| self.hooked.contains_key(id));

        for id in wanted {
            let fresh = self.located.get(&id).is_some_and(|(file, at)| file.is_some() || now.saturating_sub(*at) < LOCATE_EVERY_MS);
            if !fresh {
                let found = self.homes.iter().find_map(|home| find_rollout(home, &id));
                self.located.insert(id.clone(), (found, now));
            }
            if let Some(file) = self.located.get(&id).and_then(|(file, _)| file.clone()) {
                match stat(&file) {
                    Ok(meta) => self.read_rollout(file, &meta),
                    Err(_) => {
                        self.located.remove(&id);
                    }
                }
            }
        }
    }

    /// A rollout's last turn line (and, once, its first line), unless it is unchanged since.
    fn read_rollout(&mut self, file: PathBuf, meta: &fs::Metadata) {
        let Ok(modified) = meta.modified() else {
            return;
        };
        if self.rollouts.get(&file).is_some_and(|rollout| rollout.modified == modified && rollout.size == meta.len()) {
            return;
        }

        let head = self.rollouts.get(&file).and_then(|rollout| rollout.meta.clone()).or_else(|| read_meta(&file));
        look(&file);
        // Rollouts only grow: what was added is enough, the last turn line before it standing
        // until a later one comes.
        let grown = self.rollouts.get(&file).filter(|rollout| meta.len() > rollout.size).map(|rollout| (rollout.size, rollout.turn));
        let turn = match grown {
            Some((before, turn)) if meta.len() - before <= GROWN_MAX => scan_from(&file, before.saturating_sub(GROWN_OVERLAP), meta.len()).or(turn),
            _ => scan_tail(&file, meta.len(), read_turn),
        };
        self.rollouts.insert(file, Rollout { modified, modified_ms: system_ms(modified), size: meta.len(), meta: head, turn });
    }

    /// One rollout changed (a file-system event in a followed folder).
    pub fn refresh_file(&mut self, file: &Path) {
        if !self.ready || !file.file_name().and_then(|name| name.to_str()).is_some_and(is_rollout) {
            return;
        }
        let Some(folder) = file.parent() else {
            return;
        };
        if !self.folders().iter().any(|followed| followed == folder) {
            return;
        }
        match stat(file) {
            Ok(meta) if meta.is_file() => self.read_rollout(file.to_path_buf(), &meta),
            _ => {
                self.rollouts.remove(file);
            }
        }
    }

    /// Applies the hook's events in the order they were written; the rollouts of sessions heard
    /// of anew are looked for. Returns the listed projects where a turn just ended.
    fn apply_events(&mut self, events: Vec<HookEvent>) -> Vec<String> {
        let mut ended = Vec::new();

        for event in events {
            self.heard = self.heard.max(event.at);
            if matches!(event.event.as_str(), "SessionStart" | "SessionEnd") || !self.hooked.contains_key(&event.session_id) {
                self.moved = true;
            }
            let session = self.hooked.entry(event.session_id.clone()).or_insert_with(|| HookSession::new(event.at));
            session.apply(&event);

            if event.event == "Stop" {
                if let Some(cwd) = session.cwd.as_deref() {
                    ended.extend(owner(&self.listed, cwd).cloned());
                }
            }
        }

        // The rollouts the events tell of may have changed with them.
        if self.ready {
            let files: Vec<PathBuf> = self.located.values().filter_map(|(file, _)| file.clone()).collect();
            for file in files {
                if let Ok(meta) = stat(&file) {
                    self.read_rollout(file, &meta);
                }
            }
        }

        ended.sort();
        ended.dedup();
        ended
    }

    /// Every home's session names, read again only once its index changed.
    fn names(&mut self) -> HashMap<String, String> {
        let mut all = HashMap::new();
        for home in self.homes.clone() {
            let index = home.join("session_index.jsonl");
            let now = stat(&index).ok().and_then(|meta| Some((meta.modified().ok()?, meta.len())));
            let fresh = self.names.get(&home).is_some_and(|(stamp, _)| *stamp == now);
            if !fresh {
                self.names.insert(home.clone(), (now, if now.is_some() { session_names(&home) } else { HashMap::new() }));
            }
            if let Some((_, names)) = self.names.get(&home) {
                for (id, name) in names {
                    all.entry(id.clone()).or_insert_with(|| name.clone());
                }
            }
        }
        all
    }

    /// Where every Codex session stands, from what was read so far: the hook's events where they
    /// decide, else the rollout's last turn line. The listed projects' sessions, and those
    /// elsewhere the hook told of or whose rollout was written today. Only the seen book and the
    /// names' index are read.
    pub fn evaluate(&mut self) -> ScanResult {
        if self.homes.is_empty() {
            return ScanResult::default();
        }

        let book = self.book();
        let now = now_ms();
        let recent = now.saturating_sub(WORKING_WINDOW_MS);
        let (midnight, _, _) = crate::clock::local_day(now);
        self.hooked.retain(|_, session| now.saturating_sub(session.last) < EVENT_MAX_AGE_MS);
        let names = self.names();
        let Self { rollouts, hooked, listed, running, .. } = self;

        // Each session's latest rollout; the threads that are no session of their own (`exec`
        // runs and the like); subagents' last writes, by the session they work for; and the
        // hook's session ids that aren't their thread's.
        let mut mains: HashMap<&str, &Rollout> = HashMap::new();
        let mut excluded: HashSet<&str> = HashSet::new();
        let mut helped: HashMap<&str, u64> = HashMap::new();
        let mut thread_of: HashMap<&str, &str> = HashMap::new();
        for rollout in rollouts.values() {
            let Some(meta) = &rollout.meta else {
                continue;
            };
            if let Some(parent) = meta.subagent_of() {
                let slot = helped.entry(parent).or_default();
                *slot = (*slot).max(rollout.modified_ms);
                continue;
            }
            if !meta.interactive() {
                excluded.insert(&meta.id);
                continue;
            }
            if let Some(session) = meta.session_id.as_deref().filter(|session| *session != meta.id) {
                thread_of.insert(session, &meta.id);
            }
            match mains.get(meta.id.as_str()) {
                Some(have) if have.modified_ms >= rollout.modified_ms => {}
                _ => {
                    mains.insert(&meta.id, rollout);
                }
            }
        }

        let mut hook_of: HashMap<String, String> = HashMap::new();
        for session in hooked.keys() {
            let thread = thread_of.get(session.as_str()).map_or(session.as_str(), |thread| *thread);
            hook_of.insert(thread.to_string(), session.clone());
        }
        let mut ids: Vec<String> = mains.keys().map(|id| id.to_string()).chain(hook_of.keys().cloned()).filter(|id| !excluded.contains(id.as_str())).collect();
        ids.sort();
        ids.dedup();

        let mut by_project: HashMap<String, Vec<SessionState>> = HashMap::new();
        let mut others: Vec<SessionState> = Vec::new();

        for id in ids {
            let rollout = mains.get(id.as_str()).copied();
            let mut hook = hook_of.get(&id).and_then(|session| hooked.get_mut(session));
            let Some(cwd) = rollout.and_then(|rollout| rollout.meta.as_ref()?.cwd.clone()).or_else(|| hook.as_ref().and_then(|hook| hook.cwd.clone())) else {
                continue;
            };
            let path = owner(listed, &cwd).cloned();
            let baseline = book.seen(path.as_deref().unwrap_or(&cwd)).unwrap_or(book.since);
            // Elsewhere: what the hook told of in the last day, what its rollout says of today.
            let floor = match (&path, &hook) {
                (Some(_), _) => baseline.min(recent),
                (None, Some(_)) => 0,
                (None, None) => midnight.min(recent),
            };

            let own = rollout.map_or(0, |rollout| rollout.modified_ms);
            let any = own.max(helped.get(id.as_str()).copied().unwrap_or(0));
            if any.max(hook.as_ref().map_or(0, |hook| hook.last)) <= floor {
                continue;
            }

            let gone = hook.as_ref().is_some_and(|hook| hook.ppid.is_some()) && running.as_ref().is_some_and(|running| !running.contains(&id));
            // A turn at work by the hook that its rollout says ended or was cut short since the
            // hook's latest event (a `Stop` or `Interrupt` never heard): the rollout decides.
            if let (Some(hook), Some(TurnLine { mark: Mark::Complete | Mark::Aborted, at })) = (hook.as_mut(), rollout.and_then(|rollout| rollout.turn)) {
                if hook.phase == HookPhase::Working && at > hook.last {
                    hook.phase = HookPhase::Log;
                }
            }
            let decided = hook.as_mut().and_then(|hook| hook.state(own, any, baseline, recent));
            let from_hook = decided.is_some();
            let (mut phase, mut turn) = match (decided, rollout) {
                (Some(state), _) => state,
                (None, Some(rollout)) => rollout_state(rollout.turn, any, baseline, recent),
                (None, None) => continue,
            };
            // Its process is gone: it works and asks no more; a finished turn still waits.
            if gone && turn.is_none_or(|turn| turn.kind != TurnKind::Finished) {
                phase = SessionPhase::Idle;
                turn = None;
            }

            let hook = hook.map(|hook| &*hook);
            let rollout_turn = rollout.and_then(|rollout| rollout.turn);
            let interrupted = phase == SessionPhase::Idle
                && if from_hook { hook.is_some_and(|hook| hook.phase == HookPhase::Interrupted) } else { rollout_turn.is_some_and(|turn| turn.mark == Mark::Aborted) };
            let permission = turn.is_some_and(|turn| turn.kind == TurnKind::Permission);

            let state = SessionState {
                id: id.clone(),
                agent: Agent::Codex,
                path: path.clone().unwrap_or_default(),
                title: names.get(&id).cloned(),
                folder: Some(cwd),
                phase,
                turn,
                since: if from_hook { hook.map(|hook| hook.since) } else { rollout_turn.map(|turn| turn.at) }.or(turn.map(|turn| turn.at)),
                hooked: from_hook,
                ended: gone || hook.is_some_and(|hook| hook.phase == HookPhase::Ended),
                tool: hook.filter(|_| permission).and_then(|hook| hook.tool.clone()),
                subagents: hook.map_or(0, |hook| hook.helpers.len() as u32),
                interrupted,
            };
            match path {
                Some(path) => by_project.entry(path).or_default().push(state),
                None => others.push(state),
            }
        }

        let mut result = ScanResult::default();
        for folder_path in listed.iter() {
            let Some(mut states) = by_project.remove(folder_path) else {
                continue;
            };
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
        others.sort_by(|a, b| a.id.cmp(&b.id));
        result.others = others;
        result
    }

    /// When the earliest permission request still open in the project at `folder_path` opened,
    /// of the sessions whose process isn't known gone (`mark_seen`: a look stops before it).
    pub fn open_prompt(&mut self, folder_path: &str) -> Option<u64> {
        let scan = self.evaluate();
        scan.sessions
            .iter()
            .chain(&scan.others)
            .filter(|session| if session.path.is_empty() { session.folder.as_deref().is_some_and(|folder| crate::registry::same_path(folder, folder_path)) } else { session.path == folder_path })
            .filter_map(|session| session.turn.filter(|_| session.phase == SessionPhase::Waiting))
            .filter(|turn| turn.kind != TurnKind::Finished)
            .map(|turn| turn.at)
            .min()
    }

    /// The sessions the hook told of that haven't ended, with the hook's parent: those whose
    /// process may run.
    pub fn tied(&self) -> Vec<Tied> {
        let metas: HashMap<&str, &Meta> =
            self.rollouts.values().filter_map(|rollout| rollout.meta.as_ref()).filter(|meta| meta.interactive()).flat_map(|meta| [(meta.id.as_str(), meta)].into_iter().chain(meta.session_id.as_deref().map(|session| (session, meta)))).collect();
        // `exec` runs and the like are no sessions, here as in `evaluate`.
        let excluded: HashSet<&str> = self.rollouts.values().filter_map(|rollout| rollout.meta.as_ref()).filter(|meta| !meta.interactive()).map(|meta| meta.id.as_str()).collect();
        let mut tied: Vec<Tied> = self
            .hooked
            .iter()
            .filter(|(id, session)| session.phase != HookPhase::Ended && !excluded.contains(id.as_str()))
            .filter_map(|(id, session)| {
                let meta = metas.get(id.as_str());
                Some(Tied {
                    id: meta.map_or_else(|| id.clone(), |meta| meta.id.clone()),
                    ppid: session.ppid?,
                    cwd: session.cwd.clone().or_else(|| meta.and_then(|meta| meta.cwd.clone())),
                    started: session.started.or_else(|| meta.and_then(|meta| meta.started)),
                    vscode: meta.is_some_and(|meta| matches!(&meta.source, Source::Missing) || meta.source == Source::Named("vscode".into())),
                })
            })
            .collect();
        tied.sort_by(|a, b| a.id.cmp(&b.id));
        tied
    }

    /// A session's name and when its rollout was last written, for the Sessions page.
    pub fn peek(&mut self, id: &str) -> Option<Peek> {
        let modified = self.rollouts.values().filter(|rollout| rollout.meta.as_ref().is_some_and(|meta| meta.id == id)).map(|rollout| rollout.modified_ms).max()?;
        Some(Peek { title: self.names().remove(id), modified })
    }

    /// The rollout of thread `id`, when it was read.
    pub fn rollout_of(&self, id: &str) -> Option<PathBuf> {
        self.rollouts.iter().filter(|(_, rollout)| rollout.meta.as_ref().is_some_and(|meta| meta.id == id)).max_by_key(|(_, rollout)| rollout.modified_ms).map(|(file, _)| file.clone())
    }

    /// Where Pitwall's hook in Codex's own home stands (`hook_state_in`); `heard_before`: an
    /// earlier run heard an event after `hooks.json` as it is now (`CodexHook::heard_since_changed`).
    pub fn hook_state(&self, changed: Option<u64>, heard_before: bool) -> HookState {
        match self.hook_state_in(&self.home, changed) {
            HookState::Waiting if heard_before && !self.unheard_in(&self.home, changed) => HookState::Working,
            state => state,
        }
    }

    /// Where Pitwall's hook in `home` stands, given it is in that home's `hooks.json` (last
    /// changed at `changed`): waiting for the user's trust in Codex (`/hooks`) while a session of
    /// that home begun since that change shows in the rollouts with no event from it, or while
    /// nothing was heard since that change (in this run: what an earlier one heard isn't kept);
    /// at work once an event came.
    pub fn hook_state_in(&self, home: &Path, changed: Option<u64>) -> HookState {
        if self.unheard_in(home, changed) {
            HookState::Waiting
        } else if self.heard > changed.unwrap_or(0) {
            HookState::Working
        } else {
            HookState::Waiting
        }
    }

    /// A session of `home` begun since `hooks.json` changed (at `changed`) and since Codex was
    /// followed shows in the rollouts, past the grace, with no event from it.
    fn unheard_in(&self, home: &Path, changed: Option<u64>) -> bool {
        let now = now_ms();
        let from = changed.unwrap_or(0).max(self.since);
        self.rollouts
            .iter()
            .filter(|(file, _)| file.starts_with(home))
            .filter_map(|(_, rollout)| rollout.meta.as_ref())
            .filter(|meta| meta.interactive() && meta.started.is_some_and(|at| at > from && now.saturating_sub(at) > HOOK_GRACE_MS))
            .any(|meta| !self.hooked.contains_key(&meta.id) && !meta.session_id.as_ref().is_some_and(|session| self.hooked.contains_key(session)))
    }

    /// A full pass: the rollouts, the hook's events, then the verdict.
    #[cfg(test)]
    pub fn scan(&mut self, folder_paths: &[String]) -> ScanResult {
        AgentWatch::set_listed(self, folder_paths);
        AgentWatch::rescan(self);
        self.apply_events(read_events(&self.events_dir.clone()));
        self.evaluate()
    }
}

impl AgentWatch for CodexWatch {
    fn agent(&self) -> Agent {
        Agent::Codex
    }

    fn set_listed(&mut self, paths: &[String]) -> bool {
        if self.listed == paths {
            return false;
        }
        self.listed = paths.to_vec();
        true
    }

    fn rescan(&mut self) {
        self.rescan_rollouts();
    }

    fn ready(&self) -> bool {
        self.ready
    }

    fn evaluate(&mut self) -> ScanResult {
        CodexWatch::evaluate(self)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const LATEST: &str = include_str!("../fixtures/codex-watch/rollout-latest.jsonl");
    const ABORTED: &str = include_str!("../fixtures/codex-watch/rollout-aborted.jsonl");
    const SUBAGENT: &str = include_str!("../fixtures/codex-watch/rollout-subagent.jsonl");
    /// The same format from Codex's TUI (`source: "cli"`), a turn under way.
    pub(crate) const CLI: &str = include_str!("../fixtures/codex-watch/rollout-cli.jsonl");

    pub(crate) const PROJECT: &str = "/Users/test/projects/demo";
    pub(crate) const LATEST_ID: &str = "4ef9f383-bbab-7470-84c7-a2b6db02e9a2";
    pub(crate) const CLI_ID: &str = "5c1d0a7e-1111-7222-8333-944455556666";
    const ABORTED_ID: &str = "b03e3a44-4a56-775d-80f7-f61699591662";
    const SUBAGENT_ID: &str = "e6a54173-8247-760c-861d-1008bc7138c5";
    const PARENT_ID: &str = "945b575a-da8f-766c-8d07-e9c6b413c3ad";
    pub(crate) const SECRET: &str = "SECRET-c0dex";

    /// A rollout with every text Codex wrote in it (instructions, prompts, messages, reasoning,
    /// a tool's input and output, the turn's last message) carrying the marker.
    pub(crate) fn marked(rollout: &str) -> String {
        rollout.replace("\"<text>\"", &format!("\"{SECRET} text\""))
    }

    /// Today's rollouts of a Codex home, as Codex names them.
    pub(crate) fn write_rollouts(home: &Path, rollouts: &[(&str, &str)]) -> PathBuf {
        let day = day_folder(home, now_ms());
        fs::create_dir_all(&day).unwrap();
        for (id, rollout) in rollouts {
            fs::write(day.join(format!("rollout-2026-11-02T10-00-00-{id}.jsonl")), marked(rollout)).unwrap();
        }
        day
    }

    fn event(dir: &Path, lines: &[&str]) {
        crate::hooks::write_event(dir, &format!("{}\n", lines.join("\n"))).unwrap();
    }

    fn of<'a>(scan: &'a ScanResult, id: &str) -> Option<&'a SessionState> {
        scan.sessions.iter().chain(&scan.others).find(|session| session.id == id)
    }

    /// What Codex and the user wrote stays in Codex's files: nothing the watch hands on (the
    /// sessions, their names, a rollout's head, the sessions the hook told of) carries any of it,
    /// whatever its rollouts and the hook's files hold. Only `last_message`, asked, gives what
    /// Codex last said.
    #[test]
    fn the_watch_keeps_no_text_of_codex_or_the_user() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex");
        let registry = tmp.path().join("registry");
        fs::create_dir_all(&registry).unwrap();
        // Every turn is newer than the last look.
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        let day = write_rollouts(&home, &[(LATEST_ID, LATEST), (ABORTED_ID, ABORTED), (SUBAGENT_ID, SUBAGENT), (CLI_ID, CLI)]);
        let index = [
            json_line(CLI_ID, "Login page"),
            json_line(CLI_ID, "Login form"),
            json_line("../../etc", "Elsewhere"),
            format!(r#"{{"id":"{LATEST_ID}","thread_name":{{"text":"{SECRET}"}}}}"#),
        ];
        fs::write(home.join("session_index.jsonl"), index.join("\n")).unwrap();

        let mut watch = CodexWatch::new(home.clone(), &registry);
        watch.set_homes(vec![home.clone()]);
        let listed = vec![PROJECT.to_string()];
        let mut seen = String::new();

        let scan = watch.scan(&listed);
        seen.push_str(&format!("{scan:?}"));
        let latest = of(&scan, LATEST_ID).unwrap();
        assert_eq!((latest.agent, latest.phase, latest.turn.map(|turn| turn.kind)), (Agent::Codex, SessionPhase::Waiting, Some(TurnKind::Finished)));
        assert_eq!(latest.title, None);
        let aborted = of(&scan, ABORTED_ID).unwrap();
        assert!(aborted.phase == SessionPhase::Idle && aborted.interrupted, "Esc: over, with nothing to tell");
        let cli = of(&scan, CLI_ID).unwrap();
        assert_eq!((cli.phase, cli.title.as_deref()), (SessionPhase::Working, Some("Login form")));
        assert!(of(&scan, SUBAGENT_ID).is_none() && of(&scan, PARENT_ID).is_none(), "a subagent's thread is no session");
        assert_eq!(scan.waiting[PROJECT].kind, TurnKind::Finished);

        // The hook's events, with what it never writes thrown in.
        let events = registry.join("codex-events");
        let base = format!("session_id={CLI_ID}");
        event(&events, &["event=SessionStart", &base, "source=startup", &format!("cwd={PROJECT}"), "ppid=4242", &format!("prompt={SECRET}")]);
        event(&events, &["event=UserPromptSubmit", &base, "turn_id=019a", &format!("last_assistant_message={SECRET}")]);
        event(&events, &["event=PermissionRequest", &base, "tool_name=Bash", &format!("tool_input={SECRET}")]);
        let scan = watch.scan(&listed);
        seen.push_str(&format!("{scan:?}"));
        let cli = of(&scan, CLI_ID).unwrap();
        assert_eq!((cli.phase, cli.turn.map(|turn| turn.kind), cli.tool.as_deref(), cli.hooked), (SessionPhase::Waiting, Some(TurnKind::Permission), Some("Bash"), true));
        assert_eq!(scan.waiting[PROJECT].kind, TurnKind::Permission);
        assert_eq!(watch.open_prompt(PROJECT), cli.turn.map(|turn| turn.at));
        let tied = watch.tied();
        assert_eq!(tied.iter().map(|tie| (tie.id.as_str(), tie.ppid)).collect::<Vec<_>>(), [(CLI_ID, 4242)]);
        seen.push_str(&format!("{tied:?}"));

        event(&events, &["event=PostToolUse", &base, "tool_name=Bash", &format!("tool_response={SECRET}")]);
        let scan = watch.scan(&listed);
        assert_eq!(of(&scan, CLI_ID).unwrap().phase, SessionPhase::Working);

        // Its process gone: it works no more.
        watch.set_running(Some(HashSet::new()));
        let gone = watch.evaluate();
        let cli = of(&gone, CLI_ID).unwrap();
        assert!(cli.phase == SessionPhase::Idle && cli.ended);
        watch.set_running(None);

        event(&events, &["event=Interrupt", &base]);
        let scan = watch.scan(&listed);
        seen.push_str(&format!("{scan:?}"));
        let cli = of(&scan, CLI_ID).unwrap();
        assert!(cli.phase == SessionPhase::Idle && cli.interrupted && cli.turn.is_none());

        let files = rollouts_since(&home, 0);
        assert_eq!(files.len(), 4);
        for file in &files {
            assert!(fs::read_to_string(file).unwrap().contains(SECRET));
            seen.push_str(&format!("{:?}", rollout_meta(file)));
        }
        assert_eq!(rollout_meta(&day.join(format!("rollout-2026-11-02T10-00-00-{SUBAGENT_ID}.jsonl"))).unwrap().source.as_deref(), Some("subagent"));
        assert_eq!(rollout_meta(&day.join(format!("rollout-2026-11-02T10-00-00-{CLI_ID}.jsonl"))).unwrap().source.as_deref(), Some("cli"));
        seen.push_str(&format!("{:?}", session_names(&home)));
        seen.push_str(&format!("{:?}{:?}", watch.peek(CLI_ID), watch.peek(LATEST_ID)));
        assert!(!seen.contains(SECRET), "{seen}");

        // Asked for, what Codex last said; never what the user wrote.
        let said = last_message(&home, LATEST_ID).unwrap();
        assert_eq!(said, format!("{SECRET} text"));
    }

    fn json_line(id: &str, name: &str) -> String {
        serde_json::json!({ "id": id, "thread_name": name, "updated_at": "2026-11-02T10:00:00Z" }).to_string()
    }
}
