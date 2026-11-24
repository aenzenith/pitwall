//! Branch and working-tree state per project, from `git status --porcelain=v2 --branch`, how
//! every read of a project's repository runs (`read_command`), and the fetch that brings its
//! upstream's commits in (`fetch`).

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
    /// The branch has an upstream: it is fetched (`fetch`). The core's alone.
    #[serde(skip)]
    pub upstream: bool,
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
        } else if line.starts_with("# branch.upstream ") {
            info.upstream = true;
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
/// A fetch talks to the remote: a slow link or a host that stopped answering is given up then.
pub const FETCH_LIMIT: Duration = Duration::from_secs(60);

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

/// `git fetch` in the repository at `path`, from its branch's remote, for the commits coming in.
/// It moves the remote-tracking branches alone: no local branch, the index or the work tree.
/// Nothing asks for anything: git's own prompt is off, a credential manager shows no window, and
/// ssh asks a program that always says no for a passphrase or a host's key, never the terminal;
/// a remote that wants an answer fails. It runs no program the repository names (as
/// `read_command`), fetches no submodule and starts no maintenance in the repository. True when
/// it ended well.
pub fn fetch(path: &str) -> bool {
    if !Path::new(path).join(".git").exists() {
        return false;
    }

    let mut command = crate::process::command("git");
    command
        .args(["-C", path, "-c", "core.fsmonitor=false", "-c", "maintenance.auto=false", "-c", "gc.auto=0"])
        .args(["fetch", "--quiet", "--recurse-submodules=no"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null());
    #[cfg(unix)]
    command.env("SSH_ASKPASS", "/usr/bin/false").env("SSH_ASKPASS_REQUIRE", "force");

    crate::process::output_within(&mut command, FETCH_LIMIT).is_ok_and(|output| output.status.success())
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

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    /// Pitwall fetches every listed project on its own: the upstream's new commits come in as the
    /// branch's `behind`, and nothing of the user's moves, neither the local branch nor what is
    /// staged or changed in the work tree.
    #[test]
    fn a_fetch_moves_only_the_remote_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", tmp.path())
                .env("GIT_AUTHOR_NAME", "Dev")
                .env("GIT_AUTHOR_EMAIL", "dev@example.com")
                .env("GIT_COMMITTER_NAME", "Dev")
                .env("GIT_COMMITTER_EMAIL", "dev@example.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let remote = tmp.path().join("remote.git");
        let (mine, theirs) = (tmp.path().join("mine"), tmp.path().join("theirs"));
        git(tmp.path(), &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
        git(tmp.path(), &["clone", "-q", remote.to_str().unwrap(), mine.to_str().unwrap()]);
        fs::write(mine.join("app.txt"), "one\n").unwrap();
        git(&mine, &["add", "app.txt"]);
        git(&mine, &["commit", "-q", "-m", "One"]);
        git(&mine, &["push", "-q", "-u", "origin", "main"]);
        git(tmp.path(), &["clone", "-q", remote.to_str().unwrap(), theirs.to_str().unwrap()]);
        fs::write(theirs.join("app.txt"), "two\n").unwrap();
        git(&theirs, &["commit", "-q", "-am", "Two"]);
        git(&theirs, &["push", "-q"]);

        // Something staged, and something more changed on top of it.
        fs::write(mine.join("app.txt"), "staged\n").unwrap();
        git(&mine, &["add", "app.txt"]);
        fs::write(mine.join("app.txt"), "changed\n").unwrap();
        let path = mine.to_str().unwrap();
        let head = git(&mine, &["rev-parse", "HEAD"]);
        let staged = git(&mine, &["diff", "--cached"]);
        let before = status(path).unwrap();
        assert!(before.upstream);
        assert_eq!(before.behind, 0);

        assert!(fetch(path));
        assert_eq!(status(path).unwrap().behind, 1, "the new commit came in");
        assert_eq!(git(&mine, &["rev-parse", "HEAD"]), head, "the branch stayed");
        assert_eq!(git(&mine, &["diff", "--cached"]), staged, "what was staged stayed");
        assert_eq!(fs::read_to_string(mine.join("app.txt")).unwrap(), "changed\n", "the work tree stayed");

        // A branch with no upstream is no one's to fetch.
        git(&mine, &["switch", "-q", "-c", "alone"]);
        assert!(!status(path).unwrap().upstream);
    }
}
