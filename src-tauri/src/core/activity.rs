//! The day's activity, for the "Today" timeline. Pitwall writes down what it already watches,
//! as it changes: Claude sessions (working, waiting on you, idle, with their names), dev servers
//! (running, stopped, crashed) and custom commands (started, finished). One file per local day
//! in the app's own folder. A day's summary is built from that file and the projects' `git log`.
//! The day's Claude tokens go into the same file (`day_tokens.rs`): the week's limit as read at
//! the day's first reading and after a refill (`week`), and every few minutes the tokens in all
//! and per project with their share of the week (`spend`).

use std::collections::BTreeMap;
use std::io::Write as _;

use serde::Deserialize;

use super::*;
use crate::claude::{SessionPhase, SessionState};
use crate::clock::{day_name, local_midnight};

/// Day files older than this are deleted when the app starts.
const KEEP_DAYS: u64 = 90;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// A wait longer than this was left alone rather than waited on; it counts up to here.
const WAIT_LIMIT: u64 = 30 * 60 * 1000;
/// Heartbeats come every few seconds; a longer silence is the Mac asleep, not time spent.
const SILENCE: u64 = 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Event {
    t: u64,
    /// The project; empty for the app's own events.
    #[serde(default)]
    path: String,
    /// `claude` | `server` | `command` | `app` | `week` | `spend`
    kind: String,
    /// claude: working | waiting | idle · server: running | stopped | crashed ·
    /// command: running | ok | failed | stopped · app: start | stop | pause · week: start | reset ·
    /// spend: measured | estimated | unread (the week's share as `week` holds it)
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    /// A command's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// spend: the day's tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tokens: Option<u64>,
    /// spend: the share of the week's limit the day took, in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    week: Option<f64>,
    /// week, spend: the week's limit used as read, in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<f64>,
    /// week (reset): what the week's limit had used before it refilled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<f64>,
    /// week: when the week's limit refills, in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resets: Option<i64>,
    /// spend: each project's tokens and share of the week, by path; an empty path holds those of
    /// projects not listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projects: Option<BTreeMap<String, (u64, Option<f64>)>>,
}

impl Event {
    fn new(t: u64, path: &str, kind: &str, state: &str) -> Self {
        Self {
            t,
            path: path.into(),
            kind: kind.into(),
            state: state.into(),
            session: None,
            title: None,
            name: None,
            tokens: None,
            week: None,
            used: None,
            before: None,
            resets: None,
            projects: None,
        }
    }
}

/// The week's limit as read at the start of a stretch of the day: its first reading, or the
/// first after the limit refilled (`before` holds what it had used until then).
#[derive(Debug, Clone, PartialEq)]
pub struct WeekMark {
    pub t: u64,
    pub used: f64,
    pub before: Option<f64>,
    pub resets: Option<i64>,
}

/// A day's tokens as written down: in all and per project path (empty: projects not listed),
/// each with its share of the week's limit.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpendRecord {
    pub tokens: u64,
    pub week: Option<f64>,
    pub estimated: bool,
    pub used: Option<f64>,
    pub projects: BTreeMap<String, (u64, Option<f64>)>,
}

/// What was last written down, so only changes are.
#[derive(Default)]
pub(super) struct Recorder {
    sessions: HashMap<String, Seen>,
    servers: HashMap<String, &'static str>,
    /// When the last heartbeat came.
    beat: u64,
}

struct Seen {
    phase: SessionPhase,
    title: Option<String>,
    path: String,
    /// When it was first seen in this phase.
    since: u64,
}

/// What the recorder last saw of a session: its name and since when it is in its phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Sighting {
    pub phase: SessionPhase,
    pub title: Option<String>,
    pub since: u64,
}

