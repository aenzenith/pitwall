//! The command line, the app's end: the user's own terminal asks Pitwall (`pitwall status`,
//! `start`, `logs`, `run`, `card`…) through the app's binary run as a command (`cli.rs`), which
//! passes each call on to here. And the `pitwall://` links other apps open (`scheme.rs`).
//!
//! The line is of the radio's kind and its own: a port on 127.0.0.1 that is there only while
//! the command is installed (`settings.cli.on`; a dev build installs nothing, and has its line
//! whenever the installed app's settings, which it shares, say so), and that says nothing to a
//! caller who doesn't show the key from the line's file, which only the user can read
//! (`~/.pitwall/cli.json`). The radio's rules are Claude's and are not these: a call names its
//! project or comes from a folder of one, nothing is switched tool by tool, and no call is kept.
//!
//! What a call can do is what the window can, and no more: a saved command by its name, never a
//! command line of the caller's; a production build never; a command marked "ask first" only
//! once the user said yes at a terminal (`cli.rs`: `agreed`). A run is the terminal's as a
//! command in its foreground is: its caller leaving (^C, the terminal closed) stops it.

use std::io::{BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use super::radio::{last_lines, saved, secret, Stranger, Strangers, HELLO_LIMIT};
use super::routing::{start_done, to_stop};
use super::*;
use crate::claude::{SessionState, TurnKind};
use crate::cli::{line_file, Ask, Call, ClaudeAt, LineFile, Row, Said, Tone, LINE_VERSION, SUPPORTED};
use crate::mcp::{receive, same_secret, send, Hello, Timed, Welcome};
use crate::resolve::is_forbidden_command;

mod install;
mod scheme;

use super::output::MARKER;
use install::{Places, Refused};

/// Callers answered at once, each having shown the key; one more is closed unanswered. Those
/// who haven't yet are strangers, at the radio's door (`Stranger`, `HELLO_LIMIT`).
const CALLERS_MAX: usize = 16;
/// A caller that showed its key says what it wants within this long.
const SAY_LIMIT: Duration = Duration::from_secs(5);
/// What may wait for someone who follows an output, as the weight of its lines (`weight`): one
/// who is further behind than this doesn't keep up, and is let go.
const FOLLOW_MAX: usize = 16 * 1024 * 1024;
/// The lines of one answer to someone who follows a busy output: no more than this many, and
/// none added once they hold this much (bytes). With the longest line an output keeps
/// (`process::LINE_MAX`) on top, an answer stays within what a line of the line may be
/// (`mcp::LINE_MAX`), however it is written out.
const BATCH_LINES: usize = 512;
const BATCH_SIZE: usize = 16 * 1024;
/// A link opened again within this long (ms) after the same one does nothing.
const LINK_REST: u64 = 2_000;
/// A start, stop or restart is waited for as its spinner is (`routing.rs`), and never longer.
const ACT_LIMIT: Duration = Duration::from_secs(40);
/// The lines `logs` begins with.
const LOG_LINES: usize = 40;
/// How often a run that is followed looks whether its caller is still there, a busy output too.
const LOOK: Duration = Duration::from_millis(200);
/// A card's title and note are as long as its dialog's fields take (`CardDialog.vue`).
const TITLE_MAX: usize = 200;
const NOTE_MAX: usize = 8_000;

/// Held while the line's file is written or taken away, so it never tells of a line that has
/// closed meanwhile.
static FILE: Mutex<()> = Mutex::new(());

/// The line while it is open.
struct Listening {
    port: u16,
    /// What a caller shows, and what it is shown back.
    key: String,
    answer: String,
    /// The language the file names: written again when the app speaks another.
    lang: &'static str,
    /// Set to close it: the listener takes no caller after, and who follows an output is let go.
    closed: Arc<AtomicBool>,
}

/// Someone who follows an output: the output's key, and where its lines go.
struct Follower {
    id: u64,
    key: String,
    lines: mpsc::Sender<String>,
    /// The weight of the lines sent and not taken yet.
    waiting: Arc<AtomicUsize>,
}

/// What a line that waits for a follower counts for: its text, and what holds it.
fn weight(line: &str) -> usize {
    line.len() + 64
}

#[derive(Default)]
pub(super) struct Cli {
    listening: Option<Listening>,
    /// Who follows an output (`logs -f`, a command that runs).
    followers: Vec<Follower>,
    /// How many ever followed: each one's number.
    followed: u64,
    /// What links did last and when (ms): the thing asked for and the listed project's path,
    /// never a link's own text. A repeat within `LINK_REST` is dropped (`open_link`).
    links: Vec<(scheme::Verb, Option<String>, u64)>,
}

impl Cli {
    /// A line was kept under `key`: whoever follows that output hears it. One who doesn't keep
    /// up (more than `FOLLOW_MAX` waits for it) is let go: the output is never held up for a
    /// follower, and never kept for one without end.
    pub(super) fn heard(&mut self, key: &str, line: &str) {
        self.followers.retain(|follower| {
            if follower.key != key {
                return true;
            }
            if follower.waiting.fetch_add(weight(line), Ordering::SeqCst) + weight(line) > FOLLOW_MAX {
                // Its end sees this at once, whatever it takes of what waits meanwhile.
                follower.waiting.store(usize::MAX, Ordering::SeqCst);
                return false;
            }
            follower.lines.send(line.to_string()).is_ok()
        });
    }
}

/// A place among those who follow an output: its lines as they are kept, until this is dropped.
struct Followed<'a> {
    core: &'a Core,
    id: u64,
    lines: mpsc::Receiver<String>,
    waiting: Arc<AtomicUsize>,
}

