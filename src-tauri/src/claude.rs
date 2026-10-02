//! Claude Code session watcher. Port of the extension's `src/claude.ts`.
//! Reads only the last lines of each session log, and only to see whether the turn ended;
//! message text is never kept. "Seen" state lives in the shared `claude-seen.json`.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::registry::{now_ms, write_atomic};

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
    /// Claude waits for a permission prompt. Only the Notification hook reports this; the
    /// session log doesn't record it.
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenBook {
    /// The first run; older turns count as seen.
    pub since: u64,
    pub paths: HashMap<String, u64>,
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
            return Some(Verdict { turn: None, cwd });
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
            return Some(Verdict { turn: Some(Turn { kind: TurnKind::Asking, at }), cwd });
        }

        let stop = message.and_then(|m| m.get("stop_reason")).and_then(Value::as_str);

        if stop.is_some_and(|reason| reason != "tool_use") {
            return Some(Verdict { turn: Some(Turn { kind: TurnKind::Finished, at }), cwd });
        }

        return Some(Verdict { turn: None, cwd });
    }

    None
}

/// Claude Code's folder name for a project: every non-alphanumeric character becomes `-`.
pub fn encode_project_path(folder_path: &str) -> String {
    folder_path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Paths are case-insensitive on Windows, where VS Code lowercases the drive letter.
fn fold_case(value: &str) -> String {
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.to_string()
    }
}

/// Reads from the end of the file, growing the window until a line decides. A single tool
/// output line can be hundreds of KB, so a fixed tail is not enough.
fn read_file_tail(file: &Path, size: u64) -> Option<Verdict> {
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

        let verdict = read_tail(&lines);

        if verdict.is_some() || length >= size || span >= TAIL_MAX {
            return verdict;
        }

        span *= 4;
    }
}

struct Cached {
    modified: SystemTime,
    size: u64,
    verdict: Option<Verdict>,
}

/// A session counts as working while its log changed this recently and the turn isn't over.
const WORKING_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Hook events nobody resolved are dropped after a day.
const EVENT_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Default)]
pub struct ScanResult {
    /// Projects whose newest turn ended (or that wait on a prompt) after the user last looked.
    pub waiting: HashMap<String, Turn>,
    /// Projects where Claude is busy right now.
    pub working: HashSet<String>,
}

/// What the Notification hook wrote (see hooks.rs). Only these fields are read.
#[derive(Debug, Deserialize)]
struct HookInput {
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    notification_type: Option<String>,
}

struct HookEvent {
    cwd: String,
    turn: Turn,
}

fn modified_ms(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}

fn inside(cwd: &str, folder_path: &str) -> bool {
    let cwd = fold_case(cwd);
    let folder = fold_case(folder_path);
    cwd == folder || cwd.starts_with(&format!("{folder}{MAIN_SEPARATOR}"))
}

pub struct ClaudeWatch {
    root: PathBuf,
    seen_file: PathBuf,
    /// Seen books of older extension versions. VS Code windows not reloaded since the move
    /// to `~/.pitwall/` still record "I looked" there; read, never written.
    legacy_seen: Vec<PathBuf>,
    events_dir: PathBuf,
    files: HashMap<PathBuf, Cached>,
}

impl ClaudeWatch {
    pub fn new(claude_projects: PathBuf, registry_dir: &Path) -> Self {
        Self {
            root: claude_projects,
            seen_file: registry_dir.join("claude-seen.json"),
            legacy_seen: Vec::new(),
            events_dir: registry_dir.join("claude-events"),
            files: HashMap::new(),
        }
    }

