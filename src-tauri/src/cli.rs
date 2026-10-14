//! The command line, the terminal's end: `pitwall <subcommand>` is the app's own binary, run
//! through the link Settings › General puts on the shell's PATH. It answers and exits before
//! anything of the app starts and knows nothing itself: what it is asked goes to the running app
//! over the command line's own line (`core/cli`), and what comes back is printed.
//!
//! The line is of the radio's kind (`mcp.rs`): a port on 127.0.0.1 and two secrets, in a file
//! only the user can read (`~/.pitwall/cli.json`). Both ends show they can read it before
//! anything else is said. Without the file the line is closed; with it and nobody on the port,
//! Pitwall isn't running. It is no part of the radio: that one is Claude's, open only while the
//! radio is on, and bound to the folder its caller is in.
//!
//! What it ends with: 0 done, 1 refused or failed, 2 the command line itself was wrong, 3
//! Pitwall isn't running; stopped by ^C, what the shell gives a command ^C ended (`interrupt`).

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::i18n::{self, t};
use crate::mcp::{greet, read_line, send};
use crate::process;

/// On macOS only so far: none of it has run elsewhere. On Linux the package's own binary is
/// `pitwall` on the PATH already, and it is the app; on Windows the app has no console.
pub(crate) const SUPPORTED: bool = cfg!(target_os = "macos");
/// How the two ends talk, as the line's file and a hello say it (`v`). 2: `stop --command`, and
/// a run's caller that shuts its side of the line has left (^C), which stops what it started.
pub(crate) const LINE_VERSION: u32 = 2;
/// The line's file, in `~/.pitwall/`.
const LINE_FILE: &str = "cli.json";
/// An answer may take as long as what was asked for does: a command that runs, an output that
/// is followed until ^C.
const ANSWER_LIMIT: Duration = Duration::from_secs(60 * 60 * 24 * 365);

const DONE: i32 = 0;
const FAILED: i32 = 1;
const USAGE: i32 = 2;
const NOT_RUNNING: i32 = 3;

/// The first words that make a start of the binary a command and not the app.
const WORDS: [&str; 16] =
    ["status", "start", "stop", "restart", "logs", "run", "card", "open", "edit", "quiet", "help", "--help", "-h", "--version", "-V", "--json"];

/// The line as its file tells it: where the app listens, what each end shows the other, and the
/// language the app speaks (what this end says by itself is said in it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LineFile {
    /// `LINE_VERSION` as the app that wrote it has it; none counts as another.
    #[serde(default)]
    pub v: u32,
    pub port: u16,
    pub key: String,
    pub answer: String,
    #[serde(default)]
    pub lang: String,
}

impl LineFile {
    fn read(file: &Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
    }
}

/// The line's file in the registry folder `dir` (`~/.pitwall`).
pub(crate) fn line_file(dir: &Path) -> PathBuf {
    dir.join(LINE_FILE)
}

/// What is asked of the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "camelCase")]
pub(crate) enum Ask {
    Status,
    /// `start` | `stop` | `restart`
    Act { action: String },
    /// The dev server's output, or a saved command's by its name.
    Logs { command: Option<String>, follow: bool },
    /// A saved command, by its name. `confirmed`: the command line the user said yes to at a
    /// terminal, for one marked "ask first"; none while nobody was asked.
    Run { name: String, confirmed: Option<String> },
    /// A saved command stopped, by its name.
    StopCommand { name: String },
    Card { title: String, note: String },
    Open,
    Edit,
    /// `1h` | `morning` | `off`
    Quiet { span: String },
}

/// One call: what is asked, of which project (by name, or the one that holds `cwd`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Call {
    pub cwd: String,
    pub project: Option<String>,
    pub ask: Ask,
}

/// How a line of the app's words is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Tone {
    Plain,
    Soft,
    Good,
    Bad,
}

/// What Claude is at in a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeAt {
    /// `working` | `permission` | `asking` | `finished`
    pub state: String,
    /// A permission's tool, by its name.
    pub tool: Option<String>,
    /// Since when it waits (ms).
    pub since: Option<u64>,
}

