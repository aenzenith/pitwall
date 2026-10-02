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
    /// for as long as something of them is left; with their key.
    lingering: HashMap<u32, String>,
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

    /// Groups to keep recording; those with nothing left are dropped first.
    pub(super) fn lingering_groups(&mut self) -> Vec<(u32, String)> {
        self.lingering.retain(|pgid, _| group_alive(*pgid));
        self.lingering.iter().map(|(pgid, key)| (*pgid, key.clone())).collect()
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

/// The running process of `unit`: its pid and run id.
fn current(inner: &Inner, unit: &Unit) -> Option<(u32, u64)> {
    match unit {
        Unit::Server(path) => inner.runs.get(path).map(|run| (run.pid, run.id)),
        Unit::Command(path, id) => inner.jobs.get(&job_key(path, id)).map(|job| (job.pid, job.run)),
    }
}

/// Takes run `id` of `unit` out, if it is still the current one: its pid and start.
fn take_current(inner: &mut Inner, unit: &Unit, id: u64) -> Option<(u32, Instant)> {
    if current(inner, unit).is_none_or(|(_, run)| run != id) {
        return None;
    }
    match unit {
        Unit::Server(path) => inner.runs.remove(path).map(|run| (run.pid, run.started)),
        Unit::Command(path, cid) => inner.jobs.remove(&job_key(path, cid)).map(|job| (job.pid, job.started)),
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
        let (started, planned, disposed) = {
            let mut inner = self.lock();
            let Some((pid, started)) = take_current(&mut inner, unit, id) else {
                return;
            };
            // Whatever of its group is left stays recorded, to be cleaned up later.
            if group_alive(pid) {
                inner.supervisor.lingering.insert(pid, key.clone());
            }
            (started, inner.stopping.contains(&key), inner.disposed)
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
                Unit::Server(path) => self.launch(path, false, Vec::new()),
                Unit::Command(path, id) => self.spawn_job(path, id, false),
            }
        }
    }

    /// Stops `unit`. A start under way or a restart waiting is called off; a running process
    /// group gets SIGTERM, and SIGKILL if anything of it is left after the grace period. Its
    /// pid stays in the pids file until the whole group is gone.
    pub(super) fn stop_unit(self: &Arc<Self>, unit: &Unit) {
        let key = unit.key();
        let (found, groups, cancelled) = {
            let mut inner = self.lock();
            let cancelled = inner.supervisor.cancel(&key);
            let found = current(&inner, unit);

            if let Some((pid, _)) = found {
                inner.stopping.insert(key.clone());
                inner.supervisor.lingering.insert(pid, key.clone());
                if let Unit::Server(path) = unit {
                    inner.busy.insert(path.clone());
                }
            }
            // Stopped on purpose: no crash (or restart on its way) to show any more.
            if let (Unit::Server(path), true) = (unit, found.is_some() || cancelled) {
                inner.issues.remove(path);
            }

            let groups: Vec<u32> = inner.supervisor.lingering.iter().filter(|(_, k)| **k == key).map(|(pgid, _)| *pgid).collect();
            (found, groups, cancelled)
        };

        if groups.is_empty() {
            if cancelled {
                self.notify();
            }
            return;
        }

        self.notify();
        process::stop_groups(&groups, STOP_GRACE, KILL_WAIT);

        // The group is gone; its exit is handled on the waiter thread, after the last output.
        if let Some((_, id)) = found {
            process::wait_until(KILL_WAIT, || current(&self.lock(), unit).is_none_or(|(_, run)| run != id));
        }

        {
            let mut inner = self.lock();
            if let Some((_, id)) = found {
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
