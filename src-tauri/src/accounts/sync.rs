//! An account's MCP servers brought level with the main account's: the servers the main account's
//! `~/.claude.json` names (the user's, and each listed project's own) are given to the other
//! account through Claude Code's own CLI (`claude mcp add-json`, run with that account's folder);
//! Pitwall writes into neither file. Of those files only the servers are read, held for the sync
//! and dropped: a server's settings can hold keys and tokens (an environment variable, a header),
//! which go nowhere but into that CLI call's arguments, never through a shell, and the window gets
//! the servers' names alone. The main account wins over a server of the same name; one only the
//! other account has is left alone, and so is Pitwall's own (the radio's, `mcp::SERVER_NAME`).
//! A remote server signs in on its own in each account: it is brought over all the same, and
//! said to maybe want its sign-in in Claude (`/mcp`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::registry::same_path;

/// One CLI call starts Claude Code whole.
const CLI_LIMIT: Duration = Duration::from_secs(60);

/// One sync at a time: two would rewrite the same `.claude.json` together.
static SYNCING: Mutex<()> = Mutex::new(());

/// A server of a sync, as the window shows it: its name only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncServer {
    pub name: String,
    /// `synced` | `same` | `missing` | `login` | `onlyHere` | `failed`
    pub state: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub servers: Vec<SyncServer>,
}

impl SyncReport {
    /// Some server the main account has is missing here (or differs).
    pub fn differs(&self) -> bool {
        self.servers.iter().any(|server| server.state == "missing")
    }

    pub fn failed(&self) -> bool {
        self.servers.iter().any(|server| server.state == "failed")
    }
}

/// The servers of a `.claude.json` and nothing else of it: every other field is skipped as it
/// is read, never kept.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Config {
    #[serde(rename = "mcpServers")]
    mcp_servers: Map<String, Value>,
    projects: BTreeMap<String, ProjectConfig>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProjectConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: Map<String, Value>,
}

/// Where a server is: the user's (every folder), or one project's own (`local`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    User,
    Local(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Server {
    scope: Scope,
    name: String,
    config: Value,
}

impl Server {
    fn same_place(&self, other: &Server) -> bool {
        self.name == other.name
            && match (&self.scope, &other.scope) {
                (Scope::User, Scope::User) => true,
                (Scope::Local(a), Scope::Local(b)) => same_path(a, b),
                _ => false,
            }
    }

    /// As the window names it: a project's own with its folder's name.
    fn shown(&self) -> String {
        match &self.scope {
            Scope::User => self.name.clone(),
            Scope::Local(path) => {
                let folder = Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                format!("{} · {folder}", self.name)
            }
        }
    }

    /// A remote server without a key of its own in its headers signs in through Claude, per
    /// account.
    fn signs_in(&self) -> bool {
        let remote = matches!(self.config.get("type").and_then(Value::as_str), Some("http" | "sse" | "streamable-http" | "ws")) || self.config.get("url").is_some();
        let keyed = self.config.get("headers").and_then(Value::as_object).is_some_and(|headers| headers.keys().any(|key| key.eq_ignore_ascii_case("authorization")));
        remote && !keyed
    }
}

/// The servers of the `.claude.json` at `file`: the user's, and those of the projects in
/// `listed`. None for a file that isn't there or doesn't read.
fn read(file: &Path, listed: &[String]) -> Vec<Server> {
    let Ok(raw) = std::fs::read(file) else {
        return Vec::new();
    };
    let config: Config = serde_json::from_slice(&raw).unwrap_or_default();
    drop(raw);

    let own = |name: &String| name != crate::mcp::SERVER_NAME;
    let mut servers: Vec<Server> =
        config.mcp_servers.into_iter().filter(|(name, _)| own(name)).map(|(name, config)| Server { scope: Scope::User, name, config }).collect();
    for (path, project) in config.projects {
        if listed.iter().any(|listed| same_path(listed, &path)) {
            servers.extend(project.mcp_servers.into_iter().filter(|(name, _)| own(name)).map(|(name, config)| Server { scope: Scope::Local(path.clone()), name, config }));
        }
    }
    servers
}

/// What a sync does to one of the main account's servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Same,
    Add,
    /// The other account has one of that name, set otherwise: it goes first.
    Replace,
}

