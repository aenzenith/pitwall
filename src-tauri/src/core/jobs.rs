//! A project's custom commands (workers, migrations, tests…). Same supervision as the dev server
//! (`supervise.rs`): login shell, own process group, whole-tree stop, output per command. Workers
//! marked "keep running" come back after a crash under the same three-strikes rule.

use super::*;
use super::signals::Signal;
use crate::resolve::strip_ansi;
use crate::settings::CustomCommand;

/// Commands are keyed per project; the unit separator can't appear in a path or an id.
pub(super) fn job_key(path: &str, id: &str) -> String {
    format!("{path}\u{1f}{id}")
}

pub(super) struct Job {
    pub(super) run: u64,
    pub(super) pid: u32,
    pub(super) started: Instant,
    pub(super) started_at: u64,
}

impl Job {
    pub(super) fn pid(&self) -> u32 {
        self.pid
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub ok: bool,
    pub code: Option<i32>,
    /// Stopped by the user rather than finishing on its own.
    pub stopped: bool,
    pub finished_at: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandView {
    pub id: String,
    pub name: String,
    pub command: String,
    pub keep_running: bool,
    pub confirm: bool,
    pub with_server: bool,
    /// `running` | `busy` | `ok` | `failed` | `idle`
    pub status: &'static str,
    pub started_at: Option<u64>,
    pub result: Option<JobResult>,
    /// Marked "ask first", it started with the server and wasn't run: it waits for the user, as
    /// whoever started the server left it (`Core::hold_companions`).
    pub held: Option<Origin>,
}

pub(super) fn command_views(inner: &Inner, path: &str) -> Vec<CommandView> {
    // Only while its server runs here or is on its way back; one a terminal asks about is the
    // terminal's.
    let server = inner.runs.contains_key(path) || inner.supervisor.pending(path);
    let held = |command: &CustomCommand| {
        let by = inner.held.get(path)?.iter().find(|(id, _)| *id == command.id)?.1;
        (server && command.confirm && command.with_server && by != Origin::Terminal).then_some(by)
    };

    inner
        .settings
        .project(path)
        .commands
        .into_iter()
        .map(|command| {
            let key = job_key(path, &command.id);
            let job = inner.jobs.get(&key);
            let result = inner.job_results.get(&key).cloned();
            let status = if inner.stopping.contains(&key) {
                "busy"
            } else if job.is_some() {
                "running"
            } else {
                match &result {
                    Some(r) if r.stopped => "idle",
                    Some(r) if r.ok => "ok",
                    Some(_) => "failed",
                    None => "idle",
                }
            };

            CommandView {
                held: held(&command),
                id: command.id,
                name: command.name,
                command: command.command,
                keep_running: command.keep_running,
                confirm: command.confirm,
                with_server: command.with_server,
                status,
                started_at: job.map(|j| j.started_at),
                result,
            }
        })
        .collect()
}

/// The command `id` no longer waits for the user.
fn unhold(inner: &mut Inner, path: &str, id: &str) {
    if let Some(held) = inner.held.get_mut(path) {
        held.retain(|(held, _)| held != id);
        if held.is_empty() {
            inner.held.remove(path);
        }
    }
}

impl Core {
    fn custom_command(&self, path: &str, id: &str) -> Option<CustomCommand> {
        self.lock().settings.project(path).commands.into_iter().find(|c| c.id == id)
    }

    fn push_job_line(&self, path: &str, id: &str, line: String) {
        self.push_output(path, Some(id), line);
    }

    fn finish_job(self: &Arc<Self>, path: &str, id: &str, result: JobResult) {
        let state = if result.stopped { "stopped" } else if result.ok { "ok" } else { "failed" };
        let disposed = {
            let mut inner = self.lock();
            inner.job_results.insert(job_key(path, id), result);
            inner.disposed
        };
        // Quitting stops every command; that's no result of theirs.
        if let Some(command) = self.custom_command(path, id).filter(|_| !disposed) {
            self.record_command(path, &command.name, state);

            // Told of, and heard, as Settings says (`signals.rs`).
            if state != "stopped" {
                let kind = if state == "ok" { SignalKind::CommandDone } else { SignalKind::CommandFailed };
                self.signal(Signal { kind, path: path.to_string(), name: self.project_name(path), detail: Some(command.name) });
            }
        }
        self.record_pids();
        self.notify();
        self.deps_job_done(path, id);
    }

    /// Runs a project's command. A run by hand starts a fresh restart series, and is the user's
    /// yes to one that was held for them. False as for `spawn_job_line`, and for a command that
    /// isn't saved.
    pub fn run_command(self: &Arc<Self>, path: &str, id: &str) -> bool {
        {
            let mut inner = self.lock();
            inner.supervisor.reset_attempts(&job_key(path, id));
            unhold(&mut inner, path, id);
        }
        self.spawn_job(path, id)
    }

    pub(super) fn spawn_job(self: &Arc<Self>, path: &str, id: &str) -> bool {
        let Some(command) = self.custom_command(path, id) else {
            return false;
        };
        self.spawn_job_line(path, id, Some(&command.name), &command.command)
    }

    /// Runs `command_line` as the project's job `id`: a custom command (`name` is its name, for
    /// the day's timeline) or one of the app's own (`dependencies.rs`). False when it never got
    /// going (another start of it, a stop, quitting); otherwise its end reaches `finish_job`.
    pub(super) fn spawn_job_line(self: &Arc<Self>, path: &str, id: &str, name: Option<&str>, command_line: &str) -> bool {
        let unit = Unit::Command(path.to_string(), id.to_string());
        let Some(start) = self.begin_start(&unit) else {
            return false;
        };
        let key = job_key(path, id);

        let failed = |code| JobResult { ok: false, code, stopped: false, finished_at: now_ms() };

        let child = match process::spawn_shell(command_line, path) {
            Ok(child) => child,
            Err(error) => {
                self.push_job_line(path, id, format!("[pitwall] {}", t!("core.issue.couldNotStart", error = error)));
                drop(start);
                self.finish_job(path, id, failed(None));
                return true;
            }
        };

        let adopted = self.adopt(start, child, |inner, run, pid| {
            inner.jobs.insert(key, Job { run, pid, started: Instant::now(), started_at: now_ms() });
        });
        let Some((run, child)) = adopted else {
            return false;
        };

        // The run before stays in the output, above these.
        self.mark_run(path, Some(id));
        self.push_job_line(path, id, format!("$ {command_line}"));
        if let Some(name) = name {
            self.record_command(path, name, "running");
        }
        self.record_pids();
        self.notify();
        self.watch(unit, run, child);
        true
    }

    /// A line of run `run`'s output; a line of an earlier run is dropped.
    pub(super) fn on_job_line(&self, path: &str, id: &str, run: u64, line: String) {
        let line = strip_ansi(&line);
        let current = self.lock().jobs.get(&job_key(path, id)).is_some_and(|job| job.run == run);
        if current {
            self.push_job_line(path, id, line);
        }
    }

    /// The command's shell exited (taken out of `jobs` already); `planned` when a stop did it.
    pub(super) fn on_job_exit(
        self: &Arc<Self>,
        path: &str,
        id: &str,
        started: Instant,
        planned: bool,
        disposed: bool,
        status: Option<std::process::ExitStatus>,
    ) {
        let code = status.and_then(|s| s.code());
        let ok = code == Some(0);

        // The output says how it ended already (`output::end_line`, kept as the run was taken out).
        self.flush_output();
        self.finish_job(path, id, JobResult { ok: ok || planned, code, stopped: planned, finished_at: now_ms() });

        let keep_running = self.custom_command(path, id).is_some_and(|c| c.keep_running);

        if planned || disposed || !keep_running {
            return;
        }

        let unit = Unit::Command(path.to_string(), id.to_string());
        let Some(attempt) = self.claim_restart(&unit, started) else {
            self.push_job_line(path, id, format!("[pitwall] {}", t!("core.log.gaveUpCommand", max = MAX_RESTARTS)));
            return;
        };

        self.push_job_line(path, id, format!("[pitwall] {}", t!("core.log.crashedRestarting", attempt = attempt, max = MAX_RESTARTS)));
        self.restart_later(&unit);
    }

    /// Stops a command and its whole process tree; a restart it waits for is called off too.
    pub fn stop_command(self: &Arc<Self>, path: &str, id: &str) {
        self.stop_unit(&Unit::Command(path.to_string(), id.to_string()));
    }

    /// Commands marked "start with the dev server". Those marked "ask first" too are never run
    /// by a start, whoever started it: they are held for the user (`hold_companions`).
    pub(super) fn start_server_companions(self: &Arc<Self>, path: &str, by: Origin) {
        self.hold_companions(path, by);
        let ids: Vec<String> = self.lock().settings.project(path).commands.into_iter().filter(|c| c.with_server && !c.confirm).map(|c| c.id).collect();

        for id in ids {
            self.run_command(path, &id);
        }
    }

    /// The server just started (`by` started it): its commands marked "ask first" that start
    /// with it wait for the user, in place of those that waited before. Kept in memory only,
    /// until the user runs or skips one, or the server stops. One that runs already, or waits
    /// to come back, doesn't wait; nor does anything of a server that didn't start.
    pub(super) fn hold_companions(&self, path: &str, by: Origin) {
        {
            let mut inner = self.lock();
            let inner = &mut *inner;
            if !inner.runs.contains_key(path) {
                return;
            }
            let held: Vec<(String, Origin)> = inner
                .settings
                .project(path)
                .commands
                .into_iter()
                .filter(|c| {
                    let key = job_key(path, &c.id);
                    c.with_server && c.confirm && !inner.jobs.contains_key(&key) && !inner.supervisor.pending(&key)
                })
                .map(|c| (c.id, by))
                .collect();
            if held.is_empty() {
                inner.held.remove(path);
            } else {
                inner.held.insert(path.to_string(), held);
            }
        }
        self.notify();
    }

    /// The user's no to a command held for them: it goes, and nothing runs.
    pub fn skip_held(&self, path: &str, id: &str) {
        unhold(&mut self.lock(), path, id);
        self.notify();
    }

    /// The commands held as `by` left them, taken out: a terminal asks about them itself.
    pub(super) fn take_held(&self, path: &str, by: Origin) -> Vec<CustomCommand> {
        let mut inner = self.lock();
        let Some(held) = inner.held.get_mut(path) else {
            return Vec::new();
        };
        let (taken, kept): (Vec<_>, Vec<_>) = std::mem::take(held).into_iter().partition(|(_, origin)| *origin == by);
        *held = kept;
        if held.is_empty() {
            inner.held.remove(path);
        }
        let commands = inner.settings.project(path).commands;
        taken.into_iter().filter_map(|(id, _)| commands.iter().find(|c| c.id == id).cloned()).collect()
    }

    pub(super) fn stop_server_companions(self: &Arc<Self>, path: &str) {
        let ids: Vec<String> = {
            let inner = self.lock();
            inner
                .settings
                .project(path)
                .commands
                .into_iter()
                .filter(|c| {
                    let key = job_key(path, &c.id);
                    c.with_server && (inner.jobs.contains_key(&key) || inner.supervisor.pending(&key))
                })
                .map(|c| c.id)
                .collect()
        };

        for id in ids {
            self.stop_command(path, &id);
        }
    }

    pub fn command_output(&self, path: &str, id: &str) -> Vec<String> {
        self.read_output(&job_key(path, id))
    }
}
