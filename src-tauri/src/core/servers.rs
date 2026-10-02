//! Dev servers run here: the launch (script, package manager, a free port), their output read
//! for the address and for errors, crash restarts, and the health probe.

use super::*;
use crate::ports::is_port_served;
use crate::resolve::{
    build_command, detect_error_line, detect_package_manager, detect_port_conflict, extract_local_url, has_script,
    is_forbidden_script, parse_vite_port, port_from_url,
};

/// Whether an issue ends a start the user waits on: a refused or failed start, or a final
/// give-up, does; a crash or a hang on its way back doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueEnd {
    Final,
    Retrying,
}

pub(super) struct Run {
    pub(super) id: u64,
    pub(super) pid: u32,
    pub(super) name: String,
    /// The command line it runs.
    pub(super) command: String,
    pub(super) started: Instant,
    pub(super) started_at: u64,
    pub(super) port: Option<u16>,
    pub(super) url: Option<String>,
    buffer: String,
    open_url: bool,
    pub(super) settled: bool,
}

impl Core {
    /// A start by hand resets the restart counter.
    pub fn start(self: &Arc<Self>, path: &str, open_url: bool) {
        self.lock().supervisor.reset_attempts(path);
        let open = open_url && self.lock().settings.open_url_on_start;
        self.launch(path, open, Vec::new());

        if self.lock().runs.contains_key(path) {
            self.start_server_companions(path);
        }
    }

    pub fn restart(self: &Arc<Self>, path: &str) {
        self.stop(path);
        self.start(path, false);
    }

    /// `end` says whether the spinner of a start the user waits on stops here.
    fn set_issue(&self, path: &str, kind: &str, text: String, end: IssueEnd) {
        {
            let mut inner = self.lock();
            inner.issues.insert(path.to_string(), Issue { kind: kind.into(), text });
            if end == IssueEnd::Final {
                inner.pending.remove(path);
            }
        }
        self.notify();
    }

    pub(super) fn launch(self: &Arc<Self>, path: &str, open_url: bool, extra_args: Vec<String>) {
        // Claimed before the slow part, so a second start meanwhile does nothing.
        let Some(start) = self.begin_start(&Unit::Server(path.to_string())) else {
            return;
        };
        let (settings, project) = {
            let inner = self.lock();
            (inner.settings.clone(), inner.settings.project(path))
        };

        let script = settings.script_for(path);

        if is_forbidden_script(&script) {
            return self.set_issue(path, "error", t!("core.error.forbiddenScript", script = script), IssueEnd::Final);
        }

        let package_json = fs::read_to_string(Path::new(path).join("package.json")).unwrap_or_default();

        if !has_script(&package_json, &script) {
            return self.set_issue(path, "error", t!("core.error.noScript", script = script), IssueEnd::Final);
        }

        let manager = PackageManager::parse(&settings.package_manager).unwrap_or_else(|| {
            let detected = detect_manager(path);
            self.lock().managers.borrow_mut().insert(path.to_string(), detected);
            detected
        });

        let (args, port) = if extra_args.is_empty() {
            let base = project.port.filter(|p| *p > 0).unwrap_or_else(|| vite_port(path));
            let free = self.reservations.lock().ok().and_then(|mut r| r.reserve_free(base));

            match free {
                Some(free) if free != base => (vec!["--port".to_string(), free.to_string()], Some(free)),
                _ => (Vec::new(), Some(base)),
            }
        } else {
            let port = extra_args.iter().skip_while(|a| *a != "--port").nth(1).and_then(|p| p.parse().ok());
            (extra_args, port)
        };

        let command = match build_command(manager, &script, &args) {
            Ok(command) => command,
            Err(message) => return self.set_issue(path, "error", message, IssueEnd::Final),
        };

        let child = match process::spawn_shell(&command, path) {
            Ok(child) => child,
            Err(error) => return self.set_issue(path, "crashed", t!("core.issue.couldNotStart", error = error), IssueEnd::Final),
        };

        let adopted = self.adopt(start, child, |inner, id, pid| {
            inner.output.insert(path.to_string(), VecDeque::new());
            inner.issues.remove(path);
            inner.runs.insert(
                path.to_string(),
                Run {
                    id,
                    pid,
                    name: folder_name(path),
                    command: command.clone(),
                    started: Instant::now(),
                    started_at: now_ms(),
                    port,
                    url: None,
                    buffer: String::new(),
                    open_url,
                    settled: false,
                },
            );
        });
        let Some((run_id, child)) = adopted else {
            return;
        };

        self.push_line(path, format!("$ {command}"));
        self.push_line(path, format!("  {path}"));
        self.record_pids();
        self.notify();
        self.watch(Unit::Server(path.to_string()), run_id, child);
    }

