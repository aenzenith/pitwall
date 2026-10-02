//! End to end: real `npm run dev` servers through the login shell, a fake VS Code window in the
//! shared registry, and the routing between them.

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::registry::{now_ms, ProjectState, WindowRecord};

const SERVER_JS: &str = r#"
const i = process.argv.indexOf('--port');
const port = i > 0 ? Number(process.argv[i + 1]) : Number(require('fs').readFileSync(__dirname + '/port.txt', 'utf8'));
require('http').createServer((q, s) => s.end('ok')).listen(port, '127.0.0.1', () => console.log(`  ➜  Local:   http://localhost:${port}/`));
"#;

const CRASH_JS: &str = "console.error(\"Error: Cannot find module 'vite'\"); process.exit(1);";

struct Fixture {
    _tmp: tempfile::TempDir,
    core: Arc<Core>,
    registry_dir: std::path::PathBuf,
    project: String,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn fixture(script: &str, files: &[(&str, &str)]) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("paddock");
    let registry_dir = tmp.path().join("registry");

    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.json"), format!(r#"{{"name":"paddock","scripts":{{"dev":"{script}"}}}}"#)).unwrap();
    for (name, content) in files {
        fs::write(project.join(name), content).unwrap();
    }

    let core = Core::new(
        CoreConfig {
            registry_dir: registry_dir.clone(),
            claude_dir: tmp.path().join("claude"),
            settings_file: tmp.path().join("settings.json"),
            claude_settings: tmp.path().join("claude-settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        },
        Arc::new(|_| {}),
    );

    Fixture { project: project.to_string_lossy().into_owned(), core, registry_dir, _tmp: tmp }
}

fn view(core: &Core, path: &str) -> ProjectView {
    core.snapshot().projects.into_iter().find(|p| p.path == path).expect("project listed")
}

fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    done()
}

fn server_fixture() -> (Fixture, u16) {
    let port = free_port();
    let fx = fixture("node server.js", &[("server.js", SERVER_JS), ("port.txt", &port.to_string())]);

    fx.core.add_project(&fx.project).unwrap();
    fx.core.set_project_settings(&fx.project, ProjectSettings { port: Some(port), ..Default::default() });
    (fx, port)
}

fn fake_window(dir: &Path, id: &str, project: &str, running: bool, root: bool) {
    let record = WindowRecord {
        window_id: id.into(),
        title: "paddock — VS Code".into(),
        updated_at: now_ms(),
        projects: vec![ProjectState {
            folder_path: project.into(),
            name: "paddock".into(),
            running,
            port: running.then_some(5999),
            url: None,
            started_at: None,
            issue: None,
            output: None,
        }],
        roots: Some(if root { vec![project.into()] } else { vec![] }),
    };
    fs::write(dir.join("windows").join(format!("{id}.json")), serde_json::to_string(&record).unwrap()).unwrap();
}

fn commands_for(dir: &Path, id: &str) -> Vec<RemoteCommand> {
    fs::read_dir(dir.join("commands"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&format!("{id}__")))
        .map(|e| serde_json::from_str(&fs::read_to_string(e.path()).unwrap()).unwrap())
        .collect()
}

#[test]
fn a_drag_keeps_the_place_of_projects_that_are_not_listed_right_now() {
    let v = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // `x` belongs after `b`; it isn't listed while its window is closed.
    assert_eq!(merge_order(&v(&["a", "b", "x", "c"]), v(&["c", "a", "b"])), v(&["c", "a", "b", "x"]));
    // Hidden ones at the top stay at the top, in their order.
    assert_eq!(merge_order(&v(&["x", "y", "a", "b"]), v(&["b", "a"])), v(&["x", "y", "b", "a"]));
    // Nothing hidden: the drag is the order.
    assert_eq!(merge_order(&v(&["a", "b"]), v(&["b", "a", "n"])), v(&["b", "a", "n"]));
}

#[test]
fn a_server_starts_and_stopping_frees_the_port_of_the_whole_tree() {
    let (fx, port) = server_fixture();

    fx.core.act(&fx.project, Action::Start);

    assert!(wait_for(Duration::from_secs(30), || view(&fx.core, &fx.project).url.is_some()), "server never printed its URL");
    assert!(is_port_served(port));

    let running = view(&fx.core, &fx.project);
    assert_eq!(running.status, "running");
    assert_eq!(running.port, Some(port));
    assert!(fx.core.output(&fx.project).iter().any(|line| line.contains("npm run dev")));

    // npm → sh → node: the node grandchild holds the port; only a process-group kill frees it.
    fx.core.stop(&fx.project);

    assert_eq!(view(&fx.core, &fx.project).status, "stopped");
    assert!(wait_for(Duration::from_secs(5), || !is_port_served(port)), "the server outlived stop");
}

#[test]
fn quitting_leaves_no_server_behind() {
    let (fx, port) = server_fixture();

    fx.core.start(&fx.project, false);
    assert!(wait_for(Duration::from_secs(30), || is_port_served(port)));

    fx.core.dispose();

    assert!(wait_for(Duration::from_secs(5), || !is_port_served(port)), "a server survived quit");
}

#[test]
fn a_crashing_server_is_restarted_three_times_then_given_up() {
    let fx = fixture("node crash.js", &[("crash.js", CRASH_JS)]);
    fx.core.add_project(&fx.project).unwrap();

    fx.core.start(&fx.project, false);

    let gave_up = wait_for(Duration::from_secs(40), || {
        view(&fx.core, &fx.project).issue.is_some_and(|issue| issue.text.contains("gave up after 3 restarts"))
    });

    assert!(gave_up, "never gave up: {:?}", view(&fx.core, &fx.project).issue);

    let issue = view(&fx.core, &fx.project).issue.unwrap();
    assert_eq!(issue.kind, "crashed");
    assert!(issue.text.contains("Cannot find module 'vite'"), "{}", issue.text);
    assert_eq!(view(&fx.core, &fx.project).status, "crashed");
    assert_eq!(fx.core.output(&fx.project).iter().filter(|l| l.contains("process ended")).count(), 1, "output keeps the last run only");
}

#[test]
fn build_scripts_are_refused_and_nothing_runs() {
    let fx = fixture("node server.js", &[]);
    fx.core.add_project(&fx.project).unwrap();

    let mut settings = fx.core.settings();
    settings.script = "build".into();
    fx.core.set_settings(settings);

    fx.core.start(&fx.project, false);

    let project = view(&fx.core, &fx.project);
    assert_eq!(project.status, "stopped");
    assert!(project.issue.unwrap().text.contains("will not run"));
    assert!(fx.core.output(&fx.project).is_empty());
}

#[test]
fn a_server_running_in_vs_code_is_stopped_there_not_here() {
    let fx = fixture("node server.js", &[]);

    fake_window(&fx.registry_dir, "vscode1", &fx.project, true, true);
    fx.core.heartbeat();

    let project = view(&fx.core, &fx.project);
    assert_eq!(project.status, "running");
    assert_eq!(project.owner.as_ref().map(|o| o.id.as_str()), Some("vscode1"));

    fx.core.act_now(&fx.project, Action::Start);
    assert!(commands_for(&fx.registry_dir, "vscode1").is_empty(), "a running server is not started twice");

    fx.core.act_now(&fx.project, Action::Stop);
    let sent = commands_for(&fx.registry_dir, "vscode1");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].action, "stop");
    assert_eq!(sent[0].issued_by, fx.core.registry.id);
}

