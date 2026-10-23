//! What a restart for an update (`update.rs`) does to what is open, and what it puts back: the
//! dev servers this app ran, its terminal tabs with their last output, and the Claude sessions
//! in them (`claude --resume`). The record is written beside the settings just before the
//! restart, read once and deleted at the next start.
//!
//! A command that was running in a tab is not started again, and a Claude turn at work is lost
//! (its session comes back): the plan names both, so the user decides before the restart. A
//! tab's command line is never read; a running command is told from the terminal alone.

use serde::{Deserialize, Serialize};

use super::reveal::is_session_id;
use super::terminal::{Launch, TAB_NAME_MAX};
use super::*;

const FILE: &str = "restore.json";
/// A record older than this was left by a restart that never came back up: it is dropped.
const FRESH_MS: u64 = 10 * 60 * 1000;
/// Leaves the alternate screen, colours, a hidden cursor and mouse reporting behind: a program
/// cut off mid-screen (an editor, a pager) never got to.
const RESET: &str = "\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[0m\x1b[?25h";

/// One thing a restart cuts off for good.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cut {
    /// `claude`: a turn at work in a tab (its session is opened again). `shell`: a command
    /// running in a tab. `command`: a running custom command that is one-off or asks first.
    pub kind: &'static str,
    /// The project's name.
    pub project: String,
    /// The tab's name, or the command's.
    pub name: String,
}

/// What a restart would do to what is open, for the dialog that asks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartPlan {
    pub cut: Vec<Cut>,
    /// The projects whose dev server is started again, by name.
    pub servers: Vec<String>,
    /// The projects of the Claude sessions opened again, one entry per session.
    pub claude: Vec<String>,
    pub terminals: usize,
    pub terminal_projects: usize,
}

/// What came back after a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// The version the restart was for.
    pub version: String,
    pub servers: usize,
    pub terminals: usize,
    pub sessions: usize,
    /// The main window was up.
    pub window: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Tab {
    pub(super) path: String,
    pub(super) name: String,
    /// The user gave it that name: it doesn't take its session's.
    #[serde(default)]
    pub(super) renamed: bool,
    /// The Claude session running in it.
    pub(super) session: Option<String>,
    /// That session was at work, or (without one) a command was running.
    cut: bool,
    cols: u16,
    rows: u16,
    scrollback: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Rerun {
    path: String,
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    v: u32,
    at: u64,
    version: String,
    window: bool,
    servers: Vec<String>,
    /// Custom commands that keep running, don't ask first and weren't started by their server.
    commands: Vec<Rerun>,
    tabs: Vec<Tab>,
}

/// Everything that runs under this app right now.
pub(super) struct Standing {
    pub(super) servers: Vec<String>,
    /// Running custom commands: project, id, name, and whether it is started again.
    pub(super) commands: Vec<(String, String, String, bool)>,
    pub(super) tabs: Vec<Tab>,
}

/// The tab's last output with a line under it that says where the old shell ended.
fn replay(scrollback: &str, cut: bool, at: u64) -> String {
    let time = crate::clock::clock(at as i64);
    let line = if cut { t!("update.restored.lineCut", time = time) } else { t!("update.restored.line", time = time) };
    format!("{scrollback}{RESET}\r\n\x1b[2m── {line} ──\x1b[0m\r\n")
}

/// The record's file, readable by the user alone: it holds what the terminals showed. Written
/// beside it first and moved over it whole, so a quit in between leaves the one before (the
/// phone's channel and the radio's endpoint are kept this way too).
pub(super) fn write_private(file: &Path, json: &str) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = file.with_extension(format!("{}.tmp", std::process::id()));
    // One a run before left under this pid.
    let _ = fs::remove_file(&temp);

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&temp).and_then(|mut new| new.write_all(json.as_bytes())).and_then(|()| fs::rename(&temp, file));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

impl Core {
    fn restore_file(&self) -> PathBuf {
        self.cfg.settings_file.with_file_name(FILE)
    }

