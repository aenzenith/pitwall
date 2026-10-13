//! The team radio, Claude Code's end: `<app> --mcp` is the MCP server Claude Code starts with a
//! session (stdio, JSON-RPC 2.0, one message per line). It runs before anything of the app
//! starts and knows nothing itself: a tool call goes to the running app over the radio's
//! endpoint (`core/radio`), with the folder this process was started in and its parent's pid,
//! which is how the app tells whose call it is. No tool takes a project or a path.
//!
//! The endpoint is a port on 127.0.0.1 and two secrets, in a file only the user can read
//! (`~/.pitwall/radio.json`). Both ends show they can read it before anything else is said: this
//! end sends the key, the app answers with its own, and only then does a call cross. Without the
//! file the radio is off; with it and nobody on the port, Pitwall isn't running. Claude Code
//! gets a plain answer either way, never a broken server.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The flag that runs the app as the MCP server: `<app> --mcp`.
pub const MCP_FLAG: &str = "--mcp";
/// The name Claude Code lists the server under.
pub const SERVER_NAME: &str = "pitwall";
/// The endpoint's file, in `~/.pitwall/`.
const ENDPOINT_FILE: &str = "radio.json";
/// How the two ends talk, as the endpoint's file and a hello say it (`v`). This end lives as long
/// as its session, across the app's updates: an end of another version is told so.
pub(crate) const ENDPOINT_VERSION: u32 = 1;
/// The protocol versions this server speaks, the newest first. The tools are the same in all.
const PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// The longest line either end reads: a card's note fits many times over.
pub(crate) const LINE_MAX: usize = 256 * 1024;
/// The app is there or it isn't: a local port answers at once.
const CONNECT_LIMIT: Duration = Duration::from_secs(2);
const GREETING_LIMIT: Duration = Duration::from_secs(5);
/// A start or a restart answers once it is done (`core/radio`: up to the spinner's limit).
const ANSWER_LIMIT: Duration = Duration::from_secs(60);

const INSTRUCTIONS: &str = "Pitwall runs the dev servers of the projects the user lists in it, and keeps their boards. In such a project, ask it \
     for the server's state and output, and start or restart the server through it instead of running the dev script yourself.";
pub(crate) const RADIO_OFF: &str = "Pitwall's team radio is off. The user can turn it on in Pitwall: Settings › Claude.";
const NOT_RUNNING: &str = "Pitwall isn't running. Ask the user to open Pitwall, then try again.";
const WENT_QUIET: &str = "Pitwall stopped answering before this call was done. Check what happened before trying it again.";
const TOO_LONG: &str = "Pitwall's answer was too long to pass on. Ask for less at once.";
pub(crate) const OTHER_VERSION: &str =
    "Pitwall and this session's line to it are of different versions: the app was updated since the session began. Ask the user to start a new Claude Code session.";

/// One thing Claude can ask of Pitwall.
pub struct Tool {
    pub name: &'static str,
    /// For Claude: what it does, and when to use it.
    description: &'static str,
    /// It changes something; the others only read.
    pub acts: bool,
    /// On until the user switches it off; a tool that is off starts off.
    pub default_on: bool,
    /// Its arguments, as JSON Schema properties, and those it can't do without.
    arguments: fn() -> Value,
    required: &'static [&'static str],
}

