//! The Sessions page looking back: the sessions of an earlier day, or of every day there is
//! something kept of, one ended row each. Built when asked from what is on disk: the day files
//! Pitwall wrote (`activity.rs`: when Claude worked and waited, in the listed projects) and
//! Claude Code's session logs, of which only the file's own times and, from its last lines, the
//! session's name, its folder and the time on its last message are read; never what a message
//! says. Tokens are counted for today alone
//! (`spend.rs`), so these rows have none. The page puts the state of a session that is still
//! open over its row.

use super::activity::day_bounds;
use super::reveal::is_session_id;
use super::sessions::{RowPhase, SessionOrigin, SessionRow, SessionSpan, SessionToday};
use super::snapshot::build_snapshot;
use super::*;
use crate::claude::{encode_project_path, inside, log_brief, LogBrief};
use crate::clock::day_name;
use crate::registry::path_key;

/// Every day there is something kept of, as the page asks for it.
const ALL: &str = "all";

/// What a session's log's last lines told (`claude::log_brief`), with the log's size and last
/// write then: read again only once it changed.
pub(super) struct Brief {
    modified: u64,
    size: u64,
    told: LogBrief,
}

/// A session's log under Claude Code's projects folder.
struct Found {
    /// The folder it is in, named after the project by Claude Code.
    dir: String,
    file: PathBuf,
    modified: u64,
    /// When the file was made, where the file system tells.
    created: Option<u64>,
    size: u64,
}

fn epoch_ms(time: std::time::SystemTime) -> Option<u64> {
    time.duration_since(std::time::UNIX_EPOCH).ok().map(|since| since.as_millis() as u64)
}

/// Every session's log under `root` (`<folder>/<session id>.jsonl`; subagents log deeper and
/// aren't sessions), by session id: the last written, should a session have several.
fn session_logs(root: &Path) -> HashMap<String, Found> {
    let mut found: HashMap<String, Found> = HashMap::new();

    for dir in fs::read_dir(root).into_iter().flatten().flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();

        for entry in fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            let file = entry.path();
            if file.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let Some(id) = file.file_stem().and_then(|stem| stem.to_str()).filter(|id| is_session_id(id)).map(str::to_string) else {
                continue;
            };
            let Some(meta) = entry.metadata().ok().filter(fs::Metadata::is_file) else {
                continue;
            };
            let Some(modified) = meta.modified().ok().and_then(epoch_ms) else {
                continue;
            };
            if found.get(&id).is_some_and(|known| known.modified >= modified) {
                continue;
            }

            let created = meta.created().ok().and_then(epoch_ms);
            found.insert(id, Found { dir: name.clone(), file, modified, created, size: meta.len() });
        }
    }

    found
}

impl Core {
    /// The sessions of `period`: a day (`YYYY-MM-DD`, local; anything else is today) or `all`.
    /// A day's are those Pitwall wrote down working or waiting then, and those whose log was
    /// made then or has its last message then; `since` is when each was last heard of in those
    /// days.
    pub fn sessions_history(&self, period: &str) -> SessionsView {
        let now = now_ms();
        let every = period == ALL;
        let (start, end) = if every { (0, u64::MAX) } else { day_bounds(period).or_else(|| day_bounds(&day_name(now))).unwrap_or((0, u64::MAX)) };
        let within = |at: &u64| (start..end).contains(at);

        let (hook, projects) = {
            let inner = self.lock();
            let projects: Vec<(String, String)> = build_snapshot(&inner).projects.into_iter().map(|p| (p.path, p.name)).collect();
            (inner.claude_hook, projects)
        };

        let days = if every { self.sessions_ever(now) } else { self.sessions_on(start, now.clamp(start, end)) };
        let logs = session_logs(&self.cfg.claude_dir);

        // What their last lines tell, of the logs written since those days began (one of an
        // earlier day's sessions may have been written to again since): only the logs that
        // changed since they were last read are read.
        let mut briefs = std::mem::take(&mut self.sessions_cache().briefs);
        briefs.retain(|id, _| logs.contains_key(id));
        for (id, log) in logs.iter().filter(|(_, log)| log.modified >= start) {
            if briefs.get(id).is_none_or(|brief| brief.modified != log.modified || brief.size != log.size) {
                let told = log_brief(&log.file, log.size).unwrap_or_default();
                briefs.insert(id.clone(), Brief { modified: log.modified, size: log.size, told });
            }
        }
        // When each was last heard: its last message's time, else its log's last write.
        let last = |id: &String| briefs.get(id).and_then(|brief| brief.told.at).or(logs.get(id).map(|log| log.modified));

        let mut ids: Vec<&String> = logs
            .iter()
            .filter(|(id, log)| last(id).as_ref().is_some_and(within) || log.created.as_ref().is_some_and(within))
            .map(|(id, _)| id)
            .chain(days.keys().filter(|id| is_session_id(id)))
            .collect();
        ids.sort();
        ids.dedup();

        let mut rows: Vec<SessionRow> = Vec::new();
        for id in ids {
            let (log, day, told) = (logs.get(id), days.get(id), briefs.get(id).map(|brief| &brief.told));

            // Where it ran: as its log says; else the project it was written down under, or the
            // listed one Claude Code named its log's folder after.
            let named = || {
                let dir = log?.dir.as_str();
                projects.iter().find(|(path, _)| *path_key(&encode_project_path(path)) == *path_key(dir)).map(|(path, _)| path.clone())
            };
            let Some(folder) = told.and_then(|t| t.cwd.clone()).or_else(|| day.map(|d| d.path.clone())).or_else(named).filter(|folder| !folder.is_empty()) else {
                continue;
            };
            let listed = day
                .and_then(|d| projects.iter().find(|(path, _)| same_path(path, &d.path)))
                .or_else(|| projects.iter().filter(|(path, _)| inside(&folder, path)).max_by_key(|(path, _)| path.len()));

            let heard = [day.map(|d| d.end), last(id).filter(within), log.and_then(|l| l.created).filter(within)];
            rows.push(SessionRow {
                id: id.clone(),
                path: listed.map(|(path, _)| path.clone()),
                project: listed.map_or_else(|| folder_name(&folder), |(_, name)| name.clone()),
                folder,
                title: told.and_then(|t| t.title.clone()).or_else(|| day.and_then(|d| d.title.clone())),
                phase: RowPhase::Ended,
                turn: None,
                tool: None,
                since: heard.into_iter().flatten().max(),
                running: None,
                origin: SessionOrigin::Unknown,
                today: day.map(|d| SessionToday {
                    work: d.work,
                    wait: d.wait,
                    turns: d.turns,
                    spans: d.spans.iter().map(|&(start, end, working)| SessionSpan { start, end, kind: if working { "work" } else { "wait" } }).collect(),
                }),
                spend: None,
                subagents: 0,
            });
        }
        self.sessions_cache().briefs = briefs;

        // The last heard of first.
        rows.sort_by(|a, b| b.since.cmp(&a.since).then(a.id.cmp(&b.id)));

        SessionsView { now, hook, unlisted: rows.iter().filter(|row| row.path.is_none()).count(), sessions: rows }
    }
}