    pub(super) fn standing(&self) -> Standing {
        // Each tab's Claude session, by the shell among the session's parents.
        let mut sessions: HashMap<u64, (String, bool)> = HashMap::new();
        for (id, run) in self.read_running().unwrap_or_default() {
            if let Some((terminal, _)) = self.pitwall_terminal(&run.lineage) {
                sessions.insert(terminal, (id, run.status.as_deref() == Some("busy")));
            }
        }
        // Without a foreground group to ask, a shell with a child is running something.
        let parents: HashSet<u32> = if cfg!(windows) {
            process::process_table().map(|table| table.values().map(|process| process.parent).collect()).unwrap_or_default()
        } else {
            HashSet::new()
        };
        let states = self.terminal_tabs();

        let inner = self.lock();
        let tabs = inner
            .terminal_list
            .iter()
            .filter_map(|view| {
                let state = states.iter().find(|state| state.id == view.id)?;
                let session = sessions.get(&view.id);
                let running = state.foreground || state.pid.is_some_and(|pid| parents.contains(&pid));
                Some(Tab {
                    path: view.path.clone(),
                    name: view.name.clone(),
                    renamed: view.renamed,
                    session: session.map(|(id, _)| id.clone()),
                    cut: session.map_or(running, |(_, busy)| *busy),
                    cols: state.cols,
                    rows: state.rows,
                    // `claude --resume` draws the conversation itself.
                    scrollback: if session.is_some() { String::new() } else { state.scrollback.clone() },
                })
            })
            .collect();

        let mut servers: Vec<String> = inner.runs.keys().cloned().collect();
        servers.sort();

        let mut commands = Vec::new();
        for key in inner.jobs.keys() {
            let Some((path, id)) = key.split_once('\u{1f}') else {
                continue;
            };
            let Some(command) = inner.settings.project(path).commands.into_iter().find(|command| command.id == id) else {
                continue;
            };
            // One that starts with its server comes back with it. One marked "ask first" never
            // comes back by itself: it is cut off.
            let again = command.keep_running && !command.confirm && !(command.with_server && inner.runs.contains_key(path));
            if !command.keep_running || again || command.confirm {
                commands.push((path.to_string(), id.to_string(), command.name, again));
            }
        }
        commands.sort();

        Standing { servers, commands, tabs }
    }

    /// What a restart would cut off and what it would bring back.
    pub fn restart_plan(&self) -> RestartPlan {
        let standing = self.standing();
        let mut cut: Vec<Cut> = standing
            .tabs
            .iter()
            .filter(|tab| tab.cut)
            .map(|tab| Cut { kind: if tab.session.is_some() { "claude" } else { "shell" }, project: folder_name(&tab.path), name: tab.name.clone() })
            .collect();
        cut.extend(
            standing.commands.iter().filter(|(_, _, _, again)| !again).map(|(path, _, name, _)| Cut { kind: "command", project: folder_name(path), name: name.clone() }),
        );

        let projects: HashSet<&str> = standing.tabs.iter().map(|tab| tab.path.as_str()).collect();
        RestartPlan {
            cut,
            servers: standing.servers.iter().map(|path| folder_name(path)).collect(),
            claude: standing.tabs.iter().filter(|tab| tab.session.is_some()).map(|tab| folder_name(&tab.path)).collect(),
            terminals: standing.tabs.len(),
            terminal_projects: projects.len(),
        }
    }

    /// Writes down what runs now, for the start that follows a restart to `version`.
    pub fn save_restore(&self, version: &str, window: bool) -> Result<(), String> {
        let standing = self.standing();
        let record = Record {
            v: 1,
            at: now_ms(),
            version: version.to_string(),
            window,
            servers: standing.servers,
            commands: standing.commands.into_iter().filter(|(_, _, _, again)| *again).map(|(path, id, _, _)| Rerun { path, id }).collect(),
            tabs: standing.tabs,
        };
        let json = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        write_private(&self.restore_file(), &json).map_err(|error| error.to_string())
    }

    /// The restart didn't happen: nothing is to be put back.
    pub fn discard_restore(&self) {
        let _ = fs::remove_file(self.restore_file());
    }

