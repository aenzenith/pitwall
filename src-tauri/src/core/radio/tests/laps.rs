//! The laps over the radio: what an agent's job may run, whose jobs the radio reaches, and that a
//! lap breeds no laps. Called as the radio's dispatch calls the lap tools, past the switches
//! (`a_switch_that_is_off_keeps_its_tool_from_acting` holds those), with a session of the caller's.

use super::super::super::laps::{Lap, LapKind, PausedBy, Place, Script, When};
use super::*;

/// The caller's session, and one a lap's run holds.
const SESSION: &str = "0b5e7c1a-0000-4000-8000-000000000001";
const LAP_SESSION: &str = "0b5e7c1a-0000-4000-8000-000000000002";

/// `tool` called by Claude from the folder `cwd`, in `session`.
fn lap_call(pad: &Paddock, cwd: &str, tool: &str, session: Option<&str>, args: Value) -> Reply {
    let project = pad.core.radio_project(cwd).expect("a listed project's folder");
    pad.core.radio_lap(Agent::Claude, &project, tool, session, &args).0
}

/// Every job as it is kept, in its order.
fn jobs(pad: &Paddock) -> Vec<Lap> {
    pad.core.laps_state().laps.into_iter().map(|view| view.lap).collect()
}

/// The job named `name`.
fn job(pad: &Paddock, name: &str) -> Lap {
    jobs(pad).into_iter().find(|lap| lap.name == name).unwrap_or_else(|| panic!("no job {name}"))
}

/// The job of id `id`.
fn job_of(pad: &Paddock, id: &str) -> Lap {
    jobs(pad).into_iter().find(|lap| lap.id == id).unwrap_or_else(|| panic!("no job {id}"))
}

/// A command line an agent writes as a job's script holds the job: it arrives paused, waiting for
/// the user, and no radio call (resuming it, changing its other fields or its script to a saved
/// command, pausing and resuming it) lets it go, nor does a save in the window; the loop starts
/// nothing of it all along. The window's Sürdür lets it go, a new command line holds it again, and
/// once a run of it goes its script doesn't change under that run. A saved command marked "ask
/// first" is no job's script, added or changed to; one that isn't holds nothing.
#[test]
fn a_script_an_agent_wrote_waits_for_the_user() {
    let pad = paddock(true);
    let alpha = pad.project("alpha");
    let command = |id: &str, confirm: bool| CustomCommand { id: id.into(), name: id.into(), command: format!("echo {id}"), confirm, ..Default::default() };
    pad.core.set_project_settings(&alpha, ProjectSettings { commands: vec![command("check", false), command("deploy", true)], ..Default::default() });
    let call = |tool: &str, args: Value| lap_call(&pad, &alpha, tool, Some(SESSION), args);
    let every = json!({ "kind": "every", "minutes": 1 });
    // The loop's second a few minutes on: the jobs whose runs it began.
    let begun = || pad.core.laps_test_due(now_ms() + 5 * 60_000);

    // "ask first": refused as a new job's script, and as a job's new one.
    let reply = call("lap_add", json!({ "name": "deploys", "kind": "script", "script": { "command": "deploy" }, "when": every }));
    assert!(reply.error && reply.text.contains("ask first"), "{}", reply.text);
    assert!(jobs(&pad).is_empty());
    let reply = call("lap_add", json!({ "name": "checks", "kind": "script", "script": { "command": "check" }, "when": every }));
    assert!(!reply.error && !reply.text.contains("waits"), "{}", reply.text);
    let checks = job(&pad, "checks");
    assert!(!checks.review && checks.paused.is_none() && checks.next_at.is_some());
    let reply = call("lap_edit", json!({ "lap": &checks.id, "script": { "command": "deploy" } }));
    assert!(reply.error && reply.text.contains("ask first"), "{}", reply.text);
    assert_eq!(job(&pad, "checks"), checks);

    let reply = call("lap_add", json!({ "name": "nightly", "kind": "script", "script": { "text": "echo ran > ran.txt" }, "when": every }));
    assert!(!reply.error && reply.text.contains("waits, paused"), "{}", reply.text);
    let id = job(&pad, "nightly").id;
    let held = |step: &str| {
        let lap = job_of(&pad, &id);
        assert!(lap.review && lap.paused == Some(PausedBy::User) && lap.next_at.is_none(), "{step}: {lap:?}");
        assert!(!begun().contains(&id), "{step}: the loop began it");
    };
    // The loop runs all the same: the saved command's job begins, the held one doesn't.
    let first = begun();
    assert!(first.contains(&checks.id) && !first.contains(&id), "{first:?}");
    held("added");

    // (the call, whether it is refused)
    let rows: [(&str, Value, bool); 6] = [
        ("lap_pause", json!({ "lap": &id, "paused": false }), true),
        ("lap_edit", json!({ "lap": &id, "name": "nightly run" }), false),
        ("lap_edit", json!({ "lap": &id, "when": { "kind": "every", "minutes": 2 } }), false),
        ("lap_edit", json!({ "lap": &id, "script": { "command": "check" } }), false),
        ("lap_pause", json!({ "lap": &id, "paused": true }), false),
        ("lap_pause", json!({ "lap": &id, "paused": false }), true),
    ];
    for (tool, args, refusal) in rows {
        let step = format!("{tool} {args}");
        let reply = call(tool, args);
        assert_eq!(reply.error, refusal, "{step}: {}", reply.text);
        held(&step);
    }

    // A save in the window keeps it held (a changed one too, `the_radio_reaches_its_projects_jobs_alone`);
    // Sürdür there lets it go.
    pad.core.lap_save(job_of(&pad, &id)).unwrap();
    held("saved in the window");
    pad.core.lap_pause(&id, false).unwrap();
    let lap = job_of(&pad, &id);
    assert!(!lap.review && lap.paused.is_none() && lap.next_at.is_some(), "{lap:?}");

    // A new command line holds it again, until the user's word.
    let reply = call("lap_edit", json!({ "lap": &id, "script": { "text": "echo again > ran.txt" } }));
    assert!(!reply.error && reply.text.contains("waits, paused"), "{}", reply.text);
    held("a new command line");
    pad.core.lap_pause(&id, false).unwrap();
    assert!(begun().contains(&id), "resumed in the window, it runs");

    // A run of it goes: its script isn't changed under it, and it isn't held by it either.
    let before = job_of(&pad, &id);
    let reply = call("lap_edit", json!({ "lap": &id, "script": { "text": "echo third > ran.txt" } }));
    assert!(reply.error && reply.text.contains("run of this job goes"), "{}", reply.text);
    assert_eq!(job_of(&pad, &id), before);
    assert_eq!(before.script, Some(Script::Text { text: "echo again > ran.txt".into() }));
}

