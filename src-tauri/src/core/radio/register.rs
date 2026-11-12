//! Pitwall among Claude Code's MCP servers: added and taken out through Claude Code's own CLI
//! (`claude mcp add | remove | get`), never by editing `~/.claude.json`, which Claude Code
//! rewrites all the time; and among Codex's the same way (`codex mcp add | remove | get`), never
//! by editing its `config.toml`. The CLI runs in the user's shell, as a dev server does, and is told
//! to write when the user turns the radio on or off; showing the state only asks, but for two
//! things it puts right: a server whose app is gone while the radio is on, and this app left
//! there by a removal that failed when the radio was turned off.

use std::sync::mpsc;

use super::*;
use crate::mcp::{MCP_FLAG, SERVER_NAME};

/// The CLI starts Claude Code whole, and `get` tries the server out before it answers.
const CLI_LIMIT: Duration = Duration::from_secs(30);
/// What a shell answers for a command it doesn't have (`cmd.exe`: 9009).
const NOT_FOUND: [i32; 2] = [127, 9009];
/// How the CLI says there is no such server, from `get` and from `remove`.
const NO_SERVER: &str = "No MCP server named";
/// How `add` says the name is taken.
const TAKEN: &str = "already exists";

/// What Claude Code has under Pitwall's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Listed {
    /// The server, and it runs this app.
    Own,
    /// The server, and it runs another copy of the app, which is there.
    Other,
    /// The server, and the app it names is gone: it moved, or its build folder was cleaned.
    Gone,
    /// Nothing under the name.
    No,
    /// Claude Code's CLI isn't on the shell's PATH.
    Missing,
    /// It didn't say.
    Unknown,
}

/// What a command said and how it ended.
struct Said {
    code: Option<i32>,
    text: String,
}

/// Runs `line` in the user's shell, in the home folder. `None`: no shell, or no answer in time.
/// Tests never run it: it would be the user's own Claude Code.
fn run(line: &str) -> Option<Said> {
    run_in(line, None)
}

/// `run` for the Claude account whose folder is `account` (none: the main one).
fn run_in(line: &str, account: Option<&str>) -> Option<Said> {
    let extra: Vec<(&str, &str)> = account.map(|dir| (crate::accounts::ENV, dir)).into_iter().collect();
    run_with(line, &extra)
}

/// `run_in` for a line that writes Claude Code's configuration, run again after a moment while
/// it ends badly for no reason it names (`process::retried`): Claude Code turns `mcp add` and
/// `remove` away while another of its processes holds the configuration's lock (2.1.318). A name
/// taken or not there, no CLI, or no answer in time is answer enough.
fn write_in(line: &str, account: Option<&str>) -> Option<Said> {
    process::retried(
        || run_in(line, account),
        |said| matches!(said, Some(Said { code: Some(code), text }) if *code != 0 && !NOT_FOUND.contains(code) && !text.contains(TAKEN) && !text.contains(NO_SERVER)),
    )
}

/// `run` with `extra` in the command's environment: an account's folder for its agent's CLI.
fn run_with(line: &str, extra: &[(&str, &str)]) -> Option<Said> {
    if cfg!(test) {
        return None;
    }

    let home = dirs::home_dir()?;
    let child = process::spawn_shell_with(line, &home.to_string_lossy(), extra).ok()?;
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });

    match receiver.recv_timeout(CLI_LIMIT) {
        Ok(Ok(output)) => {
            let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            Some(Said { code: output.status.code(), text })
        }
        _ => {
            process::stop_groups(&[pid], Duration::from_millis(600), Duration::from_secs(2));
            None
        }
    }
}