impl Followed<'_> {
    fn taken(&self, line: String) -> String {
        self.waiting.fetch_sub(weight(&line), Ordering::SeqCst);
        line
    }

    /// The next lines: the first of them waited for no longer than `limit`, and with it those
    /// that are there already, as many as one answer takes. Said together, a busy output costs
    /// its follower little, and it is the reader that keeps up or doesn't. Let go for not
    /// keeping up, there are none: what still waited is not said.
    fn next(&self, limit: Duration) -> Result<Vec<String>, mpsc::RecvTimeoutError> {
        if self.waiting.load(Ordering::SeqCst) > FOLLOW_MAX {
            return Err(mpsc::RecvTimeoutError::Disconnected);
        }
        let first = self.lines.recv_timeout(limit)?;
        let mut size = first.len();
        let mut lines = vec![self.taken(first)];
        while lines.len() < BATCH_LINES && size < BATCH_SIZE {
            let Ok(line) = self.lines.try_recv() else {
                break;
            };
            size += line.len();
            lines.push(self.taken(line));
        }
        Ok(lines)
    }

    /// Let go as one who doesn't keep up is (`Cli::heard`): no more lines are kept for it, and
    /// what still waited is not said.
    fn let_go(&self) {
        self.core.lock().cli.followers.retain(|follower| follower.id != self.id);
        self.waiting.store(usize::MAX, Ordering::SeqCst);
    }
}

impl Drop for Followed<'_> {
    fn drop(&mut self) {
        self.core.lock().cli.followers.retain(|follower| follower.id != self.id);
    }
}

/// The command as Settings › General shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliState {
    /// `ready` | `unsupported` (not on this system: nothing of it is drawn)
    pub status: &'static str,
    /// Where the `pitwall` command is; none while it isn't installed.
    pub path: Option<String>,
}

/// One caller being answered; counted while it is.
struct Caller(Arc<AtomicUsize>);

impl Drop for Caller {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How a call is answered: a line at a time. An error: the caller is gone.
type Say<'a> = &'a mut dyn FnMut(Said) -> std::io::Result<()>;

/// Whether whoever made a call still waits for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Presence {
    Here,
    /// It left: ^C at its terminal (its end shuts its side of the line and still reads the
    /// answer), the terminal closed, the command killed.
    Left,
    /// The line was closed under it: the command was taken out.
    Closed,
}

fn text(text: impl Into<String>, tone: Tone) -> Said {
    Said::Text { text: text.into(), tone, open: false }
}

fn failed(text: impl Into<String>) -> Said {
    Said::Failed { text: text.into() }
}

/// Lines of output as one answer; none for no lines.
fn lines_said(mut lines: Vec<String>) -> Option<Said> {
    match lines.len() {
        0 => None,
        1 => lines.pop().map(|text| Said::Line { text }),
        _ => Some(Said::Lines { lines }),
    }
}

/// A length of time in the largest unit that holds it: `38 s`, `42 min`, `3 h`, `2 d`.
fn span(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0..60 => t!("core.cli.time.seconds", count = seconds),
        60..3_600 => t!("core.cli.time.minutes", count = seconds / 60),
        3_600..172_800 => t!("core.cli.time.hours", count = seconds / 3_600),
        _ => t!("core.cli.time.days", count = seconds / 86_400),
    }
}

/// The server in the app's words: `:5173 · 42 min`, `stopped`, `crashed — …`.
fn server_text(project: &ProjectView, now: u64) -> String {
    let issue = project.issue.as_ref().map(|issue| issue.text.as_str());
    match (project.phase, project.status) {
        (Some("starting…"), _) => t!("core.cli.server.starting"),
        (Some("restarting…"), _) => t!("core.cli.server.restarting"),
        (Some(_), _) => t!("core.cli.server.stopping"),
        (None, "running") => {
            let at = match project.port {
                Some(port) => format!(":{port}"),
                None => t!("core.cli.server.running"),
            };
            match (issue, project.started_at) {
                (Some(issue), _) => issue.to_string(),
                (None, Some(since)) => format!("{at} · {}", span(now.saturating_sub(since))),
                (None, None) => at,
            }
        }
        (None, "crashed") => issue.map(str::to_string).unwrap_or_else(|| t!("core.cli.server.crashed")),
        (None, _) => match issue {
            Some(issue) => format!("{} — {issue}", t!("core.cli.server.stopped")),
            None => t!("core.cli.server.stopped"),
        },
    }
}

/// What Claude is at in a project, and the same in the app's words. A turn that waits on the
/// user comes before one that works; a permission names its tool and nothing else of it.
fn claude_at(project: &ProjectView, sessions: &[SessionState], now: u64) -> Option<(ClaudeAt, String)> {
    let Some(turn) = project.claude else {
        let working = ClaudeAt { state: "working".into(), tool: None, since: None };
        return project.claude_working.then(|| (working, t!("core.cli.claude.working")));
    };

    let waited = span(now.saturating_sub(turn.at));
    let (state, tool, said) = match turn.kind {
        TurnKind::Permission => {
            let tool = sessions.iter().find(|session| session.path == project.path && session.turn == Some(turn)).and_then(|session| session.tool.clone());
            let said = match &tool {
                Some(tool) => format!("{} · {tool}", t!("core.cli.claude.permission")),
                None => t!("core.cli.claude.permission"),
            };
            ("permission", tool, said)
        }
        TurnKind::Asking => ("asking", None, format!("{} · {waited}", t!("core.cli.claude.asking"))),
        TurnKind::Finished => ("finished", None, format!("{} · {waited}", t!("core.cli.claude.finished"))),
    };
    Some((ClaudeAt { state: state.into(), tool, since: Some(turn.at) }, said))
}