pub const TOOLS: [Tool; 6] = [
    Tool {
        name: "server_status",
        description: "The state of this project's dev server as Pitwall runs it: running, stopped, crashed or busy, its address and port, the \
                      error Pitwall shows for it, and the project's saved commands. Use this before assuming the server is down, and instead \
                      of probing ports.",
        acts: false,
        default_on: true,
        arguments: || json!({}),
        required: &[],
    },
    Tool {
        name: "server_output",
        description: "The last lines the dev server printed. Use this to read its errors instead of starting the server yourself.",
        acts: false,
        default_on: true,
        arguments: || {
            json!({
                "lines": { "type": "integer", "minimum": 1, "maximum": 200, "description": "How many lines, from the end. 40 when left out." },
                "command": { "type": "string", "description": "The name of one of the project's saved commands, to read its output instead." },
            })
        },
        required: &[],
    },
    Tool {
        name: "server_restart",
        description: "Starts, stops or restarts this project's dev server through Pitwall, and answers with its state once that is done. Use \
                      this instead of running the dev script yourself: Pitwall keeps the one server everything else uses.",
        acts: true,
        default_on: true,
        arguments: || json!({ "action": { "type": "string", "enum": ["start", "stop", "restart"] } }),
        required: &["action"],
    },
    Tool {
        name: "run_command",
        description: "Runs one of the project's saved commands by its name (`server_status` lists them). Commands the user marked \"ask \
                      first\" and production builds are refused.",
        acts: true,
        default_on: false,
        arguments: || json!({ "name": { "type": "string", "description": "The command's name as Pitwall lists it." } }),
        required: &["name"],
    },
    Tool {
        name: "board_add",
        description: "Adds a card to this project's board in Pitwall, in its first column, for the user to see. Use this for work to do \
                      later, not for what you are doing now.",
        acts: true,
        default_on: true,
        arguments: || {
            json!({
                "title": { "type": "string", "description": "One short line." },
                "note": { "type": "string", "description": "What there is to do, in a few lines." },
            })
        },
        required: &["title"],
    },
    Tool {
        name: "board_note",
        description: "Adds a note to the board card this session was given and moves the card to review. Use this when the card's work is \
                      done or stuck, to say what was done.",
        acts: true,
        default_on: true,
        arguments: || json!({ "note": { "type": "string" } }),
        required: &["note"],
    },
];

impl Tool {
    fn listing(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": { "type": "object", "properties": (self.arguments)(), "required": self.required, "additionalProperties": false },
            "annotations": { "readOnlyHint": !self.acts },
        })
    }
}

/// The endpoint as its file tells it: where the app listens, what each end shows the other, and
/// the tools that are switched on (the list Claude Code is given, the app running or not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Endpoint {
    /// `ENDPOINT_VERSION` as the app that wrote it has it; none counts as another.
    #[serde(default)]
    pub v: u32,
    pub port: u16,
    pub key: String,
    pub answer: String,
    pub tools: Vec<String>,
}

impl Endpoint {
    fn read(file: &Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
    }
}

/// The endpoint's file in the registry folder `dir` (`~/.pitwall`).
pub(crate) fn endpoint_file(dir: &Path) -> PathBuf {
    dir.join(ENDPOINT_FILE)
}

/// This end's first line: that it can read the endpoint's file.
#[derive(Serialize, Deserialize)]
pub(crate) struct Hello {
    /// `ENDPOINT_VERSION` as this end has it; none counts as another.
    #[serde(default)]
    pub v: u32,
    pub key: String,
}

/// The app's line back: that it wrote that file.
#[derive(Serialize, Deserialize)]
pub(crate) struct Welcome {
    pub answer: String,
}

/// A tool call, with who makes it: the folder Claude Code started this server in and Claude
/// Code's own pid.
#[derive(Serialize, Deserialize)]
pub(crate) struct Call {
    pub cwd: String,
    pub ppid: u32,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

/// What a call came to, in words for Claude.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Reply {
    pub text: String,
    /// It was refused or it failed.
    pub error: bool,
}

impl Reply {
    pub(crate) fn ok(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: false }
    }

    pub(crate) fn refused(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: true }
    }
}

/// Whether two secrets are the same, in the same time whatever they hold.
pub(crate) fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |differs, (x, y)| differs | (x ^ y)) == 0
}

/// One line of `reader`, without its end; `None` at the end of the stream. A line longer than
/// `LINE_MAX` (what is left of it is skipped) or one that isn't UTF-8 is an error.
pub(crate) fn read_line(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    if reader.by_ref().take(LINE_MAX as u64 + 1).read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }

    if line.len() > LINE_MAX && line.last() != Some(&b'\n') {
        loop {
            let rest = reader.fill_buf()?;
            let (skip, done) = match rest.iter().position(|byte| *byte == b'\n') {
                Some(at) => (at + 1, true),
                None => (rest.len(), rest.is_empty()),
            };
            reader.consume(skip);
            if done {
                return Err(ErrorKind::InvalidData.into());
            }
        }
    }

    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line).map(Some).map_err(|_| ErrorKind::InvalidData.into())
}

