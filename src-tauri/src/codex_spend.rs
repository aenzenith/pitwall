//! Codex's tokens and their API-equivalent cost, as `spend.rs` counts Claude's: from the rollouts
//! of every Codex account written today (`$CODEX_HOME/sessions/**/rollout-*.jsonl`), counts only.
//!
//! What is read of a rollout: `session_meta`'s ids and folder (the session a file's tokens go to:
//! its root session, so a subagent's own rollout counts towards the session that started it),
//! `turn_context`'s model, and `token_count` events' numbers. Every other value on those lines,
//! and every other line, is skipped as it is read, never kept: no message, reasoning, tool input
//! or output, no instructions.
//!
//! A request counts once. Each `token_count` event's `last_token_usage` is one request, but the
//! same event can come again: Codex repeats it when only the limits moved, and a fork or a
//! subagent's rollout copies its parent's lines (under new times). So a request is known by the
//! thread's running total with it (`total_token_usage`) and its own counts together, the same in
//! every copy; one seen before today (a session resumed today, its file read from the start, or
//! a fork's parent, read for it when it wasn't written today) never counts today.
//! `token_usage_record` lines (Codex 0.16x) carry the same counts and are not read: older
//! rollouts don't have them.
//!
//! Prices are the API's (CODEX-FACTS §14), per million tokens: input, cached input, output.
//! Reasoning is part of output; cached input and cache writes are part of input. A model not in
//! the table counts its tokens, never a guessed cost.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::clock::local_day;
use crate::spend::{ModelSpend, SessionSpend, SpendToday, TokenTotals};
use crate::usage::parse_rfc3339;

/// A prompt longer than this many tokens is billed at the long-context rates.
const LONG_CONTEXT: u64 = 272_000;
/// Long-context rates: input and output times these.
const LONG_INPUT: f64 = 2.0;
const LONG_OUTPUT: f64 = 1.5;
/// A cache write costs this much of input (the shorter-lived one).
const CACHE_WRITE: f64 = 1.25;

/// USD per million tokens: input, cached input, output.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Price {
    input: f64,
    cached: f64,
    output: f64,
}

const fn price(input: f64, cached: f64, output: f64) -> Price {
    Price { input, cached, output }
}

/// From developers.openai.com/api/docs/pricing (Standard, up to 272K of context), read 2 Nov 2026.
const PRICES: &[(&str, Price)] = &[
    ("gpt-6-astra", price(10.0, 1.0, 50.0)),
    ("gpt-6.1-sol", price(2.0, 0.10, 10.0)),
    ("gpt-6.1-terra", price(1.0, 0.05, 5.0)),
    ("gpt-6-luna", price(0.10, 0.01, 0.50)),
    ("gpt-6-sol", price(2.0, 0.20, 10.0)),
    ("gpt-5.5", price(5.0, 0.50, 30.0)),
    ("gpt-5.4", price(2.50, 0.25, 15.0)),
    ("gpt-5.3-codex", price(1.75, 0.175, 14.0)),
];

/// A model's price: its id as listed, or with a date after it (`gpt-6.1-sol-2026-10-01`); none
/// for anything else, which is never guessed.
fn price_of(model: &str) -> Option<Price> {
    let model = model.trim().to_ascii_lowercase();
    PRICES.iter().find_map(|(id, price)| {
        let dated = model.strip_prefix(id).and_then(|rest| rest.strip_prefix('-')).is_some_and(|date| !date.is_empty() && date.bytes().all(|b| b.is_ascii_digit() || b == b'-'));
        (model == *id || dated).then_some(*price)
    })
}

/// One request's counts as Codex writes them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(default)]
struct Counts {
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

impl Counts {
    /// As the page counts tokens: input not cached, cache writes, cache reads, output.
    fn totals(&self) -> TokenTotals {
        let cached = self.cached_input_tokens.min(self.input_tokens);
        let write = self.cache_write_input_tokens.min(self.input_tokens - cached);
        let input = self.input_tokens - cached - write;
        TokenTotals { input, output: self.output_tokens, cache_write: write, cache_read: cached, total: self.input_tokens.saturating_add(self.output_tokens) }
    }