    pub(super) fn on_line(self: &Arc<Self>, path: &str, run_id: u64, line: String) {
        let clean = crate::resolve::strip_ansi(&line);
        let mut open: Option<Option<String>> = None;
        let mut conflict_port: Option<u16> = None;
        let mut changed = false;

        {
            let mut inner = self.lock();
            let Some(run) = inner.runs.get_mut(path).filter(|run| run.id == run_id) else {
                return;
            };

            run.buffer.push_str(&clean);
            run.buffer.push('\n');
            if run.buffer.len() > 4096 {
                let cut = run.buffer.len() - 4096;
                let cut = (cut..run.buffer.len()).find(|i| run.buffer.is_char_boundary(*i)).unwrap_or(0);
                run.buffer.drain(..cut);
            }

            if let Some(conflict) = detect_port_conflict(&run.buffer) {
                run.buffer.clear();
                if conflict.fatal && conflict.port > 0 && !inner.retried.contains(path) {
                    inner.retried.insert(path.to_string());
                    conflict_port = Some(conflict.port);
                }
            } else if let Some(url) = extract_local_url(&run.buffer) {
                run.buffer.clear();
                if !run.settled {
                    run.settled = true;
                    run.port = port_from_url(&url).or(run.port);
                    run.url = Some(url.clone());
                    changed = true;
                    if run.open_url {
                        open = Some(Some(url));
                    }
                }
            }

            if let Some(error) = detect_error_line(&clean, 90) {
                let current = inner.issues.get(path).map(|i| i.kind.clone());
                if current.as_deref() != Some("crashed") {
                    inner.issues.insert(path.to_string(), Issue { kind: "error".into(), text: error });
                    changed = true;
                }
            }
        }

        // Shown as plain text: colour codes from tools that ignore FORCE_COLOR are dropped.
        self.push_line(path, clean);

        if changed {
            self.notify();
        }

        if let Some(Some(local)) = open {
            self.show_in_browser(path, Some(local), true);
        }

        if let Some(port) = conflict_port {
            // The server died on a busy port: move it to a free one, once per project.
            let core = Arc::clone(self);
            let path = path.to_string();

            thread::spawn(move || {
                let free = core.reservations.lock().ok().and_then(|mut r| r.reserve_free(port.saturating_add(1)));
                if let Some(free) = free {
                    core.stop_server(&path);
                    core.launch(&path, false, vec!["--port".into(), free.to_string()]);
                }
            });
        }
    }

    /// The server's shell exited (taken out of `runs` already); `planned` when a stop did it.
    pub(super) fn on_exit(self: &Arc<Self>, path: &str, started: Instant, planned: bool, disposed: bool, status: Option<std::process::ExitStatus>) {
        let (code, signal) = describe_exit(status);

        self.push_line(path, format!("[pitwall] {}", t!("core.log.processEnded", code = code, signal = signal)));
        self.flush_output();
        self.record_pids();
        self.notify();

        if planned || disposed {
            return;
        }

        self.handle_crash(path, started, &code, &signal);
    }

    fn handle_crash(self: &Arc<Self>, path: &str, started: Instant, code: &str, signal: &str) {
        let detail = {
            let inner = self.lock();
            match inner.issues.get(path) {
                Some(issue) if issue.kind == "error" => issue.text.clone(),
                _ if code != "-" => t!("core.issue.exitCode", code = code),
                _ => t!("core.issue.exitSignal", signal = signal),
            }
        };

        self.lock().crash_unseen = true;
        self.record_crash(path);

        let unit = Unit::Server(path.to_string());
        let Some(attempt) = self.claim_restart(&unit, started) else {
            self.set_issue(path, "crashed", t!("core.issue.crashedGaveUp", detail = detail, max = MAX_RESTARTS), IssueEnd::Final);
            self.push_line(path, format!("[pitwall] {}", t!("core.log.gaveUpServer", max = MAX_RESTARTS)));
            return;
        };

        let text = t!("core.issue.crashedRestarting", detail = detail, attempt = attempt, max = MAX_RESTARTS);
        self.set_issue(path, "crashed", text, IssueEnd::Retrying);
        self.push_line(path, format!("[pitwall] {}", t!("core.log.crashedRestarting", attempt = attempt, max = MAX_RESTARTS)));
        self.restart_later(&unit);
    }

