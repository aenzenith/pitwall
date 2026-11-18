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
use crate::agent::Agent;
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
/// How long the output of a tab the panel doesn't show (`Shown`) gathers before it goes to the
/// window: a tab at work behind another then costs the window's one thread a few events a second
/// instead of one a frame. Only the event waits; the scrollback takes the output as it comes.
const HOLD: Duration = Duration::from_millis(250);
/// The most output held back for a tab not shown; more goes at once.
const HOLD_BYTES: usize = 4 * EVENT_BYTES;
/// How long a tab just opened counts as one its agent may be starting in, before its processes
/// can tell (`agent_may_run_in`).
pub(super) const STARTING: Duration = Duration::from_secs(1);
/// The variable that gives Codex its home folder: another Codex account's tab gets it, the main
/// account's never one of Pitwall's.
const CODEX_HOME: &str = "CODEX_HOME";
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
    /// `shell` | `claude` | `codex`: a session of that agent started from its own button
    /// (`Agent::tab_kind`); `chat`: a Claude chat drawn by the window (`chat.rs`).
    pub kind: String,
    /// A chat tab's session, state and title; none for every other tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<crate::chat::ChatView>,
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
    /// The account of its agent it runs with, by id; none: the main one. A Claude tab's and a
    /// shell tab's is a Claude account (`accounts/`), a Codex tab's a Codex account
    /// (`core/codex_accounts.rs`).
    pub account: Option<String>,
    /// The laps' own (a run's tab, or one a run's dialog opened its session again in): a layer of
    /// its own, as their sessions are. In no snapshot (no project's tabs), in no record of a quit
    /// or an update's restart, never brought up in the panel (`reveal_terminal`): it shows in its
    /// run's dialog alone (`Core::lap_terminal`). Set as the tab is made, so no snapshot ever has
    /// it.
    #[serde(skip)]
    pub apart: bool,
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
    /// directories. `session`: the id the new session takes (`--session-id`), one Pitwall made
    /// for a lap's run (`start::new_uuid`), so the run knows its session before it starts.
    Claude { prompt: Option<String>, start: Start, images: Option<PathBuf>, session: Option<String> },
    /// Claude Code with a session that has ended opened again (`claude --resume`), then the
    /// shell: a board card's session, going on.
    Resume { session: String },
    /// Codex, then the shell, as `Claude` is: with a first prompt, with the model and in the mode
    /// of `start`'s Codex part, and with the card's images (`codex_command`).
    Codex { prompt: Option<String>, start: Start, images: Option<PathBuf> },
    /// Codex with a session that has ended opened again (`codex resume`), then the shell.
    ResumeCodex { session: String },
}

impl Launch {
    /// The agent the tab starts; none for a shell.
    pub(super) fn agent(&self) -> Option<Agent> {
        match self {
            Self::Shell { .. } => None,
            Self::Claude { .. } | Self::Resume { .. } => Some(Agent::Claude),
            Self::Codex { .. } | Self::ResumeCodex { .. } => Some(Agent::Codex),
        }
    }
}

/// The panel's tabs (terminals and chats) whose content is on screen now, as each window whose
/// page hosts a panel last said (`Core::set_tabs_shown`, by the window's label): a tab is shown
/// while any of them shows it. Until one has said so once, every tab counts as shown. What a tab
/// that isn't shown says reaches the window whole and in order, only later (`HOLD`), and at once
/// when the tab is shown.
pub(super) struct Shown {
    ids: Mutex<Option<HashMap<String, HashSet<u64>>>>,
    /// `HOLD`, in milliseconds; a test sets its own.
    hold: std::sync::atomic::AtomicU64,
}

impl Default for Shown {
    fn default() -> Self {
        Self { ids: Mutex::new(None), hold: std::sync::atomic::AtomicU64::new(HOLD.as_millis() as u64) }
    }
}

impl Shown {
    fn ids(&self) -> MutexGuard<'_, Option<HashMap<String, HashSet<u64>>>> {
        self.ids.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether tab `id` is on screen, as far as the core was told.
    pub(super) fn has(&self, id: u64) -> bool {
        self.ids().as_ref().is_none_or(|windows| windows.values().any(|ids| ids.contains(&id)))
    }

    /// How long what a tab not shown says is held.
    pub(super) fn hold(&self) -> Duration {
        Duration::from_millis(self.hold.load(std::sync::atomic::Ordering::Relaxed))
    }