/// A listed project, for `status`: as it stands, and the same in the app's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    pub name: String,
    /// `running` | `stopped` | `crashed` | `busy`
    pub status: String,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub started_at: Option<u64>,
    pub issue: Option<String>,
    pub claude: Option<ClaudeAt>,
    pub server_text: String,
    pub claude_text: String,
}

/// A line of the app's answer. `Done`, `Failed` and `Confirm` end it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Said {
    Rows { rows: Vec<Row> },
    /// Words of the app's own. `open`: the line isn't over, the next words go on in it.
    Text { text: String, tone: Tone, open: bool },
    /// A line of output, as it was printed.
    Line { text: String },
    /// Several of them at once, in their order: a busy output is said in few answers.
    Lines { lines: Vec<String> },
    /// The command is marked "ask first" and nobody said yes: nothing ran.
    Confirm { name: String, command: String },
    Done,
    /// Refused, or it failed: why, unless that was said already.
    Failed { text: String },
}

/* ---------- the command line itself ---------- */

/// What the words after `pitwall` ask for.
#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Help,
    Version,
    Call { project: Option<String>, ask: Ask, json: bool },
}

/// A subcommand as its usage line writes it.
fn synopsis(word: &str) -> String {
    let (project, name) = (t!("core.cli.arg.project"), t!("core.cli.arg.name"));
    match word {
        "status" => "pitwall [status] [--json]".into(),
        "start" | "stop" | "restart" => format!("pitwall start | stop | restart [{project}]"),
        "logs" => format!("pitwall logs [-f] [{project}] [--command <{name}>]"),
        "run" => format!("pitwall run <{name}> [{project}]"),
        "stopCommand" => format!("pitwall stop --command <{name}> [{project}]"),
        "card" => format!("pitwall card \"<{}>\" [{project}] [--note <{}>]", t!("core.cli.arg.title"), t!("core.cli.arg.text")),
        "open" => format!("pitwall open [{project}]"),
        "edit" => format!("pitwall edit [{project}]"),
        _ => "pitwall quiet <1h | morning | off>".into(),
    }
}