    /// At start: puts back what the record names, once. Only listed projects whose folder is
    /// still there take part, only commands they still have that keep running and don't ask
    /// first, and only a session id reaches Claude's command line.
    pub fn restore(self: &Arc<Self>) -> Option<Restored> {
        let file = self.restore_file();
        let raw = fs::read(&file).ok()?;
        // Gone before anything starts: a start that fails here is never tried again.
        let _ = fs::remove_file(&file);

        let record: Record = serde_json::from_slice(&raw).ok()?;
        if record.v != 1 || now_ms().saturating_sub(record.at) > FRESH_MS {
            return None;
        }

        let listed: Vec<String> = self.snapshot().projects.into_iter().map(|project| project.path).collect();
        let known = |path: &str| listed.iter().any(|listed| same_path(listed, path)) && Path::new(path).is_dir();

        let mut restored = Restored { version: record.version, servers: 0, terminals: 0, sessions: 0, window: record.window };

        for tab in record.tabs.into_iter().filter(|tab| known(&tab.path)) {
            let session = tab.session.filter(|id| is_session_id(id));
            let launch = match &session {
                Some(session) => Launch::Resume { session: session.clone() },
                None => Launch::Shell,
            };
            let name: String = tab.name.trim().chars().take(TAB_NAME_MAX).collect();
            let size = (tab.cols > 0 && tab.rows > 0).then_some((tab.cols, tab.rows));
            let Ok(view) = self.open_terminal_as(&tab.path, &launch, (!name.is_empty()).then_some(name), tab.renamed, size, false) else {
                continue;
            };

            restored.terminals += 1;
            match session {
                Some(session) => {
                    restored.sessions += 1;
                    self.board_resumed(&session, view.id);
                }
                None => self.seed_terminal(view.id, &replay(&tab.scrollback, tab.cut, record.at)),
            }
        }

        let servers: Vec<String> = record.servers.into_iter().filter(|path| known(path)).collect();
        // Only commands the project still has, that keep running and don't ask first: the record
        // names no other.
        let settings = self.settings();
        let keeps_running = |command: &Rerun| settings.project(&command.path).commands.iter().any(|own| own.id == command.id && own.keep_running && !own.confirm);
        let commands: Vec<Rerun> = record.commands.into_iter().filter(|command| known(&command.path) && keeps_running(command)).collect();
        restored.servers = servers.len();

        // Off the caller's thread: a start resolves the project and waits for a free port.
        let core = Arc::clone(self);
        thread::spawn(move || {
            for path in &servers {
                core.start(path, false, Origin::Restore);
            }
            for command in &commands {
                core.run_command(&command.path, &command.id);
            }
        });

        Some(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_in(tmp: &Path) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude").join("projects"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        )
    }

    #[cfg(unix)]
    fn tab(path: &str, session: Option<&str>) -> Tab {
        Tab { path: path.into(), name: "zsh".into(), renamed: false, session: session.map(str::to_string), cut: false, cols: 80, rows: 24, scrollback: "old output\r\n".into() }
    }

    fn write(core: &Core, at: u64, servers: Vec<String>, tabs: Vec<Tab>) {
        let record = Record { v: 1, at, version: "9.9.9".into(), window: true, servers, commands: Vec::new(), tabs };
        write_private(&core.restore_file(), &serde_json::to_string(&record).unwrap()).unwrap();
    }

    /// The record starts processes: it is used once, and a stale one (a restart that never came
    /// back up) not at all, so no later start repeats it.
    #[test]
    fn a_record_is_used_once_and_a_stale_one_never() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());

        write(&core, now_ms() - FRESH_MS - 1, Vec::new(), Vec::new());
        assert_eq!(core.restore(), None);
        assert!(!core.restore_file().exists(), "a stale record is deleted too");

