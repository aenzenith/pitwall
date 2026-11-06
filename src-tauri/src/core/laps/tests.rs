//! The laps' tests: only what is irreversible and silent (docs/TURLAR.md › Tests).

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

#[allow(unused_imports)]
use super::*;
use super::super::backup::BackupParts;
use super::super::CoreConfig;
use crate::settings::{ClaudeAccount, CustomCommand};
use crate::start::{new_uuid, CodexMode, CodexModel, CodexStart, Mode, Model, Start};

fn core_at(root: &Path) -> Arc<Core> {
    Core::new(
        CoreConfig {
            registry_dir: root.join("registry"),
            claude_dir: root.join("claude").join("projects"),
            codex_home: root.join("codex"),
            settings_file: root.join("config").join("settings.json"),
            claude_settings: root.join("claude").join("settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        },
        Arc::new(|_| {}),
    )
}

/// A job of `kind` in `folder`, its next moment far off.
fn job(id: &str, folder: &Path, kind: LapKind) -> Lap {
    Lap {
        id: id.into(),
        name: id.into(),
        kind,
        place: Place::Folder { path: folder.to_string_lossy().into_owned() },
        prompt: "Look at the folder".into(),
        script: kind.has_script().then(|| Script::Text { text: "exit 0".into() }),
        when: When::Once { at: now_ms() + 86_400_000 },
        next_at: Some(now_ms() + 86_400_000),
        ..Default::default()
    }
}

fn script(text: &str) -> Option<Script> {
    Some(Script::Text { text: text.into() })
}

/// Takes the laps' lock, as the first tick does. A file just let go can stay locked for a moment:
/// a process another test forks at that moment holds it until it execs.
fn lock(laps: &mut Laps) -> bool {
    process::wait_until(Duration::from_secs(5), || {
        laps.lock_tried = None;
        laps.ensure_lock(now_ms()).is_some()
    })
}

/// `lap` held by `core` and written, the laps' lock taken first; a minute of retries is 20 ms.
fn put(core: &Core, lap: Lap) {
    let mut laps = core.laps();
    assert!(lock(&mut laps), "the lock");
    laps.minute_ms = 20;
    laps.laps.retain(|kept| kept.id != lap.id);
    laps.laps.push(lap);
    assert!(laps.save());
}

/// The agent's stand-in: exits with `code`, says `message`; counts its calls.
fn stand_in(core: &Core, code: i32, message: Option<&str>) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let message = message.map(str::to_string);
    core.laps().agent_hook = Some(Arc::new(move |_: &Asked| {
        counted.fetch_add(1, Ordering::SeqCst);
        Said { code, session: None, message: message.clone() }
    }));
    calls
}

/// Runs the job now (Şimdi çalıştır) on this thread; its record.
fn run_here(core: &Arc<Core>, id: &str) -> LapRun {
    let planned = core.lap_begin_now(id, now_ms()).unwrap();
    core.run_lap(planned.clone());
    core.laps().runs[id].iter().find(|run| run.id == planned.run).cloned().unwrap()
}

fn records(core: &Core, id: &str) -> Vec<LapRun> {
    core.laps().runs.get(id).cloned().unwrap_or_default()
}

/// A Claude account beside the main one, its folder made: its id.
fn account(core: &Core, root: &Path, id: &str) -> String {
    let dir = root.join(format!("account-{id}"));
    fs::create_dir_all(&dir).unwrap();
    core.lock().settings.accounts.others.push(ClaudeAccount { id: id.into(), name: id.into(), dir: dir.to_string_lossy().into_owned(), shared: false, sync: false });
    id.to_string()
}

/// A usage reading as `fuel.rs` hands it: the week's share used and the refills (ms).
fn read(week: f64, refills: &[u64]) -> Option<FuelReading> {
    Some((Some(week), refills.to_vec()))
}

/// Claude's readings as `fuel.rs` hands them: each account's by its id, and the one in use.
fn fuel(core: &Core, in_use: &str, readings: &[(&str, Option<FuelReading>)]) {
    core.laps_fuel(Agent::Claude, in_use, readings.iter().map(|(id, reading)| (id.to_string(), reading.clone())).collect());
}

#[test]
fn a_moment_runs_once_and_only_once_it_is_written_down() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let folder = root.join("work");
    fs::create_dir_all(&folder).unwrap();
    let config = root.join("config");
    let core = core_at(root);
    let now = now_ms();
    put(&core, Lap { when: When::Once { at: now - 1_000 }, next_at: Some(now - 1_000), ..job("l-once", &folder, LapKind::Script) });

    let mut started = Vec::new();
    core.laps_due_at(now, &mut |_, planned| {
        // On disk before anything starts: the record with no end, and the job moved on.
        let runs: Vec<LapRun> = serde_json::from_str(&fs::read_to_string(config.join("lap-runs").join("l-once.json")).unwrap()).unwrap();
        assert!(runs.iter().any(|run| run.id == planned.run && run.ended_at.is_none()));
        let saved: serde_json::Value = serde_json::from_str(&fs::read_to_string(config.join("laps.json")).unwrap()).unwrap();
        assert_eq!(saved["laps"][0]["runsDone"], 1);
        assert_ne!(saved["laps"][0]["nextAt"].as_u64(), Some(now - 1_000));
        started.push(planned);
    });
    assert_eq!(started.len(), 1);

    // Pitwall crashed while it went: at the next start its record is cut, and it never runs again.
    core.laps().lock.take();
    drop(core);
    let core = core_at(root);
    core.laps_due_at(now + 3_000, &mut |_, _| {});
    let runs = records(&core, "l-once");
    assert_eq!((runs.len(), runs[0].result, runs[0].reason), (1, Some(RunResult::Cut), Some(Reason::Quit)));

    // A `laps.json` that can't be written: nothing starts, the record says so, and the job keeps
    // its moment, which runs once the file can be written, a one-time job's and one that ends
    // after a run alike.
    let every = When::Every { minutes: 1, from: None, to: None, days: Vec::new() };
    put(&core, Lap { when: When::Once { at: now }, next_at: Some(now), ..job("l-once", &folder, LapKind::Script) });
    put(&core, Lap { when: every, ends: Ends { at: None, after: Some(1) }, next_at: Some(now), ..job("l-after", &folder, LapKind::Script) });
    fs::remove_file(config.join("laps.json")).unwrap();
    fs::create_dir(config.join("laps.json")).unwrap();
    fs::write(config.join("laps.json").join("in-the-way"), "").unwrap();
    for at in [now + 4_000, now + 5_000] {
        core.laps_due_at(at, &mut |_, planned| panic!("{planned:?} started with its moment unwritten"));
    }
    for id in ["l-once", "l-after"] {
        let lap = core.laps().lap(id).cloned().unwrap();
        assert_eq!((lap.on, lap.next_at, lap.runs_done), (true, Some(now), 0), "{id}: as it was");
        assert_eq!(records(&core, id).iter().filter(|run| run.reason == Some(Reason::NotStarted)).count(), 1, "{id}: one record of it");
    }
    fs::remove_dir_all(config.join("laps.json")).unwrap();
    let mut started = Vec::new();
    for at in [now + 6_000, now + 7_000] {
        core.laps_due_at(at, &mut |_, planned| started.push(planned.lap));
    }
    started.sort();
    assert_eq!(started, ["l-after", "l-once"]);
}

