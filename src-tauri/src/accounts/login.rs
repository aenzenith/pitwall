//! An account's sign-in: Claude Code's own (`claude auth login --claudeai`), run with the
//! account's folder, which opens the browser at Anthropic's sign-in page and keeps what it gets
//! where Claude Code keeps it (the account's Keychain item). Pitwall sees nothing of it but how it
//! ended and the page's address Claude prints in case the browser didn't open, which goes to the
//! window and is never kept or logged. One sign-in at a time.
//!
//! A Codex account's sign-in is Codex's own the same way (`codex login`, run with the account's
//! `CODEX_HOME`), which opens the browser at OpenAI's; what it prints goes to the window as it
//! comes, for "Show the terminal", and is never kept or logged either.

use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::i18n::t;

/// A sign-in nobody finished in this long is stopped.
const LIMIT: Duration = Duration::from_secs(15 * 60);
/// How often the sign-in is looked at while it runs.
const POLL: Duration = Duration::from_millis(300);

/// How a sign-in ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// Claude Code is signed in with the account's folder.
    Done,
    /// It ended before that (the page closed, refused, timed out), with Claude Code's last word
    /// on it.
    Closed(Option<String>),
    /// Claude Code isn't on the terminal's `PATH`.
    Missing,
    /// The user stopped it.
    Cancelled,
}

struct Running {
    pid: u32,
    cancelled: Arc<AtomicBool>,
    /// Claude Code is done; what follows it (`on_end`) is still under way.
    exited: Arc<AtomicBool>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn running() -> std::sync::MutexGuard<'static, Option<Running>> {
    RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A sign-in is under way, or what follows it.
pub fn busy() -> bool {
    running().is_some()
}

/// The user stopped the sign-in under way, perhaps after Claude Code was done with it: what
/// follows it (adding the account) checks this before it adds anything.
pub fn cancelled() -> bool {
    running().as_ref().is_some_and(|running| running.cancelled.load(Ordering::Relaxed))
}

/// A sign-in's CLI: its program, its words, the variable that names the account's folder, and
/// how to tell it is signed in afterwards.
struct Cli {
    program: Option<PathBuf>,
    args: &'static [&'static str],
    env: &'static str,
    signed_in: fn(&str) -> bool,
}

/// What a line of the sign-in goes to, as it comes (Codex's, for "Show the terminal").
type OnLine = Arc<dyn Fn(String) + Send + Sync>;
/// What the sign-in page's address goes to, once.
type OnUrl = Arc<Mutex<Option<Box<dyn FnOnce(String) + Send>>>>;

/// Starts the sign-in of the account whose folder is `dir`. `on_url` gets the sign-in page's
/// address once Claude printed it; `on_end`, on a thread of its own, how it ended.
pub fn start(dir: &str, on_url: impl Fn(String) + Send + 'static, on_end: impl FnOnce(Ended) + Send + 'static) -> Result<(), String> {
    let cli = Cli { program: super::sync::claude_program(), args: &["auth", "login", "--claudeai"], env: super::ENV, signed_in };
    run(cli, dir, true, on_url, None, on_end)
}

/// Starts the sign-in again for an account already listed, as `start` does a new one's: the one
/// whose folder is `dir`, or the main one (none), whose sign-in Claude Code is asked for without
/// the variable. Claude Code's own sign-in, which renews when its claude.ai sign-in ends.
pub fn renew(dir: Option<&str>, on_url: impl Fn(String) + Send + 'static, on_end: impl FnOnce(Ended) + Send + 'static) -> Result<(), String> {
    let (folder, signed_in): (String, fn(&str) -> bool) = match dir {
        Some(dir) => (dir.to_string(), signed_in),
        None => (super::main_dir(&dirs::home_dir().unwrap_or_default()).to_string_lossy().into_owned(), main_signed_in),
    };
    let cli = Cli { program: super::sync::claude_program(), args: &["auth", "login", "--claudeai"], env: super::ENV, signed_in };
    run(cli, &folder, dir.is_some(), on_url, None, on_end)
}

/// Starts the sign-in of the Codex account whose home is `dir`, as `start` does Claude's: `on_url`
/// gets the sign-in page's address, `on_line` every line Codex prints (escape sequences taken
/// out), `on_end` how it ended.
pub fn start_codex(dir: &str, on_url: impl Fn(String) + Send + 'static, on_line: impl Fn(String) + Send + Sync + 'static, on_end: impl FnOnce(Ended) + Send + 'static) -> Result<(), String> {
    let cli = Cli { program: super::codex_sync::codex_program(), args: &["login"], env: super::codex::ENV, signed_in: |dir| crate::codex_usage::signed_in(Path::new(dir)) };
    run(cli, dir, true, on_url, Some(Arc::new(on_line)), on_end)
}

