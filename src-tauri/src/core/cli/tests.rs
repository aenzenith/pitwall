//! The command line's gates, end to end on a core with temp folders: who the line answers, what
//! `run` never runs, and what a link may do.

use std::io::Write;

use super::*;
use crate::settings::CustomCommand;

struct Paddock {
    tmp: tempfile::TempDir,
    core: Arc<Core>,
    /// What the core announced, in order: `state`, and `reveal <path>` for a project to show.
    /// Asked for by the links' tests only, which are macOS's.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    heard: Arc<Mutex<Vec<String>>>,
}

impl Paddock {
    /// A listed project, in a folder of its own.
    fn project(&self, name: &str) -> String {
        let dir = self.tmp.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        let path = fs::canonicalize(dir).unwrap().to_string_lossy().into_owned();
        self.core.add_project(&path).unwrap();
        path
    }

    /// One call as the terminal's end makes it, and everything said back.
    fn call(&self, cwd: &str, project: Option<&str>, ask: Ask) -> Vec<Said> {
        let mut said = Vec::new();
        let call = Call { cwd: cwd.to_string(), project: project.map(str::to_string), ask };
        let mut note = |line| {
            said.push(line);
            Ok(())
        };
        self.core.cli_call(&call, &mut note, &|| Presence::Here).unwrap();
        said
    }

    /// The same, from a caller that is there as long as `waits` says.
    fn call_while(&self, cwd: &str, ask: Ask, waits: impl Fn() -> Presence) -> Vec<Said> {
        let mut said = Vec::new();
        let call = Call { cwd: cwd.to_string(), project: None, ask };
        let mut note = |line| {
            said.push(line);
            Ok(())
        };
        self.core.cli_call(&call, &mut note, &waits).unwrap();
        said
    }

    /// Whether the project's command `id` runs.
    fn runs(&self, path: &str, id: &str) -> bool {
        self.core.lock().jobs.contains_key(&job_key(path, id))
    }

    /// What the core announced since this was last asked.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn heard(&self) -> Vec<String> {
        std::mem::take(&mut *self.heard.lock().unwrap())
    }

    fn cards(&self) -> Vec<(String, String)> {
        self.core.board_state().into_iter().map(|card| (card.path, card.title)).collect()
    }
}