/// One session's day, from the day's file alone (no git), for the Sessions page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionDay {
    /// The project it was written down under.
    pub path: String,
    pub title: Option<String>,
    pub work: u64,
    pub wait: u64,
    pub turns: u32,
    /// Its working and waiting spans, the earliest first: (start, end, working).
    pub spans: Vec<(u64, u64, bool)>,
    /// The end of its last span.
    pub end: u64,
}

impl SessionDay {
    /// The same session's part in a later day, added to this one: the sums of both, under the
    /// later one's name and project.
    fn add(&mut self, other: SessionDay) {
        self.work += other.work;
        self.wait += other.wait;
        self.turns += other.turns;
        self.spans.extend(other.spans);
        if other.end >= self.end {
            self.end = other.end;
            self.path = other.path;
            self.title = other.title.or(self.title.take());
        }
    }
}

/// The day files' events as last read, by the files' size and modification time: read again
/// only when one changed.
#[derive(Default)]
pub(super) struct DayCache {
    stamps: Vec<(String, Option<(std::time::SystemTime, u64)>)>,
    events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start: u64,
    pub end: u64,
    /// Claude's spans name their session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaySession {
    pub id: String,
    pub title: Option<String>,
    pub start: u64,
    pub end: u64,
    /// Milliseconds Claude worked, and waited on you.
    pub work: u64,
    pub wait: u64,
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayCommit {
    pub at: u64,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayCommand {
    pub name: String,
    pub runs: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayProject {
    pub path: String,
    pub name: String,
    pub work: Vec<Span>,
    pub wait: Vec<Span>,
    pub server: Vec<Span>,
    pub crashes: Vec<u64>,
    pub sessions: Vec<DaySession>,
    pub commits: Vec<DayCommit>,
    pub commands: Vec<DayCommand>,
    /// Claude's tokens in it that day; none when nothing is known of them.
    pub spend: Option<ProjectSpend>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    /// `YYYY-MM-DD`, local.
    pub date: String,
    /// The day's local midnight and the next one.
    pub start: u64,
    pub end: u64,
    pub today: bool,
    pub now: u64,
    pub projects: Vec<DayProject>,
    /// Claude's tokens that day, in all projects; none for a day before they were written down.
    pub spend: Option<DaySpend>,
}

impl Core {
    pub(super) fn activity_dir(&self) -> Option<PathBuf> {
        self.cfg.settings_file.parent().map(|dir| dir.join("activity"))
    }

    fn recorder(&self) -> MutexGuard<'_, Recorder> {
        self.activity.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn record(&self, event: &Event) {
        let Some(dir) = self.activity_dir() else {
            return;
        };
        let Ok(line) = serde_json::to_string(event) else {
            return;
        };

        let _ = fs::create_dir_all(&dir);
        let file = dir.join(format!("{}.jsonl", day_name(event.t)));
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(file, "{line}");
        }
    }

    /// The app started or stops (`start` | `stop`): what was going on ends here, and whatever
    /// is still going on when it comes back is written down again.
    pub fn record_app(&self, state: &str) {
        *self.recorder() = Recorder::default();
        self.record(&Event::new(now_ms(), "", "app", state));

        if state == "start" {
            self.sweep_activity();
        }
    }

    /// Each heartbeat: Claude sessions whose state or name changed (one gone from the scan went
    /// quiet) and servers, ours and peers', that started, stopped or crashed.
    pub(super) fn record_activity(&self, sessions: &[SessionState], projects: &[ProjectView]) {
        let now = now_ms();
        let mut events = Vec::new();

        {
            let mut recorder = self.recorder();

            if recorder.beat > 0 && now.saturating_sub(recorder.beat) > SILENCE {
                // Asleep: what was going on stopped with the last heartbeat.
                events.push(Event::new(recorder.beat, "", "app", "pause"));
                recorder.sessions.clear();
                recorder.servers.clear();
            }
            recorder.beat = now;

            let mut scanned = HashSet::new();
            for session in sessions {
                scanned.insert(session.id.as_str());
                let before = recorder.sessions.get(&session.id);
                // A session first met idle tells nothing worth keeping.
                let changed = before.map_or(session.phase != SessionPhase::Idle, |seen| {
                    seen.phase != session.phase || (session.title.is_some() && seen.title != session.title)
                });
                let title = session.title.clone().or_else(|| before.and_then(|seen| seen.title.clone()));

                if changed {
                    events.push(Event { session: Some(session.id.clone()), title: title.clone(), ..Event::new(now, &session.path, "claude", phase_name(session.phase)) });
                }
                let since = before.filter(|seen| seen.phase == session.phase).map_or(now, |seen| seen.since);
                recorder.sessions.insert(session.id.clone(), Seen { phase: session.phase, title, path: session.path.clone(), since });
            }

            recorder.sessions.retain(|id, seen| {
                if scanned.contains(id.as_str()) {
                    return true;
                }
                if seen.phase != SessionPhase::Idle {
                    events.push(Event { session: Some(id.clone()), ..Event::new(now, &seen.path, "claude", "idle") });
                }
                false
            });

            for project in projects {
                let state = match project.status {
                    "running" => "running",
                    "crashed" => "crashed",
                    "stopped" => "stopped",
                    _ => continue,
                };
                let before = recorder.servers.insert(project.path.clone(), state);
                // A server first met stopped tells nothing either.
                if before != Some(state) && !(before.is_none() && state == "stopped") {
                    events.push(Event::new(now, &project.path, "server", state));
                }
            }
        }

        for event in &events {
            self.record(event);
        }
    }

    /// What the recorder last saw of each session it follows (those of the listed projects).
    pub(super) fn sightings(&self) -> HashMap<String, Sighting> {
        self.recorder()
            .sessions
            .iter()
            .map(|(id, seen)| (id.clone(), Sighting { phase: seen.phase, title: seen.title.clone(), since: seen.since }))
            .collect()
    }

    /// Our own server crashed; it may be running again before the next heartbeat looks.
    pub(super) fn record_crash(&self, path: &str) {
        self.recorder().servers.insert(path.to_string(), "crashed");
        self.record(&Event::new(now_ms(), path, "server", "crashed"));
    }

    /// A custom command started (`running`) or ended (`ok` | `failed` | `stopped`).
    pub(super) fn record_command(&self, path: &str, name: &str, state: &str) {
        self.record(&Event { name: Some(name.into()), ..Event::new(now_ms(), path, "command", state) });
    }

    /// The week's limit at the start of a stretch of the day (`WeekMark`).
    pub fn record_week(&self, mark: &WeekMark) {
        let state = if mark.before.is_some() { "reset" } else { "start" };
        self.record(&Event { used: Some(mark.used), before: mark.before, resets: mark.resets, ..Event::new(mark.t, "", "week", state) });
    }

    /// The day's week marks, the earliest first, and the week's limit used as that day's file
    /// last has it.
    pub fn week_marks(&self, day: &str) -> (Vec<WeekMark>, Option<f64>) {
        let mut events = self.read_day(day);
        events.sort_by_key(|event| event.t);

        let mut marks = Vec::new();
        let mut last = None;
        for event in events {
            match (event.kind.as_str(), event.used) {
                ("week", Some(used)) => {
                    marks.push(WeekMark { t: event.t, used, before: event.before, resets: event.resets });
                    last = Some(used);
                }
                ("spend", Some(used)) => last = Some(used),
                _ => {}
            }
        }
        (marks, last)
    }

    /// The day's tokens as they stand at `t`, written into that moment's day.
    pub(super) fn record_spend(&self, t: u64, record: &SpendRecord) {
        let state = match (record.week, record.estimated) {
            (None, _) => "unread",
            (Some(_), true) => "estimated",
            (Some(_), false) => "measured",
        };
        self.record(&Event {
            tokens: Some(record.tokens),
            week: record.week,
            used: record.used,
            projects: Some(record.projects.clone()),
            ..Event::new(t, "", "spend", state)
        });
    }

    fn read_day(&self, day: &str) -> Vec<Event> {
        let Some(dir) = self.activity_dir() else {
            return Vec::new();
        };
        fs::read_to_string(dir.join(format!("{day}.jsonl")))
            .map(|text| text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
            .unwrap_or_default()
    }

    fn sweep_activity(&self) {
        let Some(dir) = self.activity_dir() else {
            return;
        };
        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };
        let oldest = day_name(now_ms().saturating_sub(KEEP_DAYS * DAY_MS));

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.strip_suffix(".jsonl").is_some_and(|day| day.len() == 10 && day < oldest.as_str()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// A day (`YYYY-MM-DD`, local; today when `None`): every project with something going on,
    /// with Claude's working and waiting spans per session, the servers' running spans, crashes,
    /// commands run and the commits made that day.
    pub fn day_summary(&self, date: Option<&str>) -> DaySummary {
        let now = now_ms();
        let (start, end) = date
            .and_then(day_bounds)
            .or_else(|| day_bounds(&day_name(now)))
            .unwrap_or((now - now % DAY_MS, now - now % DAY_MS + DAY_MS));
        let until = now.clamp(start, end);

        // The day before tells what was already going on at midnight.
        let mut events = self.read_day(&day_name(start.saturating_sub(1)));
        events.extend(self.read_day(&day_name(start)));
        events.sort_by_key(|event| event.t);

        let mut day = fold_day(&events, start, until);

        // Commits of every listed project, and of any that only shows up in the activity.
        let names: HashMap<String, String> = self.snapshot().projects.into_iter().map(|p| (p.path, p.name)).collect();
        if until > start {
            let mut paths: Vec<String> = names.keys().cloned().collect();
            paths.extend(day.projects.keys().filter(|path| !names.contains_key(*path)).cloned());
            for path in paths {
                let commits = day_commits(&path, start, until);
                if !commits.is_empty() {
                    day.project(&path).commits = commits;
                }
            }
        }

        // Claude's tokens: today's as the logs were last looked at, a day before's as its file
        // last has them.
        let today = (start..end).contains(&now);
        let (spend, mut parts) = if today {
            let owners: HashMap<String, String> =
                day.projects.iter().flat_map(|(path, built)| built.sessions.iter().map(move |session| (session.id.clone(), path.clone()))).collect();
            let listed: Vec<String> = names.keys().cloned().collect();
            self.tokens_today(&day_name(start), &owners, &listed)
        } else {
            spend_of(&events, start, end)
        };
        for path in parts.keys().filter(|path| !path.is_empty()) {
            day.project(path);
        }

        let mut projects: Vec<DayProject> = day
            .projects
            .into_iter()
            .filter(|(path, _)| !path.is_empty())
            .map(|(path, built)| {
                let name = names.get(&path).cloned().unwrap_or_else(|| folder_name(&path));
                let spend = parts.remove(&path);
                built.finish(path, name, spend)
            })
            .filter(|p| {
                !(p.work.is_empty() && p.wait.is_empty() && p.server.is_empty() && p.crashes.is_empty() && p.commits.is_empty() && p.commands.is_empty() && p.spend.is_none())
            })
            .collect();

        let claude_time = |p: &DayProject| p.work.iter().chain(&p.wait).map(|span| span.end - span.start).sum::<u64>();
        projects.sort_by(|a, b| claude_time(b).cmp(&claude_time(a)).then(b.commits.len().cmp(&a.commits.len())).then(a.name.cmp(&b.name)));

        DaySummary { date: day_name(start), start, end, today, now, projects, spend }
    }
}

impl Core {
    /// Today's Claude sessions, by id, from the day's file alone: no git, and the file is parsed
    /// again only once it changed (`cache`).
    pub(super) fn day_sessions(&self, cache: &mut DayCache) -> HashMap<String, SessionDay> {
        let now = now_ms();
        let Some((start, _)) = day_bounds(&day_name(now)) else {
            return HashMap::new();
        };
        let Some(dir) = self.activity_dir() else {
            return HashMap::new();
        };

        let days = [day_name(start.saturating_sub(1)), day_name(start)];
        let stamps: Vec<(String, Option<(std::time::SystemTime, u64)>)> = days
            .iter()
            .map(|day| {
                let meta = fs::metadata(dir.join(format!("{day}.jsonl"))).ok();
                (day.clone(), meta.and_then(|meta| Some((meta.modified().ok()?, meta.len()))))
            })
            .collect();

        if cache.stamps != stamps {
            let mut events: Vec<Event> = days.iter().flat_map(|day| self.read_day(day)).collect();
            events.sort_by_key(|event| event.t);
            *cache = DayCache { stamps, events };
        }

        session_days(&cache.events, start, now.max(start))
    }

    /// The Claude sessions of the day that starts at `start`, by id, as far as `until`: from
    /// that day's file and the day before's.
    pub(super) fn sessions_on(&self, start: u64, until: u64) -> HashMap<String, SessionDay> {
        let mut events = self.read_day(&day_name(start.saturating_sub(1)));
        events.extend(self.read_day(&day_name(start)));
        events.sort_by_key(|event| event.t);
        session_days(&events, start, until)
    }

    /// The Claude sessions of every day before today there is a file of, by id: each one's days
    /// summed. Today's part is the live list's (`sessions_view`), which goes on counting it, so
    /// it is left out here and the page adds the two. The hours those were made of are left out
    /// too, as several days don't share a day's axis.
    pub(super) fn sessions_ever(&self, now: u64) -> HashMap<String, SessionDay> {
        let mut days: Vec<String> = self
            .activity_dir()
            .and_then(|dir| fs::read_dir(dir).ok())
            .map(|entries| entries.flatten().filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".jsonl").map(str::to_string)).collect())
            .unwrap_or_default();
        days.sort();

        let mut found: HashMap<String, SessionDay> = HashMap::new();
        for (start, end) in days.iter().filter_map(|day| day_bounds(day)).filter(|(_, end)| *end <= now) {
            for (id, day) in self.sessions_on(start, end) {
                match found.get_mut(&id) {
                    Some(known) => known.add(day),
                    None => {
                        found.insert(id, day);
                    }
                }
            }
        }
        for day in found.values_mut() {
            day.spans.clear();
        }

        found
    }
}

/// The Claude sessions of the day that starts at `start`, by id, as far as `until`: each one's
/// working and waiting spans and their sums, from `events` (the day's and the day before's, in
/// order).
fn session_days(events: &[Event], start: u64, until: u64) -> HashMap<String, SessionDay> {
    let day = fold_day(events, start, until);
    let mut found = HashMap::new();

    for (path, built) in day.projects {
        for session in built.sessions.into_iter().filter(|s| s.work + s.wait > 0) {
            let mut spans: Vec<(u64, u64, bool)> = built
                .work
                .iter()
                .map(|span| (span, true))
                .chain(built.wait.iter().map(|span| (span, false)))
                .filter(|(span, _)| span.session.as_deref() == Some(session.id.as_str()))
                .map(|(span, working)| (span.start, span.end, working))
                .collect();
            spans.sort_unstable();

            let day = SessionDay {
                path: path.clone(),
                title: session.title,
                work: session.work,
                wait: session.wait,
                turns: session.turns,
                spans,
                end: session.end,
            };
            found.insert(session.id, day);
        }
    }

    found
}

/// A day's tokens as the last `spend` event between `start` and `end` has them: in all, and per
/// project path (empty: projects not listed).
fn spend_of(events: &[Event], start: u64, end: u64) -> (Option<DaySpend>, BTreeMap<String, ProjectSpend>) {
    let Some(event) = events.iter().rev().find(|event| event.kind == "spend" && (start..end).contains(&event.t)) else {
        return (None, BTreeMap::new());
    };
    let Some(tokens) = event.tokens else {
        return (None, BTreeMap::new());
    };

    let spend = DaySpend { tokens, week: event.week, estimated: event.state == "estimated" };
    let parts = event.projects.iter().flatten().map(|(path, (tokens, week))| (path.clone(), ProjectSpend { tokens: *tokens, week: *week })).collect();
    (Some(spend), parts)
}

/// The day's Claude, server and command events folded into each project's spans, cut to the
/// day (`start`) and to `until`; what was already going on at midnight comes from the day
/// before. Commits aren't in it.
fn fold_day(events: &[Event], start: u64, until: u64) -> Day {
    let mut day = Day { start, until, projects: BTreeMap::new() };
    let mut sessions: HashMap<String, Open> = HashMap::new();
    let mut servers: HashMap<String, u64> = HashMap::new();
    let mut commands: HashSet<(String, String)> = HashSet::new();

    for event in events.iter().filter(|event| event.t < until) {
        let counts = event.t >= start;

        match event.kind.as_str() {
            "claude" => {
                let Some(id) = event.session.as_deref() else {
                    continue;
                };
                let phase = match event.state.as_str() {
                    "working" => SessionPhase::Working,
                    "waiting" => SessionPhase::Waiting,
                    _ => SessionPhase::Idle,
                };

                // The same state again only brings a new name.
                if sessions.get(id).is_none_or(|open| open.phase != phase) {
                    if let Some(open) = sessions.remove(id) {
                        day.close_session(id, &open, event.t);
                        if open.phase == SessionPhase::Working && counts {
                            day.project(&open.path).session(id).turns += 1;
                        }
                    }
                    if phase != SessionPhase::Idle {
                        sessions.insert(id.to_string(), Open { path: event.path.clone(), phase, since: event.t });
                    }
                }
                if let Some(title) = &event.title {
                    day.project(&event.path).session(id).title = Some(title.clone());
                }
            }
            "server" => {
                if let Some(since) = servers.remove(&event.path) {
                    day.close_server(&event.path, since, event.t);
                }
                match event.state.as_str() {
                    "running" => {
                        servers.insert(event.path.clone(), event.t);
                    }
                    "crashed" if counts => day.project(&event.path).crashes.push(event.t),
                    _ => {}
                }
            }
            "command" => {
                let Some(name) = &event.name else {
                    continue;
                };
                let key = (event.path.clone(), name.clone());
                // A run that never got going (refused, could not start) ends without a start.
                let started = if event.state == "running" { !commands.insert(key) } else { commands.remove(&key) };
                if counts {
                    let command = day.project(&event.path).command(name);
                    if !started {
                        command.runs += 1;
                    }
                    if event.state == "failed" {
                        command.failed += 1;
                    }
                }
            }
            "app" => {
                for (id, open) in sessions.drain() {
                    day.close_session(&id, &open, event.t);
                }
                for (path, since) in servers.drain() {
                    day.close_server(&path, since, event.t);
                }
                commands.clear();
            }
            _ => {}
        }
    }

    for (id, open) in sessions.drain() {
        day.close_session(&id, &open, until);
    }
    for (path, since) in servers.drain() {
        day.close_server(&path, since, until);
    }

    day
}

/// A Claude session's state since it was written down.
struct Open {
    path: String,
    phase: SessionPhase,
    since: u64,
}

/// A day while it is being put together; spans are cut to the day (and to now, today).
struct Day {
    start: u64,
    until: u64,
    projects: BTreeMap<String, Building>,
}

impl Day {
    fn project(&mut self, path: &str) -> &mut Building {
        self.projects.entry(path.to_string()).or_default()
    }

    fn close_session(&mut self, id: &str, open: &Open, at: u64) {
        let at = if open.phase == SessionPhase::Waiting { at.min(open.since + WAIT_LIMIT) } else { at };
        let (from, to) = (open.since.max(self.start), at.min(self.until));
        if to <= from || open.phase == SessionPhase::Idle {
            return;
        }

        let project = self.project(&open.path);
        let span = Span { start: from, end: to, session: Some(id.to_string()) };
        let working = open.phase == SessionPhase::Working;
        if working {
            project.work.push(span);
        } else {
            project.wait.push(span);
        }

        let session = project.session(id);
        session.start = session.start.min(from);
        session.end = session.end.max(to);
        if working {
            session.work += to - from;
        } else {
            session.wait += to - from;
        }
    }

    fn close_server(&mut self, path: &str, since: u64, at: u64) {
        let (from, to) = (since.max(self.start), at.min(self.until));
        if to > from {
            self.project(path).server.push(Span { start: from, end: to, session: None });
        }
    }
}

/// A project's day while it is being put together.
#[derive(Default)]
struct Building {
    work: Vec<Span>,
    wait: Vec<Span>,
    server: Vec<Span>,
    crashes: Vec<u64>,
    sessions: Vec<DaySession>,
    commits: Vec<DayCommit>,
    commands: Vec<DayCommand>,
}

impl Building {
    fn session(&mut self, id: &str) -> &mut DaySession {
        let at = self.sessions.iter().position(|s| s.id == id).unwrap_or_else(|| {
            self.sessions.push(DaySession { id: id.into(), title: None, start: u64::MAX, end: 0, work: 0, wait: 0, turns: 0 });
            self.sessions.len() - 1
        });
        &mut self.sessions[at]
    }

    fn command(&mut self, name: &str) -> &mut DayCommand {
        let at = self.commands.iter().position(|c| c.name == name).unwrap_or_else(|| {
            self.commands.push(DayCommand { name: name.into(), runs: 0, failed: 0 });
            self.commands.len() - 1
        });
        &mut self.commands[at]
    }

    fn finish(mut self, path: String, name: String, spend: Option<ProjectSpend>) -> DayProject {
        // Only sessions that did something that day; the earliest first.
        self.sessions.retain(|s| s.work + s.wait > 0);
        self.sessions.sort_by_key(|s| s.start);
        for spans in [&mut self.work, &mut self.wait, &mut self.server] {
            spans.sort_by_key(|s| s.start);
        }
        self.commands.retain(|c| c.runs > 0);
        self.commits.sort_by_key(|c| c.at);

        DayProject {
            path,
            name,
            work: self.work,
            wait: self.wait,
            server: self.server,
            crashes: self.crashes,
            sessions: self.sessions,
            commits: self.commits,
            commands: self.commands,
            spend,
        }
    }
}

fn phase_name(phase: SessionPhase) -> &'static str {
    match phase {
        SessionPhase::Working => "working",
        SessionPhase::Waiting => "waiting",
        SessionPhase::Idle => "idle",
    }
}

/// The commits made in `path` between `start` and `end`, on any branch, by whoever the
/// repository says you are. Not what `git stash` keeps: its "index on …" and "untracked files
/// on …" are commits too, reached from `refs/stash` alone.
fn day_commits(path: &str, start: u64, end: u64) -> Vec<DayCommit> {
    if !Path::new(path).join(".git").exists() {
        return Vec::new();
    }

    let email = process::output_within(crate::git::read_command(path).args(["config", "user.email"]), crate::git::LOOKUP_LIMIT)
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();

    let mut command = crate::git::read_command(path);
    command.args(["log", "--exclude=refs/stash", "--all", "--no-merges", "--fixed-strings", "--format=%ct%x1f%s"]);
    command.arg(format!("--since=@{}", start / 1000)).arg(format!("--until=@{}", end / 1000));
    if !email.is_empty() {
        command.arg(format!("--author={email}"));
    }

    let Ok(out) = process::output_within(&mut command, crate::git::LOG_LIMIT) else {
        return Vec::new();
    };

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (seconds, subject) = line.split_once('\u{1f}')?;
            let at = seconds.trim().parse::<u64>().ok()? * 1000;
            (start..end).contains(&at).then(|| DayCommit { at, subject: subject.to_string() })
        })
        .collect()
}