/// The next line as `T`; an error at the end of the stream and for anything that isn't one.
pub(crate) fn receive<T: DeserializeOwned>(reader: &mut impl BufRead) -> std::io::Result<T> {
    let line = read_line(reader)?.ok_or(ErrorKind::UnexpectedEof)?;
    serde_json::from_str(&line).map_err(|_| ErrorKind::InvalidData.into())
}

pub(crate) fn send(writer: &mut impl Write, message: &impl Serialize) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()
}

/// A stream read against one deadline: its other end has until then for all it is to say,
/// however slowly it says it. Each read gets what is left of the time.
pub(crate) struct Timed {
    stream: TcpStream,
    until: Instant,
}

impl Timed {
    pub(crate) fn new(stream: TcpStream, limit: Duration) -> Self {
        Self { stream, until: Instant::now() + limit }
    }
}

impl Read for Timed {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buffer)
    }
}

/// Claude Code's pid: this server is its child.
#[cfg(unix)]
fn parent_pid() -> u32 {
    std::os::unix::process::parent_id()
}

#[cfg(not(unix))]
fn parent_pid() -> u32 {
    let own = std::process::id();
    crate::process::process_table().and_then(|table| table.get(&own).map(|process| process.parent)).unwrap_or(0)
}

/// Why a call got no answer.
enum Lost {
    /// The app couldn't be reached: the call never left.
    Unreached,
    /// The call left, and no answer came.
    Quiet,
    /// An answer came, longer than a line may be.
    TooLong,
}

/// Passes one call to the app.
fn ask(endpoint: &Endpoint, tool: &str, args: Value) -> Result<Reply, Lost> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let greet = || -> std::io::Result<(BufReader<Timed>, TcpStream)> {
        let stream = TcpStream::connect_timeout(&address, CONNECT_LIMIT)?;
        stream.set_write_timeout(Some(GREETING_LIMIT))?;
        let mut reader = BufReader::new(Timed::new(stream.try_clone()?, GREETING_LIMIT));
        let mut writer = stream;

        send(&mut writer, &Hello { v: ENDPOINT_VERSION, key: endpoint.key.clone() })?;
        let welcome: Welcome = receive(&mut reader)?;
        // Something else on a port the app once had: it is told nothing more.
        if !same_secret(&welcome.answer, &endpoint.answer) {
            return Err(ErrorKind::PermissionDenied.into());
        }
        Ok((reader, writer))
    };
    let (mut reader, mut writer) = greet().map_err(|_| Lost::Unreached)?;

    let cwd = std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
    send(&mut writer, &Call { cwd, ppid: parent_pid(), tool: tool.to_string(), args }).map_err(|_| Lost::Unreached)?;
    reader.get_mut().until = Instant::now() + ANSWER_LIMIT;
    match read_line(&mut reader) {
        Ok(Some(line)) => serde_json::from_str(&line).map_err(|_| Lost::Quiet),
        // The app writes JSON: a line that can't be taken is one too long.
        Err(error) if error.kind() == ErrorKind::InvalidData => Err(Lost::TooLong),
        _ => Err(Lost::Quiet),
    }
}

/// A call's answer for Claude: the app's, or why there is none.
fn call(file: Option<&Path>, tool: &str, args: Value) -> Reply {
    let Some(endpoint) = file.and_then(Endpoint::read) else {
        return Reply::refused(RADIO_OFF);
    };
    if endpoint.v != ENDPOINT_VERSION {
        return Reply::refused(OTHER_VERSION);
    }

    match ask(&endpoint, tool, args) {
        Ok(reply) => reply,
        Err(Lost::Unreached) => Reply::refused(NOT_RUNNING),
        Err(Lost::Quiet) => Reply::refused(WENT_QUIET),
        Err(Lost::TooLong) => Reply::refused(TOO_LONG),
    }
}

/// The tools that are switched on, as the endpoint's file lists them: the same whether the app
/// runs or not, and none while the radio is off.
fn switched_on(file: Option<&Path>) -> Vec<String> {
    file.and_then(Endpoint::read).map(|endpoint| endpoint.tools).unwrap_or_default()
}

