//! Supervision shared by the dev server and custom commands: one start at a time, output read as
//! lines, an exit told apart from a stop, a stop that waits for the whole process group to go
//! (SIGTERM, then SIGKILL), and crash restarts after 3 s, at most three within a minute. A stop
//! also calls off a start under way and a restart waiting out its delay.

use std::sync::mpsc;

use super::*;
use crate::process::{self, group_alive};

/// After SIGTERM a process group gets this long to end before SIGKILL.
const STOP_GRACE: Duration = Duration::from_millis(600);
/// After SIGKILL, how long a stop waits for the group (and the exit's handling) to be done.
const KILL_WAIT: Duration = Duration::from_secs(3);
/// After its shell exits, how long the last output still has to arrive.
const DRAIN_WAIT: Duration = Duration::from_millis(250);
/// How much later than its run a lingering group's leader may seem to have started: Linux tells
/// a start to the second.
const START_SLACK_MS: u64 = 2_000;

/// A supervised process: a project's dev server, or one of its custom commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unit {
    Server(String),
    /// Project path and command id.
    Command(String, String),
}

impl Unit {
    pub(super) fn key(&self) -> String {
        match self {
            Unit::Server(path) => path.clone(),
            Unit::Command(path, id) => job_key(path, id),
        }
    }
}

/// Bookkeeping per unit, by `Unit::key`.
#[derive(Default)]
pub(super) struct Supervisor {
    /// Starts under way (files read, ports probed, the shell spawned): one at a time.
    starting: HashMap<String, u64>,
    /// Crash restarts waiting out their delay; a newer one replaces an older.
    restarts: HashMap<String, u64>,
    next_ticket: u64,
    /// Crash restarts used in the current series.
    attempts: HashMap<String, u32>,
    /// Process groups kept in the pids file although their shell is gone or being stopped,
    /// for as long as something of them is left; with their key and when their run started.
    lingering: HashMap<u32, (String, u64)>,
}

/// Group `pgid` of a run that started at `started_at` (ms) still has processes, and its number
/// is still that run's. Once the group is gone the system may hand the number out again; a
/// process under it that started after the run is someone else's. Windows hands a root's number
/// out again while the rest of its tree runs on: a process under it outside that tree leaves the
/// tree ours (`process::outside_tree`).
fn still_ours(pgid: u32, started_at: u64) -> bool {
    group_alive(pgid)
        && (process::started_at(pgid).is_none_or(|started| started <= started_at + START_SLACK_MS) || process::outside_tree(pgid))
}

impl Supervisor {
    fn ticket(&mut self) -> u64 {
        self.next_ticket += 1;
        self.next_ticket
    }

    /// A start under way or a restart waiting.
    pub(super) fn pending(&self, key: &str) -> bool {
        self.starting.contains_key(key) || self.restarts.contains_key(key)
    }

    /// Paths of dev servers with a start under way or a restart waiting.
    pub(super) fn pending_servers(&self) -> Vec<String> {
        let keys = self.starting.keys().chain(self.restarts.keys());
        let mut paths: Vec<String> = keys.filter(|key| !key.contains('\u{1f}')).cloned().collect();
        paths.sort();
        paths.dedup();
        paths
    }

    /// A start by hand begins a new restart series.
    pub(super) fn reset_attempts(&mut self, key: &str) {
        self.attempts.remove(key);
    }

    /// A restart slot after a crash. A run that stayed up for a minute starts a new series.
    /// `None` when the three slots are used up; otherwise this attempt's number.
    pub(super) fn next_attempt(&mut self, key: &str, started: Instant) -> Option<u32> {
        if started.elapsed() >= STABLE {
            self.attempts.remove(key);
        }

        let used = self.attempts.get(key).copied().unwrap_or(0);

        if used >= MAX_RESTARTS {
            return None;
        }

        self.attempts.insert(key.to_string(), used + 1);
        Some(used + 1)
    }

    /// Ends a start: true when it still stands (no stop came meanwhile).
    fn finish(&mut self, key: &str, ticket: u64) -> bool {
        let stands = self.starting.get(key) == Some(&ticket);
        if stands {
            self.starting.remove(key);
        }
        stands
    }

    /// A stop: the start under way and the restart waiting are both called off. True if there
    /// was either.
    fn cancel(&mut self, key: &str) -> bool {
        let starting = self.starting.remove(key).is_some();
        let restart = self.restarts.remove(key).is_some();
        starting || restart
    }