/// `text` as one word for the shell `spawn_shell` runs: in single quotes (a `'` in it closes
/// them, stands escaped, and opens them again); for `cmd.exe`, in double quotes.
fn quote(text: &str) -> String {
    if cfg!(windows) {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The app Claude Code is to run: this one. An AppImage runs from a new mount each launch, so
/// there it is the image file (as for the hook, `hooks.rs`).
fn own_app() -> Option<PathBuf> {
    let image = std::env::var_os("APPIMAGE").filter(|image| cfg!(target_os = "linux") && !image.is_empty());
    image.map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

/// The command line that adds `app` as the user's MCP server.
fn add_line(app: &Path) -> String {
    format!("claude mcp add --scope user {SERVER_NAME} -- {} {MCP_FLAG}", quote(&app.to_string_lossy()))
}

/// What a command that was to change something came to, as an error for Settings.
fn settled(said: Option<Said>) -> Result<(), String> {
    settled_for(Agent::Claude, said)
}

/// `settled` for `agent`'s CLI, in words that name it.
fn settled_for(agent: Agent, said: Option<Said>) -> Result<(), String> {
    let codex = agent == Agent::Codex;
    match said {
        Some(Said { code: Some(0), .. }) => Ok(()),
        Some(Said { code: Some(code), .. }) if NOT_FOUND.contains(&code) => Err(t!(if codex { "radio.missing.codex" } else { "radio.missing" })),
        Some(said) => {
            let last = said.text.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
            Err(t!(if codex { "core.radio.cliFailed.codex" } else { "core.radio.cliFailed" }, error = last))
        }
        None => Err(t!(if codex { "core.radio.cliSilent.codex" } else { "core.radio.cliSilent" })),
    }
}

/// Asks Claude Code what it has under Pitwall's name. `get` starts the server to try it out,
/// which changes nothing: this app in `--mcp` mode answers and ends.
pub(super) fn listed() -> Listed {
    let Some(said) = run(&format!("claude mcp get {SERVER_NAME}")) else {
        return Listed::Unknown;
    };
    // The app its `Command:` line names, and this one, each as the file system names it.
    let real = |app: PathBuf| fs::canonicalize(app).ok();
    let app = || said.text.lines().find_map(|line| line.trim().strip_prefix("Command: ")).map(|app| real(PathBuf::from(app.trim())));

    match said.code {
        Some(0) => match app() {
            // The file is gone: an app that moved, a build folder cleaned.
            Some(None) => Listed::Gone,
            Some(app) if app == own_app().and_then(real) => Listed::Own,
            _ => Listed::Other,
        },
        Some(code) if NOT_FOUND.contains(&code) => Listed::Missing,
        _ if said.text.contains(NO_SERVER) => Listed::No,
        _ => Listed::Unknown,
    }
}

/// Adds this app as the user's MCP server `pitwall`. A server already there under the name (an
/// app that moved since) is taken out first: `add` refuses a name that is taken.
pub(super) fn add() -> Result<(), String> {
    let app = own_app().ok_or_else(|| t!("core.radio.cliSilent"))?;
    let line = add_line(&app);

    match write_in(&line, None) {
        Some(said) if said.code != Some(0) && said.text.contains(TAKEN) => {
            remove()?;
            settled(write_in(&line, None))
        }
        said => settled(said),
    }
}

/// Takes Pitwall out of the user's MCP servers; not being there is as good.
pub(super) fn remove() -> Result<(), String> {
    remove_in(None)
}

/// Adds this app as the MCP server `pitwall` of the Claude account whose folder is `account`
/// (another than the main one), through the same CLI given that folder.
pub(super) fn add_in(account: &str) -> Result<(), String> {
    let app = own_app().ok_or_else(|| t!("core.radio.cliSilent"))?;
    let line = add_line(&app);

    match write_in(&line, Some(account)) {
        Some(said) if said.code != Some(0) && said.text.contains(TAKEN) => {
            remove_in(Some(account))?;
            settled(write_in(&line, Some(account)))
        }
        said => settled(said),
    }
}

/// Takes Pitwall out of the MCP servers of the account whose folder is `account` (none: the main
/// one); not being there is as good.
pub(in crate::core) fn remove_in(account: Option<&str>) -> Result<(), String> {
    match write_in(&format!("claude mcp remove {SERVER_NAME} --scope user"), account) {
        Some(said) if said.text.contains(NO_SERVER) => Ok(()),
        said => settled(said),
    }
}

/// Claude Code sessions with a line to Pitwall now: this app's `--mcp` processes, one per
/// session. A command line is only tested.
pub(super) fn lines() -> usize {
    lines_of(Agent::Claude)
}

/// `lines` of `agent`'s sessions: a line whose server Codex started is Codex's (`process::is_codex`), any
/// other Claude's, as before there were agents.
pub(super) fn lines_of(agent: Agent) -> usize {
    let names: Vec<std::ffi::OsString> =
        [own_app(), std::env::current_exe().ok()].into_iter().flatten().filter_map(|app| app.file_name().map(|name| name.to_owned())).collect();
    let Some(table) = process::process_table() else {
        return 0;
    };

    table
        .values()
        .filter(|process| Path::new(&process.program).file_name().is_some_and(|name| names.iter().any(|own| own == name)))
        .filter(|process| process.line.strip_prefix(&process.program).is_some_and(|rest| rest.trim() == MCP_FLAG))
        .filter(|process| table.get(&process.parent).is_some_and(crate::process::is_codex) == (agent == Agent::Codex))
        .count()
}


/// Whether `process`, Codex's, is its shared server (`codex app-server`), which runs many
/// sessions: a call under it is told apart by its folder alone. Its command line is only tested.
pub(super) fn is_codex_daemon(process: &process::ProcessInfo) -> bool {
    process.line.split_whitespace().any(|word| word == "app-server")
}

/// What `agent`'s CLI says its version is (`claude --version`, `codex --version`: the words of
/// the fixed list), for its line in Settings › Agents: the first version in what it printed,
/// nothing else of it.
pub(in crate::core) fn cli_version(agent: Agent) -> Option<String> {
    let said = run(&format!("{} --version", agent.id()))?;
    if said.code != Some(0) {
        return None;
    }
    crate::deps::version_in(&said.text)
}

/// Codex's CLI, for the home `home` (none: the main one, `~/.codex` or `$CODEX_HOME` as the app
/// was given it, which never gets the variable from Pitwall).
fn codex_run(line: &str, home: Option<&Path>) -> Option<Said> {
    let home = home.map(|home| home.to_string_lossy().into_owned());
    let extra: Vec<(&str, &str)> = home.as_deref().map(|home| ("CODEX_HOME", home)).into_iter().collect();
    run_with(line, &extra)
}

/// The command line that adds `app` as Codex's MCP server; `add` puts it in the place of
/// whatever is there under the name, as Codex's CLI does.
fn codex_add_line(app: &Path) -> String {
    format!("codex mcp add {SERVER_NAME} -- {} {MCP_FLAG}", quote(&app.to_string_lossy()))
}

/// What Codex has under Pitwall's name in `home`: `get --json`, whose `transport.command` names
/// the app. It changes nothing.
pub(super) fn codex_listed(home: Option<&Path>) -> Listed {
    let Some(said) = codex_run(&format!("codex mcp get {SERVER_NAME} --json"), home) else {
        return Listed::Unknown;
    };

    match said.code {
        Some(0) => {
            // The JSON is all it prints; a line before it (a warning) is skipped.
            let json = said.text.find('{').map_or("", |start| &said.text[start..]);
            let command = serde_json::from_str::<serde_json::Value>(json)
                .ok()
                .and_then(|server| server.pointer("/transport/command").and_then(serde_json::Value::as_str).map(PathBuf::from));
            match command.map(fs::canonicalize) {
                None => Listed::Unknown,
                Some(Err(_)) => Listed::Gone,
                Some(Ok(app)) if Some(app.clone()) == own_app().and_then(|own| fs::canonicalize(own).ok()) => Listed::Own,
                Some(Ok(_)) => Listed::Other,
            }
        }
        Some(code) if NOT_FOUND.contains(&code) => Listed::Missing,
        _ if said.text.contains(NO_SERVER) => Listed::No,
        _ => Listed::Unknown,
    }
}

/// Adds this app as Codex's MCP server `pitwall` in `home`.
pub(super) fn codex_add(home: Option<&Path>) -> Result<(), String> {
    let app = own_app().ok_or_else(|| t!("core.radio.cliSilent.codex"))?;
    settled_for(Agent::Codex, codex_run(&codex_add_line(&app), home))
}

/// Takes Pitwall out of Codex's MCP servers in `home`; not being there is as good.
pub(in crate::core) fn codex_remove(home: Option<&Path>) -> Result<(), String> {
    match codex_run(&format!("codex mcp remove {SERVER_NAME}"), home) {
        Some(said) if said.text.contains(NO_SERVER) => Ok(()),
        said => settled_for(Agent::Codex, said),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The app's path goes into a command line the user's shell runs, and is then written into
    /// Claude Code's configuration: whatever the path holds, it stays one word and runs nothing.
    #[test]
    fn the_apps_path_reaches_the_shell_as_one_word() {
        for path in ["/Applications/Pitwall.app/Contents/MacOS/pitwall", "/Users/o'brien/My Apps/pit wall", "/tmp/$(echo pwned)/`id`/a;b&&c|d\"e\\f"] {
            let line = add_line(Path::new(path)).replacen("claude", "printf '%s\\n'", 1);
            let output = std::process::Command::new("/bin/sh").args(["-c", &line]).output().unwrap();
            let words: Vec<&str> = std::str::from_utf8(&output.stdout).unwrap().lines().collect();

            assert_eq!(words, ["mcp", "add", "--scope", "user", "pitwall", "--", path, "--mcp"], "{line}");

            let line = codex_add_line(Path::new(path)).replacen("codex", "printf '%s\\n'", 1);
            let output = std::process::Command::new("/bin/sh").args(["-c", &line]).output().unwrap();
            let words: Vec<&str> = std::str::from_utf8(&output.stdout).unwrap().lines().collect();

            assert_eq!(words, ["mcp", "add", "pitwall", "--", path, "--mcp"], "{line}");
        }
    }
}
