//! The team radio, the app's end: a Claude Code session working in a listed project asks Pitwall
//! instead of guessing (is the server up, where, what does it print) and does a few things
//! through it (the server, a saved command, the board). Claude Code reaches it as an MCP server,
//! the app's own binary run as `--mcp` (`mcp.rs`), which passes each call on to here.
//!
//! The gates, in the order a call meets them:
//! - the endpoint: a port on 127.0.0.1 that is there only while the radio is on, and says
//!   nothing to a caller that doesn't show the key from the endpoint's file, which only the user
//!   can read (`~/.pitwall/radio.json`);
//! - the radio's own switch, read again for every call;
//! - the folder: a call acts on the listed project that holds the folder it comes from (the
//!   longest match) and on no other. No tool takes a project or a path, and from any other
//!   folder every tool is refused;
//! - the tool's switch in Settings (`run_command` starts off).
//!
//! Whose call it is comes from the caller's parent pid, through Claude Code's running sessions
//! (`sessions.rs`: its allowed fields only). Today's calls are kept per session, in memory, for
//! that session's details: the tool, the time and what came of it, never what an argument said
//! but a new card's title.

use std::io::BufReader;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::mcp::{self, receive, same_secret, send, Call, Endpoint, Hello, Reply, Timed, Welcome, TOOLS};

mod register;
mod tools;

use register::Listed;

/// Not on Windows yet: none of it has run there (how Claude Code starts its servers, the shell
/// its CLI is asked through).
const SUPPORTED: bool = cfg!(unix);
/// Callers answered at once; one more is closed unanswered.
const CALLERS_MAX: usize = 16;
/// A caller shows its key, and says what it wants, within this long: all of it, so one that says
/// a byte now and then keeps no line.
const SAY_LIMIT: Duration = Duration::from_secs(5);
/// What Claude Code said of the radio is good for this long: Settings asks each time the section
/// comes into view.
const ASKED_FOR: Duration = Duration::from_secs(30);
/// Calls kept per session, and sessions they are kept for.
const CALLS_KEPT: usize = 50;
const SESSIONS_KEPT: usize = 64;

/// Held while the endpoint's file is written or taken away, so it never tells of an endpoint
/// that has closed meanwhile.
static FILE: Mutex<()> = Mutex::new(());

/// The endpoint while it is open.
struct Listening {
    port: u16,
    /// What a caller shows, and what it is shown back (`mcp.rs`).
    key: String,
    answer: String,
    /// Set to close it: the listener takes no caller after.
    closed: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct Radio {
    listening: Option<Listening>,
    /// The calls of each session, the oldest first.
    calls: HashMap<String, VecDeque<RadioCall>>,
    /// How many calls were noted: when it moves, the window asks for a session's calls again.
    pub(super) noted: u64,
    /// What Claude Code last said of the radio, and when. Held while its CLI is asked or told:
    /// one of those at a time.
    asked: Arc<Mutex<Option<(Instant, RadioState)>>>,
}

/// Who calls: the folder Claude Code started its server in, and Claude Code's own pid.
pub(super) struct Caller {
    pub(super) cwd: String,
    pub(super) ppid: u32,
}

/// One call, for its session's details.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RadioCall {
    pub at: u64,
    pub tool: &'static str,
    pub result: CallResult,
}

/// What came of a call, as the list shows it. Of what Claude wrote, only a new card's title.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CallResult {
    /// The server as it was answered: `running` | `stopped` | `crashed` | `busy`, and the line
    /// the app shows for it.
    Status { status: &'static str, port: Option<u16>, issue: Option<String> },
    /// Lines of output read.
    Lines { count: usize },
    /// A card added to the board.
    Card { title: String },
    /// A note on the session's card, which went to review.
    Noted,
    /// A saved command run, by the name the user gave it.
    Ran { name: String },
    Refused,
}

