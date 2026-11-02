//! Codex's usage limits, for the Fuel page and the refill a board card waits for: what Codex
//! itself last heard of them, the `rate_limits` of the newest `token_count` event among an
//! account's rollouts (`$CODEX_HOME/sessions/**/rollout-*.jsonl`, their last lines only). Nothing
//! is asked of OpenAI and no token is read: the reading is as fresh as Codex's last request, and
//! the page says when that was.
//!
//! Of a line only the limits are read (`primary`: the 5-hour window, `secondary`: the week, each
//! a share used, its length and its refill; `plan_type`); everything else on it is skipped as it
//! is read. A file that isn't a rollout of the account's `sessions/` is never opened, whoever
//! names it: not `auth.json`, not the database, not a link (`is_rollout`). Whether an account is
//! signed in is told from its sign-in being there (`auth.json`, or Codex's Keychain item, asked
//! without its secret), never from what it holds.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::usage::{parse_rfc3339, Status, Usage, UsageState, Window};

/// How much of a rollout's end is read for its last limits: its last lines, never the whole file.
const TAIL: u64 = 512 * 1024;
/// Rollouts looked into for one reading, the newest first: the newest request is among them.
const MOST_FILES: usize = 16;
/// How far back a reading is looked for: a week's window and a day.
pub const SINCE_MS: u64 = 8 * 24 * 60 * 60 * 1000;
/// Whether an account without a reading is signed in is asked again after this long.
const SIGNED_IN_EVERY: Duration = Duration::from_secs(5 * 60);
/// The windows Codex names by their length in minutes: the 5-hour one and the week. Another
/// length is left out rather than shown as one of them.
const SESSION_MINUTES: u64 = 5 * 60;
const WEEK_MINUTES: u64 = 7 * 24 * 60;
/// The Keychain item Codex keeps a home's sign-in in, when it keeps it there.
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "Codex Auth";

/// The limits as one line had them.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// `five_hour` and `seven_day`, in that order, as Claude's are named (`usage.rs`), so the page
    /// draws both the same way.
    pub usage: Usage,
    /// `Plus`, `Pro`, `Team`…
    pub plan: Option<String>,
    /// When Codex wrote it: the time of its last request, in ms.
    pub at: i64,
}

impl Reading {
    /// The account's state with this reading, or none yet: signed in, nothing to show until
    /// Codex makes a request; signed out, nothing to show either way.
    pub fn state(reading: Option<&Reading>, signed_in: bool) -> UsageState {
        match reading {
            Some(reading) => UsageState { status: Status::Ready, usage: Some(reading.usage.clone()), plan: reading.plan.clone(), fetched_at: Some(reading.at), retry_at: None, error: None },
            None if signed_in => UsageState { status: Status::Ready, ..UsageState::loading() },
            None => UsageState { status: Status::SignedOut, ..UsageState::loading() },
        }
    }
}

/// A line, as far as it is read: its time, its kind, and of an event its kind and limits.
#[derive(Deserialize)]
struct Line {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    kind: Option<String>,
    rate_limits: Option<RateLimits>,
}

#[derive(Deserialize)]
struct RateLimits {
    limit_id: Option<String>,
    primary: Option<RawWindow>,
    secondary: Option<RawWindow>,
    plan_type: Option<String>,
}

#[derive(Deserialize)]
struct RawWindow {
    used_percent: Option<f64>,
    window_minutes: Option<u64>,
    /// Unix seconds.
    resets_at: Option<f64>,
}

/// Whether `path` is a rollout of the Codex home `home`: a plain file (never a link) named
/// `rollout-….jsonl` under its `sessions/` or `archived_sessions/`, with no `..` on the way.
/// Nothing else of a home is ever opened here.
pub fn is_rollout(home: &Path, path: &Path) -> bool {
    let named = path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"));
    let inside = path.starts_with(home.join("sessions")) || path.starts_with(home.join("archived_sessions"));
    let plain = !path.components().any(|part| matches!(part, Component::ParentDir));
    named && inside && plain && path.symlink_metadata().is_ok_and(|meta| meta.file_type().is_file())
}

