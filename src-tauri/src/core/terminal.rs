//! Project terminals: one login shell per tab, in a pseudo-terminal, started in the project's
//! folder. The UI only draws them (xterm.js) and sends keys; the process, its output and its end
//! live here. Terminals outlive their view: switching projects or hiding the panel keeps them, and
//! the last part of their output is kept to draw them again.

use std::io::Write;
use std::sync::mpsc;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use super::board::tab_name;
use super::reveal::is_session_id;
use super::shells::{self, Shell};
use super::*;
use crate::start::Start;

/// Output kept per terminal to redraw it when its view comes back.
const SCROLLBACK_BYTES: usize = 512 * 1024;
/// The longest name a tab keeps, as long as a card's title may be; the tab cuts what doesn't fit.
pub(super) const TAB_NAME_MAX: usize = 200;
/// How long output gathers before it goes to the window once some has just gone. A flood (`cat`
/// of a big log, a verbose build) then goes as a few events a frame instead of one per read of
/// the terminal (about 1 KB on macOS), which kept the window's one thread busy for every pane and
/// page. Output after a pause, like the echo of a key, goes at once.
const GATHER: Duration = Duration::from_millis(8);
/// The most output one event to the window carries.
const EVENT_BYTES: usize = 64 * 1024;
/// How long a tab just opened counts as one Claude may be starting in, before its processes can
/// tell (`claude_may_run_in`).
pub(super) const STARTING: Duration = Duration::from_secs(1);
/// An administrator's tab whose sudo ends with a failure this soon after it was opened is one
/// Windows wasn't given the permission for (`refused_exit`), whatever its code.
const REFUSED_WITHIN: Duration = Duration::from_secs(10);
/// What sudo ends with when its elevation was cancelled, the permission refused however long
/// Windows' dialog was read: `ERROR_CANCELLED`, or the same as an `HRESULT`.
const CANCELLED: [u32; 2] = [1223, 0x8007_04C7];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    pub id: u64,
    pub path: String,
    /// The tab's title; the user can rename it.
    pub name: String,
    /// `shell` | `claude`: a Claude Code session started from its own button.
    pub kind: String,
    /// The shell of the list a shell tab runs on Windows (`shells.rs`); none elsewhere and for a
    /// Claude tab.
    pub shell: Option<Shell>,
    /// An administrator's shell, through sudo (Windows): never put back by a record.
    pub elevated: bool,
    /// The user gave it its name: it no longer takes its session's (`name_terminals`).
    #[serde(skip)]
    pub renamed: bool,
    /// Opened by the core for a board card at its moment (`board_due`: its time, or its turn in
    /// the refill's line), not by the user: it doesn't take the keyboard. Set as the tab is made,
    /// so the first snapshot it is in says so.
    pub unasked: bool,
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
    /// The user's shell, or on Windows the shell of the list picked (none: Command Prompt);
    /// `elevated`, an administrator's, through sudo.
    Shell { shell: Option<Shell>, elevated: bool },
    /// Claude Code, then the shell: with a first prompt (a board card's text), with the model and
    /// in the mode of `start`, and with the folder of that card's images among Claude's
    /// directories.
    Claude { prompt: Option<String>, start: Start, images: Option<PathBuf> },
    /// Claude Code with a session that has ended opened again (`claude --resume`), then the
    /// shell: a board card's session, going on.
    Resume { session: String },
}

/// What a terminal's reader hands on to the thread that sends its output to the window.
enum Pty {
    Output(Vec<u8>),
    /// The shell ended, `failed` with a code other than 0 (`code`; none when it couldn't be read):
    /// what was read before goes first, then the tab.
    Exited { failed: bool, code: Option<u32> },
}

pub(super) struct Session {
    master: Box<dyn MasterPty + Send>,
    /// What the user types, for the tab's own writer thread: a write to the terminal blocks
    /// while its input queue is full (a long paste, a command that reads nothing), and must never
    /// do so under the terminals' lock, which the thread reading its output waits for, or on the
    /// caller's thread (the main one).
    input: mpsc::Sender<String>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    path: String,
    scrollback: String,
    /// Number of the last chunk sent to the window.
    seq: u64,
    /// When the tab was opened (`claude_may_run_in`, `refused`).
    opened: Instant,
    /// An administrator's shell, through sudo.
    elevated: bool,
}