#[test]
fn missed_moments_run_once_within_the_grace_and_never_past_it_its_end_or_on_a_day_off() {
    #[derive(Debug, Clone, Copy)]
    enum Since {
        /// The loop ran a second ago.
        Awake,
        /// No second since an hour before the moment (the Mac slept).
        Asleep,
        /// Pitwall was started: the first second after the files were read.
        Started,
    }
    let now = now_ms();
    let minute = 60_000;
    let every = When::Every { minutes: 15, from: None, to: None, days: Vec::new() };
    let late = |when: When, catch_up: bool, grace_min: u32, by: u64| Lap { when, missed: super::types::Missed { catch_up, grace_min }, next_at: Some(now - by), ..job("l-missed", Path::new("/"), LapKind::Script) };
    // (the job, its moment late by, since when the loop didn't run: runs, missed records)
    let cases = [
        // Asleep five hours: twenty moments, one run.
        (late(every.clone(), true, 120, 300 * minute), Since::Asleep, 1, 0),
        (late(When::Once { at: now - 300 * minute }, true, 120, 300 * minute), Since::Started, 0, 1),
        (late(When::Once { at: now - 5 * minute }, false, 120, 5 * minute), Since::Awake, 0, 1),
        (late(When::Once { at: now - 30_000 }, false, 120, 30_000), Since::Awake, 1, 0),
        // Catch-up off: a moment that came while asleep or closed is missed, however little late.
        (late(When::Once { at: now - 30_000 }, false, 120, 30_000), Since::Asleep, 0, 1),
        (late(When::Once { at: now - 30_000 }, false, 120, 30_000), Since::Started, 0, 1),
        // Ended 21 minutes ago, closed since an hour ago: none of the moments within the grace
        // is before its end.
        (Lap { ends: Ends { at: Some(now - 21 * minute), after: None }, ..late(every.clone(), true, 20, 60 * minute) }, Since::Started, 0, 1),
    ];
    for (n, (lap, since, runs, missed)) in cases.into_iter().enumerate() {
        let tmp = tempfile::tempdir().unwrap();
        let grace = if lap.missed.catch_up { u64::from(lap.missed.grace_min) * minute } else { 90_000 };
        let mut core = core_at(tmp.path());
        let moment = lap.next_at.unwrap();
        put(&core, lap);
        match since {
            Since::Awake | Since::Asleep => {
                let mut laps = core.laps();
                laps.woke_at = None;
                laps.last_tick = if matches!(since, Since::Awake) { now - 1_000 } else { moment - 60 * minute };
            }
            Since::Started => {
                core.laps().lock.take();
                core = core_at(tmp.path());
            }
        }
        let mut started = 0;
        for at in [now, now + 1_000, now + 2_000] {
            core.laps_due_at(at, &mut |_, planned| {
                assert_eq!(planned.lap, "l-missed");
                started += 1;
            });
        }
        let kept = records(&core, "l-missed");
        let skipped = kept.iter().filter(|run| run.reason == Some(Reason::Missed)).count();
        assert_eq!((started, skipped), (runs, missed), "case {n}: {since:?}");
        if runs == 1 {
            assert!(now - kept[0].due <= grace, "case {n}: the run is for a moment within the grace");
        }
    }

    // Started with three jobs due at one moment: two start in the first second, and the third,
    // catch-up off, looked at a second later, is missed all the same: its moment came while
    // Pitwall was closed.
    let tmp = tempfile::tempdir().unwrap();
    let core = core_at(tmp.path());
    let at = now - 30_000;
    for (id, catch_up) in [("l-on-1", true), ("l-on-2", true), ("l-off", false)] {
        put(&core, Lap { when: When::Once { at }, missed: super::types::Missed { catch_up, grace_min: 120 }, next_at: Some(at), ..job(id, Path::new("/"), LapKind::Script) });
    }
    core.laps().lock.take();
    let core = core_at(tmp.path());
    let mut started = Vec::new();
    for tick in [now, now + 1_000, now + 2_000] {
        core.laps_due_at(tick, &mut |_, planned| started.push(planned.lap.clone()));
    }
    started.sort();
    assert_eq!(started, ["l-on-1", "l-on-2"], "the third, catch-up off, is not run a second later");
    assert_eq!(records(&core, "l-off").iter().filter(|run| run.reason == Some(Reason::Missed)).count(), 1);

    // "Yalnız bu gün": the job's next moment is past the day, nothing runs on it however many
    // moments it has, and it leaves one record; given back, the day has its moments again.
    let tmp = tempfile::tempdir().unwrap();
    let core = core_at(tmp.path());
    let often = When::Every { minutes: 1, from: None, to: None, days: Vec::new() };
    let at = when::next_after(&often, now_ms(), &[]).unwrap();
    let date = when::date_of(at).0;
    put(&core, Lap { when: often, next_at: Some(at), ..job("l-off", tmp.path(), LapKind::Script) });
    core.lap_day_off("l-off", &date, false).unwrap();
    let next = core.laps().lap("l-off").unwrap().next_at.unwrap();
    assert_ne!(when::date_of(next).0, date, "the next moment is on the day off");
    for tick in (0..5).map(|minute| at + minute * 60_000 + 10).filter(|&tick| when::date_of(tick).0 == date) {
        core.laps_due_at(tick, &mut |_, planned| panic!("{planned:?} ran on its day off"));
    }
    let off = records(&core, "l-off").iter().filter(|run| run.reason == Some(Reason::DayOff)).count();
    assert_eq!(off, 1, "one record for the day off");
    core.lap_day_on("l-off", &date).unwrap();
    assert_eq!(core.laps().lap("l-off").unwrap().next_at.map(|next| when::date_of(next).0), Some(date), "back on the day");
}

