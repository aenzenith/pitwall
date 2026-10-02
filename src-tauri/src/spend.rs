//! Today's Claude Code token use and what it would cost at API prices, for the Fuel page.
//! Reads the session logs under `~/.claude/projects`, subagent logs in subfolders included, but
//! only the token counts of assistant lines: `message.model`, `message.usage`, ids and the
//! timestamp, from today's lines. Message text is never kept: serde skips every field not
//! declared below. Lines are counted the way ccusage (github.com/ccusage/ccusage) counts them.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::clock::local_day;
use crate::registry::now_ms;

#[cfg(test)]
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Lines without this can't hold token counts and aren't parsed.
const USAGE_MARK: &str = "\"usage\"";
/// Claude Code's own placeholder replies; no API call behind them.
const SYNTHETIC: &str = "<synthetic>";

/// One log line, as far as it is read. `message` holds the counts of an assistant line.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    session_id: Option<String>,
    request_id: Option<String>,
    is_sidechain: Option<bool>,
    message: Option<Message>,
}

/// Older Claude Code versions logged a subagent's replies inside the parent's progress lines,
/// under `data.message`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    session_id: Option<String>,
    data: Option<ProgressData>,
}

#[derive(Deserialize)]
struct ProgressData {
    message: Option<Line>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

/// An API response's token counts. `kind` and `model` are only set on its `iterations`, where
/// an advisor's calls are listed under the advisor's own model.
#[derive(Deserialize)]
struct Usage {
    #[serde(rename = "type")]
    kind: Option<String>,
    model: Option<String>,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    cache_creation: Option<CacheCreation>,
    /// `fast` when the reply came in fast mode.
    speed: Option<String>,
    iterations: Option<Vec<Usage>>,
}

#[derive(Deserialize)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tokens {
    input: u64,
    output: u64,
    write_5m: u64,
    write_1h: u64,
    read: u64,
}

impl Tokens {
    /// Cache writes are split by lifetime when the log says so; a bare total counts as
    /// five-minute writes, the default.
    fn of(usage: &Usage) -> Self {
        let (write_5m, write_1h) = match &usage.cache_creation {
            Some(split) => (split.ephemeral_5m_input_tokens, split.ephemeral_1h_input_tokens),
            None => (usage.cache_creation_input_tokens, 0),
        };
        Self { input: usage.input_tokens, output: usage.output_tokens, write_5m, write_1h, read: usage.cache_read_input_tokens }
    }

    fn total(&self) -> u64 {
        [self.input, self.output, self.write_5m, self.write_1h, self.read].iter().fold(0, |sum, n| sum.saturating_add(*n))
    }
}

/// USD per million tokens. Cache writes cost 1.25× input for five minutes and 2× for an hour;
/// `fast` multiplies everything in fast mode.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Price {
    input: f64,
    output: f64,
    read: f64,
    fast: f64,
}

impl Price {
    fn cost(&self, tokens: &Tokens, fast: bool) -> f64 {
        let n = |count: u64| count as f64;
        let per_million = n(tokens.input) * self.input
            + n(tokens.output) * self.output
            + n(tokens.write_5m) * self.input * 1.25
            + n(tokens.write_1h) * self.input * 2.0
            + n(tokens.read) * self.read;

        per_million * if fast { self.fast } else { 1.0 } / 1_000_000.0
    }
}

const fn price(input: f64, output: f64, read: f64, fast: f64) -> Price {
    Price { input, output, read, fast }
}

