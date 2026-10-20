//! What was open when Pitwall quit, and putting it back at the next start: the dev servers it
//! ran, the commands it kept running, and its terminal tabs (a Claude tab comes back with its
//! session, `claude --resume`, unless that session isn't known to have ended: it may run
//! somewhere else by then; a shell tab as a new shell). Unlike a restart for an update
//! (`restore.rs`) nothing a terminal showed is kept: the record names projects, command ids,
//! session ids and the tab names the user gave, and is read once and deleted at the next start.
//!
//! Settings' `resume` decides what that start does with it: `ask` offers it (the snapshot's
//! `resume`) until the user answers, `always` puts it back at once, `never` writes none.

use serde::{Deserialize, Serialize};

use super::restore::write_private;
use super::reveal::{is_session_id, Liveness};
use super::terminal::Launch;
use super::*;

const FILE: &str = "last-session.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct OpenTab {
    /// The name the user gave it; a tab that had its own name gets a new one.
    name: Option<String>,
    /// The Claude session that ran in it.
    session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct OpenProject {
    path: String,
    server: bool,
    /// Custom commands that keep running, don't ask first and weren't started by their server,
    /// by id.
    commands: Vec<String>,
    tabs: Vec<OpenTab>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    v: u32,
    at: u64,
    projects: Vec<OpenProject>,
}

/// The last quit's record while it waits for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Kept {
    at: u64,
    projects: Vec<OpenProject>,
}

/// One project of the last quit, as the UI names it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResumeProject {
    pub path: String,
    pub name: String,
    pub server: bool,
    /// The commands' names.
    pub commands: Vec<String>,
    pub terminals: usize,
}

/// What was open when Pitwall last quit.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResumeOffer {
    pub quit_at: u64,
    pub projects: Vec<ResumeProject>,
}

/// The offer for the snapshot: the record with its commands by name.
pub(super) fn offer(inner: &Inner) -> Option<ResumeOffer> {
    let kept = inner.resume.as_ref()?;
    let projects = kept
        .projects
        .iter()
        .map(|project| {
            let known = inner.settings.project(&project.path).commands;
            ResumeProject {
                path: project.path.clone(),
                name: folder_name(&project.path),
                server: project.server,
                commands: project.commands.iter().filter_map(|id| known.iter().find(|command| &command.id == id)).map(|command| command.name.clone()).collect(),
                terminals: project.tabs.len(),
            }
        })
        .collect();

    Some(ResumeOffer { quit_at: kept.at, projects })
}

impl Core {
    fn resume_file(&self) -> PathBuf {
        self.cfg.settings_file.with_file_name(FILE)
    }

    /// At quit, before anything is stopped: writes down what is open. Nothing open: an offer
    /// that was never answered is kept for the next start instead.
    pub fn save_last_session(&self) {
        let file = self.resume_file();
        if self.lock().settings.resume == "never" {
            let _ = fs::remove_file(&file);
            return;
        }

        let standing = self.standing();
        let mut projects: Vec<OpenProject> = Vec::new();
        let mut entry = |path: &str| -> usize {
            match projects.iter().position(|project| project.path == path) {
                Some(at) => at,
                None => {
                    projects.push(OpenProject { path: path.to_string(), ..OpenProject::default() });
                    projects.len() - 1
                }
            }
        };

        let mut filled: Vec<(usize, bool, Option<String>, Option<OpenTab>)> = Vec::new();
        for path in &standing.servers {
            filled.push((entry(path), true, None, None));
        }
        for (path, id, _, again) in &standing.commands {
            if *again {
                filled.push((entry(path), false, Some(id.clone()), None));
            }
        }
        for tab in &standing.tabs {
            let name = tab.renamed.then(|| tab.name.clone());
            filled.push((entry(&tab.path), false, None, Some(OpenTab { name, session: tab.session.clone() })));
        }
        for (at, server, command, tab) in filled {
            let project = &mut projects[at];
            project.server |= server;
            project.commands.extend(command);
            project.tabs.extend(tab);
        }

        let record = match (projects.is_empty(), self.lock().resume.take()) {
            (true, Some(kept)) => Record { v: 1, at: kept.at, projects: kept.projects },
            (true, None) => {
                let _ = fs::remove_file(&file);
                return;
            }
            (false, _) => Record { v: 1, at: now_ms(), projects },
        };

        if let Ok(json) = serde_json::to_string(&record) {
            let _ = write_private(&file, &json);
        }
    }

