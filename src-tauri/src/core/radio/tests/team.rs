//! What a session reads of an output since its last read, a saved command it stops, and who else
//! works in its project: each its own project's alone, and none of the laps'.

use super::*;

/// What `server_output` answered: the lines it handed, and what its notes said of the rest.
#[derive(Debug, Default)]
struct Answer {
    lines: Vec<String>,
    /// Lines that came and went before the read.
    missed: usize,
    /// New lines left out for `lines`.
    left: usize,
    first: bool,
    new_run: bool,
    nothing: bool,
}

fn answer(reply: &Reply) -> Answer {
    assert!(!reply.error, "{}", reply.text);
    let number = |text: &str| text.split(|c: char| !c.is_ascii_digit()).find(|part| !part.is_empty()).and_then(|part| part.parse().ok()).unwrap();
    let mut said = Answer::default();
    for line in reply.text.lines() {
        if line.starts_with('[') {
            if line.contains("came and went") {
                said.missed = number(line);
            } else if let Some(rest) = line.strip_prefix("[left out: ") {
                // New lines `lines` left out, or earlier ones the answer's size did.
                said.left += number(rest);
            } else if line.contains("first read") {
                said.first = true;
            } else if line.contains("a new run began") {
                said.new_run = true;
            } else {
                panic!("a note this test doesn't know: {line}");
            }
        } else if line == "(nothing new since your last read)" || line == "(no output)" {
            said.nothing = true;
        } else {
            said.lines.push(line.to_string());
        }
    }
    said
}

