//! The chequered flag: what is still open before the user leaves. Listed projects with changes to
//! commit or commits to push, Claude sessions that wait on them, the board's cards to review and
//! those told to wait for their moment (the core gives them only while Pitwall runs), and what
//! still runs. There is one answer (`Core::flag`) for both who ask: the dialog shows it, and
//! a quit asked for from one of Pitwall's own menus or buttons is held by it (`quit_asked`), so
//! the two can never disagree.
//!
//! Everything in it is what the core holds already (each project's Git state, where Claude
//! stands, the board, the servers and commands): nothing new is read for it, nothing of it is
//! kept, nothing leaves the machine. A project muted for notifications counts like any other:
//! this tells of nothing by itself.

use std::sync::mpsc;

use super::board::Column;
use super::snapshot::build_snapshot;
use super::*;
use crate::claude::TurnKind;
use crate::git;

/// How many projects' Git state is read at a time for a fresh answer (`flag_refresh`).
const READERS: usize = 6;

/// A project with something of Git's left: `count` changes to commit, or commits to push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagGit {
    pub path: String,
    pub name: String,
    pub branch: String,
    pub count: u32,
}

/// A Claude session that waits on the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagWait {
    pub path: String,
    pub name: String,
    pub session: String,
    /// What it waits with: a turn that ended, a question, a permission prompt.
    pub kind: TurnKind,
    /// For a permission prompt: the tool's name, when the hook told it. Never its input.
    pub tool: Option<String>,
    /// Since when it waits, in ms.
    pub since: u64,
}

/// A project's cards in "To review".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagReview {
    pub path: String,
    pub name: String,
    pub cards: usize,
}

/// A project's cards that wait for their moment (`Card::schedule`): given by the core when it
/// comes, while Pitwall runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagScheduled {
    pub path: String,
    pub name: String,
    pub cards: usize,
    /// The earliest of their moments, in ms; none while one of them waits its turn in the
    /// refill's line (`Schedule::line`): it has no moment, and comes before any that has.
    pub at: Option<u64>,
}

/// A project whose dev server runs (here, or in an editor window), or is on its way up or down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagServer {
    pub path: String,
    pub name: String,
}

/// One of a project's commands that runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagCommand {
    pub path: String,
    /// Its project's name.
    pub project: String,
    pub id: String,
    pub name: String,
}

/// What still runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagRunning {
    pub servers: Vec<FlagServer>,
    pub commands: Vec<FlagCommand>,
    /// Some of it is being stopped right now.
    pub stopping: bool,
}

/// What is still open, part by part; a part with nothing in it is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    /// Listed projects with changes, in the list's order.
    pub commit: Vec<FlagGit>,
    /// Listed projects ahead of their upstream, in the list's order.
    pub push: Vec<FlagGit>,
    /// The longest wait first.
    pub claude: Vec<FlagWait>,
    /// In the list's order; a card's project that isn't listed after them.
    pub review: Vec<FlagReview>,
    /// Listed projects with cards that wait for their moment, in the list's order.
    pub scheduled: Vec<FlagScheduled>,
    pub running: FlagRunning,
}

impl Flag {
    /// Nothing is open: a quit goes on at once, and the dialog says so in one line.
    pub fn is_empty(&self) -> bool {
        self.commit.is_empty() && self.push.is_empty() && self.claude.is_empty() && self.review.is_empty() && self.scheduled.is_empty() && self.running.servers.is_empty() && self.running.commands.is_empty()
    }
}

/// What the dialog is told: what is open, and whether a quit waits on its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagState {
    pub open: Flag,
    /// A quit was held for the dialog and isn't answered yet.
    pub asked: bool,
}