/// API prices from platform.claude.com/docs/en/about-claude/pricing, by family and version.
/// Cache reads are 0.1× input except on Fable 5.1 and Mythos 5.1 (0.025×) and Opus 5.5
/// (0.05×). Fast mode costs double on Opus 5.5, 5 and 4.8; Opus 4.6 bills it at standard rates.
const PRICES: &[(&str, (u32, u32), Price)] = &[
    ("fable", (5, 1), price(10.0, 50.0, 0.25, 1.0)),
    ("mythos", (5, 1), price(10.0, 50.0, 0.25, 1.0)),
    ("fable", (5, 0), price(10.0, 50.0, 1.0, 1.0)),
    ("mythos", (5, 0), price(10.0, 50.0, 1.0, 1.0)),
    ("opus", (5, 5), price(4.0, 20.0, 0.20, 2.0)),
    ("opus", (5, 0), price(5.0, 25.0, 0.50, 2.0)),
    ("opus", (4, 8), price(5.0, 25.0, 0.50, 2.0)),
    ("opus", (4, 7), price(5.0, 25.0, 0.50, 1.0)),
    ("opus", (4, 6), price(5.0, 25.0, 0.50, 1.0)),
    ("opus", (4, 5), price(5.0, 25.0, 0.50, 1.0)),
    ("opus", (4, 1), price(15.0, 75.0, 1.50, 1.0)),
    ("opus", (4, 0), price(15.0, 75.0, 1.50, 1.0)),
    ("sonnet", (5, 5), price(2.0, 10.0, 0.20, 1.0)),
    ("sonnet", (5, 0), price(2.0, 10.0, 0.20, 1.0)),
    ("sonnet", (4, 6), price(3.0, 15.0, 0.30, 1.0)),
    ("sonnet", (4, 5), price(3.0, 15.0, 0.30, 1.0)),
    ("sonnet", (4, 0), price(3.0, 15.0, 0.30, 1.0)),
    ("haiku", (4, 5), price(1.0, 5.0, 0.10, 1.0)),
    ("haiku", (3, 5), price(0.80, 4.0, 0.08, 1.0)),
];

const FAMILIES: [&str; 5] = ["fable", "mythos", "opus", "sonnet", "haiku"];

/// Family and version of a model id: `claude-opus-4-5-20251101` is Opus 4.5,
/// `claude-opus-4-20250514` Opus 4, `claude-3-5-haiku-20241022` Haiku 3.5. Provider prefixes
/// (`us.anthropic.`), dates, `-v1:0`, `@…` and `[1m]` don't matter. Anything else is `None`.
fn model_version(model: &str) -> Option<(&'static str, (u32, u32))> {
    let lower = model.to_ascii_lowercase();
    let rest = &lower[lower.find("claude-")? + "claude-".len()..];
    let words: Vec<&str> = rest.split(|c: char| !c.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).collect();
    let at = words.iter().position(|word| FAMILIES.contains(word))?;
    let family = FAMILIES.into_iter().find(|family| *family == words[at])?;
    // Versions are one or two short numbers; a date has eight digits.
    let short = |word: &str| word.len() <= 2 && word.bytes().all(|b| b.is_ascii_digit());
    let numbers: Vec<&str> = if at == 0 {
        words[1..].iter().take_while(|word| short(word)).copied().collect()
    } else if words[..at].iter().all(|word| short(word)) {
        words[..at].to_vec()
    } else {
        return None;
    };

    let parse = |word: &str| word.parse::<u32>().ok();
    match numbers[..] {
        [major] => Some((family, (parse(major)?, 0))),
        [major, minor] => Some((family, (parse(major)?, parse(minor)?))),
        _ => None,
    }
}

/// The API price of a model; `None` for a model not in the table, which is never guessed.
fn price_of(model: &str) -> Option<Price> {
    let (family, version) = model_version(model)?;
    PRICES.iter().find(|(f, v, _)| *f == family && *v == version).map(|(_, _, price)| *price)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    /// Five-minute and one-hour cache writes together.
    pub cache_write: u64,
    pub cache_read: u64,
    pub total: u64,
}

impl TokenTotals {
    fn add(&mut self, tokens: &Tokens) {
        self.input += tokens.input;
        self.output += tokens.output;
        self.cache_write += tokens.write_5m + tokens.write_1h;
        self.cache_read += tokens.read;
        self.total += tokens.total();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpend {
    /// The id as Claude Code logged it, such as `claude-opus-5-5`.
    pub model: String,
    pub tokens: TokenTotals,
    /// Zero when the model isn't priced.
    pub cost_usd: f64,
    pub priced: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendToday {
    /// `YYYY-MM-DD`, local.
    pub date: String,
    /// Every model's tokens, priced or not.
    pub tokens: TokenTotals,
    /// Priced models only.
    pub cost_usd: f64,
    /// The costliest first.
    pub models: Vec<ModelSpend>,
    /// Models used today without a known price; their tokens count, their cost doesn't.
    pub unpriced_models: Vec<String>,
}

/// One session's share of today, for the Sessions page. Its subagents' use is in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpend {
    pub tokens: TokenTotals,
    /// Its priced models only.
    pub cost_usd: f64,
    /// Every model it used has a price: the cost is complete.
    pub priced: bool,
    /// The models it used, the costliest first.
    pub models: Vec<String>,
}

/// One API response's counts, as found on a line.
#[derive(Clone, Copy)]
struct Entry<'a> {
    timestamp: &'a str,
    session: &'a str,
    message_id: Option<&'a str>,
    request_id: Option<&'a str>,
    sidechain: bool,
    model: &'a str,
    tokens: Tokens,
    fast: bool,
}

impl Entry<'_> {
    /// ccusage's key: the message and request ids, which Claude Code repeats on every line it
    /// writes for one response and in every transcript it copies the response to. Without a
    /// request id, the session and timestamp stand in for it.
    fn key(&self) -> u64 {
        match (self.message_id, self.request_id) {
            (Some(id), Some(request)) => hash(&["key", id, request]),
            (Some(id), None) => hash(&["key", id, self.session, self.timestamp]),
            (None, _) => hash(&["anonymous", self.session, self.timestamp, self.model]),
        }
    }

