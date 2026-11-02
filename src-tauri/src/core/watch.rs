//! File-system events instead of timers for what Pitwall reads off the disk: Claude Code's
//! session logs and running sessions, the hook's event files, Codex's rollouts of the day (while
//! Codex is on) and its hook's events, and each listed project's Git state (FSEvents on macOS,
//! through `notify`). Changes are gathered for a moment and handled on a thread of their own;
//! the slow safety poll (`POLL_EVERY`) reads everything again in case some were missed (sleep,
//! network volumes).

use std::sync::atomic::Ordering;
use std::sync::mpsc;

use notify::{EventKind, RecursiveMode, Watcher as _};

use super::*;
use crate::git;

/// Changes are handled once this long passed without another…
const QUIET: Duration = Duration::from_millis(250);
/// …or this long after the first, whichever comes first.
const QUIET_MAX: Duration = Duration::from_secs(1);

pub(super) struct Watch {
    watcher: notify::RecommendedWatcher,
    /// The Claude folders watched so far; the projects folder may not exist yet.
    claude: Vec<PathBuf>,
    /// Each listed project's watched Git folders.
    git: HashMap<String, Vec<PathBuf>>,
    /// Codex's folders watched: its day folders, or where a day folder is yet to be made, and its
    /// hook's events. None while Codex is off.
    codex: Vec<PathBuf>,
    routes: Arc<Mutex<Routes>>,
}

/// Which watched folder a changed path belongs to; shared with the thread that handles changes.
/// Paths as the file system reports them (symlinks resolved).
#[derive(Default)]
struct Routes {
    logs: Option<PathBuf>,
    events: Option<PathBuf>,
    /// Claude Code's running sessions (`~/.claude/sessions`).
    sessions: Option<PathBuf>,
    /// A project's Git folder, and the project.
    git: Vec<(PathBuf, String)>,
    /// Codex's day folders followed: as events name them, and as the watch knows them.
    codex_days: Vec<(PathBuf, PathBuf)>,
    /// The folders a day folder of Codex's is yet to be made in.
    codex_parents: Vec<PathBuf>,
    codex_events: Option<PathBuf>,
}

#[derive(Default)]
struct Changes {
    /// Session logs: (folder under the projects folder, file name).
    logs: HashSet<(String, String)>,
    events: bool,
    /// A running session started, changed or ended.
    sessions: bool,
    /// Projects whose Git state changed.
    git: HashSet<String>,
    /// Projects whose `HEAD` or `ORIG_HEAD` moved: a checkout, pull, merge or rebase, after
    /// which their dependencies are checked again.
    heads: HashSet<String>,
    /// Events were dropped: read everything again.
    rescan: bool,
    /// Codex's rollouts that changed, as its watch knows their folders.
    codex_files: HashSet<PathBuf>,
    codex_events: bool,
    /// A day folder of Codex's may have been made: its folders are looked at again.
    codex_folders: bool,
}

impl Routes {
    fn route(&self, path: &Path, changes: &mut Changes) {
        if let Some(rest) = self.logs.as_ref().and_then(|root| path.strip_prefix(root).ok()) {
            // `<folder>/<session>.jsonl`; subagents' logs, one level deeper, aren't read.
            let parts: Vec<&str> = rest.iter().filter_map(|part| part.to_str()).collect();
            if let [dir, file] = parts.as_slice() {
                if file.ends_with(".jsonl") {
                    changes.logs.insert((dir.to_string(), file.to_string()));
                }
            }
            return;
        }

        if self.sessions.as_ref().is_some_and(|dir| path.starts_with(dir)) {
            changes.sessions = true;
            return;
        }

        if self.events.as_ref().is_some_and(|dir| path.starts_with(dir)) {
            // Deleting the files we applied reports too; only files still there count.
            if path.extension().is_some_and(|ext| ext == "event" || ext == "json") && path.exists() {
                changes.events = true;
            }
            return;
        }

        if self.codex_events.as_ref().is_some_and(|dir| path.starts_with(dir)) {
            if path.extension().is_some_and(|ext| ext == "event") && path.exists() {
                changes.codex_events = true;
            }
            return;
        }
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            if let Some((_, folder)) = self.codex_days.iter().find(|(dir, _)| dir == parent) {
                if name.to_str().is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl")) {
                    changes.codex_files.insert(folder.join(name));
                }
                return;
            }
            if self.codex_parents.iter().any(|dir| dir == parent) {
                changes.codex_folders = true;
                return;
            }
        }

        for (dir, project) in &self.git {
            let Ok(rest) = path.strip_prefix(dir) else {
                continue;
            };
            let name = rest.to_string_lossy();
            if matches!(name.as_ref(), "HEAD" | "ORIG_HEAD") {
                changes.heads.insert(project.clone());
            }
            // Branch, index and refs; never git's own lock files.
            if !name.ends_with(".lock") && (matches!(name.as_ref(), "HEAD" | "index" | "packed-refs") || rest.starts_with("refs")) {
                changes.git.insert(project.clone());
            }
        }
    }
}