/// The answer to one request; `None` for what takes none (a notification, an answer to
/// something never asked). `listed`: the tools as Claude Code was last given them.
fn answer(file: Option<&Path>, message: &Value, listed: &mut Option<Vec<String>>) -> Option<Value> {
    let done = |id: &Value, result: Value| Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    let failed = |id: &Value, code: i32, text: &str| Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": text } }));

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // No request at all (a batch, a bare value); an answer of the client's own takes none.
        let stray = message.get("result").is_some() || message.get("error").is_some();
        return if stray { None } else { failed(&Value::Null, -32600, "Invalid request") };
    };
    let id = message.get("id").filter(|id| !id.is_null())?;
    let params = message.get("params");

    match method {
        "initialize" => {
            let asked = params.and_then(|params| params.get("protocolVersion")).and_then(Value::as_str);
            let version = PROTOCOLS.iter().find(|known| Some(**known) == asked).unwrap_or(&PROTOCOLS[0]);
            done(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": true } },
                    "serverInfo": { "name": SERVER_NAME, "title": "Pitwall", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => done(id, json!({})),
        "tools/list" => {
            let on = switched_on(file);
            let tools: Vec<Value> = TOOLS.iter().filter(|tool| on.iter().any(|name| name == tool.name)).map(Tool::listing).collect();
            *listed = Some(on);
            done(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = params.and_then(|params| params.get("name")).and_then(Value::as_str).unwrap_or_default();
            if !TOOLS.iter().any(|tool| tool.name == name) {
                return failed(id, -32602, &format!("Unknown tool: {name}"));
            }
            let args = params.and_then(|params| params.get("arguments")).cloned().unwrap_or(Value::Null);
            let reply = call(file, name, args);
            done(id, json!({ "content": [{ "type": "text", "text": reply.text }], "isError": reply.error }))
        }
        _ => failed(id, -32601, &format!("Method not found: {method}")),
    }
}

/// Whether `message` is a request (a method and an id) for anything but the tool list.
fn other_request(message: &Value) -> bool {
    let method = message.get("method").and_then(Value::as_str);
    method.is_some_and(|method| method != "tools/list") && message.get("id").is_some_and(|id| !id.is_null())
}

/// Answers Claude Code until it closes the stream. A line that is no JSON gets a parse error and
/// the next one is read: nothing Claude Code sends ends the server.
fn serve(mut input: impl BufRead, mut output: impl Write, file: Option<&Path>) {
    // Claude Code reads the tool list once a session: a switch flipped in Settings since reaches
    // it as a note before the next answer, and it asks for the list again.
    let mut listed: Option<Vec<String>> = None;

    loop {
        let reply = match read_line(&mut input) {
            Ok(None) => return,
            Ok(Some(line)) if line.trim().is_empty() => continue,
            Ok(Some(line)) => match serde_json::from_str::<Value>(&line) {
                Ok(message) => {
                    if let Some(before) = listed.as_mut().filter(|_| other_request(&message)) {
                        let on = switched_on(file);
                        if *before != on {
                            *before = on;
                            if send(&mut output, &json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" })).is_err() {
                                return;
                            }
                        }
                    }
                    answer(file, &message, &mut listed)
                }
                Err(_) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "Parse error" } })),
            },
            Err(error) if error.kind() == ErrorKind::InvalidData => {
                Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32600, "message": "Invalid request" } }))
            }
            Err(_) => return,
        };

        if let Some(reply) = reply {
            if send(&mut output, &reply).is_err() {
                return;
            }
        }
    }
}

/// MCP mode: when `args` (after the program) are `--mcp`, serves Claude Code over stdin and
/// stdout and returns true once it has closed them, before anything of the app starts.
pub fn mcp_mode(mut args: impl Iterator<Item = OsString>) -> bool {
    if args.next().as_deref() != Some(OsStr::new(MCP_FLAG)) {
        return false;
    }

    let file = dirs::home_dir().map(|home| endpoint_file(&home.join(".pitwall")));
    serve(std::io::stdin().lock(), std::io::stdout().lock(), file.as_deref());
    true
}