    /// The message within its session, whatever the request: a sidechain line (`/btw`) can
    /// replay the parent's message under a new request id.
    fn route(&self) -> Option<u64> {
        self.message_id.map(|id| hash(&["route", id, self.session]))
    }

    /// Of two lines for one response, the main conversation's wins over a sidechain copy, then
    /// the fuller counts (streaming writes partial ones first), then the fast-mode one.
    fn beats(&self, kept: &Counted) -> bool {
        if self.sidechain != kept.sidechain {
            return kept.sidechain;
        }
        let (mine, theirs) = (self.tokens.total(), kept.tokens.total());
        if mine != theirs {
            return mine > theirs;
        }
        self.fast && !kept.fast
    }
}

fn hash(parts: &[&str]) -> u64 {
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}

struct Counted {
    /// Index into `Spend::models`.
    model: usize,
    /// Index into `Spend::sessions`.
    session: usize,
    tokens: Tokens,
    fast: bool,
    sidechain: bool,
}

struct Model {
    id: String,
    price: Option<Price>,
}

/// Today's tokens, kept up to date by reading only what the logs gained since the last look.
/// The caller holds it behind a mutex and refreshes it off the main thread.
pub struct Spend {
    root: PathBuf,
    /// Today's local midnight and the next one, and today's `YYYY-MM-DD`.
    start: u64,
    end: u64,
    date: String,
    /// Bytes read from each log changed today. A line still being written is left for the
    /// next look.
    offsets: HashMap<PathBuf, u64>,
    /// Today's responses, one per key.
    counted: HashMap<u64, Counted>,
    /// Sidechain lines counted today, by the parent message they might replay.
    replays: HashMap<u64, Vec<u64>>,
    /// Messages of the main conversation, any day: a sidechain line repeating one isn't new use.
    parents: HashSet<u64>,
    /// Keys of responses logged before today: a copy written today isn't new use either.
    earlier: HashSet<u64>,
    models: Vec<Model>,
    /// Session ids of today's responses.
    sessions: Vec<String>,
}

// The caller moves it to a worker thread.
const _: () = {
    const fn send<T: Send>() {}
    send::<Spend>();
};

impl Spend {
    /// `projects_dir` is Claude Code's `~/.claude/projects`.
    pub fn new(projects_dir: PathBuf) -> Self {
        let mut spend = Self {
            root: projects_dir,
            start: 0,
            end: 0,
            date: String::new(),
            offsets: HashMap::new(),
            counted: HashMap::new(),
            replays: HashMap::new(),
            parents: HashSet::new(),
            earlier: HashSet::new(),
            models: Vec::new(),
            sessions: Vec::new(),
        };
        spend.begin_day(now_ms());
        spend
    }

    /// Reads what the logs changed today gained since the last refresh. The first one after
    /// launch, or after midnight, reads those logs from the start.
    pub fn refresh(&mut self) {
        self.refresh_at(now_ms());
    }

    fn refresh_at(&mut self, now: u64) {
        if !(self.start..self.end).contains(&now) {
            self.begin_day(now);
        }

        let mut logs = Vec::new();
        find_logs(&self.root, UNIX_EPOCH + Duration::from_millis(self.start), &mut logs);

        let live: HashSet<&Path> = logs.iter().map(|(path, _)| path.as_path()).collect();
        self.offsets.retain(|path, _| live.contains(path.as_path()));

        for (path, len) in &logs {
            self.read_log(path, *len);
        }
    }

    /// Everything starts over at local midnight.
    fn begin_day(&mut self, now: u64) {
        (self.start, self.end, self.date) = local_day(now);
        self.offsets.clear();
        self.counted.clear();
        self.replays.clear();
        self.parents.clear();
        self.earlier.clear();
        self.models.clear();
        self.sessions.clear();
    }