/// The limits on one line of a rollout, if it is a `token_count` event that has them.
fn parse(text: &str) -> Option<Reading> {
    let line: Line = serde_json::from_str(text).ok()?;
    if line.kind.as_deref() != Some("event_msg") {
        return None;
    }
    let payload = line.payload?;
    if payload.kind.as_deref() != Some("token_count") {
        return None;
    }
    let limits = payload.rate_limits?;
    // Codex's own limit; another one (a model's of its own, one day) isn't the plan's.
    if limits.limit_id.as_deref().is_some_and(|id| id != "codex") {
        return None;
    }
    let at = parse_rfc3339(line.timestamp.as_deref()?)?;

    let mut windows: Vec<Window> = Vec::new();
    for window in [limits.primary, limits.secondary].into_iter().flatten() {
        let Some(used) = window.used_percent.filter(|used| used.is_finite()) else {
            continue;
        };
        let id = match window.window_minutes {
            Some(SESSION_MINUTES) => "five_hour",
            Some(WEEK_MINUTES) => "seven_day",
            _ => continue,
        };
        if windows.iter().any(|known| known.id == id) {
            continue;
        }
        let resets_at = window.resets_at.filter(|at| at.is_finite() && *at > 0.0).map(|at| (at * 1000.0) as i64);
        windows.push(Window { id: id.into(), name: None, used, resets_at });
    }
    windows.sort_by_key(|window| window.id != "five_hour");
    Some(Reading { usage: Usage { windows, extra: None }, plan: limits.plan_type.as_deref().and_then(plan_name), at })
}

/// `plus` → `Plus`, `pro` → `Pro`, `enterprise` → `Enterprise`; a word that isn't a plain plan
/// name is none.
fn plan_name(plan: &str) -> Option<String> {
    let plan = plan.trim();
    if plan.is_empty() || plan.len() > 32 || !plan.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    let words: Vec<String> = plan
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map(|first| first.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        })
        .collect();
    Some(words.join(" "))
}

/// The last limits in the file at `path`: its last lines only, from the end back.
fn last_reading(path: &Path) -> Option<Reading> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL).read_to_end(&mut bytes).ok()?;
    // Begun inside a line: that line is left out.
    let whole = if from > 0 { bytes.iter().position(|b| *b == b'\n').map_or(&bytes[..0], |at| &bytes[at + 1..]) } else { &bytes[..] };

    whole.split(|b| *b == b'\n').rev().find_map(|line| {
        let text = std::str::from_utf8(line).ok()?;
        // Only an event that can hold limits is parsed at all.
        (text.contains("token_count") && text.contains("rate_limits")).then(|| parse(text)).flatten()
    })
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = path.symlink_metadata().ok()?.modified().ok()?;
    modified.duration_since(UNIX_EPOCH).ok().map(|since| since.as_millis() as i64)
}

/// The readings of every account, kept between two looks: a rollout is read again only once it
/// changed, and an account without a reading is asked whether it is signed in now and then.
#[derive(Default)]
pub struct Readings {
    /// By rollout: its length and time as last read, and the limits it ended with.
    files: HashMap<PathBuf, (u64, Option<SystemTime>, Option<Reading>)>,
    /// By home: whether it was signed in, and when that was asked.
    signed_in: HashMap<PathBuf, (bool, SystemTime)>,
}

impl Readings {
    /// The newest limits among `files` (the home's rollouts, the newest first), each one read
    /// again only when it changed. What isn't a rollout of `home` is skipped unopened.
    pub fn read(&mut self, home: &Path, files: &[PathBuf]) -> Option<Reading> {
        let mut best: Option<Reading> = None;
        let mut seen: Vec<PathBuf> = Vec::new();
        for path in files.iter().filter(|path| is_rollout(home, path)).take(MOST_FILES) {
            // A file last written before the best reading so far can't hold a newer one.
            if best.as_ref().zip(modified_ms(path)).is_some_and(|(best, modified)| modified < best.at) {
                break;
            }
            let meta = path.symlink_metadata().ok();
            let stamp = (meta.as_ref().map_or(0, |meta| meta.len()), meta.and_then(|meta| meta.modified().ok()));
            let reading = match self.files.get(path) {
                Some((len, modified, reading)) if (*len, *modified) == stamp => reading.clone(),
                _ => {
                    let reading = last_reading(path);
                    self.files.insert(path.clone(), (stamp.0, stamp.1, reading.clone()));
                    reading
                }
            };
            seen.push(path.clone());
            if let Some(reading) = reading {
                if best.as_ref().is_none_or(|best| reading.at > best.at) {
                    best = Some(reading);
                }
            }
        }
        // What isn't looked at any more is let go: this home's files not seen this time.
        self.files.retain(|path, _| !path.starts_with(home) || seen.contains(path));
        best
    }

    /// Whether the account whose home is `home` is signed in, as last asked; asked again when
    /// `again` or after a while.
    pub fn signed_in(&mut self, home: &Path, again: bool) -> bool {
        let now = SystemTime::now();
        match self.signed_in.get(home) {
            Some((signed_in, at)) if !again && now.duration_since(*at).is_ok_and(|age| age < SIGNED_IN_EVERY) => *signed_in,
            _ => {
                let signed_in = signed_in(home);
                self.signed_in.insert(home.to_path_buf(), (signed_in, now));
                signed_in
            }
        }
    }

