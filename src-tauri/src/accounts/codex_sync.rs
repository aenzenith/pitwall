//! A Codex account's MCP servers brought level with the main account's, through Codex's own CLI
//! alone: each home's servers are what `codex mcp list --json` says of it, and the other account
//! is given the main one's with `codex mcp add`, run with its `CODEX_HOME`, which puts a server
//! whole in the place of one of the same name: one that fails leaves the old one there. Pitwall
//! neither reads nor writes `config.toml`. Of the list only the servers' settings are taken, held
//! for the sync and dropped: a server's settings can hold keys and tokens (an environment
//! variable), which go nowhere but into that CLI call's arguments, never through a shell, and the
//! window gets the servers' names alone. The main account wins over a server of the same name;
//! one only the other account has is left alone, and so is Pitwall's own (the radio's,
//! `mcp::SERVER_NAME`). A server the CLI's `add` can't say as it is set (headers, a working
//! folder, variables passed through, timeouts, tools picked, turned off) is named, never copied
//! in part. A remote server without a bearer variable signs in on its own in each account
//! (`codex mcp login`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::sync::{SyncReport, SyncServer};

/// One CLI call starts Codex whole.
const CLI_LIMIT: Duration = Duration::from_secs(60);

/// One sync at a time: two would rewrite the same `config.toml` together.
static SYNCING: Mutex<()> = Mutex::new(());

/// A server as `codex mcp list --json` gives it: its name and settings, nothing of its state.
#[derive(Deserialize)]
struct Listed {
    name: String,
    enabled: Option<bool>,
    transport: Option<Value>,
    startup_timeout_sec: Option<Value>,
    tool_timeout_sec: Option<Value>,
    enabled_tools: Option<Value>,
    disabled_tools: Option<Value>,
}

/// How `codex mcp add` can say a server.
#[derive(Debug, Clone, PartialEq)]
enum How {
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, String> },
    Http { url: String, bearer: Option<String> },
    /// Set in a way `add` can't say.
    Cannot,
}

#[derive(Debug, Clone, PartialEq)]
struct Server {
    name: String,
    /// Its settings as listed, to tell two of the same name apart.
    shape: Value,
    how: How,
}

impl Server {
    /// A remote server without a key of its own signs in through Codex, per account.
    fn signs_in(&self) -> bool {
        matches!(&self.how, How::Http { bearer: None, .. })
    }
}

/// Nothing set: none, null, an empty list or table.
fn unset(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(fields)) => fields.is_empty(),
        _ => false,
    }
}

/// A name the CLI takes as a server's, never as an option.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn how(listed: &Listed) -> How {
    let Some(transport) = listed.transport.as_ref().and_then(Value::as_object) else {
        return How::Cannot;
    };
    let extra = [listed.startup_timeout_sec.as_ref(), listed.tool_timeout_sec.as_ref(), listed.enabled_tools.as_ref(), listed.disabled_tools.as_ref()];
    if listed.enabled == Some(false) || !plain_name(&listed.name) || !extra.into_iter().all(unset) {
        return How::Cannot;
    }
    let text = |key: &str| transport.get(key).and_then(Value::as_str).map(str::to_string);
    // Every field but these must be unset for `add` to say the server whole.
    let only = |keys: &[&str]| transport.iter().all(|(key, value)| key == "type" || keys.contains(&key.as_str()) || unset(Some(value)));
    match transport.get("type").and_then(Value::as_str) {
        Some("stdio") if only(&["command", "args", "env"]) => {
            let Some(command) = text("command").filter(|command| !command.is_empty()) else {
                return How::Cannot;
            };
            let args: Option<Vec<String>> = match transport.get("args") {
                None | Some(Value::Null) => Some(Vec::new()),
                Some(Value::Array(args)) => args.iter().map(|arg| arg.as_str().map(str::to_string)).collect(),
                Some(_) => None,
            };
            let env: Option<BTreeMap<String, String>> = match transport.get("env") {
                None | Some(Value::Null) => Some(BTreeMap::new()),
                // A name `--env` can't say makes the whole server one `add` can't.
                Some(Value::Object(env)) => env
                    .iter()
                    .map(|(key, value)| {
                        let value = value.as_str()?;
                        (!key.is_empty() && !key.contains('=')).then(|| (key.clone(), value.to_string()))
                    })
                    .collect(),
                Some(_) => None,
            };
            match (args, env) {
                (Some(args), Some(env)) => How::Stdio { command, args, env },
                _ => How::Cannot,
            }
        }
        Some("streamable_http") if only(&["url", "bearer_token_env_var"]) => match text("url").filter(|url| url.starts_with("https://") || url.starts_with("http://")) {
            Some(url) => How::Http { url, bearer: text("bearer_token_env_var").filter(|var| !var.is_empty()) },
            None => How::Cannot,
        },
        _ => How::Cannot,
    }
}