    pub fn with_legacy_seen(mut self, files: Vec<PathBuf>) -> Self {
        self.legacy_seen = files;
        self
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

    /// Prompts reported by the Notification hook that are still open. A prompt is answered
    /// once its session log is written again; then the event file goes.
    fn hook_events(&self) -> Vec<HookEvent> {
        let Ok(entries) = fs::read_dir(&self.events_dir) else {
            return Vec::new();
        };

        let now = now_ms();
        let mut events = Vec::new();

        for entry in entries.flatten() {
            let file = entry.path();

            if file.extension().is_none_or(|ext| ext != "json") {
                continue;
            }

            let at = modified_ms(&file).unwrap_or(0);
            let input = fs::read_to_string(&file).ok().and_then(|raw| serde_json::from_str::<HookInput>(&raw).ok());
            let kind = match input.as_ref().and_then(|i| i.notification_type.as_deref()) {
                Some("permission_prompt") => Some(TurnKind::Permission),
                Some("elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input") => Some(TurnKind::Asking),
                _ => None,
            };

            let answered = input
                .as_ref()
                .and_then(|i| i.transcript_path.as_deref())
                .and_then(|path| modified_ms(Path::new(path)))
                .is_some_and(|log| log > at + 1_500);

            match (kind, input.and_then(|i| i.cwd)) {
                (Some(kind), Some(cwd)) if !answered && now.saturating_sub(at) < EVENT_MAX_AGE_MS => {
                    events.push(HookEvent { cwd, turn: Turn { kind, at } });
                }
                _ => {
                    let _ = fs::remove_file(&file);
                }
            }
        }

        events
    }

    pub fn scan(&mut self, folder_paths: &[String]) -> ScanResult {
        let book = self.read_seen_everywhere();
        let dirs: Vec<String> = fs::read_dir(&self.root)
            .map(|entries| entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        let events = self.hook_events();
        let recent = now_ms().saturating_sub(WORKING_WINDOW_MS);
        let mut result = ScanResult::default();

        for folder_path in folder_paths {
            let baseline = book.paths.get(folder_path).copied().unwrap_or(book.since);
            let (logged, working) = self.sessions(folder_path, &dirs, baseline, recent);
            let prompted = events
                .iter()
                .filter(|event| event.turn.at > baseline && inside(&event.cwd, folder_path))
                .map(|event| event.turn);

            match logged.into_iter().chain(prompted).max_by_key(|turn| turn.at) {
                Some(turn) => {
                    result.waiting.insert(folder_path.clone(), turn);
                }
                None if working => {
                    result.working.insert(folder_path.clone());
                }
                None => {}
            }
        }

        result
    }

    /// The newest finished turn after `baseline`, and whether a session is mid-turn.
    fn sessions(&mut self, folder_path: &str, dirs: &[String], baseline: u64, recent: u64) -> (Option<Turn>, bool) {
        let encoded = fold_case(&encode_project_path(folder_path));
        let mut newest: Option<Turn> = None;
        let mut working = false;

        for dir in dirs {
            let name = fold_case(dir);
            let exact = name == encoded;

            // Sessions started in a subfolder get `<project>-sub`; the log's cwd tells them
            // apart from a neighbour such as `pitwall-docs`.
            if !exact && !name.starts_with(&format!("{encoded}-")) {
                continue;
            }

            for (verdict, modified) in self.verdicts_in(&self.root.join(dir), baseline.min(recent)) {
                if !exact && !verdict.cwd.as_deref().is_some_and(|cwd| inside(cwd, folder_path)) {
                    continue;
                }

                match verdict.turn {
                    Some(turn) if turn.at > baseline && newest.is_none_or(|current| turn.at > current.at) => {
                        newest = Some(turn);
                    }
                    None if modified > recent => working = true,
                    _ => {}
                }
            }
        }

        (newest, working)
    }

    /// Verdicts of the session logs changed after `since`, with their modification time.
    fn verdicts_in(&mut self, dir: &Path, since: u64) -> Vec<(Verdict, u64)> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };

        let mut verdicts = Vec::new();

        for entry in entries.flatten() {
            let file = entry.path();

            if file.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }

            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };

            let modified_ms = modified.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

            if modified_ms <= since {
                continue;
            }

            let verdict = match self.files.get(&file) {
                Some(cached) if cached.modified == modified && cached.size == meta.len() => cached.verdict.clone(),
                _ => {
                    let verdict = read_file_tail(&file, meta.len());
                    self.files.insert(file, Cached { modified, size: meta.len(), verdict: verdict.clone() });
                    verdict
                }
            };

            if let Some(verdict) = verdict {
                verdicts.push((verdict, modified_ms));
            }
        }

        verdicts
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