    /// Streams the log from where the last look stopped, line by line. A log that shrank is
    /// read again from the start; lines counted before are recognised by their keys.
    fn read_log(&mut self, path: &Path, len: u64) {
        let from = self.offsets.get(path).copied().filter(|offset| *offset <= len).unwrap_or(0);
        if from == len {
            return;
        }
        let Ok(file) = File::open(path) else {
            return;
        };
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        if reader.seek(SeekFrom::Start(from)).is_err() {
            return;
        }

        // Lines carry their session id; the file name stands in for one that doesn't, and for a
        // subagent's log (`<session>/subagents/agent-….jsonl`) its session's folder.
        let fallback = log_session(path);
        let mut offset = from;
        let mut line = Vec::new();

        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                // Written halfway: read again, whole, next time.
                Ok(_) if line.last() != Some(&b'\n') => break,
                Ok(read) => {
                    offset += read as u64;
                    self.take_line(&line, &fallback);
                }
            }
        }

        self.offsets.insert(path.to_path_buf(), offset);
    }

    fn take_line(&mut self, bytes: &[u8], fallback: &str) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        if !text.contains(USAGE_MARK) {
            return;
        }
        let Ok(line) = serde_json::from_str::<Line>(text) else {
            return;
        };

        let has_usage = |line: &Line| line.message.as_ref().is_some_and(|message| message.usage.is_some());
        let (line, outer_session) = if has_usage(&line) {
            (line, None)
        } else if line.kind.as_deref() == Some("progress") {
            let Ok(progress) = serde_json::from_str::<Progress>(text) else {
                return;
            };
            match progress.data.and_then(|data| data.message) {
                Some(inner) if has_usage(&inner) => (inner, progress.session_id),
                _ => return,
            }
        } else {
            return;
        };

        let Some(at) = line.timestamp.as_deref().and_then(parse_iso_ms) else {
            return;
        };
        let Some(Message { id, model: Some(model), usage: Some(usage) }) = line.message else {
            return;
        };
        if model.is_empty() || model == SYNTHETIC {
            return;
        }

        // Before today, a line only tells what not to count again; after it (a clock set
        // wrong), nothing.
        if at >= self.end {
            return;
        }
        let today = at >= self.start;
        let session = line.session_id.or(outer_session).unwrap_or_else(|| fallback.to_string());
        let message_id = id.filter(|id| !id.is_empty());
        let entry = Entry {
            timestamp: line.timestamp.as_deref().unwrap_or_default(),
            session: &session,
            message_id: message_id.as_deref(),
            request_id: line.request_id.as_deref().filter(|id| !id.is_empty()),
            sidechain: line.is_sidechain == Some(true),
            model: &model,
            tokens: Tokens::of(&usage),
            fast: usage.speed.as_deref() == Some("fast"),
        };
        self.take(entry, today);

        // An advisor's calls are listed apart, under its own model; the top-level counts
        // don't include them.
        let advisors = usage
            .iterations
            .iter()
            .flatten()
            .filter(|iteration| iteration.kind.as_deref() == Some("advisor_message"))
            .filter_map(|iteration| iteration.model.as_deref().filter(|model| !model.is_empty()).map(|model| (model, iteration)));
        for (index, (model, iteration)) in advisors.enumerate() {
            let id = message_id.as_ref().map(|id| format!("{id}:advisor:{index}"));
            let advisor = Entry {
                message_id: id.as_deref(),
                model,
                tokens: Tokens::of(iteration),
                fast: iteration.speed.as_deref() == Some("fast"),
                ..entry
            };
            self.take(advisor, today);
        }
    }

    fn take(&mut self, entry: Entry, today: bool) {
        let key = entry.key();
        let route = entry.route();

        if !today {
            self.earlier.insert(key);
            self.counted.remove(&key);
            if let (Some(route), false) = (route, entry.sidechain) {
                self.parents.insert(route);
                self.drop_replays(route);
            }
            return;
        }
        if self.earlier.contains(&key) {
            return;
        }

        match route {
            Some(route) if entry.sidechain => {
                if self.parents.contains(&route) {
                    return;
                }
            }
            Some(route) => {
                self.parents.insert(route);
                self.drop_replays(route);
            }
            None => {}
        }

        if self.counted.get(&key).is_some_and(|kept| !entry.beats(kept)) {
            return;
        }

        let model = self.model(entry.model);
        let session = self.session(entry.session);
        self.counted.insert(key, Counted { model, session, tokens: entry.tokens, fast: entry.fast, sidechain: entry.sidechain });

        if let (Some(route), true) = (route, entry.sidechain) {
            let keys = self.replays.entry(route).or_default();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }

    /// The parent's message showed up: sidechain copies of it counted so far go.
    fn drop_replays(&mut self, route: u64) {
        for key in self.replays.remove(&route).unwrap_or_default() {
            if self.counted.get(&key).is_some_and(|counted| counted.sidechain) {
                self.counted.remove(&key);
            }
        }
    }

    fn model(&mut self, id: &str) -> usize {
        self.models.iter().position(|model| model.id == id).unwrap_or_else(|| {
            self.models.push(Model { id: id.to_string(), price: price_of(id) });
            self.models.len() - 1
        })
    }

    fn session(&mut self, id: &str) -> usize {
        self.sessions.iter().position(|session| session == id).unwrap_or_else(|| {
            self.sessions.push(id.to_string());
            self.sessions.len() - 1
        })
    }

    /// Today as of the last refresh, per session id.
    pub fn by_session(&self) -> HashMap<String, SessionSpend> {
        // Per session, per model: tokens and cost.
        let mut parts: HashMap<(usize, usize), (TokenTotals, f64)> = HashMap::new();
        for counted in self.counted.values() {
            let part = parts.entry((counted.session, counted.model)).or_default();
            part.0.add(&counted.tokens);
            if let Some(price) = self.models[counted.model].price {
                part.1 += price.cost(&counted.tokens, counted.fast);
            }
        }

        // The costliest model first in each session's list.
        let mut parts: Vec<((usize, usize), (TokenTotals, f64))> = parts.into_iter().filter(|(_, (tokens, _))| tokens.total > 0).collect();
        parts.sort_by(|(a, x), (b, y)| y.1.total_cmp(&x.1).then(y.0.total.cmp(&x.0.total)).then(self.models[a.1].id.cmp(&self.models[b.1].id)));

        let mut found: HashMap<String, SessionSpend> = HashMap::new();
        for ((session, model), (tokens, cost)) in parts {
            let spend = found
                .entry(self.sessions[session].clone())
                .or_insert_with(|| SessionSpend { tokens: TokenTotals::default(), cost_usd: 0.0, priced: true, models: Vec::new() });
            spend.tokens.input += tokens.input;
            spend.tokens.output += tokens.output;
            spend.tokens.cache_write += tokens.cache_write;
            spend.tokens.cache_read += tokens.cache_read;
            spend.tokens.total += tokens.total;
            spend.cost_usd += cost;
            spend.priced &= self.models[model].price.is_some();
            spend.models.push(self.models[model].id.clone());
        }

        found
    }

    /// Today as of the last refresh.
    pub fn today(&self) -> SpendToday {
        let mut rows: Vec<ModelSpend> = self
            .models
            .iter()
            .map(|model| ModelSpend { model: model.id.clone(), tokens: TokenTotals::default(), cost_usd: 0.0, priced: model.price.is_some() })
            .collect();

        for counted in self.counted.values() {
            let row = &mut rows[counted.model];
            row.tokens.add(&counted.tokens);
            if let Some(price) = self.models[counted.model].price {
                row.cost_usd += price.cost(&counted.tokens, counted.fast);
            }
        }

        rows.retain(|row| row.tokens.total > 0);
        rows.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd).then(b.tokens.total.cmp(&a.tokens.total)).then(a.model.cmp(&b.model)));

        let mut tokens = TokenTotals::default();
        for row in &rows {
            tokens.input += row.tokens.input;
            tokens.output += row.tokens.output;
            tokens.cache_write += row.tokens.cache_write;
            tokens.cache_read += row.tokens.cache_read;
            tokens.total += row.tokens.total;
        }
        let mut unpriced_models: Vec<String> = rows.iter().filter(|row| !row.priced).map(|row| row.model.clone()).collect();
        unpriced_models.sort();

        SpendToday { date: self.date.clone(), tokens, cost_usd: rows.iter().map(|row| row.cost_usd).sum(), models: rows, unpriced_models }
    }
}

