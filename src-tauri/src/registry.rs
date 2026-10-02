//! The `~/.pitwall/` registry shared with the VS Code extension.
//! Same files and fields as the extension's `src/registry.ts`; the app is one more participant.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const STALE_MS: u64 = 20_000;
/// A record this old is deleted by whoever sweeps; only then may its pids be reaped.
const DEAD_MS: u64 = STALE_MS * 3;
const COMMAND_TTL_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// `crashed` | `error` | `unresponsive`
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    pub folder_path: String,
    pub name: String,
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<Issue>,
    /// While it runs: the server's output, mirrored to `output/<windowId>/<file>.log` and given
    /// relative to the registry folder. Missing in old extension versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRecord {
    pub window_id: String,
    pub title: String,
    pub updated_at: u64,
    pub projects: Vec<ProjectState>,
    /// Folders this participant owns even when stopped. Missing in old extension versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<String>>,
    /// Commands it takes besides start/stop/restart. Old extension versions treat an unknown
    /// command as `start`, so a new one goes only to participants that list it here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    /// Shell pids of its terminals, to find the one a Claude session runs in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminals: Option<Vec<u32>>,
}

impl WindowRecord {
    /// Projects this record owns: its roots, plus whatever it actually runs. Without `roots`
    /// (old versions) only running projects count, as favourites may have been published too.
    pub fn owned(&self) -> impl Iterator<Item = &ProjectState> {
        self.projects.iter().filter(move |project| project.running || self.has_root(&project.folder_path))
    }

    pub fn has_root(&self, folder_path: &str) -> bool {
        self.roots.as_ref().is_some_and(|roots| roots.iter().any(|root| same_path(root, folder_path)))
    }

    pub fn takes(&self, feature: &str) -> bool {
        self.features.as_ref().is_some_and(|features| features.iter().any(|f| f == feature))
    }

    /// The pid of the process that writes this record: ids are `<pid>-<time>`, both base 36.
    pub fn pid(&self) -> Option<u32> {
        participant_pid(&self.window_id)
    }
}

/// A path as participants compare it (rule 3 of PROTOCOL.md). On Windows letter case doesn't
/// count, `/` is `\` and a trailing separator is dropped (unless the path is a drive's root):
/// VS Code writes `c:\…`, the app and Claude Code `C:\…`. Elsewhere the path as it is.
pub fn path_key(path: &str) -> Cow<'_, str> {
    fold_path(path, cfg!(windows))
}

fn fold_path(path: &str, windows: bool) -> Cow<'_, str> {
    if !windows {
        return Cow::Borrowed(path);
    }

    let mut key = path.replace('/', "\\").to_lowercase();
    while key.len() > 3 && key.ends_with('\\') {
        key.pop();
    }
    Cow::Owned(key)
}

/// The two paths name the same folder, as participants compare paths (see `path_key`).
pub fn same_path(a: &str, b: &str) -> bool {
    a == b || path_key(a) == path_key(b)
}

/// The pid in a participant id (`<pid>-<time>`, both base 36).
fn participant_pid(window_id: &str) -> Option<u32> {
    let (pid, _) = window_id.split_once('-')?;
    u32::from_str_radix(pid, 36).ok()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCommand {
    pub target: String,
    /// `start` | `stop` | `restart` | `reveal-claude`
    pub action: String,
    pub folder_path: String,
    pub issued_by: String,
    pub issued_at: u64,
    /// `reveal-claude`: the session to bring up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// `reveal-claude`: the shell pid of the terminal it runs in, when it runs in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favourite {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PidEntry {
    pub path: String,
    pub pid: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PidRecord {
    window_id: String,
    #[serde(default)]
    entries: Vec<PidEntry>,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(crate) fn base36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();

    loop {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }

    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// A name part no other call in this process returns: a counter, and the clock's nanoseconds for
/// another process that got the same pid. Base 36.
fn unique() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);

    base36(count * 1_000_000_000 + nanos as u64)
}

/// Write to `<file>.<pid>-<unique>.tmp`, then rename: readers never see half a file, two threads
/// writing one file never share a temp, and the temp name doesn't end in `.json`, so directory
/// scans skip it.
pub fn write_atomic(file: &Path, content: &str) -> std::io::Result<()> {
    let temp = PathBuf::from(format!("{}.{}-{}.tmp", file.display(), std::process::id(), unique()));
    let written = fs::write(&temp, content).and_then(|()| fs::rename(&temp, file));

    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

fn read_json<T: for<'de> Deserialize<'de>>(file: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(file).ok()?).ok()
}

fn list_json(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect()
        })
        .unwrap_or_default()
}