    /// Lets go of the groups that are no longer ours (`still_ours`), before their numbers are
    /// signalled or recorded as ours. True when one went.
    pub(super) fn prune_lingering(&mut self) -> bool {
        let before = self.lingering.len();
        self.lingering.retain(|pgid, (_, started_at)| still_ours(*pgid, *started_at));
        self.lingering.len() != before
    }

    /// Groups to keep recording; those no longer ours are dropped first.
    pub(super) fn lingering_groups(&mut self) -> Vec<(u32, String)> {
        self.prune_lingering();
        self.lingering.iter().map(|(pgid, (key, _))| (*pgid, key.clone())).collect()
    }
}

/// A start under way, holding its unit's claim. Dropped before `adopt`, it gives the claim back.
pub(super) struct Start<'a> {
    core: &'a Core,
    unit: Unit,
    ticket: u64,
}

impl Drop for Start<'_> {
    fn drop(&mut self) {
        self.core.lock().supervisor.finish(&self.unit.key(), self.ticket);
    }
}

/// The running process of `unit`: its pid, run id and start (ms).
fn current(inner: &Inner, unit: &Unit) -> Option<(u32, u64, u64)> {
    match unit {
        Unit::Server(path) => inner.runs.get(path).map(|run| (run.pid, run.id, run.started_at)),
        Unit::Command(path, id) => inner.jobs.get(&job_key(path, id)).map(|job| (job.pid, job.run, job.started_at)),
    }
}

/// Takes run `id` of `unit` out, if it is still the current one: its pid and start, as an
/// instant and in ms.
fn take_current(inner: &mut Inner, unit: &Unit, id: u64) -> Option<(u32, Instant, u64)> {
    if current(inner, unit).is_none_or(|(_, run, _)| run != id) {
        return None;
    }
    match unit {
        Unit::Server(path) => inner.runs.remove(path).map(|run| (run.pid, run.started, run.started_at)),
        Unit::Command(path, cid) => inner.jobs.remove(&job_key(path, cid)).map(|job| (job.pid, job.started, job.started_at)),
    }
}

impl Core {
    /// Claims a start of `unit`: `None` while it runs, while another start of it is under way,
    /// or once the app quits. The slow part of a start (files, ports, the spawn) comes after the
    /// claim, so two starts at once never both go ahead.
    pub(super) fn begin_start(&self, unit: &Unit) -> Option<Start<'_>> {
        let mut inner = self.lock();
        if inner.disposed || current(&inner, unit).is_some() {
            return None;
        }