    fn cost(&self, price: &Price) -> f64 {
        let tokens = self.totals();
        let n = |count: u64| count as f64;
        let long = self.input_tokens > LONG_CONTEXT;
        let (input_times, output_times) = if long { (LONG_INPUT, LONG_OUTPUT) } else { (1.0, 1.0) };
        let input = n(tokens.input) * price.input + n(tokens.cache_read) * price.cached + n(tokens.cache_write) * price.input * CACHE_WRITE;
        (input * input_times + n(tokens.output) * price.output * output_times) / 1_000_000.0
    }
}

/// A line, as far as it is read: its time, where it stands in its file, its kind, and of its
/// payload the fields below; nothing else of it is kept.
#[derive(Deserialize)]
struct Line {
    timestamp: Option<String>,
    ordinal: Option<u64>,
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Payload {
    /// An event's kind.
    #[serde(rename = "type")]
    kind: Option<String>,
    /// `session_meta`: the thread's id, its root session's, its folder, and in a subagent's
    /// rollout where its own lines begin (the ones before are its parent's, copied).
    id: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    subagent_history_start_ordinal: Option<u64>,
    /// `session_meta`: in a fork's rollout, the thread it was forked from (its lines copied after).
    forked_from_id: Option<String>,
    /// `turn_context`: the model of the turn.
    model: Option<String>,
    /// `token_count`: the thread's running total and the last request's counts.
    info: Option<Info>,
}

#[derive(Deserialize)]
struct Info {
    total_token_usage: Option<Counts>,
    last_token_usage: Option<Counts>,
}

/// What is known of a rollout read so far.
#[derive(Default)]
struct FileState {
    /// Bytes read. A line still being written is left for the next look.
    offset: u64,
    /// Its root session, from its first `session_meta`.
    session: Option<String>,
    cwd: Option<String>,
    /// Lines before this ordinal are its parent's, copied into it.
    own_from: Option<u64>,
    /// The model of the turn under way.
    model: Option<String>,
    met_meta: bool,
    /// A fork's parent thread, until what it made before today is known (`earlier_of`).
    parent: Option<String>,
}

struct Counted {
    /// Index into `CodexSpend::models`.
    model: usize,
    /// Index into `CodexSpend::sessions`.
    session: usize,
    counts: Counts,
}

/// Today's Codex tokens, kept up to date by reading only what the rollouts gained since the last
/// look. The caller holds it behind a mutex and refreshes it off the main thread.
#[derive(Default)]
pub struct CodexSpend {
    /// Today's local midnight and the next one, and today's `YYYY-MM-DD`.
    start: u64,
    end: u64,
    date: String,
    files: HashMap<PathBuf, FileState>,
    /// Today's requests, one per key.
    counted: HashMap<u64, Counted>,
    /// Keys of requests made before today: a copy of one written today isn't new use.
    earlier: HashSet<u64>,
    /// Forks' parent rollouts read today for `earlier`.
    parents: HashSet<PathBuf>,
    /// Model ids, with their price.
    models: Vec<(String, Option<f64>, Option<Price>)>,
    /// Root session ids of today's requests, with the folder each one ran in.
    sessions: Vec<(String, String)>,
}

/// A request, the same in every copy of it: the thread's running total with it, and its own counts.
fn key(total: &Counts, last: &Counts) -> u64 {
    let mut hasher = DefaultHasher::new();
    (total, last).hash(&mut hasher);
    hasher.finish()
}

impl CodexSpend {
    /// Reads what `files` (today's rollouts of every account) gained since the last look; the
    /// first look after launch, or after midnight, reads them from the start. `now` in ms.
    pub fn refresh(&mut self, now: u64, files: &[PathBuf]) {
        if !(self.start..self.end).contains(&now) {
            *self = Self::default();
            (self.start, self.end, self.date) = local_day(now);
        }
        self.files.retain(|path, _| files.contains(path));
        for path in files {
            self.read(path, files);
        }
    }

