//! Project terminals: one login shell per tab, in a pseudo-terminal, started in the project's
//! folder. The UI only draws them (xterm.js) and sends keys; the process, its output and its end
//! live here. Terminals outlive their view: switching projects or hiding the panel keeps them, and
//! the last part of their output is kept to draw them again.

use std::io::Write;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use super::reveal::is_session_id;
use super::*;

/// Output kept per terminal to redraw it when its view comes back.
const SCROLLBACK_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    pub id: u64,
    pub path: String,
    /// The tab's title; the user can rename it.
    pub name: String,
    /// `shell` | `claude`: a Claude Code session started from its own button.
    pub kind: String,
}

/// A terminal's recent output and the number of its last chunk, so a view can draw the history
/// and then take only the chunks that came after it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TerminalBuffer {
    pub data: String,
    pub seq: u64,
}

/// What a terminal starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Launch {
    /// The user's shell.
    Shell,
    /// Claude Code, then the shell: with a first prompt (a board card's text) and in plan mode.
    Claude { prompt: Option<String>, plan: bool },
    /// Claude Code with a session that has ended opened again (`claude --resume`), then the
    /// shell: a board card's session, going on.
    Resume { session: String },
}

pub(super) struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    path: String,
    scrollback: String,
    /// Number of the last chunk read.
    seq: u64,
}

impl Session {
    pub(super) fn pid_entry(&self) -> Option<PidEntry> {
        self.pid.map(|pid| PidEntry { path: self.path.clone(), pid })
    }
}

impl Core {
    pub(super) fn terminal_sessions(&self) -> MutexGuard<'_, HashMap<u64, Session>> {
        self.terminals.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Opens a terminal in the project's folder: the user's shell, as a login shell. With
    /// `claude`, the shell starts Claude Code first and stays when Claude ends.
    /// `size` (cols, rows) is the view's: a shell that starts at another width draws its first
    /// prompt for that width, and zsh leaves its partial-line mark behind.
    pub fn open_terminal(self: &Arc<Self>, path: &str, claude: bool, size: Option<(u16, u16)>) -> Result<TerminalView, String> {
        let launch = if claude { Launch::Claude { prompt: None, plan: false } } else { Launch::Shell };
        self.open_terminal_as(path, &launch, None, size)
    }

    /// `open_terminal`, starting with `launch`; `name` is the tab's title instead of the
    /// numbered `claude` / shell name.
    pub(super) fn open_terminal_as(
        self: &Arc<Self>,
        path: &str,
        launch: &Launch,
        name: Option<String>,
        size: Option<(u16, u16)>,
    ) -> Result<TerminalView, String> {
        // A folder that is gone would start the shell in the home folder instead, unasked.
        if !Path::new(path).is_dir() {
            return Err(t!("core.error.notFolder", path = path));
        }
        let claude = !matches!(launch, Launch::Shell);
        let (cols, rows) = size.filter(|&(cols, rows)| cols > 0 && rows > 0).unwrap_or((80, 24));
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| error.to_string())?;

        let shell_path = CommandBuilder::new_default_prog().get_shell();
        let mut command = match launch {
            Launch::Claude { prompt, plan } => claude_command(&shell_path, prompt.as_deref(), *plan),
            Launch::Resume { session } => resume_command(&shell_path, session)?,
            Launch::Shell => CommandBuilder::new_default_prog(),
        };
        command.cwd(path);
        // Started under npm (dev runs), the app carries npm's variables; nvm in the shell
        // complains about them. A terminal starts from the user's own environment.
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy();
            if key.starts_with("npm_") || key == "INIT_CWD" || key == "NODE" {
                command.env_remove(key.as_ref());
            }
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Pitwall");
        // Opened from Finder the app has no locale; without one the shell mangles UTF-8.
        if std::env::var_os("LANG").is_none() {
            command.env("LANG", "en_US.UTF-8");
        }
        let base = if claude {
            "claude".to_string()
        } else {
            Path::new(&shell_path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "shell".into())
        };
        let kind = if claude { "claude" } else { "shell" };

