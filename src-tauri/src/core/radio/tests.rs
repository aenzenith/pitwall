//! The radio's gates, end to end on a core with temp folders: whose project a call reaches, what
//! a switch keeps from running, and who the endpoint answers.

use std::io::Write;

use serde_json::{json, Value};

use super::*;
use crate::settings::CustomCommand;

struct Paddock {
    tmp: tempfile::TempDir,
    core: Arc<Core>,
}

impl Paddock {
    /// A folder under the temp folder, made, as the file system names it.
    fn folder(&self, name: &str) -> String {
        let dir = self.tmp.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap().to_string_lossy().into_owned()
    }

    /// A listed project.
    fn project(&self, name: &str) -> String {
        let path = self.folder(name);
        self.core.add_project(&path).unwrap();
        path
    }

    fn call(&self, cwd: &str, tool: &str, args: Value) -> Reply {
        self.core.radio_call(&Caller { cwd: cwd.to_string(), ppid: 0 }, tool, &args)
    }

    fn cards(&self) -> Vec<(String, String)> {
        self.core.board_state().into_iter().map(|card| (card.path, card.title)).collect()
    }
}

/// A core whose radio is on (`on`), nothing listed yet.
fn paddock(on: bool) -> Paddock {
    let tmp = tempfile::tempdir().unwrap();
    let core = Core::new(
        CoreConfig {
            registry_dir: tmp.path().join("registry"),
            claude_dir: tmp.path().join("claude").join("projects"),
            settings_file: tmp.path().join("config").join("settings.json"),
            claude_settings: tmp.path().join("claude").join("settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        },
        Arc::new(|_| {}),
    );

    let mut settings = core.settings();
    settings.radio.on = on;
    core.set_settings(settings);
    Paddock { tmp, core }
}

/// The folder a call comes from decides the project, whatever the call itself says: the listed
/// project that holds it, the innermost of two, and nothing at all from any other folder.
#[test]
fn a_call_reaches_the_project_of_its_folder_and_no_other() {
    let pad = paddock(true);
    let (alpha, beta) = (pad.project("alpha"), pad.project("beta"));
    let inner = pad.project("alpha/packages/inner");
    let (deep, inner_deep) = (pad.folder("alpha/src/pages"), pad.folder("alpha/packages/inner/src"));
    // A neighbour whose name only starts like a project's, a folder above them all, and one that
    // isn't on disk, whose text alone starts in a project and leads into the other.
    let (neighbour, above) = (pad.folder("alpha-two"), pad.folder("."));
    let ghost = format!("{alpha}/ghost/../../beta");

    pad.core.push_line(&alpha, "alpha's line".into());
    pad.core.push_line(&beta, "beta's line".into());

    // Arguments that name another project are no arguments of any tool.
    let aimed = |extra: Value| {
        let mut args = json!({ "project": beta, "path": beta, "cwd": beta });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        args
    };

    let read = pad.call(&deep, "server_output", aimed(json!({})));
    assert_eq!((read.text.as_str(), read.error), ("alpha's line", false));

    assert!(!pad.call(&deep, "board_add", aimed(json!({ "title": "from alpha" }))).error);
    assert!(!pad.call(&inner_deep, "board_add", aimed(json!({ "title": "from inner" }))).error);
    assert_eq!(pad.cards(), [(alpha.clone(), "from alpha".to_string()), (inner.clone(), "from inner".to_string())]);

    for cwd in [neighbour.as_str(), above.as_str(), "", ghost.as_str()] {
        for tool in &TOOLS {
            let args = aimed(json!({ "title": "from outside", "note": "n", "name": "x", "action": "start" }));
            let reply = pad.call(cwd, tool.name, args);
            assert!(reply.error && reply.text.contains("isn't one of Pitwall's projects"), "{cwd} {}: {}", tool.name, reply.text);
        }
    }
    assert_eq!(pad.cards().len(), 2, "nothing was added from outside");
    assert_eq!(pad.core.output(&beta), ["beta's line"], "beta's output was never read");
    let busy = pad.core.snapshot().projects.iter().any(|project| project.status != "stopped");
    assert!(!busy, "nothing was started from outside");
}

/// A tool that is switched off does nothing when it is called all the same, `run_command` is
/// off until the user turns it on, and on, it never runs a command marked "ask first" or a
/// production build. With the radio itself off, no tool answers.
#[test]
fn a_switch_that_is_off_keeps_its_tool_from_acting() {
    let pad = paddock(true);
    let alpha = pad.project("alpha");
    let command = |id: &str, line: &str, confirm: bool| CustomCommand { id: id.into(), name: id.into(), command: line.into(), confirm, ..Default::default() };
    let commands =
        vec![command("safe", "echo ran > safe.txt", false), command("ask", "echo ran > ask.txt", true), command("build", "echo ran > build.txt; npm run build", false)];
    pad.core.set_project_settings(&alpha, ProjectSettings { commands, ..Default::default() });
    let ran = |name: &str| Path::new(&alpha).join(format!("{name}.txt")).exists();

    // Off as it starts.
    let reply = pad.call(&alpha, "run_command", json!({ "name": "safe" }));
    assert!(reply.error && reply.text.contains("switched this tool off"), "{}", reply.text);
    assert!(!ran("safe"));

    pad.core.radio_tool("run_command", true).unwrap();
    for name in ["ask", "build"] {
        let reply = pad.call(&alpha, "run_command", json!({ "name": name }));
        assert!(reply.error, "{name}: {}", reply.text);
    }
    // The one that may run does, so the refusals above are the gates' and no accident.
    let reply = pad.call(&alpha, "run_command", json!({ "name": "safe" }));
    assert!(!reply.error && reply.text.contains("exit code 0"), "{}", reply.text);
    assert!(ran("safe") && !ran("ask") && !ran("build"));

    pad.core.radio_tool("board_add", false).unwrap();
    assert!(pad.call(&alpha, "board_add", json!({ "title": "a card" })).error);
    assert!(pad.cards().is_empty());

    // The radio off: the same call that just ran is refused.
    let mut settings = pad.core.settings();
    settings.radio.on = false;
    pad.core.set_settings(settings);
    fs::remove_file(Path::new(&alpha).join("safe.txt")).unwrap();
    let reply = pad.call(&alpha, "run_command", json!({ "name": "safe" }));
    assert!(reply.error && reply.text.contains("radio is off"), "{}", reply.text);
    assert!(!ran("safe"));
}

/// One line to the endpoint: what is sent, then everything it says until it closes. A port
/// nobody listens on any more says nothing.
fn say(port: u16, lines: &[String]) -> String {
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else {
        return String::new();
    };
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    for line in lines {
        // A caller the endpoint has closed on already can't be written to; what it said counts.
        let _ = stream.write_all(format!("{line}\n").as_bytes());
    }
    let mut said = String::new();
    let _ = stream.read_to_string(&mut said);
    said
}

/// The endpoint is there only while the radio is on, its file is the user's alone, and a caller
/// that doesn't show the file's key is told nothing and gets nothing done.
#[test]
fn the_endpoint_answers_only_who_shows_its_key() {
    use std::os::unix::fs::PermissionsExt;

    let pad = paddock(false);
    let alpha = pad.project("alpha");
    let file = pad.core.radio_file();

    // Off: no endpoint, and a file a run before left is taken away.
    fs::write(&file, "{}").unwrap();
    pad.core.radio_start();
    assert!(!file.exists());

    pad.core.radio_turn(true).unwrap();
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let endpoint: Endpoint = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let hello = |key: &str| serde_json::to_string(&Hello { v: mcp::ENDPOINT_VERSION, key: key.into() }).unwrap();
    let card = serde_json::to_string(&Call { cwd: alpha.clone(), ppid: 0, tool: "board_add".into(), args: json!({ "title": "a card" }) }).unwrap();

    // No key, a wrong key, the key's start, the other end's secret, no JSON: not a word back.
    let strangers =
        [hello(""), hello("0".repeat(32).as_str()), hello(&endpoint.key[..31]), hello(&endpoint.answer), card.clone(), "GET / HTTP/1.1".to_string()];
    for first in strangers {
        assert_eq!(say(endpoint.port, &[first.clone(), card.clone()]), "", "{first}");
    }
    assert!(pad.cards().is_empty(), "a stranger's call did nothing");

    // The key: the answer the file promises, then the call's.
    let said = say(endpoint.port, &[hello(&endpoint.key), card.clone()]);
    let mut lines = said.lines();
    let welcome: Welcome = serde_json::from_str(lines.next().unwrap()).unwrap();
    let reply: Reply = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!((welcome.answer, reply.error), (endpoint.answer.clone(), false));
    assert_eq!(pad.cards(), [(alpha.clone(), "a card".to_string())]);

    // Off again: the file goes, and the key that worked opens nothing.
    pad.core.radio_turn(false).unwrap();
    assert!(!file.exists());
    assert!(say(endpoint.port, &[hello(&endpoint.key), card]).is_empty());
    assert_eq!(pad.cards().len(), 1);
}

/// Strangers who come and say nothing (any process here can) keep no caller out: with as many
/// of them on the line as callers are answered at once, and with many more, a caller that shows
/// the key is answered and its call is done. And a stranger is soon hung up on, not kept for as
/// long as a caller may take to say what it wants.
#[test]
fn silent_strangers_keep_no_caller_out() {
    let pad = paddock(false);
    let alpha = pad.project("alpha");
    pad.core.radio_turn(true).unwrap();
    let endpoint: Endpoint = serde_json::from_str(&fs::read_to_string(pad.core.radio_file()).unwrap()).unwrap();
    let hello = serde_json::to_string(&Hello { v: mcp::ENDPOINT_VERSION, key: endpoint.key.clone() }).unwrap();
    let card = |title: &str| serde_json::to_string(&Call { cwd: alpha.clone(), ppid: 0, tool: "board_add".into(), args: json!({ "title": title }) }).unwrap();
    let silent = |count: usize| -> Vec<TcpStream> { (0..count).map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap()).collect() };

    let mut held = Vec::new();
    for strangers in [CALLERS_MAX, 4 * CALLERS_MAX] {
        held.extend(silent(strangers));
        let title = format!("past {strangers} strangers");
        let said = say(endpoint.port, &[hello.clone(), card(&title)]);
        let mut lines = said.lines();
        let welcome = lines.next().map(|line| serde_json::from_str::<Welcome>(line).unwrap().answer);
        assert_eq!(welcome, Some(endpoint.answer.clone()), "{strangers} silent strangers kept the caller out");
        let reply: Reply = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert!(!reply.error, "{}", reply.text);
        assert_eq!(pad.cards().last(), Some(&(alpha.clone(), title)));
    }

    // Every one of them is hung up on in a moment, well before a caller's time is up.
    for mut stranger in held {
        stranger.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let read = stranger.read(&mut [0u8; 1]);
        assert!(matches!(read, Ok(0)) || read.as_ref().is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset), "{read:?}");
    }
    pad.core.radio_turn(false).unwrap();
}