/// Where Settings' look at the PATH puts the line's switch (none: it stays as it is). On for a
/// link of this app that was `found`. Off only when the shell's PATH was `read` and holds none:
/// a shell that gave no PATH says nothing about what is in it.
fn line_follows(found: bool, read: bool, on: bool) -> Option<bool> {
    match (found, on) {
        (true, false) => Some(true),
        (false, true) if read => Some(false),
        _ => None,
    }
}

/// `name` with its letters composed: a folder named in Finder has them in two parts (`u` and
/// `¨`), a name typed at a terminal or written into a link whole (`ü`). The same name either way.
#[cfg(target_os = "macos")]
fn composed(name: &str) -> String {
    objc2::rc::autoreleasepool(|_| objc2_foundation::NSString::from_str(name).precomposedStringWithCanonicalMapping().to_string())
}

/// Elsewhere there is neither the command line nor a link yet (`SUPPORTED`).
#[cfg(not(target_os = "macos"))]
fn composed(name: &str) -> String {
    name.to_string()
}

/// `name` in any letter case, Turkish's too: `İstanbul` is `istanbul`, and `ILIK` is `ılık`.
fn folded(name: &str) -> String {
    composed(name).to_lowercase().replace("i\u{307}", "i").replace('ı', "i")
}

impl Core {
    fn cli_file(&self) -> PathBuf {
        line_file(&self.cfg.registry_dir)
    }

    /// At start: the line opens when the command is installed. Not, a file a run before left
    /// goes.
    pub fn cli_start(self: &Arc<Self>) {
        if SUPPORTED && self.lock().settings.cli.on {
            if let Err(error) = self.cli_open() {
                eprintln!("[pitwall] {error}");
            }
        } else {
            let _ = fs::remove_file(self.cli_file());
        }
    }

    /// Opens the line: a port of its own, reachable from this machine only, new secrets, and the
    /// file that tells them. Open already, it stays as it is.
    fn cli_open(self: &Arc<Self>) -> Result<(), String> {
        let failed = |error: std::io::Error| t!("core.cli.noLine", error = error);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(failed)?;
        let port = listener.local_addr().map_err(failed)?.port();
        let closed = Arc::new(AtomicBool::new(false));
        let listening = Listening { port, key: secret(), answer: secret(), lang: i18n::current(), closed: Arc::clone(&closed) };
        {
            let mut inner = self.lock();
            if inner.cli.listening.is_some() {
                return Ok(());
            }
            inner.cli.listening = Some(listening);
        }
        if let Err(error) = self.cli_publish() {
            self.lock().cli.listening = None;
            return Err(failed(error));
        }

        // The listener only hands callers on: each is answered on a thread of its own, so one
        // that follows an output holds nobody else up.
        let core = Arc::downgrade(self);
        let callers = Arc::new(AtomicUsize::new(0));
        let strangers = Arc::new(Strangers::default());
        thread::spawn(move || {
            let mut came: u64 = 0;
            for stream in listener.incoming() {
                if closed.load(Ordering::SeqCst) {
                    break;
                }
                let Some(core) = core.upgrade() else {
                    break;
                };
                let Ok(stream) = stream else {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                };

                // A stranger until it shows the key: it has a place among the callers only then.
                came += 1;
                let Ok(stranger) = Stranger::come(&strangers, came, &stream) else {
                    continue;
                };
                let (closed, callers) = (Arc::clone(&closed), Arc::clone(&callers));
                thread::spawn(move || {
                    let _ = core.cli_answer(stream, stranger, &callers, &closed);
                });
            }
        });
        Ok(())
    }

    /// Writes the line's file as things are now: where the app listens, and the language it
    /// speaks. For the user alone, and whole: the other end never reads half of it.
    fn cli_publish(&self) -> std::io::Result<()> {
        let _file = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let line = {
            let mut inner = self.lock();
            let Some(listening) = &mut inner.cli.listening else {
                return Ok(());
            };
            listening.lang = i18n::current();
            LineFile { v: LINE_VERSION, port: listening.port, key: listening.key.clone(), answer: listening.answer.clone(), lang: listening.lang.to_string() }
        };

        super::restore::write_private(&self.cli_file(), &serde_json::to_string(&line)?)
    }