        write(&core, now_ms(), Vec::new(), Vec::new());
        assert_eq!(core.restore(), Some(Restored { version: "9.9.9".into(), servers: 0, terminals: 0, sessions: 0, window: true }));
        assert_eq!(core.restore(), None);
        assert!(!core.restore_file().exists());
    }

    /// The record is a file anyone running as the user can write: a folder that isn't a listed
    /// project gets no shell and no server from it, and what isn't a session id never reaches
    /// Claude's command line (the tab comes back as a plain shell).
    #[cfg(unix)]
    #[test]
    fn only_listed_projects_and_session_ids_come_back() {
        let tmp = tempfile::tempdir().unwrap();
        let listed = tmp.path().join("paddock");
        let unlisted = tmp.path().join("elsewhere");
        fs::create_dir_all(&listed).unwrap();
        fs::create_dir_all(&unlisted).unwrap();
        let (listed, unlisted) = (listed.to_string_lossy().into_owned(), unlisted.to_string_lossy().into_owned());

        let core = core_in(tmp.path());
        core.add_project(&listed).unwrap();

        write(
            &core,
            now_ms(),
            vec![unlisted.clone()],
            vec![tab(&unlisted, None), tab(&listed, Some("x; touch pwned")), tab(&tmp.path().join("gone").to_string_lossy(), None)],
        );
        let restored = core.restore().expect("a fresh record");

        assert_eq!((restored.servers, restored.terminals, restored.sessions), (0, 1, 0));
        let tabs = core.lock().terminal_list.clone();
        assert_eq!(tabs.iter().map(|tab| (tab.path.as_str(), tab.kind.as_str())).collect::<Vec<_>>(), vec![(listed.as_str(), "shell")]);
        let shown = core.terminal_buffer(tabs[0].id).data;
        assert!(shown.starts_with("old output\r\n") && shown.contains("──"), "{shown:?}");

        core.dispose();
    }

    #[cfg(unix)]
    fn wait_for(mut done: impl FnMut() -> bool) -> bool {
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// The record names the commands that keep running and don't ask first, and nothing else
    /// starts one: a one-off command marked "ask first" that a record written by hand names would
    /// run unasked, a migration with nobody there, and so would a worker marked "ask first" that
    /// the user said yes to before the restart.
    #[cfg(unix)]
    #[test]
    fn only_commands_that_keep_running_come_back() {
        use crate::settings::{CustomCommand, ProjectSettings};

        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy().into_owned();
        let (worked, migrated, watched) = (tmp.path().join("worked"), tmp.path().join("migrated"), tmp.path().join("watched"));

        let core = core_in(tmp.path());
        core.add_project(&project).unwrap();
        let command = |id: &str, line: String, keep_running: bool, confirm: bool| CustomCommand { id: id.into(), name: id.into(), command: line, keep_running, confirm, with_server: false };
        let commands = vec![
            command("migrate", format!("touch '{}'", migrated.display()), false, true),
            command("worker", format!("touch '{}'; sleep 30", worked.display()), true, false),
            command("watch", format!("touch '{}'; sleep 30", watched.display()), true, true),
        ];
        core.set_project_settings(&project, ProjectSettings { commands, ..Default::default() });

        let rerun = |id: &str| Rerun { path: project.clone(), id: id.into() };
        let commands = vec![rerun("migrate"), rerun("worker"), rerun("watch")];
        let record = Record { v: 1, at: now_ms(), version: "9.9.9".into(), window: false, servers: Vec::new(), commands, tabs: Vec::new() };
        write_private(&core.restore_file(), &serde_json::to_string(&record).unwrap()).unwrap();
        core.restore().expect("a fresh record");

        assert!(wait_for(|| worked.exists()), "the worker came back");
        thread::sleep(Duration::from_millis(500));
        assert!(!migrated.exists(), "an ask-first command was run");
        assert!(!watched.exists(), "an ask-first worker came back unasked");

        core.dispose();
    }

    /// A restart ends whatever runs in a tab, and nothing starts it again: the plan must name a
    /// running command, or the dialog would promise that everything comes back. A shell at its
    /// prompt is no such thing.
    #[cfg(unix)]
    #[test]
    fn a_command_running_in_a_tab_is_named_and_an_idle_shell_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy().into_owned();

        let core = core_in(tmp.path());
        core.add_project(&project).unwrap();
        let tab = core.open_terminal(&project, false, Some((80, 24))).unwrap();

        // The shell reads its start files first; at its prompt nothing is cut.
        assert!(wait_for(|| !core.terminal_buffer(tab.id).data.is_empty() && core.restart_plan().cut.is_empty()), "the shell never came to rest");
        let plan = core.restart_plan();
        assert_eq!((plan.terminals, plan.terminal_projects, plan.servers.len(), plan.claude.len()), (1, 1, 0, 0));

        core.write_terminal(tab.id, "sleep 60\r");
        let named = Cut { kind: "shell", project: "paddock".into(), name: tab.name.clone() };
        assert!(wait_for(|| core.restart_plan().cut == vec![named.clone()]), "{:?}", core.restart_plan());

        core.dispose();
    }

    /// A file written over is the one before or the new one at every moment, never gone or half
    /// there: the phone's channel is kept in such a file, and one that can't be read is a channel
    /// that is off, its token lost.
    #[cfg(unix)]
    #[test]
    fn a_private_file_is_whole_while_it_is_written_over() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicBool, Ordering};

        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config").join("phone.json");
        write_private(&file, &"a".repeat(8192)).unwrap();

        let done = Arc::new(AtomicBool::new(false));
        let (read, stop) = (file.clone(), Arc::clone(&done));
        let reader = thread::spawn(move || {
            let mut broken = 0;
            while !stop.load(Ordering::SeqCst) {
                broken += usize::from(!fs::read_to_string(&read).is_ok_and(|text| text.len() == 8192));
            }
            broken
        });
        for round in 0..300 {
            write_private(&file, &["a", "b"][round % 2].repeat(8192)).unwrap();
        }
        done.store(true, Ordering::SeqCst);

        assert_eq!(reader.join().unwrap(), 0, "read while gone or half written");
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(file.parent().unwrap()).unwrap().count(), 1, "nothing is left beside it");
    }

    /// The record holds what the terminals showed: nobody but the user reads it.
    #[cfg(unix)]
    #[test]
    fn the_record_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());
        core.save_restore("9.9.9", false).unwrap();

        let mode = fs::metadata(core.restore_file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