/// Copies favourites and Claude seen state from older storage folders, once: only when the
/// target has no such file, from the first source that has one. Never overwrites.
pub fn adopt_old_storage(target: &Path, sources: &[PathBuf]) {
    for file in ["favorites.json", "claude-seen.json"] {
        let to = target.join(file);

        if to.exists() {
            continue;
        }

        if let Some(from) = sources.iter().map(|dir| dir.join(file)).find(|candidate| candidate.exists()) {
            let _ = fs::create_dir_all(target);
            let _ = fs::copy(from, to);
        }
    }
}

/// A participant still counts as alive while its record is younger than the sweep's limit, or
/// while the process in its id runs. An id without a pid has only its record to go by.
fn participant_alive(window_id: &str, records: &[WindowRecord], now: u64) -> bool {
    let fresh = records.iter().any(|record| record.window_id == window_id && now.saturating_sub(record.updated_at) <= DEAD_MS);
    fresh || participant_pid(window_id).is_some_and(crate::process::process_alive)
}

pub struct Registry {
    pub id: String,
    dir: PathBuf,
    title: String,
    /// Registry folders of older extension versions (VS Code's own storage). VS Code windows
    /// not reloaded since the extension moved to `~/.pitwall/` still live there; they are read
    /// and sent commands, never otherwise written to.
    legacy: Vec<PathBuf>,
    /// Which folder each peer was last seen in, so its commands land where it reads them.
    peer_dirs: Mutex<HashMap<String, PathBuf>>,
}

impl Registry {
    pub fn new(dir: PathBuf, title: &str) -> Self {
        let id = format!("{}-{}", base36(std::process::id() as u64), base36(now_ms()));

        for sub in ["windows", "commands", "pids"] {
            let _ = fs::create_dir_all(dir.join(sub));
        }

        Self { id, dir, title: title.to_string(), legacy: Vec::new(), peer_dirs: Mutex::new(HashMap::new()) }
    }

    /// Also read peers from these older registry folders (those that exist).
    pub fn with_legacy(mut self, dirs: Vec<PathBuf>) -> Self {
        self.legacy = dirs.into_iter().filter(|dir| dir.join("windows").is_dir()).collect();
        self
    }

    /// A participant with a fixed id, standing in for a VS Code window in tests.
    #[cfg(test)]
    pub fn with_id(dir: PathBuf, title: &str, id: &str) -> Self {
        Self { id: id.to_string(), ..Self::new(dir, title) }
    }

    fn own_file(&self) -> PathBuf {
        self.dir.join("windows").join(format!("{}.json", self.id))
    }

    fn own_pids(&self) -> PathBuf {
        self.dir.join("pids").join(format!("{}.json", self.id))
    }

    /// Heartbeat. The app owns a project only while it runs it, so `roots` stays empty: a
    /// project open in a VS Code window keeps belonging to that window.
    pub fn publish(&self, projects: Vec<ProjectState>) {
        let record = WindowRecord {
            window_id: self.id.clone(),
            title: self.title.clone(),
            updated_at: now_ms(),
            projects,
            roots: Some(Vec::new()),
            features: None,
            terminals: None,
        };

        if let Ok(json) = serde_json::to_string(&record) {
            let _ = write_atomic(&self.own_file(), &json);
        }
    }