/// The words after a subcommand: those that stand alone, and the flags among them. A flag that
/// takes a value takes the next word; after `--` every word stands alone.
struct Words {
    alone: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

fn sort(words: &[String], switches: &[&str], valued: &[&str]) -> Option<Words> {
    let mut sorted = Words { alone: Vec::new(), flags: Vec::new() };
    let mut rest = words.iter();
    let mut plain = false;

    while let Some(word) = rest.next() {
        if plain || !word.starts_with('-') || word == "-" {
            sorted.alone.push(word.clone());
        } else if word == "--" {
            plain = true;
        } else if switches.contains(&word.as_str()) {
            sorted.flags.push((word.clone(), None));
        } else if valued.contains(&word.as_str()) {
            sorted.flags.push((word.clone(), Some(rest.next()?.clone())));
        } else {
            return None;
        }
    }
    Some(sorted)
}

/// What `words` ask for, or the usage line of what they got wrong.
fn parse(words: &[String]) -> Result<Parsed, String> {
    let Some(first) = words.first().map(String::as_str) else {
        return Ok(Parsed::Call { project: None, ask: Ask::Status, json: false });
    };
    let rest = &words[1..];
    let usage = |word: &str| t!("core.cli.usage", line = synopsis(word));
    let call = |project: Option<String>, ask: Ask| Ok(Parsed::Call { project, ask, json: false });
    // At most `most` words that stand alone, the last of them the project.
    let sorted = |switches: &[&str], valued: &[&str], most: usize| sort(rest, switches, valued).filter(|sorted| sorted.alone.len() <= most).ok_or_else(|| usage(first));

    match first {
        "help" | "--help" | "-h" => Ok(Parsed::Help),
        "--version" | "-V" => Ok(Parsed::Version),
        "status" | "--json" => {
            let sorted = sorted(&["--json"], &[], 0)?;
            Ok(Parsed::Call { project: None, ask: Ask::Status, json: first == "--json" || !sorted.flags.is_empty() })
        }
        // With `--command`, one of the project's saved commands rather than its dev server.
        "stop" => {
            let line = if rest.iter().any(|word| word == "--command" || word == "-c") { "stopCommand" } else { first };
            let mut sorted = sort(rest, &[], &["--command", "-c"]).filter(|sorted| sorted.alone.len() <= 1).ok_or_else(|| usage(line))?;
            let project = sorted.alone.pop();
            match sorted.flags.pop() {
                Some((_, Some(name))) => call(project, Ask::StopCommand { name }),
                _ => call(project, Ask::Act { action: first.to_string() }),
            }
        }
        "start" | "restart" => {
            let mut sorted = sorted(&[], &[], 1)?;
            call(sorted.alone.pop(), Ask::Act { action: first.to_string() })
        }
        "logs" => {
            let mut sorted = sorted(&["-f", "--follow"], &["--command", "-c"], 1)?;
            let follow = sorted.flags.iter().any(|(flag, _)| flag == "-f" || flag == "--follow");
            let command = sorted.flags.iter().rev().find_map(|(_, value)| value.clone());
            call(sorted.alone.pop(), Ask::Logs { command, follow })
        }
        "run" => {
            let sorted = sorted(&[], &[], 2)?;
            let mut alone = sorted.alone.into_iter();
            let name = alone.next().ok_or_else(|| usage(first))?;
            call(alone.next(), Ask::Run { name, confirmed: None })
        }
        "card" => {
            let sorted = sorted(&[], &["--note"], 2)?;
            let note = sorted.flags.iter().rev().find_map(|(_, value)| value.clone()).unwrap_or_default();
            let mut alone = sorted.alone.into_iter();
            let title = alone.next().ok_or_else(|| usage(first))?;
            call(alone.next(), Ask::Card { title, note })
        }
        "open" | "edit" => {
            let mut sorted = sorted(&[], &[], 1)?;
            call(sorted.alone.pop(), if first == "open" { Ask::Open } else { Ask::Edit })
        }
        "quiet" => match sorted(&[], &[], 1)?.alone.pop() {
            Some(span) if matches!(span.as_str(), "1h" | "morning" | "off") => call(None, Ask::Quiet { span }),
            _ => Err(usage(first)),
        },
        _ => Err(format!("{}\n{}", t!("core.cli.unknown", word = clean(first)), t!("core.cli.seeHelp"))),
    }
}

fn help() -> String {
    let lines = [
        ("status", "core.cli.help.status"),
        ("start", "core.cli.help.act"),
        ("logs", "core.cli.help.logs"),
        ("run", "core.cli.help.run"),
        ("stopCommand", "core.cli.help.stopCommand"),
        ("card", "core.cli.help.card"),
        ("open", "core.cli.help.open"),
        ("edit", "core.cli.help.edit"),
        ("quiet", "core.cli.help.quiet"),
    ];
    let mut text = String::new();
    for (word, key) in lines {
        text.push_str(&format!("  {}\n      {}\n", synopsis(word), i18n::text(key, &[])));
    }
    format!("{}\n\n{text}\n{}\n", t!("core.cli.help.about"), t!("core.cli.help.project"))
}

/* ---------- what is printed ---------- */

/// `text` without what would steer a terminal: no control character but the tab.
fn clean(text: &str) -> String {
    text.chars().filter(|c| *c == '\t' || !c.is_control()).collect()
}

/// Colour, on a terminal only and never with `NO_COLOR`.
#[derive(Clone, Copy)]
struct Paint {
    on: bool,
    /// The terminal has the 256 colours: Claude's orange is one of them.
    wide: bool,
}

impl Paint {
    fn of(terminal: bool) -> Self {
        let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        let term = std::env::var("TERM").unwrap_or_default();
        Self { on: terminal && !set("NO_COLOR") && term != "dumb", wide: term.contains("256") || set("COLORTERM") }
    }

    fn with(&self, code: &str, text: &str) -> String {
        if self.on && !text.is_empty() {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_string()
        }
    }

