//! The coding agents Pitwall follows, starts and counts: Claude Code and Codex today, more later.
//! Each is a word of a fixed list (`Agent`), with its name (a brand, never translated), its home
//! folder and what Pitwall can do with it (`Caps`). Anything read from a file that isn't one of
//! the words is Claude: every file written before there was a second agent names none.
//!
//! Which agents are on is the user's (`settings::AgentsSettings`, this Mac's own): Claude unless
//! turned off; another one found on this Mac is on until the user turns it off, and one the user
//! turned off is never turned on by Pitwall. An agent that is off is read for nothing
//! (`Core::agent_on`).
//!
//! Each agent's watch implements `AgentWatch`; the core merges the results of those that are on
//! (`core/claude_state.rs`: `merge_agents`), so whatever comes after (the snapshot, the board,
//! signals, the flag, the tower, the day's file) sees sessions, each with its `agent`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::claude::ScanResult;

/// A coding agent, by the word files and the window name it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl Agent {
    /// Every agent, in the order the window lists them.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    /// The word files, the window and the day's file name it with.
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Its name: a brand, the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// One of the list, or nothing.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.id() == text)
    }

    pub fn is_claude(&self) -> bool {
        *self == Self::Claude
    }

    /// What Pitwall can do with it. A capability an agent lacks shows nothing for it in the
    /// window, never a control that does nothing.
    pub fn caps(self) -> Caps {
        match self {
            Self::Claude => Caps { hook: true, permission: true, start: true, resume: true, models: true, modes: true, fuel: true, tokens: true, accounts: true, radio: true, vscode: true },
            // Its permission waits come from the hook alone; nothing brings a session up inside
            // VS Code's Codex (CODEX.md, gap 8).
            Self::Codex => Caps { hook: true, permission: true, start: true, resume: true, models: true, modes: true, fuel: true, tokens: true, accounts: true, radio: true, vscode: false },
        }
    }

    /// Its home folder under the user's home `home`: Claude Code's `~/.claude`; Codex's
    /// `$CODEX_HOME` when it is set (Codex itself requires it to exist then), else `~/.codex`.
    pub fn home_in(self, home: &Path) -> PathBuf {
        match self {
            Self::Claude => home.join(".claude"),
            Self::Codex => std::env::var_os("CODEX_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".codex")),
        }
    }

    /// Its home folder for the user Pitwall runs as (`dirs::home_dir`, as everywhere else).
    pub fn home(self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| self.home_in(&home))
    }

    /// The `kind` of its sessions' events in the day's file (`core/activity.rs`): `claude`,
    /// `codex`. An older Pitwall skips a kind it doesn't know.
    pub fn day_kind(self) -> &'static str {
        self.id()
    }

    /// The `kind` of the day's file events that hold its week's limit (`week` for Claude, as
    /// before), and of those that hold its tokens (`spend`). Codex's have kinds of their own, so
    /// an older Pitwall never reads them as Claude's.
    pub fn week_kind(self) -> &'static str {
        match self {
            Self::Claude => "week",
            Self::Codex => "codexWeek",
        }
    }

    pub fn spend_kind(self) -> &'static str {
        match self {
            Self::Claude => "spend",
            Self::Codex => "codexSpend",
        }
    }

    /// The `kind` of a terminal tab that runs it (`TerminalView::kind`).
    pub fn tab_kind(self) -> &'static str {
        self.id()
    }

    /// The agent a terminal tab of `kind` runs; none for a shell.
    pub fn of_tab(kind: &str) -> Option<Self> {
        Self::parse(kind)
    }
}

/// Whatever a file says that isn't one of the words is Claude: another type, an agent of a newer
/// Pitwall, nothing at all.
impl<'de> Deserialize<'de> for Agent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Value::deserialize(deserializer)?.as_str().and_then(Self::parse).unwrap_or_default())
    }
}

/// What Pitwall can do with an agent, as data the window draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    /// Pitwall's hook can be installed into it: turns told the moment they happen.
    pub hook: bool,
    /// A permission wait can be told (through the hook).
    pub permission: bool,
    /// Pitwall starts sessions of it (a terminal tab, a board card).
    pub start: bool,
    /// A session that ended opens again (`--resume`, `resume`).
    pub resume: bool,
    /// A model, and a mode, can be picked for a session it starts (`start.rs`).
    pub models: bool,
    pub modes: bool,
    /// Its limits are read (the Fuel page).
    pub fuel: bool,
    /// Its tokens and their API-equivalent cost are counted.
    pub tokens: bool,
    /// More than one account (a home folder each).
    pub accounts: bool,
    /// The team radio can be one of its MCP servers.
    pub radio: bool,
    /// A session of it in VS Code can be brought up there.
    pub vscode: bool,
}

/// Where Pitwall's hook stands in an agent: not installed; installed, but the agent hasn't run
/// it yet (Codex runs a hook only once the user trusted it in `/hooks`); or at work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HookState {
    #[default]
    Missing,
    Waiting,
    Working,
}

/// One agent as the snapshot carries it (`Snapshot::agents`): every place that names or counts an
/// agent builds from this list.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: Agent,
    pub name: &'static str,
    /// It is followed: read, counted, offered in start menus.
    pub on: bool,
    /// It is installed on this Mac, as far as its home folder tells (Claude: always).
    pub found: bool,
    /// The user never said on or off (`AgentSettings::on` unset): it is on because it was found,
    /// and the user is yet to be told so once (`AgentSettings::told`).
    pub undecided: bool,
    pub caps: Caps,
    /// Pitwall's hook in it.
    pub hook: HookState,
    /// The hook is installed but isn't this version's (or runs an app that moved): installing
    /// it again puts it right.
    pub hook_outdated: bool,
}

/// What each agent's watch does for the core, driven as `ClaudeWatch` is today: the projects to
/// follow, a full read of what the agent wrote for them, and where every session stands from what
/// was read so far. Events (hooks) and single files are each watch's own, outside the trait.
pub trait AgentWatch {
    fn agent(&self) -> Agent;
    /// The projects to follow; true when the list changed.
    fn set_listed(&mut self, paths: &[String]) -> bool;
    /// Reads what the agent wrote for the listed projects since it was last read.
    fn rescan(&mut self);
    /// A full read ran: nothing is known before.
    fn ready(&self) -> bool;
    /// Where every session stands, from what was read so far. Every `SessionState` it gives has
    /// `agent` set to `self.agent()`.
    fn evaluate(&mut self) -> ScanResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Files of today name no agent, and a newer Pitwall may name one this one doesn't know:
    /// either way the session, card or project is Claude's, and the file still reads.
    #[test]
    fn whatever_isnt_a_known_agent_is_claude() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default)]
            agent: Agent,
        }
        let read = |json: serde_json::Value| serde_json::from_value::<Holder>(json).unwrap().agent;
        assert_eq!(read(serde_json::json!({})), Agent::Claude);
        assert_eq!(read(serde_json::json!({ "agent": "codex" })), Agent::Codex);
        for odd in [serde_json::json!("opencode"), serde_json::json!("Codex"), serde_json::json!(" codex"), serde_json::json!(7), serde_json::json!(null), serde_json::json!(["codex"])] {
            assert_eq!(read(serde_json::json!({ "agent": odd })), Agent::Claude, "{odd}");
        }
        for agent in Agent::ALL {
            assert_eq!(serde_json::to_value(agent).unwrap(), serde_json::json!(agent.id()));
            assert_eq!(read(serde_json::json!({ "agent": agent.id() })), agent);
        }
    }
}