#[test]
fn a_project_open_in_vs_code_is_started_by_that_window() {
    let fx = fixture("node server.js", &[]);

    fake_window(&fx.registry_dir, "vscode2", &fx.project, false, true);
    fx.core.heartbeat();
    assert_eq!(view(&fx.core, &fx.project).open_in.as_deref(), Some("paddock — VS Code"));

    fx.core.act_now(&fx.project, Action::Start);

    let sent = commands_for(&fx.registry_dir, "vscode2");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].action, "start");
    assert!(fx.core.output(&fx.project).is_empty(), "the app must not run it itself");
}

#[test]
fn a_start_command_from_vs_code_runs_the_server_here_and_is_published() {
    let (fx, port) = server_fixture();
    let vscode = Registry::with_id(fx.registry_dir.clone(), "VS Code", "vscode3");

    vscode.send(&fx.core.registry.id, "start", &fx.project);
    fx.core.tick(1);

    assert!(wait_for(Duration::from_secs(30), || is_port_served(port)));
    assert!(wait_for(Duration::from_secs(10), || view(&fx.core, &fx.project).url.is_some()));

    // The extension sees it as ours: running, with its port.
    fx.core.heartbeat();
    let ours = vscode.read_peers().into_iter().find(|r| r.window_id == fx.core.registry.id).expect("app published");
    let published = ours.owned().find(|p| p.folder_path == fx.project).expect("server published");
    assert!(published.running);
    assert_eq!(published.port, Some(port));

    fx.core.stop(&fx.project);
    assert!(wait_for(Duration::from_secs(5), || !is_port_served(port)));
}