    fn read(&mut self, path: &Path, files: &[PathBuf]) {
        let Ok(meta) = path.symlink_metadata() else {
            return;
        };
        let len = meta.len();
        let mut state = self.files.remove(path).unwrap_or_default();
        // A file that shrank was written anew: read again from the start; requests counted
        // before are known by their keys.
        if state.offset > len {
            state = FileState::default();
        }
        if state.offset < len {
            if let Ok(file) = File::open(path) {
                let mut reader = BufReader::with_capacity(64 * 1024, file);
                if reader.seek(SeekFrom::Start(state.offset)).is_ok() {
                    let mut line = Vec::new();
                    loop {
                        line.clear();
                        match reader.read_until(b'\n', &mut line) {
                            Ok(0) | Err(_) => break,
                            // Written halfway: read again, whole, next time.
                            Ok(_) if line.last() != Some(&b'\n') => break,
                            Ok(read) => {
                                state.offset += read as u64;
                                self.take_line(&line, &mut state);
                                if let Some(parent) = state.parent.take() {
                                    self.earlier_of(path, &parent, files);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.files.insert(path.to_path_buf(), state);
    }

    fn take_line(&mut self, bytes: &[u8], state: &mut FileState) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        // Only the kinds read are parsed at all.
        if !(text.contains("token_count") || text.contains("session_meta") || text.contains("turn_context")) {
            return;
        }
        let Ok(line) = serde_json::from_str::<Line>(text) else {
            return;
        };
        let Some(payload) = line.payload else {
            return;
        };
        match line.kind.as_deref() {
            // The file's own is its first; a subagent's rollout copies its parent's after it.
            Some("session_meta") if !state.met_meta => {
                state.met_meta = true;
                state.session = payload.session_id.or(payload.id).filter(|id| !id.is_empty());
                state.cwd = payload.cwd;
                state.own_from = payload.subagent_history_start_ordinal;
                // A subagent's copies are told by their ordinals.
                state.parent = payload.forked_from_id.filter(|id| !id.is_empty() && state.own_from.is_none());
            }
            Some("turn_context") => {
                if let Some(model) = payload.model.filter(|model| !model.trim().is_empty()) {
                    state.model = Some(model);
                }
            }
            Some("event_msg") if payload.kind.as_deref() == Some("token_count") => {
                let copied = state.own_from.zip(line.ordinal).is_some_and(|(own_from, ordinal)| ordinal < own_from);
                let Some(Info { total_token_usage: Some(total), last_token_usage: Some(last) }) = payload.info else {
                    return;
                };
                let Some(at) = line.timestamp.as_deref().and_then(parse_rfc3339).and_then(|at| u64::try_from(at).ok()) else {
                    return;
                };
                if copied || at >= self.end || last.input_tokens.saturating_add(last.output_tokens) == 0 {
                    return;
                }
                let key = key(&total, &last);
                if at < self.start {
                    self.earlier.insert(key);
                    self.counted.remove(&key);
                    return;
                }
                if self.earlier.contains(&key) || self.counted.contains_key(&key) {
                    return;
                }
                let (Some(model), Some(session)) = (state.model.clone(), state.session.clone()) else {
                    return;
                };
                let model = self.model(&model);
                let session = self.session(&session, state.cwd.as_deref().unwrap_or_default());
                self.counted.insert(key, Counted { model, session, counts: last });
            }
            _ => {}
        }
    }

    /// A fork's parent thread `id`, when its rollout isn't one of today's (`files`): the requests
    /// it made before today are known as such, so their copies in the fork, under today's times,
    /// never count. Found by name in the fork's own home (a compressed or archived one isn't), read
    /// once a day, of each line only what `take_line` reads.
    fn earlier_of(&mut self, fork: &Path, id: &str, files: &[PathBuf]) {
        let Some(home) = fork.ancestors().find(|dir| dir.file_name().is_some_and(|name| name == "sessions")).and_then(Path::parent) else {
            return;
        };
        let Some(parent) = crate::codex::find_rollout(home, id) else {
            return;
        };
        if files.contains(&parent) || !crate::codex_usage::is_rollout(home, &parent) || !self.parents.insert(parent.clone()) {
            return;
        }
        let Ok(file) = File::open(&parent) else {
            return;
        };
        for bytes in BufReader::with_capacity(64 * 1024, file).split(b'\n').map_while(Result::ok) {
            let Some(text) = std::str::from_utf8(&bytes).ok().filter(|text| text.contains("token_count")) else {
                continue;
            };
            let Ok(Line { timestamp, kind, payload: Some(Payload { kind: event, info: Some(Info { total_token_usage: Some(total), last_token_usage: Some(last) }), .. }), .. }) = serde_json::from_str::<Line>(text) else {
                continue;
            };
            let before = timestamp.as_deref().and_then(parse_rfc3339).and_then(|at| u64::try_from(at).ok()).is_some_and(|at| at < self.start);
            if before && kind.as_deref() == Some("event_msg") && event.as_deref() == Some("token_count") {
                let key = key(&total, &last);
                self.earlier.insert(key);
                self.counted.remove(&key);
            }
        }
    }

    fn model(&mut self, id: &str) -> usize {
        self.models.iter().position(|(model, _, _)| model == id).unwrap_or_else(|| {
            self.models.push((id.to_string(), None, price_of(id)));
            self.models.len() - 1
        })
    }

    fn session(&mut self, id: &str, cwd: &str) -> usize {
        self.sessions.iter().position(|(session, _)| session == id).unwrap_or_else(|| {
            self.sessions.push((id.to_string(), cwd.to_string()));
            self.sessions.len() - 1
        })
    }

    /// Today as of the last look; the date of a look not made yet is today's all the same.
    pub fn today(&self) -> SpendToday {
        let mut rows: Vec<ModelSpend> = self.models.iter().map(|(id, _, price)| ModelSpend { model: id.clone(), tokens: TokenTotals::default(), cost_usd: 0.0, priced: price.is_some() }).collect();
        for counted in self.counted.values() {
            let row = &mut rows[counted.model];
            add(&mut row.tokens, &counted.counts.totals());
            if let Some(price) = &self.models[counted.model].2 {
                row.cost_usd += counted.counts.cost(price);
            }
        }
        rows.retain(|row| row.tokens.total > 0);
        rows.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd).then(b.tokens.total.cmp(&a.tokens.total)).then(a.model.cmp(&b.model)));

        let mut tokens = TokenTotals::default();
        for row in &rows {
            add(&mut tokens, &row.tokens);
        }
        let mut unpriced_models: Vec<String> = rows.iter().filter(|row| !row.priced).map(|row| row.model.clone()).collect();
        unpriced_models.sort();
        SpendToday { date: self.date.clone(), tokens, cost_usd: rows.iter().map(|row| row.cost_usd).sum(), models: rows, unpriced_models }
    }

    /// Today as of the last look, per root session id. Its `folder` is the folder the session
    /// ran in (`session_meta.cwd`), never sent to a page.
    pub fn by_session(&self) -> HashMap<String, SessionSpend> {
        let mut parts: HashMap<(usize, usize), (TokenTotals, f64)> = HashMap::new();
        for counted in self.counted.values() {
            let part = parts.entry((counted.session, counted.model)).or_default();
            add(&mut part.0, &counted.counts.totals());
            if let Some(price) = &self.models[counted.model].2 {
                part.1 += counted.counts.cost(price);
            }
        }
        let mut parts: Vec<((usize, usize), (TokenTotals, f64))> = parts.into_iter().filter(|(_, (tokens, _))| tokens.total > 0).collect();
        parts.sort_by(|(a, x), (b, y)| y.1.total_cmp(&x.1).then(y.0.total.cmp(&x.0.total)).then(self.models[a.1].0.cmp(&self.models[b.1].0)));

        let mut found: HashMap<String, SessionSpend> = HashMap::new();
        for ((session, model), (tokens, cost)) in parts {
            let (id, cwd) = &self.sessions[session];
            let spend = found.entry(id.clone()).or_insert_with(|| SessionSpend { tokens: TokenTotals::default(), cost_usd: 0.0, priced: true, models: Vec::new(), folder: cwd.clone() });
            add(&mut spend.tokens, &tokens);
            spend.cost_usd += cost;
            spend.priced &= self.models[model].2.is_some();
            spend.models.push(self.models[model].0.clone());
        }
        found
    }
}

fn add(sum: &mut TokenTotals, tokens: &TokenTotals) {
    sum.input += tokens.input;
    sum.output += tokens.output;
    sum.cache_write += tokens.cache_write;
    sum.cache_read += tokens.cache_read;
    sum.total += tokens.total;
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{json, Value};

    use super::*;

    const DAY_MS: u64 = 24 * 60 * 60 * 1000;

    /// A moment of the local day `now` is in, `ms` after its midnight, as a rollout writes it.
    fn at(now: u64, ms: u64) -> String {
        let (start, _, _) = local_day(now);
        let when = chrono::DateTime::from_timestamp_millis((start + ms) as i64).unwrap();
        when.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    fn counts(input: u64, cached: u64, output: u64) -> Value {
        json!({ "input_tokens": input, "cached_input_tokens": cached, "cache_write_input_tokens": 0, "output_tokens": output, "reasoning_output_tokens": output / 4, "total_tokens": input + output })
    }

    fn meta(id: &str, session: &str, own_from: Option<u64>) -> Value {
        let mut payload = json!({ "id": id, "session_id": session, "cwd": "/Users/test/projects/demo", "source": "vscode", "base_instructions": { "text": "SECRET instructions" } });
        if let Some(own_from) = own_from {
            payload["subagent_history_start_ordinal"] = json!(own_from);
            payload["source"] = json!({ "subagent": { "thread_spawn": { "parent_thread_id": session, "depth": 1 } } });
        }
        json!({ "timestamp": "2026-01-01T00:00:00.000Z", "type": "session_meta", "payload": payload })
    }

    fn line(at: &str, ordinal: u64, kind: &str, payload: Value) -> Value {
        json!({ "timestamp": at, "ordinal": ordinal, "type": kind, "payload": payload })
    }

    fn request(at: &str, ordinal: u64, total: Value, last: Value) -> Value {
        line(at, ordinal, "event_msg", json!({ "type": "token_count", "info": { "total_token_usage": total, "last_token_usage": last, "model_context_window": 475000 }, "rate_limits": null }))
    }

    fn write(path: &Path, lines: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, lines.iter().map(|line| format!("{line}\n")).collect::<String>()).unwrap();
    }

    /// One request counts once, however often Codex writes it and wherever it is copied: its
    /// event repeated when only the limits moved, a subagent's rollout copying its parent's lines
    /// before its own, a fork copying them under new times (of a session last written today or
    /// before), and a session from yesterday resumed today in the same file. A subagent's own
    /// requests go to the session that started it. No text of any line reaches what is counted.
    #[test]
    fn a_request_counts_once_wherever_it_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let now = local_day(crate::registry::now_ms()).0 + 12 * 60 * 60 * 1000;
        let (morning, noon, yesterday) = (at(now, 9 * 3_600_000), at(now, 11 * 3_600_000), at(now - DAY_MS, 10 * 3_600_000));
        let turn = |at: &str, ordinal| line(at, ordinal, "turn_context", json!({ "model": "gpt-6.1-sol", "cwd": "/Users/test/projects/demo", "turn_id": "t" }));
        let words = |at: &str, ordinal| line(at, ordinal, "response_item", json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "SECRET prompt about token_count" }] }));

