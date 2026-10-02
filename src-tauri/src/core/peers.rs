//! The other participants of the shared registry (VS Code windows): our record published and
//! theirs read on each heartbeat, the output of servers they run followed here, and the process
//! groups written down so that those a dead participant left behind can be reaped.

use std::io::{Seek, SeekFrom};

use super::*;
use super::snapshot::build_snapshot;
use crate::registry::ProjectState;

/// Most bytes read from a followed output file at once; a bigger burst skips to its newest part.
const TAIL_CHUNK: u64 = 256 * 1024;
/// A "line" that never ends is cut here.
const TAIL_LINE_MAX: usize = 64 * 1024;

/// Where we are in another participant's output file.
pub(super) struct Tail {
    file: PathBuf,
    offset: u64,
    /// Bytes after the last complete line.
    rest: Vec<u8>,
}

impl Core {
    /// Publish our state, read everybody else's, see where Claude stands.
    pub fn heartbeat(self: &Arc<Self>) {
        let own: Vec<ProjectState> = {
            let inner = self.lock();
            inner
                .runs
                .iter()
                .map(|(path, run)| ProjectState {
                    folder_path: path.clone(),
                    name: run.name.clone(),
                    running: true,
                    port: run.port,
                    url: run.url.clone(),
                    started_at: Some(run.started_at),
                    issue: inner.issues.get(path).cloned(),
                    output: None,
                })
                .collect()
        };

        self.registry.publish(own);

        let peers = self.registry.read_peers();
        let favourites = self.registry.read_favourites();
        let paths: Vec<String> = {
            let mut inner = self.lock();
            inner.peers = peers;
            inner.favourites = favourites;
            build_snapshot(&inner).projects.into_iter().map(|p| p.path).collect()
        };

        self.follow_projects(&paths);
        let hook = self.hook.status();
        {
            let mut inner = self.lock();
            inner.claude_hook = hook.installed;
            inner.claude_hook_outdated = hook.outdated;
        }

        if !self.refresh_claude() {
            self.notify();
        }
    }

    /// Servers another participant runs show their output here too: each mirrors it to a file
    /// under `output/` and names it in its record. The last lines are loaded once, then the file
    /// is followed every tick. A file that shrank was started again (or trimmed): read it anew.
    pub(super) fn follow_peer_output(&self) {
        let wanted: Vec<(String, PathBuf)> = {
            let inner = self.lock();
            let mut wanted: Vec<(String, PathBuf)> = Vec::new();

            for project in inner.peers.iter().flat_map(|peer| peer.projects.iter()) {
                if !project.running || inner.runs.contains_key(&project.folder_path) || wanted.iter().any(|(path, _)| path == &project.folder_path) {
                    continue;
                }
                if let Some(file) = project.output.as_deref().and_then(|relative| self.registry.output_file(relative)) {
                    wanted.push((project.folder_path.clone(), file));
                }
            }

            wanted
        };

        // Stopped or moved elsewhere: stop following. Its lines stay until the next run.
        let mut tails = std::mem::take(&mut self.lock().tails);
        tails.retain(|path, tail| wanted.iter().any(|(p, file)| p == path && file == &tail.file));

        for (path, file) in wanted {
            let Ok(len) = fs::metadata(&file).map(|meta| meta.len()) else {
                continue;
            };

            let fresh = !tails.contains_key(&path);
            let tail = tails.entry(path.clone()).or_insert_with(|| Tail { file: file.clone(), offset: 0, rest: Vec::new() });

            if len < tail.offset {
                tail.offset = 0;
                tail.rest.clear();
            }
            if len == tail.offset && !fresh {
                continue;
            }

            let from = tail.offset.max(len.saturating_sub(TAIL_CHUNK));
            let Some(mut bytes) = read_range(&file, from, len) else {
                continue;
            };

            // Started in the middle of the file: the first line is partial.
            if from > tail.offset {
                tail.rest.clear();
                let start = bytes.iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| i + 1);
                bytes.drain(..start);
            }

            tail.offset = len;
            tail.rest.extend_from_slice(&bytes);
            let mut lines = take_lines(&mut tail.rest);

            if fresh {
                self.lock().output.insert(path.clone(), VecDeque::new());
                let skip = lines.len().saturating_sub(OUTPUT_LINES);
                lines.drain(..skip);
            }

            for line in lines {
                self.push_line(&path, line);
            }
        }

        self.lock().tails = tails;
    }

    /// Process groups left by a participant that died without cleaning up: SIGTERM now, SIGKILL
    /// for whatever is left a moment later. The number of groups found.
    pub fn reap_orphans(&self) -> usize {
        if cfg!(windows) {
            // Windows reuses pids quickly; without a check on the image name it is not safe.
            return 0;
        }

        let groups: Vec<u32> = self.registry.take_orphans().into_iter().map(|orphan| orphan.pid).filter(|pid| process::group_alive(*pid)).collect();

        if !groups.is_empty() {
            let reaped = groups.clone();
            thread::spawn(move || process::stop_groups(&reaped, Duration::from_millis(600), Duration::from_secs(3)));
        }

        groups.len()
    }

    /// Writes down every process group we started and that may still have processes, for the
    /// orphan cleanup after a crash. One writer at a time, so the file is never stale or torn.
    pub(super) fn record_pids(&self) {
        let _writing = self.pids_lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries: Vec<PidEntry> = {
            let mut inner = self.lock();
            let lingering = inner.supervisor.lingering_groups();
            let mut entries: Vec<PidEntry> = inner
                .runs
                .iter()
                .map(|(path, run)| PidEntry { path: path.clone(), pid: run.pid })
                .chain(inner.jobs.iter().map(|(key, job)| PidEntry { path: key.clone(), pid: job.pid() }))
                .collect();
            for (pid, key) in lingering {
                if !entries.iter().any(|entry| entry.pid == pid) {
                    entries.push(PidEntry { path: key, pid });
                }
            }
            entries
        };
        entries.extend(self.terminal_sessions().values().filter_map(|session| session.pid_entry()));
        self.registry.record_pids(&entries);
    }
}

fn read_range(file: &Path, from: u64, to: u64) -> Option<Vec<u8>> {
    let mut handle = fs::File::open(file).ok()?;
    handle.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    handle.take(to.saturating_sub(from)).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Complete lines out of `rest`, cleaned like our own output; the unfinished end stays.
fn take_lines(rest: &mut Vec<u8>) -> Vec<String> {
    let end = match rest.iter().rposition(|&b| b == b'\n') {
        Some(at) => at + 1,
        None if rest.len() > TAIL_LINE_MAX => rest.len(),
        None => return Vec::new(),
    };

    let complete: Vec<u8> = rest.drain(..end).collect();
    String::from_utf8_lossy(&complete)
        .split_terminator('\n')
        .map(|line| crate::resolve::strip_ansi(line.trim_end_matches('\r')))
        .collect()
}
