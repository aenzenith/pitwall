//! A session's earlier turns (O1), for a chat that opens a session with a log: `--resume`
//! replays nothing, so the core reads the log once, when the tab opens, and hands the turns to
//! the window as frames of the shapes the process itself writes (`user`, `assistant`). Only
//! those lines are read: never the `api-request*` lines (they can be large), the meta lines, a
//! subagent's (`isSidechain`) or a compaction's summary; images and documents go without their
//! data, and an assistant line without its thinking. What is read is held in the tab's memory
//! only, as the stream is.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::{Map, Value};

use super::wire::drop_media;

/// The most turns' frames kept: the latest.
pub const FRAMES_MAX: usize = 2_000;
/// The most bytes of them kept.
pub const BYTES_MAX: usize = 8 * 1024 * 1024;
/// A log line longer than this isn't read.
const LINE_MAX: usize = 8 * 1024 * 1024;

/// The frames of the log at `file`, oldest first, the latest `FRAMES_MAX` of them and
/// `BYTES_MAX` at most; none when it can't be read.
pub fn read(file: &Path) -> Vec<Value> {
    read_to(file, None)
}

/// `read`, up to and with the entry `at` (a fork's cut) when one is named: the latest frames
/// before it, however far back in the log it is; none when the log holds no such entry, rather
/// than turns the fork doesn't have.
pub fn read_to(file: &Path, at: Option<&str>) -> Vec<Value> {
    let Ok(opened) = File::open(file) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(opened);
    let mut frames: VecDeque<(Value, usize)> = VecDeque::new();
    let mut bytes = 0usize;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match read_line(&mut reader, &mut line) {
            Ok(false) => break,
            Ok(true) => {}
            Err(()) => continue,
        }
        let Some(frame) = frame_of(&line) else {
            continue;
        };
        let size = line.len();
        let cut = at.is_some_and(|at| frame.get("uuid").and_then(Value::as_str) == Some(at));
        bytes += size;
        frames.push_back((frame, size));
        while frames.len() > FRAMES_MAX || (bytes > BYTES_MAX && frames.len() > 1) {
            if let Some((_, size)) = frames.pop_front() {
                bytes -= size;
            }
        }
        if cut {
            return frames.into_iter().map(|(frame, _)| frame).collect();
        }
    }
    if at.is_some() {
        return Vec::new();
    }
    frames.into_iter().map(|(frame, _)| frame).collect()
}

/// One line into `line`, without its break: `Ok(false)` at the end, `Err` for one longer than
/// `LINE_MAX` (skipped to its end, nothing of it held).
fn read_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> Result<bool, ()> {
    let mut long = false;
    let mut any = false;
    loop {
        let (done, used) = match reader.fill_buf() {
            Ok([]) => return if any { if long { Err(()) } else { Ok(true) } } else { Ok(false) },
            Ok(chunk) => {
                any = true;
                let (part, done) = match chunk.iter().position(|byte| *byte == b'\n') {
                    Some(at) => (&chunk[..at], true),
                    None => (chunk, false),
                };
                if !long {
                    if line.len() + part.len() > LINE_MAX {
                        long = true;
                        line.clear();
                    } else {
                        line.extend_from_slice(part);
                    }
                }
                (done, part.len() + usize::from(done))
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Ok(false),
        };
        reader.consume(used);
        if done {
            return if long { Err(()) } else { Ok(true) };
        }
    }
}

/// A log line as the frame the process would have written for it; none for every other line.
fn frame_of(line: &[u8]) -> Option<Value> {
    // The large lines of other kinds begin with their type; they are never parsed.
    if line.starts_with(b"{\"type\":\"") && !line.starts_with(b"{\"type\":\"user\"") && !line.starts_with(b"{\"type\":\"assistant\"") {
        return None;
    }
    if !contains(line, b"\"type\":\"user\"") && !contains(line, b"\"type\":\"assistant\"") {
        return None;
    }
    let value: Value = serde_json::from_slice(line).ok()?;
    let fields = value.as_object()?;
    let kind = fields.get("type").and_then(Value::as_str)?;
    if !matches!(kind, "user" | "assistant") {
        return None;
    }
    let flag = |key: &str| fields.get(key).and_then(Value::as_bool) == Some(true);
    if flag("isMeta") || flag("isSidechain") || flag("isCompactSummary") || flag("isVisibleInTranscriptOnly") {
        return None;
    }
    let mut message = fields.get("message")?.clone();
    if let Some(content) = message.get_mut("content") {
        drop_media(content);
        // Thinking stays in the log: never its text, nor its signature.
        if let Some(blocks) = content.as_array_mut().filter(|_| kind == "assistant") {
            blocks.retain(|block| !matches!(block.get("type").and_then(Value::as_str), Some("thinking" | "redacted_thinking")));
        }
    }
    let mut frame = Map::new();
    frame.insert("type".into(), Value::String(kind.into()));
    frame.insert("message".into(), message);
    for (from, to) in [("uuid", "uuid"), ("parentUuid", "parent_uuid"), ("timestamp", "timestamp"), ("sessionId", "session_id"), ("toolUseResult", "tool_use_result")] {
        if let Some(value) = fields.get(from).filter(|value| !value.is_null()) {
            frame.insert(to.into(), value.clone());
        }
    }
    frame.insert("parent_tool_use_id".into(), Value::Null);
    if kind == "user" {
        if let Some(origin) = fields.get("origin") {
            frame.insert("origin".into(), origin.clone());
        }
    }
    Some(Value::Object(frame))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}
