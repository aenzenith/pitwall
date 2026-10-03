//! The Dependencies page's state; the checks themselves are `crate::deps`. Each listed project is
//! inspected at start, when its Git `HEAD` or `ORIG_HEAD` moves (a checkout, pull, merge, rebase),
//! when one of its lock, manifest or install-state files changes size or time (a stat-only poll
//! every minute: reading a megabyte of installed.json for every project each minute would be too
//! much), after one of its install jobs ends, and on request. The main window gets `deps` with
//! every report when anything in them changed.
//!
//! The login shell is asked for the runtimes' versions and the managers' executables in one call
//! (`deps::probe`), only for what the inspected projects use: before the first reports go out,
//! when a project brings something not asked yet, and anew on request.
//!
//! A project's pending migrations come from its migration tool's status command (Laravel:
//! `php artisan migrate:status`), which boots the app and queries its database, so it runs less
//! often: at start (after the project's first check), when `HEAD` or `ORIG_HEAD` moves, on
//! request, after the migrate job or the install the tool depends on ends, and when the poll
//! finds its migration files changed. One thread reads them, one project after the other, never
//! two at once (`deps_read_migrations`).
//!
//! Installs and migrate run as the project's jobs (`jobs.rs`): output in its own tab
//! (`deps:<ecosystem>`, `deps:migrate:<tool>`), stopped with the project's other commands.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::UNIX_EPOCH;

use super::*;
use crate::deps::{self, DepReport, DepScan, DepScans, Inspection, MigrationRead, MigrationTool, MigrationsError, Probed, ScanDetails, Wanted};
use crate::registry::write_atomic;

/// The stat-only poll runs this often…
const DEPS_POLL_EVERY: u64 = 60;
/// …this many seconds into each period, away from the files poll at its start.
const DEPS_POLL_AT: u64 = 30;
/// After `HEAD` or `ORIG_HEAD` moves, Git may still be writing the work tree.
const HEAD_SETTLE: Duration = Duration::from_millis(1500);
/// An install is the job `deps:<ecosystem>`…
const JOB_PREFIX: &str = "deps:";
/// …and a migrate `deps:migrate:<tool>`.
const MIGRATE_PREFIX: &str = "deps:migrate:";
/// The last network scan per project, in the app's config folder.
const SCANS_FILE: &str = "dep-scans.json";

/// Each watched file's (`deps::watched`) modification time (ns) and size, or `None` while it
/// doesn't exist.
type Print = Vec<Option<(u128, u64)>>;

/// Projects waiting for one thread to work through them, and whether that thread runs.
#[derive(Default)]
struct Queue {
    paths: Mutex<HashSet<String>>,
    busy: AtomicBool,
}

impl Queue {
    fn paths(&self) -> MutexGuard<'_, HashSet<String>> {
        self.paths.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Adds `paths`; true when the caller is to start the thread that works through them.
    fn push(&self, paths: Vec<String>) -> bool {
        self.paths().extend(paths);
        !self.busy.swap(true, Ordering::SeqCst)
    }

    /// The working thread's next paths; `None` when there are none left, and the thread ends.
    fn next(&self) -> Option<Vec<String>> {
        loop {
            let paths: Vec<String> = self.paths().drain().collect();
            if !paths.is_empty() {
                return Some(paths);
            }
            self.busy.store(false, Ordering::SeqCst);
            // Queued between the drain and the store: this thread takes it after all.
            let queued = !self.paths().is_empty();
            if !queued || self.busy.swap(true, Ordering::SeqCst) {
                return None;
            }
        }
    }

    fn contains(&self, path: &str) -> bool {
        self.paths().contains(path)
    }
}

#[derive(Default)]
pub(super) struct Dependencies {
    state: Mutex<DepsState>,
    /// Projects waiting to be inspected.
    inspect: Queue,
    /// Projects waiting for their migration tools' status.
    status: Queue,
    /// The first check at start was asked for: projects listed later have their migrations read
    /// with their first check.
    started: AtomicBool,
    polling: AtomicBool,
    /// Held while the reports are built and sent, so an older set never overtakes a newer one.
    sending: Mutex<()>,
}

