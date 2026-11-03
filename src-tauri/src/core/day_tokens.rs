//! The Today page's tokens: what Claude Code used in the day, per project, and the share of the
//! week's limit it took. `fuel.rs` hands over each look at the logs (`DayFuel`); the core splits
//! it by project and writes it into the day's file every few minutes and at a quit, so a day
//! gone by still shows it (`activity.rs`). Only counts are written: tokens and shares, never a
//! session's text. Codex's tokens are held and written beside Claude's (`codexSpend`), counts
//! only, with no share of a week: the day's share of the week's limit is the main Claude
//! account's.

use std::collections::BTreeMap;

use super::activity::{day_bounds, SpendRecord};
use super::*;
use crate::claude::encode_project_path;
use crate::spend::SessionSpend;

/// How often the day's tokens are written down while they change.
const WRITE_EVERY: u64 = 5 * 60 * 1000;

/// One look at the day's tokens, from `fuel.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct DayFuel {
    /// `YYYY-MM-DD`, local.
    pub date: String,
    pub tokens: u64,
    /// The share of the week's limit the day took, in percent; none before the limit was read
    /// that day, and when all of the day's use came before that reading.
    pub week: Option<f64>,
    /// Part of `week` was worked out from the tokens: the limit was first read after the day's
    /// first use.
    pub estimated: bool,
    /// The week's limit used, as last read.
    pub used: Option<f64>,
    /// The day's use per session id. A Claude session's `folder` is the folder under
    /// `~/.claude/projects` its logs are in; a Codex session's is the folder it ran in.
    pub sessions: HashMap<String, SessionSpend>,
}

/// The day's tokens in all projects, on the Today page.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaySpend {
    pub tokens: u64,
    pub week: Option<f64>,
    pub estimated: bool,
}

/// One project's part of them.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSpend {
    pub tokens: u64,
    pub week: Option<f64>,
}

/// The last look at the day's tokens and what was last written down of them.
#[derive(Default)]
pub(super) struct DayTokens {
    latest: Option<(u64, DayFuel)>,
    written: Option<Written>,
}

struct Written {
    date: String,
    at: u64,
    tokens: u64,
    week: Option<f64>,
}

impl DayTokens {
    fn wrote(&self, fuel: &DayFuel) -> bool {
        self.written.as_ref().is_some_and(|written| written.date == fuel.date && written.tokens == fuel.tokens && written.week == fuel.week)
    }
}

impl Core {
    fn day_tokens(&self, agent: Agent) -> MutexGuard<'_, DayTokens> {
        let held = match agent {
            Agent::Claude => &self.day_tokens,
            Agent::Codex => &self.codex_day_tokens,
        };
        held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Each look at the logs. What changed is written down once the last write is a few minutes
    /// back; when the day turned, the day before's last look first, into its own day.
    pub fn day_fuel(&self, fuel: DayFuel) {
        self.day_fuel_of(Agent::Claude, fuel);
    }

    /// `day_fuel` of `agent`'s tokens: Codex's from each look at its rollouts (`codex_spend.rs`).
    pub fn day_fuel_of(&self, agent: Agent, fuel: DayFuel) {
        let now = now_ms();
        let (turned, due) = {
            let mut held = self.day_tokens(agent);
            let turned = held.latest.take().filter(|(_, last)| last.date != fuel.date && !held.wrote(last));
            let due = fuel.tokens > 0
                && !held.wrote(&fuel)
                && held.written.as_ref().is_none_or(|written| written.date != fuel.date || now >= written.at + WRITE_EVERY);
            if due {
                held.written = Some(Written { date: fuel.date.clone(), at: now, tokens: fuel.tokens, week: fuel.week });
            }
            held.latest = Some((now, fuel.clone()));
            (turned, due.then_some(fuel))
        };

        if let Some((at, last)) = turned {
            self.write_tokens(agent, at, &last);
        }
        if let Some(fuel) = due {
            self.write_tokens(agent, now, &fuel);
        }
    }

    /// At a quit: the last look of each agent, if it isn't written down yet.
    pub(super) fn flush_day_tokens(&self) {
        for agent in Agent::ALL {
            self.flush_day_tokens_of(agent);
        }
    }

    /// Codex turned off: what was last looked at of its tokens is written down, then let go.
    pub fn codex_day_off(&self) {
        self.flush_day_tokens_of(Agent::Codex);
        *self.day_tokens(Agent::Codex) = DayTokens::default();
    }

    fn flush_day_tokens_of(&self, agent: Agent) {
        let last = {
            let mut held = self.day_tokens(agent);
            let last = held.latest.take().filter(|(_, fuel)| fuel.tokens > 0 && !held.wrote(fuel));
            if let Some((at, fuel)) = &last {
                held.written = Some(Written { date: fuel.date.clone(), at: *at, tokens: fuel.tokens, week: fuel.week });
            }
            last
        };
        if let Some((at, fuel)) = last {
            self.write_tokens(agent, at, &fuel);
        }
    }