        let key = unit.key();
        if inner.supervisor.starting.contains_key(&key) {
            return None;
        }
        let ticket = inner.supervisor.ticket();
        inner.supervisor.starting.insert(key, ticket);
        Some(Start { core: self, unit: unit.clone(), ticket })
    }

    /// Takes a freshly spawned process in: `insert` records it with its run id and pid, under
    /// the same lock that checks the claim. A start that a stop called off meanwhile (or that
    /// lost to quitting) ends its process at once and gives `None`. Follow it with `watch`.
    pub(super) fn adopt(&self, start: Start<'_>, child: Child, insert: impl FnOnce(&mut Inner, u64, u32)) -> Option<(u64, Child)> {
        let pid = child.id();
        let id = {
            let mut inner = self.lock();
            let stands = inner.supervisor.finish(&start.unit.key(), start.ticket) && !inner.disposed;
            stands.then(|| {
                inner.next_run += 1;
                let id = inner.next_run;
                insert(&mut inner, id, pid);
                id
            })
        };
        drop(start);

        match id {
            Some(id) => Some((id, child)),
            None => {
                let mut child = child;
                thread::spawn(move || {
                    process::stop_groups(&[pid], STOP_GRACE, KILL_WAIT);
                    let _ = child.wait();
                });
                None
            }
        }
    }

    /// Follows run `id` of `unit`: each output line, then its exit once the last lines are in.
    pub(super) fn watch(self: &Arc<Self>, unit: Unit, id: u64, mut child: Child) {
        // Each reader holds a sender; when all have reached the end, the receiver hears so.
        let (done, drained) = mpsc::channel::<()>();
        let streams = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)];

        for stream in streams.into_iter().flatten() {
            let core = Arc::clone(self);
            let unit = unit.clone();
            let done = done.clone();

            process::read_lines(stream, move |line| {
                let _held = &done;
                match &unit {
                    Unit::Server(path) => core.on_line(path, id, line),
                    Unit::Command(path, cid) => core.on_job_line(path, cid, id, line),
                }
            });
        }
        drop(done);

        let core = Arc::clone(self);

        thread::spawn(move || {
            let status = child.wait().ok();
            // A process that outlives the shell may hold the output open; don't wait on it long.
            let _ = drained.recv_timeout(DRAIN_WAIT);
            core.on_unit_exit(&unit, id, status);
        });
    }

    fn on_unit_exit(self: &Arc<Self>, unit: &Unit, id: u64, status: Option<std::process::ExitStatus>) {
        let key = unit.key();
        let (path, job) = match unit {
            Unit::Server(path) => (path, None),
            Unit::Command(path, cid) => (path, Some(cid.as_str())),
        };
        // The line that says it ended goes into the output in the same hold that takes the run
        // out: a start that finds the run gone writes its own lines under that one.
        let taken = self.keep_line_with(path, job, |inner| {
            let (pid, started, started_at) = take_current(inner, unit, id)?;
            // Whatever of its group is left stays recorded, to be cleaned up later.
            if still_ours(pid, started_at) {
                inner.supervisor.lingering.insert(pid, (key.clone(), started_at));
            }
            // Written down with the run taken out: the next start, however soon, finds it.
            let planned = inner.stopping.contains(&key);
            inner.ended.insert(key.clone(), (now_ms(), output::end_detail(unit, planned, status)));
            Some(((started, planned, inner.disposed), output::end_line(unit, planned, status)))
        });
        let Some((started, planned, disposed)) = taken else {
            return;
        };

        match unit {
            Unit::Server(path) => self.on_exit(path, started, planned, disposed, status),
            Unit::Command(path, cid) => self.on_job_exit(path, cid, started, planned, disposed, status),
        }
    }

    /// A restart slot for `unit` after a crash (see `Supervisor::next_attempt`).
    pub(super) fn claim_restart(&self, unit: &Unit, started: Instant) -> Option<u32> {
        self.lock().supervisor.next_attempt(&unit.key(), started)
    }

    /// Starts `unit` again after the restart delay, unless a stop came meanwhile, the app is
    /// quitting, or a newer crash took over.
    pub(super) fn restart_later(self: &Arc<Self>, unit: &Unit) {
        let key = unit.key();
        let ticket = {
            let mut inner = self.lock();
            let ticket = inner.supervisor.ticket();
            inner.supervisor.restarts.insert(key.clone(), ticket);
            ticket
        };

        thread::sleep(RESTART_DELAY);

        let wanted = {
            let mut inner = self.lock();
            let mine = inner.supervisor.restarts.get(&key) == Some(&ticket);
            if mine {
                inner.supervisor.restarts.remove(&key);
            }
            mine && !inner.disposed
        };

        if wanted {
            match unit {
                Unit::Server(path) => {
                    self.launch(path, false, Vec::new());
                    self.hold_companions(path, Origin::Crash);
                }
                Unit::Command(path, id) => {
                    self.spawn_job(path, id);
                }
            }
        }
    }

    /// Stops `unit`. A start under way or a restart waiting is called off; a running process
    /// group gets SIGTERM, and SIGKILL if anything of it is left after the grace period. Its
    /// pid stays in the pids file until the whole group is gone.
    pub(super) fn stop_unit(self: &Arc<Self>, unit: &Unit) {
        let key = unit.key();
        let (found, groups, cancelled, pruned) = {
            let mut inner = self.lock();
            let cancelled = inner.supervisor.cancel(&key);
            // A group of an earlier run that is gone by now isn't signalled under its number.
            let pruned = inner.supervisor.prune_lingering();
            let found = current(&inner, unit);

            if let Some((pid, _, started_at)) = found {
                inner.stopping.insert(key.clone());
                // Its shell may be gone already, its exit not handled yet. Not waited for, it is
                // ours whatever its start says: Linux tells a start from the boot time as the
                // wall clock has it now, which moves when the clock is set.
                if process::unreaped_child(pid) || still_ours(pid, started_at) {
                    inner.supervisor.lingering.insert(pid, (key.clone(), started_at));
                }
                if let Unit::Server(path) = unit {
                    inner.busy.insert(path.clone());
                }
            }
            // Stopped on purpose: no crash (or restart on its way) to show any more.
            if let (Unit::Server(path), true) = (unit, found.is_some() || cancelled) {
                inner.issues.remove(path);
            }

            let groups: Vec<u32> = inner.supervisor.lingering.iter().filter(|(_, (k, _))| *k == key).map(|(pgid, _)| *pgid).collect();
            (found, groups, cancelled, pruned)
        };

        if found.is_none() && groups.is_empty() {
            if pruned {
                self.record_pids();
            }
            if cancelled {
                self.notify();
            }
            return;
        }

        self.notify();
        process::stop_groups(&groups, STOP_GRACE, KILL_WAIT);

        // The group is gone; its exit is handled on the waiter thread, after the last output.
        if let Some((_, id, _)) = found {
            process::wait_until(KILL_WAIT, || current(&self.lock(), unit).is_none_or(|(_, run, _)| run != id));
        }

        {
            let mut inner = self.lock();
            if let Some((_, id, _)) = found {
                take_current(&mut inner, unit, id);
            }
            inner.stopping.remove(&key);
            if let Unit::Server(path) = unit {
                inner.busy.remove(path);
            }
        }

        self.record_pids();
        self.notify();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::settings::{CustomCommand, ProjectSettings};
    use std::os::unix::process::CommandExt;

    /// A process group of the test's own, as another app's would be: killed by its pid when the
    /// test ends, whatever happened.
    struct Stranger(std::process::Child);

    impl Drop for Stranger {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Stranger {
        fn new() -> Self {
            Stranger(std::process::Command::new("/bin/sleep").arg("30").process_group(0).spawn().unwrap())
        }

        fn pgid(&self) -> u32 {
            self.0.id()
        }

        /// Ended by now: someone signalled its group.
        fn signalled(&mut self) -> bool {
            process::wait_until(Duration::from_millis(1500), || !matches!(self.0.try_wait(), Ok(None)))
        }
    }

    /// Once a group of ours is gone its number isn't ours any more: the system hands it out again,
    /// and a Stop or the quit would signal whatever group has it by then, a group Pitwall never
    /// started. The loop lets go of it at its next pass, and a number given out again before that
    /// is told by the start of the process under it.
    #[test]
    fn a_group_that_is_gone_is_never_signalled_under_its_number() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().into_owned();
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude"),
                settings_file: tmp.path().join("settings.json"),
                claude_settings: tmp.path().join("claude-settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        core.add_project(&path).unwrap();
        let once = CustomCommand { id: "once".into(), name: "once".into(), command: "sleep 1 & echo started".into(), keep_running: false, confirm: false, with_server: false };
        core.set_project_settings(&path, ProjectSettings { commands: vec![once], ..Default::default() });
        let pids = || fs::read_dir(tmp.path().join("registry").join("pids")).map(|dir| dir.flatten().filter_map(|e| fs::read_to_string(e.path()).ok()).collect::<String>()).unwrap_or_default();

        // Its shell ends at once, the sleep of its group a moment later.
        assert!(core.run_command(&path, "once"));
        let key = job_key(&path, "once");
        let pgid = core.lock().jobs.get(&key).map(|job| job.pid()).expect("running");
        assert!(process::wait_until(Duration::from_secs(10), || core.lock().supervisor.lingering.contains_key(&pgid)), "kept for its group");
        let entry = core.lock().supervisor.lingering[&pgid].clone();
        assert!(process::wait_until(Duration::from_secs(10), || !group_alive(pgid)), "the sleep never ended");

        core.tick(1);
        assert!(!core.lock().supervisor.lingering.contains_key(&pgid), "a group that is gone is still held as ours");
        assert!(!pids().contains(&format!("\"pid\":{pgid}")), "a group that is gone is still in the pids file");

        // The number given out again before the loop's next pass, to a group that started after
        // the run.
        thread::sleep(Duration::from_millis((entry.1 + START_SLACK_MS + 1_500).saturating_sub(now_ms())));
        let mut stranger = Stranger::new();
        let number = stranger.pgid();
        let held = || core.lock().supervisor.lingering.insert(number, entry.clone());

        held();
        core.stop_command(&path, "once");
        held();
        core.record_pids();
        let recorded = pids().contains(&format!("\"pid\":{number}"));
        held();
        core.dispose();

        assert!(!recorded, "another's group was recorded as ours");
        assert!(!stranger.signalled(), "a group Pitwall never started was signalled");
    }
}
