//! One run: the checks before anything runs, the script, its gate, the prompt, the agent (a tab
//! that doesn't take the view, or the background with read-only tools), its end, the timeout,
//! retries, the file the user named, and keeping the computer awake while it goes.

use std::collections::{HashSet, VecDeque};
use std::io::{ErrorKind, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::TimeZone;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::super::board::{born_for, stand, tab_name, Stand, UNSEEN_MS};
use super::super::sessions::Running;
use super::super::signals::Signal;
use super::*;
use crate::start::new_uuid;

/// Of a script's output, the prompt gets the last this many bytes.
const PROMPT_SCRIPT_BYTES: usize = 32 * 1024;
/// Of a script's output, at most this much is held while it runs.
const LINES_HELD: usize = 4000;
const BYTES_HELD: usize = 256 * 1024;
/// How often a run looks at its process, its stop and its deadline; and at its tab's session.
const POLL: Duration = Duration::from_millis(100);
const FOLLOW: Duration = Duration::from_secs(1);
/// What is stopped is given this long to end before it is killed, then this long to go.
const GRACE: Duration = Duration::from_millis(600);
const KILL_WAIT: Duration = Duration::from_millis(300);

/// How one try of a run ended.
#[derive(Debug, Clone)]
struct End {
    result: RunResult,
    reason: Option<Reason>,
    code: Option<i32>,
    session: Option<String>,
    head: Option<String>,
    hash: Option<String>,
    file: Option<String>,
    script: bool,
    account: Option<String>,
}

impl End {
    fn new(result: RunResult, reason: Option<Reason>) -> Self {
        Self { result, reason, code: None, session: None, head: None, hash: None, file: None, script: false, account: None }
    }

    fn skipped(reason: Reason) -> Self {
        Self::new(RunResult::Skipped, Some(reason))
    }

    /// Stopped before its end: by its time (a failure), else by the user or a quit.
    fn halted(reason: Reason) -> Self {
        match reason {
            Reason::Timeout => Self::new(RunResult::Failed, Some(Reason::Timeout)),
            reason => Self::new(RunResult::Cut, Some(reason)),
        }
    }

    /// An exit code's end: 0 ok, else a failure.
    fn exited(code: Option<i32>) -> Self {
        let mut end = if code == Some(0) { Self::new(RunResult::Ok, None) } else { Self::new(RunResult::Failed, Some(Reason::Exit)) };
        end.code = code;
        end
    }
}

/// What a script came to.
enum Ran {
    Ended { code: Option<i32>, lines: Vec<String> },
    Stopped(Reason),
    NotStarted,
}

/// The last lines of a script, held while it runs.
#[derive(Default)]
struct Lines {
    lines: VecDeque<String>,
    bytes: usize,
}

impl Lines {
    fn push(&mut self, line: String) {
        let line = crate::resolve::strip_ansi(&line);
        self.bytes += line.len() + 1;
        self.lines.push_back(line);
        while self.lines.len() > LINES_HELD || self.bytes > BYTES_HELD {
            match self.lines.pop_front() {
                Some(old) => self.bytes -= old.len() + 1,
                None => break,
            }
        }
    }
}

/// A stream that counts itself in `ends` once it has ended, so the reader's end can be waited for.
struct Counted<R> {
    inner: R,
    ends: Arc<AtomicUsize>,
    done: bool,
}

impl<R: std::io::Read> std::io::Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf);
        let ended = match &read {
            Ok(0) => true,
            Err(error) => error.kind() != ErrorKind::Interrupted,
            Ok(_) => false,
        };
        if ended && !self.done {
            self.done = true;
            self.ends.fetch_add(1, Ordering::SeqCst);
        }
        read
    }
}

/// Of `codex exec --json`'s stdout, the first line alone: these two fields, nothing else.
#[derive(Deserialize)]
struct FirstLine {
    #[serde(rename = "type")]
    kind: String,
    thread_id: Option<String>,
}

/// The thread's id, from a `thread.started` line.
fn thread_started(line: &str) -> Option<String> {
    let first: FirstLine = serde_json::from_str(line).ok()?;
    (first.kind == "thread.started").then_some(first.thread_id?).filter(|id| is_session_id(id))
}

fn stopped(stop: &StopFlag) -> Option<Reason> {
    *stop.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The last `max` bytes of `text` (at a character's start), and whether that cut anything.
fn tail_of(text: &str, max: usize) -> (bool, &str) {
    if text.len() <= max {
        return (false, text);
    }
    let mut from = text.len() - max;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    (true, &text[from..])
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The folder's Git HEAD, as `git rev-parse HEAD` says it.
fn git_head(folder: &str) -> Option<String> {
    let mut command = crate::git::read_command(folder);
    command.args(["rev-parse", "HEAD"]);
    let output = process::output_within(&mut command, crate::git::LOOKUP_LIMIT).ok()?;
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && head.len() >= 7 && head.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(head)
}

/// Waits for `child` (leading the process group `pid`) to end, stopping the group when `stop`
/// says so or `deadline` passes. Its exit code (none when a signal ended it), or why it was
/// stopped. What it left running in its group is stopped too.
fn wait_child(child: &mut std::process::Child, pid: u32, deadline: Option<Instant>, stop: &StopFlag) -> Result<Option<i32>, Reason> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if process::group_alive(pid) {
                    process::stop_groups(&[pid], GRACE, KILL_WAIT);
                }
                return Ok(status.code());
            }
            Ok(None) => {}
            Err(_) => return Ok(None),
        }
        let reason = stopped(stop).or_else(|| deadline.filter(|deadline| Instant::now() >= *deadline).map(|_| Reason::Timeout));
        if let Some(reason) = reason {
            process::stop_groups(&[pid], GRACE, KILL_WAIT);
            let _ = child.wait();
            return Err(reason);
        }
        thread::sleep(POLL);
    }
}

