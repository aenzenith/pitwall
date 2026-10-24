//! Dev servers run here: the launch (script, package manager, a free port), their output read
//! for the address and for errors, crash restarts, and the health probe.

use super::*;
use crate::ports::is_port_served;
use crate::resolve::{
    build_command, detect_error_line, detect_package_manager, detect_port_conflict, extract_local_url, has_script,
    is_unsafe_script, is_update_line, parse_vite_port, port_from_url, runs_vite, script_line,
};

/// Whether an issue ends a start the user waits on: a refused or failed start, or a final
/// give-up, does; a crash or a hang on its way back doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueEnd {
    Final,
    Retrying,
}

/// How sure a server's address is, from the least to the most: a surer one read later takes the
/// place of the one it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Found {
    /// A bare URL in a line: a proxy's target or another process of the script may print one
    /// before the server says its own, or the server says its own so (Django, Rails, Express).
    Bare,
    /// The server's own address line (`Local:`).
    Labelled,
    /// A URL on the port Pitwall started it on.
    Asked,
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
    /// The port Pitwall started it on (the project's own, its Vite config's, or the free one
    /// given instead); none when the script picks its own.
    asked: Option<u16>,
    /// How sure `url` is, once it has one.
    found: Option<Found>,
    /// It printed URLs on more than one port: with none of them sure, which is the server's own
    /// can't be told.
    ports_differ: bool,
    open_url: bool,
    pub(super) settled: bool,
}

impl Run {
    /// The port the health probe asks, when it is surely the server's own: the one Pitwall
    /// started it on, the one its own `Local:` line gives, or a bare URL's while it is the only
    /// port the run printed. With bare URLs on two ports or more (a proxy's target and the
    /// server's), none: the probe leaves it alone rather than restart a server that is fine
    /// because the other one went down.
    fn probed_port(&self) -> Option<u16> {
        let sure = match self.found {
            Some(Found::Bare) => !self.ports_differ,
            found => found.is_some(),
        };
        self.port.filter(|_| sure)
    }
}

impl Core {
    /// A start by hand resets the restart counter and the one move off a busy port. `by`: who
    /// starts it, for the commands that start with it and ask first (`jobs.rs`:
    /// `start_server_companions`).
    pub fn start(self: &Arc<Self>, path: &str, open_url: bool, by: Origin) {
        {
            let mut inner = self.lock();
            inner.supervisor.reset_attempts(path);
            inner.retried.remove(path);
        }
        let open = open_url && self.lock().settings.open_url_on_start;
        self.launch(path, open, Vec::new());

        if self.lock().runs.contains_key(path) {
            self.start_server_companions(path, by);
        }
    }

    pub fn restart(self: &Arc<Self>, path: &str, by: Origin) {
        self.stop(path);
        self.start(path, false, by);
    }

    /// `end` says whether the spinner of a start the user waits on stops here.
    fn set_issue(&self, path: &str, kind: &str, text: impl Fn() -> String + Send + Sync + 'static, end: IssueEnd) {
        {
            let mut inner = self.lock();
            inner.issues.insert(path.to_string(), Issue { kind: kind.into(), text: text() });
            inner.issue_texts.insert(path.to_string(), Arc::new(text));
            if end == IssueEnd::Final {
                inner.pending.remove(path);
            }
        }
        self.notify();
    }