/// `since_read` hands a reader the lines of the output it names that came since it last read
/// that output, and nothing of another project's or another command's (of the same name), whoever
/// else reads them; and whatever happens to the output meanwhile (its oldest lines trimmed in a
/// flood, the output emptied, a new run) it never hands a line twice nor leaves one out without
/// saying so.
#[test]
fn since_read_hands_each_reader_its_own_outputs_new_lines_once() {
    let pad = paddock(true);
    let (alpha, beta) = (pad.project("alpha"), pad.project("beta"));
    let task = |id: &str| CustomCommand { id: id.into(), name: "task".into(), command: "true".into(), ..Default::default() };
    pad.core.set_project_settings(&alpha, ProjectSettings { commands: vec![task("alpha-task")], ..Default::default() });
    pad.core.set_project_settings(&beta, ProjectSettings { commands: vec![task("beta-task")], ..Default::default() });

    // The four outputs, by project and command (none: the server's); both commands are "task".
    let outputs: [(&str, Option<&str>, &str); 4] =
        [(&alpha, None, "alpha server"), (&alpha, Some("alpha-task"), "alpha task"), (&beta, None, "beta server"), (&beta, Some("beta-task"), "beta task")];
    // Readers told apart by their process (no session here): two of Claude's in alpha, one in
    // beta, and one of Codex's in alpha, under a stand-in for its TUI (a process of its name; unix
    // only, its reads skipped elsewhere), which leaves no `claude-read` marker where it read.
    #[allow(unused_mut)]
    #[cfg_attr(not(unix), allow(clippy::useless_vec))]
    let mut callers = vec![Caller { cwd: alpha.clone(), ppid: 3_900_000_001 }, Caller { cwd: alpha.clone(), ppid: 3_900_000_002 }, Caller { cwd: beta.clone(), ppid: 3_900_000_003 }];
    #[cfg(unix)]
    let mut tui = {
        use std::os::unix::process::CommandExt;

        fs::create_dir_all(pad.tmp.path().join("codex")).unwrap();
        assert!(pad.core.look_for_agents());
        let mut settings = pad.core.settings();
        settings.agents.codex.radio = true;
        pad.core.set_settings(settings);
        let tui = std::process::Command::new("/bin/sleep").arg0("codex").arg("30").spawn().unwrap();
        assert!(process::seen_with(tui.id(), "codex"));
        callers.push(Caller { cwd: alpha.clone(), ppid: tui.id() });
        tui
    };
    let (x, z, y, c) = (0, 1, 2, 3);

    enum Step {
        Push(usize, usize),
        /// A reader reads an output, with `since_read` or without, so many lines at most.
        Read(usize, usize, bool, Option<u64>),
        /// The output emptied, as a server moved off a busy port leaves it.
        Empty(usize),
        /// A run of the output ended and the next began (`mark_run`).
        Rerun(usize),
    }
    use Step::*;
    let steps = [
        Push(0, 3),
        Push(2, 2),
        Push(1, 2),
        Push(3, 1),
        Read(x, 0, true, None),
        Read(c, 0, true, None),
        Push(0, 1),
        Push(2, 1),
        Push(1, 1),
        Read(x, 0, true, None),
        Read(x, 1, true, None),
        Read(x, 1, true, None),
        Read(y, 2, true, None),
        Read(y, 3, true, None),
        Read(z, 0, true, None),
        // A flood: the output keeps its last 500 lines.
        Push(0, 700),
        Read(x, 0, true, Some(200)),
        Read(z, 0, true, None),
        Read(c, 0, true, Some(200)),
        // A read without `since_read` moves the reader's place too.
        Push(0, 5),
        Read(x, 0, false, Some(1)),
        Push(0, 2),
        Read(x, 0, true, None),
        // Codex's reader read to the end, past Claude's last marker: where it read is where the
        // output is emptied, and it is told of the new run all the same.
        Read(c, 0, true, None),
        // Emptied, then more than it held before: what came after is all new.
        Empty(0),
        Push(0, 600),
        Read(x, 0, true, Some(200)),
        Read(z, 0, true, Some(200)),
        Read(c, 0, true, Some(200)),
        // The command's run ends with a line nobody read: the next run is read from its beginning.
        Read(c, 1, true, None),
        Push(1, 1),
        Rerun(1),
        Push(1, 2),
        Read(x, 1, true, None),
        Read(c, 1, true, None),
        // Beta's, untouched by all of it.
        Read(y, 3, true, None),
        Read(y, 2, true, None),
        Push(3, 1),
        Read(y, 3, true, None),
    ];

    // As the test pushed them: what each (reader, output) is due, the lines handed so far, who
    // read an output before, and who is due the note that a new run began.
    let mut due: HashMap<(usize, usize), Vec<String>> = HashMap::new();
    let mut handed: HashSet<(usize, String)> = HashSet::new();
    let mut read_before: HashSet<(usize, usize)> = HashSet::new();
    let mut rerun: HashSet<(usize, usize)> = HashSet::new();
    let mut pushed = [0usize; 4];

    for (at, step) in steps.into_iter().enumerate() {
        match step {
            Push(output, count) => {
                let (path, job, name) = outputs[output];
                for _ in 0..count {
                    pushed[output] += 1;
                    let line = format!("{name} {}", pushed[output]);
                    pad.core.push_output(path, job, line.clone());
                    for reader in 0..callers.len() {
                        due.entry((reader, output)).or_default().push(line.clone());
                    }
                }
            }
            Empty(output) | Rerun(output) => {
                let (path, job, _) = outputs[output];
                if matches!(step, Empty(_)) {
                    pad.core.forget_output(path);
                } else {
                    let key = job.map_or_else(|| path.to_string(), |job| job_key(path, job));
                    pad.core.lock().ended.insert(key, (now_ms(), "exited:0".into()));
                    pad.core.mark_run(path, job);
                }
                due.retain(|(_, of), _| *of != output);
                rerun.extend(read_before.iter().filter(|(_, of)| *of == output));
            }
            // Codex's reader, where there is none.
            Read(reader, ..) if reader >= callers.len() => {}
            Read(reader, output, since, lines) => {
                let mut args = json!({ "since_read": since });
                if outputs[output].1.is_some() {
                    args["command"] = json!("task");
                }
                if let Some(lines) = lines {
                    args["lines"] = json!(lines);
                }
                let said = answer(&pad.core.radio_call(&callers[reader], "server_output", &args));
                let due = due.remove(&(reader, output)).unwrap_or_default();
                let first = read_before.insert((reader, output));
                let new_run = rerun.remove(&(reader, output));

                for line in &said.lines {
                    assert!(line.starts_with(&format!("{} ", outputs[output].2)), "step {at}: \"{line}\" is another output's");
                    assert!(handed.insert((reader, line.clone())) || !since, "step {at}: \"{line}\" was handed before");
                }
                if !since {
                    continue;
                }
                // The newest of what was due, and the rest of it said: nothing left out unsaid.
                assert_eq!(said.lines, due[due.len() - said.lines.len()..], "step {at}");
                assert_eq!(said.missed + said.left + said.lines.len(), due.len(), "step {at}: {said:?}, {} due", due.len());
                assert_eq!((said.first, said.new_run, said.nothing), (first, new_run, due.is_empty() && !new_run), "step {at}: {said:?}");
            }
        }
    }

    #[cfg(unix)]
    {
        let _ = tui.kill();
        let _ = tui.wait();
    }
}