#[derive(Default)]
struct DepsState {
    checked: HashMap<String, Inspection>,
    prints: HashMap<String, Print>,
    /// What the login shell answered about runtimes and executables, as last asked.
    probed: Probed,
    scans: HashMap<String, DepScans>,
    scans_loaded: bool,
    /// Each project's last status read, by migration tool.
    migrations: HashMap<String, HashMap<&'static str, MigrationRead>>,
    /// Each project's migration files' listing as of its last status read, by migration tool.
    listings: HashMap<(String, &'static str), Option<u64>>,
    /// Projects whose migrations are to be read once their first check is done.
    after_check: HashSet<String>,
    /// Install jobs (project, ecosystem) from their start until the project's check after
    /// their end: `installing` clears in the same report that has the new check.
    installing: HashSet<(String, &'static str)>,
    /// Install jobs found gone at the last poll without their end being heard (a stop that
    /// outwaited the process): cleared if still so at the next.
    orphaned: HashSet<(String, &'static str)>,
    /// Who waits for a job's end (`deps_migrate`), by job key.
    waiters: HashMap<String, Vec<mpsc::Sender<JobResult>>>,
    /// The reports as last sent (or handed to the page).
    sent: String,
}

fn install_job(ecosystem: &str) -> String {
    format!("{JOB_PREFIX}{ecosystem}")
}

fn migrate_job(tool: &str) -> String {
    format!("{MIGRATE_PREFIX}{tool}")
}

fn stat(meta: &fs::Metadata) -> Option<(u128, u64)> {
    Some((meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos(), meta.len()))
}

fn fingerprint(dir: &Path) -> Print {
    deps::watched().iter().map(|name| stat(&fs::metadata(dir.join(name)).ok()?)).collect()
}

/// Reads the project's files; their sizes and times first, so a change while reading shows at
/// the next poll.
fn inspected(path: &str) -> (Print, Inspection) {
    let dir = Path::new(path);
    (fingerprint(dir), deps::inspect(dir))
}

/// The names, times and sizes of a tool's migration files (`MigrationTool::sources`: a folder's
/// files, or a file), as one number; `None` while none of them exists.
fn migrations_listing(tool: &dyn MigrationTool, dir: &Path) -> Option<u64> {
    let mut files: Vec<(String, u128, u64)> = Vec::new();
    let mut found = false;

    for source in tool.sources(dir) {
        let name = source.to_string_lossy().into_owned();
        let at = dir.join(&source);
        if let Ok(entries) = fs::read_dir(&at) {
            found = true;
            files.extend(entries.filter_map(|entry| {
                let entry = entry.ok()?;
                let (time, size) = stat(&entry.metadata().ok()?)?;
                Some((format!("{name}/{}", entry.file_name().to_string_lossy()), time, size))
            }));
        } else if let Some((time, size)) = fs::metadata(&at).ok().and_then(|meta| stat(&meta)) {
            found = true;
            files.push((name, time, size));
        }
    }
    if !found {
        return None;
    }

    files.sort();
    let mut hasher = DefaultHasher::new();
    files.hash(&mut hasher);
    Some(hasher.finish())
}

impl Core {
    fn deps_lock(&self) -> MutexGuard<'_, DepsState> {
        self.deps.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Every listed project's report, in list order; one never inspected is inspected now.
    pub fn deps_state(self: &Arc<Self>) -> Vec<DepReport> {
        let _sending = self.deps.sending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let paths = self.listed_paths();
        let unchecked: Vec<String> = {
            let state = self.deps_lock();
            paths.iter().filter(|path| !state.checked.contains_key(*path)).cloned().collect()
        };
        for path in &unchecked {
            self.deps_inspect(path);
        }
        // What they use that the login shell wasn't asked about yet is asked off this thread;
        // the reports with the answers follow as `deps`.
        let wanted = {
            let state = self.deps_lock();
            state.probed.unasked(Wanted::of(unchecked.iter().filter_map(|path| state.checked.get(path))))
        };
        if !wanted.is_empty() {
            let core = Arc::clone(self);
            thread::spawn(move || {
                core.deps_probe(wanted, false);
                core.deps_send();
            });
        }

        let reports = self.deps_reports(&paths);
        self.deps_lock().sent = serde_json::to_string(&reports).unwrap_or_default();
        reports
    }

    /// Inspects one listed project, or all, again, and asks the login shell for the runtimes and
    /// executables anew. Blocks until done.
    pub fn deps_check(self: &Arc<Self>, path: Option<&str>) {
        let listed = self.listed_paths();
        let paths: Vec<String> = match path {
            Some(path) => listed.into_iter().filter(|listed| listed == path).collect(),
            None => listed,
        };
        for path in &paths {
            self.deps_inspect(path);
        }
        let wanted = Wanted::of(self.deps_lock().checked.values());
        self.deps_probe(wanted, true);
        self.deps_send();
        self.deps_migrations(paths);
    }

    /// Runs the project's install for `ecosystem` (its manager's own install command, by its
    /// lock file) as its job `deps:<ecosystem>`. Refused while its dev server starts, where
    /// there is nothing to install, and when the login shell lacks the manager.
    pub fn deps_install(self: &Arc<Self>, path: &str, ecosystem: &str) -> Result<(), String> {
        let ecosystem = deps::ecosystem(ecosystem).ok_or_else(|| t!("core.error.depsEcosystem", ecosystem = ecosystem))?;
        self.deps_listed(path)?;
        {
            let inner = self.lock();
            let starting = inner.supervisor.pending(path) || inner.pending.get(path).is_some_and(|p| matches!(p.action, Action::Start | Action::Restart));
            if starting {
                return Err(t!("core.error.depsServerStarting"));
            }
        }
        let (command, needs) = deps::install_command(ecosystem, Path::new(path))?;
        self.deps_on_path(needs)?;
        self.deps_run(path, &install_job(ecosystem.id()), command, Some(ecosystem.id()), None)
    }

    /// Runs exactly `tool`'s forward-only migrate command (Laravel: `php artisan migrate`) as
    /// the project's job `deps:migrate:<tool>` and waits for it to end: an error when it fails
    /// or is stopped. Refused when the project doesn't use the tool and when the tool refuses
    /// (production; `deps::migrate_command`). Only ever called from the user's explicit confirm.
    pub fn deps_migrate(self: &Arc<Self>, path: &str, tool: &str) -> Result<(), String> {
        let tool = deps::migration_tool(tool).ok_or_else(|| t!("core.error.depsTool", tool = tool))?;
        self.deps_listed(path)?;
        let commands = deps::migrate_command(tool, Path::new(path))?;
        self.deps_on_path(commands.needs)?;
        let command = commands.migrate;
        let job = migrate_job(tool.id());
        let (sender, ended) = mpsc::channel();
        self.deps_run(path, &job, command, None, Some(sender))?;

        let key = job_key(path, &job);
        let mut gone = 0;
        let result = loop {
            match ended.recv_timeout(Duration::from_secs(1)) {
                Ok(result) => break Some(result),
                Err(mpsc::RecvTimeoutError::Disconnected) => break None,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // A job whose end was never heard (a stop that outwaited it) ends the wait too.
                    let inner = self.lock();
                    let running = inner.jobs.contains_key(&key) || inner.supervisor.pending(&key);
                    gone = if running { 0 } else { gone + 1 };
                    if gone >= 5 {
                        break inner.job_results.get(&key).cloned();
                    }
                }
            }
        };

        match result {
            Some(result) if result.stopped => Err(t!("core.error.depsCommandStopped", command = command)),
            Some(result) if result.ok => Ok(()),
            Some(result) => {
                Err(t!("core.error.depsCommandFailed", command = command, code = result.code.map_or_else(|| "-".to_string(), |code| code.to_string())))
            }
            None => Err(t!("core.error.depsCommandStopped", command = command)),
        }
    }

    /// The network scan of `ecosystem` (the outdated and the vulnerable packages, counted and
    /// listed), on a thread of its own; `running` until it ends, the last scan's counts and
    /// lists kept till then. A scan already running is left to finish.
    pub fn deps_scan(self: &Arc<Self>, path: &str, ecosystem: &str) -> Result<(), String> {
        let ecosystem = deps::ecosystem(ecosystem).ok_or_else(|| t!("core.error.depsEcosystem", ecosystem = ecosystem))?;
        self.deps_listed(path)?;
        let dir = Path::new(path);
        let manager = ecosystem.detect(dir).ok_or_else(|| t!("core.error.depsNothing", file = ecosystem.manifest()))?;
        self.deps_on_path(ecosystem.needs(dir, manager))?;
        let started = now_ms();
        {
            let mut state = self.deps_lock();
            self.load_scans(&mut state);
            let scans = state.scans.entry(path.to_string()).or_default();
            if scans.get(ecosystem.id()).is_some_and(|scan| scan.running) {
                return Ok(());
            }
            let last = scans.remove(ecosystem.id()).unwrap_or_default();
            scans.insert(ecosystem.id().to_string(), DepScan { at: started, running: true, ..last });
        }
        self.deps_send();

        let core = Arc::clone(self);
        let path = path.to_string();
        thread::spawn(move || {
            // `at` is when this attempt began.
            let scan = DepScan { at: started, ..deps::scan(ecosystem, Path::new(&path)) };
            core.deps_lock().scans.entry(path).or_default().insert(ecosystem.id().to_string(), scan);
            core.save_scans();
            core.deps_send();
        });
        Ok(())
    }

    /// What the last scan of `ecosystem` listed: the outdated packages and the advisories its
    /// counts are of. `None` for a scan kept from before they were listed, or one never made.
    pub fn deps_scan_details(&self, path: &str, ecosystem: &str) -> Option<ScanDetails> {
        let mut state = self.deps_lock();
        self.load_scans(&mut state);
        state.scans.get(path)?.get(ecosystem)?.details.clone()
    }

    fn deps_listed(&self, path: &str) -> Result<(), String> {
        if self.listed_paths().iter().any(|listed| listed == path) {
            Ok(())
        } else {
            Err(t!("core.error.depsNotListed", path = path))
        }
    }

    fn job_running(&self, key: &str) -> bool {
        let inner = self.lock();
        inner.jobs.contains_key(key) || inner.supervisor.pending(key)
    }

    /// Starts one of our jobs. An install (of the ecosystem `install`) shows as `installing`
    /// (sent before this returns) until the check after its end; `waiter` hears its end.
    fn deps_run(
        self: &Arc<Self>,
        path: &str,
        id: &str,
        command: &str,
        install: Option<&'static str>,
        waiter: Option<mpsc::Sender<JobResult>>,
    ) -> Result<(), String> {
        let key = job_key(path, id);
        {
            let mut inner = self.lock();
            if inner.jobs.contains_key(&key) || inner.supervisor.pending(&key) {
                return Err(t!("core.error.depsRunning"));
            }
            inner.supervisor.reset_attempts(&key);
        }
        {
            let mut state = self.deps_lock();
            if let Some(ecosystem) = install {
                state.installing.insert((path.to_string(), ecosystem));
            }
            if let Some(waiter) = waiter {
                state.waiters.entry(key.clone()).or_default().push(waiter);
            }
        }

        if !self.spawn_job_line(path, id, None, command, true) {
            // Another start of it won the race (or the app quits): that one owns `installing`.
            if let Some(ecosystem) = install.filter(|_| !self.job_running(&key)) {
                self.deps_lock().installing.remove(&(path.to_string(), ecosystem));
            }
            self.deps_send();
            return Err(t!("core.error.depsRunning"));
        }
        self.deps_send();
        Ok(())
    }

    /// One of a project's jobs ended (`jobs.rs`, off the main thread). After an install the
    /// project is inspected again, and `installing` clears together with the new check. After
    /// a migrate, or the install one of its migration tools depends on, its migrations are read
    /// again, `running` from before a waiting `deps_migrate` hears the end.
    pub(super) fn deps_job_done(self: &Arc<Self>, path: &str, id: &str) {
        let Some(rest) = id.strip_prefix(JOB_PREFIX) else {
            return;
        };
        let key = job_key(path, id);
        let migrate = id.starts_with(MIGRATE_PREFIX);
        let installed = if migrate { None } else { deps::ecosystem(rest).map(|ecosystem| ecosystem.id()) };

        // Migrate changes nothing a check reads.
        if let Some(ecosystem) = installed {
            self.deps_inspect_then(path, |state| {
                state.installing.remove(&(path.to_string(), ecosystem));
            });
        }
        self.deps_send();
        let depends = installed.is_some_and(|ecosystem| {
            let state = self.deps_lock();
            let found = state.checked.get(path).into_iter().flat_map(|inspection| &inspection.migrations);
            found.filter_map(|found| deps::migration_tool(found.tool)).any(|tool| tool.ecosystem() == ecosystem)
        });
        if migrate || depends {
            self.deps_migrations(vec![path.to_string()]);
        }

        let result = self.lock().job_results.get(&key).cloned();
        let waiters = self.deps_lock().waiters.remove(&key).unwrap_or_default();
        if let Some(result) = result {
            for waiter in waiters {
                let _ = waiter.send(result.clone());
            }
        }
    }

    /* ---------- the login shell ---------- */

    /// Asks the login shell (`deps::probe`, blocking) for what projects' reports need: with
    /// `anew` all of `wanted` again (something installed since shows), otherwise only what was
    /// never asked.
    fn deps_probe(&self, wanted: Wanted, anew: bool) {
        let wanted = if anew { wanted } else { self.deps_lock().probed.unasked(wanted) };
        if wanted.is_empty() {
            return;
        }
        if anew {
            process::reread_terminal_env();
        }
        let probed = deps::probe(&wanted);
        self.deps_lock().probed.merge(probed);
    }

    /// Refuses when the login shell lacks one of `needs`; it is asked once more before that, in
    /// case the executable was installed since.
    fn deps_on_path(&self, needs: &'static [&'static str]) -> Result<(), String> {
        let lacking = || {
            let state = self.deps_lock();
            needs.iter().copied().find(|tool| state.probed.lacks(tool))
        };
        if lacking().is_none() {
            return Ok(());
        }
        process::reread_terminal_env();
        let probed = deps::probe(&Wanted::tools(needs));
        self.deps_lock().probed.merge(probed);
        match lacking() {
            Some(tool) => Err(t!("core.error.depsToolMissing", tool = tool)),
            None => Ok(()),
        }
    }

    /* ---------- when to look ---------- */

    /// From the core's loop: everything at start, then the stat-only poll.
    pub(super) fn deps_tick(self: &Arc<Self>, tick: u64) {
        if tick == 0 {
            let core = Arc::clone(self);
            thread::spawn(move || {
                let paths = core.listed_paths();
                core.deps.started.store(true, Ordering::SeqCst);
                core.deps_queue(paths.clone());
                core.deps_send();
                core.deps_migrations(paths);
            });
        } else if tick % DEPS_POLL_EVERY == DEPS_POLL_AT {
            self.deps_poll();
        }
    }

    /// Inspects the projects whose watched files changed since their last inspection, or that
    /// were never inspected; forgets those no longer listed.
    fn deps_poll(self: &Arc<Self>) {
        if self.deps.polling.swap(true, Ordering::SeqCst) {
            return;
        }
        let core = Arc::clone(self);

        thread::spawn(move || {
            let paths = core.listed_paths();
            let prints: Vec<(String, Print)> = paths.iter().map(|path| (path.clone(), fingerprint(Path::new(path)))).collect();
            let tools: Vec<(String, &'static str)> = {
                let state = core.deps_lock();
                let checked = paths.iter().filter_map(|path| Some((path, state.checked.get(path)?)));
                checked.flat_map(|(path, inspection)| inspection.migrations.iter().map(move |found| (path.clone(), found.tool))).collect()
            };
            let listings: Vec<(Option<u64>, (String, &'static str))> = tools
                .into_iter()
                .filter_map(|(path, id)| Some((migrations_listing(deps::migration_tool(id)?, Path::new(&path)), (path, id))))
                .collect();
            let installing: Vec<(String, &'static str)> = core.deps_lock().installing.iter().cloned().collect();
            let gone: HashSet<(String, &'static str)> =
                installing.into_iter().filter(|(path, ecosystem)| !core.job_running(&job_key(path, &install_job(ecosystem)))).collect();
            let (changed, migrated) = {
                let mut state = core.deps_lock();
                state.checked.retain(|path, _| paths.contains(path));
                state.prints.retain(|path, _| paths.contains(path));
                state.migrations.retain(|path, _| paths.contains(path));
                state.listings.retain(|(path, _), _| paths.contains(path));
                state.after_check.retain(|path| paths.contains(path));
                // Migration files added, removed or changed since the tool's last status read;
                // one never read yet is read at start or after the project's first check.
                let mut migrated: Vec<String> =
                    listings.into_iter().filter(|(listing, key)| state.listings.get(key).is_some_and(|last| last != listing)).map(|(_, (path, _))| path).collect();
                migrated.dedup();
                let orphaned: Vec<(String, &'static str)> = gone.intersection(&state.orphaned).cloned().collect();
                for install in &orphaned {
                    state.installing.remove(install);
                }
                state.orphaned = gone;
                let mut changed: Vec<String> =
                    prints.into_iter().filter(|(path, print)| state.prints.get(path) != Some(print)).map(|(path, _)| path).collect();
                changed.extend(orphaned.into_iter().map(|(path, _)| path));
                (changed, migrated)
            };

            core.deps_queue(changed);
            // A project that left the list.
            core.deps_send();
            core.deps_migrations(migrated);
            core.deps.polling.store(false, Ordering::SeqCst);
        });
    }

    /// `HEAD` or `ORIG_HEAD` of these projects moved (`watch.rs`).
    pub(super) fn deps_heads_changed(self: &Arc<Self>, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        let core = Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(HEAD_SETTLE);
            core.deps_queue(paths.clone());
            core.deps_migrations(paths);
        });
    }

    /// The listed projects changed (`watch.rs`): new ones are inspected now, and after the start
    /// their migrations read.
    pub(super) fn deps_follow(self: &Arc<Self>, paths: &[String]) {
        let fresh: Vec<String> = {
            let state = self.deps_lock();
            paths.iter().filter(|path| !state.checked.contains_key(*path)).cloned().collect()
        };
        self.deps_queue(fresh.clone());
        if self.deps.started.load(Ordering::SeqCst) {
            self.deps_migrations(fresh);
        }
    }

    /// Inspects these projects off the caller's thread, one thread at a time; projects asked for
    /// meanwhile are taken right after. What they use that the login shell wasn't asked about
    /// yet is asked before they show, so a project's first report has its runtimes.
    fn deps_queue(self: &Arc<Self>, paths: Vec<String>) {
        if paths.is_empty() || !self.deps.inspect.push(paths) {
            return;
        }
        let core = Arc::clone(self);
        thread::spawn(move || {
            while let Some(paths) = core.deps.inspect.next() {
                let read: Vec<(Print, Inspection)> = paths.iter().map(|path| inspected(path)).collect();
                core.deps_probe(Wanted::of(read.iter().map(|(_, inspection)| inspection)), false);
                for (path, read) in paths.iter().zip(read) {
                    core.deps_store(path, read, |_| {});
                }
                core.deps_send();
                let checked: Vec<String> = {
                    let mut state = core.deps_lock();
                    paths.into_iter().filter(|path| state.after_check.remove(path)).collect()
                };
                core.deps_migrations(checked);
            }
        });
    }

    /* ---------- pending migrations ---------- */

    /// Reads these projects' pending migrations again (`deps_read_migrations`), off the caller's
    /// thread, one project after the other on one thread. A project asked for while its read
    /// runs is read once more after it. One without a migration tool is left out; one not
    /// checked yet is read after its first check. Nothing runs once the app quits.
    fn deps_migrations(self: &Arc<Self>, paths: Vec<String>) {
        if paths.is_empty() || self.lock().disposed {
            return;
        }
        let (paths, unchecked) = {
            let mut state = self.deps_lock();
            let (mut ready, mut unchecked) = (Vec::new(), Vec::new());
            for path in paths {
                let tools: Option<Vec<&'static str>> = state.checked.get(&path).map(|inspection| inspection.migrations.iter().map(|found| found.tool).collect());
                match tools {
                    Some(tools) if tools.is_empty() => {}
                    Some(tools) => {
                        let reads = state.migrations.entry(path.clone()).or_default();
                        for tool in tools {
                            reads.entry(tool).or_default().running = true;
                        }
                        ready.push(path);
                    }
                    None => {
                        state.after_check.insert(path.clone());
                        unchecked.push(path);
                    }
                }
            }
            (ready, unchecked)
        };
        self.deps_queue(unchecked);
        if paths.is_empty() {
            return;
        }
        self.deps_send();

        if !self.deps.status.push(paths) {
            return;
        }
        let core = Arc::clone(self);
        thread::spawn(move || {
            while let Some(paths) = core.deps.status.next() {
                for path in &paths {
                    core.deps_read_migrations(path);
                }
            }
        });
    }

    /// One project's status reads (`deps::migration_status`), tool by tool, blocking: `running`
    /// while one runs, sent when it flips and when the result lands. A failed read keeps the
    /// last list and says why; a tool whose executable the login shell lacks isn't run.
    fn deps_read_migrations(&self, path: &str) {
        let tools: Vec<(&'static str, &'static [&'static str])> = {
            let state = self.deps_lock();
            let found = state.checked.get(path).into_iter().flat_map(|inspection| &inspection.migrations);
            found.map(|found| (found.tool, found.commands.needs)).collect()
        };
        if tools.is_empty() || self.lock().disposed {
            let mut state = self.deps_lock();
            for read in state.migrations.get_mut(path).into_iter().flat_map(|reads| reads.values_mut()) {
                read.running = false;
            }
            drop(state);
            self.deps_send();
            return;
        }

        let dir = Path::new(path);
        for (id, needs) in tools {
            let Some(tool) = deps::migration_tool(id) else {
                continue;
            };
            let started = now_ms();
            let listing = migrations_listing(tool, dir);
            let lacking = {
                let mut state = self.deps_lock();
                state.listings.insert((path.to_string(), id), listing);
                let read = state.migrations.entry(path.to_string()).or_default().entry(id).or_default();
                read.running = true;
                read.at = started;
                needs.iter().any(|tool| state.probed.lacks(tool))
            };
            self.deps_send();

            let result = if lacking { Err(MigrationsError::ToolMissing) } else { deps::migration_status(tool, dir) };
            // Asked for again meanwhile: still `running`, read once more right after.
            let again = self.deps.status.contains(path);
            {
                let mut state = self.deps_lock();
                let read = state.migrations.entry(path.to_string()).or_default().entry(id).or_default();
                match result {
                    Ok(pending) => {
                        read.pending = pending;
                        read.error = None;
                    }
                    Err(error) => read.error = Some(error),
                }
                read.at = started;
                read.running = again;
            }
            self.deps_send();
        }
    }

    /* ---------- reports ---------- */

    fn deps_inspect(&self, path: &str) {
        self.deps_inspect_then(path, |_| {});
    }

    /// Inspects, then applies `then` under the same lock that stores the result.
    fn deps_inspect_then(&self, path: &str, then: impl FnOnce(&mut DepsState)) {
        self.deps_store(path, inspected(path), then);
    }

    /// Stores what `inspected` read and applies `then` under the same lock. A result older than
    /// the one stored (another thread read the files before this one) is dropped.
    fn deps_store(&self, path: &str, (print, inspection): (Print, Inspection), then: impl FnOnce(&mut DepsState)) {
        let mut state = self.deps_lock();
        if state.checked.get(path).is_none_or(|stored| stored.checked_at <= inspection.checked_at) {
            state.prints.insert(path.to_string(), print);
            state.checked.insert(path.to_string(), inspection);
        }
        then(&mut state);
    }

    /// The reports of the inspected ones of `paths`, with runtimes, tools, scans, status reads
    /// and running installs.
    fn deps_reports(&self, paths: &[String]) -> Vec<DepReport> {
        let mut state = self.deps_lock();
        self.load_scans(&mut state);
        paths
            .iter()
            .filter_map(|path| {
                let inspection = state.checked.get(path)?;
                // The counts only: what they are of is asked for apart (`deps_scan_details`).
                let scans: DepScans = state.scans.get(path).map(|scans| scans.iter().map(|(ecosystem, scan)| (ecosystem.clone(), scan.counts())).collect()).unwrap_or_default();
                let installing = deps::ECOSYSTEMS.iter().map(|ecosystem| ecosystem.id()).find(|ecosystem| state.installing.contains(&(path.clone(), *ecosystem)));
                Some(deps::report(path, inspection, &state.probed, scans, state.migrations.get(path), installing))
            })
            .collect()
    }

    /// Sends every report to the main window when anything in them changed.
    fn deps_send(&self) {
        let _sending = self.deps.sending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let reports = self.deps_reports(&self.listed_paths());
        let json = serde_json::to_string(&reports).unwrap_or_default();
        {
            let mut state = self.deps_lock();
            if state.sent == json {
                return;
            }
            state.sent = json;
        }
        self.emit(CoreEvent::Deps(reports));
    }

    /* ---------- the last scans, kept ---------- */

    fn scans_file(&self) -> Option<PathBuf> {
        Some(self.cfg.settings_file.parent()?.join(SCANS_FILE))
    }

    /// Reads the kept scans once; none of them is running any more.
    fn load_scans(&self, state: &mut DepsState) {
        if state.scans_loaded {
            return;
        }
        state.scans_loaded = true;
        let Some(text) = self.scans_file().and_then(|file| fs::read_to_string(file).ok()) else {
            return;
        };
        let Ok(kept) = serde_json::from_str::<HashMap<String, DepScans>>(&text) else {
            return;
        };
        for (path, mut scans) in kept {
            for scan in scans.values_mut() {
                scan.running = false;
            }
            state.scans.entry(path).or_insert(scans);
        }
    }

    fn save_scans(&self) {
        let Some(file) = self.scans_file() else {
            return;
        };
        let json = {
            let state = self.deps_lock();
            serde_json::to_string(&state.scans).unwrap_or_default()
        };
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = write_atomic(&file, &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command this app may ever run against a database, by hand: per migration tool,
    /// each spelling of its read-only status command and of its forward-only migrate command.
    /// A tool added to `deps::MIGRATION_TOOLS`, or a spelling added to a tool, fails the test
    /// below until its row is here, word for word.
    const EXPECTED: &[(&str, &[(&str, &str)])] = &[
        (
            "django",
            &[
                (".venv/bin/python manage.py showmigrations --plan", ".venv/bin/python manage.py migrate --no-input"),
                ("venv/bin/python manage.py showmigrations --plan", "venv/bin/python manage.py migrate --no-input"),
                ("uv run python manage.py showmigrations --plan", "uv run python manage.py migrate --no-input"),
                ("poetry run python manage.py showmigrations --plan", "poetry run python manage.py migrate --no-input"),
                ("pipenv run python manage.py showmigrations --plan", "pipenv run python manage.py migrate --no-input"),
                ("python3 manage.py showmigrations --plan", "python3 manage.py migrate --no-input"),
            ],
        ),
        (
            "doctrine",
            &[(
                "php bin/console doctrine:migrations:list --no-ansi --no-interaction",
                "php bin/console doctrine:migrations:migrate --no-interaction --no-ansi",
            )],
        ),
        (
            "efcore",
            &[
                ("dotnet ef migrations list --no-build --no-color --json --prefix-output", "dotnet ef database update"),
                ("dotnet ef migrations list --no-build --no-color --json --prefix-output", "dotnet ef database update"),
            ],
        ),
        (
            "flyway",
            &[
                ("./mvnw -B flyway:info", "./mvnw flyway:migrate"),
                ("mvn -B flyway:info", "mvn flyway:migrate"),
                ("./gradlew flywayInfo", "./gradlew flywayMigrate"),
                ("gradle flywayInfo", "gradle flywayMigrate"),
            ],
        ),
        ("laravel", &[("php artisan migrate:status --no-ansi --no-interaction", "php artisan migrate")]),
        (
            "prisma",
            &[
                ("npx --no-install prisma migrate status", "npx --no-install prisma migrate deploy"),
                (
                    "pnpm --config.verify-deps-before-run=false exec prisma migrate status",
                    "pnpm --config.verify-deps-before-run=false exec prisma migrate deploy",
                ),
                ("yarn exec prisma migrate status", "yarn exec prisma migrate deploy"),
                ("bunx --no-install prisma migrate status", "bunx --no-install prisma migrate deploy"),
            ],
        ),
        ("rails", &[("bin/rails db:migrate:status", "bin/rails db:migrate")]),
    ];

    /// What no command of a migration tool may contain: anything that drops, rolls back,
    /// rewrites, seeds or forces, and anything that would chain a second command.
    const FORBIDDEN: &[&str] = &[
        "fresh", "refresh", "rollback", "reset", "wipe", "drop", "seed", "flush", "force", "schema:load", "migrate dev", "down", "undo", "revert",
        "clean", "purge", "truncate", "delete", "repair", "push", "sync", ";", "&", "|", "`", "$", "<", ">", "\n",
    ];

    /// The database gate. Every registered migration tool runs only the commands listed above:
    /// one read-only status command and one forward-only migrate command per spelling, none of
    /// which drops, rolls back, seeds or forces. Laravel's migrate is exactly
    /// `php artisan migrate`; under `APP_ENV=production`, or without an `APP_ENV`, it is refused
    /// and nothing runs.
    #[test]
    fn migrate_is_plain_and_refused_under_production() {
        let registered: Vec<&str> = deps::MIGRATION_TOOLS.iter().map(|tool| tool.id()).collect();
        for (id, _) in EXPECTED {
            assert_eq!(registered.iter().filter(|known| *known == id).count(), 1, "{id} is listed here once and registered once");
        }
        for tool in deps::MIGRATION_TOOLS {
            let id = tool.id();
            let expected = EXPECTED.iter().find(|(known, _)| *known == id).unwrap_or_else(|| panic!("{id} has no row in EXPECTED")).1;
            let variants: Vec<(&str, &str)> = tool.variants().iter().map(|commands| (commands.status, commands.migrate)).collect();
            assert_eq!(variants, expected, "{id} runs exactly the commands listed here");
            assert!(!variants.is_empty(), "{id}");

            for (status, migrate) in variants {
                assert_ne!(status, migrate, "{id}");
                for command in [status, migrate] {
                    assert_eq!(command, command.trim(), "{id}");
                    for word in FORBIDDEN {
                        assert!(!command.to_lowercase().contains(*word), "{id}: {command:?} has {word:?}");
                    }
                }
            }
        }

        let laravel = deps::migration_tool("laravel").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("shop");
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, content: &str| fs::write(dir.join(name), content).unwrap();
        write("composer.json", r#"{"require":{"laravel/framework":"^13.0"}}"#);
        write("composer.lock", r#"{"packages":[{"name":"laravel/framework","version":"v13.1.0"}]}"#);
        write(".env", "APP_ENV=local\n");
        assert!(deps::migrate_command(laravel, &dir).is_err(), "no artisan: not Laravel");
        assert!(deps::migration_status(laravel, &dir).is_err(), "no artisan: not Laravel");

        write("artisan", "<?php\n");
        let commands = deps::migrate_command(laravel, &dir).unwrap();
        assert_eq!(commands.migrate, "php artisan migrate");
        assert_eq!(commands.status, "php artisan migrate:status --no-ansi --no-interaction");

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
        let path = dir.to_string_lossy().into_owned();
        core.add_project(&path).unwrap();
        let job = migrate_job("laravel");

        // Laravel reads a missing APP_ENV as production.
        // A line inside a multi-line value is no assignment.
        for env in [
            "APP_ENV=production\n",
            "APP_ENV=\"production\"\n",
            "APP_ENV=Production # live\n",
            "APP_ENV=local\nAPP_ENV=production\n",
            "DB_CONNECTION=mysql\n",
            "APP_ENV=production\nPRIVATE_KEY=\"-----BEGIN KEY-----\nAPP_ENV=local\n-----END KEY-----\"\n",
        ] {
            write(".env", env);
            assert!(deps::migrate_command(laravel, &dir).is_err(), "{env:?} must refuse");
            assert!(core.deps_migrate(&path, "laravel").is_err(), "{env:?} must refuse");
        }
        fs::remove_file(dir.join(".env")).unwrap();
        assert!(core.deps_migrate(&path, "laravel").is_err(), "no .env must refuse");
        // A tool there is none of runs nothing either.
        assert!(core.deps_migrate(&path, "laravel; php artisan migrate:fresh").is_err());

        assert!(!core.lock().jobs.contains_key(&job_key(&path, &job)));
        assert!(core.command_output(&path, &job).is_empty());
    }
}