/// The servers of a home as `codex mcp list --json` printed them, Pitwall's own left out.
fn servers_of(listed: &[u8]) -> Vec<Server> {
    let listed: Vec<Listed> = serde_json::from_slice(listed).unwrap_or_default();
    listed
        .into_iter()
        .filter(|server| server.name != crate::mcp::SERVER_NAME)
        .map(|server| {
            let how = how(&server);
            let shape = serde_json::json!({
                "enabled": server.enabled,
                "transport": server.transport,
                "startup_timeout_sec": server.startup_timeout_sec,
                "tool_timeout_sec": server.tool_timeout_sec,
                "enabled_tools": server.enabled_tools,
                "disabled_tools": server.disabled_tools,
            });
            Server { name: server.name, shape, how }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Same,
    Add,
    /// The other account has one of that name, set otherwise: `add` puts the main one's in its
    /// place.
    Replace,
}

/// What a sync would do: each of the main account's servers, and the other account's own.
#[derive(Debug, Default)]
pub struct Plan {
    steps: Vec<(Server, Step)>,
    only_here: Vec<Server>,
}

fn plan_of(main: Vec<Server>, other: Vec<Server>) -> Plan {
    let steps: Vec<(Server, Step)> = main
        .into_iter()
        .map(|server| {
            let step = match other.iter().find(|theirs| theirs.name == server.name) {
                Some(theirs) if theirs.shape == server.shape => Step::Same,
                Some(_) => Step::Replace,
                None => Step::Add,
            };
            (server, step)
        })
        .collect();
    let only_here = other.into_iter().filter(|theirs| !steps.iter().any(|(server, _)| server.name == theirs.name)).collect();
    Plan { steps, only_here }
}

/// The plan for the account whose home is `dir`: both homes listed through Codex's CLI. Empty
/// when Codex isn't there (or can't list).
pub fn plan(home: &Path, dir: &str) -> Plan {
    let Some(codex) = codex_program() else {
        return Plan::default();
    };
    let list = |dir: Option<&str>| list_cli(&codex, dir, home).map(|listed| servers_of(&listed)).unwrap_or_default();
    plan_of(list(None), list(Some(dir)))
}

impl Plan {
    /// The plan as the window shows it, before syncing.
    pub fn report(&self) -> SyncReport {
        let mut servers: Vec<SyncServer> = self
            .steps
            .iter()
            .map(|(server, step)| {
                let state = match step {
                    Step::Same if server.signs_in() => "login",
                    Step::Same => "same",
                    _ if server.how == How::Cannot => "cantCopy",
                    Step::Add | Step::Replace => "missing",
                };
                SyncServer { name: server.name.clone(), state }
            })
            .collect();
        servers.extend(self.only_here.iter().map(|server| SyncServer { name: server.name.clone(), state: "onlyHere" }));
        SyncReport { servers }
    }

    /// Carries the plan out through `run`, which is handed each CLI call's arguments and says
    /// whether it went well. A server `add` can't say is left as it is, there and here.
    fn carry_out(self, mut run: impl FnMut(&[String]) -> bool) -> SyncReport {
        let mut servers = Vec::new();
        for (server, step) in self.steps {
            let add = match &server.how {
                How::Stdio { command, args, env } => {
                    let mut add: Vec<String> = vec!["mcp".into(), "add".into(), server.name.clone()];
                    for (key, value) in env {
                        add.extend(["--env".into(), format!("{key}={value}")]);
                    }
                    add.push("--".into());
                    add.push(command.clone());
                    add.extend(args.iter().cloned());
                    Some(add)
                }
                How::Http { url, bearer } => {
                    let mut add: Vec<String> = vec!["mcp".into(), "add".into(), server.name.clone(), "--url".into(), url.clone()];
                    if let Some(bearer) = bearer {
                        add.extend(["--bearer-token-env-var".into(), bearer.clone()]);
                    }
                    Some(add)
                }
                How::Cannot => None,
            };
            let state = match (step, add) {
                (Step::Same, _) if server.signs_in() => "login",
                (Step::Same, _) => "same",
                (_, None) => "cantCopy",
                // Never taken out first: `add` replaces it whole, and one that fails leaves it.
                (_, Some(add)) => match run(&add) {
                    false => "failed",
                    true if server.signs_in() => "login",
                    true => "synced",
                },
            };
            servers.push(SyncServer { name: server.name.clone(), state });
        }
        servers.extend(self.only_here.iter().map(|server| SyncServer { name: server.name.clone(), state: "onlyHere" }));
        SyncReport { servers }
    }
}

/// Syncs the account whose home is `dir` (another than the main one). `None` when Codex isn't
/// there.
pub fn sync(home: &Path, dir: &str) -> Option<SyncReport> {
    let _one = SYNCING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let codex = codex_program()?;
    let plan = plan(home, dir);
    Some(plan.carry_out(|args| run_cli(&codex, dir, home, args)))
}

/// Codex's CLI. Tests never find it: it would be the user's own.
pub fn codex_program() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::process::terminal_program("codex")
}

/// `codex mcp list --json` of the home `dir` (none: the main one's, which is given no variable).
fn list_cli(codex: &Path, dir: Option<&str>, home: &Path) -> Option<Vec<u8>> {
    let env: Vec<(&str, &str)> = dir.map(|dir| (super::codex::ENV, dir)).into_iter().collect();
    let mut command = crate::process::terminal_command(codex, &env);
    command.args(["mcp", "list", "--json"]).current_dir(home);
    let output = crate::process::output_within(&mut command, CLI_LIMIT).ok()?;
    output.status.success().then_some(output.stdout)
}

/// One CLI call for the account whose home is `dir`. What it prints is never kept.
fn run_cli(codex: &Path, dir: &str, home: &Path, args: &[String]) -> bool {
    let mut command = crate::process::terminal_command(codex, &[(super::codex::ENV, dir)]);
    command.args(args).current_dir(home);
    crate::process::output_within(&mut command, CLI_LIMIT).is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stdio(name: &str, command: &str, args: &[&str], env: Value) -> Value {
        json!({ "name": name, "enabled": true, "disabled_reason": null, "transport": { "type": "stdio", "command": command, "args": args, "env": env, "env_vars": [], "cwd": null }, "startup_timeout_sec": null, "tool_timeout_sec": null, "auth_status": "unsupported" })
    }

    fn http(name: &str, url: &str, bearer: Option<&str>, headers: Value) -> Value {
        json!({ "name": name, "enabled": true, "disabled_reason": null, "transport": { "type": "streamable_http", "url": url, "bearer_token_env_var": bearer, "http_headers": headers, "env_http_headers": null }, "startup_timeout_sec": null, "tool_timeout_sec": null, "auth_status": "not_logged_in" })
    }

    /// A sync writes another account's MCP servers through `codex mcp add` alone: the main
    /// account's go over (one set otherwise replaced), a key in a server's settings only into
    /// that call's arguments, a server `add` can't say as it is (headers here) is named and not
    /// copied in part, what that account has alone stays, Pitwall's own entry is the radio's,
    /// and nothing but names reaches the window.
    #[test]
    fn a_codex_sync_adds_the_main_accounts_servers_and_nothing_else() {
        let main = json!([
            stdio("github", "npx", &["-y", "gh-mcp"], json!({ "GITHUB_TOKEN": "ghp_secret" })),
            http("figma", "https://mcp.figma.com/mcp", None, Value::Null),
            http("linear", "https://mcp.linear.app/mcp", Some("LINEAR_KEY"), Value::Null),
            http("headed", "https://example.com/mcp", None, json!({ "X-Key": "hdr_secret" })),
            stdio("notes", "notes", &[], Value::Null),
            stdio("same", "same", &["--flag"], Value::Null),
            stdio("--bad", "x", &[], Value::Null),
            stdio("pitwall", "/Applications/Pitwall.app/Contents/MacOS/pitwall", &["--mcp"], Value::Null),
        ]);
        let theirs = json!([stdio("notes", "old-notes", &[], Value::Null), stdio("same", "same", &["--flag"], Value::Null), stdio("mine", "mine", &[], Value::Null), stdio("pitwall", "/old", &["--mcp"], Value::Null)]);
        let plan = plan_of(servers_of(main.to_string().as_bytes()), servers_of(theirs.to_string().as_bytes()));
        let before = plan.report();

        let mut calls: Vec<Vec<String>> = Vec::new();
        let report = plan.carry_out(|args| {
            calls.push(args.to_vec());
            true
        });

        let call = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                call(&["mcp", "add", "github", "--env", "GITHUB_TOKEN=ghp_secret", "--", "npx", "-y", "gh-mcp"]),
                call(&["mcp", "add", "figma", "--url", "https://mcp.figma.com/mcp"]),
                call(&["mcp", "add", "linear", "--url", "https://mcp.linear.app/mcp", "--bearer-token-env-var", "LINEAR_KEY"]),
                call(&["mcp", "add", "notes", "--", "notes"]),
            ]
        );
        let shown = |report: &SyncReport| report.servers.iter().map(|server| (server.name.clone(), server.state)).collect::<Vec<_>>();
        let named = |pairs: &[(&str, &'static str)]| pairs.iter().map(|(name, state)| (name.to_string(), *state)).collect::<Vec<_>>();
        assert_eq!(
            shown(&report),
            named(&[("github", "synced"), ("figma", "login"), ("linear", "synced"), ("headed", "cantCopy"), ("notes", "synced"), ("same", "same"), ("--bad", "cantCopy"), ("mine", "onlyHere")])
        );
        assert_eq!(
            shown(&before),
            named(&[("github", "missing"), ("figma", "missing"), ("linear", "missing"), ("headed", "cantCopy"), ("notes", "missing"), ("same", "same"), ("--bad", "cantCopy"), ("mine", "onlyHere")])
        );
        let sent = serde_json::to_string(&report).unwrap();
        assert!(!sent.contains("ghp_secret") && !sent.contains("hdr_secret"), "names only reach the window");
    }
}