    fn tone(&self, tone: Tone, text: &str) -> String {
        match tone {
            Tone::Plain => text.to_string(),
            Tone::Soft => self.with("2", text),
            Tone::Good => self.with("32", text),
            Tone::Bad => self.with("31", text),
        }
    }

    fn claude(&self, text: &str) -> String {
        self.with(if self.wide { "38;5;209" } else { "33" }, text)
    }
}

/// How many of a terminal's cells `text` takes: two for a character of the wide East Asian
/// scripts (kana, han, hangul, the full-width forms), none for one that joins the one before
/// it, one for every other.
fn cells(text: &str) -> usize {
    let wide = |c: u32| {
        matches!(c, 0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
    };
    let joins = |c: u32| matches!(c, 0x0300..=0x036F | 0x200B..=0x200F | 0x3099..=0x309A | 0xFE00..=0xFE0F);
    text.chars().map(|c| c as u32).map(|c| if joins(c) { 0 } else if wide(c) { 2 } else { 1 }).sum()
}

/// The projects, a line each: a mark, the name, the server, and what Claude is at. A column is
/// as wide as what stands in it and has something after it: a line ends where its words do.
fn table(rows: &[Row], paint: Paint) -> String {
    let width = cells;
    let gap = |text: &str, column: usize| " ".repeat(column.saturating_sub(width(text)) + 2);
    let names = rows.iter().map(|row| width(&row.name)).max().unwrap_or(0);
    let servers = rows.iter().filter(|row| !row.claude_text.is_empty()).map(|row| width(&row.server_text)).max().unwrap_or(0);

    let mut text = String::new();
    for row in rows {
        let (name, server, claude) = (clean(&row.name), clean(&row.server_text), clean(&row.claude_text));
        let mark = match row.status.as_str() {
            "running" => paint.with("32", "●"),
            "crashed" => paint.with("31", "✕"),
            _ => paint.with("2", "○"),
        };
        text.push_str(&format!("{mark} {name}{}{}", gap(&name, names), paint.with("2", &server)));
        if !claude.is_empty() {
            text.push_str(&format!("{}{}", gap(&server, servers), paint.claude(&claude)));
        }
        text.push('\n');
    }
    text
}

/// The same as JSON: what stands, not the words for it.
fn rows_json(rows: &[Row]) -> String {
    let rows: Vec<_> = rows
        .iter()
        .map(|row| json!({ "name": row.name, "status": row.status, "port": row.port, "url": row.url, "startedAt": row.started_at, "issue": row.issue, "claude": row.claude }))
        .collect();
    serde_json::to_string_pretty(&rows).unwrap_or_default()
}

/* ---------- the line ---------- */

/// Why a call got no answer.
#[derive(Debug, PartialEq, Eq)]
enum Lost {
    /// No file: the line is closed, or Pitwall never ran.
    Closed,
    /// The file, and nobody on its port: Pitwall isn't running.
    Unreached,
    /// Somebody on the file's port who didn't answer as Pitwall does: Pitwall with no place
    /// free for one more caller, or another program on a port Pitwall once had.
    Unwelcomed,
    /// The app that wrote the file is of another version than this command.
    OtherVersion,
    /// The call left, and the answer broke off.
    Quiet,
}

/// Passes one call to the app and hands each line of its answer to `each`, until the line that
/// ends it, or until `each` wants no more (false).
fn talk(file: Option<&LineFile>, call: &Call, mut each: impl FnMut(&Said) -> bool) -> Result<Said, Lost> {
    let file = file.ok_or(Lost::Closed)?;
    if file.v != LINE_VERSION {
        return Err(Lost::OtherVersion);
    }

    // Nothing listens only when the port refuses: whatever goes wrong after it took the caller
    // in (no welcome, another's, silence) is somebody's doing.
    let lost = |error: std::io::Error| if error.kind() == std::io::ErrorKind::ConnectionRefused { Lost::Unreached } else { Lost::Unwelcomed };
    let (mut reader, mut writer) = greet(file.port, LINE_VERSION, &file.key, &file.answer).map_err(lost)?;
    send(&mut writer, call).map_err(|_| Lost::Quiet)?;
    // While a run is answered, ^C stops what it started (`interrupt`); disarmed before the line
    // is closed.
    let _armed = matches!(call.ask, Ask::Run { .. }).then(|| interrupt::arm(&writer));
    reader.get_mut().extend(ANSWER_LIMIT);

    loop {
        let line = read_line(&mut reader).ok().flatten().ok_or(Lost::Quiet)?;
        let said: Said = serde_json::from_str(&line).map_err(|_| Lost::Quiet)?;
        if matches!(said, Said::Done | Said::Failed { .. } | Said::Confirm { .. }) {
            return Ok(said);
        }
        if !each(&said) {
            return Ok(Said::Done);
        }
    }
}

/// ^C while `run` follows what it started. The first one stops the command too, as it would one
/// run in the foreground: this end shuts its side of the line, which the app takes for its
/// caller having left (`core/cli`: a run's caller that leaves takes what it started with it),
/// and what the app says until the command's end is still printed. A second one ends this
/// command at once, as ^C always does; the stop asked for goes on in Pitwall. Either way it ends
/// as a command ^C stopped does. Armed for a run's answer only: everywhere else ^C ends the
/// command, and with it nothing but the following (`logs -f`) or the waiting.
mod interrupt {
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicBool, Ordering};

    static PRESSED: AtomicBool = AtomicBool::new(false);

    /// Whether ^C came while a run was answered.
    pub(super) fn pressed() -> bool {
        PRESSED.load(Ordering::SeqCst)
    }

    /// The handler, until this is dropped.
    pub(super) struct Armed(());

    impl Drop for Armed {
        fn drop(&mut self) {
            system::disarm();
        }
    }

    /// From now until the answer to the call sent on `line` ends.
    pub(super) fn arm(line: &TcpStream) -> Armed {
        system::arm(line);
        Armed(())
    }

    /// What the command ends with, once it was stopped by ^C. On Unix it is ended by the signal
    /// itself, as a shell expects of a command that ^C stopped (a script stops there too).
    pub(super) fn leave() -> i32 {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        system::leave()
    }

    #[cfg(unix)]
    mod system {
        use std::io::IsTerminal;
        use std::net::TcpStream;
        use std::os::fd::AsRawFd;
        use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

        use super::PRESSED;

        /// The line's socket while armed, -1 while not.
        static LINE: AtomicI32 = AtomicI32::new(-1);
        /// Errors go to a terminal: the line the `^C` it shows is on is ended.
        static TERMINAL: AtomicBool = AtomicBool::new(false);

        /// Only what a signal handler may do: stores, a write and a shutdown.
        extern "C" fn on_interrupt(_: libc::c_int) {
            PRESSED.store(true, Ordering::SeqCst);
            if TERMINAL.load(Ordering::SeqCst) {
                // SAFETY: a write of a static byte to a descriptor; signal-safe.
                unsafe { libc::write(libc::STDERR_FILENO, b"\n".as_ptr().cast(), 1) };
            }
            let line = LINE.load(Ordering::SeqCst);
            if line >= 0 {
                // SAFETY: the socket is open while armed (`Armed` goes before the line);
                // signal-safe.
                unsafe { libc::shutdown(line, libc::SHUT_WR) };
            }
        }

        pub(super) fn arm(line: &TcpStream) {
            // SAFETY: plain sigaction calls with zeroed, then filled structures.
            unsafe {
                // ^C ignored (a job a script sent to the background): it stays ignored.
                let mut was: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(libc::SIGINT, std::ptr::null(), &mut was) != 0 || was.sa_sigaction == libc::SIG_IGN {
                    return;
                }
                TERMINAL.store(std::io::stderr().is_terminal(), Ordering::SeqCst);
                LINE.store(line.as_raw_fd(), Ordering::SeqCst);

                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_interrupt as extern "C" fn(libc::c_int) as libc::sighandler_t;
                // Once only: a second ^C finds the system's own again, which ends the command.
                action.sa_flags = libc::SA_RESTART | libc::SA_RESETHAND;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
            }
        }

        pub(super) fn disarm() {
            if LINE.swap(-1, Ordering::SeqCst) >= 0 {
                // SAFETY: puts back the system's own handling.
                unsafe { libc::signal(libc::SIGINT, libc::SIG_DFL) };
            }
        }

        pub(super) fn leave() -> i32 {
            // SAFETY: the system's own handling, then the signal: the process ends by it.
            unsafe {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::raise(libc::SIGINT);
            }
            // Still here (the signal is blocked): what a shell says of a command ^C ended.
            130
        }
    }

    #[cfg(windows)]
    mod system {
        use std::io::{IsTerminal, Write};
        use std::mem::ManuallyDrop;
        use std::net::{Shutdown, TcpStream};
        use std::os::windows::io::{AsRawSocket, FromRawSocket, RawSocket};
        use std::sync::atomic::Ordering;
        use std::sync::{Mutex, PoisonError};

        use windows_sys::core::BOOL;
        use windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;
        use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT};

        use super::PRESSED;

        /// The line's socket while armed. Windows calls the handler on a thread of its own:
        /// under this lock the socket is never closed while it is being shut.
        static LINE: Mutex<Option<RawSocket>> = Mutex::new(None);

        unsafe extern "system" fn on_interrupt(kind: u32) -> BOOL {
            // The console closed, a logoff, a second ^C: Windows's own, which ends the command.
            if (kind != CTRL_C_EVENT && kind != CTRL_BREAK_EVENT) || PRESSED.swap(true, Ordering::SeqCst) {
                return 0;
            }
            if std::io::stderr().is_terminal() {
                let _ = std::io::stderr().write_all(b"\n");
            }
            let line = LINE.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(socket) = *line {
                // SAFETY: the socket is open while it is held here; borrowed, never closed.
                let stream = ManuallyDrop::new(unsafe { TcpStream::from_raw_socket(socket) });
                let _ = stream.shutdown(Shutdown::Write);
            }
            1
        }

        pub(super) fn arm(line: &TcpStream) {
            *LINE.lock().unwrap_or_else(PoisonError::into_inner) = Some(line.as_raw_socket());
            // SAFETY: a handler of the right signature, added.
            unsafe { SetConsoleCtrlHandler(Some(on_interrupt), 1) };
        }

        pub(super) fn disarm() {
            // SAFETY: the same handler, taken away.
            unsafe { SetConsoleCtrlHandler(Some(on_interrupt), 0) };
            *LINE.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }

        /// What a command ^C ended ends with on Windows.
        pub(super) fn leave() -> i32 {
            STATUS_CONTROL_C_EXIT
        }
    }
}

