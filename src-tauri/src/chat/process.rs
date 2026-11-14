//! A chat's `claude` process: started as a lap's agent in the background is
//! (`core/laps/run.rs` `lap_background`), the program found on the terminal's `PATH` and run
//! directly with the terminal's environment, so its pid is Claude's own and no shell reads any
//! word of its line; in the project's own folder, in a process group of its own (a job object on
//! Windows), its three pipes the core's.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::process;

/// What a chat's process is started with.
pub struct Spawn {
    /// The `claude` program (`process::terminal_program`).
    pub program: PathBuf,
    /// Its arguments (`start::chat_args`).
    pub args: Vec<String>,
    /// The project's own folder.
    pub cwd: PathBuf,
    /// The account's folder, for one that isn't the main one (`accounts::start_with`).
    pub account: Option<PathBuf>,
    /// Settings › Claude: a turn a restart cut off goes on (`continueInterrupted`).
    pub continue_interrupted: bool,
}

/// The variables a chat's Claude is given (the plan's Process section): the session's state as
/// frames, file checkpoints for rewinds, a typed reason for a start that failed.
pub const CHAT_VARS: [(&str, &str); 3] = [
    ("CLAUDE_CODE_SDK_READS_SESSION_STATE", "1"),
    ("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true"),
    ("CLAUDE_CODE_STARTUP_FAILURE_RESULTS", "1"),
];
/// Set while Settings › Claude says a cut-off turn goes on; taken out otherwise.
pub const RESUME_INTERRUPTED: &str = "CLAUDE_CODE_RESUME_INTERRUPTED_TURN";

/// What npm hands a script it runs, which a dev run of Pitwall carries (`open_tab` takes the
/// same out of a terminal).
fn npm_var(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| name.starts_with("npm_") || name == "INIT_CWD" || name == "NODE")
}

/// The command a chat runs, not started: the terminal's environment (`process::terminal_command`)
/// made a chat's by `prepare`.
pub fn command(spawn: &Spawn) -> Command {
    let mut command = process::terminal_command(&spawn.program, &[]);
    command.args(&spawn.args).current_dir(&spawn.cwd);
    prepare(&mut command, spawn.account.as_deref(), spawn.continue_interrupted);
    command
}

/// A chat's environment over what `command` would have: Claude's session variables (a parent
/// Claude's, a card's) and npm's taken out, whatever the app's or the terminal's environment
/// holds; the main account never gets `CLAUDE_CONFIG_DIR`, another one (`account`) its folder;
/// the chat's own variables; its three pipes.
fn prepare(command: &mut Command, account: Option<&Path>, continue_interrupted: bool) {
    process::without_session_vars(command);
    let mut npm: Vec<OsString> = command.get_envs().map(|(name, _)| name.to_os_string()).filter(|name| npm_var(name)).collect();
    npm.extend(std::env::vars_os().map(|(name, _)| name).filter(|name| npm_var(name)));
    for name in npm {
        command.env_remove(name);
    }
    command.env_remove(crate::accounts::ENV);
    if let Some(dir) = account {
        command.env(crate::accounts::ENV, dir);
    }
    command.envs(CHAT_VARS);
    if continue_interrupted {
        command.env(RESUME_INTERRUPTED, "1");
    } else {
        command.env_remove(RESUME_INTERRUPTED);
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
}

/// Starts it, the leader of a process group of its own (on Windows started suspended in a job of
/// its own, `process::spawn_contained`), so a stop takes what it started down with it.
pub fn spawn(spawn: &Spawn) -> std::io::Result<Child> {
    process::spawn_contained(&mut command(spawn))
}

/// The `claude` a chat runs, on the terminal's `PATH`.
pub fn program() -> Option<PathBuf> {
    process::terminal_program("claude")
}

/// Signals the process's whole group: SIGTERM, or SIGKILL with `force` (Windows ends its job).
pub fn stop(pid: u32, force: bool) -> bool {
    process::kill_tree(pid, force)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Test 2: Claude's session variables and npm's never reach a chat's process, whatever the
    /// environment it would have holds; `CLAUDE_CONFIG_DIR` only for an account that isn't the
    /// main one (one dataset over the variables). The environment is the command's own, as the
    /// terminal's would be laid on it, so no test's process environment is touched.
    #[test]
    fn a_parent_sessions_variables_never_reach_a_chat() {
        let carried = [
            "CLAUDECODE",
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_SESSION_ATTENDED",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_EXECPATH",
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN",
            "CLAUDE_PID",
            "CLAUDE_EFFORT",
            "AI_AGENT",
            "PITWALL_PROMPT",
            "PITWALL_IMAGES",
            "CODEX_THREAD_ID",
            "npm_config_prefix",
            "npm_lifecycle_event",
            "INIT_CWD",
            "NODE",
            RESUME_INTERRUPTED,
        ];
        let dir = tempfile::tempdir().unwrap();
        let account = dir.path().join("account");
        for (account, continued) in [(None, false), (Some(account.as_path()), true), (Some(account.as_path()), false), (None, true)] {
            let mut command = Command::new("/usr/bin/env");
            for name in carried {
                command.env(name, "from-a-parent");
            }
            command.env(crate::accounts::ENV, "/somewhere/else");
            prepare(&mut command, account, continued);
            let output = command.stdin(Stdio::null()).output().unwrap();
            let env = String::from_utf8_lossy(&output.stdout).into_owned();
            let set: Vec<(&str, &str)> = env.lines().filter_map(|line| line.split_once('=')).collect();
            for name in carried.iter().filter(|name| **name != RESUME_INTERRUPTED) {
                assert!(!set.iter().any(|(set, _)| set == name), "{name} reached the chat");
            }
            let config: Vec<&str> = set.iter().filter(|(name, _)| *name == crate::accounts::ENV).map(|(_, value)| *value).collect();
            match account {
                None => assert!(config.is_empty(), "the main account got {config:?}"),
                Some(dir) => assert_eq!(config, [dir.to_str().unwrap()]),
            }
            for (name, value) in CHAT_VARS {
                assert!(set.contains(&(name, value)), "{name}");
            }
            let resumed: Vec<&str> = set.iter().filter(|(name, _)| *name == RESUME_INTERRUPTED).map(|(_, value)| *value).collect();
            assert_eq!(resumed, if continued { vec!["1"] } else { vec![] });
        }
    }
}
