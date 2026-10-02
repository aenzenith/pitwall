//! File-system events instead of timers for what Pitwall reads off the disk: Claude Code's
//! session logs, the hook's event files and each listed project's Git state (FSEvents on macOS,
//! through `notify`). Changes are gathered for a moment and handled on a thread of their own;
//! the slow safety poll (`POLL_EVERY`) reads everything again in case some were missed (sleep,
//! network volumes).

use std::sync::mpsc;

use notify::{EventKind, RecursiveMode, Watcher as _};

use super::*;

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
    routes: Arc<Mutex<Routes>>,
}

/// Which watched folder a changed path belongs to; shared with the thread that handles changes.
/// Paths as the file system reports them (symlinks resolved).
#[derive(Default)]
struct Routes {
    logs: Option<PathBuf>,
    events: Option<PathBuf>,
    /// A project's Git folder, and the project.
    git: Vec<(PathBuf, String)>,
}

#[derive(Default)]
struct Changes {
    /// Session logs: (folder under the projects folder, file name).
    logs: HashSet<(String, String)>,
    events: bool,
    /// Projects whose Git state changed.
    git: HashSet<String>,
    /// Events were dropped: read everything again.
    rescan: bool,
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

        if self.events.as_ref().is_some_and(|dir| path.starts_with(dir)) {
            // Deleting the files we applied reports too; only files still there count.
            if path.extension().is_some_and(|ext| ext == "event" || ext == "json") && path.exists() {
                changes.events = true;
            }
            return;
        }

        for (dir, project) in &self.git {
            let Ok(rest) = path.strip_prefix(dir) else {
                continue;
            };
            let name = rest.to_string_lossy();
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

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
            claude.apply_events();
        }

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

        self.watch_paths(paths);
        self.lock().git.retain(|path, _| paths.contains(path));
        self.refresh_git(added);
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
        *slot = Some(Watch { watcher, claude: Vec::new(), git: HashMap::new(), routes });
    }

    /// Watches the Claude folders and the Git folders of `paths`, and stops watching those of
    /// projects no longer listed. The watcher restarts only when something changed.
    fn watch_paths(&self, paths: &[String]) {
        let (logs, events) = match self.claude.lock() {
            Ok(claude) => (claude.root().to_path_buf(), claude.events_dir().to_path_buf()),
            Err(_) => return,
        };
        let mut slot = self.watch_slot();
        let Some(watch) = slot.as_mut() else {
            return;
        };
        // Our own folder; the hook creates it too.
        let _ = fs::create_dir_all(&events);

        let mut add: Vec<(PathBuf, RecursiveMode)> = Vec::new();
        let mut remove: Vec<PathBuf> = Vec::new();

        for (dir, mode) in [(logs.clone(), RecursiveMode::Recursive), (events.clone(), RecursiveMode::NonRecursive)] {
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
            if batch.add(dir, *mode).is_ok() && (dir == &logs || dir == &events) {
                watch.claude.push(dir.clone());
            }
        }
        let _ = batch.commit();

        let mut routes = watch.routes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        routes.logs = watch.claude.contains(&logs).then(|| canonical(&logs));
        routes.events = watch.claude.contains(&events).then(|| canonical(&events));
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
        let claude_changed = changes.rescan || changes.events || !changes.logs.is_empty();
        let mut git: Vec<String> = changes.git.into_iter().collect();

        if claude_changed {
            if let Ok(mut claude) = self.claude.lock() {
                if changes.rescan {
                    claude.rescan();
                }
                for (dir, file) in &changes.logs {
                    claude.refresh_log(dir, file);
                }
                if changes.events || changes.rescan {
                    // A turn that ended may have changed files: their Git state too.
                    git.extend(claude.apply_events());
                }
            }
            self.refresh_claude();
        }

        if changes.rescan {
            git = self.listed_paths();
        }
        self.refresh_git(git);
    }
}