    /// The day's tokens as they stood at `at`, split by project, into their own day's file.
    fn write_tokens(&self, agent: Agent, at: u64, fuel: &DayFuel) {
        let Some((start, end)) = day_bounds(&fuel.date) else {
            return;
        };
        // A look just before midnight may be handed over just after it.
        let at = at.clamp(start, end - 1);
        let owners: HashMap<String, String> = self.sessions_on(start, at).into_iter().map(|(id, day)| (id, day.path)).collect();
        let parts = split(agent, fuel, &owners, &self.listed_paths());

        let record = SpendRecord {
            tokens: fuel.tokens,
            week: fuel.week.map(round),
            estimated: fuel.estimated,
            used: fuel.used,
            projects: parts.into_iter().map(|(path, part)| (path, (part.tokens, part.week.map(round)))).collect(),
        };
        match agent {
            Agent::Claude => self.record_spend(at, &record),
            _ => self.record_spend_of(agent, at, &record),
        }
    }

    /// Today's tokens as the logs were last looked at, if that look is of `date`: in all, and per
    /// project path. `owners`: the project each session of the day's file was written down under;
    /// `listed`: the listed projects' paths.
    pub(super) fn tokens_today(&self, date: &str, owners: &HashMap<String, String>, listed: &[String]) -> (Option<DaySpend>, BTreeMap<String, ProjectSpend>) {
        self.held_today(Agent::Claude, date, owners, listed)
    }

    fn held_today(&self, agent: Agent, date: &str, owners: &HashMap<String, String>, listed: &[String]) -> (Option<DaySpend>, BTreeMap<String, ProjectSpend>) {
        let held = self.day_tokens(agent);
        let Some((_, fuel)) = held.latest.as_ref().filter(|(_, fuel)| fuel.date == date && (agent.is_claude() || fuel.tokens > 0)) else {
            return (None, BTreeMap::new());
        };
        (Some(DaySpend { tokens: fuel.tokens, week: fuel.week, estimated: fuel.estimated }), split(agent, fuel, owners, listed))
    }

    /// `tokens_today` of `agent`'s tokens: Claude's as above; Codex's as its rollouts were last
    /// looked at (`codex_spend.rs`), none while it wasn't looked at today; either's none while it
    /// is off.
    pub(super) fn tokens_today_of(&self, agent: Agent, date: &str, owners: &HashMap<String, String>, listed: &[String]) -> (Option<DaySpend>, BTreeMap<String, ProjectSpend>) {
        match agent {
            _ if !self.agent_on(agent) => (None, BTreeMap::new()),
            Agent::Claude => self.tokens_today(date, owners, listed),
            Agent::Codex => self.held_today(agent, date, owners, listed),
        }
    }
}

/// Each session's tokens to its project: the one the day's file has it under, else the listed
/// project whose folder its logs are in (Codex: the innermost listed project holding the folder it
/// ran in), else the projects not listed (an empty path). A project's share of the week is the
/// day's, in the part of the day's cost it had (of its tokens when no model used has a price).
fn split(agent: Agent, fuel: &DayFuel, owners: &HashMap<String, String>, listed: &[String]) -> BTreeMap<String, ProjectSpend> {
    let folders: Vec<(String, &String)> = listed.iter().map(|path| (encode_project_path(path), path)).collect();
    let project_of = |folder: &str| -> Option<String> {
        if agent.is_claude() {
            return folders.iter().find(|(encoded, _)| encoded == folder).map(|(_, path)| (*path).clone());
        }
        let folder = Path::new(folder);
        listed.iter().filter(|path| !path.is_empty() && folder.starts_with(path)).max_by_key(|path| path.len()).cloned()
    };
    let priced = fuel.sessions.values().any(|session| session.cost_usd > 0.0);
    let weight = |session: &SessionSpend| if priced { session.cost_usd } else { session.tokens.total as f64 };
    let whole: f64 = fuel.sessions.values().map(weight).sum();

    let mut parts: BTreeMap<String, (u64, f64)> = BTreeMap::new();
    for (id, session) in &fuel.sessions {
        let path = owners
            .get(id)
            .filter(|path| !path.is_empty())
            .cloned()
            .or_else(|| project_of(&session.folder))
            .unwrap_or_default();
        let part = parts.entry(path).or_default();
        part.0 += session.tokens.total;
        part.1 += weight(session);
    }

    parts
        .into_iter()
        .filter(|(_, (tokens, _))| *tokens > 0)
        .map(|(path, (tokens, part))| {
            let week = fuel.week.map(|week| if whole > 0.0 { week * part / whole } else { 0.0 });
            (path, ProjectSpend { tokens, week })
        })
        .collect()
}

/// Two decimals are plenty for a share of the week.
fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