    /// Live records of every other participant.
    pub fn read_peers(&self) -> Vec<WindowRecord> {
        let now = now_ms();
        let mut peers: Vec<WindowRecord> = Vec::new();
        let mut dirs = HashMap::new();

        for dir in std::iter::once(&self.dir).chain(self.legacy.iter()) {
            for record in list_json(&dir.join("windows")).iter().filter_map(|file| read_json::<WindowRecord>(file)) {
                let live = record.window_id != self.id && now.saturating_sub(record.updated_at) <= STALE_MS;

                if live && !peers.iter().any(|peer| peer.window_id == record.window_id) {
                    dirs.insert(record.window_id.clone(), dir.clone());
                    peers.push(record);
                }
            }
        }

        if let Ok(mut known) = self.peer_dirs.lock() {
            *known = dirs;
        }

        peers
    }

    pub fn send(&self, target: &str, action: &str, folder_path: &str) {
        self.deliver(self.command(target, action, folder_path));
    }

    /// Asks a participant that takes `reveal-claude` to bring a Claude session up.
    pub fn send_reveal(&self, target: &str, folder_path: &str, session_id: &str, terminal_pid: Option<u32>) {
        self.deliver(RemoteCommand {
            session_id: Some(session_id.to_string()),
            terminal_pid,
            ..self.command(target, "reveal-claude", folder_path)
        });
    }

    fn command(&self, target: &str, action: &str, folder_path: &str) -> RemoteCommand {
        RemoteCommand {
            target: target.to_string(),
            action: action.to_string(),
            folder_path: folder_path.to_string(),
            issued_by: self.id.clone(),
            issued_at: now_ms(),
            session_id: None,
            terminal_pid: None,
        }
    }

    fn deliver(&self, command: RemoteCommand) {
        let target = command.target.as_str();
        let dir = self.peer_dirs.lock().ok().and_then(|known| known.get(target).cloned()).unwrap_or_else(|| self.dir.clone());
        // `<rand>` is unique to this send, even for sends in the same millisecond.
        let rand = format!("{}{}", base36(std::process::id() as u64), unique());
        let file = dir.join("commands").join(format!("{}__{}-{}.json", target, now_ms(), rand));

        if let Ok(json) = serde_json::to_string(&command) {
            let _ = write_atomic(&file, &json);
        }
    }

    /// Commands addressed to us: read, delete, and return the fresh ones.
    pub fn drain_commands(&self) -> Vec<RemoteCommand> {
        let prefix = format!("{}__", self.id);
        let now = now_ms();
        let mut commands = Vec::new();

        for file in list_json(&self.dir.join("commands")) {
            let mine = file.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(&prefix));

            if !mine {
                continue;
            }

            let command = read_json::<RemoteCommand>(&file);
            let _ = fs::remove_file(&file);

            if let Some(command) = command {
                if now.saturating_sub(command.issued_at) <= COMMAND_TTL_MS {
                    commands.push(command);
                }
            }
        }