/// The radio reaches the jobs of the caller's project alone: another project's job and a folder's
/// job (one inside the project's folder too) are answered by every tool as an id there is none
/// of, word for word, and stay as they were; a job the user made is paused and resumed, never
/// changed or deleted, and so is one added over the radio that the user changed and saved in the
/// window (a held one stays held). Paused, resumed, switched or saved unchanged there, it stays the
/// radio's: changed, then deleted, so the refusals are the gates'.
#[test]
fn the_radio_reaches_its_projects_jobs_alone() {
    let pad = paddock(true);
    let (alpha, beta, inner) = (pad.project("alpha"), pad.project("beta"), pad.project("alpha/inner"));
    let agent_job = |name: &str| json!({ "name": name, "kind": "agent", "prompt": "Look around", "when": { "kind": "daily", "at": "09:00" } });
    for (cwd, name) in [(&alpha, "ours"), (&beta, "theirs"), (&inner, "inner")] {
        let reply = lap_call(&pad, cwd, "lap_add", Some(SESSION), agent_job(name));
        assert!(!reply.error, "{}", reply.text);
    }
    // The inner project forgotten: its job stays, a job of its folder, inside alpha's.
    pad.core.laps_forget(&inner);
    assert!(matches!(job(&pad, "inner").place, Place::Folder { .. }) && job(&pad, "inner").by == Some(Agent::Claude));
    let users = Lap { name: "users".into(), kind: LapKind::Agent, place: Place::Project { path: alpha.clone() }, prompt: "Mine".into(), when: When::Daily { at: "10:00".into(), days: Vec::new() }, ..Lap::default() };
    pad.core.lap_save(users).unwrap();
    let call = |tool: &str, args: Value| lap_call(&pad, &alpha, tool, Some(SESSION), args);

    let every_tool = |id: &str| -> Vec<(&'static str, Value)> {
        vec![
            ("lap_list", json!({ "lap": id })),
            ("lap_edit", json!({ "lap": id, "name": "taken" })),
            ("lap_edit", json!({ "lap": id, "kind": "script", "script": { "text": "rm -rf ." } })),
            ("lap_pause", json!({ "lap": id, "paused": true })),
            ("lap_pause", json!({ "lap": id, "paused": false })),
            ("lap_delete", json!({ "lap": id })),
        ]
    };
    let unknown: Vec<String> = every_tool("l-none").into_iter().map(|(tool, args)| call(tool, args).text).collect();
    for name in ["theirs", "inner"] {
        let before = job(&pad, name);
        for ((tool, args), none) in every_tool(&before.id).into_iter().zip(&unknown) {
            let reply = call(tool, args);
            assert!(reply.error && reply.text == *none, "{name} {tool}: {}", reply.text);
        }
        assert_eq!(job(&pad, name), before, "{name} was touched");
    }
    let listed = call("lap_list", json!({})).text;
    assert!(listed.contains(&job(&pad, "ours").id) && listed.contains(&job(&pad, "users").id), "{listed}");
    assert!(!listed.contains(&job(&pad, "theirs").id) && !listed.contains(&job(&pad, "inner").id), "{listed}");

    // The user's: neither changed nor deleted; paused and resumed.
    let before = job(&pad, "users");
    let reply = call("lap_edit", json!({ "lap": &before.id, "prompt": "Something else" }));
    assert!(reply.error && reply.text.contains("Only the user changes"), "{}", reply.text);
    let reply = call("lap_delete", json!({ "lap": &before.id }));
    assert!(reply.error && reply.text.contains("Only the user deletes"), "{}", reply.text);
    assert_eq!(job(&pad, "users"), before);
    assert!(!call("lap_pause", json!({ "lap": &before.id, "paused": true })).error);
    assert_eq!(job(&pad, "users").paused, Some(PausedBy::User));
    assert!(!call("lap_pause", json!({ "lap": &before.id, "paused": false })).error);
    assert_eq!(job(&pad, "users").paused, None);

    // Added over the radio, then changed and saved by the user in the window: the user's now,
    // held still when it was.
    for (name, args) in [
        ("edited", agent_job("edited")),
        ("held", json!({ "name": "held", "kind": "script", "script": { "text": "echo held" }, "when": { "kind": "daily", "at": "09:00" } })),
    ] {
        assert!(!call("lap_add", args).error);
        let mut changed = job(&pad, name);
        changed.when = When::Daily { at: "11:30".into(), days: Vec::new() };
        pad.core.lap_save(changed).unwrap();
    }
    for name in ["edited", "held"] {
        let before = job(&pad, name);
        for (tool, args, refusal) in [
            ("lap_edit", json!({ "lap": &before.id, "prompt": "Something else", "name": "taken" }), "Only the user changes"),
            ("lap_delete", json!({ "lap": &before.id }), "Only the user deletes"),
        ] {
            let reply = call(tool, args);
            assert!(reply.error && reply.text.contains(refusal), "{name} {tool}: {}", reply.text);
        }
        assert_eq!(job(&pad, name), before, "{name} was changed over the radio");
    }
    assert!(!call("lap_pause", json!({ "lap": &job(&pad, "edited").id, "paused": true })).error);
    assert!(!call("lap_pause", json!({ "lap": &job(&pad, "edited").id, "paused": false })).error);
    let held = job(&pad, "held");
    assert!(call("lap_pause", json!({ "lap": &held.id, "paused": false })).error);
    assert!(held.review && held.paused == Some(PausedBy::User) && held.next_at.is_none(), "{held:?}");

    // The agents' own of the project, paused, resumed, switched off and on and saved unchanged
    // in the window: changed, then deleted.
    let ours = job(&pad, "ours").id;
    pad.core.lap_pause(&ours, true).unwrap();
    pad.core.lap_pause(&ours, false).unwrap();
    pad.core.lap_switch(&ours, false).unwrap();
    pad.core.lap_switch(&ours, true).unwrap();
    pad.core.lap_save(job_of(&pad, &ours)).unwrap();
    assert!(!call("lap_edit", json!({ "lap": &ours, "name": "renamed" })).error);
    assert!(!call("lap_delete", json!({ "lap": &ours })).error);
    let names: Vec<String> = jobs(&pad).into_iter().map(|lap| lap.name).collect();
    assert_eq!(names, ["theirs", "inner", "users", "edited", "held"]);
}