    /// The language changed: each issue is said again in it (the caller tells the windows).
    pub(super) fn retell_issues(&self) {
        let mut inner = self.lock();
        let Inner { issues, issue_texts, .. } = &mut *inner;
        for (path, issue) in issues.iter_mut() {
            if let Some(text) = issue_texts.get(path) {
                issue.text = text();
            }
        }
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

        if is_unsafe_script(&script) {
            return self.set_issue(path, "error", move || t!("core.error.forbiddenScript", script = script), IssueEnd::Final);
        }

        // A folder that was moved or deleted reads as a package.json with no scripts: said as
        // it is, without its path (the command line and the radio print an issue's text).
        if !Path::new(path).is_dir() {
            return self.set_issue(path, "error", || t!("core.issue.folderGone"), IssueEnd::Final);
        }
        // The folder inside it that it runs in (Settings › its dev server).
        let Some(dir) = project.work_dir(path) else {
            return self.set_issue(path, "error", || t!("core.issue.workFolderGone"), IssueEnd::Final);
        };

        let package_json = crate::files::read_regular_string(&dir.join("package.json"), crate::files::MANIFEST_BYTES).unwrap_or_default();

        if !has_script(&package_json, &script) {
            return self.set_issue(path, "error", move || t!("core.error.noScript", script = script), IssueEnd::Final);
        }

        let manager = PackageManager::parse(&settings.package_manager).unwrap_or_else(|| {
            let detected = detect_manager(&dir);
            self.lock().managers.insert(path.to_string(), detected);
            detected
        });

        // `--port` only for a project that asks for a port: its own in the settings, or its Vite
        // config's (`vite_port`). Any other script keeps the port it picks itself (Next, Nuxt and
        // Astro move on by themselves), and some refuse the flag; its port is known once it prints
        // its address.
        let (args, port) = if !extra_args.is_empty() {
            let port = extra_args.iter().skip_while(|a| *a != "--port").nth(1).and_then(|p| p.parse().ok());
            (extra_args, port)
        } else if let Some(base) = project.port.filter(|p| *p > 0).or_else(|| vite_port(&dir, &package_json, &script)) {
            let free = self.reservations.lock().ok().and_then(|mut r| r.reserve_free(base, path));

            match free {
                Some(free) if free != base => (vec!["--port".to_string(), free.to_string()], Some(free)),
                _ => (Vec::new(), Some(base)),
            }
        } else {
            (Vec::new(), None)
        };

        let command = match build_command(manager, &script, &args) {
            Ok(command) => command,
            Err(message) => return self.set_issue(path, "error", move || build_command(manager, &script, &args).err().unwrap_or_else(|| message.clone()), IssueEnd::Final),
        };

        let child = match process::spawn_shell(&command, &dir.to_string_lossy()) {
            Ok(child) => child,
            Err(error) => {
                let error = error.to_string();
                return self.set_issue(path, "crashed", move || t!("core.issue.couldNotStart", error = error), IssueEnd::Final);
            }
        };

        let adopted = self.adopt(start, child, |inner, id, pid| {
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
                    asked: port,
                    found: None,
                    ports_differ: false,
                    open_url,
                    settled: false,
                },
            );
        });
        let Some((run_id, child)) = adopted else {
            return;
        };

        // The run before stays in the output, above these.
        self.mark_run(path, None);
        self.push_line(path, format!("$ {command}"));
        self.push_line(path, format!("  {}", dir.display()));
        self.record_pids();
        self.publish_changed();
        self.notify();
        self.watch(Unit::Server(path.to_string()), run_id, child);
    }

    pub(super) fn on_line(self: &Arc<Self>, path: &str, run_id: u64, line: String) {
        let clean = crate::resolve::strip_ansi(&line);
        // What a server says (its address, a busy port, an error, a change it took) stands on one
        // line: each line is read once, on its own, before the core's lock is taken.
        let conflict = detect_port_conflict(&clean);
        let address = if conflict.is_none() { extract_local_url(&clean) } else { None };
        let error = detect_error_line(&clean, 90);
        let updated = error.is_none() && is_update_line(&clean);
        let mut open: Option<String> = None;
        let mut conflict_port: Option<u16> = None;
        let mut changed = false;

        {
            let mut inner = self.lock();
            let Inner { runs, retried, issues, issue_texts, .. } = &mut *inner;
            let Some(run) = runs.get_mut(path).filter(|run| run.id == run_id) else {
                return;
            };

            if let Some(conflict) = conflict {
                // The server itself died on its busy port before it was up: moved to a free one,
                // once per start by hand. Another process of the script that finds its own port
                // busy, or anything said once the server is up, is no reason to move it.
                let own = run.asked.is_none_or(|port| port == conflict.port);
                if conflict.fatal && conflict.port > 0 && !run.settled && own && !retried.contains(path) {
                    retried.insert(path.to_string());
                    conflict_port = Some(conflict.port);
                }
            } else if let Some(local) = address {
                let port = port_from_url(&local.url);
                let found = if port.is_some() && port == run.asked {
                    Found::Asked
                } else if local.labelled {
                    Found::Labelled
                } else {
                    Found::Bare
                };
                // The first address settles the run; a surer one said later takes its place (a
                // proxy's target printed before the server's own `Local:` line).
                if run.found.is_none_or(|had| found > had) {
                    run.found = Some(found);
                    // A URL whose port can't be read leaves the one Pitwall started it on, never
                    // the port of the address it replaces.
                    run.port = port.or(run.asked);
                    run.url = Some(local.url.clone());
                    changed = true;
                    if !run.settled {
                        run.settled = true;
                        if run.open_url {
                            open = Some(local.url);
                        }
                    }
                } else if port != run.port {
                    run.ports_differ = true;
                }
            }

            if let Some(error) = error {
                if issues.get(path).is_none_or(|issue| issue.kind != "crashed") {
                    issues.insert(path.to_string(), Issue { kind: "error".into(), text: error.clone() });
                    issue_texts.insert(path.to_string(), Arc::new(move || error.clone()));
                    changed = true;
                }
            } else if updated && issues.get(path).is_some_and(|issue| issue.kind == "error") {
                // The server took a change since the error: it was mended (one that wasn't is
                // printed again right after).
                issues.remove(path);
                issue_texts.remove(path);
                changed = true;
            }
        }

        // Shown as plain text: colour codes from tools that ignore FORCE_COLOR are dropped.
        self.push_line(path, clean);

        if changed {
            self.notify();
        }

        if let Some(local) = open {
            self.show_in_browser(path, Some(local), true);
        }

        if let Some(port) = conflict_port {
            // The server died on its busy port: move it to a free one.
            let core = Arc::clone(self);
            let path = path.to_string();

            thread::spawn(move || {
                let free = core.reservations.lock().ok().and_then(|mut r| r.reserve_free(port.saturating_add(1), &path));
                if let Some(free) = free {
                    core.stop_server(&path);
                    // The same start, moved: what the busy port left behind is no run to keep.
                    core.forget_output(&path);
                    core.launch(&path, false, vec!["--port".into(), free.to_string()]);
                }
            });
        }
    }

    /// The server's shell exited (taken out of `runs` already, and the output says so);
    /// `planned` when a stop did it.
    pub(super) fn on_exit(self: &Arc<Self>, path: &str, started: Instant, planned: bool, disposed: bool, status: Option<std::process::ExitStatus>) {
        let (code, signal) = describe_exit(status);

        self.flush_output();
        self.record_pids();
        self.notify();

        if planned || disposed {
            return;
        }

        self.handle_crash(path, started, &code, &signal);
    }

    fn handle_crash(self: &Arc<Self>, path: &str, started: Instant, code: &str, signal: &str) {
        let detail: IssueText = {
            let inner = self.lock();
            match (inner.issues.get(path), inner.issue_texts.get(path)) {
                (Some(issue), Some(text)) if issue.kind == "error" => Arc::clone(text),
                _ if code != "-" => {
                    let code = code.to_string();
                    Arc::new(move || t!("core.issue.exitCode", code = code))
                }
                _ => {
                    let signal = signal.to_string();
                    Arc::new(move || t!("core.issue.exitSignal", signal = signal))
                }
            }
        };

        self.lock().crash_unseen = true;
        self.record_crash(path);

        let unit = Unit::Server(path.to_string());
        let Some(attempt) = self.claim_restart(&unit, started) else {
            // Down for good: nothing waits to run with it any more.
            self.lock().held.remove(path);
            self.set_issue(path, "crashed", move || t!("core.issue.crashedGaveUp", detail = detail(), max = MAX_RESTARTS), IssueEnd::Final);
            self.push_line(path, format!("[pitwall] {}", t!("core.log.gaveUpServer", max = MAX_RESTARTS)));
            return;
        };

        self.set_issue(path, "crashed", move || t!("core.issue.crashedRestarting", detail = detail(), attempt = attempt, max = MAX_RESTARTS), IssueEnd::Retrying);
        self.push_line(path, format!("[pitwall] {}", t!("core.log.crashedRestarting", attempt = attempt, max = MAX_RESTARTS)));
        self.restart_later(&unit);
    }

    /// Stops the dev server and the commands that run along with it; those held for the user
    /// (`jobs.rs`) go too.
    pub fn stop(self: &Arc<Self>, path: &str) {
        if self.lock().held.remove(path).is_some() {
            self.notify();
        }
        self.stop_server(path);
        self.stop_server_companions(path);
    }

    /// The dev server only; recovery paths restart it without touching its companions.
    fn stop_server(self: &Arc<Self>, path: &str) {
        self.stop_unit(&Unit::Server(path.to_string()));
    }

    /* ---------- health ---------- */

    /// Only a port that is surely the server's own is probed (`Run::probed_port`).
    pub fn check_health(self: &Arc<Self>) {
        let targets: Vec<(String, u64, u16)> = {
            let inner = self.lock();
            inner
                .runs
                .iter()
                .filter(|(path, _)| !inner.busy.contains(*path))
                .filter_map(|(path, run)| run.probed_port().map(|port| (path.clone(), run.id, port)))
                .collect()
        };

        for (path, run_id, port) in targets {
            let alive = is_port_served(port);
            let current = self.lock().issues.get(&path).map(|issue| issue.kind.clone());

            if !alive && current.as_deref() != Some("unresponsive") {
                self.set_issue(&path, "unresponsive", move || t!("core.issue.notResponding", port = port), IssueEnd::Retrying);
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

        let (started, probed) = {
            let inner = self.lock();
            match inner.runs.get(path) {
                Some(run) if run.id == run_id && !inner.busy.contains(path) && !inner.disposed => (run.started, run.probed_port()),
                _ => return,
            }
        };

        // Meanwhile the server said its own address (after a proxy's target), or another port
        // made it unsure: the silent port is no longer its own.
        if probed != Some(port) || is_port_served(port) {
            let mut inner = self.lock();
            if inner.issues.get(path).is_some_and(|issue| issue.kind == "unresponsive") {
                inner.issues.remove(path);
            }
            drop(inner);
            self.notify();
            return;
        }

        let Some(attempt) = self.claim_restart(&Unit::Server(path.to_string()), started) else {
            self.set_issue(path, "unresponsive", move || t!("core.issue.notRespondingGaveUp", port = port, max = MAX_RESTARTS), IssueEnd::Final);
            return;
        };

        self.push_line(path, format!("[pitwall] {}", t!("core.log.notRespondingRestarting", port = port, attempt = attempt, max = MAX_RESTARTS)));
        self.stop_server(path);
        self.launch(path, false, Vec::new());
        self.hold_companions(path, Origin::Crash);
    }
}

pub(super) fn describe_exit(status: Option<std::process::ExitStatus>) -> (String, String) {
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

/// The package manager the lock file in the folder a project runs in names (npm without one).
/// It lists the folder, so never under the core's lock.
pub(super) fn detect_manager(dir: &Path) -> PackageManager {
    let files: Vec<String> =
        fs::read_dir(dir).map(|entries| entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    detect_package_manager(&files)
}

/// The command a project's dev server would run, as the launcher builds it: Settings' package
/// manager or the lock file's, and the project's script. The lock file is read once per
/// project on the Git thread (`Core::detect_managers`) and again at each start; until then it
/// is npm.
pub(super) fn run_command(inner: &Inner, path: &str) -> String {
    let script = inner.settings.script_for(path);
    let manager = PackageManager::parse(&inner.settings.package_manager)
        .unwrap_or_else(|| inner.managers.get(path).copied().unwrap_or(PackageManager::Npm));

    build_command(manager, &script, &[]).unwrap_or_else(|_| format!("{manager} run {}", script.trim()))
}

/// The port a Vite project wants: `server.port` in its Vite config, else Vite's own 5173 when the
/// dev script (`script` in `package_json`) runs Vite. None for a project without a Vite config in
/// the folder it runs in, and for one whose config is there for Vitest only: its own server may
/// refuse the flag, or be moved off the port it picks.
fn vite_port(dir: &Path, package_json: &str, script: &str) -> Option<u16> {
    let sources: Vec<String> = ["vite.config.ts", "vite.config.js", "vite.config.mts", "vite.config.mjs"]
        .iter()
        .filter_map(|name| fs::read_to_string(dir.join(name)).ok())
        .collect();

    if sources.is_empty() {
        return None;
    }

    let vite = script_line(package_json, script).is_some_and(|line| runs_vite(&line));
    sources.iter().find_map(|source| parse_vite_port(source)).or(vite.then_some(5173))
}