        let first = (counts(20_000, 12_000, 300), counts(20_000, 12_000, 300));
        let second = (counts(52_000, 32_000, 600), counts(32_000, 20_000, 300));
        let parent = tmp.path().join("sessions/a/rollout-parent.jsonl");
        write(
            &parent,
            &[
                meta("parent", "parent", None),
                // Yesterday's request, in a session resumed today: never today's.
                turn(&yesterday, 1),
                request(&yesterday, 2, counts(9_000, 0, 100), counts(9_000, 0, 100)),
                turn(&morning, 3),
                words(&morning, 4),
                request(&morning, 5, first.0.clone(), first.1.clone()),
                // The same event again: only the limits moved.
                request(&morning, 6, first.0.clone(), first.1.clone()),
                request(&noon, 7, second.0.clone(), second.1.clone()),
            ],
        );
        // A subagent's rollout: its own meta, its parent's copied lines (under the fork's time),
        // then its own request.
        let subagent = tmp.path().join("sessions/b/rollout-subagent.jsonl");
        write(
            &subagent,
            &[
                meta("child", "parent", Some(3)),
                meta("parent", "parent", None),
                request(&noon, 2, counts(9_000, 0, 100), counts(9_000, 0, 100)),
                turn(&noon, 3),
                request(&noon, 4, counts(5_000, 1_000, 50), counts(5_000, 1_000, 50)),
            ],
        );
        // A fork: the parent's requests copied under new times, then nothing of its own.
        let fork = tmp.path().join("sessions/b/rollout-fork.jsonl");
        write(&fork, &[meta("fork", "fork", None), turn(&noon, 1), request(&noon, 2, first.0.clone(), first.1.clone()), request(&noon, 3, second.0, second.1)]);
        // A fork of a session last written yesterday, whose rollout isn't one of today's: what it
        // copies of it is yesterday's; one request is its own.
        let old = (counts(400_000, 300_000, 9_000), counts(400_000, 300_000, 9_000));
        write(&tmp.path().join("sessions/2026/1/01/rollout-2026-01-01T10-00-00-old.jsonl"), &[meta("old", "old", None), turn(&yesterday, 1), request(&yesterday, 2, old.0.clone(), old.1.clone())]);
        let mut forked = meta("again", "again", None);
        forked["payload"]["forked_from_id"] = json!("old");
        let again = tmp.path().join("sessions/b/rollout-again.jsonl");
        write(&again, &[forked, meta("old", "old", None), turn(&noon, 1), request(&noon, 2, old.0, old.1), turn(&noon, 3), request(&noon, 4, counts(410_000, 400_000, 100), counts(10_000, 0, 100))]);