#[test]
fn no_agent_is_called_past_a_gate_a_guard_or_a_switch() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("work");
    fs::create_dir_all(&folder).unwrap();
    let core = core_at(tmp.path());
    let calls = stand_in(&core, 0, Some("done"));
    let gated = |id: &str, gate: ScriptGate, text: &str| Lap { gate: Gate { script: gate, ..Gate::default() }, script: script(text), ..job(id, &folder, LapKind::ScriptAgent) };
    let fueled = |id: &str, then: FuelThen| Lap { fuel: Some(FuelGuard { over: 50, then }), ..job(id, &folder, LapKind::Agent) };
    let codex = Lap { start: Start { agent: Agent::Codex, ..Start::default() }, ..job("l-codex", &folder, LapKind::Agent) };
    const MAIN: &str = crate::accounts::MAIN;
    const B: &str = "b0b0";
    account(&core, tmp.path(), B);
    let own = |id: &str, then: FuelThen| Lap { account: Some(B.into()), ..fueled(id, then) };
    // The readings handed just before the run: the account in use past the guard's share and B
    // well under it, or one of them not read yet.
    type Readings = Box<dyn Fn() -> Vec<(&'static str, Option<FuelReading>)>>;
    let over = |refill: u64| -> Readings { Box::new(move || vec![(MAIN, read(80.0, &[now_ms() + refill])), (B, read(10.0, &[]))]) };
    let given = |readings: Vec<(&'static str, Option<FuelReading>)>| -> Readings { Box::new(move || readings.clone()) };
    // (job, the fuel readings before it, why it was skipped: none when the agent was called)
    let cases: Vec<(Lap, Readings, Option<Reason>)> = vec![
        (gated("l-fail-ok", ScriptGate::OnFailure, "exit 0"), given(vec![]), Some(Reason::Gate)),
        (gated("l-fail-bad", ScriptGate::OnFailure, "exit 3"), given(vec![]), None),
        (gated("l-out-none", ScriptGate::OnOutput, "exit 0"), given(vec![]), Some(Reason::Gate)),
        (gated("l-out-some", ScriptGate::OnOutput, "echo found"), given(vec![]), None),
        (fueled("l-fuel-skip", FuelThen::Skip), over(300), Some(Reason::Fuel)),
        (fueled("l-fuel-wait", FuelThen::Wait), over(300), None),
        (fueled("l-fuel-late", FuelThen::Wait), over(5 * 3_600_000), Some(Reason::Fuel)),
        // A job with its own account goes by that account's reading, not the one in use's…
        (own("l-fuel-own", FuelThen::Skip), over(300), None),
        // …and with none of it, it is never run blind.
        (own("l-fuel-unread", FuelThen::Wait), given(vec![(MAIN, read(10.0, &[])), (B, None)]), Some(Reason::NoReading)),
        (fueled("l-fuel-none", FuelThen::Skip), given(vec![(MAIN, None)]), Some(Reason::NoReading)),
        // Codex's home isn't there: Codex is off.
        (codex, given(vec![]), Some(Reason::AgentOff)),
    ];
    for (lap, readings, skipped) in cases {
        let id = lap.id.clone();
        let readings = readings();
        if !readings.is_empty() {
            fuel(&core, MAIN, &readings);
        }
        put(&core, lap);
        let before = calls.load(Ordering::SeqCst);
        let run = run_here(&core, &id);
        let called = calls.load(Ordering::SeqCst) > before;
        assert_eq!((run.reason, called), (skipped, skipped.is_none()), "{id}");
    }

    // The git gate: no commit since the last run, no agent.
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let done = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=Lap", "-c", "user.email=lap@example.com", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "one"]);
    put(&core, Lap { gate: Gate { git: true, ..Gate::default() }, ..job("l-git", &repo, LapKind::Agent) });
    let before = calls.load(Ordering::SeqCst);
    assert_eq!(run_here(&core, "l-git").result, Some(RunResult::Ok));
    assert_eq!(run_here(&core, "l-git").reason, Some(Reason::Git));
    assert_eq!(calls.load(Ordering::SeqCst), before + 1);
    git(&["commit", "-q", "--allow-empty", "-m", "two"]);
    assert_eq!(run_here(&core, "l-git").result, Some(RunResult::Ok));
    assert_eq!(calls.load(Ordering::SeqCst), before + 2);

    // The agent turned off while the script runs (or the fuel guard waits): it is given nothing,
    // and the run is skipped, no failure. (`sleep` is a shell's word on unix alone.)
    #[cfg(unix)]
    {
        put(&core, Lap { script: script("sleep 1; echo found"), place_run: Where::Background, ..job("l-off-later", &folder, LapKind::ScriptAgent) });
        let before = calls.load(Ordering::SeqCst);
        let planned = core.lap_begin_now("l-off-later", now_ms()).unwrap();
        let going = {
            let core = Arc::clone(&core);
            let planned = planned.clone();
            thread::spawn(move || core.run_lap(planned))
        };
        assert!(process::wait_until(Duration::from_secs(10), || core.laps().going.get("l-off-later").is_some_and(|going| going.pgid.is_some())), "the script runs");
        core.lock().settings.agents.of_mut(Agent::Claude).on = Some(false);
        going.join().unwrap();
        let run = records(&core, "l-off-later").into_iter().find(|run| run.id == planned.run).unwrap();
        let streak = core.laps().lap("l-off-later").unwrap().streak;
        assert_eq!((run.result, run.reason, calls.load(Ordering::SeqCst), streak), (Some(RunResult::Skipped), Some(Reason::AgentOff), before, 0));
    }
}

/// A lap runs and writes nowhere a lap may not (Claude's, Codex's, Pitwall's own folders), however
/// its folder came to be one: made a link into Claude's after the job was saved, written into
/// `laps.json` by hand, a listed project inside Claude's. Each is resolved at every run and
/// skipped as `notFolder`, its script never run. A Codex account's home is one of them while
/// Codex is off, as its main home is.
#[cfg(unix)]
#[test]
fn a_lap_runs_and_writes_nowhere_a_lap_may_not() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for dir in ["claude/projects/inside", "config", "work", "codex-work"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let core = core_at(&root);
    let marker = |name: &str| root.join(format!("{name}.ran"));
    let pwd = |name: &str| script(&format!("pwd -P > '{}'", marker(name).display()));
    let ran = |name: &str| fs::read_to_string(marker(name)).ok().map(|text| text.trim().to_string());

    let work = root.join("work");
    let saved = core.lap_save(Lap { name: "Sweep".into(), kind: LapKind::Script, place: Place::Folder { path: work.to_string_lossy().into_owned() }, script: pwd("linked"), ..Lap::default() }).unwrap();
    run_here(&core, &saved.id);
    assert_eq!(ran("linked"), Some(work.to_string_lossy().into_owned()), "a plain folder runs");
    fs::remove_file(marker("linked")).unwrap();
    fs::remove_dir(&work).unwrap();
    std::os::unix::fs::symlink(root.join("claude"), &work).unwrap();

    put(&core, Lap { script: pwd("by-hand"), ..job("l-by-hand", &root.join("config"), LapKind::Script) });
    let inside = root.join("claude/projects/inside").to_string_lossy().into_owned();
    core.add_project(&inside).unwrap();
    put(&core, Lap { place: Place::Project { path: inside }, script: pwd("project"), ..job("l-project", &root, LapKind::Script) });

    for (id, name) in [(saved.id.as_str(), "linked"), ("l-by-hand", "by-hand"), ("l-project", "project")] {
        let run = run_here(&core, id);
        assert_eq!((run.result, run.reason, ran(name)), (Some(RunResult::Skipped), Some(Reason::NotFolder), None), "{name}");
    }

    // Codex's homes, its main one and an account's not named `.codex*`, while it is off.
    fs::create_dir_all(root.join("codex")).unwrap();
    assert!(!core.agent_on(Agent::Codex));
    let account = ClaudeAccount { id: "a1b2".into(), name: "work".into(), dir: root.join("codex-work").to_string_lossy().into_owned(), ..Default::default() };
    core.lock().settings.agents.codex.accounts.others.push(account);
    let roots = core.laps_refused_roots();
    for dir in ["codex", "codex-work"] {
        let dir = root.join(dir);
        let file = run::result_file(&format!("{}/r.md", dir.display()), "x", now_ms(), &root, &roots);
        assert_eq!((core.lap_folder(&dir.to_string_lossy()).is_err(), file), (true, None), "{}", dir.display());
    }
}

/// A lap runs only as `lap_save` takes it, whatever brought it: a backup (a file that "may come
/// from anywhere") or a `laps.json` written by hand. Unchecked, an agent job ran every minute and
/// one failing run called its agent 256 times, and a script ran in Pitwall's own folder. A
/// backup's job `lap_save` refuses doesn't come in (the report counts the jobs written); written
/// by hand, it is neither switched on nor resumed, and runs nothing from the tick or the button.
#[test]
fn a_lap_runs_only_as_lap_save_takes_it_whatever_brought_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let (work, config) = (root.join("work"), root.join("config"));
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&config).unwrap();
    let core = core_at(&root);
    let calls = stand_in(&core, 1, None);
    let marker = root.join("ran-in.txt");
    let often = serde_json::json!({
        "id": "l-often", "name": "Watch", "kind": "agent", "prompt": "Look at the folder", "where": "background",
        "place": { "kind": "folder", "path": work }, "when": { "kind": "every", "minutes": 1 }, "retry": { "times": 255, "everyMin": 0 },
    });
    let inside = serde_json::json!({
        "id": "l-inside", "name": "Sweep", "kind": "script", "place": { "kind": "folder", "path": config },
        "script": { "kind": "text", "text": format!("pwd -P > '{}'", marker.display()) }, "when": { "kind": "daily", "at": "09:00" },
    });
    let fine = serde_json::json!({
        "id": "l-fine", "name": "Nightly", "kind": "script", "place": { "kind": "folder", "path": work },
        "script": { "kind": "text", "text": "true" }, "when": { "kind": "daily", "at": "09:00" },
    });

    let file = root.join("pitwall-backup.json");
    let backup = serde_json::json!({ "format": "pitwall-backup", "v": 1, "createdAt": 1_790_000_000_000u64, "app": "9.9.9", "home": "/Users/them", "laps": [often, inside, fine] });
    fs::write(&file, backup.to_string()).unwrap();
    let only_laps = BackupParts { settings: false, projects: false, board: false, activity: false, laps: true };
    assert_eq!(core.backup_import(&file, only_laps).map(|report| report.laps), Ok(1));
    assert_eq!(core.laps_state().laps.iter().map(|view| view.lap.id.as_str()).collect::<Vec<_>>(), ["l-fine"]);

    let now = now_ms();
    for (raw, reason) in [(often, Reason::NotStarted), (inside, Reason::NotFolder)] {
        let lap: Lap = serde_json::from_value(raw).unwrap();
        let id = lap.id.clone();
        assert!(core.lap_save(lap.clone()).is_err(), "{id}: lap_save refuses it");
        put(&core, Lap { paused: Some(PausedBy::User), ..lap.clone() });
        assert!(core.lap_pause(&id, false).is_err() && core.lap_switch(&id, true).is_err(), "{id}: resumed or switched on");
        put(&core, Lap { next_at: Some(now - 1_000), ..lap });
        core.laps_due_at(now, &mut |core, planned| core.run_lap(planned));
        let run = run_here(&core, &id);
        let reasons: Vec<Option<Reason>> = records(&core, &id).iter().map(|run| run.reason).collect();
        assert_eq!((run.result, reasons), (Some(RunResult::Skipped), vec![Some(reason); 2]), "{id}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0, "an agent was called");
    assert!(!marker.exists(), "the script ran in {}", fs::read_to_string(&marker).unwrap_or_default());
}

#[test]
fn a_refill_job_runs_once_for_each_refill_of_its_own_account() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("work");
    fs::create_dir_all(&folder).unwrap();
    let core = core_at(tmp.path());
    let main = crate::accounts::MAIN;
    let theirs = account(&core, tmp.path(), "b0b0");
    let now = now_ms();
    let (week, session, refill) = (now + 5 * 86_400_000, now + 2 * 3_600_000, now + 3 * 3_600_000);
    put(&core, Lap { when: When::Refill, next_at: None, ..job("l-in-use", &folder, LapKind::Agent) });
    put(&core, Lap { when: When::Refill, next_at: None, account: Some(theirs.clone()), ..job("l-theirs", &folder, LapKind::Agent) });
    let next = |id: &str| core.laps().lap(id).unwrap().next_at;
    let read_both = vec![(main, read(12.0, &[session, week])), (theirs.as_str(), read(5.0, &[refill]))];
    // (the account in use, the readings, the next moment of a job with none, and of one with B)
    let steps = [
        // No 5-hour session under way: the week's refill; B not read yet: no moment.
        (main, vec![(main, read(10.0, &[week])), (theirs.as_str(), None)], Some(week), None),
        // A session began meanwhile: its refill is sooner. B is read.
        (main, read_both.clone(), Some(session), Some(refill)),
        // B is switched to: a job that names no account follows it.
        (theirs.as_str(), read_both, Some(refill), Some(refill)),
    ];
    for (step, (in_use, readings, mine, b)) in steps.into_iter().enumerate() {
        fuel(&core, in_use, &readings);
        let unread = core.laps_state().laps.iter().find(|view| view.lap.id == "l-theirs").unwrap().no_reading;
        assert_eq!((next("l-in-use"), next("l-theirs"), unread), (mine, b, b.is_none()), "step {step}");
    }

    // The API's refill moment moves a little between readings: one refill is one run, whatever
    // the job does with a moment that comes while its run goes.
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    core.laps().agent_hook = Some(Arc::new(move |_: &Asked| {
        thread::sleep(Duration::from_millis(1_500));
        counted.fetch_add(1, Ordering::SeqCst);
        Said { code: 0, session: None, message: None }
    }));
    let mut wrong = Vec::new();
    for overlap in [Overlap::Wait, Overlap::Skip, Overlap::Stop] {
        let id = format!("l-refill-{overlap:?}").to_lowercase();
        core.laps().laps.clear();
        put(&core, Lap { when: When::Refill, overlap, next_at: None, ..job(&id, &folder, LapKind::Agent) });
        let refill = now_ms() + 60_000;
        fuel(&core, main, &[(main, read(10.0, &[refill]))]);
        assert_eq!(next(&id), Some(refill));
        fuel(&core, main, &[(main, read(10.0, &[refill + 400]))]);
        let before = calls.load(Ordering::SeqCst);
        let mut threads = Vec::new();
        for at in [refill + 100, refill + 1_100] {
            core.laps_due_at(at, &mut |core, planned| {
                let core = Arc::clone(core);
                threads.push(thread::spawn(move || core.run_lap(planned)));
            });
        }
        // A reading while it runs, the refill moved again.
        fuel(&core, main, &[(main, read(11.0, &[refill + 700]))]);
        assert!(process::wait_until(Duration::from_secs(10), || !core.laps().going.contains_key(&id)), "the first run ends");
        fuel(&core, main, &[(main, read(12.0, &[refill + 900]))]);
        core.laps_due_at(refill + 3_100, &mut |core, planned| {
            let core = Arc::clone(core);
            threads.push(thread::spawn(move || core.run_lap(planned)));
        });
        for handle in threads {
            handle.join().unwrap();
        }
        let runs = calls.load(Ordering::SeqCst) - before;
        let overlaps = records(&core, &id).iter().filter(|run| run.reason == Some(Reason::Overlap)).count();
        if (runs, overlaps) != (1, 0) {
            wrong.push(format!("{overlap:?}: {runs} agent runs, {overlaps} overlap records"));
        }
    }
    assert!(wrong.is_empty(), "one refill: {wrong:?}");
}