/// What is open, from the projects as the snapshot has them, the last scan's sessions and the
/// board's cards. A project muted for notifications counts like any other.
pub(super) fn gather(projects: &[ProjectView], sessions: &[SessionState], cards: &[Card]) -> Flag {
    let name = |path: &str| projects.iter().find(|project| project.path == path).map_or_else(|| folder_name(path), |project| project.name.clone());
    let git = |count: fn(&GitInfo) -> u32| -> Vec<FlagGit> {
        let rows = projects.iter().filter_map(|project| {
            let info = project.git.as_ref()?;
            let count = count(info);
            (count > 0).then(|| FlagGit { path: project.path.clone(), name: project.name.clone(), branch: info.branch.clone(), count })
        });
        rows.collect()
    };

    // Sessions of listed projects only, as the list and the menu bar count them.
    let mut claude: Vec<FlagWait> = sessions
        .iter()
        .filter(|session| session.phase == SessionPhase::Waiting && !session.path.is_empty())
        .filter_map(|session| {
            let turn = session.turn?;
            Some(FlagWait {
                path: session.path.clone(),
                name: name(&session.path),
                session: session.id.clone(),
                kind: turn.kind,
                tool: session.tool.clone().filter(|_| turn.kind == TurnKind::Permission),
                since: session.since.unwrap_or(turn.at),
            })
        })
        .collect();
    // One session is one wait. With two listed projects one inside the other, a session of the
    // inner one that comes up under both is the inner project's alone.
    claude.sort_by(|a, b| a.session.cmp(&b.session).then_with(|| b.path.len().cmp(&a.path.len())));
    claude.dedup_by(|next, kept| next.session == kept.session);
    claude.sort_by(|a, b| a.since.cmp(&b.since).then_with(|| a.session.cmp(&b.session)));

    let mut review: Vec<FlagReview> = Vec::new();
    for card in cards.iter().filter(|card| card.column == Column::Review) {
        match review.iter_mut().find(|row| row.path == card.path) {
            Some(row) => row.cards += 1,
            None => review.push(FlagReview { path: card.path.clone(), name: name(&card.path), cards: 1 }),
        }
    }
    let place = |path: &str| projects.iter().position(|project| project.path == path).unwrap_or(usize::MAX);
    review.sort_by_key(|row| place(&row.path));

    // Up next and told to wait. Only a listed project's card is ever given at its moment
    // (`board_due`): no other has a give to lose.
    let mut scheduled: Vec<FlagScheduled> = Vec::new();
    for card in cards.iter().filter(|card| card.column == Column::Queued) {
        let (Some(schedule), Some(project)) = (card.schedule, projects.iter().find(|project| project.path == card.path)) else {
            continue;
        };
        let at = (!schedule.line).then_some(schedule.at);
        match scheduled.iter_mut().find(|row| row.path == card.path) {
            Some(row) => {
                row.cards += 1;
                row.at = row.at.zip(at).map(|(earliest, at)| earliest.min(at));
            }
            None => scheduled.push(FlagScheduled { path: card.path.clone(), name: project.name.clone(), cards: 1, at }),
        }
    }
    scheduled.sort_by_key(|row| place(&row.path));

    let servers = projects.iter().filter(|project| matches!(project.status, "running" | "busy"));
    let commands = projects.iter().flat_map(|project| {
        let running = project.commands.iter().filter(|command| matches!(command.status, "running" | "busy"));
        running.map(|command| FlagCommand { path: project.path.clone(), project: project.name.clone(), id: command.id.clone(), name: command.name.clone() })
    });
    let stopping = projects.iter().any(|project| project.phase == Some("stopping…") || project.commands.iter().any(|command| command.status == "busy"));

    Flag {
        commit: git(|info| info.changes),
        push: git(|info| info.ahead),
        claude,
        review,
        scheduled,
        running: FlagRunning { servers: servers.map(|project| FlagServer { path: project.path.clone(), name: project.name.clone() }).collect(), commands: commands.collect(), stopping },
    }
}

/// Whether a quit asked for from Pitwall's own menus and buttons is held for the dialog: only
/// while Settings says so, something is open, and no earlier quit still waits on its answer. A
/// second quit while the dialog is up (or while it can't come up) is the answer: it goes on.
fn holds(at_quit: bool, asked: bool, flag: &Flag) -> bool {
    at_quit && !asked && !flag.is_empty()
}

impl Core {
    /// What is still open, as things stand in the core now.
    pub fn flag(&self) -> Flag {
        let (projects, sessions) = {
            let inner = self.lock();
            (build_snapshot(&inner).projects, inner.scanned.clone())
        };
        // The board's lock is never held together with the core's.
        let cards = self.board().cards.clone();
        gather(&projects, &sessions, &cards)
    }

    /// The same, with whether a quit waits on the dialog.
    pub fn flag_state(&self) -> FlagState {
        FlagState { open: self.flag(), asked: self.quit_waits() }
    }

