//! Project terminals: one login shell per tab, in a pseudo-terminal, started in the project's
//! folder. The UI only draws them (xterm.js) and sends keys; the process, its output and its end
//! live here. Terminals outlive their view: switching projects or hiding the panel keeps them, and
//! the last part of their output is kept to draw them again.

use std::io::Write;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

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
        let (cols, rows) = size.filter(|&(cols, rows)| cols > 0 && rows > 0).unwrap_or((80, 24));
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| error.to_string())?;

        let shell_path = CommandBuilder::new_default_prog().get_shell();
        let mut command = if claude { claude_command(&shell_path) } else { CommandBuilder::new_default_prog() };
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
            let name = if same == 0 { base } else { format!("{base} {}", same + 1) };
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
    }

    /// Every terminal's shell, for shutdown.
    pub(super) fn terminal_pids(&self) -> Vec<u32> {
        self.terminal_sessions().values().filter_map(|session| session.pid).collect()
    }
}

/// The user's shell, interactive and login (so PATH and version managers load), running
/// `claude` and then becoming an ordinary shell when Claude ends.
#[cfg(unix)]
fn claude_command(shell: &str) -> CommandBuilder {
    let mut command = CommandBuilder::new(shell);
    command.args(["-l", "-i", "-c", &format!("claude; exec '{}' -l", shell.replace('\'', "'\\''"))]);
    command
}

#[cfg(windows)]
fn claude_command(_shell: &str) -> CommandBuilder {
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(["/K", "claude"]);
    command
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
