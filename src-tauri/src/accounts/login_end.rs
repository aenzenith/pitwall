//! When an account's claude.ai sign-in ends, as Claude Code's own CLI says it: `claude auth status
//! --json`, run with the account's folder as `CLAUDE_CONFIG_DIR` (the main account without the
//! variable), through no shell. Of its answer only `loggedIn` and `loginExpiresAt` are read; the
//! rest of it (the account's email, its organisation…) is skipped as it streams past and kept
//! nowhere. Claude Code says it from 2.1.318 on (`SINCE`); an older one isn't asked.
//!
//! Asking may make Claude Code renew its token, and an older one that ended mid-renewal could
//! sign out (fixed in 2.1.317): a slow answer is never cut. Pitwall stops waiting for it, and the
//! process is left to end on its own, stopped only well after (`STOP_AFTER`).

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The first Claude Code that says when its sign-in ends.
pub const SINCE: [u64; 3] = [2, 1, 318];

/// How long its answer is waited for.
const ANSWER_WITHIN: Duration = Duration::from_secs(20);
/// When a process that still hasn't ended is stopped.
const STOP_AFTER: Duration = Duration::from_secs(120);

/// What Claude Code said of an account's sign-in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginEnd {
    /// Whether it is signed in at all; none when it didn't say.
    pub logged_in: Option<bool>,
    /// When the stored claude.ai sign-in ends, in ms; none for any other sign-in (a Console key,
    /// a token of its own) or when it didn't say.
    pub ends_at: Option<i64>,
}

/// `claude auth status --json` as far as it is read. Every other field is skipped unread.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    #[serde(default)]
    logged_in: Option<bool>,
    /// An RFC 3339 time (`2026-11-16T13:19:07.069Z`); ms taken too.
    #[serde(default)]
    login_expires_at: Option<serde_json::Value>,
}

/// Whether the Claude Code whose `--version` said `version` says when its sign-in ends.
pub fn speaks(version: &str) -> bool {
    let numbers: Vec<u64> = version.trim().split(|c: char| !c.is_ascii_digit()).filter(|part| !part.is_empty()).take(3).filter_map(|part| part.parse().ok()).collect();
    numbers.len() == 3 && numbers.as_slice() >= SINCE.as_slice()
}

/// What `claude auth status --json` printed, read from `output` up to its object's end: anything
/// before its first `{` (a warning line) is passed over, and every field but the two is skipped as
/// it streams past. None when there is no such object.
pub fn read(output: impl Read) -> Option<LoginEnd> {
    let mut output = BufReader::new(output);
    let mut before = Vec::new();
    // Up to and with the first `{`; what came before it is dropped.
    output.read_until(b'{', &mut before).ok()?;
    if before.last() != Some(&b'{') {
        return None;
    }
    let object = std::io::Cursor::new(b"{").chain(output);
    let status = Status::deserialize(&mut serde_json::Deserializer::from_reader(object)).ok()?;
    Some(LoginEnd { logged_in: status.logged_in, ends_at: status.login_expires_at.as_ref().and_then(ms_of) })
}

fn ms_of(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::String(text) => chrono::DateTime::parse_from_rfc3339(text.trim()).ok().map(|at| at.timestamp_millis()),
        serde_json::Value::Number(number) => number.as_i64().filter(|ms| *ms > 0),
        _ => None,
    }
}

/// The command that asks Claude Code at `program` about the account whose folder is `dir`, none
/// for the main one: its variable is taken out then, so that one the user's shell sets doesn't
/// name another account.
pub fn command(program: &Path, dir: Option<&str>) -> Command {
    let mut command = match dir {
        Some(dir) => crate::process::terminal_command(program, &[(super::ENV, dir)]),
        None => {
            let mut command = crate::process::terminal_command(program, &[]);
            command.env_remove(super::ENV);
            command
        }
    };
    command.args(["auth", "status", "--json"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command
}

/// Asks Claude Code when the sign-in of the account whose folder is `dir` (none: the main one)
/// ends. None without Claude Code, under test, or when it didn't answer in time.
pub fn ask(dir: Option<&str>) -> Option<LoginEnd> {
    let program = super::sync::claude_program()?;
    let mut child = command(&program, dir).spawn().ok()?;
    let stdout = child.stdout.take()?;
    let (sender, answer) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(read(stdout));
    });
    let said = answer.recv_timeout(ANSWER_WITHIN).ok().flatten();
    thread::spawn(move || let_end(child));
    said
}