        let mut child = pair.slave.spawn_command(command).map_err(|error| error.to_string())?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|error| error.to_string())?;
        let writer = pair.master.take_writer().map_err(|error| error.to_string())?;
        let killer = child.clone_killer();
        let pid = child.process_id();
        // On Windows, where the shell leads no process group, it gets a job of its own instead.
        if let Some(pid) = pid {
            process::contain(pid);
        }

        let view = {
            let mut inner = self.lock();
            inner.next_run += 1;
            let id = inner.next_run;
            let same = inner.terminal_list.iter().filter(|t| t.path == path && t.kind == kind).count();
            let name = match name {
                Some(name) => name,
                None if same == 0 => base,
                None => format!("{base} {}", same + 1),
            };
            let view = TerminalView { id, path: path.to_string(), name, kind: kind.into() };
            inner.terminal_list.push(view.clone());
            view
        };

        self.terminal_sessions().insert(view.id, Session { master: pair.master, writer, killer, pid, path: path.to_string(), scrollback: String::new(), seq: 0 });
        self.record_pids();
        self.notify();

        let core = Arc::clone(self);
        let id = view.id;
        thread::spawn(move || core.read_terminal(id, reader));

        let core = Arc::clone(self);
        thread::spawn(move || {
            let _ = child.wait();
            core.forget_terminal(id);
        });

        Ok(view)
    }

    fn read_terminal(&self, id: u64, mut reader: Box<dyn Read + Send>) {
        let mut buffer = [0u8; 16 * 1024];
        let mut pending: Vec<u8> = Vec::new();

        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };

            pending.extend_from_slice(&buffer[..read]);
            let data = take_text(&mut pending);
            if data.is_empty() {
                continue;
            }

            let seq = match self.terminal_sessions().get_mut(&id) {
                Some(session) => {
                    session.scrollback.push_str(&data);
                    trim_scrollback(&mut session.scrollback);
                    session.seq += 1;
                    session.seq
                }
                None => continue,
            };

            self.emit(CoreEvent::Terminal { id, seq, data });
        }
    }

    /// A new title for a tab; an empty one keeps the old.
    pub fn rename_terminal(&self, id: u64, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }

        let renamed = {
            let mut inner = self.lock();
            match inner.terminal_list.iter_mut().find(|t| t.id == id) {
                Some(view) => {
                    view.name = name.chars().take(40).collect();
                    true
                }
                None => false,
            }
        };

        if renamed {
            self.notify();
        }
    }

    pub fn write_terminal(&self, id: u64, data: &str) {
        if let Some(session) = self.terminal_sessions().get_mut(&id) {
            let _ = session.writer.write_all(data.as_bytes());
            let _ = session.writer.flush();
        }
    }

    pub fn resize_terminal(&self, id: u64, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        if let Some(session) = self.terminal_sessions().get(&id) {
            let _ = session.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    /// What the terminal has shown lately, to draw it again.
    pub fn terminal_buffer(&self, id: u64) -> TerminalBuffer {
        self.terminal_sessions()
            .get(&id)
            .map(|session| TerminalBuffer { data: session.scrollback.clone(), seq: session.seq })
            .unwrap_or(TerminalBuffer { data: String::new(), seq: 0 })
    }

    /// Closes a tab: the pseudo-terminal goes (the shell and its jobs get SIGHUP), then the
    /// shell's process group is ended for good.
    pub fn close_terminal(&self, id: u64) {
        let Some(mut session) = self.terminal_sessions().remove(&id) else {
            return;
        };

        let pid = session.pid;
        let _ = session.killer.kill();
        drop(session);

        if let Some(pid) = pid {
            kill_tree(pid, false);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(600));
                kill_tree(pid, true);
            });
        }

        self.forget_terminal(id);
    }

    /// The shell ended (`exit`, or closed): the tab goes away.
    fn forget_terminal(&self, id: u64) {
        self.terminal_sessions().remove(&id);

        let removed = {
            let mut inner = self.lock();
            let before = inner.terminal_list.len();
            inner.terminal_list.retain(|t| t.id != id);
            before != inner.terminal_list.len()
        };

        if removed {
            self.record_pids();
            self.emit(CoreEvent::TerminalExit { id });
            self.notify();
        }
        // A board card's Claude ran in it: the card goes to review.
        self.board_terminal_gone(id);
    }

    /// Every terminal's shell: its pid, the terminal's id and its project.
    pub(super) fn terminal_shells(&self) -> Vec<(u32, u64, String)> {
        self.terminal_sessions().iter().filter_map(|(id, session)| Some((session.pid?, *id, session.path.clone()))).collect()
    }

    /// Every terminal's shell, for shutdown.
    pub(super) fn terminal_pids(&self) -> Vec<u32> {
        self.terminal_sessions().values().filter_map(|session| session.pid).collect()
    }
}

/// The environment variable a first prompt travels in, from Pitwall to Claude's command line.
#[cfg(unix)]
const PROMPT_VAR: &str = "PITWALL_PROMPT";

/// The user's shell, interactive and login (so PATH and version managers load), running
/// `claude` and then becoming an ordinary shell when Claude ends. `plan` starts Claude in plan
/// mode.
///
/// A first prompt (a board card's text) is never put into the command line: it goes in
/// `PITWALL_PROMPT`, and the fixed script hands it to Claude as one quoted argument after `--`,
/// so no shell syntax in it runs and nothing in it reads as one of Claude's options. The shell
/// that stays after Claude is started without it.
#[cfg(unix)]
pub(super) fn claude_command(shell: &str, prompt: Option<&str>, plan: bool) -> CommandBuilder {
    let quoted = shell.replace('\'', "'\\''");
    let mode = if plan { " --permission-mode plan" } else { "" };
    let script = match prompt {
        Some(_) => format!("claude{mode} -- \"${PROMPT_VAR}\"; exec env -u {PROMPT_VAR} '{quoted}' -l"),
        None => format!("claude{mode}; exec '{quoted}' -l"),
    };

    let mut command = CommandBuilder::new(shell);
    command.args(["-l", "-i", "-c", &script]);
    if let Some(prompt) = prompt {
        command.env(PROMPT_VAR, prompt);
    }
    command
}