/// Whether a command marked "ask first" may run: only when the user says yes at a terminal.
/// Without one nobody is asked, and it doesn't.
fn agreed(terminal: bool, answer: impl FnOnce() -> String) -> bool {
    terminal && matches!(answer().trim().to_lowercase().as_str(), "y" | "yes")
}

/// One call, printed as it is answered; what the command ends with.
fn run(file: Option<&LineFile>, project: Option<String>, ask: Ask, json: bool) -> i32 {
    let out = std::io::stdout();
    let paint = Paint::of(out.is_terminal());
    let cwd = std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
    // An output that is followed has no end of its own: it ends with whoever reads it.
    let follows = matches!(ask, Ask::Logs { follow: true, .. });
    let mut call = Call { cwd, project, ask };

    loop {
        // A pipe that closed (`| head`) ends the printing, not the command: a run still goes to
        // its end, for what it ends with. Only a follow ends there, having nothing else to wait
        // for.
        let print = |said: &Said| {
            let mut out = out.lock();
            let written = match said {
                Said::Rows { rows } if json => writeln!(out, "{}", rows_json(rows)),
                Said::Rows { rows } if rows.is_empty() => writeln!(out, "{}", t!("core.cli.project.empty")),
                Said::Rows { rows } => write!(out, "{}", table(rows, paint)),
                Said::Text { text, tone, open } => write!(out, "{}{}", paint.tone(*tone, &clean(text)), if *open { "" } else { "\n" }),
                Said::Line { text } => writeln!(out, "{}", clean(text)),
                Said::Lines { lines } => lines.iter().try_for_each(|line| writeln!(out, "{}", clean(line))),
                _ => Ok(()),
            };
            let written = written.and_then(|()| out.flush());
            !(follows && written.is_err())
        };

        let answer = talk(file, &call, print);
        // ^C while a run was answered: what it started was stopped, and what came of it said.
        // It ends as a command ^C stopped does, whatever that was.
        if interrupt::pressed() {
            if let Ok(Said::Failed { text }) = &answer {
                if !text.is_empty() {
                    eprintln!("{}", clean(text));
                }
            }
            return interrupt::leave();
        }

        return match answer {
            Ok(Said::Failed { text }) => {
                if !text.is_empty() {
                    eprintln!("{}", clean(&text));
                }
                FAILED
            }
            Ok(Said::Confirm { name, command }) => {
                let terminal = std::io::stdin().is_terminal();
                let asked = || {
                    eprint!("{}\n{} ", t!("core.cli.run.confirm", name = clean(&name), command = clean(&command)), t!("core.cli.run.yesNo"));
                    let mut answer = String::new();
                    let _ = std::io::stdin().read_line(&mut answer);
                    answer
                };
                if !agreed(terminal, asked) {
                    if !terminal {
                        eprintln!("{}", t!("core.cli.run.askFirst", name = clean(&name)));
                    }
                    return FAILED;
                }
                // The yes is for the line that was shown: one that changed meanwhile is asked
                // about again.
                if let Ask::Run { confirmed, .. } = &mut call.ask {
                    *confirmed = Some(command);
                }
                continue;
            }
            Ok(_) => DONE,
            Err(Lost::Closed) => {
                eprintln!("{}", t!("core.cli.closed"));
                NOT_RUNNING
            }
            Err(Lost::Unreached) => {
                eprintln!("{}", t!("core.cli.notRunning"));
                NOT_RUNNING
            }
            Err(Lost::Unwelcomed) => {
                eprintln!("{}", t!("core.cli.noWelcome"));
                FAILED
            }
            Err(Lost::OtherVersion) => {
                eprintln!("{}", t!("core.cli.otherVersion"));
                FAILED
            }
            Err(Lost::Quiet) => {
                eprintln!("{}", t!("core.cli.wentQuiet"));
                FAILED
            }
        };
    }
}

