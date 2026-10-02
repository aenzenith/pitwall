//! A project's custom commands (workers, migrations, tests…). Same process handling as the dev
//! server: login shell, own process group, whole-tree stop, output per command. Workers marked
//! "keep running" come back after a crash under the same three-strikes rule.

use super::*;
use crate::resolve::{is_forbidden_command, strip_ansi};
use crate::settings::CustomCommand;

/// Commands are keyed per project; the unit separator can't appear in a path or an id.
pub(super) fn job_key(path: &str, id: &str) -> String {
    format!("{path}\u{1f}{id}")
}

pub(super) struct Job {
    run: u64,
    pid: u32,
    started: Instant,
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
            let status = if inner.job_stopping.contains(&key) {
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

        self.emit(CoreEvent::Output { path: path.to_string(), job: Some(id.to_string()), line });
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
        }
        self.record_pids();
        self.notify();
    }

    /// Runs a project's command. A run by hand starts a fresh output and restart series.
    pub fn run_command(self: &Arc<Self>, path: &str, id: &str) {
        self.lock().attempts.remove(&job_key(path, id));
        self.spawn_job(path, id, true);
    }

    fn spawn_job(self: &Arc<Self>, path: &str, id: &str, fresh: bool) {
        let Some(command) = self.custom_command(path, id) else {
            return;
        };
        let key = job_key(path, id);

        {
            let mut inner = self.lock();
            if inner.jobs.contains_key(&key) || inner.disposed {
                return;
            }
            if fresh {
                inner.output.insert(key.clone(), VecDeque::new());
            }
        }

        let failed = |code| JobResult { ok: false, code, stopped: false, finished_at: now_ms() };

        if is_forbidden_command(&command.command) {
            self.push_job_line(path, id, format!("[pitwall] {}", t!("core.log.refusedBuild", command = command.command)));
            return self.finish_job(path, id, failed(None));
        }

        let mut child = match spawn_shell(&command.command, path) {
            Ok(child) => child,
            Err(error) => {
                self.push_job_line(path, id, format!("[pitwall] {}", t!("core.issue.couldNotStart", error = error)));
                return self.finish_job(path, id, failed(None));
            }
        };

        let run = {
            let mut inner = self.lock();
            inner.next_run += 1;
            let run = inner.next_run;
            inner.jobs.insert(key, Job { run, pid: child.id(), started: Instant::now(), started_at: now_ms() });
            run
        };

        self.push_job_line(path, id, format!("$ {}", command.command));
        self.record_command(path, &command.name, "running");
        self.record_pids();
        self.notify();

        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
        {
            let core = Arc::clone(self);
            let (path, id) = (path.to_string(), id.to_string());

            thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                let mut bytes = Vec::new();

                while reader.read_until(b'\n', &mut bytes).unwrap_or(0) > 0 {
                    let line = strip_ansi(String::from_utf8_lossy(&bytes).trim_end_matches(['\n', '\r']));
                    bytes.clear();

                    let current = core.lock().jobs.get(&job_key(&path, &id)).is_some_and(|job| job.run == run);
                    if current {
                        core.push_job_line(&path, &id, line);
                    }
                }
            });
        }

        let core = Arc::clone(self);
        let (path, id) = (path.to_string(), id.to_string());

        thread::spawn(move || {
            let status = child.wait();
            core.on_job_exit(&path, &id, run, status.ok());
        });
    }

    fn on_job_exit(self: &Arc<Self>, path: &str, id: &str, run: u64, status: Option<std::process::ExitStatus>) {
        let key = job_key(path, id);
        let (planned, started, disposed) = {
            let mut inner = self.lock();
            if !inner.jobs.get(&key).is_some_and(|job| job.run == run) {
                return;
            }
            let job = inner.jobs.remove(&key);
            (inner.job_stopping.contains(&key), job.map(|j| j.started), inner.disposed)
        };

        let code = status.and_then(|s| s.code());
        let ok = code == Some(0);
        let text = code.map(|c| c.to_string()).unwrap_or_else(|| "-".into());

        let line = if planned { t!("core.log.stopped", code = text) } else { t!("core.log.exited", code = text) };
        self.push_job_line(path, id, format!("[pitwall] {line}"));
        self.finish_job(path, id, JobResult { ok: ok || planned, code, stopped: planned, finished_at: now_ms() });

        let keep_running = self.custom_command(path, id).is_some_and(|c| c.keep_running);

        if planned || disposed || !keep_running {
            return;
        }

        let Some(attempt) = self.claim_restart(&key, started.unwrap_or_else(Instant::now)) else {
            self.push_job_line(path, id, format!("[pitwall] {}", t!("core.log.gaveUpCommand", max = MAX_RESTARTS)));
            return;
        };

        self.push_job_line(path, id, format!("[pitwall] {}", t!("core.log.crashedRestarting", attempt = attempt, max = MAX_RESTARTS)));
        thread::sleep(RESTART_DELAY);

        let blocked = {
            let inner = self.lock();
            inner.disposed || inner.jobs.contains_key(&key)
        };

        if !blocked {
            self.spawn_job(path, id, false);
        }
    }

    /// Stops a command and its whole process tree.
    pub fn stop_command(self: &Arc<Self>, path: &str, id: &str) {
        let key = job_key(path, id);
        let Some((pid, run)) = ({
            let mut inner = self.lock();
            let found = inner.jobs.get(&key).map(|job| (job.pid, job.run));
            if found.is_some() {
                inner.job_stopping.insert(key.clone());
            }
            found
        }) else {
            return;
        };

        self.notify();
        kill_tree(pid, false);

        let gone = |core: &Self| !core.lock().jobs.get(&key).is_some_and(|job| job.run == run);

        if !wait_until(Duration::from_millis(600), || gone(self)) {
            kill_tree(pid, true);
            wait_until(Duration::from_secs(3), || gone(self));
        }

        {
            let mut inner = self.lock();
            if inner.jobs.get(&key).is_some_and(|job| job.run == run) {
                inner.jobs.remove(&key);
            }
            inner.job_stopping.remove(&key);
        }

        self.record_pids();
        self.notify();
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
                .filter(|c| c.with_server && inner.jobs.contains_key(&job_key(path, &c.id)))
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