#[test]
fn what_a_runs_session_waits_on_is_told_by_the_laps() {
    use crate::claude::{SessionState, Turn, TurnKind};

    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("work");
    fs::create_dir_all(&folder).unwrap();
    let core = core_at(tmp.path());
    put(&core, job("l-told", &folder, LapKind::Agent));
    let now = now_ms();
    let going = new_uuid();
    let _run = core.lap_begin_now("l-told", now).unwrap();
    core.laps().going.get_mut("l-told").unwrap().session = Some(going.clone());
    assert!(core.lap_sessions().contains(&going), "the run's session is the laps' own");

    // No project's state has it: what it waits on is told here, each turn once, under the job's
    // folder's name, a permission by its tool's name.
    let waits = |at: u64, kind: TurnKind| SessionState {
        id: going.clone(),
        agent: Agent::Claude,
        path: String::new(),
        folder: Some(folder.to_string_lossy().into_owned()),
        title: None,
        phase: SessionPhase::Waiting,
        turn: Some(Turn { kind, at }),
        since: Some(at),
        hooked: true,
        ended: false,
        tool: Some("Bash".into()),
        subagents: 0,
        interrupted: false,
    };
    let told = |state: SessionState| core.lap_waits(&[state]).into_iter().map(|signal| (signal.kind, signal.name, signal.detail)).collect::<Vec<_>>();
    assert_eq!(told(waits(now, TurnKind::Permission)), [(SignalKind::ClaudePermission, "work".to_string(), Some("Bash".to_string()))]);
    assert!(told(waits(now, TurnKind::Permission)).is_empty(), "a turn is told once");
    assert_eq!(told(waits(now + 1, TurnKind::Asking)), [(SignalKind::ClaudeAsking, "work".to_string(), None)]);
}

