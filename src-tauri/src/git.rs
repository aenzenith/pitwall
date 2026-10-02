//! Branch and working-tree state per project, from `git status --porcelain=v2 --branch`.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GitInfo {
    /// Branch name, or the short commit when detached.
    pub branch: String,
    /// Changed, staged, untracked and conflicted entries.
    pub changes: u32,
    pub ahead: u32,
    pub behind: u32,
}

pub fn parse_status(output: &str) -> Option<GitInfo> {
    let mut info = GitInfo::default();
    let mut oid = String::new();
    let mut seen_head = false;

    for line in output.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            info.branch = head.to_string();
            seen_head = true;
        } else if let Some(id) = line.strip_prefix("# branch.oid ") {
            oid = id.chars().take(7).collect();
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    info.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    info.behind = n.parse().unwrap_or(0);
                }
            }
        } else if !line.starts_with('#') && !line.is_empty() {
            info.changes += 1;
        }
    }

    if info.branch == "(detached)" {
        info.branch = oid;
    }

    seen_head.then_some(info)
}

/// `None` when the folder isn't a Git work tree or git isn't available.
pub fn status(path: &str) -> Option<GitInfo> {
    if !Path::new(path).join(".git").exists() {
        return None;
    }

    let output = Command::new("git")
        .args(["-C", path, "status", "--porcelain=v2", "--branch", "--untracked-files=normal"])
        // Read-only: don't take the index lock and race the user's own git commands.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    output.status.success().then(|| parse_status(&String::from_utf8_lossy(&output.stdout))).flatten()
}