    /// Closes the line: nobody listens, a caller on its way in gets nothing, who follows an
    /// output is let go, and the file goes.
    fn cli_close(&self) {
        let _file = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let listening = {
            let mut inner = self.lock();
            inner.cli.followers.clear();
            inner.cli.listening.take()
        };
        let _ = fs::remove_file(self.cli_file());

        if let Some(listening) = listening {
            listening.closed.store(true, Ordering::SeqCst);
            // The listener waits for a caller: this last one lets it see that it is closed.
            let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, listening.port)), Duration::from_secs(1));
        }
    }

    /// The line's switch: the line follows it, now and at the next start.
    fn cli_turn(self: &Arc<Self>, on: bool) -> Result<(), String> {
        self.lock().settings.cli.on = on;
        self.save_settings();

        let opened = if on {
            self.cli_open()
        } else {
            self.cli_close();
            Ok(())
        };
        if opened.is_err() {
            self.lock().settings.cli.on = false;
            self.save_settings();
        }
        self.notify();
        opened
    }

    /// One caller: its key, then its call. Anything else on the line (a wrong key, no JSON, a
    /// line too long, silence) closes it without a word. Only one who showed the key counts
    /// among the `callers`: until then it is a `stranger`, with a stranger's place and time.
    fn cli_answer(self: &Arc<Self>, stream: TcpStream, stranger: Stranger, callers: &Arc<AtomicUsize>, closed: &AtomicBool) -> std::io::Result<()> {
        stream.set_write_timeout(Some(SAY_LIMIT))?;
        let mut reader = BufReader::new(Timed::new(stream.try_clone()?, HELLO_LIMIT));
        let probe = stream.try_clone()?;
        let mut writer = stream;

        let hello = receive::<Hello>(&mut reader);
        drop(stranger);
        let hello = hello?;
        // The key as it is now: a caller that came in before the line was closed has none.
        let (answer, lang) = {
            let inner = self.lock();
            match inner.cli.listening.as_ref().filter(|listening| same_secret(&hello.key, &listening.key)) {
                Some(listening) => (listening.answer.clone(), listening.lang),
                None => return Ok(()),
            }
        };
        let _caller = Caller(Arc::clone(callers));
        if callers.fetch_add(1, Ordering::SeqCst) >= CALLERS_MAX {
            return Ok(());
        }
        send(&mut writer, &Welcome { answer })?;

        reader.get_mut().extend(SAY_LIMIT);
        let call: Call = receive(&mut reader)?;
        // What of the answer hasn't gone out yet. A caller that doesn't keep up (its terminal
        // paused, a pager) takes a line only in part before the time runs out: the rest goes
        // first the next time, so no line is ever cut short (`cli_run` lets it fall behind).
        let mut unsent: Vec<u8> = Vec::new();
        let mut say = |said: Said| {
            unsent.extend(serde_json::to_vec(&said)?);
            unsent.push(b'\n');
            while !unsent.is_empty() {
                match writer.write(&unsent) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(wrote) => {
                        unsent.drain(..wrote);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            writer.flush()
        };
        // The command is as old as its link's app, the app as old as its last start: another
        // version is told so, and nothing is done for it.
        if hello.v != LINE_VERSION {
            return say(failed(t!("core.cli.otherVersion")));
        }
        // What the command says by itself, it says in the language the file names.
        if lang != i18n::current() {
            let _ = self.cli_publish();
        }

        // Whether the caller left (^C), or the line was closed. A caller that is still there has
        // nothing more to say: the read only runs out of time. One that shut its side, or is
        // gone, has the line's end.
        let caller = || {
            let _ = probe.set_read_timeout(Some(Duration::from_millis(1)));
            let left = match probe.peek(&mut [0u8; 1]) {
                Ok(read) => read == 0,
                Err(error) => !matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut),
            };
            match (left, closed.load(Ordering::SeqCst)) {
                (true, _) => Presence::Left,
                (false, true) => Presence::Closed,
                (false, false) => Presence::Here,
            }
        };
        self.cli_call(&call, &mut say, &caller)
    }

    /// One call. `say`: its answer, a line at a time. `caller`: whether anybody still waits for
    /// it.
    pub(super) fn cli_call(self: &Arc<Self>, call: &Call, say: Say, caller: &dyn Fn() -> Presence) -> std::io::Result<()> {
        if call.ask == Ask::Status {
            return self.cli_status(say);
        }
        if let Ask::Quiet { span } = &call.ask {
            return self.cli_quiet(span, say);
        }

        let project = match self.cli_project(&call.cwd, call.project.as_deref()) {
            Ok(project) => project,
            Err(why) => return say(failed(why)),
        };
        match &call.ask {
            Ask::Act { action } => self.cli_act(&project, action, say),
            Ask::Logs { command, follow } => self.cli_logs(&project, command.as_deref(), *follow, say, caller),
            Ask::Run { name, confirmed } => self.cli_run(&project, name, confirmed.as_deref(), say, caller),
            Ask::StopCommand { name } => self.cli_stop_command(&project, name, say),
            Ask::Card { title, note } => self.cli_card(&project, title, note, say),
            Ask::Open => self.cli_open_address(&project, say),
            Ask::Edit => {
                self.open_editor(&project.path);
                say(text(t!("core.cli.edit.opened", name = project.name), Tone::Plain))?;
                say(Said::Done)
            }
            Ask::Status | Ask::Quiet { .. } => say(Said::Done),
        }
    }

    /// The project a call is about: the one it names, by its whole name or by the only name that
    /// begins so; without a name, the listed project that holds the folder it comes from. Else
    /// why there is none, with the names there are.
    fn cli_project(&self, cwd: &str, named: Option<&str>) -> Result<ProjectView, String> {
        let projects = self.snapshot().projects;
        let names = |among: &[&ProjectView]| among.iter().map(|project| project.name.as_str()).collect::<Vec<_>>().join(", ");
        let all: Vec<&ProjectView> = projects.iter().collect();
        if all.is_empty() {
            return Err(t!("core.cli.project.empty"));
        }

        let Some(named) = named.map(str::trim).filter(|named| !named.is_empty()) else {
            return self.radio_project(cwd).ok_or_else(|| t!("core.cli.project.none", names = names(&all)));
        };
        // Its whole name as it is written, else in any letter case, else how it begins.
        let (whole, lower) = (composed(named), folded(named));
        let tests: [&dyn Fn(&ProjectView) -> bool; 3] =
            [&|project| composed(&project.name) == whole, &|project| folded(&project.name) == lower, &|project| folded(&project.name).starts_with(&lower)];
        let found = tests.iter().map(|test| all.iter().copied().filter(|project| test(project)).collect::<Vec<_>>()).find(|found| !found.is_empty()).unwrap_or_default();

        match found.as_slice() {
            [project] => Ok((*project).clone()),
            [] => Err(t!("core.cli.project.unknown", name = named, names = names(&all))),
            several => Err(t!("core.cli.project.ambiguous", name = named, names = names(several))),
        }
    }

    fn cli_view(&self, path: &str) -> Option<ProjectView> {
        self.snapshot().projects.into_iter().find(|project| project.path == path)
    }

    /// Waits, a quarter of a second at a time and never under the lock, until `done` or `limit`.
    fn cli_wait(&self, limit: Duration, done: impl Fn(&Inner) -> bool) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if done(&self.lock()) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    /// Every listed project, in the list's order.
    fn cli_status(&self, say: Say) -> std::io::Result<()> {
        let now = now_ms();
        let (projects, sessions) = {
            let inner = self.lock();
            (snapshot::build_snapshot(&inner).projects, inner.scanned.clone())
        };

        let rows = projects
            .iter()
            .map(|project| {
                let (claude, claude_text) = claude_at(project, &sessions, now).unzip();
                Row {
                    name: project.name.clone(),
                    status: project.status.to_string(),
                    port: project.port,
                    url: project.url.clone(),
                    started_at: project.started_at,
                    issue: project.issue.as_ref().map(|issue| issue.text.clone()),
                    claude,
                    server_text: server_text(project, now),
                    claude_text: claude_text.unwrap_or_default(),
                }
            })
            .collect();
        say(Said::Rows { rows })?;
        say(Said::Done)
    }

    /// Start, stop or restart, routed as a click on the project's button is, and answered once
    /// its spinner would stop.
    fn cli_act(self: &Arc<Self>, project: &ProjectView, action: &str, say: Say) -> std::io::Result<()> {
        let Some(action) = Action::parse(action) else {
            return say(failed(t!("core.cli.unknown", word = action)));
        };
        let (path, name) = (project.path.as_str(), project.name.as_str());

        let idle = match action {
            Action::Start => project.status == "running",
            Action::Stop => !to_stop(&self.lock(), path),
            Action::Restart => false,
        };
        if idle {
            let said = if action == Action::Start { t!("core.cli.act.running", name = name) } else { t!("core.cli.act.idle", name = name) };
            say(text(said, Tone::Plain))?;
            return say(Said::Done);
        }

        let said = match action {
            Action::Start => t!("core.cli.act.starting", name = name),
            Action::Stop => t!("core.cli.act.stopping", name = name),
            Action::Restart => t!("core.cli.act.restarting", name = name),
        };
        say(Said::Text { text: said, tone: Tone::Plain, open: true })?;

        self.mark_pending(path, action);
        self.route(path, action, true);
        self.cli_wait(ACT_LIMIT, |inner| start_done(inner, path));
        // Its spinner goes with the next look at the state.
        self.notify();

        let after = self.cli_view(path);
        let status = after.as_ref().map_or("stopped", |project| project.status);
        let stands = match &after {
            // The address is what a start is asked for.
            Some(ProjectView { status: "running", port: Some(port), issue: None, .. }) => format!(":{port}"),
            Some(project) => server_text(project, now_ms()),
            None => t!("core.cli.server.stopped"),
        };
        let done = if action == Action::Stop { status != "running" } else { matches!(status, "running" | "busy") };

        say(text(format!(" {stands}"), if done { Tone::Plain } else { Tone::Bad }))?;
        say(if done { Said::Done } else { failed("") })
    }

    /// The saved command of that name, or what to say when the project has none.
    fn cli_command<'a>(project: &'a ProjectView, name: &str) -> Result<&'a CommandView, String> {
        saved(project, name).ok_or_else(|| {
            let names: Vec<&str> = project.commands.iter().map(|command| command.name.as_str()).collect();
            if names.is_empty() {
                t!("core.cli.command.none", project = project.name)
            } else {
                t!("core.cli.command.unknown", project = project.name, name = name.trim(), names = names.join(", "))
            }
        })
    }

    /// The lines kept under `key` and, to go on with what comes after them, a place among those
    /// who follow it: both in one hold of the lock a line is kept under, so none is missed and
    /// none comes twice.
    fn cli_follow(&self, key: &str, follow: bool) -> (Vec<String>, Option<Followed<'_>>) {
        let mut inner = self.lock();
        let kept = inner.output.get(key).map(|lines| lines.iter().cloned().collect()).unwrap_or_default();
        let followed = follow.then(|| {
            let (sender, lines) = mpsc::channel();
            let waiting = Arc::new(AtomicUsize::new(0));
            inner.cli.followed += 1;
            let id = inner.cli.followed;
            inner.cli.followers.push(Follower { id, key: key.to_string(), lines: sender, waiting: Arc::clone(&waiting) });
            Followed { core: self, id, lines, waiting }
        });
        (kept, followed)
    }

    /// The last lines of the dev server's output, or of a saved command's: the current run's,
    /// without the core's own markers. Followed, every line after them until the caller leaves,
    /// which ends the following and nothing else: the output's command was started elsewhere.
    fn cli_logs(&self, project: &ProjectView, command: Option<&str>, follow: bool, say: Say, caller: &dyn Fn() -> Presence) -> std::io::Result<()> {
        let key = match command {
            Some(name) => match Self::cli_command(project, name) {
                Ok(command) => job_key(&project.path, &command.id),
                Err(why) => return say(failed(why)),
            },
            None => project.path.clone(),
        };

        let (lines, followed) = self.cli_follow(&key, follow);
        for line in last_lines(&lines, LOG_LINES) {
            say(Said::Line { text: line.to_string() })?;
        }
        let Some(followed) = followed else {
            return say(Said::Done);
        };

        loop {
            match followed.next(Duration::from_millis(250)) {
                Ok(mut lines) => {
                    lines.retain(|line| !line.starts_with(MARKER));
                    if let Some(said) = lines_said(lines) {
                        say(said)?;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) if caller() == Presence::Here => {}
                Err(_) => return Ok(()),
            }
        }
    }

    /// One of the project's saved commands, by its name, with its output until it ends. Never
    /// what the app itself won't run (`is_forbidden_command`), and one the user wants to be
    /// asked about first only once they were: `confirmed` is the command line they said yes to,
    /// and until it is the line the command has now it is only said that it asks, with that
    /// line. Only a run it starts is followed, and that run is the caller's as a command in its
    /// terminal's foreground is: when the caller leaves (^C, the terminal closed) it is stopped,
    /// and what came of it is still said to a caller that only shut its side. One that doesn't
    /// keep up with the output hears no more of it, and the run stays its own all the same. A
    /// command running already is someone else's: refused, never followed or stopped from here.
    fn cli_run(self: &Arc<Self>, project: &ProjectView, name: &str, confirmed: Option<&str>, say: Say, caller: &dyn Fn() -> Presence) -> std::io::Result<()> {
        let command = match Self::cli_command(project, name) {
            Ok(command) => command,
            Err(why) => return say(failed(why)),
        };
        let name = command.name.as_str();

        if is_forbidden_command(&command.command) {
            return say(failed(t!("core.cli.run.build", name = name)));
        }
        if command.confirm && confirmed != Some(command.command.as_str()) {
            return say(Said::Confirm { name: name.to_string(), command: command.command.clone() });
        }
        if command.status == "running" || command.status == "busy" {
            return say(failed(t!("core.cli.run.running", name = name)));
        }

        // ^C before anything ran: nothing is started.
        if caller() == Presence::Left {
            return Ok(());
        }

        let key = job_key(&project.path, &command.id);
        let (_, followed) = self.cli_follow(&key, true);
        let began = now_ms();
        let started = self.run_command(&project.path, &command.id);
        // The run this call started, by its number; none once it is over already.
        let ours = if started { self.lock().jobs.get(&key).map(|job| job.run) } else { None };

        // This run's result, not the one before's.
        let ended = || self.lock().job_results.get(&key).filter(|result| result.finished_at >= began).cloned();
        if !started {
            // Started elsewhere meanwhile, or not at all.
            let said = if self.lock().jobs.contains_key(&key) { t!("core.cli.run.running", name = name) } else { t!("core.cli.run.notStarted", name = name) };
            return say(failed(said));
        }
        say(text(t!("core.cli.run.started", name = name, command = command.command), Tone::Soft))?;
        // A worker runs until it is stopped: there is no end to wait for.
        let Some(followed) = followed.filter(|_| !command.keep_running) else {
            return say(Said::Done);
        };

        // The command line as the output repeats it, and how the core says it ended: said here
        // in the line above and the one below.
        let echo = format!("$ {}", command.command);
        let endings: Vec<(String, String)> = ["core.log.exited", "core.log.stopped"]
            .iter()
            .filter_map(|key| {
                let said = format!("[pitwall] {}", i18n::text(key, &[("code", "\u{0}".into())]));
                said.split_once('\u{0}').map(|(before, after)| (before.to_string(), after.to_string()))
            })
            .collect();
        let ending = |line: &str| endings.iter().any(|(before, after)| line.len() >= before.len() + after.len() && line.starts_with(before) && line.ends_with(after));
        let own = |line: &str| line.starts_with(MARKER) || line == echo || ending(line);

        // The caller left: this run goes with it, as its Stop button stops it, and only while
        // the run going is still this call's. On a thread of its own, so that its last lines are
        // said meanwhile. Gives when it was asked for.
        let stop = || {
            if ours.is_some() && self.lock().jobs.get(&key).map(|job| job.run) == ours {
                let (core, path, id) = (Arc::clone(self), project.path.clone(), command.id.clone());
                thread::spawn(move || core.stop_command(&path, &id));
            }
            Instant::now()
        };
        let mut stopped: Option<Instant> = None;
        let mut looked = Instant::now();
        // Let go for not keeping up with a busy output: no more of its lines are said, and the
        // run is still the caller's, stopped when it leaves, its result said when it ends.
        let mut behind = false;

        let result = loop {
            let next = if behind {
                thread::sleep(LOOK);
                Err(mpsc::RecvTimeoutError::Timeout)
            } else {
                followed.next(LOOK)
            };
            // Looked at between lines too: a busy output has no pause to look in.
            if looked.elapsed() >= LOOK {
                looked = Instant::now();
                match caller() {
                    Presence::Left if stopped.is_none() => stopped = Some(stop()),
                    // Taken out: the run goes on, as it would with no command line.
                    Presence::Closed => return Ok(()),
                    _ => {}
                }
            }
            match next {
                Ok(mut lines) => {
                    lines.retain(|line| !own(line));
                    if let Some(said) = lines_said(lines) {
                        if let Err(error) = say(said) {
                            // Not taken in time (its terminal paused, a pager): it is behind,
                            // and the run is still its own.
                            if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
                                followed.let_go();
                                behind = true;
                                continue;
                            }
                            // Gone while it was told: the run goes with it all the same.
                            if stopped.is_none() && caller() == Presence::Left {
                                stop();
                            }
                            return Err(error);
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(result) = ended() {
                        break result;
                    }
                    // A stop that never ends it: nothing more is waited for.
                    if stopped.is_some_and(|at| at.elapsed() > ACT_LIMIT) {
                        return say(failed(""));
                    }
                }
                // Let go as the line closed (its followers go before its callers see it): taken
                // out, as above. A line found closed a moment later is seen there.
                Err(mpsc::RecvTimeoutError::Disconnected) if caller() == Presence::Closed => return Ok(()),
                Err(mpsc::RecvTimeoutError::Disconnected) => behind = true,
            }
        };
        // Its last lines were kept before its result was.
        while let Ok(mut lines) = followed.next(Duration::ZERO) {
            lines.retain(|line| !own(line));
            if let Some(said) = lines_said(lines) {
                say(said)?;
            }
        }

        let took = span(result.finished_at.saturating_sub(began));
        if result.ok && !result.stopped {
            say(text(t!("core.cli.run.done", name = name, took = took), Tone::Good))?;
            return say(Said::Done);
        }
        let said = match (result.stopped, result.code) {
            (true, _) => t!("core.cli.run.stopped", name = name, took = took),
            (false, Some(code)) => t!("core.cli.run.failedCode", name = name, code = code, took = took),
            (false, None) => t!("core.cli.run.failed", name = name, took = took),
        };
        say(text(said, Tone::Bad))?;
        say(failed(""))
    }

    /// One of the project's saved commands stopped, by its name, as its Stop button stops it:
    /// its whole process tree, and a restart it waits for called off. Answered once it is down.
    /// The project's own and no other's: the name is looked for among its commands only.
    fn cli_stop_command(self: &Arc<Self>, project: &ProjectView, name: &str, say: Say) -> std::io::Result<()> {
        let command = match Self::cli_command(project, name) {
            Ok(command) => command,
            Err(why) => return say(failed(why)),
        };
        let (path, name) = (project.path.as_str(), command.name.as_str());
        let key = job_key(path, &command.id);
        let runs = |inner: &Inner| inner.jobs.contains_key(&key) || inner.supervisor.pending(&key);

        if !runs(&self.lock()) {
            say(text(t!("core.cli.stop.idle", name = name), Tone::Plain))?;
            return say(Said::Done);
        }
        say(Said::Text { text: t!("core.cli.stop.stopping", name = name), tone: Tone::Plain, open: true })?;
        self.stop_command(path, &command.id);
        say(text(format!(" {}", t!("core.cli.stop.stopped")), Tone::Plain))?;
        say(Said::Done)
    }

    /// A card in the first column of the project's board.
    fn cli_card(&self, project: &ProjectView, title: &str, note: &str, say: Say) -> std::io::Result<()> {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let note = note.trim();
        if title.is_empty() || title.chars().count() > TITLE_MAX {
            return say(failed(t!("core.cli.card.title", max = TITLE_MAX)));
        }
        // Counted as the dialog's field counts.
        if note.encode_utf16().count() > NOTE_MAX {
            return say(failed(t!("core.cli.card.note", max = NOTE_MAX)));
        }

        match self.board_add(&project.path, &title, note, &[]) {
            Ok(_) => {
                say(text(t!("core.cli.card.added", name = project.name), Tone::Plain))?;
                say(Said::Done)
            }
            Err(error) => say(failed(error)),
        }
    }

    /// The project's address in the browser, as "Open in browser" opens it.
    fn cli_open_address(self: &Arc<Self>, project: &ProjectView, say: Say) -> std::io::Result<()> {
        let Some(url) = self.resolve_url(&project.path, project.url.clone()).and_then(|url| crate::links::normalize(&url)) else {
            return say(failed(t!("core.cli.open.none", name = project.name)));
        };
        self.open_browser(&project.path);
        say(text(url, Tone::Plain))?;
        say(Said::Done)
    }

    /// The quiet mode the popover's bell sets: for an hour, until tomorrow morning, or off.
    fn cli_quiet(&self, span: &str, say: Say) -> std::io::Result<()> {
        let said = match span {
            "1h" => {
                self.quiet_start(QuietFor::Hour);
                t!("core.cli.quiet.hour")
            }
            "morning" => {
                self.quiet_start(QuietFor::Morning);
                t!("core.cli.quiet.morning")
            }
            "off" => {
                self.quiet_end();
                t!("core.cli.quiet.off")
            }
            other => return say(failed(t!("core.cli.unknown", word = other))),
        };
        say(text(said, Tone::Plain))?;
        say(Said::Done)
    }

    /* ---------- the command, in Settings › General ---------- */

    /// Where the `pitwall` command is. In the installed app the link is the line's switch: one
    /// taken away by hand closes the line, one that is there opens it. It asks the shell for its
    /// PATH, so off the main thread.
    pub fn cli_state(self: &Arc<Self>) -> CliState {
        if !SUPPORTED {
            return CliState { status: "unsupported", path: None };
        }

        let places = Places::here();
        let found = places.as_ref().and_then(Places::found);
        if crate::is_bundled() && places.is_some() {
            let on = self.lock().settings.cli.on;
            if let Some(on) = line_follows(found.is_some(), process::shell_path().is_some(), on) {
                let _ = self.cli_turn(on);
            }
        }
        CliState { status: "ready", path: found.zip(places).map(|(link, places)| places.shown(&link)) }
    }

    /// Puts the `pitwall` command on the shell's PATH, as a link to this app, and opens the
    /// line. Never from a dev build: its binary moves with the build folder.
    pub fn cli_install(self: &Arc<Self>) -> Result<CliState, String> {
        if !SUPPORTED {
            return Ok(CliState { status: "unsupported", path: None });
        }
        if !crate::is_bundled() {
            return Err(t!("core.cli.install.dev"));
        }

        // The PATH as the shell gives it now: a folder put on it since the app started (what
        // the refusal below asks for) counts at once.
        process::reread_terminal_env();
        let places = Places::here().ok_or_else(|| t!("core.cli.install.noFolder"))?;
        let link = places.install().map_err(|refused| match refused {
            Refused::Taken(other) => t!("core.cli.install.taken", path = places.shown(&other)),
            Refused::NoFolder => t!("core.cli.install.noFolder"),
        })?;
        if let Err(error) = self.cli_turn(true) {
            places.remove();
            return Err(error);
        }
        Ok(CliState { status: "ready", path: Some(places.shown(&link)) })
    }

    /// Takes the command out again (only a link that leads to this app) and closes the line.
    pub fn cli_remove(self: &Arc<Self>) -> Result<CliState, String> {
        if !SUPPORTED {
            return Ok(CliState { status: "unsupported", path: None });
        }

        let left = Places::here().and_then(|places| {
            places.remove();
            places.found().map(|link| places.shown(&link))
        });
        match left {
            // A link that couldn't be taken away: the command still works, so the line stays.
            Some(path) => Err(t!("core.cli.install.kept", path = path)),
            None => {
                let _ = self.cli_turn(false);
                Ok(CliState { status: "ready", path: None })
            }
        }
    }

    /* ---------- pitwall:// links ---------- */

    /// Whether `pitwall://` links are answered.
    pub fn cli_links(&self, on: bool) {
        self.lock().settings.cli.links = on;
        self.save_settings();
        self.notify();
    }

    /// A `pitwall://` link the system handed over. Any app or web page can open one, so it does
    /// one of four things to a listed project and nothing else (`scheme.rs`), and only while the
    /// user has links switched on. Opened again within two seconds of the same one it does
    /// nothing: a page that loops can't hammer a server. Nothing of the link is kept, shown or
    /// said. True when Pitwall's window is to come up.
    pub fn open_link(self: &Arc<Self>, url: &str) -> bool {
        self.open_link_at(url, now_ms())
    }

    /// `open_link`, at the moment `now` (ms).
    fn open_link_at(self: &Arc<Self>, url: &str, now: u64) -> bool {
        if !SUPPORTED || !self.lock().settings.cli.links {
            return false;
        }
        let Some(link) = scheme::parse(url) else {
            return false;
        };

        // The project's name as it is listed, whole (its letters composed or not), and of one
        // project only.
        let project = match &link.project {
            Some(name) => {
                let (projects, name) = (self.snapshot().projects, composed(name));
                let mut named = projects.into_iter().filter(|project| composed(&project.name) == name);
                match (named.next(), named.next()) {
                    (Some(project), None) => Some(project),
                    _ => return false,
                }
            }
            None => None,
        };

        // The same thing to the same project as a moment ago: dropped, and it has to stop for
        // `LINK_REST` before it is answered again.
        {
            let asked = (link.action, project.as_ref().map(|project| project.path.as_str()));
            let mut inner = self.lock();
            let links = &mut inner.cli.links;
            links.retain(|(_, _, at)| now.saturating_sub(*at) < LINK_REST);
            if let Some((_, _, at)) = links.iter_mut().find(|(action, path, _)| (*action, path.as_deref()) == asked) {
                *at = now;
                return false;
            }
            links.push((asked.0, asked.1.map(str::to_string), now));
        }

        match (link.action, project) {
            (scheme::Verb::Show, None) => true,
            (scheme::Verb::Show, Some(project)) => {
                self.emit(CoreEvent::RevealProject { path: project.path, job: None });
                false
            }
            (scheme::Verb::Act(action), Some(project)) => {
                self.act(&project.path, action);
                false
            }
            (scheme::Verb::Act(_), None) => false,
        }
    }

    /// The link that does `action` (`start` | `stop` | `restart` | `show`) to the listed project
    /// at `path`, for the row's "Copy link". None for a name another listed project shares: a
    /// link on it does nothing (`open_link`).
    pub fn link_text(&self, path: &str, action: &str) -> Option<String> {
        let verb = scheme::Verb::parse(action)?;
        let projects = self.snapshot().projects;
        let project = projects.iter().find(|project| project.path == path)?;
        if projects.iter().filter(|other| composed(&other.name) == composed(&project.name)).count() > 1 {
            return None;
        }
        Some(scheme::text(verb, Some(&project.name)))
    }
}

// The line is on macOS only (`SUPPORTED`), and its links are a Unix file system's.
#[cfg(all(test, unix))]
mod tests;