    /// Stops the dev server and the commands that run along with it.
    pub fn stop(self: &Arc<Self>, path: &str) {
        self.stop_server(path);
        self.stop_server_companions(path);
    }

    /// The dev server only; recovery paths restart it without touching its companions.
    fn stop_server(self: &Arc<Self>, path: &str) {
        self.stop_unit(&Unit::Server(path.to_string()));
    }

    /* ---------- health ---------- */

    pub fn check_health(self: &Arc<Self>) {
        let targets: Vec<(String, u64, u16)> = {
            let inner = self.lock();
            inner
                .runs
                .iter()
                .filter(|(path, _)| !inner.busy.contains(*path))
                .filter_map(|(path, run)| run.port.filter(|_| run.settled).map(|port| (path.clone(), run.id, port)))
                .collect()
        };

        for (path, run_id, port) in targets {
            let alive = is_port_served(port);
            let current = self.lock().issues.get(&path).map(|issue| issue.kind.clone());

            if !alive && current.as_deref() != Some("unresponsive") {
                self.set_issue(&path, "unresponsive", t!("core.issue.notResponding", port = port), IssueEnd::Retrying);
                let core = Arc::clone(self);
                thread::spawn(move || core.recover_unresponsive(&path, run_id, port));
            } else if alive && current.as_deref() == Some("unresponsive") {
                self.lock().issues.remove(&path);
                self.notify();
            }
        }
    }

    fn recover_unresponsive(self: &Arc<Self>, path: &str, run_id: u64, port: u16) {
        thread::sleep(RESTART_DELAY);

        let started = {
            let inner = self.lock();
            match inner.runs.get(path) {
                Some(run) if run.id == run_id && !inner.busy.contains(path) && !inner.disposed => run.started,
                _ => return,
            }
        };

        if is_port_served(port) {
            let mut inner = self.lock();
            if inner.issues.get(path).is_some_and(|issue| issue.kind == "unresponsive") {
                inner.issues.remove(path);
            }
            drop(inner);
            self.notify();
            return;
        }

        let Some(attempt) = self.claim_restart(&Unit::Server(path.to_string()), started) else {
            self.set_issue(path, "unresponsive", t!("core.issue.notRespondingGaveUp", port = port, max = MAX_RESTARTS), IssueEnd::Final);
            return;
        };

        self.push_line(path, format!("[pitwall] {}", t!("core.log.notRespondingRestarting", port = port, attempt = attempt, max = MAX_RESTARTS)));
        self.stop_server(path);
        self.launch(path, false, Vec::new());
    }
}

fn describe_exit(status: Option<std::process::ExitStatus>) -> (String, String) {
    let Some(status) = status else {
        return ("-".into(), "-".into());
    };

    let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "-".into());

    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|s| s.to_string()).unwrap_or_else(|| "-".into())
    };
    #[cfg(not(unix))]
    let signal = "-".to_string();

    (code, signal)
}

/// The package manager a project's lock file names (npm without one).
fn detect_manager(path: &str) -> PackageManager {
    let files: Vec<String> =
        fs::read_dir(path).map(|entries| entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    detect_package_manager(&files)
}

/// The command a project's dev server would run, as the launcher builds it: Settings' package
/// manager or the lock file's, and the project's script. The lock file is read once per
/// project (and again at each start).
pub(super) fn run_command(inner: &Inner, path: &str) -> String {
    let script = inner.settings.script_for(path);
    let manager = PackageManager::parse(&inner.settings.package_manager)
        .unwrap_or_else(|| *inner.managers.borrow_mut().entry(path.to_string()).or_insert_with(|| detect_manager(path)));

    build_command(manager, &script, &[]).unwrap_or_else(|_| format!("{manager} run {}", script.trim()))
}

/// The port a project wants: `server.port` in its Vite config, else 5173.
fn vite_port(path: &str) -> u16 {
    ["vite.config.ts", "vite.config.js", "vite.config.mts", "vite.config.mjs"]
        .iter()
        .filter_map(|name| fs::read_to_string(Path::new(path).join(name)).ok())
        .find_map(|source| parse_vite_port(&source))
        .unwrap_or(5173)
}