// `sleep` is a shell's word on unix alone.
#[cfg(unix)]
#[test]
fn a_moment_that_comes_while_the_last_run_goes_skips_waits_or_stops_it() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("work");
    fs::create_dir_all(&folder).unwrap();
    let core = core_at(tmp.path());
    // (what the job does, its script, what the user does while the run goes: how the run ends,
    // runs after it)
    let cases = [
        (Overlap::Skip, "sleep 1", "", RunResult::Ok, 0),
        (Overlap::Wait, "sleep 1", "", RunResult::Ok, 1),
        (Overlap::Stop, "sleep 30", "", RunResult::Cut, 1),
        // A moment held for the run is let go once the job is paused or switched off.
        (Overlap::Wait, "sleep 1", "pause", RunResult::Ok, 0),
        (Overlap::Wait, "sleep 1", "off", RunResult::Ok, 0),
        (Overlap::Stop, "sleep 30", "pause", RunResult::Cut, 0),
    ];
    for (n, (overlap, line, then, result, after)) in cases.into_iter().enumerate() {
        let id = format!("l-overlap-{n}");
        put(&core, Lap { overlap, script: script(line), ..job(&id, &folder, LapKind::Script) });
        let first = core.lap_begin_now(&id, now_ms()).unwrap();
        let going = {
            let core = Arc::clone(&core);
            let first = first.clone();
            thread::spawn(move || core.run_lap(first))
        };
        assert!(process::wait_until(Duration::from_secs(10), || core.laps().going.get(&id).is_some_and(|going| going.pgid.is_some())), "{id}: the script runs");

        let now = now_ms();
        core.laps().lap_mut(&id).unwrap().next_at = Some(now - 1_000);
        core.laps_due_at(now, &mut |_, planned| panic!("{id}: {planned:?} started beside the run before it"));
        match then {
            "pause" => core.lap_pause(&id, true).unwrap(),
            "off" => core.lap_switch(&id, false).unwrap(),
            _ => {}
        }
        going.join().unwrap();
        let ended = records(&core, &id).into_iter().find(|run| run.id == first.run).unwrap();

        let mut started = Vec::new();
        core.laps_due_at(now_ms(), &mut |_, planned| started.push(planned));
        assert_eq!((ended.result, started.len()), (Some(result), after), "{id}: {overlap:?} {then}");
        if result == RunResult::Cut {
            assert_eq!(ended.reason, Some(Reason::Stopped));
        }
        if overlap == Overlap::Skip {
            assert!(records(&core, &id).iter().any(|run| run.reason == Some(Reason::Overlap)));
        }
    }
}

