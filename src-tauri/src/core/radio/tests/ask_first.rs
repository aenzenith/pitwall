//! No start runs a command marked "ask first": the radio is one of the starts. Not run on Windows
//! yet: it starts a Node server and stops it from outside, which has run on macOS and Linux only.

use super::super::super::cli::Presence;
use super::*;
use crate::cli::{Agreed, Ask, Companion, Said};

/// A way to start a server, and what it said of it to the command line.
type Start<'a> = Box<dyn Fn() -> Vec<Said> + 'a>;

/// One that says nothing.
fn quiet<'a>(start: impl Fn() + 'a) -> Start<'a> {
    Box::new(move || {
        start();
        Vec::new()
    })
}

/// No start runs a command marked "ask first" that starts with the server, whoever starts it: a
/// click, a team that starts with Pitwall, a restore after an update, what was open at quit, a
/// link, Claude over the radio, the supervisor after a crash, the command line with a terminal
/// and without one. Those not marked run with the server as before (a crash restart runs none:
/// they never stopped). What wasn't run waits for the user as that start left it, the command
/// line's terminal asks about it itself and leaves nothing, and it goes when the server stops. A
/// yes from the terminal runs it only to the line it was shown.
#[test]
fn no_start_runs_a_companion_marked_ask_first() {
    use crate::settings::Team;

    let pad = paddock(true);
    let alpha = pad.project("alpha");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let server = format!("require('http').createServer((q, s) => s.end('ok')).listen({port}, '127.0.0.1', () => console.log('Local: http://localhost:{port}/'))");
    fs::write(Path::new(&alpha).join("package.json"), r#"{"name":"alpha","scripts":{"dev":"node server.js"}}"#).unwrap();
    fs::write(Path::new(&alpha).join("server.js"), server).unwrap();
    let line = |id: &str| format!("echo ran > {id}.txt");
    let command = |id: &str, confirm: bool, with_server: bool| CustomCommand { id: id.into(), name: id.into(), command: line(id), confirm, with_server, ..Default::default() };
    let commands = vec![command("plain", false, true), command("ask", true, true), command("solo", true, false)];
    pad.core.set_project_settings(&alpha, ProjectSettings { port: Some(port), commands, ..Default::default() });
    pad.core.radio_tool("server_restart", true).unwrap();
    let mut settings = pad.core.settings();
    settings.cli.links = true;
    settings.teams = vec![Team { id: "team".into(), name: "team".into(), projects: vec![alpha.clone()], collapsed: false, start_at_launch: true }];
    pad.core.set_settings(settings);
    let config = pad.tmp.path().join("config");

    let file = |id: &str| Path::new(&alpha).join(format!("{id}.txt"));
    let ran = |id: &str| {
        let key = job_key(&alpha, id);
        let inner = pad.core.lock();
        file(id).exists() || inner.jobs.contains_key(&key) || inner.job_results.contains_key(&key)
    };
    let held = || pad.core.snapshot().projects.into_iter().find(|p| p.path == alpha).and_then(|p| p.commands.into_iter().find(|c| c.id == "ask")?.held);
    let run_id = || pad.core.lock().runs.get(&alpha).map(|run| run.id);
    // The start is over, its spinner gone: a restart that hasn't settled leaves the row busy
    // after a stop that went around the window (`Core::stop`), and a team skips a busy project.
    let at_rest = || pad.core.snapshot().projects.into_iter().find(|p| p.path == alpha).is_some_and(|p| p.status != "busy");
    let until = |done: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
    };
    let cli = |ask: Ask| {
        let mut said = Vec::new();
        let call = crate::cli::Call { cwd: alpha.clone(), project: None, ask };
        let mut note = |line| {
            said.push(line);
            Ok(())
        };
        pad.core.cli_call(&call, &mut note, &|| Presence::Here).unwrap();
        said
    };
    let skipped = t!("core.cli.act.skipped", name = "ask");

    // (who starts it, how, whether the server is stopped after, whether the plain one runs, what
    // the window shows of the one that asks first)
    let mut rows: Vec<(&str, Start, bool, bool, Option<Origin>)> = vec![
        ("click", quiet(|| pad.core.act(&alpha, Action::Start, Origin::User)), false, true, Some(Origin::User)),
        ("click on restart", quiet(|| pad.core.act(&alpha, Action::Restart, Origin::User)), false, true, Some(Origin::User)),
        (
            "crash",
            quiet(|| {
                let pid = pad.core.lock().runs[&alpha].pid;
                assert!(process::kill_tree(pid, true));
            }),
            true,
            false,
            Some(Origin::Crash),
        ),
        ("team at launch", quiet(|| pad.core.start_launch_teams()), true, true, Some(Origin::Launch)),
        (
            "restore after an update",
            quiet(|| {
                let record = json!({ "v": 1, "at": now_ms(), "version": "0", "window": false, "servers": [alpha], "commands": [], "tabs": [] });
                fs::write(config.join("restore.json"), record.to_string()).unwrap();
                assert!(pad.core.restore().is_some());
            }),
            true,
            true,
            Some(Origin::Restore),
        ),
        (
            "what was open at quit",
            quiet(|| {
                let record = json!({ "v": 1, "at": now_ms(), "projects": [{ "path": alpha, "server": true }] });
                fs::write(config.join("last-session.json"), record.to_string()).unwrap();
                pad.core.resume_start(false);
                pad.core.resume_restore(false);
            }),
            true,
            true,
            Some(Origin::Restore),
        ),
        ("claude", quiet(|| assert!(!pad.call(&alpha, "server_restart", json!({ "action": "start" })).error)), false, true, Some(Origin::Claude)),
        ("claude on restart", quiet(|| assert!(!pad.call(&alpha, "server_restart", json!({ "action": "restart" })).error)), true, true, Some(Origin::Claude)),
        ("terminal", Box::new(|| cli(Ask::Act { action: "start".into(), terminal: true })), true, true, None),
        ("no terminal", Box::new(|| cli(Ask::Act { action: "start".into(), terminal: false })), true, true, Some(Origin::NoTerminal)),
    ];
    // Links are answered on macOS only (`cli::SUPPORTED`).
    if crate::cli::SUPPORTED {
        rows.push(("link", quiet(|| assert!(!pad.core.open_link("pitwall://start?project=alpha"))), true, true, Some(Origin::Link)));
    }

    let mut seen = Vec::new();
    let mut want = Vec::new();
    for (who, start, stop, plain, shown) in &rows {
        let _ = fs::remove_file(file("plain"));
        let before = run_id();
        let said = start();
        until(&|| run_id().is_some_and(|id| Some(id) != before) && (!plain || file("plain").exists()) && held() == *shown && at_rest());
        thread::sleep(Duration::from_millis(300));
        // The terminal is asked about it, and nothing else is; without one it is said so.
        let asked = matches!(said.last(), Some(Said::Companions { commands }) if commands.iter().map(|c| c.id.as_str()).eq(["ask"]));
        let told = said.iter().any(|said| matches!(said, Said::Text { text, .. } if *text == skipped));
        seen.push((*who, file("plain").exists(), ran("ask"), held(), asked, told));
        want.push((*who, *plain, false, *shown, *who == "terminal", *who == "no terminal"));
        if *stop {
            pad.core.stop(&alpha);
            seen.push((*who, false, ran("ask"), held(), false, false));
            want.push((*who, false, false, None, false, false));
        }
    }

    // The yes, after a start at a terminal: to a line that changed since it is no yes, a command
    // that doesn't start with the server is none of its business, and the line shown runs.
    let yes = |id: &str, line: &str| cli(Ask::Companions { agreed: vec![Agreed { id: id.into(), command: line.into() }] });
    let again = yes("ask", "echo changed");
    let other = yes("solo", &line("solo"));
    thread::sleep(Duration::from_millis(300));
    let unasked = (ran("ask"), ran("solo"));
    let agreed = yes("ask", &line("ask"));
    until(&|| file("ask").exists());

    // Nothing of it outlives the test, whatever it found.
    pad.core.dispose();
    assert_eq!(seen, want, "(who, plain ran, ask ran, shown, asked at the terminal, told it was skipped)");
    assert_eq!(again, [Said::Companions { commands: vec![Companion { id: "ask".into(), name: "ask".into(), command: line("ask") }] }]);
    assert_eq!(other, [Said::Done]);
    assert_eq!(unasked, (false, false), "(ask ran, solo ran) before a yes to their line");
    assert_eq!(agreed.last(), Some(&Said::Done));
    assert!(file("ask").exists());
}