        commands
    }

    pub fn record_pids(&self, entries: &[PidEntry]) {
        if entries.is_empty() {
            let _ = fs::remove_file(self.own_pids());
            return;
        }

        let record = PidRecord { window_id: self.id.clone(), entries: entries.to_vec() };

        if let Ok(json) = serde_json::to_string(&record) {
            let _ = write_atomic(&self.own_pids(), &json);
        }
    }

    /// Pids left behind by participants that are clearly dead; their files are deleted. A
    /// participant is dead once its record is gone or older than the sweep's limit and the
    /// process in its id is gone too: a window that slept, or whose extension host was blocked
    /// for a while, keeps its servers. A live participant's entries are never returned.
    ///
    /// Pids can be reused. A file written before the last boot names nothing of ours: it is
    /// deleted, nothing returned. A pid whose process started after the file was written is
    /// someone else's now and is left out. (A group whose leader is gone keeps its id from
    /// being reused while any member lives.)
    pub fn take_orphans(&self) -> Vec<PidEntry> {
        let now = now_ms();
        let records = self.all_records();
        let booted = crate::process::booted_at();
        let mut orphans = Vec::new();

        for file in list_json(&self.dir.join("pids")) {
            let record = read_json::<PidRecord>(&file);

            if let Some(record) = &record {
                if record.window_id == self.id || participant_alive(&record.window_id, &records, now) {
                    continue;
                }
            }

            let written = fs::metadata(&file).and_then(|meta| meta.modified()).ok();
            let written = written.and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map(|at| at.as_millis() as u64);

            if let (Some(record), Some(written)) = (record, written) {
                if booted.is_none_or(|booted| written >= booted) {
                    let ours = |entry: &PidEntry| crate::process::started_at(entry.pid).is_none_or(|started| started <= written);
                    orphans.extend(record.entries.into_iter().filter(ours));
                }
            }

            let _ = fs::remove_file(&file);
        }

        orphans
    }

    /// Every participant record in our folder and the legacy ones, however old.
    fn all_records(&self) -> Vec<WindowRecord> {
        std::iter::once(&self.dir)
            .chain(self.legacy.iter())
            .flat_map(|dir| list_json(&dir.join("windows")))
            .filter_map(|file| read_json::<WindowRecord>(&file))
            .collect()
    }

    /// A participant's output file, from the relative path in its record. Only plain names under
    /// `output/` resolve: a record can't point the app at any other file.
    pub fn output_file(&self, relative: &str) -> Option<PathBuf> {
        let relative = Path::new(relative);
        let mut parts = relative.components();
        let plain = relative.components().all(|part| matches!(part, std::path::Component::Normal(_)));

        if !plain || parts.next().and_then(|part| part.as_os_str().to_str()) != Some("output") || relative.extension().and_then(|e| e.to_str()) != Some("log") {
            return None;
        }

        Some(self.dir.join(relative))
    }

    /// Deletes dead records, expired commands and the output folders of participants that have
    /// no record any more.
    pub fn sweep(&self) {
        let now = now_ms();

        for file in list_json(&self.dir.join("windows")) {
            let dead = read_json::<WindowRecord>(&file).is_none_or(|record| now.saturating_sub(record.updated_at) > DEAD_MS);

            if dead {
                let _ = fs::remove_file(file);
            }
        }

        for file in list_json(&self.dir.join("commands")) {
            let expired = read_json::<RemoteCommand>(&file).is_none_or(|command| now.saturating_sub(command.issued_at) > COMMAND_TTL_MS);

            if expired {
                let _ = fs::remove_file(file);
            }
        }

        let Ok(entries) = fs::read_dir(self.dir.join("output")) else {
            return;
        };

        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().to_string();
            let alive = self.dir.join("windows").join(format!("{id}.json")).exists();

            if !alive && entry.path().is_dir() {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    pub fn read_favourites(&self) -> Vec<Favourite> {
        read_json(&self.dir.join("favorites.json")).unwrap_or_default()
    }

    /// Adds or removes a favourite; returns whether it is a favourite now.
    pub fn set_favourite(&self, favourite: Favourite, on: bool) -> bool {
        let mut list = self.read_favourites();
        let exists = list.iter().any(|item| same_path(&item.path, &favourite.path));

        if exists == on {
            return on;
        }

        if on {
            list.push(favourite);
            list.sort_by_key(|item| item.name.to_lowercase());
        } else {
            list.retain(|item| !same_path(&item.path, &favourite.path));
        }

        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let _ = write_atomic(&self.dir.join("favorites.json"), &json);
        }

        on
    }

    /// On quit: our record and pid list go; peers stop seeing us at once.
    pub fn dispose(&self) {
        let _ = fs::remove_file(self.own_file());
        let _ = fs::remove_file(self.own_pids());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, updated_at: u64, projects: Vec<ProjectState>, roots: Option<Vec<String>>) -> WindowRecord {
        WindowRecord { window_id: id.into(), title: id.into(), updated_at, projects, roots, features: None, terminals: None }
    }

    fn project(path: &str, running: bool) -> ProjectState {
        ProjectState {
            folder_path: path.into(),
            name: path.into(),
            running,
            port: None,
            url: None,
            started_at: None,
            issue: None,
            output: None,
        }
    }

    fn write(dir: &Path, sub: &str, name: &str, value: &impl Serialize) {
        fs::write(dir.join(sub).join(name), serde_json::to_string(value).unwrap()).unwrap();
    }

    #[test]
    fn a_record_without_roots_only_owns_what_it_runs() {
        let old = record("w", 0, vec![project("/a", false), project("/b", true)], None);
        let new = record("w", 0, vec![project("/a", false), project("/b", true)], Some(vec!["/a".into()]));

        assert_eq!(old.owned().map(|p| p.folder_path.as_str()).collect::<Vec<_>>(), ["/b"]);
        assert_eq!(new.owned().map(|p| p.folder_path.as_str()).collect::<Vec<_>>(), ["/a", "/b"]);
    }

    /// On Windows the extension's `c:\…` and the app's `C:\…` are one project (rule 3), or a
    /// server would start twice; a neighbour that only shares the start stays another.
    #[test]
    fn windows_paths_match_whatever_their_case_and_separators() {
        let key = |path| fold_path(path, true).into_owned();

        assert_eq!(key(r"c:\Users\Me\paddock"), key(r"C:\users\me\paddock\"));
        assert_eq!(key("C:/Users/me/paddock"), key(r"c:\users\me\paddock"));
        assert_eq!(key(r"C:\"), r"c:\");
        assert_ne!(key(r"C:\Users\me\paddock"), key(r"C:\Users\me\paddock-docs"));
        assert_eq!(fold_path("/Users/Me", false), "/Users/Me");
    }

    #[test]
    fn orphan_cleanup_never_returns_a_live_participants_pids() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = Registry::new(tmp.path().to_path_buf(), "Pitwall");

        write(tmp.path(), "windows", "alive.json", &record("alive", now_ms(), vec![], Some(vec![])));
        write(tmp.path(), "pids", "alive.json", &PidRecord { window_id: "alive".into(), entries: vec![PidEntry { path: "/a".into(), pid: 111 }] });
        write(tmp.path(), "pids", "dead.json", &PidRecord { window_id: "dead".into(), entries: vec![PidEntry { path: "/b".into(), pid: 222 }] });
        registry.record_pids(&[PidEntry { path: "/c".into(), pid: 333 }]);

        let orphans = registry.take_orphans();

        assert_eq!(orphans, vec![PidEntry { path: "/b".into(), pid: 222 }]);
        assert!(tmp.path().join("pids/alive.json").exists());
        assert!(!tmp.path().join("pids/dead.json").exists());
        assert_eq!(registry.take_orphans(), vec![]);
    }

    #[test]
    fn commands_reach_only_their_target_and_only_once() {
        let tmp = tempfile::tempdir().unwrap();
        let app = Registry::new(tmp.path().to_path_buf(), "Pitwall");
        let other = Registry::new(tmp.path().to_path_buf(), "VS Code");
        let other = Registry { id: format!("{}-other", other.id), ..other };

        app.send(&other.id, "stop", "/p");

        assert!(app.drain_commands().is_empty());

        let received = other.drain_commands();

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].action, "stop");
        assert_eq!(received[0].issued_by, app.id);
        assert!(other.drain_commands().is_empty());
    }

    #[test]
    fn commands_sent_in_the_same_millisecond_all_arrive() {
        let tmp = tempfile::tempdir().unwrap();
        let app = std::sync::Arc::new(Registry::new(tmp.path().to_path_buf(), "Pitwall"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));

        let senders: Vec<_> = (0..8)
            .map(|thread| {
                let (app, barrier) = (std::sync::Arc::clone(&app), std::sync::Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    for n in 0..25 {
                        app.send("target", "stop", &format!("/p{thread}-{n}"));
                    }
                })
            })
            .collect();
        for sender in senders {
            sender.join().unwrap();
        }

        let names: Vec<String> = fs::read_dir(tmp.path().join("commands")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|name| name.starts_with("target__") && name.ends_with(".json")), "{names:?}");

        let target = Registry::with_id(tmp.path().to_path_buf(), "VS Code", "target");
        let mut paths: Vec<String> = target.drain_commands().into_iter().map(|c| c.folder_path).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 200);
    }

    #[test]
    fn old_storage_is_adopted_once_and_never_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join(".pitwall");
        let current = tmp.path().join("aenzenith.pitwall-vscode");
        let deleted = tmp.path().join("aenzenith.pitwall");

        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&deleted).unwrap();
        fs::write(current.join("favorites.json"), "[\"new\"]").unwrap();
        fs::write(deleted.join("favorites.json"), "[\"old\"]").unwrap();
        fs::write(deleted.join("claude-seen.json"), "{\"since\":7,\"paths\":{}}").unwrap();

        adopt_old_storage(&target, &[current.clone(), deleted.clone()]);

        assert_eq!(fs::read_to_string(target.join("favorites.json")).unwrap(), "[\"new\"]");
        assert_eq!(fs::read_to_string(target.join("claude-seen.json")).unwrap(), "{\"since\":7,\"paths\":{}}");

        fs::write(target.join("favorites.json"), "[]").unwrap();
        adopt_old_storage(&target, &[current, deleted]);

        assert_eq!(fs::read_to_string(target.join("favorites.json")).unwrap(), "[]");
    }

    #[test]
    fn windows_on_an_old_extension_are_seen_and_get_their_commands_where_they_read() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join(".pitwall");
        let old = tmp.path().join("globalStorage").join("aenzenith.pitwall-vscode");

        fs::create_dir_all(old.join("windows")).unwrap();
        fs::create_dir_all(old.join("commands")).unwrap();

        let app = Registry::new(shared.clone(), "Pitwall").with_legacy(vec![old.clone(), tmp.path().join("missing")]);

        write(&shared, "windows", "new.json", &record("new", now_ms(), vec![project("/a", false)], Some(vec!["/a".into()])));
        write(&old, "windows", "old.json", &record("old", now_ms(), vec![project("/b", true)], Some(vec!["/b".into()])));

        let mut ids: Vec<String> = app.read_peers().into_iter().map(|r| r.window_id).collect();
        ids.sort();
        assert_eq!(ids, ["new", "old"]);

        app.send("old", "stop", "/b");
        app.send("new", "start", "/a");

        let in_dir = |dir: &Path, id: &str| {
            fs::read_dir(dir.join("commands")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(&format!("{id}__")))
        };
        assert!(in_dir(&old, "old"), "the old window reads its own folder");
        assert!(in_dir(&shared, "new"));
        assert!(!in_dir(&shared, "old"));
    }

    #[test]
    fn an_output_path_resolves_only_inside_the_output_folder() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::with_id(dir.path().to_path_buf(), "Pitwall", "app");

        assert_eq!(registry.output_file("output/1a-b/3f2a.log"), Some(dir.path().join("output/1a-b/3f2a.log")));

        for bad in ["output/../favorites.json", "output/1a-b/../../x.log", "/etc/passwd", "/tmp/output/x.log", "windows/1a-b.json", "output/x.json", "x.log"] {
            assert_eq!(registry.output_file(bad), None, "{bad}");
        }
    }

    #[test]
    fn sweeping_removes_only_dead_participants_output() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::with_id(dir.path().to_path_buf(), "Pitwall", "app");
        write(dir.path(), "windows", "live.json", &record("live", now_ms(), vec![], None));

        for id in ["live", "dead"] {
            fs::create_dir_all(dir.path().join("output").join(id)).unwrap();
            fs::write(dir.path().join("output").join(id).join("a.log"), "x\n").unwrap();
        }

        registry.sweep();

        assert!(dir.path().join("output/live/a.log").exists());
        assert!(!dir.path().join("output/dead").exists());
    }

    #[test]
    fn the_extensions_record_format_parses() {
        let json = r#"{"windowId":"1a-b","title":"paddock","updatedAt":1,"projects":[{"folderPath":"/p","name":"p","running":true,"port":5173,"issue":{"kind":"error","text":"x"},"output":"output/1a-b/3f.log","futureField":1}],"roots":["/p"]}"#;
        let parsed: WindowRecord = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.projects[0].port, Some(5173));
        assert_eq!(parsed.projects[0].output.as_deref(), Some("output/1a-b/3f.log"));
        assert!(parsed.has_root("/p"));
    }
}