fn paddock() -> Paddock {
    let tmp = tempfile::tempdir().unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let notes = Arc::clone(&heard);
    let core = Core::new(
        CoreConfig {
            registry_dir: tmp.path().join("registry"),
            claude_dir: tmp.path().join("claude").join("projects"),
            settings_file: tmp.path().join("config").join("settings.json"),
            claude_settings: tmp.path().join("claude").join("settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        },
        Arc::new(move |event| match event {
            CoreEvent::State => notes.lock().unwrap().push("state".to_string()),
            CoreEvent::RevealProject { path, .. } => notes.lock().unwrap().push(format!("reveal {path}")),
            _ => {}
        }),
    );
    Paddock { tmp, core, heard }
}

/// One line to the port: what is sent, then everything it says until it closes. A port nobody
/// listens on any more says nothing.
fn say(port: u16, lines: &[String]) -> String {
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else {
        return String::new();
    };
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    for line in lines {
        // A caller the line has closed on already can't be written to; what it said counts.
        let _ = stream.write_all(format!("{line}\n").as_bytes());
    }
    let mut said = String::new();
    let _ = stream.read_to_string(&mut said);
    said
}

/// The line is there only while the command is installed, its file is the user's alone, a
/// caller that doesn't show the file's key is told nothing and gets nothing done, and one of
/// another version is told so and gets nothing done either.
#[test]
fn the_line_answers_only_who_shows_its_key_in_its_version() {
    use std::os::unix::fs::PermissionsExt;

    let pad = paddock();
    let alpha = pad.project("alpha");
    let file = pad.core.cli_file();

    // Not installed: no line, and a file a run before left is taken away.
    fs::write(&file, "{}").unwrap();
    pad.core.cli_start();
    assert!(!file.exists());

    pad.core.cli_turn(true).unwrap();
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let line: LineFile = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let hello = |v: u32, key: &str| serde_json::to_string(&Hello { v, key: key.into() }).unwrap();
    let card = serde_json::to_string(&Call { cwd: alpha.clone(), project: None, ask: Ask::Card { title: "a card".into(), note: String::new() } }).unwrap();

    // No key, a wrong key, the key's start, the other end's secret, no hello, no JSON: not a
    // word back.
    let strangers = [
        hello(LINE_VERSION, ""),
        hello(LINE_VERSION, "0".repeat(32).as_str()),
        hello(LINE_VERSION, &line.key[..31]),
        hello(LINE_VERSION, &line.answer),
        card.clone(),
        "GET / HTTP/1.1".to_string(),
    ];
    for first in strangers {
        assert_eq!(say(line.port, &[first.clone(), card.clone()]), "", "{first}");
    }
    assert!(pad.cards().is_empty(), "a stranger's call did nothing");

    // The key, in another version (a hello that names none counts as one): told so, nothing done.
    for first in [hello(LINE_VERSION + 1, &line.key), hello(0, &line.key), format!(r#"{{"key":"{}"}}"#, line.key)] {
        let said = say(line.port, &[first.clone(), card.clone()]);
        let last: Said = serde_json::from_str(said.lines().last().unwrap()).unwrap();
        assert_eq!(last, Said::Failed { text: t!("core.cli.otherVersion") }, "{first}");
    }
    assert!(pad.cards().is_empty(), "a call of another version did nothing");

    // The key, in this version: the answer the file promises, then the call's.
    let said = say(line.port, &[hello(LINE_VERSION, &line.key), card.clone()]);
    let mut lines = said.lines();
    let welcome: Welcome = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(welcome.answer, line.answer);
    assert_eq!(lines.last().map(|last| serde_json::from_str::<Said>(last).unwrap()), Some(Said::Done));
    assert_eq!(pad.cards(), [(alpha.clone(), "a card".to_string())]);

    // Taken out again: the file goes, and the key that worked opens nothing.
    pad.core.cli_turn(false).unwrap();
    assert!(!file.exists());
    assert!(say(line.port, &[hello(LINE_VERSION, &line.key), card]).is_empty());
    assert_eq!(pad.cards().len(), 1);
}

/// `run` takes a saved command's name and nothing else, never runs a production build, and
/// runs a command marked "ask first" only once the terminal's end says the user agreed.
#[test]
fn run_refuses_a_build_and_what_asks_first() {
    let pad = paddock();
    let alpha = pad.project("alpha");
    let command = |id: &str, line: &str, confirm: bool| CustomCommand { id: id.into(), name: id.into(), command: line.into(), confirm, ..Default::default() };
    let commands = vec![
        command("safe", "echo ran > safe.txt", false),
        command("ask", "echo ran > ask.txt", true),
        command("build", "echo ran > build.txt; npm run build", false),
        command("askbuild", "echo ran > askbuild.txt; npx vite build", true),
    ];
    pad.core.set_project_settings(&alpha, ProjectSettings { commands: commands.clone(), ..Default::default() });
    let ran = |name: &str| Path::new(&alpha).join(format!("{name}.txt")).exists();
    // Agreed to: the terminal's end says the yes was for the line the command has.
    let line = |name: &str| commands.iter().find(|command| command.name == name).map_or_else(|| name.to_string(), |command| command.command.clone());
    let run = |name: &str, confirmed: bool| pad.call(&alpha, None, Ask::Run { name: name.into(), confirmed: confirmed.then(|| line(name)) });
    let refused = |said: &[Said]| matches!(said, [Said::Failed { text }] if !text.is_empty());

    // A build: refused, agreed to or not.
    for (name, confirmed) in [("build", false), ("build", true), ("askbuild", false), ("askbuild", true)] {
        assert!(refused(&run(name, confirmed)), "{name} {confirmed}");
    }
    // "Ask first", and nobody was: it only says that it asks.
    assert_eq!(run("ask", false), [Said::Confirm { name: "ask".into(), command: "echo ran > ask.txt".into() }]);
    // A command line of the caller's is no saved command's name.
    assert!(refused(&run("echo ran > safe.txt", true)));
    assert!(refused(&run("safe; echo ran > safe.txt", true)));
    assert!(!ran("safe") && !ran("ask") && !ran("build") && !ran("askbuild"));

    // The ones that may run do, so the refusals above are the gates' and no accident.
    assert_eq!(run("safe", false).last(), Some(&Said::Done));
    assert_eq!(run("ask", true).last(), Some(&Said::Done));
    assert!(ran("safe") && ran("ask") && !ran("build") && !ran("askbuild"));
}

/// A link does one of its four things to a listed project, only while links are on (they start
/// off), and nothing at all otherwise: no card, no command, no start of a project it doesn't
/// name whole.
#[cfg(target_os = "macos")]
#[test]
fn a_link_acts_only_while_links_are_on_and_only_on_a_listed_project() {
    let pad = paddock();
    let alpha = pad.project("alpha");
    pad.project("alpha-two");
    let start = scheme::text(scheme::Verb::Act(Action::Start), Some("alpha"));
    let nothing: [&str; 0] = [];

    // Off until the user turns them on. A start is announced before `open_link` is back (its
    // spinner), so nothing heard is nothing done.
    assert!(!pad.core.settings().cli.links);
    pad.heard();
    for url in [start.as_str(), "pitwall://show", "pitwall://show?project=alpha", "pitwall://stop?project=alpha"] {
        assert!(!pad.core.open_link(url), "{url}");
    }
    assert_eq!(pad.heard(), nothing, "links are off");

    pad.core.cli_links(true);
    pad.heard();
    for url in ["pitwall://start?project=alp", "pitwall://start?project=ALPHA", "pitwall://start?project=beta", "pitwall://start", "pitwall://run?project=alpha", "pitwall://card?project=alpha"] {
        assert!(!pad.core.open_link(url), "{url}");
    }
    assert_eq!(pad.heard(), nothing, "no link named a listed project whole");
    assert!(pad.cards().is_empty());

    // The window comes up for `show` alone; a project's `show` is that project's, shown.
    assert!(pad.core.open_link("pitwall://show"));
    assert!(!pad.core.open_link("pitwall://show?project=alpha"));
    assert_eq!(pad.heard(), [format!("reveal {alpha}")]);

    // A start: the project it names and no other (it has no dev script, which is how it ends).
    assert!(!pad.core.open_link(&start));
    assert_eq!(pad.heard().first().map(String::as_str), Some("state"));
    assert!(process::wait_until(Duration::from_secs(10), || pad.core.lock().issues.contains_key(&alpha)));
    assert_eq!(pad.core.lock().issues.keys().collect::<Vec<_>>(), [&alpha]);
}

/// The yes is for the command line the question showed. Edited in Settings meanwhile, the saved
/// command of that name is another line: it doesn't run on that yes, and is asked about again.
#[test]
fn a_yes_is_for_the_line_that_was_shown() {
    let pad = paddock();
    let alpha = pad.project("alpha");
    let command = |line: &str| CustomCommand { id: "ask".into(), name: "ask".into(), command: line.into(), confirm: true, ..Default::default() };
    let saved = |line: &str| pad.core.set_project_settings(&alpha, ProjectSettings { commands: vec![command(line)], ..Default::default() });
    let run = |confirmed: Option<&str>| pad.call(&alpha, None, Ask::Run { name: "ask".into(), confirmed: confirmed.map(str::to_string) });
    let ran = |name: &str| Path::new(&alpha).join(format!("{name}.txt")).exists();
    let asks = |line: &str| [Said::Confirm { name: "ask".into(), command: line.into() }];

    saved("echo ran > shown.txt");
    assert_eq!(run(None), asks("echo ran > shown.txt"));

    saved("echo ran > other.txt");
    assert_eq!(run(Some("echo ran > shown.txt")), asks("echo ran > other.txt"), "the yes was for another line");
    assert_eq!(run(Some("")), asks("echo ran > other.txt"));
    assert!(!ran("shown") && !ran("other"));

    assert_eq!(run(Some("echo ran > other.txt")).last(), Some(&Said::Done));
    assert!(ran("other") && !ran("shown"));
}

/// `stop --command` stops a command of the call's project, the one it names or the one of its
/// folder, and never another project's: not one of the same name, not one only another has.
#[test]
fn stop_command_stops_only_its_own_projects_command() {
    let pad = paddock();
    let alpha = pad.project("alpha");
    let beta = pad.project("beta");
    let worker = |id: &str| CustomCommand { id: id.into(), name: id.into(), command: "sleep 30".into(), keep_running: true, ..Default::default() };
    pad.core.set_project_settings(&alpha, ProjectSettings { commands: vec![worker("worker")], ..Default::default() });
    pad.core.set_project_settings(&beta, ProjectSettings { commands: vec![worker("worker"), worker("queue")], ..Default::default() });
    for (path, id) in [(&alpha, "worker"), (&beta, "worker"), (&beta, "queue")] {
        assert!(pad.core.run_command(path, id) && pad.runs(path, id), "{path} {id}");
    }
    let stop = |cwd: &str, project: Option<&str>, name: &str| pad.call(cwd, project, Ask::StopCommand { name: name.into() });

    // From alpha's folder: alpha's worker, not beta's of the same name, and nothing for a name
    // only beta has.
    assert_eq!(stop(&alpha, None, "worker").last(), Some(&Said::Done));
    assert!(!pad.runs(&alpha, "worker") && pad.runs(&beta, "worker"));
    assert!(matches!(stop(&alpha, None, "queue").as_slice(), [Said::Failed { text }] if !text.is_empty()));
    assert!(pad.runs(&beta, "queue"));

    // Beta's, from its folder or by its name: so the above was the gate's and no accident.
    assert_eq!(stop(&beta, None, "worker").last(), Some(&Said::Done));
    assert_eq!(stop(&alpha, Some("beta"), "queue").last(), Some(&Said::Done));
    assert!(!pad.runs(&beta, "worker") && !pad.runs(&beta, "queue"));
}

/// A run goes with its caller (^C, the terminal closed): what the call started is stopped, and
/// its end is still said. Nothing else is: not a command it found running, not one whose output
/// was only followed, and not a run whose line was closed (the command taken out).
#[test]
fn a_caller_that_leaves_stops_only_the_run_it_started() {
    let pad = paddock();
    let alpha = pad.project("alpha");
    let command = |id: &str, keep_running: bool| CustomCommand { id: id.into(), name: id.into(), command: "sleep 30".into(), keep_running, ..Default::default() };
    pad.core.set_project_settings(&alpha, ProjectSettings { commands: vec![command("task", false), command("worker", true)], ..Default::default() });
    // Leaves once the command it runs is under way, as ^C does.
    let leaves_at = |id: &'static str, how: Presence| {
        let pad = &pad;
        let alpha = alpha.clone();
        move || if pad.runs(&alpha, id) { how } else { Presence::Here }
    };

    let said = pad.call_while(&alpha, Ask::Run { name: "task".into(), confirmed: None }, leaves_at("task", Presence::Left));
    assert!(!pad.runs(&alpha, "task"));
    assert!(pad.core.lock().job_results.get(&job_key(&alpha, "task")).is_some_and(|result| result.stopped));
    assert!(matches!(said.as_slice(), [.., Said::Text { tone: Tone::Bad, .. }, Said::Failed { .. }]), "{said:?}");

    // The line closed under it: the run goes on.
    let said = pad.call_while(&alpha, Ask::Run { name: "task".into(), confirmed: None }, leaves_at("task", Presence::Closed));
    assert!(pad.runs(&alpha, "task"), "{said:?}");
    pad.core.stop_command(&alpha, "task");

    // Started elsewhere: neither a follow nor a run that finds it running takes it down.
    assert!(pad.core.run_command(&alpha, "worker"));
    let left = || Presence::Left;
    pad.call_while(&alpha, Ask::Logs { command: Some("worker".into()), follow: true }, left);
    assert!(matches!(pad.call_while(&alpha, Ask::Run { name: "worker".into(), confirmed: None }, left).as_slice(), [Said::Failed { .. }]));
    assert!(pad.runs(&alpha, "worker"));
    pad.core.stop_command(&alpha, "worker");
}

/// A name two listed projects share is nobody's: a link on it does nothing.
#[cfg(target_os = "macos")]
#[test]
fn a_link_on_a_name_two_projects_share_does_nothing() {
    let pad = paddock();
    pad.project("a/dup");
    pad.project("b/dup");
    let names: Vec<String> = pad.core.snapshot().projects.into_iter().map(|project| project.name).collect();
    assert_eq!(names, ["dup", "dup"]);
    let nothing: [&str; 0] = [];

    pad.core.cli_links(true);
    pad.heard();
    for url in ["pitwall://start?project=dup", "pitwall://show?project=dup", "pitwall://restart?project=dup"] {
        assert!(!pad.core.open_link(url), "{url}");
    }
    assert_eq!(pad.heard(), nothing);
}

/// A link opened again at once (a page that loops, a key held down) does nothing the second
/// time: the same thing to the same project is answered again only after two seconds without it.
#[cfg(target_os = "macos")]
#[test]
fn a_link_opened_again_at_once_is_dropped() {
    let pad = paddock();
    let alpha = pad.project("alpha");
    let beta = pad.project("beta");
    pad.core.cli_links(true);
    pad.heard();
    let show = |name: &str, at: u64| pad.core.open_link_at(&format!("pitwall://show?project={name}"), at);
    let at = 1_000_000;

    show("alpha", at);
    show("alpha", at + 500);
    // Still within two seconds of the last one: a repeat that goes on is never answered.
    show("alpha", at + 2_400);
    assert_eq!(pad.heard(), [format!("reveal {alpha}")]);

    // Another project, or another thing to the same one, is no repeat.
    show("beta", at + 2_500);
    assert_eq!(pad.heard(), [format!("reveal {beta}")]);
    assert!(pad.core.open_link_at("pitwall://show", at + 2_500));
    assert!(!pad.core.open_link_at("pitwall://show", at + 2_600));

    // Two seconds after the last one it is answered again.
    show("alpha", at + 2_400 + 2_000);
    assert_eq!(pad.heard(), [format!("reveal {alpha}")]);
}