/// `cmd.exe /K` can't take arbitrary text safely: `%VAR%` expands before quoting is parsed, so
/// quotes, `&` or `|` in a card would end the argument and run as commands. Claude starts
/// without the card's text here (it stays unused) rather than risk that.
#[cfg(windows)]
pub(super) fn claude_command(_shell: &str, _prompt: Option<&str>, plan: bool) -> CommandBuilder {
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(["/K", "claude"]);
    if plan {
        command.args(["--permission-mode", "plan"]);
    }
    command
}

/// The same shell, opening a session again (`claude --resume <id>`) and then becoming an ordinary
/// shell. The id is written into the command line, so only a session id gets there (a UUID: hex
/// digits and dashes).
#[cfg(unix)]
pub(super) fn resume_command(shell: &str, session: &str) -> Result<CommandBuilder, String> {
    if !is_session_id(session) {
        return Err(format!("not a session id: {session}"));
    }
    let quoted = shell.replace('\'', "'\\''");
    let script = format!("claude --resume {session}; exec '{quoted}' -l");

    let mut command = CommandBuilder::new(shell);
    command.args(["-l", "-i", "-c", &script]);
    Ok(command)
}

#[cfg(windows)]
pub(super) fn resume_command(_shell: &str, session: &str) -> Result<CommandBuilder, String> {
    if !is_session_id(session) {
        return Err(format!("not a session id: {session}"));
    }
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(["/K", "claude", "--resume", session]);
    Ok(command)
}

/// Text out of `pending`; an unfinished UTF-8 sequence at the end waits for the next read.
fn take_text(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(text) => {
            let text = text.to_owned();
            pending.clear();
            text
        }
        Err(error) if error.error_len().is_none() => {
            let valid = error.valid_up_to();
            let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
            pending.drain(..valid);
            text
        }
        Err(_) => {
            let text = String::from_utf8_lossy(pending).into_owned();
            pending.clear();
            text
        }
    }
}

/// Keeps the scrollback under its limit, cutting at a line start.
fn trim_scrollback(scrollback: &mut String) {
    if scrollback.len() <= SCROLLBACK_BYTES {
        return;
    }

    let mut cut = scrollback.len() - SCROLLBACK_BYTES;
    while !scrollback.is_char_boundary(cut) {
        cut += 1;
    }
    let cut = scrollback[cut..].find('\n').map_or(cut, |at| cut + at + 1);
    scrollback.drain(..cut);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A card's text reaches Claude only through `PITWALL_PROMPT`: shell syntax in it never
    /// becomes part of the command line, whatever it holds.
    #[test]
    fn a_card_never_reaches_the_command_line() {
        let title = r#""; touch /tmp/pwned; echo ""#;
        let prompt = format!("{title}\n\n$(touch /tmp/pwned) `touch /tmp/pwned` ' --dangerously-skip-permissions");
        let argv = |command: &CommandBuilder| -> Vec<String> { command.get_argv().iter().map(|arg| arg.to_string_lossy().into_owned()).collect() };

        for plan in [false, true] {
            let command = claude_command("/bin/zsh", Some(&prompt), plan);
            let args = argv(&command);
            for arg in &args {
                assert!(!arg.contains("pwned") && !arg.contains("dangerously"), "{arg}");
            }
            assert_eq!(command.get_env("PITWALL_PROMPT"), Some(std::ffi::OsStr::new(&prompt)));
            // The same command line for any text: it only names the variable.
            assert_eq!(args, argv(&claude_command("/bin/zsh", Some("x"), plan)));

            let script = args.last().unwrap();
            assert!(script.contains(r#"-- "$PITWALL_PROMPT";"#), "{script}");
            // The shell that stays after Claude doesn't keep it.
            assert!(script.ends_with("exec env -u PITWALL_PROMPT '/bin/zsh' -l"), "{script}");
        }
    }

    /// A session opened again has its id written into the command line: nothing but a session
    /// id is ever written there.
    #[test]
    fn only_a_session_id_reaches_the_resume_command() {
        let id = "9bb85e86-f618-4861-9858-03ec8fc36c28";
        let command = resume_command("/bin/zsh", id).unwrap();
        let script = command.get_argv().last().unwrap().to_string_lossy().into_owned();
        assert_eq!(script, format!("claude --resume {id}; exec '/bin/zsh' -l"));

        for bad in ["", "x; touch /tmp/pwned", "$(touch /tmp/pwned)", "9bb85e86-f618-4861-9858-03ec8fc36c2;", "--dangerously-skip-permissions"] {
            assert!(resume_command("/bin/zsh", bad).is_err(), "{bad}");
        }
    }
}