/// The radio as Claude Code has it: what Settings waits for. The tools are no part of it
/// (`radio_tools`), and each one's switch is the settings' own.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RadioState {
    /// `on` | `off` | `unsupported` (not on this system)
    pub status: &'static str,
    /// Claude Code's CLI wasn't found: Pitwall can't be added to it or taken out.
    pub missing: bool,
    /// Claude Code sessions with a line to Pitwall right now.
    pub sessions: usize,
}

/// One tool, for its row in Settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RadioTool {
    pub name: &'static str,
    /// It changes something; the others only read.
    pub acts: bool,
}

impl RadioState {
    fn off(missing: bool) -> Self {
        Self { status: "off", missing, sessions: 0 }
    }

    fn on(missing: bool) -> Self {
        Self { status: "on", missing, sessions: register::lines() }
    }

    fn unsupported() -> Self {
        Self { status: "unsupported", missing: false, sessions: 0 }
    }
}

/// The tools there are (`mcp::TOOLS`), for their rows in Settings. Nothing is asked for it: the
/// rows stand before Claude Code's CLI has said how the radio stands (`Core::radio_state`).
pub fn radio_tools() -> Vec<RadioTool> {
    TOOLS.iter().map(|tool| RadioTool { name: tool.name, acts: tool.acts }).collect()
}

/// One caller being answered; counted while it is.
struct Line(Arc<AtomicUsize>);

impl Drop for Line {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(unix)]
fn system_random(bytes: &mut [u8]) -> bool {
    fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(bytes)).is_ok()
}

#[cfg(not(unix))]
fn system_random(_bytes: &mut [u8]) -> bool {
    false
}

/// Fills `bytes` with what nobody can guess: from the system's generator, and where that is no
/// file, from the keys std draws from it for its hashers. The phone's topic is made of it too
/// (`relay.rs`).
pub(in crate::core) fn random(bytes: &mut [u8]) {
    use std::hash::BuildHasher;

    if !system_random(bytes) {
        for (at, part) in bytes.chunks_mut(8).enumerate() {
            let drawn = std::collections::hash_map::RandomState::new().hash_one((at, std::process::id(), now_ms())).to_le_bytes();
            part.copy_from_slice(&drawn[..part.len()]);
        }
    }
}