#[test]
fn a_command_that_asks_first_is_never_run_by_a_lap() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let project = root.join("paddock");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.json"), r#"{"name":"paddock","scripts":{"dev":"vite"}}"#).unwrap();
    let core = core_at(root);
    core.add_project(&project.to_string_lossy()).unwrap();
    let path = core.listed_paths().into_iter().next().expect("listed");
    let marker = root.join("migrated");
    let migrate = CustomCommand { id: "c-migrate".into(), name: "migrate".into(), command: format!("touch '{}'", marker.display()), confirm: true, ..Default::default() };
    core.set_project_settings(&path, ProjectSettings { commands: vec![migrate], ..Default::default() });
    let command = Some(Script::Command { id: "c-migrate".into() });

    // The project's job, from the tick and from its button.
    put(&core, Lap { place: Place::Project { path: path.clone() }, script: command.clone(), next_at: Some(now_ms() - 1_000), ..job("l-ask", &project, LapKind::Script) });
    core.laps_due_at(now_ms(), &mut |core, planned| core.run_lap(planned));
    run_here(&core, "l-ask");
    // A folder's job carrying it (a backup's): it can't be checked, so it never runs.
    put(&core, Lap { script: command.clone(), ..job("l-ask-folder", &project, LapKind::Script) });
    run_here(&core, "l-ask-folder");

    let reasons: Vec<Option<Reason>> = records(&core, "l-ask").into_iter().chain(records(&core, "l-ask-folder")).map(|run| run.reason).collect();
    assert_eq!(reasons, vec![Some(Reason::AskFirst); 3]);
    assert!(!marker.exists(), "the command ran");
    let saved = core.lap_save(Lap { id: String::new(), name: "Migrate nightly".into(), place: Place::Project { path }, script: command, ..job("", &project, LapKind::Script) });
    assert_eq!(saved, Err(t!("core.laps.error.command")));
}