/// Runs the sign-in of `cli` for the folder `dir`, given to it by its variable (`named`), or with
/// the variable taken out for the main account, Claude's own folder.
fn run(cli: Cli, dir: &str, named: bool, on_url: impl Fn(String) + Send + 'static, on_line: Option<OnLine>, on_end: impl FnOnce(Ended) + Send + 'static) -> Result<(), String> {
    let mut slot = running();
    if slot.is_some() {
        return Err(t!("core.accounts.loginBusy"));
    }
    let Some(program) = cli.program else {
        drop(slot);
        thread::spawn(move || on_end(Ended::Missing));
        return Ok(());
    };
    let home = dirs::home_dir().unwrap_or_default();
    let mut command = match named {
        true => crate::process::terminal_command(&program, &[(cli.env, dir)]),
        false => {
            let mut command = crate::process::terminal_command(&program, &[]);
            command.env_remove(cli.env);
            command
        }
    };
    command.args(cli.args).current_dir(&home).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            drop(slot);
            let error = error.to_string();
            thread::spawn(move || on_end(Ended::Closed(Some(error))));
            return Ok(());
        }
    };

    let (cancelled, exited) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    *slot = Some(Running { pid: child.id(), cancelled: Arc::clone(&cancelled), exited: Arc::clone(&exited) });
    drop(slot);

    let said: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    // Claude prints the page's address on its output; Codex on either.
    let on_url: OnUrl = Arc::new(Mutex::new(Some(Box::new(on_url))));
    let tell_url = move |line: &str| {
        if let Some(url) = sign_in_url(line) {
            let first = on_url.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
            if let Some(on_url) = first {
                on_url(url);
            }
        }
    };
    if let Some(stdout) = child.stdout.take() {
        let (tell_url, on_line) = (tell_url.clone(), on_line.clone());
        crate::process::read_lines(stdout, move |line| {
            tell_url(&line);
            if let Some(on_line) = &on_line {
                on_line(plain(&line));
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let said = Arc::clone(&said);
        let lines = on_line.is_some();
        crate::process::read_lines(stderr, move |line| {
            if lines {
                tell_url(&line);
            }
            let line = plain(&line);
            if let Some(on_line) = &on_line {
                on_line(line.clone());
            }
            if !line.is_empty() {
                *said.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(line);
            }
        });
    }

    let dir = dir.to_string();
    let signed_in = cli.signed_in;
    thread::spawn(move || {
        let ended = wait(&mut child, &cancelled, &dir, &said, signed_in);
        exited.store(true, Ordering::Relaxed);
        // Busy until the account is added or let go: a second sign-in waits for it, and a
        // cancel still reaches it (`cancelled`).
        on_end(ended);
        *running() = None;
    });
    Ok(())
}

/// Stops the sign-in under way, if any.
pub fn cancel() {
    let slot = running();
    if let Some(running) = slot.as_ref() {
        running.cancelled.store(true, Ordering::Relaxed);
        // Its pid may be another process's by now.
        if !running.exited.load(Ordering::Relaxed) {
            crate::process::kill_tree(running.pid, true);
        }
    }
}

fn wait(child: &mut Child, cancelled: &AtomicBool, dir: &str, said: &Mutex<Option<String>>, signed_in: fn(&str) -> bool) -> Ended {
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < LIMIT && !cancelled.load(Ordering::Relaxed) => thread::sleep(POLL),
            _ => {
                crate::process::kill_tree(child.id(), true);
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ended::Cancelled;
    }
    // Its last words come in on their own threads; a moment for them.
    thread::sleep(Duration::from_millis(100));
    let last = said.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    match status {
        Some(status) if status.success() && signed_in(dir) => Ended::Done,
        _ => Ended::Closed(last),
    }
}

/// Whether Claude Code holds a sign-in for the account whose folder is `dir`: its Keychain item
/// is there (asked without its secret), or its credentials file.
pub fn signed_in(dir: &str) -> bool {
    let file = Path::new(dir).join(".credentials.json").is_file();
    file || keychain_item(&super::keychain_service(Some(dir)))
}

/// Whether Claude Code holds a sign-in for the main account, whose folder is `dir`: its own
/// Keychain item (named without a folder) or its credentials file.
fn main_signed_in(dir: &str) -> bool {
    Path::new(dir).join(".credentials.json").is_file() || keychain_item(&super::keychain_service(None))
}

#[cfg(target_os = "macos")]
fn keychain_item(service: &str) -> bool {
    // Without `-w` or `-g` the item's attributes only, never its secret.
    let mut command = crate::process::command("/usr/bin/security");
    command.args(["find-generic-password", "-s", service]);
    crate::process::output_within(&mut command, Duration::from_secs(10)).is_ok_and(|output| output.status.success())
}

#[cfg(not(target_os = "macos"))]
fn keychain_item(_service: &str) -> bool {
    false
}

/// The sign-in page's address in a line of Claude's: the first `https://` up to a space or a
/// terminal's control character (it can come wrapped as a terminal link).
fn sign_in_url(line: &str) -> Option<String> {
    let at = line.find("https://")?;
    let url: String = line[at..].chars().take_while(|c| !c.is_whitespace() && !c.is_control()).collect();
    (url.len() > "https://".len()).then_some(url)
}

/// A line without a terminal's escape sequences.
fn plain(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI (`ESC [ … letter`) or OSC (`ESC ] … BEL | ESC \`).
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.peek() == Some(&'\\')) {
                            chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out.trim().to_string()
}