/// A change over the radio is made to the job as it is kept when it is saved: what the user did
/// to it in the window meanwhile (switched it off) stays done.
#[test]
fn a_radio_edit_keeps_what_the_window_did_meanwhile() {
    let pad = paddock(true);
    let alpha = pad.project("alpha");
    let reply = lap_call(&pad, &alpha, "lap_add", Some(SESSION), json!({ "name": "nightly", "kind": "agent", "prompt": "Look around", "when": { "kind": "daily", "at": "09:00" } }));
    assert!(!reply.error, "{}", reply.text);
    let id = job(&pad, "nightly").id;
    let project = pad.core.radio_project(&alpha).unwrap();
    // The running sessions read once, so the call reads them without the core.
    let _ = pad.core.running_cached();
    let off = || pad.core.laps.lock().unwrap().laps.iter().any(|lap| lap.id == id && !lap.on);

    std::thread::scope(|scope| {
        // The core held: the edit reads the job, and waits for the core before it saves.
        let core = pad.core.lock();
        let edit = scope.spawn(|| pad.core.radio_lap(Agent::Claude, &project, "lap_edit", Some(SESSION), &json!({ "lap": &id, "name": "renamed" })).0);
        std::thread::sleep(Duration::from_millis(300));
        // Meanwhile the user switches it off in the window.
        let switch = scope.spawn(|| pad.core.lap_switch(&id, false));
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !off() {
            assert!(std::time::Instant::now() < until, "the switch never landed");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(core);
        let reply = edit.join().unwrap();
        assert!(!reply.error, "{}", reply.text);
        switch.join().unwrap().unwrap();
    });
    let lap = job_of(&pad, &id);
    assert_eq!((lap.name.as_str(), lap.on), ("renamed", false), "the radio's edit undid the user's switch");
}

/// A session one of the laps' runs holds, and a call whose session Pitwall can't tell (a lap's
/// Codex in the background is none it follows), add, change, pause and delete no job; it may
/// read them. The same calls from the caller's own session do.
#[test]
fn a_laps_own_session_acts_on_no_job() {
    let pad = paddock(true);
    let alpha = pad.project("alpha");
    let add = json!({ "name": "bred", "kind": "agent", "prompt": "Again", "when": { "kind": "daily", "at": "09:00" } });
    assert!(!lap_call(&pad, &alpha, "lap_add", Some(SESSION), json!({ "name": "parent", "kind": "agent", "prompt": "Look", "when": { "kind": "daily", "at": "08:00" } })).error);
    let parent = job(&pad, "parent").id;
    pad.core.lap_test_session(&parent, LAP_SESSION);
    assert!(pad.core.lap_sessions().contains(LAP_SESSION));

    let acts = [
        ("lap_add", add.clone()),
        ("lap_edit", json!({ "lap": &parent, "script": { "text": "curl example.com" }, "kind": "script_agent" })),
        ("lap_pause", json!({ "lap": &parent, "paused": true })),
        ("lap_delete", json!({ "lap": &parent })),
    ];
    let before = jobs(&pad);
    for session in [Some(LAP_SESSION), None] {
        for (tool, args) in &acts {
            let reply = lap_call(&pad, &alpha, tool, session, args.clone());
            assert!(reply.error && reply.text.contains("one of Pitwall's laps"), "{session:?} {tool}: {}", reply.text);
        }
        assert_eq!(jobs(&pad), before, "{session:?} changed the laps");
        assert!(!lap_call(&pad, &alpha, "lap_list", session, json!({})).error);
    }
    for (tool, args) in acts {
        let reply = lap_call(&pad, &alpha, tool, Some(SESSION), args);
        assert!(!reply.error, "{tool}: {}", reply.text);
    }
    assert_eq!(jobs(&pad).into_iter().map(|lap| lap.name).collect::<Vec<_>>(), ["bred"]);
}