    /// Lets go of every home but `homes`.
    pub fn keep(&mut self, homes: &[PathBuf]) {
        self.files.retain(|path, _| homes.iter().any(|home| path.starts_with(home)));
        self.signed_in.retain(|home, _| homes.contains(home));
    }
}

/// Whether Codex holds a sign-in for the home `home`: its `auth.json` is there (its metadata, the
/// file never opened), or its Keychain item (asked for without its secret).
pub fn signed_in(home: &Path) -> bool {
    home.join("auth.json").symlink_metadata().is_ok() || keychain_item(home)
}

/// The account of Codex's Keychain item for the home `home`: `cli|` and the first 16 hex digits of
/// the SHA-256 of the home's path with links followed, as Codex names it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn keychain_account(home: &Path) -> String {
    let canonical = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let hex: String = digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect();
    format!("cli|{hex}")
}

#[cfg(target_os = "macos")]
fn keychain_item(home: &Path) -> bool {
    // Tests never ask the user's Keychain.
    if cfg!(test) {
        return false;
    }
    // Without `-w` or `-g` the item's attributes only, never its secret; nothing printed is kept.
    let mut command = crate::process::command("/usr/bin/security");
    command.args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", &keychain_account(home)]);
    crate::process::output_within(&mut command, Duration::from_secs(10)).is_ok_and(|output| output.status.success())
}

/// Elsewhere Codex keeps it in the system's own store, which isn't asked: `auth.json` alone tells.
#[cfg(not(target_os = "macos"))]
fn keychain_item(_home: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A `token_count` line as Codex 0.166 writes it: real counts and limits, from a recorded
    /// rollout (`fixtures/codex`).
    fn token_count(at: &str, session: (f64, u64, i64), week: Option<(f64, i64)>, plan: &str) -> String {
        let secondary = match week {
            Some((used, resets)) => format!(r#"{{"used_percent":{used},"window_minutes":10080,"resets_at":{resets}}}"#),
            None => "null".into(),
        };
        format!(
            r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":19592,"cached_input_tokens":12928,"cache_write_input_tokens":0,"output_tokens":278,"reasoning_output_tokens":0,"total_tokens":19870}},"last_token_usage":{{"input_tokens":19592,"cached_input_tokens":12928,"cache_write_input_tokens":0,"output_tokens":278,"reasoning_output_tokens":0,"total_tokens":19870}},"model_context_window":475000}},"rate_limits":{{"limit_id":"codex","limit_name":null,"primary":{{"used_percent":{},"window_minutes":{},"resets_at":{}}},"secondary":{secondary},"credits":{{"has_credits":false,"unlimited":false,"balance":"0"}},"individual_limit":null,"spend_control_reached":null,"plan_type":"{plan}","rate_limit_reached_type":null}}}}}}"#,
            session.0, session.1, session.2
        )
    }

    /// A home's sign-in is never what the reading reads, whatever names it: a lister that hands
    /// over `auth.json`, a link dressed as a rollout or a path climbing out of `sessions/` gets
    /// nothing read of them. Their content would show (a plan of their own) if it were.
    #[test]
    fn the_reading_never_opens_the_sign_in() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex");
        let day = home.join("sessions/2026/11/02");
        fs::create_dir_all(&day).unwrap();
        let marker = token_count("2026-11-02T13:00:00.000Z", (99.0, 300, 1_793_000_000), None, "MARKER");
        fs::write(home.join("auth.json"), format!("{marker}\n")).unwrap();
        let rollout = day.join("rollout-2026-11-02T10-00-00-a.jsonl");
        fs::write(&rollout, format!("{}\n", token_count("2026-11-02T10:00:00.000Z", (12.0, 300, 1_792_990_000), Some((40.0, 1_793_300_000)), "plus"))).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(home.join("auth.json"), day.join("rollout-2026-11-02T12-00-00-b.jsonl")).unwrap();

        let handed = [home.join("auth.json"), day.join("rollout-2026-11-02T12-00-00-b.jsonl"), day.join("../../../../auth.json"), rollout.clone()];
        let reading = Readings::default().read(&home, &handed).unwrap();
        assert_eq!(reading.plan.as_deref(), Some("Plus"));
        assert_eq!(reading.usage.windows.iter().map(|window| (window.id.as_str(), window.used)).collect::<Vec<_>>(), [("five_hour", 12.0), ("seven_day", 40.0)]);
        assert_eq!(reading.usage.windows[0].resets_at, Some(1_792_990_000_000));
        assert!(!is_rollout(&home, &home.join("auth.json")));
    }
}