/// A job's name made safe for a file's: letters, digits, `-` and `_`, spaces as `-`.
fn safe_name(name: &str) -> String {
    let safe: String = name.trim().chars().filter_map(|c| if c.is_whitespace() { Some('-') } else { (c.is_alphanumeric() || c == '-' || c == '_').then_some(c) }).collect();
    if safe.is_empty() { "lap".into() } else { safe }
}

/// Where a run's result goes: `template` filled (`{date}` or `{tarih}` 2026-11-04, `{time}` or
/// `{saat}` 0830, `{name}` or `{ad}` the job's name made safe), in one pass so that a name never
/// fills a token of its own; `~` the home folder; its folder resolved with links and `..` followed
/// must be there, under `home` and outside `refused` and the agents' and Pitwall's own folders.
/// None: refused.
pub(super) fn result_file(template: &str, name: &str, now: u64, home: &Path, refused: &[PathBuf]) -> Option<PathBuf> {
    let path = result_path(template, name, now, home)?;
    let file_name = path.file_name()?.to_os_string();
    let parent = path.parent()?.canonicalize().ok()?;
    let home = home.canonicalize().ok()?;
    if !parent.starts_with(&home) || refused_place(&parent, &home, refused) {
        return None;
    }
    Some(parent.join(file_name))
}

/// `template` filled and under the home folder as `result_file` makes it, nothing resolved: an
/// absolute path that names a file, or none.
pub(super) fn result_path(template: &str, name: &str, now: u64, home: &Path) -> Option<PathBuf> {
    let date = when::date_of(now).0;
    let time = chrono::Local.timestamp_millis_opt(i64::try_from(now).ok()?).single().map(|at| at.format("%H%M").to_string())?;
    let name = safe_name(name);
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        filled.push_str(&rest[..open]);
        let token = rest[open..].find('}').map(|close| &rest[open..=open + close]);
        let value = match token {
            Some("{date}" | "{tarih}") => Some(date.as_str()),
            Some("{time}" | "{saat}") => Some(time.as_str()),
            Some("{name}" | "{ad}") => Some(name.as_str()),
            _ => None,
        };
        match (token, value) {
            (Some(token), Some(value)) => {
                filled.push_str(value);
                rest = &rest[open + token.len()..];
            }
            _ => {
                filled.push('{');
                rest = &rest[open + 1..];
            }
        }
    }
    filled.push_str(rest);
    let path = expand_home(filled.trim(), home);
    (path.is_absolute() && path.file_name().is_some()).then_some(path)
}