        let mut spend = CodexSpend::default();
        spend.refresh(now, &[parent.clone(), subagent.clone(), fork.clone(), again.clone()]);
        let today = spend.today();
        assert_eq!(today.tokens.total, 20_300 + 32_300 + 5_050 + 10_100, "each of today's requests once");
        assert_eq!(today.tokens.cache_read, 12_000 + 20_000 + 1_000);
        assert_eq!(today.models.len(), 1);
        assert!((today.cost_usd - (8_000.0 * 2.0 + 12_000.0 * 0.1 + 300.0 * 10.0 + 12_000.0 * 2.0 + 20_000.0 * 0.1 + 300.0 * 10.0 + 4_000.0 * 2.0 + 1_000.0 * 0.1 + 50.0 * 10.0 + 10_000.0 * 2.0 + 100.0 * 10.0) / 1e6).abs() < 1e-9);

        let sessions = spend.by_session();
        assert_eq!(sessions.len(), 2, "the subagent's request is its parent session's");
        assert_eq!(sessions["parent"].tokens.total, today.tokens.total - 10_100);
        assert_eq!(sessions["again"].tokens.total, 10_100);

        // Read again with nothing new: the same.
        spend.refresh(now, &[parent, subagent, fork, again]);
        assert_eq!(spend.today(), today);
        let shown = format!("{:?}{:?}", serde_json::to_string(&today).unwrap(), sessions.values().map(|session| serde_json::to_string(session).unwrap()).collect::<Vec<_>>());
        assert!(!shown.contains("SECRET"), "no text of a line is kept");
    }
}