/// Waits for `child` to end on its own, stopping it only after `STOP_AFTER`.
fn let_end(mut child: Child) {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if started.elapsed() < STOP_AFTER => thread::sleep(Duration::from_millis(250)),
            Ok(None) => {
                crate::process::kill_tree(child.id(), true);
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Claude Code 2.1.322 prints, its account's details made up.
    const STATUS: &str = r#"{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "email": "someone@example.com",
  "orgId": "00000000-0000-4000-8000-000000000000",
  "orgName": "Example Org",
  "subscriptionType": "max",
  "storedClaudeAiLogin": {"nested": ["values", 1, {"deep": true}]},
  "loginExpiresAt": "2026-11-16T13:19:07.069Z",
  "configDirectory": "/Users/someone/.claude"
}
"#;

    #[test]
    fn only_the_two_fields_are_read() {
        let end = read(STATUS.as_bytes()).expect("an answer");
        assert_eq!(end, LoginEnd { logged_in: Some(true), ends_at: Some(1_794_835_147_069) });
        // Nothing else of it is kept: the type has no other field to hold it.
        let shown = serde_json::to_string(&end).unwrap();
        assert_eq!(shown, r#"{"loggedIn":true,"endsAt":1794835147069}"#);
        assert!(!shown.contains("example"));
    }

    #[test]
    fn a_line_before_the_object_is_passed_over() {
        let printed = format!("Warning: something about the shell\n{STATUS}");
        assert_eq!(read(printed.as_bytes()).and_then(|end| end.ends_at), Some(1_794_835_147_069));
    }

    #[test]
    fn signed_out_or_without_a_date() {
        assert_eq!(read(r#"{"loggedIn": false, "authMethod": "none"}"#.as_bytes()), Some(LoginEnd { logged_in: Some(false), ends_at: None }));
        // An older Claude Code, or another kind of sign-in: no date.
        assert_eq!(read(r#"{"loggedIn": true, "authMethod": "api_key"}"#.as_bytes()), Some(LoginEnd { logged_in: Some(true), ends_at: None }));
        // A date it can't read is none, not a wrong one.
        assert_eq!(read(r#"{"loggedIn": true, "loginExpiresAt": "next week"}"#.as_bytes()).and_then(|end| end.ends_at), None);
        assert_eq!(read(r#"{"loggedIn": true, "loginExpiresAt": 1794835147069}"#.as_bytes()).and_then(|end| end.ends_at), Some(1_794_835_147_069));
    }

    #[test]
    fn no_object_no_answer() {
        assert_eq!(read("command not found\n".as_bytes()), None);
        assert_eq!(read("".as_bytes()), None);
        assert_eq!(read("{\"loggedIn\": tr".as_bytes()), None);
    }

    #[test]
    fn asked_only_from_the_version_that_says_it() {
        assert!(!speaks("2.1.317"));
        assert!(speaks("2.1.318"));
        assert!(speaks("2.1.322"));
        assert!(speaks("2.2.0"));
        assert!(speaks("3.0.1 (Claude Code)"));
        assert!(!speaks("2.0.999"));
        assert!(!speaks("1.9.400"));
        assert!(!speaks("2.1"));
        assert!(!speaks(""));
    }

    /// The main account is asked without the variable (one the shell set would name another
    /// account's sign-in); any other with its own folder.
    #[test]
    fn each_account_is_asked_with_its_own_folder() {
        let env_of = |command: &Command| command.get_envs().find(|(name, _)| *name == super::super::ENV).map(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()));
        let program = Path::new("/usr/bin/true");

        let main = command(program, None);
        assert_eq!(env_of(&main), Some(None), "the main account's variable is taken out");

        let other = command(program, Some("/Users/someone/.claude-work"));
        assert_eq!(env_of(&other), Some(Some("/Users/someone/.claude-work".to_string())));

        let args: Vec<_> = other.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["auth", "status", "--json"]);
    }
}