/// A secret nobody can guess, as 32 hex digits.
fn secret() -> String {
    let mut bytes = [0u8; 16];
    random(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Core {
    fn radio_file(&self) -> PathBuf {
        mcp::endpoint_file(&self.cfg.registry_dir)
    }

    /// At start: the endpoint opens when the radio is on. Off, a file a run before left goes.
    pub fn radio_start(self: &Arc<Self>) {
        if SUPPORTED && self.lock().settings.radio.on {
            if let Err(error) = self.radio_open() {
                eprintln!("[pitwall] {error}");
            }
        } else {
            let _ = fs::remove_file(self.radio_file());
        }
    }

    /// Opens the endpoint: a port of its own, reachable from this machine only, new secrets, and
    /// the file that tells them. Open already, it stays as it is.
    fn radio_open(self: &Arc<Self>) -> Result<(), String> {
        let failed = |error: std::io::Error| t!("core.radio.noEndpoint", error = error);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(failed)?;
        let port = listener.local_addr().map_err(failed)?.port();
        let closed = Arc::new(AtomicBool::new(false));
        let listening = Listening { port, key: secret(), answer: secret(), closed: Arc::clone(&closed) };
        {
            let mut inner = self.lock();
            if inner.radio.listening.is_some() {
                return Ok(());
            }
            inner.radio.listening = Some(listening);
        }
        if let Err(error) = self.radio_publish() {
            self.lock().radio.listening = None;
            return Err(failed(error));
        }

        // The listener only hands callers on: each is answered on a thread of its own, so a call
        // that waits (a server on its way up) holds nobody else up.
        let core = Arc::downgrade(self);
        let callers = Arc::new(AtomicUsize::new(0));
        thread::spawn(move || {
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

                let line = Line(Arc::clone(&callers));
                if callers.fetch_add(1, Ordering::SeqCst) >= CALLERS_MAX {
                    continue;
                }
                thread::spawn(move || {
                    let _line = line;
                    let _ = core.radio_answer(stream);
                });
            }
        });
        Ok(())
    }

    /// Writes the endpoint's file as things are now: where the app listens, and the tools that
    /// are on. For the user alone, and whole: the other end never reads half of it.
    fn radio_publish(&self) -> std::io::Result<()> {
        let _file = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let endpoint = {
            let inner = self.lock();
            let Some(listening) = &inner.radio.listening else {
                return Ok(());
            };
            let tools = TOOLS.iter().filter(|tool| inner.settings.radio.allows(tool.name)).map(|tool| tool.name.to_string()).collect();
            Endpoint { v: mcp::ENDPOINT_VERSION, port: listening.port, key: listening.key.clone(), answer: listening.answer.clone(), tools }
        };

        super::restore::write_private(&self.radio_file(), &serde_json::to_string(&endpoint)?)
    }

    /// Closes the endpoint: nobody listens, a caller on its way in gets nothing, and the file
    /// goes.
    fn radio_close(&self) {
        let _file = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let listening = self.lock().radio.listening.take();
        let _ = fs::remove_file(self.radio_file());

        if let Some(listening) = listening {
            listening.closed.store(true, Ordering::SeqCst);
            // The listener waits for a caller: this last one lets it see that it is closed.
            let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, listening.port)), Duration::from_secs(1));
        }
    }

    /// One caller: its key, then its call. Anything else on the line (a wrong key, no JSON, a
    /// line too long, silence) closes it without a word.
    fn radio_answer(self: &Arc<Self>, stream: TcpStream) -> std::io::Result<()> {
        stream.set_write_timeout(Some(SAY_LIMIT))?;
        let mut reader = BufReader::new(Timed::new(stream.try_clone()?, SAY_LIMIT));
        let mut writer = stream;

        let hello: Hello = receive(&mut reader)?;
        // The key as it is now: a caller that came in before the radio was closed has none.
        let answer = {
            let inner = self.lock();
            inner.radio.listening.as_ref().filter(|listening| same_secret(&hello.key, &listening.key)).map(|listening| listening.answer.clone())
        };
        let Some(answer) = answer else {
            return Ok(());
        };
        send(&mut writer, &Welcome { answer })?;

        let call: Call = receive(&mut reader)?;
        // The other end is as old as its session: one of another version of the app is told so,
        // and nothing is done for it.
        let reply = if hello.v == mcp::ENDPOINT_VERSION {
            self.radio_call(&Caller { cwd: call.cwd, ppid: call.ppid }, &call.tool, &call.args)
        } else {
            Reply::refused(mcp::OTHER_VERSION)
        };
        send(&mut writer, &reply)
    }

    /// The radio's own switch: the endpoint follows it, now and at the next start.
    fn radio_turn(self: &Arc<Self>, on: bool) -> Result<(), String> {
        self.lock().settings.radio.on = on;
        self.save_settings();

        let opened = if on {
            self.radio_open()
        } else {
            self.radio_close();
            Ok(())
        };
        if opened.is_err() {
            self.lock().settings.radio.on = false;
            self.save_settings();
        }
        self.notify();
        opened
    }

    /// Turns the radio on or off. On: Pitwall is added to Claude Code's MCP servers as this
    /// app's own binary (in the place of whatever is there under its name), then the endpoint
    /// opens. Off: Pitwall is taken out again and the endpoint closes, whatever the CLI says;
    /// when it couldn't be taken out that is said once, and it goes the next time the state is
    /// asked (`radio_ask`). Until then it only answers that the radio is off. Claude Code's CLI
    /// is told to write here; it takes a moment, so off the main thread.
    pub fn radio_set(self: &Arc<Self>, on: bool) -> Result<RadioState, String> {
        if !SUPPORTED {
            return Ok(RadioState::unsupported());
        }
        let asked = Arc::clone(&self.lock().radio.asked);
        let mut asked = asked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // Whatever comes of it, what Claude Code said before holds no more.
        *asked = None;

        let state = if on {
            register::add()?;
            if let Err(error) = self.radio_turn(true) {
                let _ = register::remove();
                return Err(error);
            }
            RadioState::on(false)
        } else {
            let removed = register::remove();
            let _ = self.radio_turn(false);
            removed?;
            RadioState::off(false)
        };
        *asked = Some((Instant::now(), state.clone()));
        Ok(state)
    }

    /// The radio as Claude Code has it. What its CLI says is asked by one caller at a time and
    /// good for `ASKED_FOR`; it takes a moment, so off the main thread.
    pub fn radio_state(self: &Arc<Self>) -> RadioState {
        if !SUPPORTED {
            return RadioState::unsupported();
        }
        let asked = Arc::clone(&self.lock().radio.asked);
        let mut asked = asked.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = match asked.as_ref().filter(|(at, _)| at.elapsed() < ASKED_FOR) {
            Some((_, state)) => state.clone(),
            None => {
                let state = self.radio_ask();
                *asked = Some((Instant::now(), state.clone()));
                state
            }
        };
        drop(asked);
        state
    }

    /// Asks Claude Code's CLI how the radio stands.
    fn radio_ask(self: &Arc<Self>) -> RadioState {
        if !self.lock().settings.radio.on {
            return match register::listed() {
                // Left there when the radio was turned off and it couldn't be taken out: it is
                // now, and nothing is said of it. Another copy's is not this one's to take.
                Listed::Own => {
                    let _ = register::remove();
                    RadioState::off(false)
                }
                listed => RadioState::off(listed == Listed::Missing),
            };
        }

        match register::listed() {
            // Taken out of Claude Code by hand: nothing can call any more, so the radio is off
            // here too.
            Listed::No => {
                let _ = self.radio_turn(false);
                RadioState::off(false)
            }
            // The app it named is gone (it moved, a build folder was cleaned): this one is put in
            // its place.
            Listed::Gone => {
                let _ = register::add();
                RadioState::on(false)
            }
            listed => RadioState::on(listed == Listed::Missing),
        }
    }

    /// One tool's switch.
    pub fn radio_tool(&self, name: &str, on: bool) -> Result<(), String> {
        if !TOOLS.iter().any(|tool| tool.name == name) {
            return Err(format!("Unknown tool: {name}"));
        }

        self.lock().settings.radio.tools.insert(name.to_string(), on);
        self.save_settings();
        // The list Claude Code is given follows the switches.
        let _ = self.radio_publish();
        self.notify();
        Ok(())
    }

    /// Notes a call for its session's details; one whose session can't be told is noted nowhere.
    fn radio_note(&self, session: Option<&str>, tool: &'static str, result: CallResult) {
        let Some(session) = session else {
            return;
        };

        {
            let mut inner = self.lock();
            let radio = &mut inner.radio;
            // Room for one more session: the one quiet the longest goes.
            if !radio.calls.contains_key(session) && radio.calls.len() >= SESSIONS_KEPT {
                let quietest = radio.calls.iter().min_by_key(|(_, calls)| calls.back().map_or(0, |call| call.at)).map(|(id, _)| id.clone());
                if let Some(id) = quietest {
                    radio.calls.remove(&id);
                }
            }

            let calls = radio.calls.entry(session.to_string()).or_default();
            calls.push_back(RadioCall { at: now_ms(), tool, result });
            while calls.len() > CALLS_KEPT {
                calls.pop_front();
            }
            radio.noted += 1;
        }
        self.notify();
    }

    /// Today's calls of `session`, the newest first.
    pub fn radio_calls(&self, session: &str) -> Vec<RadioCall> {
        let (midnight, _, _) = crate::clock::local_day(now_ms());
        let inner = self.lock();
        inner.radio.calls.get(session).map(|calls| calls.iter().rev().filter(|call| call.at >= midnight).cloned().collect()).unwrap_or_default()
    }
}

// The endpoint is on macOS and Linux only (`SUPPORTED`).
#[cfg(all(test, unix))]
mod tests;