/// The Git folders that change with a project's branch, index and refs: `.git` itself (not its
/// objects) and `.git/refs`; for a linked work tree, its own folder in the main repository.
fn git_dirs(project: &str) -> Vec<(PathBuf, RecursiveMode)> {
    let dot = Path::new(project).join(".git");

    if dot.is_dir() {
        let refs = dot.join("refs");
        let mut dirs = vec![(dot, RecursiveMode::NonRecursive)];
        if refs.is_dir() {
            dirs.push((refs, RecursiveMode::Recursive));
        }
        return dirs;
    }

    let Ok(text) = fs::read_to_string(&dot) else {
        return Vec::new();
    };
    match text.lines().find_map(|line| line.strip_prefix("gitdir:")).map(|dir| Path::new(project).join(dir.trim())) {
        Some(dir) if dir.is_dir() => vec![(dir, RecursiveMode::NonRecursive)],
        _ => Vec::new(),
    }
}

/// A watched folder as its events name it: FSEvents (macOS) reports resolved paths
/// (`/private/var/…` for `/var/…`), the watchers of Linux and Windows the path as it was given
/// (resolving it there would add `\\?\` on Windows, and no event would match).
fn canonical(path: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

impl Core {
    fn watch_slot(&self) -> MutexGuard<'_, Option<Watch>> {
        self.watch.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The paths of every project the app shows.
    pub(super) fn listed_paths(&self) -> Vec<String> {
        self.snapshot().projects.into_iter().map(|p| p.path).collect()
    }

    /// The safety poll, and the start: Claude's logs and the hook's events read in full, every
    /// project's Git state, and the watched folders set up (again, until they exist).
    pub(super) fn poll_files(self: &Arc<Self>) {
        self.start_watching();

        let paths = self.listed_paths();
        if let Ok(mut claude) = self.claude.lock() {
            claude.set_listed(&paths);
            claude.rescan();
        }
        crate::claude::take_events(&self.claude);
        // Codex's, while it is on; let go while it is off.
        self.follow_codex(&paths);

        self.watch_paths(&paths);
        self.lock().git.retain(|path, _| paths.contains(path));
        self.refresh_claude();
        self.refresh_git(paths);
    }

    /// The listed projects changed (added, removed, a window opened): follow their logs and Git
    /// folders now rather than at the next poll.
    pub(super) fn follow_projects(self: &Arc<Self>, paths: &[String]) {
        let added: Vec<String> = {
            let Ok(mut claude) = self.claude.lock() else {
                return;
            };
            let before = claude.listed().to_vec();
            if !claude.set_listed(paths) {
                return;
            }
            if claude.ready() {
                claude.rescan();
            }
            paths.iter().filter(|path| !before.contains(path)).cloned().collect()
        };

        self.follow_codex_projects(paths);
        self.watch_paths(paths);
        self.lock().git.retain(|path, _| paths.contains(path));
        self.refresh_git(added);
        self.deps_follow(paths);
    }

    /// Creates the watcher and the thread that handles its changes, once.
    fn start_watching(self: &Arc<Self>) {
        let mut slot = self.watch_slot();
        if slot.is_some() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        let Ok(watcher) = notify::recommended_watcher(sender) else {
            return;
        };
        let routes = Arc::new(Mutex::new(Routes::default()));
        let core = Arc::clone(self);
        let shared = Arc::clone(&routes);

        thread::spawn(move || core.handle_changes(&receiver, &shared));
        *slot = Some(Watch { watcher, claude: Vec::new(), git: HashMap::new(), codex: Vec::new(), routes });
    }

    /// Watches Codex's folders while it is on (`CodexWatch::folders`: each home's today's and
    /// yesterday's, a day folder not made yet by the nearest folder above it that is there) and
    /// its hook's events; none of them while it is off.
    pub(super) fn watch_codex(&self) {
        let (days, events) = match self.codex.lock() {
            Ok(codex) => {
                let days = codex.folders();
                let events = (!days.is_empty()).then(|| codex.events_dir().to_path_buf());
                (days, events)
            }
            Err(_) => return,
        };
        let mut slot = self.watch_slot();
        let Some(watch) = slot.as_mut() else {
            return;
        };

        let mut found: Vec<PathBuf> = Vec::new();
        let mut parents: Vec<PathBuf> = Vec::new();
        for day in &days {
            if day.is_dir() {
                found.push(day.clone());
            } else if let Some(above) = day.ancestors().skip(1).take(4).find(|dir| dir.is_dir()) {
                parents.push(above.to_path_buf());
            }
        }
        if let Some(events) = &events {
            let _ = fs::create_dir_all(events);
        }
        let mut wanted: Vec<PathBuf> = found.iter().chain(&parents).cloned().chain(events.clone()).collect();
        wanted.sort();
        wanted.dedup();

        if wanted != watch.codex {
            let mut batch = watch.watcher.paths_mut();
            for dir in watch.codex.iter().filter(|dir| !wanted.contains(dir)) {
                let _ = batch.remove(dir);
            }
            let mut watched: Vec<PathBuf> = watch.codex.iter().filter(|dir| wanted.contains(dir)).cloned().collect();
            for dir in wanted.iter().filter(|dir| !watch.codex.contains(dir)) {
                if batch.add(dir, RecursiveMode::NonRecursive).is_ok() {
                    watched.push(dir.clone());
                }
            }
            let _ = batch.commit();
            watch.codex = watched;
        }

        let mut routes = watch.routes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        routes.codex_days = found.iter().filter(|dir| watch.codex.contains(dir)).map(|dir| (canonical(dir), dir.clone())).collect();
        routes.codex_parents = parents.iter().filter(|dir| watch.codex.contains(dir)).map(|dir| canonical(dir)).collect();
        routes.codex_events = events.filter(|dir| watch.codex.contains(dir)).map(|dir| canonical(&dir));
    }

    /// Watches the Claude folders and the Git folders of `paths`, and stops watching those of
    /// projects no longer listed. The watcher restarts only when something changed.
    fn watch_paths(&self, paths: &[String]) {
        let (logs, events) = match self.claude.lock() {
            Ok(claude) => (claude.root().to_path_buf(), claude.events_dir().to_path_buf()),
            Err(_) => return,
        };
        let sessions = self.sessions_dir();
        let mut slot = self.watch_slot();
        let Some(watch) = slot.as_mut() else {
            return;
        };
        // Our own folder; the hook creates it too.
        let _ = fs::create_dir_all(&events);

        let mut add: Vec<(PathBuf, RecursiveMode)> = Vec::new();
        let mut remove: Vec<PathBuf> = Vec::new();

        let claude_dirs = [(Some(logs.clone()), RecursiveMode::Recursive), (Some(events.clone()), RecursiveMode::NonRecursive), (sessions.clone(), RecursiveMode::NonRecursive)];
        for (dir, mode) in claude_dirs.into_iter().filter_map(|(dir, mode)| Some((dir?, mode))) {
            if !watch.claude.contains(&dir) && dir.is_dir() {
                add.push((dir, mode));
            }
        }

        let wanted: HashMap<&String, Vec<(PathBuf, RecursiveMode)>> = paths.iter().map(|path| (path, git_dirs(path))).collect();
        watch.git.retain(|project, dirs| {
            let keep = wanted.get(project).is_some_and(|want| want.iter().map(|(dir, _)| dir).eq(dirs.iter()));
            if !keep {
                remove.append(dirs);
            }
            keep
        });
        for (project, dirs) in &wanted {
            if !watch.git.contains_key(*project) {
                add.extend(dirs.iter().cloned());
                watch.git.insert((*project).clone(), dirs.iter().map(|(dir, _)| dir.clone()).collect());
            }
        }

        if add.is_empty() && remove.is_empty() {
            return;
        }

        let mut batch = watch.watcher.paths_mut();
        for dir in &remove {
            let _ = batch.remove(dir);
        }
        for (dir, mode) in &add {
            if batch.add(dir, *mode).is_ok() && (dir == &logs || dir == &events || Some(dir) == sessions.as_ref()) {
                watch.claude.push(dir.clone());
            }
        }
        let _ = batch.commit();

        let mut routes = watch.routes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        routes.logs = watch.claude.contains(&logs).then(|| canonical(&logs));
        routes.events = watch.claude.contains(&events).then(|| canonical(&events));
        routes.sessions = sessions.filter(|dir| watch.claude.contains(dir)).map(|dir| canonical(&dir));
        routes.git = watch.git.iter().filter_map(|(project, dirs)| Some((canonical(dirs.first()?), project.clone()))).collect();
    }

    /// The watcher's thread: gathers changes until they settle, then handles them.
    fn handle_changes(self: Arc<Self>, receiver: &mpsc::Receiver<notify::Result<notify::Event>>, routes: &Mutex<Routes>) {
        let take = |event: notify::Result<notify::Event>, changes: &mut Changes| {
            let Ok(event) = event else {
                return;
            };
            if event.need_rescan() {
                changes.rescan = true;
            }
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let routes = routes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            for path in &event.paths {
                routes.route(path, changes);
            }
        };

        while let Ok(first) = receiver.recv() {
            let mut changes = Changes::default();
            let deadline = Instant::now() + QUIET_MAX;
            take(first, &mut changes);

            loop {
                match receiver.recv_timeout(QUIET.min(deadline.saturating_duration_since(Instant::now()))) {
                    Ok(event) => take(event, &mut changes),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }

            self.apply_changes(changes);
        }
    }

    fn apply_changes(self: &Arc<Self>, changes: Changes) {
        if changes.sessions || changes.rescan {
            self.sessions_changed();
        }

        self.deps_heads_changed(changes.heads.into_iter().collect());

        // A session's own status too: `busy` keeps a turn that ended at work.
        let claude_changed = changes.rescan || changes.events || changes.sessions || !changes.logs.is_empty();
        let mut git: Vec<String> = changes.git.into_iter().collect();

        if claude_changed {
            if let Ok(mut claude) = self.claude.lock() {
                if changes.rescan {
                    claude.rescan();
                }
                for (dir, file) in &changes.logs {
                    claude.refresh_log(dir, file);
                }
            }
            if changes.events || changes.rescan {
                // A turn that ended may have changed files: their Git state too.
                git.extend(crate::claude::take_events(&self.claude));
            }
        }

        // Codex's, while it is on (its folders aren't watched while it is off).
        let codex_changed = (changes.rescan || changes.codex_events || changes.codex_folders || !changes.codex_files.is_empty()) && self.agent_on(Agent::Codex);
        if codex_changed {
            if changes.codex_events || changes.rescan {
                git.extend(self.codex_take_events());
            }
            if let Ok(mut codex) = self.codex.lock() {
                // Events may tell of a session whose rollout is an older day's.
                if changes.rescan || changes.codex_events || changes.codex_folders {
                    crate::agent::AgentWatch::rescan(&mut *codex);
                }
                for file in &changes.codex_files {
                    codex.refresh_file(file);
                }
            }
            if changes.codex_events || changes.codex_folders || changes.rescan {
                self.watch_codex();
            }
        }

        if claude_changed || codex_changed {
            self.refresh_claude();
        }

        if changes.rescan {
            git = self.listed_paths();
        }
        self.refresh_git(git);
    }

    /// Branch and changes of these projects, off the caller's thread. One thread at a time
    /// reads them; projects asked for meanwhile are read right after.
    pub(super) fn refresh_git(self: &Arc<Self>, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        self.git_queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).extend(paths);
        if self.git_busy.swap(true, Ordering::SeqCst) {
            return;
        }

        let core = Arc::clone(self);

        thread::spawn(move || loop {
            let paths: Vec<String> = core.git_queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).drain().collect();

            if paths.is_empty() {
                core.git_busy.store(false, Ordering::SeqCst);
                // Queued between the drain and the store: this thread takes it after all.
                let queued = !core.git_queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty();
                if queued && !core.git_busy.swap(true, Ordering::SeqCst) {
                    continue;
                }
                return;
            }
            core.detect_managers(&paths);

            let states: Vec<(String, Result<Option<GitInfo>, git::TimedOut>)> = paths.into_iter().map(|path| (path.clone(), git::read_status(&path))).collect();
            {
                let mut inner = core.lock();
                for (path, state) in states {
                    match state {
                        Ok(Some(info)) => inner.git.insert(path, info),
                        Ok(None) => inner.git.remove(&path),
                        // Not read in time: what was read last stays until a read gets through.
                        Err(git::TimedOut) => None,
                    };
                }
            }
            core.notify();
        });
    }

    /// Settings stopped naming the package manager: each listed project's lock file is looked
    /// for now (`detect_managers`), not at the next poll, as its row shows npm until then.
    pub(super) fn managers_unnamed(self: &Arc<Self>) {
        self.refresh_git(self.listed_paths());
    }

    /// The package manager of each of these projects whose lock file wasn't looked for yet
    /// (`servers::run_command` shows npm until then): their folders are listed here, off the
    /// core's lock, as a large one or one on a share that stopped answering takes its time.
    /// Nothing is listed while Settings names the manager. A start that read the folder
    /// meanwhile is kept.
    fn detect_managers(&self, paths: &[String]) {
        let unknown: Vec<(&String, ProjectSettings)> = {
            let inner = self.lock();
            if PackageManager::parse(&inner.settings.package_manager).is_some() {
                return;
            }
            paths.iter().filter(|path| !inner.managers.contains_key(*path)).map(|path| (path, inner.settings.project(path))).collect()
        };
        if unknown.is_empty() {
            return;
        }

        // In the folder it runs in; one whose folder isn't there is looked for again at the next poll.
        let detected: Vec<(&String, PackageManager)> =
            unknown.into_iter().filter_map(|(path, project)| Some((path, servers::detect_manager(&project.work_dir(path)?)))).collect();
        let mut inner = self.lock();
        for (path, manager) in detected {
            inner.managers.entry(path.clone()).or_insert(manager);
        }
    }
}
