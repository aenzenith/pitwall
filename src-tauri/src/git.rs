//! Branch and working-tree state per project, from `git status --porcelain=v2 --branch`, and
//! how every read of a project's repository runs (`read_command`).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

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

/// How long a read of a repository may take before it is given up (`process::output_within`):
/// status walks the whole work tree, log the history; the rest look a value up.
pub const STATUS_LIMIT: Duration = Duration::from_secs(15);
pub const LOG_LIMIT: Duration = Duration::from_secs(10);
pub const LOOKUP_LIMIT: Duration = Duration::from_secs(5);

/// git in the repository at `path`, for a read Pitwall makes on its own: it runs no program the
/// repository names (`core.fsmonitor`'s hook, which status would start; `gpg.program`, which
/// `log.showSignature` has log start for every signed commit), takes no lock the user's own git
/// commands would race for, and reads no input.
pub fn read_command(path: &str) -> Command {
    let mut command = crate::process::command("git");
    command
        .args(["-C", path, "-c", "core.fsmonitor=false", "-c", "log.showSignature=false"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    command
}

/// A status read that didn't end within `STATUS_LIMIT`; git was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedOut;

/// `None` when the folder isn't a Git work tree, git isn't available or the read didn't end in
/// time.
pub fn status(path: &str) -> Option<GitInfo> {
    read_status(path).ok().flatten()
}

/// `Ok(None)` when the folder isn't a Git work tree or git isn't available.
pub fn read_status(path: &str) -> Result<Option<GitInfo>, TimedOut> {
    if !Path::new(path).join(".git").exists() {
        return Ok(None);
    }

    let mut command = read_command(path);
    command.args(["status", "--porcelain=v2", "--branch", "--untracked-files=normal"]);
    let output = match crate::process::output_within(&mut command, STATUS_LIMIT) {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => return Err(TimedOut),
        Err(_) => return Ok(None),
    };

    Ok(output.status.success().then(|| parse_status(&String::from_utf8_lossy(&output.stdout))).flatten())
}