/// The session a log belongs to by its path: its file name, or for a subagent's log
/// (`<session>/subagents/agent-….jsonl`) its session's folder.
fn log_session(path: &Path) -> String {
    let parent = path.parent();
    let session = match parent.and_then(Path::file_name) {
        Some(name) if name == "subagents" => parent.and_then(Path::parent).and_then(Path::file_name),
        _ => path.file_stem(),
    };
    session.map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Every `.jsonl` under `dir`, at any depth (subagents log into `<session>/subagents/`),
/// changed since `since`, with its length. Symlinked folders aren't followed.
fn find_logs(dir: &Path, since: SystemTime, logs: &mut Vec<(PathBuf, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();

        if kind.is_dir() {
            find_logs(&path, since, logs);
            continue;
        }
        if !kind.is_file() || path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.modified().is_ok_and(|modified| modified >= since) {
            logs.push((path, meta.len()));
        }
    }
}

/// Milliseconds since the epoch for `2026-10-02T11:20:31.123Z`.
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

    (ms >= 0.0).then_some(ms.round() as u64)
}

/// Year, month and day of a day count since the epoch (Howard Hinnant).
#[cfg(test)]
fn civil(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use serde_json::{json, Value};

    use super::*;

    fn iso(ms: u64) -> String {
        let (year, month, day) = civil((ms / DAY_MS) as i64);
        let rest = ms % DAY_MS;
        format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
    }

    /// An assistant line as Claude Code writes it, message text included.
    fn reply(at: u64, session: &str, id: &str, request: &str, model: &str, usage: Value) -> Value {
        json!({
            "type": "assistant",
            "timestamp": iso(at),
            "sessionId": session,
            "requestId": request,
            "message": { "id": id, "model": model, "role": "assistant", "content": [{ "type": "text", "text": "private words" }], "usage": usage },
        })
    }

    fn sidechain(mut line: Value) -> Value {
        line["isSidechain"] = json!(true);
        line
    }

    fn write(path: &Path, lines: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(path, text).unwrap();
    }

    fn row<'a>(today: &'a SpendToday, model: &str) -> &'a ModelSpend {
        today.models.iter().find(|row| row.model == model).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn one_response_counts_once_whatever_lines_and_files_repeat_it() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("-Users-me-paddock");
        let now = now_ms();
        let opus = "claude-opus-5-5";

        // A subagent log, read before its parent: its replay of m1 counts until m1 shows up.
        let replay = sidechain(reply(now, "s1", "m1", "r9", opus, json!({ "input_tokens": 10, "output_tokens": 50, "cache_read_input_tokens": 9000 })));
        let own = sidechain(reply(now, "s1", "m4", "r4", "claude-sonnet-5", json!({ "input_tokens": 7, "output_tokens": 0 })));
        write(&project.join("s1/subagents/agent-a.jsonl"), &[replay, own]);

        let mut spend = Spend::new(tmp.path().to_path_buf());
        spend.refresh_at(now);
        assert_eq!(spend.today().tokens.cache_read, 9000);

        write(
            &project.join("s1.jsonl"),
            &[
                // Streaming writes one response as several lines, the counts filling in.
                reply(now, "s1", "m1", "r1", opus, json!({ "input_tokens": 10, "output_tokens": 1 })),
                reply(now, "s1", "m1", "r1", opus, json!({ "input_tokens": 10, "output_tokens": 50 })),
                reply(now, "s1", "m1", "r1", opus, json!({ "input_tokens": 10, "output_tokens": 50 })),
                reply(now, "s1", "m2", "r2", "<synthetic>", json!({ "input_tokens": 1000, "output_tokens": 0 })),
                json!({ "type": "user", "timestamp": iso(now), "message": { "role": "user", "content": [{ "type": "text", "text": "usage" }] } }),
                reply(
                    now,
                    "s1",
                    "m3",
                    "r3",
                    opus,
                    json!({ "input_tokens": 5, "output_tokens": 5, "iterations": [
                        { "type": "message", "model": null, "input_tokens": 5, "output_tokens": 5 },
                        { "type": "advisor_message", "model": "claude-fable-5-1", "input_tokens": 100, "output_tokens": 10 },
                    ] }),
                ),
            ],
        );
        spend.refresh_at(now);
        // A resumed session copies the response into its own log, here an early partial line.
        write(&project.join("s2.jsonl"), &[reply(now, "s2", "m1", "r1", opus, json!({ "input_tokens": 10, "output_tokens": 1 }))]);
        spend.refresh_at(now);

        let today = spend.today();
        assert_eq!((row(&today, opus).tokens.input, row(&today, opus).tokens.output), (15, 55));
        assert_eq!(row(&today, "claude-fable-5-1").tokens.total, 110);
        assert_eq!(row(&today, "claude-sonnet-5").tokens.total, 7);
        assert_eq!(today.tokens.cache_read, 0, "the sidechain replay went");
        assert_eq!(today.tokens.total, 187);
        assert_eq!(today.models.len(), 3, "the synthetic reply isn't a model");
    }

    /// Each session gets its own tokens and cost, its subagents' included: their lines carry the
    /// session's id, or in an older log only the folder they're in tells it.
    #[test]
    fn tokens_go_to_their_session_subagents_included() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("-Users-me-paddock");
        let now = now_ms();
        let opus = "claude-opus-5-5";
        let usage = |input: u64, output: u64| json!({ "input_tokens": input, "output_tokens": output });
        let anonymous = |mut line: Value| {
            line.as_object_mut().unwrap().remove("sessionId");
            line
        };

        write(&project.join("s1.jsonl"), &[reply(now, "s1", "m1", "r1", opus, usage(1_000_000, 0))]);
        write(
            &project.join("s1/subagents/agent-a.jsonl"),
            &[
                sidechain(reply(now, "s1", "m2", "r2", "claude-haiku-4-5", usage(0, 1_000_000))),
                sidechain(anonymous(reply(now, "s1", "m3", "r3", "claude-haiku-4-5", usage(10, 0)))),
            ],
        );
        write(&project.join("s2.jsonl"), &[anonymous(reply(now, "s2", "m4", "r4", "claude-opus-5-6", usage(7, 0)))]);

        let mut spend = Spend::new(tmp.path().to_path_buf());
        spend.refresh_at(now);
        let sessions = spend.by_session();

        let mut ids: Vec<&String> = sessions.keys().collect();
        ids.sort();
        assert_eq!(ids, ["s1", "s2"], "a subagent's log is no session of its own");

        let s1 = &sessions["s1"];
        assert_eq!((s1.tokens.input, s1.tokens.output, s1.tokens.total), (1_000_010, 1_000_000, 2_000_010));
        assert!(close(s1.cost_usd, 4.0 + 5.0 + 0.00001), "{}", s1.cost_usd);
        assert!(s1.priced);
        assert_eq!(s1.models, ["claude-haiku-4-5", opus], "the costliest first");

        let s2 = &sessions["s2"];
        assert_eq!((s2.tokens.total, s2.cost_usd, s2.priced), (7, 0.0, false), "a model without a price leaves the cost incomplete");

        let total: u64 = sessions.values().map(|session| session.tokens.total).sum();
        assert_eq!(total, spend.today().tokens.total, "every token is some session's");
    }

    #[test]
    fn only_the_local_day_counts_and_midnight_starts_over() {
        let tmp = tempfile::tempdir().unwrap();
        let now = now_ms();
        let (start, end, date) = local_day(now);
        let sonnet = "claude-sonnet-4-5-20250929";
        let usage = |input: u64| json!({ "input_tokens": input, "output_tokens": 0 });

        write(
            &tmp.path().join("p/s.jsonl"),
            &[
                reply(start - 1, "s", "y1", "ry", sonnet, usage(1000)),
                reply(start, "s", "t1", "rt", sonnet, usage(1)),
                // Yesterday's response again, copied or replayed today, isn't today's use.
                reply(start + 1000, "s", "y1", "ry", sonnet, usage(1000)),
                sidechain(reply(start + 1000, "s", "y1", "r-side", sonnet, usage(1000))),
                reply(end, "s", "z1", "rz", sonnet, usage(100_000)),
            ],
        );

        let mut spend = Spend::new(tmp.path().to_path_buf());
        spend.refresh_at(now);
        let today = spend.today();
        assert_eq!(today.date, date);
        assert_eq!(today.tokens.input, 1);

        spend.refresh_at(end);
        let tomorrow = spend.today();
        assert_ne!(tomorrow.date, date);
        assert_eq!(tomorrow.tokens.total, 0);
        assert!(tomorrow.models.is_empty());
    }

    #[test]
    fn a_line_is_read_once_it_ends_and_a_shrunk_log_is_read_again() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("p/s.jsonl");
        let now = now_ms();
        let haiku = "claude-haiku-4-5";
        let long = reply(now, "s", "a", "ra", haiku, json!({ "input_tokens": 3, "output_tokens": 4 })).to_string();
        let (head, tail) = long.split_at(long.len() / 2);

        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(&log, head).unwrap();
        let mut spend = Spend::new(tmp.path().to_path_buf());
        spend.refresh_at(now);
        assert_eq!(spend.today().tokens.total, 0);

        fs::OpenOptions::new().append(true).open(&log).unwrap().write_all(format!("{tail}\n").as_bytes()).unwrap();
        spend.refresh_at(now);
        spend.refresh_at(now);
        assert_eq!(spend.today().tokens.total, 7);

        // Rewritten shorter: read from the start; what was counted stays, once.
        let short = reply(now, "s", "b", "rb", haiku, json!({ "input_tokens": 1 }));
        assert!(short.to_string().len() < long.len());
        write(&log, &[short]);
        spend.refresh_at(now);
        assert_eq!(spend.today().tokens.total, 8);
    }

    #[test]
    fn unknown_models_count_tokens_but_no_cost() {
        let tmp = tempfile::tempdir().unwrap();
        let now = now_ms();
        let million = json!({ "input_tokens": 1_000_000, "output_tokens": 0 });

        write(
            &tmp.path().join("p/s.jsonl"),
            &[
                reply(now, "s", "a", "ra", "claude-sonnet-4-5", million.clone()),
                reply(now, "s", "b", "rb", "claude-opus-5-6", million.clone()),
                reply(now, "s", "c", "rc", "claude-3-opus-20240229", million),
            ],
        );

        let mut spend = Spend::new(tmp.path().to_path_buf());
        spend.refresh_at(now);
        let today = spend.today();
        assert_eq!(today.tokens.input, 3_000_000);
        assert!(close(today.cost_usd, 3.0));
        assert_eq!(today.unpriced_models, ["claude-3-opus-20240229", "claude-opus-5-6"]);
        assert!(!row(&today, "claude-opus-5-6").priced);
        assert_eq!(row(&today, "claude-opus-5-6").cost_usd, 0.0);
    }

    #[test]
    fn cost_follows_cache_tiers_fast_mode_and_model_ids() {
        let million = |input, output, write_5m, write_1h, read| Tokens { input, output, write_5m, write_1h, read };
        let cost = |model: &str, tokens: Tokens, fast: bool| price_of(model).unwrap().cost(&tokens, fast);
        let m = 1_000_000;

        // Opus 5.5: 4 in, 20 out, 5 and 8 for 5m and 1h writes, 0.20 for reads.
        assert!(close(cost("claude-opus-5-5", million(m, m, m, m, m), false), 37.2));
        assert!(close(cost("claude-opus-5-5", million(m, 0, 0, 0, 0), true), 8.0));
        assert!(close(cost("claude-opus-4-6", million(m, 0, 0, 0, 0), true), 5.0), "Opus 4.6 bills fast mode at standard rates");
        assert!(close(cost("claude-fable-5-1", million(0, 0, 0, 0, m), false), 0.25));
        assert!(close(cost("claude-fable-5", million(0, 0, 0, 0, m), false), 1.0));
        assert!(close(cost("claude-sonnet-4-6", million(0, 0, 0, 0, m), false), 0.30));

        // A bare cache-write total is priced as five-minute writes.
        let bare: Usage = serde_json::from_value(json!({ "input_tokens": 0, "output_tokens": 0, "cache_creation_input_tokens": m })).unwrap();
        assert!(close(cost("claude-sonnet-4-5", Tokens::of(&bare), false), 3.75));
        let split: Usage = serde_json::from_value(json!({ "cache_creation_input_tokens": m, "cache_creation": { "ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": m } })).unwrap();
        assert!(close(cost("claude-sonnet-4-5", Tokens::of(&split), false), 6.0));

        // Ids carry dates, provider prefixes and context suffixes.
        assert!(close(cost("claude-opus-4-20250514", million(m, 0, 0, 0, 0), false), 15.0));
        assert!(close(cost("us.anthropic.claude-opus-4-1-20250805-v1:0", million(m, 0, 0, 0, 0), false), 15.0));
        assert!(close(cost("claude-sonnet-4-5-20250929[1m]", million(m, 0, 0, 0, 0), false), 3.0));
        assert!(close(cost("claude-3-5-haiku-20241022", million(0, m, 0, 0, 0), false), 4.0));
        assert!(price_of("claude-3-5-sonnet-20241022").is_none());
        assert!(price_of("claude-opus-5-5-1").is_none());
    }
}