/// `stop_command` stops a saved command of the call's project, by its name, and nothing else: not
/// another project's of the same name, not one only another project has (a project inside it
/// included), and never the dev server, whatever it is called.
#[cfg(unix)]
#[test]
fn stop_command_stops_only_its_own_projects_command_and_never_the_server() {
    let pad = paddock(true);
    let (alpha, beta) = (pad.project("alpha"), pad.project("beta"));
    let inner = pad.project("alpha/packages/inner");
    let worker = |id: &str| CustomCommand { id: id.into(), name: id.into(), command: "sleep 30".into(), ..Default::default() };
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let server = format!("require('http').createServer((q, s) => s.end('ok')).listen({port}, '127.0.0.1', () => console.log('Local: http://localhost:{port}/'))");
    fs::write(Path::new(&alpha).join("package.json"), r#"{"name":"alpha","scripts":{"dev":"node server.js"}}"#).unwrap();
    fs::write(Path::new(&alpha).join("server.js"), server).unwrap();
    pad.core.set_project_settings(&alpha, ProjectSettings { port: Some(port), commands: vec![worker("worker")], ..Default::default() });
    pad.core.set_project_settings(&beta, ProjectSettings { commands: vec![worker("worker"), worker("queue")], ..Default::default() });
    pad.core.radio_tool("stop_command", true).unwrap();

    let runs = |path: &str, id: &str| pad.core.lock().jobs.contains_key(&job_key(path, id));
    let server = || pad.core.snapshot().projects.into_iter().find(|p| p.path == alpha).filter(|p| p.status == "running").and_then(|_| pad.core.lock().runs.get(&alpha).map(|run| run.id));
    pad.core.act(&alpha, Action::Start, Origin::User);
    let deadline = Instant::now() + Duration::from_secs(20);
    while server().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let up = server().expect("the dev server came up");
    for (path, id) in [(&alpha, "worker"), (&beta, "worker"), (&beta, "queue")] {
        assert!(pad.core.run_command(path, id) && runs(path, id), "{path} {id}");
    }

    // (from, the name asked for, whether it is refused)
    let rows: [(&str, &str, bool); 11] = [
        (&alpha, "worker", false),
        (&alpha, "queue", true),
        (&inner, "worker", true),
        (&inner, "queue", true),
        (&alpha, "dev", true),
        (&alpha, "server", true),
        (&alpha, "dev server", true),
        (&alpha, "alpha", true),
        (&alpha, "node server.js", true),
        (&alpha, "", true),
        (&alpha, "WORKER", false),
    ];
    let mut wrong = Vec::new();
    for (from, name, refused) in rows {
        let reply = pad.call(from, "stop_command", json!({ "name": name }));
        if reply.error != refused {
            wrong.push(format!("{from} {name:?}: {}", reply.text));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert!(!runs(&alpha, "worker"), "alpha's own was stopped");
    assert!(runs(&beta, "worker") && runs(&beta, "queue"), "beta's weren't");
    assert_eq!(server(), Some(up), "the dev server runs on, the same run");

    // Beta's, from its own folder: so the above was the gate's and no accident.
    for name in ["worker", "queue"] {
        let reply = pad.call(&beta, "stop_command", json!({ "name": name }));
        assert!(!reply.error && reply.text.starts_with("Stopped"), "{}", reply.text);
    }
    assert!(!runs(&beta, "worker") && !runs(&beta, "queue"));

    // Nothing of it outlives the test.
    pad.core.dispose();
}

/// A paddock whose laps hold `sessions`, kept in the runs of a job that ran (`Core::lap_sessions`).
#[cfg(unix)]
fn paddock_with_laps(sessions: &[&str]) -> Paddock {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    fs::create_dir_all(config.join("lap-runs")).unwrap();
    fs::write(config.join("laps.json"), json!({ "laps": [{ "id": "l-team", "name": "nightly", "on": false }] }).to_string()).unwrap();
    let runs: Vec<Value> =
        sessions.iter().enumerate().map(|(at, session)| json!({ "id": format!("r-{at}"), "lap": "l-team", "due": 1, "startedAt": 1, "endedAt": 2, "session": session })).collect();
    fs::write(config.join("lap-runs").join("l-team.json"), Value::Array(runs).to_string()).unwrap();
    let core = Core::new(
        CoreConfig {
            registry_dir: tmp.path().join("registry"),
            claude_dir: tmp.path().join("claude").join("projects"),
            codex_home: tmp.path().join("codex"),
            settings_file: config.join("settings.json"),
            claude_settings: tmp.path().join("claude").join("settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        },
        Arc::new(|_| {}),
    );
    let mut settings = core.settings();
    settings.radio.on = true;
    core.set_settings(settings);
    Paddock { tmp, core }
}

/// `team_status` lists the sessions the caller's project counts (at work, waiting, idle while
/// they run) and no other: not a lap's, which is a layer of its own however it got there, not
/// another project's (one inside it included), and of none its name.
#[cfg(unix)]
#[test]
fn team_status_lists_its_projects_sessions_and_no_laps_or_names() {
    use std::os::unix::process::CommandExt;

    use super::super::super::board::Column;
    use crate::claude::{SessionState, Turn, TurnKind};

    let id = |n: u32| format!("{n:08x}-0000-4000-8000-000000000000");
    let (at_work, waits, idle, lap_works, lap_idle, beta_works, beta_idle, inner_works) = (id(1), id(2), id(3), id(4), id(5), id(6), id(7), id(8));
    let pad = paddock_with_laps(&[&lap_works, &lap_idle]);
    assert!(pad.core.lap_sessions().contains(&lap_works) && pad.core.lap_sessions().contains(&lap_idle));
    let (alpha, beta) = (pad.project("alpha"), pad.project("beta"));
    let inner = pad.project("alpha/packages/inner");
    let deep = pad.folder("alpha/src");

    // The idle ones still run: Claude Code's own record of each, under a process of its name.
    let mut claude = std::process::Command::new("/bin/sleep").arg0("claude").arg("30").spawn().unwrap();
    assert!(process::seen_with(claude.id(), "claude"));
    let sessions = pad.tmp.path().join("claude").join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    for (session, cwd) in [(&idle, &alpha), (&lap_idle, &alpha), (&beta_idle, &beta)] {
        let record = json!({ "pid": claude.id(), "sessionId": session, "cwd": cwd, "entrypoint": "cli", "status": "idle" });
        fs::write(sessions.join(format!("{session}.json")), record.to_string()).unwrap();
    }

    // What the last scan said, each with a name, the laps' too as though they had been set apart
    // by nothing; and the rows it made of them.
    let state = |id: &str, agent: Agent, path: &str, phase: SessionPhase, turn: Option<Turn>, tool: Option<&str>| SessionState {
        id: id.to_string(),
        agent,
        path: path.to_string(),
        folder: Some(path.to_string()),
        title: Some(format!("Secret name of {id}")),
        phase,
        turn,
        since: Some(now_ms() - 60_000),
        hooked: true,
        ended: false,
        tool: tool.map(str::to_string),
        subagents: 0,
        helping: 0,
        interrupted: false,
    };
    let permission = Some(Turn { kind: TurnKind::Permission, at: now_ms() });
    let scanned = vec![
        state(&at_work, Agent::Claude, &alpha, SessionPhase::Working, None, None),
        state(&waits, Agent::Codex, &alpha, SessionPhase::Waiting, permission, Some("Bash")),
        state(&idle, Agent::Claude, &alpha, SessionPhase::Idle, None, None),
        state(&lap_works, Agent::Claude, &alpha, SessionPhase::Working, None, None),
        state(&lap_idle, Agent::Claude, &alpha, SessionPhase::Idle, None, None),
        state(&beta_works, Agent::Claude, &beta, SessionPhase::Working, None, None),
        state(&beta_idle, Agent::Claude, &beta, SessionPhase::Idle, None, None),
        state(&inner_works, Agent::Codex, &inner, SessionPhase::Working, None, None),
    ];
    {
        let mut core = pad.core.lock();
        for session in scanned.iter().filter(|session| session.phase != SessionPhase::Idle) {
            let live = LiveSession { id: session.id.clone(), agent: session.agent, phase: session.phase, turn: session.turn, since: session.since, tool: session.tool.clone(), helpers: 0 };
            core.claude_sessions.entry(session.path.clone()).or_default().push(live);
        }
        core.scanned = scanned;
    }
    // A card each was given: alpha's at work, and beta's.
    for (path, title, session) in [(&alpha, "Fix the login form", &at_work), (&beta, "Beta's own card", &beta_works)] {
        let card = pad.core.board_add(path, title, "", &[]).unwrap();
        let mut board = pad.core.board();
        let held = board.cards.iter_mut().find(|held| held.id == card.id).unwrap();
        (held.column, held.session) = (Column::Claude, Some(session.clone()));
    }

    // (from, the sessions listed, what is said of them)
    let rows: [(&str, usize, &[&str]); 4] = [
        (&alpha, 3, &["- Claude: working since", "on the board card \"Fix the login form\"", "- Codex: waiting on a permission for Bash", "- Claude: idle, in a terminal"]),
        (&deep, 3, &["3 sessions"]),
        (&beta, 2, &["2 sessions"]),
        (&inner, 1, &["- Codex: working since"]),
    ];
    let never: Vec<String> = [&at_work, &waits, &idle, &lap_works, &lap_idle, &beta_works, &beta_idle, &inner_works]
        .into_iter()
        .cloned()
        .chain(["Secret".to_string(), pad.tmp.path().to_string_lossy().into_owned(), "/private/".to_string()])
        .collect();
    let mut wrong = Vec::new();
    for (from, count, said) in rows {
        let reply = pad.call(from, "team_status", json!({}));
        let listed = reply.text.lines().filter(|line| line.starts_with("- ")).count();
        let missing: Vec<&&str> = said.iter().filter(|said| !reply.text.contains(**said)).collect();
        let leaked: Vec<&String> = never.iter().filter(|never| reply.text.contains(never.as_str())).collect();
        if reply.error || listed != count || !missing.is_empty() || !leaked.is_empty() || (from != beta && reply.text.contains("Beta's own card")) {
            wrong.push(format!("from {from}: {listed} listed, missing {missing:?}, leaked {leaked:?}:\n{}", reply.text));
        }
    }

    let _ = claude.kill();
    let _ = claude.wait();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}
