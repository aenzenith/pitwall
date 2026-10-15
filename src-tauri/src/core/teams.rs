//! Teams: projects listed together and started and stopped together. They live in the app's
//! settings only (the VS Code extension keeps seeing single projects), in the order they are
//! drawn; a project is in one team at most, and a team's own order is the order its servers
//! start in.

use std::sync::atomic::{AtomicU64, Ordering};

use super::routing::{start_done, to_stop};
use super::snapshot::build_snapshot;
use super::*;
use crate::registry::base36;
use crate::settings::Team;

const NAME_LENGTH: usize = 40;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// The name as it is kept: trimmed and cut to a tab's length. None for an empty one.
fn clean_name(name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(NAME_LENGTH).collect();
    (!name.is_empty()).then_some(name)
}

/// `paths` without repeats, first place kept.
fn unique(paths: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for path in paths {
        if !kept.contains(&path) {
            kept.push(path);
        }
    }
    kept
}

/// Takes `paths` out of every team but `keep`: a project is in one team at most.
fn leave(teams: &mut [Team], paths: &[String], keep: Option<&str>) {
    for team in teams.iter_mut().filter(|team| Some(team.id.as_str()) != keep) {
        team.projects.retain(|path| !paths.contains(path));
    }
}

/// Each project's place by its team: (the team's place, its own in the team). The snapshot
/// lists the teams' projects first, in this order.
pub(super) fn places(teams: &[Team]) -> HashMap<&str, (usize, usize)> {
    let mut places = HashMap::new();
    for (at, team) in teams.iter().enumerate() {
        for (place, path) in team.projects.iter().enumerate() {
            places.entry(path.as_str()).or_insert((at, place));
        }
    }
    places
}

/// Calls off the team's start under way: it starts nothing more, and the projects whose turn
/// hadn't come are no longer on their way.
pub(super) fn call_off(inner: &mut Inner, id: &str) {
    if let Some(run) = inner.team_runs.remove(id) {
        inner.pending.retain(|_, pending| pending.queued != Some(run));
    }
}

/// Every team's start under way is called off.
pub(super) fn call_off_all(inner: &mut Inner) {
    inner.team_runs.clear();
    inner.pending.retain(|_, pending| pending.queued.is_none());
}

/// Whether start `run` of the team still stands: no stop of it, no newer start and no quit since.
pub(super) fn stands(inner: &Inner, id: &str, run: u64) -> bool {
    !inner.disposed && inner.team_runs.get(id) == Some(&run)
}

impl Core {
    fn change_teams(&self, change: impl FnOnce(&mut Vec<Team>)) {
        change(&mut self.lock().settings.teams);
        self.save_settings();
        self.notify();
    }

    fn team(&self, id: &str) -> Option<Team> {
        self.lock().settings.teams.iter().find(|team| team.id == id).cloned()
    }

    /// A new team, last in the list, with these projects (each leaves the team it was in).
    pub fn team_create(&self, name: &str, projects: Vec<String>) -> Result<Team, String> {
        let name = clean_name(name).ok_or_else(|| t!("core.error.teamName"))?;
        let id = format!("team-{}-{}", base36(now_ms()), base36(NEXT_ID.fetch_add(1, Ordering::Relaxed)));
        let team = Team { id, name, projects: unique(projects), collapsed: false, start_at_launch: false };
        let made = team.clone();

        self.change_teams(|teams| {
            leave(teams, &team.projects, None);
            teams.push(team);
        });
        Ok(made)
    }