/// The local midnight starting a `YYYY-MM-DD` day, and the next one.
pub(super) fn day_bounds(date: &str) -> Option<(u64, u64)> {
    let mut parts = date.split('-').map(|part| part.parse::<i32>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let start = local_midnight(year, month as u32, day as u32)?;
    let end = local_midnight(year, month as u32, day as u32 + 1)?;
    (end > start).then_some((start, end))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    use super::*;

    /// Pitwall reads every listed project's Git state on its own, and its log for the day's
    /// summary: a program the repository's own `.git/config` names (an fsmonitor hook, a
    /// signature program for `log.showSignature`) never runs for that, and both reads still
    /// answer.
    #[test]
    fn a_read_runs_no_program_the_repository_names() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("shop");
        let path = repo.to_str().unwrap();
        let git = |args: &[&str], input: Option<&str>| {
            let mut command = Command::new("git");
            command.arg("-C").arg(&repo).args(args).env("GIT_CONFIG_NOSYSTEM", "1").env("HOME", tmp.path());
            command.stdin(Stdio::piped()).stdout(Stdio::piped());
            let mut child = command.spawn().unwrap();
            if let Some(input) = input {
                child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            }
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        fs::create_dir_all(&repo).unwrap();
        git(&["init", "-q"], None);
        fs::write(repo.join("README.md"), "shop\n").unwrap();
        git(&["add", "README.md"], None);

        // A commit of today's with a signature on it, made without signing anything.
        let email = "dev@example.com";
        let when = now_ms() / 1000 - 60;
        let tree = git(&["write-tree"], None);
        let signature = "-----BEGIN PGP SIGNATURE-----\n \n ZHVtbXk=\n -----END PGP SIGNATURE-----";
        let body = format!("tree {tree}\nauthor Dev <{email}> {when} +0000\ncommitter Dev <{email}> {when} +0000\ngpgsig {signature}\n\nSigned work\n");
        let commit = git(&["hash-object", "-t", "commit", "-w", "--stdin"], Some(&body));
        git(&["update-ref", "HEAD", &commit], None);

        let program = |name: &str| {
            let marker = tmp.path().join(format!("{name}-ran"));
            let script = tmp.path().join(name);
            fs::write(&script, format!("#!/bin/sh\necho ran >> '{}'\nexit 0\n", marker.display())).unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            (script, marker)
        };
        let (fsmonitor, fsmonitor_ran) = program("fsmonitor");
        let (gpg, gpg_ran) = program("gpg");
        let mut config = fs::OpenOptions::new().append(true).open(repo.join(".git").join("config")).unwrap();
        write!(config, "[user]\n\temail = {email}\n[core]\n\tfsmonitor = {}\n[log]\n\tshowSignature = true\n[gpg]\n\tprogram = {}\n", fsmonitor.display(), gpg.display()).unwrap();
        drop(config);

        assert!(crate::git::status(path).is_some_and(|info| info.changes == 0), "the state is read");
        let commits = day_commits(path, (when - 3600) * 1000, (when + 3600) * 1000);
        assert_eq!(commits.iter().map(|commit| commit.subject.as_str()).collect::<Vec<_>>(), ["Signed work"], "the log is read");
        let ran: Vec<&str> = [("fsmonitor hook", &fsmonitor_ran), ("signature program", &gpg_ran)].into_iter().filter(|(_, marker)| marker.exists()).map(|(name, _)| name).collect();
        assert!(ran.is_empty(), "the repository's own programs ran: {ran:?}");
    }
}