/// What a sync would do: each of the main account's servers, and the other account's own.
#[derive(Debug, Default)]
pub struct Plan {
    steps: Vec<(Server, Step)>,
    only_here: Vec<Server>,
}

fn plan_of(main: Vec<Server>, other: Vec<Server>) -> Plan {
    let steps = main
        .into_iter()
        .map(|server| {
            let step = match other.iter().find(|theirs| theirs.same_place(&server)) {
                Some(theirs) if theirs.config == server.config => Step::Same,
                Some(_) => Step::Replace,
                None => Step::Add,
            };
            (server, step)
        })
        .collect::<Vec<_>>();
    let only_here = other.into_iter().filter(|theirs| !steps.iter().any(|(server, _)| server.same_place(theirs))).collect();
    Plan { steps, only_here }
}

/// The plan for the account whose folder is `dir`, from the two `.claude.json` files: the main
/// account's in the home folder, the other's in its folder.
pub fn plan(home: &Path, dir: &str, listed: &[String]) -> Plan {
    plan_of(read(&home.join(".claude.json"), listed), read(&Path::new(dir).join(".claude.json"), listed))
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
                    Step::Add | Step::Replace => "missing",
                };
                SyncServer { name: server.shown(), state }
            })
            .collect();
        servers.extend(self.only_here.iter().map(|server| SyncServer { name: server.shown(), state: "onlyHere" }));
        SyncReport { servers }
    }

    /// Carries the plan out through `run`, which is handed each CLI call's arguments and the
    /// folder it runs in (a project's for its own servers) and says whether it went well.
    fn carry_out(self, mut run: impl FnMut(&[String], Option<&str>) -> bool) -> SyncReport {
        let mut servers = Vec::new();
        for (server, step) in self.steps {
            let (scope, cwd) = match &server.scope {
                Scope::User => ("user", None),
                Scope::Local(path) => ("local", Some(path.as_str())),
            };
            let done = match step {
                Step::Same => true,
                Step::Add | Step::Replace => {
                    if step == Step::Replace {
                        // Not being there any more is as good: `add-json` says otherwise.
                        run(&["mcp".into(), "remove".into(), "-s".into(), scope.into(), server.name.clone()], cwd);
                    }
                    let json = serde_json::to_string(&server.config).unwrap_or_default();
                    run(&["mcp".into(), "add-json".into(), "-s".into(), scope.into(), server.name.clone(), json], cwd)
                }
            };
            let state = match (done, step) {
                (false, _) => "failed",
                (true, _) if server.signs_in() => "login",
                (true, Step::Same) => "same",
                (true, _) => "synced",
            };
            servers.push(SyncServer { name: server.shown(), state });
        }
        servers.extend(self.only_here.iter().map(|server| SyncServer { name: server.shown(), state: "onlyHere" }));
        SyncReport { servers }
    }
}

/// Syncs the account whose folder is `dir` (another than the main one): its CLI is Claude Code's
/// own, found on the terminal's `PATH`, given that folder. `None` when Claude Code isn't there.
pub fn sync(home: &Path, dir: &str, listed: &[String]) -> Option<SyncReport> {
    let _one = SYNCING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let claude = claude_program()?;
    let plan = plan(home, dir, listed);
    Some(plan.carry_out(|args, cwd| run_cli(&claude, dir, home, args, cwd)))
}

/// Claude Code's CLI. Tests never find it: it would be the user's own.
pub fn claude_program() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::process::terminal_program("claude")
}

