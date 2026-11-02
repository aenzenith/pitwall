//! The terminal's history of Codex, as `history.rs`'s `claude_past` is Claude's: the project's
//! past Codex sessions, from the rollouts whose `session_meta.cwd` is in the project's folder
//! (`codex::rollout_meta`) and their names (`codex::session_names`). Built when the column opens,
//! nothing stored. CX3's file.
//!
//! Only plain rollouts are listed: one Codex compressed (`.jsonl.zst`, after a week without a
//! change) is left out until Codex turns it back into a plain one, since whether `codex resume`
//! opens it couldn't be checked.

use super::history::PastSession;
use super::*;
use crate::codex::RolloutMeta;

/// Whether a rollout's session is one the user had with Codex, as `codex resume` lists them: not
/// one `codex exec` ran, nor one Codex ran for itself (an MCP call, a subagent, its own work).
fn interactive(meta: &RolloutMeta) -> bool {
    let source = meta
        .source
        .as_deref()
        .unwrap_or("vscode")
        .to_ascii_lowercase();
    !["exec", "mcp", "subagent", "internal", "unknown"]
        .iter()
        .any(|other| source.starts_with(other))
}

/// Whether the folder a session ran in is the project's or one inside it.
fn within(cwd: &str, project: &str) -> bool {
    if same_path(cwd, project) {
        return true;
    }
    let project = project.trim_end_matches(['/', '\\']);
    cwd.strip_prefix(project)
        .is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// When a file last changed, in ms.
fn modified(file: &Path) -> Option<u64> {
    Some(
        fs::metadata(file)
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as u64,
    )
}

impl Core {
    /// The project's past Codex sessions, the last heard of first: those `codex resume` opens
    /// again from the project's folder, of every Codex account's home. None while Codex is off:
    /// then nothing of Codex is read.
    pub fn codex_past(&self, path: &str) -> Vec<PastSession> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut past: Vec<PastSession> = Vec::new();
        for home in self.codex_homes() {
            let mut names: Option<HashMap<String, String>> = None;
            for file in crate::codex::rollouts_since(&home, 0) {
                let Some(meta) = crate::codex::rollout_meta(&file) else {
                    continue;
                };
                if !super::reveal::is_session_id(&meta.id)
                    || !interactive(&meta)
                    || !meta.cwd.as_deref().is_some_and(|cwd| within(cwd, path))
                {
                    continue;
                }
                // The newest rollout of a session stands for it (they come newest first).
                if !seen.insert(meta.id.clone()) {
                    continue;
                }
                let title = names
                    .get_or_insert_with(|| crate::codex::session_names(&home))
                    .get(&meta.id)
                    .cloned();
                past.push(PastSession {
                    at: modified(&file).or(meta.started),
                    id: meta.id,
                    agent: Agent::Codex,
                    title,
                });
            }
        }
        past.sort_by_key(|session| std::cmp::Reverse(session.at));
        past
    }
}
