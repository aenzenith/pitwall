//! A project's custom commands (workers, migrations, tests…). Same supervision as the dev server
//! (`supervise.rs`): login shell, own process group, whole-tree stop, output per command. Workers
//! marked "keep running" come back after a crash under the same three-strikes rule.

use super::*;
use crate::resolve::{is_forbidden_command, strip_ansi};
use crate::settings::CustomCommand;

/// Commands are keyed per project; the unit separator can't appear in a path or an id.
pub(super) fn job_key(path: &str, id: &str) -> String {
    format!("{path}\u{1f}{id}")
}

pub(super) struct Job {
    pub(super) run: u64,
    pub(super) pid: u32,
    pub(super) started: Instant,
    started_at: u64,
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
}

pub(super) fn command_views(inner: &Inner, path: &str) -> Vec<CommandView> {
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

impl Core {
    fn custom_command(&self, path: &str, id: &str) -> Option<CustomCommand> {
        self.lock().settings.project(path).commands.into_iter().find(|c| c.id == id)
    }

    fn push_job_line(&self, path: &str, id: &str, line: String) {
        let key = job_key(path, id);
        {
            let mut inner = self.lock();
            let lines = inner.output.entry(key).or_default();
            lines.push_back(line.clone());
            while lines.len() > OUTPUT_LINES {
                lines.pop_front();
            }
        }

        self.send_output(path, Some(id), line);
    }

    fn finish_job(&self, path: &str, id: &str, result: JobResult) {
        let state = if result.stopped { "stopped" } else if result.ok { "ok" } else { "failed" };
        let disposed = {
            let mut inner = self.lock();
            inner.job_results.insert(job_key(path, id), result);
            inner.disposed
        };
        // Quitting stops every command; that's no result of theirs.
        if let Some(command) = self.custom_command(path, id).filter(|_| !disposed) {
            self.record_command(path, &command.name, state);

            let sound = self.settings().sounds.command_done;
            if state != "stopped" && !sound.is_empty() {
                self.emit(CoreEvent::Sound(sound));
            }
        }
        self.record_pids();
        self.notify();
    }

    /// Runs a project's command. A run by hand starts a fresh output and restart series.
    pub fn run_command(self: &Arc<Self>, path: &str, id: &str) {
        self.lock().supervisor.reset_attempts(&job_key(path, id));
        self.spawn_job(path, id, true);
    }

    pub(super) fn spawn_job(self: &Arc<Self>, path: &str, id: &str, fresh: bool) {
        let Some(command) = self.custom_command(path, id) else {
            return;
        };
        let unit = Unit::Command(path.to_string(), id.to_string());
        let Some(start) = self.begin_start(&unit) else {
            return;
        };
        let key = job_key(path, id);

        if fresh {
            self.lock().output.insert(key.clone(), VecDeque::new());
        }

        let failed = |code| JobResult { ok: false, code, stopped: false, finished_at: now_ms() };

        if is_forbidden_command(&command.command) {
            self.push_job_line(path, id, format!("[pitwall] {}", t!("core.log.refusedBuild", command = command.command)));
            drop(start);
            return self.finish_job(path, id, failed(None));
        }

        let child = match process::spawn_shell(&command.command, path) {
            Ok(child) => child,
            Err(error) => {
                self.push_job_line(path, id, format!("[pitwall] {}", t!("core.issue.couldNotStart", error = error)));
                drop(start);
                return self.finish_job(path, id, failed(None));
            }
        };

        let adopted = self.adopt(start, child, |inner, run, pid| {
            inner.jobs.insert(key, Job { run, pid, started: Instant::now(), started_at: now_ms() });
        });
        let Some((run, child)) = adopted else {
            return;
        };

        self.push_job_line(path, id, format!("$ {}", command.command));
        self.record_command(path, &command.name, "running");
        self.record_pids();
        self.notify();
        self.watch(unit, run, child);
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
        let text = code.map(|c| c.to_string()).unwrap_or_else(|| "-".into());

        let line = if planned { t!("core.log.stopped", code = text) } else { t!("core.log.exited", code = text) };
        self.push_job_line(path, id, format!("[pitwall] {line}"));
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

    /// Commands marked "start with the dev server".
    pub(super) fn start_server_companions(self: &Arc<Self>, path: &str) {
        let ids: Vec<String> = self.lock().settings.project(path).commands.into_iter().filter(|c| c.with_server).map(|c| c.id).collect();

        for id in ids {
            self.run_command(path, &id);
        }
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
        self.lock().output.get(&job_key(path, id)).map(|lines| lines.iter().cloned().collect()).unwrap_or_default()
    }
}