/// One CLI call for the account whose folder is `dir`, in `cwd` (none: the home folder). What it
/// prints is never kept. A project's folder that isn't there fails it: in the home folder the
/// server would be the home folder's own.
fn run_cli(claude: &Path, dir: &str, home: &Path, args: &[String], cwd: Option<&str>) -> bool {
    let cwd = match cwd {
        Some(cwd) if Path::new(cwd).is_dir() => PathBuf::from(cwd),
        Some(_) => return false,
        None => home.to_path_buf(),
    };
    let mut command = crate::process::terminal_command(claude, &[(super::ENV, dir)]);
    command.args(args).current_dir(cwd);
    crate::process::output_within(&mut command, CLI_LIMIT).is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(file: &Path, value: Value) {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, value.to_string()).unwrap();
    }

    /// A sync writes another account's MCP servers: the main account's go over (one set
    /// otherwise replaced), but what that account has alone stays untouched, Pitwall's own entry
    /// is the radio's to write, a project not listed is left out, and nothing else of either file
    /// is read into what the window gets.
    #[test]
    fn a_sync_writes_the_main_accounts_servers_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, dir) = (tmp.path().join("home"), tmp.path().join("home/.claude-work"));
        let shop = home.join("projects/shop").to_string_lossy().into_owned();
        let secret = json!({ "command": "npx", "args": ["gh-mcp"], "env": { "GITHUB_TOKEN": "ghp_secret" } });
        write(
            &home.join(".claude.json"),
            json!({
                "oauthAccount": { "emailAddress": "me@example.com" },
                "mcpServers": { "github": secret, "figma": { "type": "http", "url": "https://mcp.figma.com/mcp" }, "notes": { "command": "notes" }, "pitwall": { "command": "/Applications/Pitwall.app/x", "args": ["--mcp"] } },
                "projects": { shop.clone(): { "mcpServers": { "db": { "command": "db-mcp" } } }, "/elsewhere": { "mcpServers": { "other": { "command": "x" } } } }
            }),
        );
        write(
            &dir.join(".claude.json"),
            json!({ "mcpServers": { "notes": { "command": "old-notes" }, "mine": { "command": "mine" }, "pitwall": { "command": "/old/pitwall", "args": ["--mcp"] } } }),
        );

        let plan = plan(&home, &dir.to_string_lossy(), std::slice::from_ref(&shop));
        let mut calls: Vec<(Vec<String>, Option<String>)> = Vec::new();
        let report = plan.carry_out(|args, cwd| {
            calls.push((args.to_vec(), cwd.map(str::to_string)));
            true
        });

        let call = |args: &[&str], cwd: Option<&str>| (args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(), cwd.map(str::to_string));
        assert_eq!(
            calls,
            [
                call(&["mcp", "add-json", "-s", "user", "github", &secret.to_string()], None),
                call(&["mcp", "add-json", "-s", "user", "figma", r#"{"type":"http","url":"https://mcp.figma.com/mcp"}"#], None),
                call(&["mcp", "remove", "-s", "user", "notes"], None),
                call(&["mcp", "add-json", "-s", "user", "notes", r#"{"command":"notes"}"#], None),
                call(&["mcp", "add-json", "-s", "local", "db", r#"{"command":"db-mcp"}"#], Some(&shop)),
            ]
        );
        let shown: Vec<(&str, &str)> = report.servers.iter().map(|server| (server.name.as_str(), server.state)).collect();
        assert_eq!(shown, [("github", "synced"), ("figma", "login"), ("notes", "synced"), ("db · shop", "synced"), ("mine", "onlyHere")]);
        assert!(!serde_json::to_string(&report).unwrap().contains("ghp_secret"), "names only reach the window");
    }

    #[test]
    fn a_plan_tells_what_differs_before_anything_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, dir) = (tmp.path().to_path_buf(), tmp.path().join(".claude-work"));
        write(&home.join(".claude.json"), json!({ "mcpServers": { "a": { "command": "a" }, "b": { "command": "b" } } }));
        write(&dir.join(".claude.json"), json!({ "mcpServers": { "a": { "command": "a" } } }));
        let report = plan(&home, &dir.to_string_lossy(), &[]).report();
        assert!(report.differs());
        let shown: Vec<(&str, &str)> = report.servers.iter().map(|server| (server.name.as_str(), server.state)).collect();
        assert_eq!(shown, [("a", "same"), ("b", "missing")]);
        // An account with no file of its own yet: everything is missing.
        assert_eq!(plan(&home, &tmp.path().join("none").to_string_lossy(), &[]).report().servers.len(), 2);
    }
}