    /// At start: reads the last quit's record, once. `restored`: a restart for an update already
    /// put back what was open, so the record is only dropped. Only listed projects whose folder
    /// is still there take part, only commands the project still has that keep running and don't
    /// ask first, and only a session id reaches Claude's command line.
    pub fn resume_start(self: &Arc<Self>, restored: bool) {
        let file = self.resume_file();
        let Ok(raw) = fs::read(&file) else {
            return;
        };
        // Gone before anything starts: a start that fails here is never tried again.
        let _ = fs::remove_file(&file);

        let (mode, settings) = {
            let inner = self.lock();
            (inner.settings.resume.clone(), inner.settings.clone())
        };
        if restored || mode == "never" {
            return;
        }
        let Ok(record) = serde_json::from_slice::<Record>(&raw) else {
            return;
        };
        if record.v != 1 {
            return;
        }

        let listed: Vec<String> = self.snapshot().projects.into_iter().map(|project| project.path).collect();
        let projects: Vec<OpenProject> = record
            .projects
            .into_iter()
            .filter(|project| listed.iter().any(|known| same_path(known, &project.path)) && Path::new(&project.path).is_dir())
            .map(|mut project| {
                let known = settings.project(&project.path).commands;
                project.commands.retain(|id| known.iter().any(|command| &command.id == id && command.keep_running && !command.confirm));
                for tab in &mut project.tabs {
                    tab.session = tab.session.take().filter(|id| is_session_id(id));
                    tab.name = tab.name.take().map(|name| name.trim().chars().take(40).collect::<String>()).filter(|name| !name.is_empty());
                }
                project
            })
            .filter(|project| project.server || !project.commands.is_empty() || !project.tabs.is_empty())
            .collect();
        if projects.is_empty() {
            return;
        }

        let kept = Kept { at: record.at, projects };
        if mode == "always" {
            self.put_back(kept);
        } else {
            self.lock().resume = Some(kept);
            self.notify();
        }
    }

    /// The user's answer: put it back. `always`: from now on without asking.
    pub fn resume_restore(self: &Arc<Self>, always: bool) {
        let (kept, noted) = {
            let mut inner = self.lock();
            let kept = inner.resume.take();
            let noted = always && inner.settings.resume != "always";
            if noted {
                inner.settings.resume = "always".into();
            }
            (kept, noted)
        };

        if noted {
            self.save_settings();
        }
        if let Some(kept) = kept {
            self.put_back(kept);
        }
        self.notify();
    }

    /// The user's answer: start empty.
    pub fn resume_dismiss(&self) {
        self.lock().resume = None;
        self.notify();
    }

    /// What a tab comes back as: its Claude session going on, or a shell. None for a session
    /// that isn't known to have ended: a running session is never opened a second time
    /// (`reveal.rs`), so its tab stays away.
    fn launch_of(&self, tab: &OpenTab) -> Option<Launch> {
        let Some(session) = &tab.session else {
            return Some(Launch::Shell);
        };
        matches!(self.liveness(session), Liveness::Stopped | Liveness::Ended).then(|| Launch::Resume { session: session.clone() })
    }

    fn put_back(self: &Arc<Self>, kept: Kept) {
        for project in &kept.projects {
            for tab in &project.tabs {
                let Some(launch) = self.launch_of(tab) else {
                    continue;
                };
                let Ok(view) = self.open_terminal_as(&project.path, &launch, tab.name.clone(), tab.name.is_some(), None, false) else {
                    continue;
                };
                if let Some(session) = &tab.session {
                    self.board_resumed(session, view.id);
                }
            }
        }

        // Off the caller's thread: a start resolves the project and waits for a free port. A
        // server a window runs by now, or would run, is that window's.
        let core = Arc::clone(self);
        thread::spawn(move || {
            for project in &kept.projects {
                if project.server {
                    core.start_quietly(&project.path, Origin::Restore);
                }
                for id in &project.commands {
                    core.run_command(&project.path, id);
                }
            }
        });
    }
}