#[test]
fn an_agent_is_started_with_the_fixed_flags_alone() {
    let mut starts: Vec<Start> = Vec::new();
    for model in std::iter::once(None).chain(Model::ALL.map(Some)) {
        for mode in Mode::ALL {
            starts.push(Start::claude(model, mode));
        }
    }
    for model in std::iter::once(None).chain(CodexModel::ALL.map(Some)) {
        for mode in CodexMode::ALL {
            starts.push(Start { agent: Agent::Codex, codex: CodexStart { model, mode }, ..Start::default() });
        }
    }
    let fixed = ["-p", "--session-id", "--tools", "Read,Grep,Glob", "--strict-mcp-config", "--model", "--no-daemon", "exec", "-s", "read-only", "--skip-git-repo-check", "--json", "-m", "-"];
    let models: Vec<&str> = Model::ALL.iter().map(|model| model.alias()).chain(CodexModel::ALL.iter().map(|model| model.id())).collect();
    for start in starts {
        let session = new_uuid();
        assert!(is_session_id(&session), "{session}");
        // The prompt goes on stdin: nothing of it is an argument.
        let flags = start.background_args(Some(&session)).unwrap();
        for flag in &flags {
            assert!(fixed.contains(&flag.as_str()) || models.contains(&flag.as_str()) || *flag == session, "{start:?}: {flag}");
        }
        match start.agent {
            Agent::Claude => assert!(flags.windows(2).any(|pair| pair == ["--session-id", session.as_str()]) && flags.windows(2).any(|pair| pair == ["--tools", "Read,Grep,Glob"]) && flags.contains(&"--strict-mcp-config".to_string())),
            Agent::Codex => assert!(flags.windows(2).any(|pair| pair == ["-s", "read-only"]) && flags.contains(&"--no-daemon".to_string()) && flags.last().map(String::as_str) == Some("-")),
        }
    }
    // A session id that isn't Pitwall's own UUID starts nothing, in the background or in a tab.
    assert_eq!(Start::default().background_args(Some("x; rm -rf ~")), None);
    assert_ne!(new_uuid(), new_uuid());
    #[cfg(unix)]
    {
        let line = |session: &str| {
            let command = super::super::terminal::claude_command("/bin/zsh", Some("hi"), Start::default(), None, Some(session));
            command.get_argv().iter().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ")
        };
        let session = new_uuid();
        assert!(line(&session).contains(&format!("--session-id {session}")));
        assert!(!line("x; rm -rf ~").contains("--session-id"));
    }
}

#[test]
fn a_failing_lap_is_tried_again_in_its_own_run_and_pauses_after_too_many() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("work");
    fs::create_dir_all(&folder).unwrap();
    let core = core_at(tmp.path());
    let calls = stand_in(&core, 1, None);
    put(&core, Lap { retry: Some(Retry { times: 2, every_min: 1 }), pause_after: Some(2), ..job("l-fails", &folder, LapKind::Agent) });

    let run = run_here(&core, "l-fails");
    assert_eq!((run.result, run.tries, calls.load(Ordering::SeqCst)), (Some(RunResult::Failed), 3, 3));
    assert_eq!(records(&core, "l-fails").len(), 1, "a retry is the same run");
    assert_eq!(core.laps().lap("l-fails").unwrap().streak, 1);

    run_here(&core, "l-fails");
    let lap = core.laps().lap("l-fails").cloned().unwrap();
    assert_eq!((lap.streak, lap.paused, lap.next_at), (2, Some(PausedBy::Failures), None));
    assert!(core.lap_paused_now(&folder.to_string_lossy(), "l-fails"));

    core.lap_pause("l-fails", false).unwrap();
    assert_eq!(core.laps().lap("l-fails").unwrap().streak, 0);
    stand_in(&core, 1, None);
    run_here(&core, "l-fails");
    stand_in(&core, 0, None);
    assert_eq!(run_here(&core, "l-fails").result, Some(RunResult::Ok));
    assert_eq!(core.laps().lap("l-fails").unwrap().streak, 0, "a good run starts the count anew");
}

#[test]
fn a_result_file_is_never_written_over_nor_outside_the_home_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let outside = tmp.path().join("outside");
    for dir in ["notes", ".claude/projects", ".claude-work", ".codex", "pitwall-config"] {
        fs::create_dir_all(home.join(dir)).unwrap();
    }
    fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, home.join("away")).unwrap();
    let roots = vec![home.join("pitwall-config")];
    let now = now_ms();
    let outside_file = outside.join("r.md").to_string_lossy().into_owned();
    let cases: Vec<(&str, bool)> = vec![
        ("~/notes/{ad}-{tarih}-{saat}.md", true),
        (&outside_file, false),
        ("~/../outside/r.md", false),
        ("~/notes/../../outside/r.md", false),
        ("~/away/r.md", false),
        ("~/.claude/projects/r.md", false),
        ("~/.claude-work/r.md", false),
        ("~/.codex/r.md", false),
        ("~/pitwall-config/r.md", false),
        ("~/missing/r.md", false),
        ("notes/r.md", false),
    ];
    for (template, allowed) in cases {
        assert_eq!(run::result_file(template, "Günlük rapor", now, &home, &roots).is_some(), allowed, "{template}");
    }
    let named = run::result_file("~/notes/{ad}.md", "Günlük rapor / gece", now, &home, &roots).unwrap();
    assert_eq!(named.file_name().unwrap(), "Günlük-rapor--gece.md");
    // The English tokens too, and a word in braces that is no token stays as written.
    let english = run::result_file("~/notes/{name}-{x}.md", "Günlük rapor", now, &home, &roots).unwrap();
    assert_eq!(english.file_name().unwrap(), "Günlük-rapor-{x}.md");

    let file = run::result_file("~/notes/report.md", "x", now, &home, &roots).unwrap();
    assert_eq!(run::write_result(&file, "one"), Some(file.clone()));
    let second = run::write_result(&file, "two").unwrap();
    assert_eq!(second.file_name().unwrap(), "report (2).md");
    assert_eq!(fs::read_to_string(&file).unwrap(), "one");
    // A link at the name is never written through.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("target.md"), home.join("notes").join("linked.md")).unwrap();
        let linked = run::result_file("~/notes/linked.md", "x", now, &home, &roots).unwrap();
        assert_eq!(run::write_result(&linked, "three").unwrap().file_name().unwrap(), "linked (2).md");
        assert!(!outside.join("target.md").exists());
    }
}