    #[cfg(all(test, unix))]
    pub(super) fn set_hold(&self, hold: Duration) {
        self.hold.store(hold.as_millis() as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What a terminal's reader hands on to the thread that sends its output to the window.
enum Pty {
    Output(Vec<u8>),
    /// The tab is shown now (`set_tabs_shown`): what was held for it goes at once.
    Shown,
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
    /// How much of the end of `scrollback` the window wasn't sent yet (a tab not shown).
    held: usize,
    /// To the thread that sends its output to the window (`Pty::Shown`).
    wake: mpsc::Sender<Pty>,
    /// When the tab was opened (`agent_may_run_in`, `refused`).
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
        self.open_claude_terminal_with(path, start, size, None, None)
    }

    /// `open_claude_terminal` with the account `account` names ("Start with account ›";
    /// `accounts::MAIN` for the main one); none: the one in use. `prompt`, the text typed into the
    /// empty panel's box, is Claude's first job (`first_prompt`), as a board card's text is.
    pub fn open_claude_terminal_with(self: &Arc<Self>, path: &str, start: Option<Start>, size: Option<(u16, u16)>, account: Option<&str>, prompt: Option<String>) -> Result<TerminalView, String> {
        let start = start.unwrap_or_else(|| self.lock().settings.project(path).start_with(Agent::Claude));
        self.open_terminal_with(path, &Launch::Claude { prompt: first_prompt(prompt), start, images: None, session: None }, None, false, size, false, account)
    }

    /// A Codex tab in the project's folder ("New Codex"), as `open_claude_terminal_with` is
    /// Claude's: with the model and in the mode of `start`'s Codex part, or, without one, as the
    /// project's own settings say for Codex; with the Codex account `account` names
    /// (`accounts::MAIN` for the main one), else the one in use; with `prompt` as its first job.
    pub fn open_codex_terminal_with(self: &Arc<Self>, path: &str, start: Option<Start>, size: Option<(u16, u16)>, account: Option<&str>, prompt: Option<String>) -> Result<TerminalView, String> {
        let start = start.unwrap_or_else(|| self.lock().settings.project(path).start_with(Agent::Codex));
        self.open_terminal_with(path, &Launch::Codex { prompt: first_prompt(prompt), start, images: None }, None, false, size, false, account)
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
        self.open_terminal_with(path, launch, name, renamed, size, unasked, None)
    }

    /// `open_terminal_as` with the account the tab runs with: the one `account` names
    /// (`accounts::MAIN` for the main one), else the one in use. Another account than the main
    /// one is given to the tab's shell, and so to Claude in it, as `CLAUDE_CONFIG_DIR`: only the
    /// folder of an account listed, as the settings hold it; the main one gets nothing of it.
    /// A Codex tab's account is a Codex account (`codex_start_with`), given as `CODEX_HOME` in
    /// the same way, and its Claude is the main one. Codex starts in no tab while it is off.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_terminal_with(
        self: &Arc<Self>,
        path: &str,
        launch: &Launch,
        name: Option<String>,
        renamed: bool,
        size: Option<(u16, u16)>,
        unasked: bool,
        account: Option<&str>,
    ) -> Result<TerminalView, String> {
        self.open_tab(path, launch, name, renamed, size, unasked, account, false)
    }

    /// A tab of the laps' (`TerminalView::apart`), named `name`: a run's agent, or a run's session
    /// opened again for its dialog; with the account `account` names, as `open_terminal_with`
    /// takes it.
    pub(super) fn open_terminal_apart(self: &Arc<Self>, path: &str, launch: &Launch, name: String, size: Option<(u16, u16)>, account: Option<&str>) -> Result<TerminalView, String> {
        self.open_tab(path, launch, Some(name), false, size, false, account, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn open_tab(
        self: &Arc<Self>,
        path: &str,
        launch: &Launch,
        name: Option<String>,
        renamed: bool,
        size: Option<(u16, u16)>,
        unasked: bool,
        account: Option<&str>,
        apart: bool,
    ) -> Result<TerminalView, String> {
        // A folder that is gone would start the shell in the home folder instead, unasked.
        if !Path::new(path).is_dir() {
            return Err(t!("core.error.notFolder", path = path));
        }
        let agent = launch.agent();
        let account = match agent {
            Some(Agent::Codex) => {
                if !self.agent_on(Agent::Codex) {
                    return Err(t!("terminal.codexOff"));
                }
                self.codex_start_with(account)?
            }
            _ => {
                if agent == Some(Agent::Claude) && !self.agent_on(Agent::Claude) {
                    return Err(t!("terminal.claudeOff"));
                }
                let inner = self.lock();
                crate::accounts::start_with(&inner.settings.accounts, account)?.map(|account| (account.id.clone(), PathBuf::from(&account.dir)))
            }
        };
        let claude = agent.is_some();
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
            Launch::Claude { prompt, start, images, session } => claude_command(&shell_path, prompt.as_deref(), *start, images.as_deref(), session.as_deref()),
            Launch::Resume { session } => resume_command(&shell_path, session)?,
            Launch::Codex { prompt, start, images } => codex_command(&shell_path, prompt.as_deref(), *start, images.as_deref())?,
            Launch::ResumeCodex { session } => codex_resume_command(&shell_path, session)?,
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
        // The main account is Claude Code without the variable, whatever the app was started with.
        // A Codex tab's account is Codex's; its main one is the home Pitwall follows as the main
        // (`Agent::home_in`: the app's own `CODEX_HOME`, else `~/.codex`), so it is left as is.
        match (agent, &account) {
            (Some(Agent::Codex), account) => {
                command.env_remove(crate::accounts::ENV);
                if let Some((_, home)) = account {
                    command.env(CODEX_HOME, home);
                }
            }
            (_, Some((_, dir))) => command.env(crate::accounts::ENV, dir),
            (_, None) => command.env_remove(crate::accounts::ENV),
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Pitwall");
        // Opened from Finder the app has no locale; without one the shell mangles UTF-8.
        if std::env::var_os("LANG").is_none() {
            command.env("LANG", "en_US.UTF-8");
        }
        let base = match shell {
            _ if claude => agent.map_or("claude", Agent::tab_kind).to_string(),
            Some(shell) => shell.name(),
            None => Path::new(&shell_path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "shell".into()),
        };
        let kind = agent.map_or("shell", Agent::tab_kind);

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
            // The laps' own tabs aren't in the panel to tell apart from.
            let same = inner.terminal_list.iter().filter(|t| t.path == path && t.kind == kind && !t.apart).count();
            let name = match name {
                Some(name) => name,
                // A shell of the list goes by its own name; two of one are told apart where they
                // show (`distinctNames` in the window).
                None if same == 0 || shell.is_some() => base,
                None => format!("{base} {}", same + 1),
            };
            let account = account.map(|(id, _)| id);
            let view = TerminalView { id, path: path.to_string(), name, kind: kind.into(), chat: None, shell, elevated, renamed, unasked, account, apart };
            inner.terminal_list.push(view.clone());
            view
        };

        let (output, read) = mpsc::channel::<Pty>();
        let exited = output.clone();
        let session = Session { master: pair.master, input, killer, pid, path: path.to_string(), scrollback: String::new(), seq: 0, held: 0, wake: output.clone(), opened: Instant::now(), elevated };
        self.terminal_sessions().insert(view.id, session);
        self.record_pids();
        self.notify();

        thread::spawn(move || read_terminal(reader, output));

        let core = Arc::clone(self);
        let id = view.id;
        thread::spawn(move || core.stream_terminal(id, apart, read));

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
    /// gathered for `GATHER` after the last. The scrollback takes it then, whatever the tab; the
    /// window is sent it then too while the tab is shown (`Shown`; the laps' own, `apart`,
    /// always), else once it has been held for `Shown::hold`, is `HOLD_BYTES` long, the tab is
    /// shown (`Pty::Shown`) or its shell ended. The tab goes once its shell has ended, after the
    /// output read before that, unless Windows refused it its permission (`refused`).
    fn stream_terminal(&self, id: u64, apart: bool, read: mpsc::Receiver<Pty>) {
        let mut pending: Vec<u8> = Vec::new();
        let mut sent: Option<Instant> = None;
        let mut exited: Option<(bool, Option<u32>)> = None;
        // What the scrollback took and the window wasn't sent yet, and since when.
        let mut held = String::new();
        let mut since: Option<Instant> = None;

        while exited.is_none() {
            let first = match since {
                None => match read.recv() {
                    Ok(first) => first,
                    Err(_) => break,
                },
                Some(since) => match read.recv_timeout((since + self.shown.hold()).saturating_duration_since(Instant::now())) {
                    Ok(first) => first,
                    Err(mpsc::RecvTimeoutError::Timeout) => Pty::Shown,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
            };
            let mut now = matches!(first, Pty::Shown);
            if !now {
                if let Some(wait) = sent.and_then(|sent| GATHER.checked_sub(sent.elapsed())) {
                    thread::sleep(wait);
                }
            }
            for message in std::iter::once(first).chain(read.try_iter()) {
                match message {
                    Pty::Output(bytes) => pending.extend_from_slice(&bytes),
                    Pty::Shown => now = true,
                    Pty::Exited { failed, code } => {
                        exited = Some((failed, code));
                        break;
                    }
                }
            }

            let data = take_text(&mut pending);
            if !data.is_empty() {
                self.keep_terminal(id, &data);
                held.push_str(&data);
                sent = Some(Instant::now());
                since = since.or(sent);
            }
            if now || exited.is_some() || apart || held.len() >= HOLD_BYTES || self.shown.has(id) {
                self.emit_terminal(id, &std::mem::take(&mut held));
                since = None;
            }
        }

        self.emit_terminal(id, &held);
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

    /// Output into the terminal's scrollback and on to the window.
    fn send_terminal(&self, id: u64, data: &str) {
        self.keep_terminal(id, data);
        self.emit_terminal(id, data);
    }

    /// Output into the terminal's scrollback, the window still to be sent it (`emit_terminal`).
    fn keep_terminal(&self, id: u64, data: &str) {
        if let Some(session) = self.terminal_sessions().get_mut(&id) {
            session.scrollback.push_str(data);
            trim_scrollback(&mut session.scrollback);
            // What the trim cut is the window's all the same: it is sent the whole of it.
            session.held = (session.held + data.len()).min(session.scrollback.len());
        }
    }

    /// Everything the scrollback took since the last time (`data`) on to the window, in events of
    /// at most `EVENT_BYTES`, each numbered after the last. Only the terminal's own thread calls
    /// it, so the events go in their order.
    fn emit_terminal(&self, id: u64, data: &str) {
        if data.is_empty() {
            return;
        }
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
                session.held = 0;
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

    /// The panel's tabs whose content is on screen now in `window` (its label), from its page,
    /// which hosts a panel: the whole of them, in the place of what that window said before;
    /// what another window shows stays shown. What is held for a tab no window showed goes to
    /// the window at once (its own thread sends it, woken here).
    pub fn set_tabs_shown(&self, window: &str, ids: Vec<u64>) {
        let ids: HashSet<u64> = ids.into_iter().collect();
        let new: Vec<u64> = {
            let mut shown = self.shown.ids();
            // Before anyone said anything every tab was shown: none is new.
            let new = shown.as_ref().map(|windows| ids.iter().filter(|id| !windows.values().any(|were| were.contains(id))).copied().collect()).unwrap_or_default();
            shown.get_or_insert_with(HashMap::new).insert(window.to_string(), ids);
            new
        };
        if new.is_empty() {
            return;
        }
        {
            let sessions = self.terminal_sessions();
            for session in new.iter().filter_map(|id| sessions.get(id)) {
                let _ = session.wake.send(Pty::Shown);
            }
        }
        self.chats_shown(&new);
    }

    /// The panel went to another window's page (to the terminals' own window, or back from it):
    /// the page it left says nothing more (`showTabs` in `lib/terminals.ts`), so what any window
    /// said is forgotten, and the page that has the panel now says what it shows as it mounts.
    /// Until then no tab counts as shown; what is held for one goes when that page names it.
    pub(super) fn tabs_host_changed(&self) {
        if let Some(windows) = self.shown.ids().as_mut() {
            windows.clear();
        }
    }

    /// `window` is gone (destroyed): what it showed is shown no more, unless another window
    /// shows it.
    pub fn tabs_window_gone(&self, window: &str) {
        if let Some(windows) = self.shown.ids().as_mut() {
            windows.remove(window);
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
    /// name leaves the tab's as it is, and so do the laps' own tabs (`apart`), which show in no
    /// panel. Runs after every refresh of Claude's state
    /// (`refresh_claude`) and costs nothing while no tab is left to name. Codex's tabs are named
    /// the same way first (`name_codex_terminals`).
    pub(super) fn name_terminals(&self) {
        self.name_codex_terminals();
        self.name_chats();
        let open: Vec<u64> = self.lock().terminal_list.iter().filter(|t| t.kind == "claude" && !t.renamed && !t.apart).map(|t| t.id).collect();
        if open.is_empty() {
            return;
        }
        // Before the watch is locked: it may read the process table.
        let Some(running) = self.running_cached() else {
            return;
        };

        let shells = self.terminal_shells();
        let sessions = tab_sessions(&running, &shells, &open);
        if sessions.is_empty() {
            return;
        }

        let names: Vec<(u64, String)> = match self.claude.lock() {
            Ok(mut watch) => sessions
                .iter()
                .filter_map(|(terminal, (session, folder))| Some((*terminal, tab_name(&watch.peek(session, folder)?.title?))))
                .filter(|(_, name)| !name.is_empty())
                .collect(),
            Err(_) => return,
        };
        self.give_names(names);
    }

    /// Gives each Codex tab the name of the session that runs in it, as `name_terminals` does
    /// Claude's: the name `/rename` gave it (`session_index.jsonl`), as Codex's watch holds it
    /// for the session (`SessionState::title`). Nothing while Codex is off or no Codex tab is
    /// left to name.
    fn name_codex_terminals(&self) {
        let open: Vec<u64> = self.lock().terminal_list.iter().filter(|t| t.kind == Agent::Codex.tab_kind() && !t.renamed && !t.apart).map(|t| t.id).collect();
        if open.is_empty() {
            return;
        }
        let Some(running) = self.codex_running() else {
            return;
        };
        let shells = self.terminal_shells();
        let sessions = tab_sessions(&running, &shells, &open);
        if sessions.is_empty() {
            return;
        }

        let names: Vec<(u64, String)> = {
            let inner = self.lock();
            sessions
                .iter()
                .filter_map(|(terminal, (session, _))| {
                    let title = inner.scanned.iter().find(|state| state.agent == Agent::Codex && &state.id == *session)?.title.as_deref()?;
                    Some((*terminal, tab_name(title)))
                })
                .filter(|(_, name)| !name.is_empty())
                .collect()
        };
        self.give_names(names);
    }

    /// Each tab of `names` takes its new title, unless the user renamed it meanwhile.
    pub(super) fn give_names(&self, names: Vec<(u64, String)>) {
        if names.is_empty() {
            return;
        }
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

    /// What the terminal has shown lately, to draw it again: up to the last chunk sent to the
    /// window, what is held after it (a tab not shown) coming as chunks of its own.
    pub fn terminal_buffer(&self, id: u64) -> TerminalBuffer {
        self.terminal_sessions()
            .get(&id)
            .map(|session| TerminalBuffer { data: session.scrollback[..session.scrollback.len() - session.held].to_string(), seq: session.seq })
            .unwrap_or(TerminalBuffer { data: String::new(), seq: 0 })
    }

    /// Closes a tab: the pseudo-terminal goes (the shell and its jobs get SIGHUP), then the
    /// shell's process group is ended for good.
    pub fn close_terminal(&self, id: u64) {
        if self.close_chat(id) {
            return;
        }
        let Some(mut session) = self.terminal_sessions().remove(&id) else {
            return;
        };

        let pid = session.pid;
        let _ = session.killer.kill();
        drop(session);
        self.tab_gone(pid);

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
        if let Some(session) = self.terminal_sessions().remove(&id) {
            self.tab_gone(session.pid);
        }

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
    /// A chat's Claude stands where a tab's shell does (`chat_shells`): it is its tab's process.
    pub(super) fn terminal_shells(&self) -> Vec<(u32, u64, String)> {
        let mut shells: Vec<(u32, u64, String)> = self.terminal_sessions().iter().filter_map(|(id, session)| Some((session.pid?, *id, session.path.clone()))).collect();
        shells.extend(self.chat_shells());
        shells
    }

    /// Whether its agent may still run in terminal `id`, one a session was opened again in: a
    /// process under its shell names the tab's agent (`claude`, or `codex` in a Codex tab), or on
    /// Unix the shell itself still does until the agent ends there and it becomes the plain login
    /// shell (`resume_command`, `codex_resume_command`; `cmd.exe` keeps `/K claude` in its line for
    /// good). A tab just opened, or one whose processes can't be told, may be such a tab too.
    /// Command lines are only tested, as `read_running` tests them. The process table is read
    /// afresh; a pass that asks of several tabs gives them one (`agent_may_run_with`).
    pub(super) fn agent_may_run_in(&self, id: u64) -> bool {
        self.agent_may_run_with(id, &mut None)
    }

    /// `agent_may_run_in` with the pass's process table, read here if the pass hasn't yet; a tab
    /// just opened or without a shell needs none.
    pub(super) fn agent_may_run_with(&self, id: u64, pass: &mut process::PassTable) -> bool {
        // A chat's Claude runs in it until its process has ended, the tab closed or not.
        if let Some(running) = self.chat_alive(id) {
            return running;
        }
        let agent = self.lock().terminal_list.iter().find(|t| t.id == id).and_then(|t| Agent::of_tab(&t.kind)).unwrap_or_default();
        let Some((shell, opened)) = self.terminal_sessions().get(&id).map(|session| (session.pid, session.opened)) else {
            return false;
        };
        if opened.elapsed() < STARTING {
            return true;
        }
        let Some(shell) = shell else {
            return true;
        };
        let Some(table) = process::pass_table(pass) else {
            return true;
        };

        let word = agent.tab_kind();
        table.iter().any(|(&pid, process)| {
            let under = || if pid == shell { cfg!(unix) } else { super::sessions::lineage(table, pid).contains(&shell) };
            process.line.to_lowercase().contains(word) && under()
        })
    }

    /// The process group of what runs in tab `id`'s foreground, when that isn't its shell: the
    /// agent a lap's run started there, which its timeout stops while the shell stays
    /// (`laps/run.rs`). Read from the terminal itself, as `Session::foreground` is.
    #[cfg(unix)]
    pub(super) fn terminal_foreground(&self, id: u64) -> Option<u32> {
        let sessions = self.terminal_sessions();
        let session = sessions.get(&id)?;
        let group = u32::try_from(session.master.process_group_leader()?).ok()?;
        (group > 1 && Some(group) != session.pid).then_some(group)
    }

    /// Something other than its shell runs in tab `id`: on Unix what holds its foreground
    /// (`terminal_foreground`); on Windows, which has no foreground group, a process its shell
    /// started, told from the pass's process table (read here if the pass hasn't yet; Unix needs
    /// none). A tab whose shell or processes can't be told holds something.
    pub(super) fn terminal_busy(&self, id: u64, pass: &mut process::PassTable) -> bool {
        #[cfg(unix)]
        {
            let _ = pass;
            self.terminal_foreground(id).is_some() || !self.terminal_sessions().get(&id).is_some_and(|session| session.pid.is_some())
        }
        #[cfg(not(unix))]
        {
            let Some(shell) = self.terminal_sessions().get(&id).and_then(|session| session.pid) else {
                return true;
            };
            process::pass_table(pass).is_none_or(|table| table.values().any(|process| process.parent == shell))
        }
    }

    /// Tab `id` is one of the laps' own (`TerminalView::apart`).
    pub(super) fn terminal_apart(&self, id: u64) -> bool {
        self.lock().terminal_list.iter().any(|view| view.id == id && view.apart)
    }

    /// Brings tab `id` of the project at `path` up in the window's terminal panel; never one of the
    /// laps' own (`apart`), which shows in its run's dialog alone: whatever finds a session running
    /// in one leaves it there. Whether it was brought up.
    pub(super) fn reveal_terminal(&self, path: String, id: u64) -> bool {
        if self.terminal_apart(id) {
            return false;
        }
        self.emit(CoreEvent::RevealTerminal { path, id });
        true
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
            session.held = session.held.min(session.scrollback.len());
        }
    }
}

/// The most of a prompt typed into the empty panel's box that reaches the agent: it travels to
/// the agent's process twice (in `PITWALL_PROMPT` and as its argument), and the system bounds the
/// two together (1 MB on macOS).
const FIRST_PROMPT_BYTES: usize = 100_000;

/// A prompt typed into the empty terminal panel's box as the agent is given it: without NUL
/// characters (no environment variable can hold one), trimmed and cut to `FIRST_PROMPT_BYTES` on
/// a character's boundary; none when nothing is left. A lone word gets a line break after it:
/// Claude reads a prompt that is one of its subcommands' names (`update`, `doctor`…) as that
/// subcommand even after `--` (see the board's), and only one that is the name whole.
fn first_prompt(text: Option<String>) -> Option<String> {
    let text: String = text?.chars().filter(|&c| c != '\0').collect();
    let mut text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    if text.len() > FIRST_PROMPT_BYTES {
        let mut end = FIRST_PROMPT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    if !text.contains(char::is_whitespace) {
        text.push('\n');
    }
    Some(text)
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
///
/// `session`, the id a lap's run gives its new session (`--session-id`), is written into the
/// line: only a session id (a UUID, `is_session_id`) gets there, anything else is left out.
#[cfg(unix)]
pub(super) fn claude_command(shell: &str, prompt: Option<&str>, start: Start, images: Option<&Path>, session: Option<&str>) -> CommandBuilder {
    let quoted = shell.replace('\'', "'\\''");
    let mode = format!("{}{}", start.line(), session_flag(session));
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
pub(super) fn claude_command(_shell: &str, _prompt: Option<&str>, start: Start, _images: Option<&Path>, session: Option<&str>) -> CommandBuilder {
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(windows_args(start));
    if let Some(session) = session.filter(|session| is_session_id(session)) {
        command.args(["--session-id", session]);
    }
    command
}

/// ` --session-id <id>` for a session id, nothing for anything else.
#[cfg(unix)]
fn session_flag(session: Option<&str>) -> String {
    session.filter(|session| is_session_id(session)).map(|session| format!(" --session-id {session}")).unwrap_or_default()
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

/// The user's shell running Codex and then becoming an ordinary shell, as `claude_command` does
/// for Claude: `codex --no-daemon` (its process stays the tab's own, under its shell), then the
/// model and the mode of `start.codex` (`CodexStart::line`), and a first prompt only through
/// `PITWALL_PROMPT` after `--`; the card's images' folder as `--add-dir "$PITWALL_IMAGES"`, as
/// Claude's. Every word on the line is a constant: never an option that skips Codex's approvals
/// or its sandbox.
#[cfg(unix)]
pub(super) fn codex_command(shell: &str, prompt: Option<&str>, start: Start, images: Option<&Path>) -> Result<CommandBuilder, String> {
    let quoted = shell.replace('\'', "'\\''");
    let mode = start.codex.line();
    let images = images.filter(|_| prompt.is_some());
    let script = match (prompt, images) {
        (Some(_), Some(_)) => format!("codex --no-daemon{mode} --add-dir \"${IMAGES_VAR}\" -- \"${PROMPT_VAR}\"; exec env -u {PROMPT_VAR} -u {IMAGES_VAR} '{quoted}' -l"),
        (Some(_), None) => format!("codex --no-daemon{mode} -- \"${PROMPT_VAR}\"; exec env -u {PROMPT_VAR} '{quoted}' -l"),
        (None, _) => format!("codex --no-daemon{mode}; exec '{quoted}' -l"),
    };

    let mut command = CommandBuilder::new(shell);
    command.args(["-l", "-i", "-c", &script]);
    if let Some(prompt) = prompt {
        command.env(PROMPT_VAR, prompt);
    }
    if let Some(images) = images {
        command.env(IMAGES_VAR, images);
    }
    Ok(command)
}

/// On Windows Codex starts without the card's text, as Claude does (`claude_command`).
#[cfg(windows)]
pub(super) fn codex_command(_shell: &str, _prompt: Option<&str>, start: Start, _images: Option<&Path>) -> Result<CommandBuilder, String> {
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(codex_windows_args(start));
    Ok(command)
}

/// What `cmd.exe` is given for Codex on Windows: `/K codex --no-daemon`, then the model and the
/// mode as arguments of their own (`CodexStart::args`), each a constant.
#[cfg(any(windows, test))]
fn codex_windows_args(start: Start) -> Vec<&'static str> {
    let mut args = vec!["/K", "codex", "--no-daemon"];
    args.extend(start.codex.args());
    args
}

/// The same shell opening a Codex session again (`codex resume --no-daemon <id>`), then becoming
/// an ordinary shell: only a session id gets into the line (`is_session_id`), never a session's
/// name, which `codex resume` would take too.
#[cfg(unix)]
pub(super) fn codex_resume_command(shell: &str, session: &str) -> Result<CommandBuilder, String> {
    if !is_session_id(session) {
        return Err(format!("not a session id: {session}"));
    }
    let quoted = shell.replace('\'', "'\\''");
    let script = format!("codex resume --no-daemon {session}; exec '{quoted}' -l");

    let mut command = CommandBuilder::new(shell);
    command.args(["-l", "-i", "-c", &script]);
    Ok(command)
}

#[cfg(windows)]
pub(super) fn codex_resume_command(_shell: &str, session: &str) -> Result<CommandBuilder, String> {
    if !is_session_id(session) {
        return Err(format!("not a session id: {session}"));
    }
    let mut command = CommandBuilder::new("cmd.exe");
    command.args(["/K", "codex", "resume", "--no-daemon", session]);
    Ok(command)
}

/// Each tab of `open`'s session among `running`: the one nearest its shell (one that the agent
/// started itself runs further down), the one started last of two as near; with the session's
/// folder, else the tab's project.
fn tab_sessions<'a>(running: &'a HashMap<String, super::sessions::Running>, shells: &'a [(u32, u64, String)], open: &[u64]) -> HashMap<u64, (&'a String, &'a str)> {
    let mut sessions: HashMap<u64, (usize, u64, &String, &str)> = HashMap::new();
    for (session, run) in running {
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
    sessions.into_iter().map(|(terminal, (_, _, session, folder))| (terminal, (session, folder))).collect()
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
    use crate::start::{CodexMode, CodexModel, CodexStart, Mode, Model};

    /// Every way a Claude session starts, with what it must put on the command line, written out:
    /// on Unix as part of the shell's line, on Windows as arguments of `cmd.exe`.
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

    /// Every way a Codex session starts, the same way, after `codex --no-daemon`.
    const CODEX_STARTS: [(Option<CodexModel>, CodexMode, &str, &[&str]); 10] = [
        (None, CodexMode::Normal, "", &[]),
        (None, CodexMode::ReadOnly, " -s read-only", &["-s", "read-only"]),
        (Some(CodexModel::Sol), CodexMode::Normal, " -m gpt-6.1-sol", &["-m", "gpt-6.1-sol"]),
        (Some(CodexModel::Sol), CodexMode::ReadOnly, " -m gpt-6.1-sol -s read-only", &["-m", "gpt-6.1-sol", "-s", "read-only"]),
        (Some(CodexModel::Terra), CodexMode::Normal, " -m gpt-6.1-terra", &["-m", "gpt-6.1-terra"]),
        (Some(CodexModel::Terra), CodexMode::ReadOnly, " -m gpt-6.1-terra -s read-only", &["-m", "gpt-6.1-terra", "-s", "read-only"]),
        (Some(CodexModel::Astra), CodexMode::Normal, " -m gpt-6-astra", &["-m", "gpt-6-astra"]),
        (Some(CodexModel::Astra), CodexMode::ReadOnly, " -m gpt-6-astra -s read-only", &["-m", "gpt-6-astra", "-s", "read-only"]),
        (Some(CodexModel::Luna), CodexMode::Normal, " -m gpt-6-luna", &["-m", "gpt-6-luna"]),
        (Some(CodexModel::Luna), CodexMode::ReadOnly, " -m gpt-6-luna -s read-only", &["-m", "gpt-6-luna", "-s", "read-only"]),
    ];

    /// Both agents' starts: the start, how its line begins, its own words for the model and the
    /// mode, and everything `cmd.exe` is given on Windows.
    fn every_start() -> Vec<(Start, &'static str, &'static str, Vec<&'static str>)> {
        let claude = STARTS.into_iter().map(|(model, mode, line, args)| (Start::claude(model, mode), "claude", line, [&["/K", "claude"][..], args].concat()));
        let codex = CODEX_STARTS.into_iter().map(|(model, mode, line, args)| {
            (Start { agent: Agent::Codex, codex: CodexStart { model, mode }, ..Start::default() }, "codex --no-daemon", line, [&["/K", "codex", "--no-daemon"][..], args].concat())
        });
        claude.chain(codex).collect()
    }

    /// The tab's command for `start`, as `open_terminal_with` makes it for its agent.
    fn command(start: Start, prompt: Option<&str>, images: Option<&Path>) -> CommandBuilder {
        match start.agent {
            Agent::Claude => claude_command("/bin/zsh", prompt, start, images, None),
            Agent::Codex => codex_command("/bin/zsh", prompt, start, images).unwrap(),
        }
    }

    fn windows(start: Start) -> Vec<&'static str> {
        match start.agent {
            Agent::Claude => windows_args(start),
            Agent::Codex => codex_windows_args(start),
        }
    }

    fn script(command: &CommandBuilder) -> String {
        command.get_argv().last().unwrap().to_string_lossy().into_owned()
    }

    /// The model and the mode are the one thing of the user's choosing that is written into an
    /// agent's command line. In both forms of it (the script the Unix shell runs, the arguments
    /// `cmd.exe` gets on Windows) that is the fixed words of the agent's two lists, before `--`,
    /// and nothing else: a choice named in any other words starts the agent as if none had been
    /// made, and Codex never with an option that skips its approvals or its sandbox.
    #[test]
    fn the_model_and_the_mode_are_on_the_command_line_in_their_own_words_only() {
        for (start, program, line, args) in every_start() {
            assert_eq!(script(&command(start, None, None)), format!("{program}{line}; exec '/bin/zsh' -l"));
            assert_eq!(script(&command(start, Some("x"), None)), format!("{program}{line} -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT '/bin/zsh' -l"));
            assert_eq!(
                script(&command(start, Some("x"), Some(Path::new("/tmp/images")))),
                format!("{program}{line} --add-dir \"$PITWALL_IMAGES\" -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT -u PITWALL_IMAGES '/bin/zsh' -l")
            );

            assert_eq!(windows(start), args);
        }

        // Named in other words (by the window, a settings file, the board's file): nothing of it
        // is on either command line.
        for (model, mode) in [
            ("opus; touch /tmp/pwned", "plan; touch /tmp/pwned"),
            ("sonnet --dangerously-skip-permissions", "bypassPermissions"),
            ("$(touch /tmp/pwned)", "`touch /tmp/pwned`"),
            ("claude-opus-5-5", "acceptEdits --dangerously-skip-permissions"),
            ("' -- ; touch /tmp/pwned #", "dontAsk"),
            ("gpt-6.1-sol; touch /tmp/pwned", "read-only; touch /tmp/pwned"),
            ("gpt-6.1-sol --dangerously-bypass-approvals-and-sandbox", "danger-full-access"),
            ("GPT-6-LUNA", "workspace-write"),
            ("gpt-5.5", "--yolo"),
            ("-c sandbox_mode=danger-full-access", "readOnly --full-auto"),
        ] {
            let said = Start::of(Some(model), Some(mode));
            let read: Start = serde_json::from_value(serde_json::json!({ "model": model, "mode": mode })).unwrap();
            for start in [said, read] {
                assert_eq!(script(&command(start, None, None)), "claude; exec '/bin/zsh' -l", "{model} {mode}");
                assert_eq!(script(&command(start, Some("x"), None)), "claude -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT '/bin/zsh' -l", "{model} {mode}");
                assert_eq!(windows(start), ["/K", "claude"], "{model} {mode}");
            }

            let said = Start::chosen(Some("codex"), Some(model), Some(mode));
            let read: Start = serde_json::from_value(serde_json::json!({ "agent": "codex", "codex": { "model": model, "mode": mode } })).unwrap();
            for start in [said, read] {
                assert_eq!(script(&command(start, None, None)), "codex --no-daemon; exec '/bin/zsh' -l", "{model} {mode}");
                assert_eq!(script(&command(start, Some("x"), None)), "codex --no-daemon -- \"$PITWALL_PROMPT\"; exec env -u PITWALL_PROMPT '/bin/zsh' -l", "{model} {mode}");
                assert_eq!(windows(start), ["/K", "codex", "--no-daemon"], "{model} {mode}");
            }
        }
    }

    /// A card's text reaches the agent only through `PITWALL_PROMPT`: shell syntax in it never
    /// becomes part of the command line, whatever it holds.
    #[test]
    fn a_card_never_reaches_the_command_line() {
        let title = r#""; touch /tmp/pwned; echo ""#;
        let prompt = format!("{title}\n\n$(touch /tmp/pwned) `touch /tmp/pwned` ' --dangerously-skip-permissions --yolo");
        let argv = |command: &CommandBuilder| -> Vec<String> { command.get_argv().iter().map(|arg| arg.to_string_lossy().into_owned()).collect() };

        for (start, _, _, _) in every_start() {
            let made = command(start, Some(&prompt), None);
            let args = argv(&made);
            for arg in &args {
                assert!(!arg.contains("pwned") && !arg.contains("dangerously") && !arg.contains("yolo"), "{arg}");
            }
            assert_eq!(made.get_env("PITWALL_PROMPT"), Some(std::ffi::OsStr::new(&prompt)));
            // The same command line for any text: it only names the variable.
            assert_eq!(args, argv(&command(start, Some("x"), None)));

            let script = args.last().unwrap();
            assert!(script.contains(r#"-- "$PITWALL_PROMPT";"#), "{script}");
            // The shell that stays after the agent doesn't keep it.
            assert!(script.ends_with("exec env -u PITWALL_PROMPT '/bin/zsh' -l"), "{script}");

            // The folder of its images travels the same way, whatever its path holds.
            let folder = Path::new(r#"/Users/it's "me"; touch /tmp/pwned/board-images/a-0"#);
            let made = command(start, Some(&prompt), Some(folder));
            let args = argv(&made);
            for arg in &args {
                assert!(!arg.contains("pwned") && !arg.contains("dangerously") && !arg.contains("yolo"), "{arg}");
            }
            assert_eq!(made.get_env("PITWALL_IMAGES"), Some(folder.as_os_str()));
            assert_eq!(made.get_env("PITWALL_PROMPT"), Some(std::ffi::OsStr::new(&prompt)));
            let script = args.last().unwrap();
            assert!(script.contains(r#" --add-dir "$PITWALL_IMAGES" -- "$PITWALL_PROMPT";"#), "{script}");
            assert!(script.ends_with("exec env -u PITWALL_PROMPT -u PITWALL_IMAGES '/bin/zsh' -l"), "{script}");
        }
    }

    /// A session opened again has its id written into the command line: nothing but a session
    /// id is ever written there, for either agent (never a Codex session's name, which
    /// `codex resume` would take too).
    #[test]
    fn only_a_session_id_reaches_the_resume_command() {
        let id = "9bb85e86-f618-4861-9858-03ec8fc36c28";
        assert_eq!(script(&resume_command("/bin/zsh", id).unwrap()), format!("claude --resume {id}; exec '/bin/zsh' -l"));
        assert_eq!(script(&codex_resume_command("/bin/zsh", id).unwrap()), format!("codex resume --no-daemon {id}; exec '/bin/zsh' -l"));

        for bad in ["", "x; touch /tmp/pwned", "$(touch /tmp/pwned)", "9bb85e86-f618-4861-9858-03ec8fc36c2;", "--dangerously-skip-permissions", "--last", "my session", "--yolo"] {
            assert!(resume_command("/bin/zsh", bad).is_err(), "{bad}");
            assert!(codex_resume_command("/bin/zsh", bad).is_err(), "{bad}");
        }
    }

    /// A prompt typed into the empty panel's box: no NUL reaches the variable, a blank one opens
    /// the agent with none, a long one is cut on a character's boundary, and a lone word is never
    /// the whole name of one of Claude's subcommands.
    #[test]
    fn a_first_prompt_is_cleaned() {
        assert_eq!(first_prompt(Some("  fix\0 the tests \n".into())), Some("fix the tests".into()));
        assert_eq!(first_prompt(None), None);
        for blank in ["", "  \n\t ", "\0\0", " \0 \n"] {
            assert_eq!(first_prompt(Some(blank.into())), None, "{blank:?}");
        }
        assert_eq!(first_prompt(Some(" update ".into())), Some("update\n".into()));

        // Three bytes, then two-byte letters: the bound falls inside one, which is left out.
        let cut = first_prompt(Some(format!("ab {}", "ğ".repeat(FIRST_PROMPT_BYTES)))).unwrap();
        assert_eq!(cut.len(), FIRST_PROMPT_BYTES - 1);
        assert!(cut.starts_with("ab ğ") && cut.ends_with('ğ'));
    }

    /// What the window would hear of terminals: each chunk's number and text, and a tab's end.
    #[derive(Debug, Clone, PartialEq)]
    enum Heard {
        Data(u64, String),
        Exit,
    }

    fn shell_core(tmp: &Path) -> (Arc<Core>, String, Arc<Mutex<Vec<Heard>>>) {
        let heard: Arc<Mutex<Vec<Heard>>> = Arc::default();
        let into = Arc::clone(&heard);
        let sink: Sink = Arc::new(move |event| match event {
            CoreEvent::Terminal { seq, data, .. } => into.lock().unwrap().push(Heard::Data(seq, data)),
            CoreEvent::TerminalExit { .. } => into.lock().unwrap().push(Heard::Exit),
            _ => {}
        });
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude").join("projects"),
                codex_home: tmp.join("codex"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            sink,
        );
        let project = tmp.join("paddock");
        fs::create_dir_all(&project).unwrap();
        (core, project.to_string_lossy().into_owned(), heard)
    }

    fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        done()
    }

    /// The text of every chunk heard, their numbers checked: each one after the last, none left
    /// out (a window drops a chunk numbered at or below the last it drew).
    fn text_of(heard: &Mutex<Vec<Heard>>) -> String {
        let heard = heard.lock().unwrap();
        let mut text = String::new();
        let mut last = 0;
        for each in heard.iter() {
            if let Heard::Data(seq, data) = each {
                assert_eq!(*seq, last + 1, "a chunk out of its place");
                last = *seq;
                text.push_str(data);
            }
        }
        text
    }

    fn scrollback_of(core: &Core, id: u64) -> String {
        core.terminal_tabs().into_iter().find(|tab| tab.id == id).map(|tab| tab.scrollback).unwrap_or_default()
    }

    /// The output of a tab the panel doesn't show is held back from the window, never lost: the
    /// window gets every byte the scrollback took, once and in order, at once when the tab is
    /// shown; a view drawn meanwhile gets the history up to the last chunk sent and the rest as
    /// chunks; and the tab's end comes after its last output. Before the panel said anything,
    /// output goes as it always did.
    #[test]
    fn a_tab_not_shown_loses_no_output_and_ends_after_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (core, project, heard) = shell_core(tmp.path());
        // Longer than the test: only being shown, or the end, sends what is held.
        core.shown.set_hold(Duration::from_secs(600));
        let id = core.open_terminal(&project, false, Some((80, 24))).unwrap().id;

        // Nothing said yet: every tab counts as shown.
        core.write_terminal(id, "printf 'one%s\\n' -$((40+2))\r");
        assert!(wait_for(Duration::from_secs(15), || text_of(&heard).contains("one-42")), "{:?}", heard.lock().unwrap());

        // Hidden: the scrollback takes the output, the window isn't sent it.
        core.set_tabs_shown("main", Vec::new());
        core.write_terminal(id, "printf 'two%s\\n' -$((40+3))\r");
        assert!(wait_for(Duration::from_secs(15), || scrollback_of(&core, id).contains("two-43")));
        thread::sleep(Duration::from_millis(200));
        let before = text_of(&heard);
        assert!(!before.contains("two-43"), "a hidden tab's output wasn't held");
        // A view drawn now: the history it reads and the chunks after it are the whole.
        let buffer = core.terminal_buffer(id);
        assert_eq!(buffer.seq, heard.lock().unwrap().iter().filter(|each| matches!(each, Heard::Data(..))).count() as u64);
        assert_eq!(buffer.data, before);

        // Shown: at once, whatever the hold.
        core.set_tabs_shown("main", vec![id]);
        assert!(wait_for(Duration::from_secs(2), || text_of(&heard).contains("two-43")), "being shown didn't send what was held");
        assert_eq!(text_of(&heard), scrollback_of(&core, id));

        // Hidden again, and the shell ends: its last output first, then its end.
        core.set_tabs_shown("main", Vec::new());
        core.write_terminal(id, "printf 'three%s\\n' -$((40+4)); exit\r");
        assert!(wait_for(Duration::from_secs(15), || heard.lock().unwrap().contains(&Heard::Exit)), "the tab never ended");
        assert!(text_of(&heard).contains("three-44"), "the end overtook the last output");
        assert_eq!(heard.lock().unwrap().last(), Some(&Heard::Exit));

        core.dispose();
    }

    /// How many events a tab at work costs the window, shown and not: a line every few
    /// milliseconds for two seconds, and a flood of 20 MB. Run by hand (`--ignored --nocapture`); it asserts only that
    /// nothing was lost.
    #[test]
    #[ignore]
    fn measure_the_events_of_a_busy_tab() {
        for (what, line) in [
            ("steady", "i=0; while [ $i -lt 400 ]; do echo \"line $i aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"; i=$((i+1)); sleep 0.005; done; printf 'done%s\\n' -$((1+1))\r"),
            ("flood", "yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 20000000; printf 'done%s\\n' -$((1+1))\r"),
        ] {
            for hidden in [false, true] {
                let tmp = tempfile::tempdir().unwrap();
                let (core, project, heard) = shell_core(tmp.path());
                let id = core.open_terminal(&project, false, Some((80, 24))).unwrap().id;
                assert!(wait_for(Duration::from_secs(15), || !scrollback_of(&core, id).is_empty()));
                thread::sleep(Duration::from_millis(500));
                if hidden {
                    core.set_tabs_shown("main", Vec::new());
                }
                let events = |heard: &Mutex<Vec<Heard>>| heard.lock().unwrap().len();
                let (first, began) = (events(&heard), Instant::now());
                core.write_terminal(id, line);
                let done = |heard: &Mutex<Vec<Heard>>| heard.lock().unwrap().iter().rev().take(8).any(|each| matches!(each, Heard::Data(_, data) if data.contains("done-2")));
                assert!(wait_for(Duration::from_secs(60), || done(&heard)), "{what}: {} events", events(&heard));
                text_of(&heard);
                let (took, count) = (began.elapsed(), events(&heard) - first);
                let bytes: usize = heard.lock().unwrap()[first..].iter().map(|each| if let Heard::Data(_, data) = each { data.len() } else { 0 }).sum();
                println!("{what} hidden={hidden}: {count} events, {bytes} bytes in {took:?} ({:.1} events/s)", count as f64 / took.as_secs_f64());
                core.dispose();
            }
        }
    }
}