#[cfg(all(test, unix))]
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

    /// The record is a file anyone running as the user can write, and it opens shells: nothing
    /// comes back before the user says so, a folder that isn't a listed project gets no shell
    /// from it, what isn't a session id never reaches Claude's command line (the tab comes back
    /// as a plain shell), and the record is used once.
    #[test]
    fn nothing_comes_back_unasked_and_only_listed_projects_do() {
        let tmp = tempfile::tempdir().unwrap();
        let listed = tmp.path().join("paddock");
        let unlisted = tmp.path().join("elsewhere");
        fs::create_dir_all(&listed).unwrap();
        fs::create_dir_all(&unlisted).unwrap();
        let (listed, unlisted) = (listed.to_string_lossy().into_owned(), unlisted.to_string_lossy().into_owned());

        let core = core_in(tmp.path());
        core.add_project(&listed).unwrap();

        let tab = |session: Option<&str>| OpenTab { name: None, session: session.map(str::to_string) };
        let record = Record {
            v: 1,
            at: now_ms(),
            projects: vec![
                OpenProject { path: unlisted.clone(), server: false, commands: Vec::new(), tabs: vec![tab(None)] },
                OpenProject { path: listed.clone(), server: false, commands: vec!["gone".into()], tabs: vec![tab(Some("x; touch pwned"))] },
            ],
        };
        write_private(&core.resume_file(), &serde_json::to_string(&record).unwrap()).unwrap();

        core.resume_start(false);
        assert!(!core.resume_file().exists(), "read once");
        assert!(core.lock().terminal_list.is_empty(), "nothing opens before the answer");
        let offered = core.snapshot().resume.expect("an offer");
        assert_eq!(offered.projects.iter().map(|project| (project.path.as_str(), project.terminals, project.commands.len())).collect::<Vec<_>>(), vec![(listed.as_str(), 1, 0)]);

        core.resume_restore(false);
        let tabs = core.lock().terminal_list.clone();
        assert_eq!(tabs.iter().map(|tab| (tab.path.as_str(), tab.kind.as_str())).collect::<Vec<_>>(), vec![(listed.as_str(), "shell")]);
        assert_eq!(core.snapshot().resume, None);

        core.resume_start(false);
        assert_eq!(core.lock().terminal_list.len(), 1, "no second time");

        core.dispose();
    }

    /// Putting back what was open is no yes to a command marked "ask first": a worker the user
    /// said yes to before the quit, which a record written by an older version or by hand names,
    /// is neither offered nor run; one that doesn't ask first comes back.
    #[test]
    fn a_command_marked_ask_first_never_comes_back_by_itself() {
        use crate::settings::{CustomCommand, ProjectSettings};

        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy().into_owned();
        let (worked, watched) = (tmp.path().join("worked"), tmp.path().join("watched"));

        let core = core_in(tmp.path());
        core.add_project(&project).unwrap();
        let command = |id: &str, line: String, confirm: bool| CustomCommand { id: id.into(), name: id.into(), command: line, keep_running: true, confirm, with_server: false };
        let commands = vec![command("worker", format!("touch '{}'; sleep 30", worked.display()), false), command("watch", format!("touch '{}'; sleep 30", watched.display()), true)];
        core.set_project_settings(&project, ProjectSettings { commands, ..Default::default() });

        let open = OpenProject { path: project.clone(), server: false, commands: vec!["worker".into(), "watch".into()], tabs: Vec::new() };
        let record = Record { v: 1, at: now_ms(), projects: vec![open] };
        write_private(&core.resume_file(), &serde_json::to_string(&record).unwrap()).unwrap();

        core.resume_start(false);
        let offered = core.snapshot().resume.expect("an offer");
        assert_eq!(offered.projects.iter().map(|project| project.commands.clone()).collect::<Vec<_>>(), vec![vec!["worker".to_string()]]);
        core.resume_restore(false);

        let until = Instant::now() + Duration::from_secs(15);
        while !worked.exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(worked.exists(), "the worker came back");
        thread::sleep(Duration::from_millis(500));
        assert!(!watched.exists(), "an ask-first worker came back unasked");

        core.dispose();
    }

    /// A process that passes for Claude Code's (`claude` in its command line), gone with the
    /// test however that ends.
    struct Sleeper(std::process::Child);

    impl Drop for Sleeper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Two processes on one session write one log, and nothing shows it. A tab's session comes
    /// back only once Claude Code's own list says no process runs it: while one does (the user
    /// went on with it in another terminal since the quit), the tab stays away.
    #[test]
    fn a_session_that_runs_is_not_opened_a_second_time() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join("claude");
        let core = core_in(tmp.path());
        let id = "9bb85e86-f618-4861-9858-03ec8fc36c28";
        let tab = OpenTab { name: None, session: Some(id.into()) };

        // Its log, written a moment ago, and Claude Code's file of the process that runs it.
        let log = claude.join("projects").join("-paddock").join(format!("{id}.jsonl"));
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(&log, "{}\n").unwrap();
        let sleeper = Sleeper(std::process::Command::new("sh").args(["-c", "read line", "claude"]).stdin(std::process::Stdio::piped()).spawn().unwrap());
        let pid = sleeper.0.id();
        assert!(crate::process::seen_with(pid, "claude"), "the stand-in shows as Claude Code's process");
        fs::create_dir_all(claude.join("sessions")).unwrap();
        fs::write(claude.join("sessions").join(format!("{pid}.json")), serde_json::json!({ "pid": pid, "sessionId": id }).to_string()).unwrap();

        assert_eq!(core.launch_of(&tab), None);

        // That process gone, the same tab comes back with its session; a shell tab always does.
        drop(sleeper);
        assert_eq!(core.launch_of(&tab), Some(Launch::Resume { session: id.into() }));
        assert_eq!(core.launch_of(&OpenTab::default()), Some(Launch::Shell));
    }
}