/// Writes `text` into a new file at `path`, never over one that is there (a link too): ` (2)`,
/// ` (3)`… before its extension. Readable by others, as the user's own documents are.
pub(super) fn write_result(path: &Path, text: &str) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let extension = path.extension().map(|extension| extension.to_string_lossy().into_owned());
    for n in 1..=100 {
        let candidate = match (n, &extension) {
            (1, _) => path.to_path_buf(),
            (_, Some(extension)) => path.with_file_name(format!("{stem} ({n}).{extension}")),
            (_, None) => path.with_file_name(format!("{stem} ({n})")),
        };
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o644);
        match options.open(&candidate) {
            Ok(mut file) => return file.write_all(text.as_bytes()).ok().map(|_| candidate),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

impl Core {
    /// A run the tick or a button began (its record written): tried, tried again on a failure as
    /// its job says, and ended. On a thread of its own.
    pub(super) fn run_lap(self: &Arc<Self>, planned: Planned) {
        let begun = {
            let mut laps = self.laps();
            let stop = laps.going_of(&planned).map(|going| Arc::clone(&going.stop));
            let due = laps.record_mut(&planned.lap, &planned.run).map(|record| record.due);
            stop.zip(due)
        };
        let Some((stop, due)) = begun else {
            return;
        };
        loop {
            let lap = self.laps().lap(&planned.lap).cloned();
            let Some(lap) = lap else {
                return;
            };
            let end = self.lap_try(&planned, &lap, due, &stop);
            if end.result == RunResult::Failed {
                if let Some(retry) = lap.retry {
                    if self.lap_retry(&planned, retry, &stop) {
                        continue;
                    }
                }
            }
            // Taken over by the quit: nothing more of it is written.
            if stopped(&stop) != Some(Reason::Quit) {
                self.lap_finish(&planned, end);
            }
            return;
        }
    }

    /// Waits for the run's next try when it has one left: the same record, its `tries` one more,
    /// `retry` while it waits. False when it has none, or it was stopped meanwhile (then it is
    /// ended here).
    fn lap_retry(self: &Arc<Self>, planned: &Planned, retry: Retry, stop: &StopFlag) -> bool {
        let wait = {
            let mut laps = self.laps();
            let minute = laps.minute();
            let Some(record) = laps.record_mut(&planned.lap, &planned.run) else {
                return false;
            };
            if record.tries > u32::from(retry.times) {
                return false;
            }
            record.tries += 1;
            if let Some(going) = laps.going_of(planned) {
                going.phase = "retry";
                going.pgid = None;
            }
            laps.save_runs(&planned.lap);
            Duration::from_millis(minute * u64::from(retry.every_min))
        };
        self.laps_changed();
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if let Some(reason) = stopped(stop) {
                if reason != Reason::Quit {
                    self.lap_finish(planned, End::halted(reason));
                }
                return false;
            }
            thread::sleep(POLL);
        }
        true
    }

    /// One try: the checks, the script and its gate, the agent, what comes after.
    fn lap_try(self: &Arc<Self>, planned: &Planned, lap: &Lap, due: u64, stop: &StopFlag) -> End {
        let agent = lap.start.agent;
        self.lap_phase(planned, if lap.kind.has_script() { "script" } else { "agent" });

        // The checks, each a skipped run with nothing more read.
        if lap.kind.has_agent() && !self.agent_on(agent) {
            return End::skipped(Reason::AgentOff);
        }
        // The job as `lap_save` would take it, whatever brought it (a backup, a hand-written
        // `laps.json`), its folder resolved now: what runs, runs there.
        let mut checked = lap.clone();
        if let Err((_, reason)) = self.lap_refused(&mut checked, now_ms(), &self.laps_known(), Check::Run) {
            return End::skipped(reason);
        }
        let lap = &checked;
        let folder = lap.place.path().to_string();
        let script_dir = match &lap.place {
            // The project's working folder, resolved as the job's own folder is.
            Place::Project { path } => {
                let home = dirs::home_dir().unwrap_or_default();
                let work = self.work_dir(path).filter(|_| Path::new(path).is_dir()).and_then(|dir| dir.canonicalize().ok());
                match work.filter(|dir| dir.is_dir() && !refused_place(dir, &home, &self.laps_refused_roots())) {
                    Some(dir) => dir,
                    None => return End::skipped(Reason::NotFolder),
                }
            }
            Place::Folder { path } => PathBuf::from(path),
        };
        let line = if lap.kind.has_script() {
            match &lap.script {
                Some(Script::Text { text }) if !text.trim().is_empty() => text.clone(),
                // Only a listed project's saved command, one that doesn't ask first, is run: a
                // folder's job naming one (a backup's) runs none.
                Some(Script::Command { id }) => {
                    let command = match &lap.place {
                        Place::Project { path } => self.lock().settings.project(path).commands.into_iter().find(|command| command.id == *id),
                        Place::Folder { .. } => None,
                    };
                    match command {
                        Some(command) if !command.confirm => command.command,
                        _ => return End::skipped(Reason::AskFirst),
                    }
                }
                _ => return End::skipped(Reason::NotStarted),
            }
        } else {
            String::new()
        };
        if lap.kind.has_agent() {
            if let Some(guard) = lap.fuel {
                if let Some(end) = self.lap_fuel(planned, lap, guard, due, stop) {
                    return end;
                }
            }
        }
        let head = if lap.gate.git && Path::new(&folder).join(".git").exists() { git_head(&folder) } else { None };
        if let Some(head) = &head {
            let previous = self.laps().previous(&lap.id, &planned.run, |run| run.head.clone());
            if previous.as_ref() == Some(head) {
                return End { head: Some(head.clone()), ..End::skipped(Reason::Git) };
            }
        }

        if lap.awake {
            let hold = Awake::hold(&t!("core.laps.awake"));
            let mut laps = self.laps();
            if let Some(going) = laps.going_of(planned) {
                if going.awake.is_none() {
                    going.awake = hold;
                }
            }
        }
        let minute = self.laps().minute();
        let deadline = lap.timeout_min.map(|minutes| Instant::now() + Duration::from_millis(u64::from(minutes) * minute));

        // The script.
        let mut script: Option<(Option<i32>, Vec<String>)> = None;
        if lap.kind.has_script() {
            let (code, lines) = match self.lap_script(planned, &line, &script_dir, deadline, stop) {
                Ran::Ended { code, lines } => (code, lines),
                Ran::Stopped(reason) => return End { script: true, head, ..End::halted(reason) },
                Ran::NotStarted => return End { head, ..End::new(RunResult::Failed, Some(Reason::NotStarted)) },
            };
            {
                let laps = self.laps();
                if laps.going.get(&planned.lap).is_some_and(|going| going.run == planned.run) {
                    laps.store.save_output(&planned.lap, &planned.run, &line, &lines);
                }
            }
            if lap.kind == LapKind::Script {
                let text = Some(store::kept_output(&lines).join("\n"));
                let end = End { script: true, head, ..End::exited(code) };
                return self.lap_after(planned, lap, end, text);
            }
            let printed = lines.iter().any(|line| !line.trim().is_empty());
            let called = match lap.gate.script {
                ScriptGate::None => true,
                ScriptGate::OnFailure => code != Some(0),
                ScriptGate::OnOutput => printed,
            };
            if !called {
                return End { script: true, code, head, ..End::skipped(Reason::Gate) };
            }
            script = Some((code, lines));
        }

        // The agent, still on after the script and the fuel guard's wait: turned off meanwhile,
        // it is given nothing.
        if !self.agent_on(agent) {
            let code = script.as_ref().and_then(|(code, _)| *code);
            return End { script: script.is_some(), code, head, ..End::skipped(Reason::AgentOff) };
        }
        let prompt = self.lap_prompt(lap, &planned.run, script.as_ref());
        self.lap_phase(planned, "agent");
        let (end, said) = self.lap_agent(planned, lap, &prompt, deadline, stop);
        let end = End { script: script.is_some(), code: end.code.or(script.as_ref().and_then(|(code, _)| *code)), head, ..end };
        self.lap_after(planned, lap, end, said)
    }

    /// The fuel guard, on the reading of the account the job runs with (`fuel_of`): past its
    /// share, the run is skipped, or waits (`waiting`) for that account's next refill when that
    /// comes within the grace; with no reading of it, skipped, never run blind. The run's end when
    /// it doesn't go on.
    fn lap_fuel(&self, planned: &Planned, lap: &Lap, guard: FuelGuard, due: u64, stop: &StopFlag) -> Option<End> {
        let now = now_ms();
        let Some(fuel) = self.laps().fuel_of(lap).cloned() else {
            return Some(End::skipped(Reason::NoReading));
        };
        // A plan without a week's limit: nothing holds the run.
        let week = fuel.week?;
        if week <= f64::from(guard.over) {
            return None;
        }
        let grace = if lap.missed.catch_up { u64::from(lap.missed.grace_min) * 60_000 } else { 90_000 };
        let refill = fuel.refills.iter().copied().filter(|&at| at > now).min().filter(|&at| at <= due.max(now) + grace);
        let (FuelThen::Wait, Some(refill)) = (guard.then, refill) else {
            return Some(End::skipped(Reason::Fuel));
        };
        self.lap_phase(planned, "waiting");
        while now_ms() < refill {
            if let Some(reason) = stopped(stop) {
                return Some(End::halted(reason));
            }
            thread::sleep(POLL);
        }
        None
    }

    /// The script in the user's shell, its own process group, in `cwd`: its exit code and its
    /// last lines (ANSI taken out).
    fn lap_script(&self, planned: &Planned, line: &str, cwd: &Path, deadline: Option<Instant>, stop: &StopFlag) -> Ran {
        let Ok(mut child) = process::spawn_shell(line, &cwd.to_string_lossy()) else {
            return Ran::NotStarted;
        };
        let pid = child.id();
        self.lap_group(planned, Some(pid));
        let lines = Arc::new(Mutex::new(Lines::default()));
        let ends = Arc::new(AtomicUsize::new(0));
        let mut streams = 0;
        if let Some(stdout) = child.stdout.take() {
            let lines = Arc::clone(&lines);
            streams += 1;
            process::read_lines(Counted { inner: stdout, ends: Arc::clone(&ends), done: false }, move |line| lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(line));
        }
        if let Some(stderr) = child.stderr.take() {
            let lines = Arc::clone(&lines);
            streams += 1;
            process::read_lines(Counted { inner: stderr, ends: Arc::clone(&ends), done: false }, move |line| lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(line));
        }
        let waited = wait_child(&mut child, pid, deadline, stop);
        self.lap_group(planned, None);
        // What it printed last is read to its end, unless something it left holds the pipe.
        process::wait_until(Duration::from_secs(1), || ends.load(Ordering::SeqCst) >= streams);
        let lines: Vec<String> = lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).lines.iter().cloned().collect();
        match waited {
            Ok(code) => Ran::Ended { code, lines },
            Err(reason) => Ran::Stopped(reason),
        }
    }

    /// The prompt: the job's own, the script's output (its end, 32 KB at most), the previous
    /// run's result with "Süreklilik", and the line asking for a summary, as a card's (a
    /// one-word prompt is never alone).
    fn lap_prompt(&self, lap: &Lap, run: &str, script: Option<&(Option<i32>, Vec<String>)>) -> String {
        let mut text = lap.prompt.trim().to_string();
        if let Some((code, lines)) = script {
            let code = code.map_or_else(|| "-".to_string(), |code| code.to_string());
            text.push_str(&format!("\n\n{}\n", t!("core.laps.prompt.script", code = code)));
            let output = lines.join("\n");
            if output.trim().is_empty() {
                text.push_str(&t!("core.laps.prompt.noOutput"));
            } else {
                let (cut, tail) = tail_of(&output, PROMPT_SCRIPT_BYTES);
                if cut {
                    text.push_str(&t!("core.laps.cut"));
                    text.push('\n');
                }
                text.push_str(&format!("```\n{tail}\n```"));
            }
        }
        if lap.carry {
            if let Some(previous) = self.lap_carry(lap, run) {
                text.push_str(&format!("\n\n{}\n```\n{previous}\n```", t!("core.laps.prompt.previous")));
            }
        }
        // A run in the background only reads: what it found, not what it changed.
        let background = lap.place_run == Where::Background || cfg!(windows);
        text.push_str(&format!("\n\n{}", if background { t!("core.laps.prompt.summary") } else { t!("board.summaryPrompt") }));
        text
    }

    /// The previous run's result, read now: what its agent last said, else its script's output.
    fn lap_carry(&self, lap: &Lap, run: &str) -> Option<String> {
        let (session, agent, output) = {
            let laps = self.laps();
            let previous = laps.runs.get(&lap.id)?.iter().rev().find(|record| record.id != run && record.ended_at.is_some() && (record.session.is_some() || record.script))?;
            let output = if previous.session.is_none() { laps.store.read_output(&lap.id, &previous.id) } else { None };
            (previous.session.clone(), previous.agent.unwrap_or(Agent::Claude), output)
        };
        let text = match session {
            Some(session) => self.lap_message(agent, &session).0,
            None => output.map(|lines| lines.join("\n")),
        }?;
        (!text.trim().is_empty()).then(|| tail_of(&text, PROMPT_SCRIPT_BYTES).1.to_string())
    }

    /// The account the job's agent runs with: its own, else the one in use; one that was
    /// removed (or whose folder is gone) is the main one. Its id (none: the main one) and the
    /// folder it is given as.
    fn lap_account(&self, lap: &Lap) -> (Option<String>, Option<PathBuf>) {
        match lap.start.agent {
            Agent::Claude => {
                let inner = self.lock();
                match crate::accounts::start_with(&inner.settings.accounts, lap.account.as_deref()) {
                    Ok(Some(account)) => (Some(account.id.clone()), Some(PathBuf::from(&account.dir))),
                    _ => (None, None),
                }
            }
            Agent::Codex => match self.codex_start_with(lap.account.as_deref()) {
                Ok(Some((id, home))) => (Some(id), Some(home)),
                _ => (None, None),
            },
        }
    }

    /// The agent, in a tab or the background (or the tests' stand-in): its end, and what it said
    /// when the stand-in tells it.
    fn lap_agent(self: &Arc<Self>, planned: &Planned, lap: &Lap, prompt: &str, deadline: Option<Instant>, stop: &StopFlag) -> (End, Option<String>) {
        let agent = lap.start.agent;
        let (account, dir) = self.lap_account(lap);
        // Claude's session is Pitwall's own, made for this run before it starts.
        let session = (agent == Agent::Claude).then(new_uuid);
        // On Windows a tab's agent starts without its prompt (`terminal::claude_command`: cmd.exe
        // can't take text safely), so a lap's runs in the background there.
        let background = lap.place_run == Where::Background || cfg!(windows);
        let args = if background { lap.start.background_args(session.as_deref()) } else { None };

        let hook = self.laps().agent_hook.clone();
        let (end, said) = if let Some(hook) = hook {
            let said = hook(&Asked { lap: lap.id.clone(), agent, prompt: prompt.to_string(), background, args, session: session.clone() });
            let session = said.session.clone().or(session);
            self.lap_session(planned, session.clone(), None, agent);
            (End { session, ..End::exited(Some(said.code)) }, said.message)
        } else if background {
            (self.lap_background(planned, lap, args, prompt, session, dir.as_deref(), deadline, stop), None)
        } else {
            (self.lap_tab(planned, lap, prompt, session, account.as_deref(), deadline, stop), None)
        };
        (End { account, ..end }, said)
    }

    /// The agent with no tab: read-only (`Start::background_args`), with the account's folder,
    /// in the job's folder, its own process group; ended by its exit.
    #[allow(clippy::too_many_arguments)]
    fn lap_background(&self, planned: &Planned, lap: &Lap, args: Option<Vec<String>>, prompt: &str, session: Option<String>, dir: Option<&Path>, deadline: Option<Instant>, stop: &StopFlag) -> End {
        let agent = lap.start.agent;
        let not_started = || End::new(RunResult::Failed, Some(Reason::NotStarted));
        let (Some(args), Some(program)) = (args, process::terminal_program(agent.id())) else {
            return not_started();
        };
        let mut command = process::terminal_command(&program, &[]);
        command.args(&args).current_dir(lap.place.path()).stdin(Stdio::null()).stderr(Stdio::null());
        command.stdout(if agent == Agent::Codex { Stdio::piped() } else { Stdio::null() });
        // The prompt, on stdin (`Start::background_args`).
        command.stdin(Stdio::piped());
        process::without_session_vars(&mut command);
        command.env_remove(crate::accounts::ENV);
        match (agent, dir) {
            (Agent::Claude, Some(dir)) => {
                command.env(crate::accounts::ENV, dir);
            }
            (Agent::Codex, Some(home)) => {
                command.env("CODEX_HOME", home);
            }
            _ => {}
        }
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let Ok(mut child) = command.spawn() else {
            return not_started();
        };
        let pid = child.id();
        #[cfg(windows)]
        process::contain(pid);
        self.lap_group(planned, Some(pid));
        if session.is_some() {
            self.lap_session(planned, session.clone(), None, agent);
        }
        // Written by a thread of its own and closed, so the agent sees its end: a prompt longer
        // than the pipe holds would otherwise wait on an agent that hasn't read it yet.
        if let Some(mut stdin) = child.stdin.take() {
            let prompt = prompt.to_string();
            std::thread::spawn(move || {
                use std::io::Write;
                let _ = stdin.write_all(prompt.as_bytes());
            });
        }

        // Codex's thread, from its first line; every other line is read and dropped unparsed.
        let thread = Arc::new(Mutex::new(None::<String>));
        let read = Arc::new(AtomicUsize::new(0));
        if let Some(stdout) = child.stdout.take() {
            let thread = Arc::clone(&thread);
            let mut first = true;
            process::read_lines(Counted { inner: stdout, ends: Arc::clone(&read), done: false }, move |line| {
                if std::mem::take(&mut first) {
                    *thread.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = thread_started(&line);
                }
            });
        }
        let waited = wait_child(&mut child, pid, deadline, stop);
        self.lap_group(planned, None);
        // Its first line may still be on its way to the reader: read to the end, a second at most.
        if agent == Agent::Codex {
            process::wait_until(Duration::from_secs(1), || read.load(Ordering::SeqCst) >= 1);
        }
        let session = session.or_else(|| thread.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone());
        if session.is_some() && agent == Agent::Codex {
            self.lap_session(planned, session.clone(), None, agent);
        }
        let end = match waited {
            Ok(code) => End::exited(code),
            Err(reason) => End::halted(reason),
        };
        End { session, ..end }
    }

    /// The agent in a tab of the laps' own (`open_terminal_apart`): in no panel, shown in the run's
    /// dialog alone (`lap_terminal`), the job's last tab closed first once nothing runs in it.
    /// Ended once its session's turn is over by the board's rule, or its session ended; stopped by
    /// interrupting the agent, the tab's shell left. An agent never seen at work ten minutes after
    /// the give didn't start, as a card's in the board's line.
    #[allow(clippy::too_many_arguments)]
    fn lap_tab(self: &Arc<Self>, planned: &Planned, lap: &Lap, prompt: &str, session: Option<String>, account: Option<&str>, deadline: Option<Instant>, stop: &StopFlag) -> End {
        let agent = lap.start.agent;
        let folder = lap.place.path().to_string();
        self.lap_close_last_tab(&lap.id);
        let launch = match agent {
            Agent::Claude => Launch::Claude { prompt: Some(prompt.to_string()), start: lap.start, images: None, session: session.clone() },
            Agent::Codex => Launch::Codex { prompt: Some(prompt.to_string()), start: lap.start, images: None },
        };
        let account = account.unwrap_or(crate::accounts::MAIN);
        let given = now_ms();
        let Ok(view) = self.open_terminal_apart(&folder, &launch, tab_name(&lap.name), None, Some(account)) else {
            return End::new(RunResult::Failed, Some(Reason::NotStarted));
        };
        let tab = view.id;
        self.lap_session(planned, session.clone(), Some(tab), agent);
        self.laps().tabs.insert(lap.id.clone(), LastTab { tab, session: session.clone(), agent });

        let mut session = session;
        let (mut worked, mut ran) = (false, false);
        let mut permission: Option<String> = None;
        let mut look = Instant::now();
        loop {
            let reason = stopped(stop).or_else(|| deadline.filter(|deadline| Instant::now() >= *deadline).map(|_| Reason::Timeout));
            if let Some(reason) = reason {
                if reason != Reason::Quit {
                    self.lap_interrupt(tab);
                }
                return End { session, ..End::halted(reason) };
            }
            if Instant::now() < look {
                thread::sleep(POLL);
                continue;
            }
            look = Instant::now() + FOLLOW;

            let shells = self.terminal_shells();
            let Some(shell) = shells.iter().find(|(_, id, _)| *id == tab).map(|(pid, _, _)| *pid) else {
                // The user closed the tab.
                return End { session, ..if worked { End::new(RunResult::Ok, None) } else { End::halted(Reason::Stopped) } };
            };
            if !worked && now_ms().saturating_sub(given) > UNSEEN_MS {
                return End { session, ..End::new(RunResult::Failed, Some(Reason::NotStarted)) };
            }
            if session.is_none() {
                session = self.lap_codex_session(tab, shell, given, &folder, &shells);
                if session.is_some() {
                    self.lap_session(planned, session.clone(), Some(tab), agent);
                    if let Some(last) = self.laps().tabs.get_mut(&lap.id).filter(|last| last.tab == tab) {
                        last.session = session.clone();
                    }
                }
            }
            let Some(id) = session.clone() else {
                continue;
            };
            let running = self.running_of(agent);
            let scanned = self.lap_state(&id);
            if running.as_ref().is_some_and(|running| running.contains_key(&id)) && (agent == Agent::Claude || self.codex_tied_ids().contains(&id)) {
                ran = true;
            }
            let waits = scanned.as_ref().filter(|state| state.phase == SessionPhase::Waiting && state.turn.is_some_and(|turn| turn.kind == crate::claude::TurnKind::Permission)).and_then(|state| state.tool.clone());
            if waits != permission {
                permission = waits;
                self.laps_changed();
            }
            match stand(&id, scanned.as_ref(), running.as_ref(), ran) {
                Stand::Busy => worked = true,
                // A finished turn after the give is the run's work done, though it went unseen.
                Stand::Over(at) if worked || at.is_some_and(|at| at > given) => return End { session, ..End::new(RunResult::Ok, None) },
                Stand::Interrupted if worked => return End { session, ..End::halted(Reason::Stopped) },
                Stand::Ended => return End { session, ..End::new(RunResult::Ok, None) },
                _ => {}
            }
        }
    }

    /// The Codex session a lap's tab runs: the first one started in it soon after the give, as a
    /// card's; else, no process tying one to it, by the rollouts (`codex_adopt_for`), the
    /// sessions cards and laps hold left out.
    fn lap_codex_session(&self, tab: u64, shell: u32, given: u64, folder: &str, shells: &[(u32, u64, String)]) -> Option<String> {
        let running = self.codex_running()?;
        let first = running
            .iter()
            .filter(|(_, run)| run.lineage.contains(&shell))
            .filter_map(|(session, run)| Some((session.clone(), born_for(run, given)?)))
            .min_by_key(|(_, born)| *born)
            .map(|(session, _)| session);
        if first.is_some() {
            return first;
        }
        let mut held: HashSet<String> = self.board().cards.iter().filter_map(|card| card.session.clone()).collect();
        held.extend(self.lap_sessions());
        let mut links = HashMap::new();
        let running: HashMap<Agent, HashMap<String, Running>> = HashMap::from([(Agent::Codex, running)]);
        self.codex_adopt_for(&[(tab, given, folder.to_string())], &held, &mut links, &running, shells);
        links.remove(&tab).map(|(session, _)| session)
    }

    /// The job's last tab is let go before a new run's opens: closed when no dialog shows it and
    /// its agent is done (its turn over, or it ended), else kept loose until it is
    /// (`lap_sweep_tabs`). An agent's session stays open after its turn, so the job's earlier
    /// sessions would otherwise live on unseen until the quit.
    fn lap_close_last_tab(&self, lap: &str) {
        let last = self.laps().tabs.remove(lap);
        let Some(last) = last else {
            return;
        };
        if !self.terminal_shells().iter().any(|(_, id, _)| *id == last.tab) {
            return;
        }
        let shown = self.laps().shown.contains(&last.tab);
        if !shown && self.lap_tab_done(&last) {
            self.close_terminal(last.tab);
        } else {
            self.laps().loose.insert(last.tab, last);
        }
    }

    /// The agent in the laps' tab `last` is done: its session's turn is over, or it ended, by the
    /// board's rule; with nothing to tell it by, nothing but the tab's shell runs there.
    pub(super) fn lap_tab_done(&self, last: &LastTab) -> bool {
        let Some(session) = &last.session else {
            return !self.lap_tab_holds(last.tab);
        };
        let running = self.running_of(last.agent);
        let scanned = self.lap_state(session);
        match stand(session, scanned.as_ref(), running.as_ref(), true) {
            Stand::Over(_) | Stand::Interrupted | Stand::Ended => true,
            Stand::Busy => false,
            Stand::Unknown => !self.lap_tab_holds(last.tab),
        }
    }

    /// The loose tabs of the laps' (`Laps::loose`): each closed once no dialog shows it and its
    /// agent is done; one already gone forgotten.
    pub(in crate::core) fn lap_sweep_tabs(&self) {
        let loose: Vec<LastTab> = {
            let laps = self.laps();
            laps.loose.values().filter(|last| !laps.shown.contains(&last.tab)).cloned().collect()
        };
        if loose.is_empty() {
            return;
        }
        let open: HashSet<u64> = self.terminal_shells().iter().map(|(_, id, _)| *id).collect();
        for last in loose {
            if open.contains(&last.tab) && !self.lap_tab_done(&last) {
                continue;
            }
            let closed = {
                let mut laps = self.laps();
                // A dialog may have shown it since.
                !laps.shown.contains(&last.tab) && laps.loose.remove(&last.tab).is_some()
            };
            if closed && open.contains(&last.tab) {
                self.close_terminal(last.tab);
            }
        }
    }

    /// Stops the agent in tab `tab`, its shell left: Ctrl-C twice, 300 ms apart, then the
    /// foreground group's SIGTERM if it is still there.
    fn lap_interrupt(&self, tab: u64) {
        #[cfg(unix)]
        {
            let Some(group) = self.terminal_foreground(tab) else {
                return;
            };
            let Ok(group) = i32::try_from(group) else {
                return;
            };
            // SAFETY: a signal to the tab's foreground process group, which isn't its shell's.
            unsafe {
                libc::kill(-group, libc::SIGINT);
            }
            thread::sleep(Duration::from_millis(300));
            // SAFETY: as above.
            unsafe {
                libc::kill(-group, libc::SIGINT);
            }
            if !process::wait_until(Duration::from_secs(2), || !process::group_alive(group as u32)) {
                // SAFETY: as above.
                unsafe {
                    libc::kill(-group, libc::SIGTERM);
                }
            }
        }
        #[cfg(windows)]
        {
            self.write_terminal(tab, "\u{3}");
            thread::sleep(Duration::from_millis(300));
            self.write_terminal(tab, "\u{3}");
        }
    }

    /// After a good end: what the agent said (or the script printed) read once when the job
    /// needs it, hashed for "same as before", written into the job's file, and dropped.
    fn lap_after(&self, planned: &Planned, lap: &Lap, mut end: End, text: Option<String>) -> End {
        if end.result != RunResult::Ok || !(lap.gate.same_silent || lap.file.is_some()) {
            return end;
        }
        let text = match (text, &end.session) {
            (Some(text), _) => Some(text),
            (None, Some(session)) => self.lap_message(lap.start.agent, session).0,
            (None, None) => None,
        };
        let Some(text) = text else {
            return end;
        };
        let hash = sha256_hex(&text);
        if lap.gate.same_silent && self.laps().previous(&lap.id, &planned.run, |run| run.hash.clone()).as_ref() == Some(&hash) {
            end.result = RunResult::Silent;
        }
        end.hash = Some(hash);
        if let Some(template) = &lap.file {
            let home = dirs::home_dir().unwrap_or_default();
            let file = result_file(template, &lap.name, now_ms(), &home, &self.laps_refused_roots()).and_then(|path| write_result(&path, &text));
            end.file = file.map(|file| file.to_string_lossy().into_owned());
        }
        end
    }

    /// The run's end: its record, the job's failures in a row (and its pause after too many),
    /// both files written; then, with no guard held, what the job wants told.
    fn lap_finish(self: &Arc<Self>, planned: &Planned, end: End) {
        let now = now_ms();
        let told = {
            let mut laps = self.laps();
            if laps.quit || laps.going_of(planned).is_none() {
                return;
            }
            let going = laps.going.remove(&planned.lap);
            let Some(record) = laps.record_mut(&planned.lap, &planned.run) else {
                return;
            };
            record.ended_at = Some(now);
            record.result = Some(end.result);
            // Done or failed: the page shows it unread until its result is opened.
            record.unread = matches!(end.result, RunResult::Ok | RunResult::Failed);
            record.reason = end.reason;
            record.code = end.code;
            if end.session.is_some() {
                record.session = end.session.clone();
            }
            record.hash = end.hash.clone();
            if end.head.is_some() {
                record.head = end.head.clone();
            }
            record.file = end.file.clone();
            record.script |= end.script;
            record.tab = None;
            record.account = end.account.clone();
            let Some(lap) = laps.lap_mut(&planned.lap) else {
                return;
            };
            match end.result {
                RunResult::Failed => lap.streak += 1,
                RunResult::Ok | RunResult::Silent => lap.streak = 0,
                RunResult::Skipped | RunResult::Cut => {}
            }
            let paused = end.result == RunResult::Failed && lap.paused.is_none() && lap.pause_after.is_some_and(|after| lap.streak >= after);
            if paused {
                lap.paused = Some(PausedBy::Failures);
                lap.next_at = None;
            }
            let lap = lap.clone();
            if paused {
                laps.held.remove(&lap.id);
            }
            laps.save_runs(&planned.lap);
            laps.save();
            // The awake hold goes with it.
            drop(going);
            (lap, paused)
        };
        self.laps_changed();

        let (lap, paused) = told;
        let kind = match end.result {
            RunResult::Ok if lap.notify.done => Some(SignalKind::LapDone),
            RunResult::Failed if lap.notify.failed || paused => Some(SignalKind::LapFailed),
            _ => None,
        };
        let Some(kind) = kind else {
            return;
        };
        let path = lap.place.path().to_string();
        let name = match &lap.place {
            Place::Project { .. } => self.project_name(&path),
            Place::Folder { .. } => Path::new(&path).file_name().map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned()),
        };
        let agent = if lap.kind.has_agent() { lap.start.agent } else { Agent::Claude };
        self.signal(Signal { kind, path: path.clone(), name, detail: Some(lap.name.clone()), agent });
        // The job's own switch asked for a sound: a command's, or Claude's finished one while
        // commands play none (their default), so that the switch is never silent.
        if lap.notify.sound {
            self.sound(&[&path], |sounds| if sounds.command_done.is_empty() { &sounds.claude_finished } else { &sounds.command_done });
        }
    }

    /// The run's phase, told to the page when it changed.
    fn lap_phase(&self, planned: &Planned, phase: &'static str) {
        let changed = {
            let mut laps = self.laps();
            match laps.going_of(planned) {
                Some(going) if going.phase != phase => {
                    going.phase = phase;
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.laps_changed();
        }
    }

    /// The process group the run's script or background agent leads, while it does; written
    /// down for the cleanup after a crash, as a server's is (`peers.rs`).
    fn lap_group(&self, planned: &Planned, pgid: Option<u32>) {
        let known = self.laps().going_of(planned).map(|going| going.pgid = pgid).is_some();
        if known {
            self.record_pids();
        }
    }

    /// The run's session (and tab), on its record as soon as it is known.
    fn lap_session(&self, planned: &Planned, session: Option<String>, tab: Option<u64>, agent: Agent) {
        {
            let mut laps = self.laps();
            if laps.quit {
                return;
            }
            let Some(going) = laps.going_of(planned) else {
                return;
            };
            going.session = session.clone();
            going.tab = tab;
            going.agent = Some(agent);
            let Some(record) = laps.record_mut(&planned.lap, &planned.run) else {
                return;
            };
            record.session = session;
            record.tab = tab;
            record.agent = Some(agent);
            laps.save_runs(&planned.lap);
        }
        self.laps_changed();
    }
}