impl Session {
    pub(super) fn pid_entry(&self) -> Option<PidEntry> {
        self.pid.map(|pid| PidEntry { path: self.path.clone(), pid })
    }

    /// Something other than the shell holds the terminal: a command it started is running.
    #[cfg(unix)]
    fn foreground(&self) -> bool {
        match (self.master.process_group_leader(), self.pid) {
            (Some(group), Some(shell)) => group > 0 && group as u32 != shell,
            _ => false,
        }
    }

    /// Windows has no foreground group; `restore.rs` looks at the shell's children instead.
    #[cfg(not(unix))]
    fn foreground(&self) -> bool {
        false
    }
}

/// A tab as a restart finds it (`restore.rs`).
pub(super) struct TabState {
    pub(super) id: u64,
    pub(super) pid: Option<u32>,
    pub(super) cols: u16,
    pub(super) rows: u16,
    /// A command runs in it, as far as its terminal tells.
    pub(super) foreground: bool,
    pub(super) scrollback: String,
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
        if claude {
            return self.open_claude_terminal(path, None, size);
        }
        self.open_terminal_as(path, &Launch::Shell { shell: None, elevated: false }, None, false, size, false)
    }

    /// A Claude Code tab in the project's folder ("New Claude"): with the model and in the mode
    /// of `start`, or, without one, as the project's own settings say.
    pub fn open_claude_terminal(self: &Arc<Self>, path: &str, start: Option<Start>, size: Option<(u16, u16)>) -> Result<TerminalView, String> {
        let start = start.unwrap_or_else(|| self.project_start(path));
        self.open_terminal_as(path, &Launch::Claude { prompt: None, start, images: None }, None, false, size, false)
    }

    /// How a Claude session of the project at `path` starts when nothing else was chosen: the
    /// model and the mode of the project's settings.
    pub(super) fn project_start(&self, path: &str) -> Start {
        self.lock().settings.project(path).start()
    }

    /// `open_terminal`, starting with `launch`; `name` is the tab's title instead of the
    /// numbered `claude` / shell name, and `renamed` says the user chose it (a tab put back
    /// after a restart). `unasked`: the core opens it for a board card at its moment, not the
    /// user (`TerminalView::unasked`).
    pub(super) fn open_terminal_as(
        self: &Arc<Self>,
        path: &str,
        launch: &Launch,
        name: Option<String>,
        renamed: bool,
        size: Option<(u16, u16)>,
        unasked: bool,
    ) -> Result<TerminalView, String> {
        // A folder that is gone would start the shell in the home folder instead, unasked.
        if !Path::new(path).is_dir() {
            return Err(t!("core.error.notFolder", path = path));
        }
        let claude = !matches!(launch, Launch::Shell { .. });
        let (shell, elevated) = match launch {
            Launch::Shell { shell, elevated } => (shells::on_this_system(*shell), *elevated),
            _ => (None, false),
        };
        let (cols, rows) = size.filter(|&(cols, rows)| cols > 0 && rows > 0).unwrap_or((80, 24));
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| error.to_string())?;

        let shell_path = CommandBuilder::new_default_prog().get_shell();
        let mut command = match launch {
            Launch::Claude { prompt, start, images } => claude_command(&shell_path, prompt.as_deref(), *start, images.as_deref()),
            Launch::Resume { session } => resume_command(&shell_path, session)?,
            Launch::Shell { .. } => shells::command(shell, elevated)?,
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
        let base = match shell {
            _ if claude => "claude".to_string(),
            Some(shell) => shell.name(),
            None => Path::new(&shell_path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "shell".into()),
        };
        let kind = if claude { "claude" } else { "shell" };

        let mut child = pair.slave.spawn_command(command).map_err(|error| error.to_string())?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|error| error.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|error| error.to_string())?;
        let (input, typed) = mpsc::channel::<String>();
        thread::spawn(move || {
            for data in typed {
                if writer.write_all(data.as_bytes()).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        });
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
                // A shell of the list goes by its own name; two of one are told apart where they
                // show (`distinctNames` in the window).
                None if same == 0 || shell.is_some() => base,
                None => format!("{base} {}", same + 1),
            };
            let view = TerminalView { id, path: path.to_string(), name, kind: kind.into(), shell, elevated, renamed, unasked };
            inner.terminal_list.push(view.clone());
            view
        };

        let session = Session { master: pair.master, input, killer, pid, path: path.to_string(), scrollback: String::new(), seq: 0, opened: Instant::now(), elevated };
        self.terminal_sessions().insert(view.id, session);
        self.record_pids();
        self.notify();

        let (output, read) = mpsc::channel::<Pty>();
        let exited = output.clone();
        thread::spawn(move || read_terminal(reader, output));

        let core = Arc::clone(self);
        let id = view.id;
        thread::spawn(move || core.stream_terminal(id, read));

        let core = Arc::clone(self);
        thread::spawn(move || {
            let status = child.wait().ok();
            let (failed, code) = (!status.as_ref().is_some_and(|status| status.success()), status.map(|status| status.exit_code()));
            // Through the thread that sends the output, after what the shell printed last.
            if exited.send(Pty::Exited { failed, code }).is_err() {
                core.forget_terminal(id);
            }
        });

        Ok(view)
    }

    /// Sends a terminal's output on to the window: output after a pause at once, the rest
    /// gathered for `GATHER` after the last. The tab goes once its shell has ended, after the
    /// output read before that, unless Windows refused it its permission (`refused`).
    fn stream_terminal(&self, id: u64, read: mpsc::Receiver<Pty>) {
        let mut pending: Vec<u8> = Vec::new();
        let mut sent: Option<Instant> = None;
        let mut exited: Option<(bool, Option<u32>)> = None;

        while exited.is_none() {
            let Ok(first) = read.recv() else {
                break;
            };
            if let Some(wait) = sent.and_then(|sent| GATHER.checked_sub(sent.elapsed())) {
                thread::sleep(wait);
            }
            for message in std::iter::once(first).chain(read.try_iter()) {
                match message {
                    Pty::Output(bytes) => pending.extend_from_slice(&bytes),
                    Pty::Exited { failed, code } => {
                        exited = Some((failed, code));
                        break;
                    }
                }
            }

            let data = take_text(&mut pending);
            if !data.is_empty() {
                self.send_terminal(id, &data);
                sent = Some(Instant::now());
            }
        }

        if let Some((failed, code)) = exited {
            if !self.refused(id, failed, code) {
                self.forget_terminal(id);
            }
        }
    }

    /// An administrator's tab whose sudo ended as one Windows refused the permission ends
    /// (`refused_exit`): no shell opened. The tab stays with the core's own line saying so, its
    /// process forgotten, until it is closed.
    fn refused(&self, id: u64, failed: bool, code: Option<u32>) -> bool {
        let refused = match self.terminal_sessions().get_mut(&id) {
            Some(session) if session.elevated && refused_exit(failed, code, session.opened.elapsed()) => {
                session.pid = None;
                true
            }
            _ => false,
        };
        if refused {
            self.send_terminal(id, &format!("\r\n\x1b[2m── {} ──\x1b[0m\r\n", t!("terminal.adminDenied")));
            self.record_pids();
        }
        refused
    }

    /// Output into the terminal's scrollback and on to the window, in events of at most
    /// `EVENT_BYTES`, each numbered after the last.
    fn send_terminal(&self, id: u64, data: &str) {
        let mut chunks = Vec::new();
        let mut rest = data;
        while !rest.is_empty() {
            let mut end = rest.len().min(EVENT_BYTES);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let (chunk, after) = rest.split_at(end);
            chunks.push(chunk);
            rest = after;
        }

        let first = match self.terminal_sessions().get_mut(&id) {
            Some(session) => {
                session.scrollback.push_str(data);
                trim_scrollback(&mut session.scrollback);
                let first = session.seq + 1;
                session.seq += chunks.len() as u64;
                first
            }
            None => return,
        };

        for (seq, chunk) in (first..).zip(chunks) {
            self.emit(CoreEvent::Terminal { id, seq, data: chunk.to_string() });
        }
    }

    /// A new title for a tab, from the user; an empty one keeps the old, and so does the one it
    /// has. The tab keeps that title from here on, whatever its session is called.
    pub fn rename_terminal(&self, id: u64, name: &str) {
        let name: String = name.trim().chars().take(TAB_NAME_MAX).collect();
        if name.is_empty() {
            return;
        }

        let renamed = {
            let mut inner = self.lock();
            match inner.terminal_list.iter_mut().find(|t| t.id == id && t.name != name) {
                Some(view) => {
                    view.name = name;
                    view.renamed = true;
                    true
                }
                None => false,
            }
        };

        if renamed {
            self.notify();
        }
    }

    /// Gives each Claude tab the name of the session that runs in it, as Claude Code shows it in
    /// `/resume`: once the session has one, and again when it changes (`/rename`, or another
    /// session in the tab). A tab the user renamed keeps their title, and a session without a
    /// name leaves the tab's as it is. Runs after every refresh of Claude's state
    /// (`refresh_claude`) and costs nothing while no tab is left to name.
    pub(super) fn name_terminals(&self) {
        let open: Vec<u64> = self.lock().terminal_list.iter().filter(|t| t.kind == "claude" && !t.renamed).map(|t| t.id).collect();
        if open.is_empty() {
            return;
        }
        // Before the watch is locked: it may read the process table.
        let Some(running) = self.running_cached() else {
            return;
        };

        // Each tab's session: the Claude nearest its shell (one that Claude started itself runs
        // further down), the one started last of two as near.
        let shells = self.terminal_shells();
        let mut sessions: HashMap<u64, (usize, u64, &String, &str)> = HashMap::new();
        for (session, run) in &running {
            let Some((depth, terminal, path)) =
                shells.iter().find_map(|(shell, terminal, path)| Some((run.lineage.iter().position(|pid| pid == shell)?, *terminal, path.as_str())))
            else {
                continue;
            };
            if !open.contains(&terminal) {
                continue;
            }
            let started = run.started_at.unwrap_or(0);
            let nearer = sessions.get(&terminal).is_none_or(|&(near, last, id, _)| (depth, std::cmp::Reverse(started), session) < (near, std::cmp::Reverse(last), id));
            if nearer {
                sessions.insert(terminal, (depth, started, session, run.cwd.as_deref().unwrap_or(path)));
            }
        }
        if sessions.is_empty() {
            return;
        }

        let names: Vec<(u64, String)> = match self.claude.lock() {
            Ok(mut watch) => sessions
                .iter()
                .filter_map(|(terminal, (_, _, session, folder))| Some((*terminal, tab_name(&watch.peek(session, folder)?.title?))))
                .filter(|(_, name)| !name.is_empty())
                .collect(),
            Err(_) => return,
        };

        let named = {
            let mut inner = self.lock();
            let mut named = false;
            for (terminal, name) in names {
                // Renamed by the user meanwhile: theirs stays.
                if let Some(view) = inner.terminal_list.iter_mut().find(|t| t.id == terminal && !t.renamed && t.name != name) {
                    view.name = name;
                    named = true;
                }
            }
            named
        };

        if named {
            self.notify();
        }
    }

    pub fn write_terminal(&self, id: u64, data: &str) {
        if let Some(session) = self.terminal_sessions().get(&id) {
            let _ = session.input.send(data.to_string());
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

    /// Whether Claude may still run in terminal `id`, one a session was opened again in: a
    /// process under its shell names `claude`, or on Unix the shell itself still does until
    /// Claude ends there and it becomes the plain login shell (`resume_command`; `cmd.exe` keeps
    /// `/K claude` in its line for good). A tab just opened, or one whose processes can't be told,
    /// may be such a tab too. Command lines are only tested, as `read_running` tests them.
    pub(super) fn claude_may_run_in(&self, id: u64) -> bool {
        let Some((shell, opened)) = self.terminal_sessions().get(&id).map(|session| (session.pid, session.opened)) else {
            return false;
        };
        if opened.elapsed() < STARTING {
            return true;
        }
        let (Some(shell), Some(table)) = (shell, process::process_table()) else {
            return true;
        };

        table.iter().any(|(&pid, process)| {
            let under = || if pid == shell { cfg!(unix) } else { super::sessions::lineage(&table, pid).contains(&shell) };
            process.line.to_lowercase().contains("claude") && under()
        })
    }

    /// Every terminal's shell, for shutdown.
    pub(super) fn terminal_pids(&self) -> Vec<u32> {
        self.terminal_sessions().values().filter_map(|session| session.pid).collect()
    }

    /// Quit: every tab's shell gets SIGHUP and hangs up its jobs, as a closed tab's does
    /// (`close_terminal`). A job sent to the background is a process group of its own, which
    /// ending the shell's group alone leaves running.
    #[cfg(unix)]
    pub(super) fn hang_up_terminals(&self) {
        for session in self.terminal_sessions().values_mut() {
            let _ = session.killer.kill();
        }
    }

    /// Every open tab's process, size and last output, for a restart (`restore.rs`).
    pub(super) fn terminal_tabs(&self) -> Vec<TabState> {
        self.terminal_sessions()
            .iter()
            .map(|(id, session)| {
                let size = session.master.get_size().ok();
                TabState {
                    id: *id,
                    pid: session.pid,
                    cols: size.as_ref().map_or(0, |size| size.cols),
                    rows: size.as_ref().map_or(0, |size| size.rows),
                    foreground: session.foreground(),
                    scrollback: session.scrollback.clone(),
                }
            })
            .collect()
    }

    /// Makes tab `id` an administrator's, as sudo would have opened it on Windows: the records'
    /// rules for one are tried on any system.
    #[cfg(all(test, unix))]
    pub(super) fn mark_elevated(&self, id: u64) {
        if let Some(view) = self.lock().terminal_list.iter_mut().find(|view| view.id == id) {
            view.elevated = true;
        }
        if let Some(session) = self.terminal_sessions().get_mut(&id) {
            session.elevated = true;
        }
    }

    /// Puts `text` before whatever a new tab has shown so far: the output of the tab it stands
    /// in for after a restart.
    pub(super) fn seed_terminal(&self, id: u64, text: &str) {
        if let Some(session) = self.terminal_sessions().get_mut(&id) {
            session.scrollback.insert_str(0, text);
            trim_scrollback(&mut session.scrollback);
        }
    }
}

/// The environment variable a first prompt travels in, from Pitwall to Claude's command line.
#[cfg(unix)]
const PROMPT_VAR: &str = "PITWALL_PROMPT";

/// The environment variable the folder of a card's images travels in, the same way.
#[cfg(unix)]
const IMAGES_VAR: &str = "PITWALL_IMAGES";

/// The user's shell, interactive and login (so PATH and version managers load), running
/// `claude` and then becoming an ordinary shell when Claude ends. `start` names Claude's model
/// and mode: its own words for them (`Start::line`: `--model sonnet --permission-mode plan`),
/// which are the only thing but the fixed script that is written into the command line.
///
/// A first prompt (a board card's text) is never put into the command line: it goes in
/// `PITWALL_PROMPT`, and the fixed script hands it to Claude as one quoted argument after `--`,
/// so no shell syntax in it runs and nothing in it reads as one of Claude's options. The shell
/// that stays after Claude is started without it.
///
/// `images`, the folder of the images that prompt names, becomes one of Claude's directories
/// (`--add-dir`): Claude reads a file outside the project only after asking, and a card's images
/// are outside it. The folder goes in `PITWALL_IMAGES`, never into the command line either.
#[cfg(unix)]
pub(super) fn claude_command(shell: &str, prompt: Option<&str>, start: Start, images: Option<&Path>) -> CommandBuilder {
    let quoted = shell.replace('\'', "'\\''");
    let mode = start.line();
    let images = images.filter(|_| prompt.is_some());
    let script = match (prompt, images) {
        (Some(_), Some(_)) => format!("claude{mode} --add-dir \"${IMAGES_VAR}\" -- \"${PROMPT_VAR}\"; exec env -u {PROMPT_VAR} -u {IMAGES_VAR} '{quoted}' -l"),
        (Some(_), None) => format!("claude{mode} -- \"${PROMPT_VAR}\"; exec env -u {PROMPT_VAR} '{quoted}' -l"),
        (None, _) => format!("claude{mode}; exec '{quoted}' -l"),
    };

    let mut command = CommandBuilder::new(shell);
    command.args(["-l", "-i", "-c", &script]);
    if let Some(prompt) = prompt {
        command.env(PROMPT_VAR, prompt);
    }
    if let Some(images) = images {
        command.env(IMAGES_VAR, images);
    }
    command
}

/// `cmd.exe /K` can't take arbitrary text safely: `%VAR%` expands before quoting is parsed, so
/// quotes, `&` or `|` in a card would end the argument and run as commands. Claude starts
/// without the card's text here (it stays unused) rather than risk that.
#[cfg(windows)]
pub(super) fn claude_command(_shell: &str, _prompt: Option<&str>, start: Start, _images: Option<&Path>) -> CommandBuilder {
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(windows_args(start));
    command
}

/// What `cmd.exe` is given on Windows: `/K claude`, then the model and the mode as arguments of
/// their own (`Start::args`), each a constant.
#[cfg(any(windows, test))]
fn windows_args(start: Start) -> Vec<&'static str> {
    let mut args = vec!["/K", "claude"];
    args.extend(start.args());
    args
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

/// Whether an administrator's tab whose sudo ended so, `age` after it was opened, was refused the
/// permission: sudo's code for a cancelled elevation at any age, or any failure within
/// `REFUSED_WITHIN` (another code, or one that couldn't be read).
fn refused_exit(failed: bool, code: Option<u32>, age: Duration) -> bool {
    code.is_some_and(|code| CANCELLED.contains(&code)) || (failed && age < REFUSED_WITHIN)
}

/// Hands a terminal's output on as it is read (`stream_terminal`), until the terminal closes.
/// Once its tab has gone nothing takes it, but it is read to the end all the same: a job left
/// running in it writes on.
fn read_terminal(mut reader: Box<dyn Read + Send>, output: mpsc::Sender<Pty>) {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let _ = output.send(Pty::Output(buffer[..read].to_vec()));
    }
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

/// Keeps the scrollback under its limit, cutting at a line start. It grows to half as long again
/// before it is cut back to the limit: cut back on every chunk, the whole of it moved each time.
fn trim_scrollback(scrollback: &mut String) {
    if scrollback.len() <= SCROLLBACK_BYTES + SCROLLBACK_BYTES / 2 {
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
    use crate::start::{Mode, Model};

    /// Every way a session starts, with what it must put on the command line, written out: on
    /// Unix as part of the shell's line, on Windows as arguments of `cmd.exe`.
    const STARTS: [(Option<Model>, Mode, &str, &[&str]); 15] = [
        (None, Mode::Normal, "", &[]),
        (None, Mode::Plan, " --permission-mode plan", &["--permission-mode", "plan"]),
        (None, Mode::AcceptEdits, " --permission-mode acceptEdits", &["--permission-mode", "acceptEdits"]),
        (Some(Model::Fable), Mode::Normal, " --model fable", &["--model", "fable"]),
        (Some(Model::Fable), Mode::Plan, " --model fable --permission-mode plan", &["--model", "fable", "--permission-mode", "plan"]),
        (Some(Model::Fable), Mode::AcceptEdits, " --model fable --permission-mode acceptEdits", &["--model", "fable", "--permission-mode", "acceptEdits"]),
        (Some(Model::Opus), Mode::Normal, " --model opus", &["--model", "opus"]),
        (Some(Model::Opus), Mode::Plan, " --model opus --permission-mode plan", &["--model", "opus", "--permission-mode", "plan"]),
        (Some(Model::Opus), Mode::AcceptEdits, " --model opus --permission-mode acceptEdits", &["--model", "opus", "--permission-mode", "acceptEdits"]),
        (Some(Model::Sonnet), Mode::Normal, " --model sonnet", &["--model", "sonnet"]),
        (Some(Model::Sonnet), Mode::Plan, " --model sonnet --permission-mode plan", &["--model", "sonnet", "--permission-mode", "plan"]),
        (Some(Model::Sonnet), Mode::AcceptEdits, " --model sonnet --permission-mode acceptEdits", &["--model", "sonnet", "--permission-mode", "acceptEdits"]),
        (Some(Model::Haiku), Mode::Normal, " --model haiku", &["--model", "haiku"]),
        (Some(Model::Haiku), Mode::Plan, " --model haiku --permission-mode plan", &["--model", "haiku", "--permission-mode", "plan"]),
        (Some(Model::Haiku), Mode::AcceptEdits, " --model haiku --permission-mode acceptEdits", &["--model", "haiku", "--permission-mode", "acceptEdits"]),
    ];

    fn script(command: &CommandBuilder) -> String {
        command.get_argv().last().unwrap().to_string_lossy().into_owned()
    }

    /// The model and the mode are the one thing of the user's choosing that is written into
    /// Claude's command line. In both forms of it (the script the Unix shell runs, the arguments
    /// `cmd.exe` gets on Windows) that is the fixed words of the two lists, before `--`, and
    /// nothing else: a choice named in any other words starts Claude as if none had been made.
    #[test]
    fn the_model_and_the_mode_are_on_the_command_line_in_their_own_words_only() {
        for (model, mode, line, args) in STARTS {
            let start = Start { model, mode };

            assert_eq!(script(&claude_command("/bin/zsh", None, start, None)), format!("claude{line}; exec '/bin/zsh' -l"));
            assert_eq!(script(&claude_command("/bin/zsh", Some("x"), start, None)), format!("claude{line} -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT '/bin/zsh' -l"));
            assert_eq!(
                script(&claude_command("/bin/zsh", Some("x"), start, Some(Path::new("/tmp/images")))),
                format!("claude{line} --add-dir \"$PITWALL_IMAGES\" -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT -u PITWALL_IMAGES '/bin/zsh' -l")
            );

            assert_eq!(windows_args(start), [&["/K", "claude"][..], args].concat());
        }

        // Named in other words (by the window, a settings file, the board's file): nothing of it
        // is on either command line.
        for (model, mode) in [
            ("opus; touch /tmp/pwned", "plan; touch /tmp/pwned"),
            ("sonnet --dangerously-skip-permissions", "bypassPermissions"),
            ("$(touch /tmp/pwned)", "`touch /tmp/pwned`"),
            ("claude-opus-5-5", "acceptEdits --dangerously-skip-permissions"),
            ("' -- ; touch /tmp/pwned #", "dontAsk"),
        ] {
            let said = Start::of(Some(model), Some(mode));
            let read: Start = serde_json::from_value(serde_json::json!({ "model": model, "mode": mode })).unwrap();
            for start in [said, read] {
                assert_eq!(script(&claude_command("/bin/zsh", None, start, None)), "claude; exec '/bin/zsh' -l", "{model} {mode}");
                assert_eq!(script(&claude_command("/bin/zsh", Some("x"), start, None)), "claude -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT '/bin/zsh' -l", "{model} {mode}");
                assert_eq!(windows_args(start), ["/K", "claude"], "{model} {mode}");
            }
        }
    }

    /// A card's text reaches Claude only through `PITWALL_PROMPT`: shell syntax in it never
    /// becomes part of the command line, whatever it holds.
    #[test]
    fn a_card_never_reaches_the_command_line() {
        let title = r#""; touch /tmp/pwned; echo ""#;
        let prompt = format!("{title}\n\n$(touch /tmp/pwned) `touch /tmp/pwned` ' --dangerously-skip-permissions");
        let argv = |command: &CommandBuilder| -> Vec<String> { command.get_argv().iter().map(|arg| arg.to_string_lossy().into_owned()).collect() };

        for (model, mode, _, _) in STARTS {
            let start = Start { model, mode };
            let command = claude_command("/bin/zsh", Some(&prompt), start, None);
            let args = argv(&command);
            for arg in &args {
                assert!(!arg.contains("pwned") && !arg.contains("dangerously"), "{arg}");
            }
            assert_eq!(command.get_env("PITWALL_PROMPT"), Some(std::ffi::OsStr::new(&prompt)));
            // The same command line for any text: it only names the variable.
            assert_eq!(args, argv(&claude_command("/bin/zsh", Some("x"), start, None)));

            let script = args.last().unwrap();
            assert!(script.contains(r#"-- "$PITWALL_PROMPT";"#), "{script}");
            // The shell that stays after Claude doesn't keep it.
            assert!(script.ends_with("exec env -u PITWALL_PROMPT '/bin/zsh' -l"), "{script}");

            // The folder of its images travels the same way, whatever its path holds.
            let folder = Path::new(r#"/Users/it's "me"; touch /tmp/pwned/board-images/a-0"#);
            let command = claude_command("/bin/zsh", Some(&prompt), start, Some(folder));
            let args = argv(&command);
            for arg in &args {
                assert!(!arg.contains("pwned") && !arg.contains("dangerously"), "{arg}");
            }
            assert_eq!(command.get_env("PITWALL_IMAGES"), Some(folder.as_os_str()));
            assert_eq!(command.get_env("PITWALL_PROMPT"), Some(std::ffi::OsStr::new(&prompt)));
            let script = args.last().unwrap();
            assert!(script.contains(r#" --add-dir "$PITWALL_IMAGES" -- "$PITWALL_PROMPT";"#), "{script}");
            assert!(script.ends_with("exec env -u PITWALL_PROMPT -u PITWALL_IMAGES '/bin/zsh' -l"), "{script}");
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