#[test]
fn a_broken_laps_file_is_kept_aside_and_what_is_kept_stays_private() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    fs::create_dir_all(&config).unwrap();
    let settings = config.join("settings.json");
    let aside = || fs::read_dir(&config).unwrap().flatten().map(|entry| entry.path()).filter(|path| path.to_string_lossy().contains("laps.json.broken-")).collect::<Vec<_>>();
    let ids = |laps: &Laps| laps.laps.iter().map(|lap| lap.id.clone()).collect::<Vec<_>>();
    let good = serde_json::to_value(job("l-good", tmp.path(), LapKind::Script)).unwrap();
    // One job that doesn't fit costs only itself (here, one of a form this Pitwall doesn't know).
    let some = serde_json::json!({ "v": 1, "laps": [good, { "id": "l-newer", "when": { "kind": "lunar" } }, { "id": "../evil", "kind": 7 }] }).to_string();
    // (the file: the jobs read)
    let cases: [(&str, &[&str]); 2] = [("{not json", &[]), (&some, &["l-good"])];
    for (n, (text, kept)) in cases.into_iter().enumerate() {
        fs::write(config.join("laps.json"), text).unwrap();
        // Read at the start, nothing moved: another Pitwall may hold the laps.
        let mut laps = Laps::load(&settings, now_ms());
        assert_eq!((ids(&laps), aside().len()), (kept.iter().map(|id| id.to_string()).collect::<Vec<_>>(), n), "case {n}: read");
        // The lock taken: the file as it was kept aside, and what fits still there and written back.
        assert!(lock(&mut laps));
        assert_eq!(ids(&laps), kept, "case {n}: after the lock");
        let kept_aside = aside();
        assert_eq!(kept_aside.len(), n + 1);
        assert!(kept_aside.iter().any(|file| fs::read_to_string(file).unwrap() == text));
        let written = fs::read_to_string(config.join("laps.json")).ok().map(|text| text.contains("l-good"));
        assert_eq!(written, (!kept.is_empty()).then_some(true), "case {n}: written back");
    }

    // Written readable by the user alone; pruned to `keep`, outputs and all, the oldest that
    // ended first, never the run that goes.
    let mut laps = Laps::load(&settings, now_ms());
    assert!(lock(&mut laps));
    laps.laps = vec![Lap { keep: 2, ..job("l-good", tmp.path(), LapKind::Script) }];
    assert!(laps.save());
    for n in 0..4 {
        let run = format!("r-{n}");
        let ended_at = (n > 0).then_some(1);
        laps.runs.entry("l-good".into()).or_default().push(LapRun { id: run.clone(), lap: "l-good".into(), ended_at, script: true, ..Default::default() });
        assert!(laps.store.save_output("l-good", &run, "npm test", &["secret line".into()]));
        assert!(laps.save_runs("l-good"));
    }
    // The command line it ran is kept apart from what it printed: a carry or a prompt reads the
    // printed lines alone, and a line printed with the core's mark is never taken for the command.
    assert!(laps.store.save_output("l-good", "r-3", "make\nmake test", &["\u{1e}$ rm -rf /".into(), "ok".into()]));
    assert_eq!(laps.store.read_output("l-good", "r-3"), Some(vec!["$ rm -rf /".to_string(), "ok".to_string()]));
    assert_eq!(laps.store.read_kept("l-good", "r-3").and_then(|(command, _)| command).as_deref(), Some("make\nmake test"));
    assert_eq!(laps.runs["l-good"].iter().map(|run| run.id.as_str()).collect::<Vec<_>>(), ["r-0", "r-3"]);
    let logs = config.join("lap-runs").join("l-good");
    assert!(!logs.join("r-1.log").exists() && !logs.join("r-2.log").exists() && logs.join("r-3.log").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in [config.join("laps.json"), config.join("lap-runs").join("l-good.json"), logs.join("r-3.log")] {
            assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600, "{}", file.display());
        }
    }
}

#[test]
fn an_order_never_loses_a_job_nor_writes_one_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let folder = root.join("work");
    fs::create_dir_all(&folder).unwrap();
    let settings = root.join("config").join("settings.json");
    let core = core_at(root);
    let all = ["l-a", "l-b", "l-c", "l-d"];
    for id in all {
        put(&core, Lap { prompt: format!("{id}'s own words"), ..job(id, &folder, LapKind::Agent) });
    }
    let jobs = core.laps().laps.clone();

    // (what the call names: the order that comes of it from a, b, c, d)
    let cases: [(&[&str], [&str; 4]); 5] = [
        (&["l-d", "l-c", "l-b", "l-a"], ["l-d", "l-c", "l-b", "l-a"]),
        // Those it doesn't name keep their order, after the named ones.
        (&["l-c", "l-a"], ["l-c", "l-a", "l-b", "l-d"]),
        // An id it doesn't know is skipped; one named twice keeps its first place.
        (&["l-x", "l-b", "l-b", "", "../l-a", "l-d", "l-b"], ["l-b", "l-d", "l-a", "l-c"]),
        (&["l-c", "l-c", "l-c", "l-c", "l-c"], ["l-c", "l-a", "l-b", "l-d"]),
        (&[], ["l-a", "l-b", "l-c", "l-d"]),
    ];
    for (named, wanted) in cases {
        core.lap_reorder(&all.map(String::from)).unwrap();
        core.lap_reorder(&named.iter().map(|id| id.to_string()).collect::<Vec<_>>()).unwrap();

        // Held, written and sent out in that order, every job whole and once.
        let wanted: Vec<Lap> = wanted.iter().map(|id| jobs.iter().find(|lap| lap.id == *id).unwrap().clone()).collect();
        assert_eq!(core.laps().laps, wanted, "{named:?}: held");
        assert_eq!(Laps::load(&settings, now_ms()).laps, wanted, "{named:?}: on disk");
        let sent: Vec<String> = core.laps_state().laps.into_iter().map(|view| view.lap.id).collect();
        assert_eq!(sent, wanted.iter().map(|lap| lap.id.clone()).collect::<Vec<_>>(), "{named:?}: sent");
    }
}
