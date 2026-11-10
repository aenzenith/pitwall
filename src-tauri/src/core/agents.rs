//! Which coding agents are on (`agent.rs`): as Settings › Agents says, else Claude, and another
//! agent once its home folder is found. Looking for that folder is the only thing done for an
//! agent that is off; nothing of it is read, watched or counted (`agent_on`).

use super::*;
use crate::agent::AgentInfo;

/// The agents whose home folder is there now: Claude always (its own checks say more), any other
/// by one look at its home folder, nothing inside it.
pub(super) fn look(cfg: &CoreConfig) -> HashSet<Agent> {
    Agent::ALL
        .into_iter()
        .filter(|agent| match agent {
            Agent::Claude => true,
            Agent::Codex => cfg.codex_home.is_dir(),
        })
        .collect()
}

/// Whether `agent` is followed, as the settings and the last look say.
pub(super) fn agent_on(inner: &Inner, agent: Agent) -> bool {
    inner.settings.agents.is_on(agent, inner.found.contains(&agent))
}

/// Every agent as the snapshot carries it.
pub(super) fn agent_infos(inner: &Inner) -> Vec<AgentInfo> {
    Agent::ALL
        .into_iter()
        .map(|agent| {
            let (hook, hook_outdated) = match agent {
                Agent::Claude => (if inner.claude_hook { HookState::Working } else { HookState::Missing }, inner.claude_hook_outdated),
                Agent::Codex => (inner.codex_hook, inner.codex_hook_outdated),
            };
            let found = inner.found.contains(&agent);
            AgentInfo {
                id: agent,
                name: agent.name(),
                on: agent_on(inner, agent),
                found,
                undecided: !agent.is_claude() && found && inner.settings.agents.of(agent).on.is_none() && !inner.settings.agents.of(agent).told,
                caps: agent.caps(),
                hook,
                hook_outdated,
            }
        })
        .collect()
}

impl Core {
    /// Whether `agent` is followed now (`agent_on`).
    pub fn agent_on(&self, agent: Agent) -> bool {
        agent_on(&self.lock(), agent)
    }

    /// Looks for the agents' home folders again (Settings › Agents opened, the user's asking);
    /// true when what is found changed, the agents then followed as they now are.
    pub fn look_for_agents(self: &Arc<Self>) -> bool {
        let found = look(&self.cfg);
        let changed = {
            let mut inner = self.lock();
            let changed = inner.found != found;
            inner.found = found;
            changed
        };
        if changed {
            self.agents_changed();
        }
        changed
    }

    /// An agent was turned on or off, or found: its watch starts or stops (`follow_agents`), the
    /// radio's endpoint follows (`radio_follow`) and the window hears of it.
    pub fn agents_changed(self: &Arc<Self>) {
        self.follow_agents();
        self.radio_follow();
        // Nothing on the board waits for an agent that is off.
        self.board_agents_off();
        self.notify();
    }

    /// Turns `agent` on or off at the user's word (Settings › Agents, the found notice, the first
    /// run): it stays so, whatever a look finds later, and the user counts as told. Claude too:
    /// off, nothing of it is read, as of any agent. What Pitwall put into it (its hook, the
    /// radio's entry) stays where it is.
    pub fn agent_set_on(self: &Arc<Self>, agent: Agent, on: bool) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let part = inner.settings.agents.of_mut(agent);
            part.on = Some(on);
            part.told = true;
        }
        self.save_settings();
        self.agents_changed();
        Ok(())
    }

    /// The user was told that `agent` was found and turned on (the notice closed, the first run's
    /// step seen): it isn't told again. Nothing else changes.
    pub fn agent_told(&self, agent: Agent) {
        {
            let mut inner = self.lock();
            let part = inner.settings.agents.of_mut(agent);
            if part.told {
                return;
            }
            part.told = true;
        }
        self.save_settings();
        self.notify();
    }

    /// Where `agent`'s fuel shows, at the user's word (Settings › Agents): in the menu bar
    /// (`bar`) and in the popover's foot (`popover`), each left as it is when none. Claude's are
    /// Settings' own (`fuel_in_bar`, `fuel_in_popover`), as before.
    pub fn agent_set_fuel(&self, agent: Agent, bar: Option<bool>, popover: Option<bool>) -> Result<(), String> {
        if agent.is_claude() {
            return Err(format!("{}'s fuel is in Settings' form", agent.name()));
        }
        {
            let mut inner = self.lock();
            let part = inner.settings.agents.of_mut(agent);
            // On is as never said: the copy of the agents' part goes while it is the default.
            let said = |now: Option<bool>, was: Option<bool>| now.map_or(was, |on| (!on).then_some(false));
            part.fuel_in_bar = said(bar, part.fuel_in_bar);
            part.fuel_in_popover = said(popover, part.fuel_in_popover);
        }
        self.save_settings();
        self.notify();
        Ok(())
    }

    /// Each agent's line under its name in Settings › Agents: its home folder as the user reads it
    /// and, for one that is on and found, its CLI's version as `--version` says it (asked through
    /// the user's shell, good for `VERSION_FOR`). It takes a moment, so off the main thread.
    pub fn agent_details(&self) -> Vec<AgentDetail> {
        let home = dirs::home_dir().unwrap_or_default();
        Agent::ALL
            .into_iter()
            .map(|agent| {
                let folder = match agent {
                    Agent::Claude => self.cfg.claude_dir.parent().map_or_else(|| self.cfg.claude_dir.clone(), Path::to_path_buf),
                    Agent::Codex => self.cfg.codex_home.clone(),
                };
                let asked = {
                    let inner = self.lock();
                    agent_on(&inner, agent) && inner.found.contains(&agent)
                };
                let version = if asked { self.agent_version(agent) } else { None };
                AgentDetail { id: agent, version, home: crate::accounts::shown(&home, &folder.to_string_lossy()) }
            })
            .collect()
    }

    /// `agent`'s CLI version, as last asked or asked now.
    pub(super) fn agent_version(&self, agent: Agent) -> Option<String> {
        if let Some((_, version)) = self.lock().agent_versions.get(&agent).filter(|(at, _)| at.elapsed() < VERSION_FOR) {
            return version.clone();
        }
        let version = super::radio::cli_version(agent);
        self.lock().agent_versions.insert(agent, (Instant::now(), version.clone()));
        version
    }
}