/// Started through the link the app installs: macOS names the path a process was started by,
/// and that is the link.
fn through_link() -> bool {
    std::env::current_exe().and_then(std::fs::symlink_metadata).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Why a start of the command asks the app nothing.
#[derive(Debug, PartialEq, Eq)]
enum Unasked {
    /// Its words were wrong: the usage line of what they got wrong.
    Usage(String),
    /// It was started from inside a Claude Code session.
    InsideClaude,
}

/// What a start of the command is to do: what its `words` ask for, unless it was started from
/// inside a Claude Code session (one of that session's variables among `vars`, the variables
/// the process was started with; an empty one isn't set). From there the command asks the app
/// nothing: Claude's way to Pitwall is the team radio, with its switch per tool and its one
/// project. What the command says of itself (its help, its version) it still says. A guard, not
/// a wall: whatever runs as the user can read the line's file.
fn asked(words: &[String], vars: impl Iterator<Item = (OsString, OsString)>) -> Result<Parsed, Unasked> {
    let parsed = parse(words).map_err(Unasked::Usage)?;
    let names = vars.filter(|(_, value)| !value.is_empty()).map(|(name, _)| name.to_string_lossy().into_owned());
    if matches!(parsed, Parsed::Call { .. }) && process::in_claude_session(names) {
        return Err(Unasked::InsideClaude);
    }
    Ok(parsed)
}

/// Command mode: when the binary was started as the `pitwall` command (through its link, or
/// with one of its subcommands), answers and gives what the process is to end with, before
/// anything of the app starts. `None`: it is the app that was started.
pub fn cli_mode(mut args: impl Iterator<Item = OsString>) -> Option<i32> {
    if !SUPPORTED {
        return None;
    }
    let _program = args.next();
    let words: Vec<String> = args.map(|word| word.to_string_lossy().into_owned()).collect();
    if !words.first().is_some_and(|first| WORDS.contains(&first.as_str())) && !through_link() {
        return None;
    }

    let file = dirs::home_dir().and_then(|home| LineFile::read(&line_file(&home.join(".pitwall"))));
    // In the app's language, as it last said; the system's while it never has.
    i18n::set(i18n::resolve(file.as_ref().map_or("", |file| file.lang.as_str())));

    Some(match asked(&words, std::env::vars_os()) {
        Ok(Parsed::Help) => {
            print!("{}", help());
            DONE
        }
        Ok(Parsed::Version) => {
            println!("pitwall {}", env!("CARGO_PKG_VERSION"));
            DONE
        }
        Ok(Parsed::Call { project, ask, json }) => run(file.as_ref(), project, ask, json),
        Err(Unasked::InsideClaude) => {
            eprintln!("{}", t!("core.cli.insideClaude"));
            FAILED
        }
        Err(Unasked::Usage(usage)) => {
            eprintln!("{usage}");
            USAGE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    /// A command marked "ask first" runs only on a yes typed at a terminal. Without a terminal
    /// (a script, Claude's own shell) nobody is asked, whatever stdin holds, and anything but a
    /// yes is a no.
    #[test]
    fn ask_first_takes_a_yes_at_a_terminal() {
        let never = || -> String { panic!("nobody is asked without a terminal") };
        assert!(!agreed(false, never));

        for (answer, yes) in [("y\n", true), ("Y\n", true), (" yes \n", true), ("\n", false), ("n\n", false), ("yep\n", false), ("", false)] {
            assert_eq!(agreed(true, || answer.to_string()), yes, "{answer:?}");
        }
        // What the command line itself says is never a yes: only the answer to the question is.
        assert_eq!(parse(&words("run migrate")), Ok(Parsed::Call { project: None, ask: Ask::Run { name: "migrate".into(), confirmed: None }, json: false }));
        assert!(parse(&words("run migrate --yes")).is_err());
        assert!(parse(&words("run migrate web --confirmed")).is_err());
    }

    /// Run from a Claude Code session's shell, no call reaches the app, whatever it asks for:
    /// the radio's switches would otherwise bind nothing. Its help and version are still said.
    /// Told from the variables the command was started with, as `cli_mode` hands them over.
    #[test]
    fn nothing_is_asked_from_inside_a_claude_session() {
        let vars = |set: &[(&str, &str)]| set.iter().map(|(name, value)| (OsString::from(name), OsString::from(value))).collect::<Vec<_>>().into_iter();
        // The user's own terminal: none of a session's variables, or one that is set to nothing.
        let own = [("PATH", "/usr/bin:/bin"), ("HOME", "/Users/me"), ("SHELL", "/bin/zsh"), ("CLAUDE_CONFIG_DIR", "/Users/me/.claude"), ("CLAUDECODE", "")];
        let inside = |mark: &str| vars(&[("PATH", "/usr/bin:/bin"), ("HOME", "/Users/me"), (mark, "1")]);

        // No word at all is `status`, through the installed link.
        for line in ["", "status", "status --json", "--json", "start", "stop web", "stop --command test", "restart", "logs", "logs -f", "run test", "card x", "open", "edit", "quiet 1h"] {
            assert!(matches!(asked(&words(line), vars(&own)), Ok(Parsed::Call { .. })), "{line:?} goes on in the user's own terminal");
            for mark in ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_ENTRYPOINT"] {
                assert_eq!(asked(&words(line), inside(mark)), Err(Unasked::InsideClaude), "{line:?} is held with {mark} set");
            }
        }
        assert_eq!(asked(&words("--help"), inside("CLAUDECODE")), Ok(Parsed::Help));
        assert_eq!(asked(&words("--version"), inside("CLAUDECODE")), Ok(Parsed::Version));
        assert!(matches!(asked(&words("bogus"), inside("CLAUDECODE")), Err(Unasked::Usage(_))));
    }
}
