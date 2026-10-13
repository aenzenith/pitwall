//! Pitwall among Claude Code's MCP servers: added and taken out through Claude Code's own CLI
//! (`claude mcp add | remove | get`), never by editing `~/.claude.json`, which Claude Code
//! rewrites all the time. The CLI runs in the user's shell, as a dev server does, and is told
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
    if cfg!(test) {
        return None;
    }

    let home = dirs::home_dir()?;
    let child = process::spawn_shell(line, &home.to_string_lossy()).ok()?;
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
    match said {
        Some(Said { code: Some(0), .. }) => Ok(()),
        Some(Said { code: Some(code), .. }) if NOT_FOUND.contains(&code) => Err(t!("radio.missing")),
        Some(said) => {
            let last = said.text.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
            Err(t!("core.radio.cliFailed", error = last))
        }
        None => Err(t!("core.radio.cliSilent")),
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

    match run(&line) {
        Some(said) if said.code != Some(0) && said.text.contains("already exists") => {
            remove()?;
            settled(run(&line))
        }
        said => settled(said),
    }
}

/// Takes Pitwall out of the user's MCP servers; not being there is as good.
pub(super) fn remove() -> Result<(), String> {
    match run(&format!("claude mcp remove {SERVER_NAME} --scope user")) {
        Some(said) if said.text.contains(NO_SERVER) => Ok(()),
        said => settled(said),
    }
}

/// Claude Code sessions with a line to Pitwall now: this app's `--mcp` processes, one per
/// session. A command line is only tested.
pub(super) fn lines() -> usize {
    let names: Vec<std::ffi::OsString> =
        [own_app(), std::env::current_exe().ok()].into_iter().flatten().filter_map(|app| app.file_name().map(|name| name.to_owned())).collect();
    let Some(table) = process::process_table() else {
        return 0;
    };

    table
        .values()
        .filter(|process| Path::new(&process.program).file_name().is_some_and(|name| names.iter().any(|own| own == name)))
        .filter(|process| process.line.strip_prefix(&process.program).is_some_and(|rest| rest.trim() == MCP_FLAG))
        .count()
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
        }
    }
}