fn command(id: &str, cmd: &str, keep_running: bool, with_server: bool) -> crate::settings::CustomCommand {
    crate::settings::CustomCommand {
        id: id.into(),
        name: id.into(),
        command: cmd.into(),
        keep_running,
        confirm: false,
        with_server,
    }
}

fn command_view(core: &Core, path: &str, id: &str) -> CommandView {
    view(core, path).commands.into_iter().find(|c| c.id == id).expect("command listed")
}

#[test]
fn custom_commands_run_stop_with_their_tree_and_record_results() {
    let worker_port = free_port();
    let fx = fixture(
        "node server.js",
        &[("server.js", SERVER_JS), ("port.txt", &free_port().to_string()), ("worker.js", &SERVER_JS.replace("port.txt", "worker-port.txt")), ("worker-port.txt", &worker_port.to_string())],
    );
    fx.core.add_project(&fx.project).unwrap();
    fx.core.set_project_settings(
        &fx.project,
        ProjectSettings {
            commands: vec![
                command("worker", "node worker.js", true, false),
                command("fail", "node -e \"process.exit(3)\"", false, false),
                command("build", "npm run build", false, false),
            ],
            ..Default::default()
        },
    );

    // A worker: its grandchild holds a port until the whole tree is stopped.
    fx.core.run_command(&fx.project, "worker");
    assert!(wait_for(Duration::from_secs(30), || is_port_served(worker_port)), "worker never listened");
    assert_eq!(command_view(&fx.core, &fx.project, "worker").status, "running");

    fx.core.stop_command(&fx.project, "worker");
    assert!(wait_for(Duration::from_secs(5), || !is_port_served(worker_port)), "the worker outlived stop");
    let stopped = command_view(&fx.core, &fx.project, "worker");
    assert_eq!(stopped.status, "idle");
    assert!(stopped.result.is_some_and(|r| r.stopped));

    // A one-off that fails: the exit code is kept.
    fx.core.run_command(&fx.project, "fail");
    assert!(wait_for(Duration::from_secs(20), || command_view(&fx.core, &fx.project, "fail").status == "failed"));
    assert_eq!(command_view(&fx.core, &fx.project, "fail").result.and_then(|r| r.code), Some(3));

    // A production build is refused before anything runs.
    fx.core.run_command(&fx.project, "build");
    assert_eq!(command_view(&fx.core, &fx.project, "build").status, "failed");
    assert!(fx.core.command_output(&fx.project, "build").iter().any(|l| l.contains("refused")));
    assert!(!fx.core.command_output(&fx.project, "build").iter().any(|l| l.starts_with("$ ")), "nothing was spawned");
}

#[test]
fn a_companion_command_starts_and_stops_with_the_dev_server() {
    let (fx, port) = server_fixture();
    let worker_port = free_port();
    fs::write(Path::new(&fx.project).join("worker.js"), SERVER_JS.replace("port.txt", "worker-port.txt")).unwrap();
    fs::write(Path::new(&fx.project).join("worker-port.txt"), worker_port.to_string()).unwrap();

    let mut settings = view(&fx.core, &fx.project).settings;
    settings.commands = vec![command("queue", "node worker.js", true, true)];
    fx.core.set_project_settings(&fx.project, settings);

    fx.core.start(&fx.project, false);
    assert!(wait_for(Duration::from_secs(30), || is_port_served(port) && is_port_served(worker_port)), "server and companion both up");

    fx.core.stop(&fx.project);
    assert!(wait_for(Duration::from_secs(5), || !is_port_served(port) && !is_port_served(worker_port)), "both down with the server");
}

#[test]
fn starting_deletes_only_day_files_older_than_kept() {
    let fx = fixture("node server.js", &[]);
    let dir = fx._tmp.path().join("activity");
    fs::create_dir_all(&dir).unwrap();
    for name in ["2020-01-01.jsonl", "2999-01-01.jsonl", "notes.txt", "2020-01-01.json"] {
        fs::write(dir.join(name), "{}\n").unwrap();
    }

    fx.core.record_app("start");

    assert!(!dir.join("2020-01-01.jsonl").exists());
    for kept in ["2999-01-01.jsonl", "notes.txt", "2020-01-01.json"] {
        assert!(dir.join(kept).exists(), "{kept} kept");
    }
}
