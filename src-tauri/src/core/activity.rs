//! The day's activity, for the "Today" timeline. Pitwall writes down what it already watches,
//! as it changes: Claude sessions (working, waiting on you, idle, with their names), dev servers
//! (running, stopped, crashed) and custom commands (started, finished). One file per local day
//! in the app's own folder. A day's summary is built from that file and the projects' `git log`.

use std::collections::BTreeMap;
use std::io::Write as _;

use serde::Deserialize;

use super::*;
use crate::claude::{SessionPhase, SessionState};

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
    /// `claude` | `server` | `command` | `app`
    kind: String,
    /// claude: working | waiting | idle · server: running | stopped | crashed ·
    /// command: running | ok | failed | stopped · app: start | stop | pause
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    /// A command's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl Event {
    fn new(t: u64, path: &str, kind: &str, state: &str) -> Self {
        Self { t, path: path.into(), kind: kind.into(), state: state.into(), session: None, title: None, name: None }
    }
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
}

impl Core {
    fn activity_dir(&self) -> Option<PathBuf> {
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
                recorder.sessions.insert(session.id.clone(), Seen { phase: session.phase, title, path: session.path.clone() });
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

    /// Our own server crashed; it may be running again before the next heartbeat looks.
    pub(super) fn record_crash(&self, path: &str) {
        self.recorder().servers.insert(path.to_string(), "crashed");
        self.record(&Event::new(now_ms(), path, "server", "crashed"));
    }

    /// A custom command started (`running`) or ended (`ok` | `failed` | `stopped`).
    pub(super) fn record_command(&self, path: &str, name: &str, state: &str) {
        self.record(&Event { name: Some(name.into()), ..Event::new(now_ms(), path, "command", state) });
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

        let mut projects: Vec<DayProject> = day
            .projects
            .into_iter()
            .filter(|(path, _)| !path.is_empty())
            .map(|(path, built)| {
                let name = names.get(&path).cloned().unwrap_or_else(|| folder_name(&path));
                built.finish(path, name)
            })
            .filter(|p| !(p.work.is_empty() && p.wait.is_empty() && p.server.is_empty() && p.crashes.is_empty() && p.commits.is_empty() && p.commands.is_empty()))
            .collect();

        let claude_time = |p: &DayProject| p.work.iter().chain(&p.wait).map(|span| span.end - span.start).sum::<u64>();
        projects.sort_by(|a, b| claude_time(b).cmp(&claude_time(a)).then(b.commits.len().cmp(&a.commits.len())).then(a.name.cmp(&b.name)));

        DaySummary { date: day_name(start), start, end, today: (start..end).contains(&now), now, projects }
    }
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

    fn finish(mut self, path: String, name: String) -> DayProject {
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
/// repository says you are.
fn day_commits(path: &str, start: u64, end: u64) -> Vec<DayCommit> {
    if !Path::new(path).join(".git").exists() {
        return Vec::new();
    }

    let email = Command::new("git")
        .args(["-C", path, "config", "user.email"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();

    let mut command = Command::new("git");
    command.args(["-C", path, "log", "--all", "--no-merges", "--fixed-strings", "--format=%ct%x1f%s"]);
    command.arg(format!("--since=@{}", start / 1000)).arg(format!("--until=@{}", end / 1000));
    if !email.is_empty() {
        command.arg(format!("--author={email}"));
    }

    let Ok(out) = command.output() else {
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

/// `YYYY-MM-DD` of `ms`, local time.
fn day_name(ms: u64) -> String {
    let (year, month, day) = local_date(ms);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The local midnight starting a `YYYY-MM-DD` day, and the next one.
fn day_bounds(date: &str) -> Option<(u64, u64)> {
    let mut parts = date.split('-').map(|part| part.parse::<i32>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let start = local_midnight(year, month, day)?;
    let end = local_midnight(year, month, day + 1)?;
    (end > start).then_some((start, end))
}

#[cfg(unix)]
fn local_date(ms: u64) -> (i32, i32, i32) {
    let seconds = (ms / 1000) as libc::time_t;
    // SAFETY: localtime_r only fills the struct it is handed.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds, &mut tm) };
    (tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

#[cfg(unix)]
fn local_midnight(year: i32, month: i32, day: i32) -> Option<u64> {
    // SAFETY: mktime only reads and normalises the struct it is handed (day 32 → next month).
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    tm.tm_isdst = -1;
    let seconds = unsafe { libc::mktime(&mut tm) };
    (seconds >= 0).then(|| seconds as u64 * 1000)
}

/// Without the C library's local time (Windows), days are counted in UTC.
#[cfg(not(unix))]
fn local_date(ms: u64) -> (i32, i32, i32) {
    let days = (ms / DAY_MS) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as i32, month as i32, day as i32)
}

#[cfg(not(unix))]
fn local_midnight(year: i32, month: i32, day: i32) -> Option<u64> {
    let (y, m) = if month <= 2 { (i64::from(year) - 1, i64::from(month) + 9) } else { (i64::from(year), i64::from(month) - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days >= 0).then(|| days as u64 * DAY_MS)
}