    pub fn team_rename(&self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name).ok_or_else(|| t!("core.error.teamName"))?;
        self.change_teams(|teams| {
            if let Some(team) = teams.iter_mut().find(|team| team.id == id) {
                team.name = name;
            }
        });
        Ok(())
    }

    /// The team goes; its projects stay listed, in no team.
    pub fn team_delete(&self, id: &str) {
        self.change_teams(|teams| teams.retain(|team| team.id != id));
    }

    /// The team's projects, in order: a reorder, one added (it leaves the team it was in) or one
    /// dropped (it is in no team then).
    pub fn team_projects(&self, id: &str, projects: Vec<String>) {
        let projects = unique(projects);
        self.change_teams(|teams| {
            if !teams.iter().any(|team| team.id == id) {
                return;
            }
            leave(teams, &projects, Some(id));
            if let Some(team) = teams.iter_mut().find(|team| team.id == id) {
                team.projects = projects;
            }
        });
    }

    /// The teams' own order after a drag; one `ids` doesn't name keeps its place after them.
    pub fn team_order(&self, ids: Vec<String>) {
        self.change_teams(|teams| {
            teams.sort_by_key(|team| ids.iter().position(|id| id == &team.id).unwrap_or(usize::MAX));
        });
    }

    pub fn team_collapse(&self, id: &str, collapsed: bool) {
        self.change_teams(|teams| {
            if let Some(team) = teams.iter_mut().find(|team| team.id == id) {
                team.collapsed = collapsed;
            }
        });
    }

    /// Whether its servers start when Pitwall does.
    pub fn team_launch(&self, id: &str, on: bool) {
        self.change_teams(|teams| {
            if let Some(team) = teams.iter_mut().find(|team| team.id == id) {
                team.start_at_launch = on;
            }
        });
    }

    /// Starts the team's projects that don't run, one after the other in the team's order: the
    /// next goes once the start of the one before counts as done, as its spinner tells
    /// (`routing.rs`). A stop of the team, a stop of everything and quitting call off what
    /// hasn't begun.
    pub fn team_start(self: &Arc<Self>, id: &str) {
        self.start_team(id, Origin::User);
    }

    /// `by`: who starts it. The servers a click starts open their address, when Settings says so.
    fn start_team(self: &Arc<Self>, id: &str, by: Origin) {
        let open_url = by == Origin::User;
        let Some(team) = self.team(id) else {
            return;
        };

        let (idle, run) = {
            let mut inner = self.lock();
            // A start of it still under way ends here; what it hadn't begun is this one's.
            call_off(&mut inner, id);
            let listed = build_snapshot(&inner).projects;
            let idle: Vec<String> = team
                .projects
                .into_iter()
                .filter(|path| listed.iter().any(|project| &project.path == path && project.status != "running" && project.status != "busy"))
                .collect();

            inner.next_run += 1;
            let run = inner.next_run;
            inner.team_runs.insert(id.to_string(), run);
            // On their way from now; no time runs for one until its turn.
            for path in &idle {
                inner.pending.insert(path.clone(), Pending::start(Some(run)));
            }
            (idle, run)
        };
        self.notify();

        let core = Arc::clone(self);
        let id = id.to_string();
        thread::spawn(move || {
            for path in &idle {
                // Its turn, unless the start was called off meanwhile.
                {
                    let mut inner = core.lock();
                    if !stands(&inner, &id, run) {
                        return;
                    }
                    inner.pending.insert(path.clone(), Pending::start(None));
                }
                core.route(path, Action::Start, open_url, by);
                core.wait_started(path, &id, run);
            }

            let mut inner = core.lock();
            if stands(&inner, &id, run) {
                inner.team_runs.remove(&id);
            }
        });
    }

    /// Stops the team: its start under way goes no further, and every project of it that runs
    /// (here, settled or not, or in a window) or is on its way up here is stopped.
    pub fn team_stop(self: &Arc<Self>, id: &str) {
        let Some(team) = self.team(id) else {
            return;
        };
        let paths: Vec<String> = {
            let mut inner = self.lock();
            call_off(&mut inner, id);
            team.projects.into_iter().filter(|path| to_stop(&inner, path)).collect()
        };

        self.notify();
        for path in paths {
            self.act(&path, Action::Stop, Origin::User);
        }
    }

    /// At start: the teams that start with Pitwall. Nobody clicked: no browser opens for them.
    pub fn start_launch_teams(self: &Arc<Self>) {
        let ids: Vec<String> = self.lock().settings.teams.iter().filter(|team| team.start_at_launch).map(|team| team.id.clone()).collect();
        for id in ids {
            self.start_team(&id, Origin::Launch);
        }
    }

    /// Waits until the start just made for `path` counts as done (`routing::start_done`: it
    /// printed its address here or has run for a while, a window runs it, or it failed), or
    /// start `run` of its team was called off.
    fn wait_started(&self, path: &str, id: &str, run: u64) {
        loop {
            {
                let inner = self.lock();
                if start_done(&inner, path) || !stands(&inner, id, run) {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(250));
        }
    }
}