/// How long an agent's version, as its CLI said it, is shown before it is asked again.
const VERSION_FOR: Duration = Duration::from_secs(10 * 60);

/// One agent's line in Settings › Agents (`Core::agent_details`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetail {
    pub id: Agent,
    /// What its CLI's `--version` said, the version alone; none when it is off, not found or
    /// didn't say.
    pub version: Option<String>,
    /// Its home folder, `~` for the home folder.
    pub home: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_in(tmp: &Path) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude").join("projects"),
                codex_home: tmp.join("codex"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        )
    }

    /// Codex turned off by the user stays off: its home found again (at a start, when the window
    /// comes back) turns nothing on and tells nothing, and what the settings file keeps says so
    /// for the next start too. Found and never decided, it is on, and the user is told once.
    #[test]
    fn an_agent_turned_off_is_never_turned_on_by_a_look() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());
        let info = |core: &Core| agent_infos(&core.lock()).into_iter().find(|info| info.id == Agent::Codex).unwrap();
        assert!(!core.agent_on(Agent::Codex), "not on this Mac: off");

        fs::create_dir_all(tmp.path().join("codex")).unwrap();
        assert!(core.look_for_agents());
        assert!(core.agent_on(Agent::Codex) && info(&core).undecided, "found: on, and the user is to be told");
        core.agent_told(Agent::Codex);
        assert!(core.agent_on(Agent::Codex) && !info(&core).undecided, "told once: still on, told no more");

        core.agent_set_on(Agent::Codex, false).unwrap();
        fs::remove_dir(tmp.path().join("codex")).unwrap();
        core.look_for_agents();
        fs::create_dir_all(tmp.path().join("codex")).unwrap();
        core.look_for_agents();
        assert!(!core.agent_on(Agent::Codex) && !info(&core).undecided, "found again, it stays off");
        let again = core_in(tmp.path());
        assert!(!again.agent_on(Agent::Codex), "and so after a start");
    }

    /// Claude turned off is read for nothing, as any agent: its logs aren't scanned, its last
    /// words aren't read, its running sessions aren't looked for and its radio isn't turned on,
    /// from the start that finds it off too. On again, it is followed as before.
    #[test]
    fn claude_turned_off_is_read_for_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("app");
        fs::create_dir_all(&project).unwrap();
        let path = project.to_string_lossy().to_string();
        let logs = tmp.path().join("claude").join("projects").join(crate::claude::encode_project_path(&path));
        fs::create_dir_all(&logs).unwrap();
        let id = "6f1c2a4e-1b2c-4d3e-8f90-1234567890ab";
        let line = serde_json::json!({"type": "assistant", "cwd": path, "message": {"stop_reason": "end_turn", "content": [{"type": "text", "text": "Done here."}]}});
        fs::write(logs.join(format!("{id}.jsonl")), format!("{line}\n")).unwrap();

        let claude_scanned = |core: &Arc<Core>| {
            core.poll_files();
            core.follow_agents();
            core.refresh_claude();
            core.lock().scanned.iter().any(|session| session.agent == Agent::Claude && session.id == id)
        };
        let core = core_in(tmp.path());
        core.add_project(&path).unwrap();
        core.agent_set_on(Agent::Claude, false).unwrap();
        assert!(!claude_scanned(&core), "off: its logs aren't scanned");
        assert_eq!(core.last_message(id), None);
        assert_eq!(core.running_cached(), None);
        assert!(core.radio_set(true).is_err(), "its radio isn't turned on");

        let again = core_in(tmp.path());
        assert!(!again.agent_on(Agent::Claude) && again.sessions_cache().claude_off, "and so after a start");
        assert!(!claude_scanned(&again));

        again.agent_set_on(Agent::Claude, true).unwrap();
        assert!(claude_scanned(&again), "on: followed again");
        assert_eq!(again.last_message(id).as_deref(), Some("Done here."));
    }
}