    /// Reads every listed project's Git state again, now, for an answer that is not up to a
    /// minute old (a file changed by hand moves nothing in `.git`, so only the slow poll sees
    /// it). It is the read the core does anyway (`watch.rs`), a few projects at a time; what
    /// isn't back within `limit` stays as it was held, so nothing ever waits long on it.
    pub fn flag_refresh(self: &Arc<Self>, limit: Duration) {
        let paths = self.listed_paths();
        if paths.is_empty() {
            return;
        }

        let count = paths.len();
        let queue = Arc::new(Mutex::new(paths));
        let (sender, receiver) = mpsc::channel();
        for _ in 0..count.min(READERS) {
            let (queue, sender) = (Arc::clone(&queue), sender.clone());
            thread::spawn(move || loop {
                let next = queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop();
                let Some(path) = next else {
                    return;
                };
                let state = git::status(&path);
                // Nobody waits any more: the rest is left to the next poll.
                if sender.send((path, state)).is_err() {
                    return;
                }
            });
        }
        drop(sender);

        let deadline = Instant::now() + limit;
        let mut read: Vec<(String, Option<GitInfo>)> = Vec::with_capacity(count);
        while read.len() < count {
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(state) => read.push(state),
                Err(_) => break,
            }
        }

        {
            let mut inner = self.lock();
            for (path, state) in read {
                match state {
                    Some(info) => inner.git.insert(path, info),
                    None => inner.git.remove(&path),
                };
            }
        }
        self.notify();
    }

    /// A quit asked for from one of Pitwall's own menus or buttons. True: it is held, and the
    /// dialog is to show what is open (it waits on the answer from now on: `quit_waits`).
    /// False: it goes on, as it did before there was a dialog.
    ///
    /// Only such a quit ever comes here. An update's restart or its install on the way out, the
    /// system logging out or shutting down, a development run replacing the app and the hand-over
    /// to an instance that already runs never ask.
    pub fn quit_asked(&self) -> bool {
        let flag = self.flag();
        let mut inner = self.lock();
        inner.flag_asked = holds(inner.settings.flag.at_quit, inner.flag_asked, &flag);
        inner.flag_asked
    }

    /// The held quit was given up (Cancel, Esc, the dialog or its window gone): the next one
    /// asks again.
    pub fn quit_dropped(&self) {
        self.lock().flag_asked = false;
    }

    /// A quit is held and waits on the dialog's answer.
    pub fn quit_waits(&self) -> bool {
        self.lock().flag_asked
    }

    /// Settings › General's switch, and the dialog's box: whether a quit shows what is open.
    pub fn set_flag_at_quit(&self, on: bool) {
        {
            let mut inner = self.lock();
            if inner.settings.flag.at_quit == on {
                return;
            }
            inner.settings.flag.at_quit = on;
        }

        self.save_settings();
        self.notify();
    }

    /// "Stop all" in the dialog: every server, those on their way up too, and every command
    /// that runs. Each stop shows in the snapshot until its process group is gone.
    pub fn flag_stop_all(self: &Arc<Self>) {
        let commands = self.flag().running.commands;
        self.stop_all();
        for command in commands {
            let core = Arc::clone(self);
            thread::spawn(move || core.stop_command(&command.path, &command.id));
        }
    }

    /// A row of "Waiting for push": the window shows that project in the list, where its
    /// terminal panel opens a new shell tab as its own button does (at the panel's size, which
    /// only the page knows). Only a listed project.
    pub fn flag_terminal(&self, path: &str) -> Result<(), String> {
        let Some(path) = self.listed_paths().into_iter().find(|listed| same_path(listed, path)) else {
            return Err(t!("core.error.notFolder", path = path));
        };
        self.emit(CoreEvent::RevealProject { path, job: None });
        Ok(())
    }

    /// A row of "To review": the window's Board page. Which board it shows is the page's own.
    pub fn flag_board(&self) {
        self.emit(CoreEvent::RevealPage("board"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::Turn;
    use crate::settings::CustomCommand;

    fn core(dir: &Path) -> (Arc<Core>, String) {
        let project = dir.join("paddock");
        fs::create_dir_all(&project).unwrap();
        let core = Core::new(
            CoreConfig {
                registry_dir: dir.join("registry"),
                claude_dir: dir.join("claude"),
                settings_file: dir.join("settings.json"),
                claude_settings: dir.join("claude-settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        let project = project.to_string_lossy().into_owned();
        core.add_project(&project).unwrap();
        (core, project)
    }

    fn waiting(path: &str, kind: TurnKind, tool: Option<&str>) -> SessionState {
        SessionState {
            id: "0f3c1a52-7d7e-4a44-9d0b-4c1b9c7f2a10".into(),
            path: path.into(),
            folder: Some(path.into()),
            title: None,
            phase: SessionPhase::Waiting,
            turn: Some(Turn { kind, at: 1_000 }),
            since: Some(1_000),
            hooked: true,
            ended: false,
            tool: tool.map(str::to_string),
            subagents: 0,
            interrupted: false,
        }
    }

    fn card(path: &str, column: Column) -> Card {
        Card {
            id: format!("card-{column:?}"),
            path: path.into(),
            title: "Pit stop".into(),
            note: String::new(),
            column,
            created_at: 1,
            moved_at: 1,
            session: None,
            terminal: None,
            given_at: None,
            worked: false,
            ran: false,
            rested: false,
            images: Vec::new(),
            start: None,
            schedule: None,
        }
    }

    /// Each kind of thing left open, put into a core that has nothing open, by its name.
    fn leave_open(core: &Arc<Core>, project: &str, kind: &str) {
        let git = |changes, ahead| GitInfo { branch: "main".into(), changes, ahead, behind: 0 };
        match kind {
            "commit" => drop(core.lock().git.insert(project.into(), git(2, 0))),
            "push" => drop(core.lock().git.insert(project.into(), git(0, 3))),
            "finished" => core.lock().scanned.push(waiting(project, TurnKind::Finished, None)),
            "question" => core.lock().scanned.push(waiting(project, TurnKind::Asking, None)),
            "permission" => core.lock().scanned.push(waiting(project, TurnKind::Permission, Some("Bash"))),
            "review" => core.board().cards.push(card(project, Column::Review)),
            // Told to wait an hour, as the board's own button tells it.
            "scheduled" => {
                let id = core.board_add(project, "Pit stop", "", &[]).unwrap().id;
                core.board_schedule(&id, now_ms() + 3_600_000, true, Default::default()).unwrap();
            }
            // A dev server on its way down still runs.
            "server" => drop(core.lock().busy.insert(project.into())),
            "command" => {
                let command = CustomCommand { id: "queue".into(), name: "queue:work".into(), command: "true".into(), ..Default::default() };
                core.set_project_settings(project, ProjectSettings { commands: vec![command], ..Default::default() });
                core.lock().stopping.insert(job_key(project, "queue"));
            }
            other => panic!("unknown kind: {other}"),
        }
    }

    const KINDS: [&str; 9] = ["commit", "push", "finished", "question", "permission", "review", "scheduled", "server", "command"];

    #[test]
    fn a_quit_is_held_by_each_kind_of_thing_left_open_and_by_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let (clean, project) = core(tmp.path());
        // What is no reason to ask: a branch behind its upstream, a session at work, cards in
        // every other column.
        clean.lock().git.insert(project.clone(), GitInfo { branch: "main".into(), changes: 0, ahead: 0, behind: 4 });
        clean.lock().scanned.push(SessionState { phase: SessionPhase::Working, turn: None, ..waiting(&project, TurnKind::Finished, None) });
        clean.board().cards.extend([Column::Queued, Column::Claude, Column::Done].map(|column| card(&project, column)));
        assert_eq!(clean.flag(), Flag::default());
        assert!(!clean.quit_asked(), "nothing open: the quit goes on");
        assert!(!clean.quit_waits());

        for kind in KINDS {
            let tmp = tempfile::tempdir().unwrap();
            let (core, project) = core(tmp.path());
            leave_open(&core, &project, kind);

            assert!(!core.flag().is_empty(), "{kind} is open");
            assert!(core.quit_asked(), "{kind}: the quit is held");
            assert!(core.quit_waits(), "{kind}: it waits on the dialog");
        }
    }

    #[test]
    fn a_held_quit_never_stands_in_the_way_of_the_next_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (core, project) = core(tmp.path());
        leave_open(&core, &project, "commit");

        // Asked again while the first waits (the dialog up, or its window unable to come up).
        assert!(core.quit_asked());
        assert!(!core.quit_asked(), "the second quit goes on");
        assert!(!core.quit_waits());

        // Given up: the next one asks again.
        assert!(core.quit_asked());
        core.quit_dropped();
        assert!(!core.quit_waits());
        assert!(core.quit_asked());
    }

    #[test]
    fn the_switch_off_holds_no_quit_whatever_is_open() {
        let tmp = tempfile::tempdir().unwrap();
        let (core, project) = core(tmp.path());
        for kind in KINDS {
            leave_open(&core, &project, kind);
        }
        core.set_flag_at_quit(false);

        assert!(!core.flag().is_empty());
        assert!(!core.quit_asked());
        assert!(!core.quit_waits());
        // And it is kept: the next start doesn't ask either.
        assert!(!Settings::load(&tmp.path().join("settings.json")).flag.at_quit);
    }
}
