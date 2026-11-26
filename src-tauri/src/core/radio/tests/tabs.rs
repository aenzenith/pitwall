//! The tab tools on real tabs (a login shell each, as the panel opens them): what a session may
//! close, write into or start again, and what it may never reach.

use super::*;
use crate::core::terminal::Launch;

/// The calling session: one Pitwall can tell, none of the laps'.
const CALLER: &str = "0b7f9a52-4a0e-4c38-9d1e-6f1f3c1c9a01";

fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Each case is a tab the call must leave as it was: another project's, the laps' own, an
/// administrator's, a pinned one, one at work; a shell takes no message and no session is started
/// in a shell that runs a program. A call from a session Pitwall can't tell acts on nothing.
#[test]
fn a_tab_tool_reaches_only_the_tabs_it_may() {
    let pad = paddock(true);
    let (alpha, beta) = (pad.project("alpha"), pad.project("beta"));
    let mut settings = pad.core.settings();
    for tool in ["tab_open", "tab_close", "tab_send", "tab_wake", "tab_pin", "tab_rename"] {
        settings.radio.tools.insert(tool.into(), true);
    }
    pad.core.set_settings(settings);
    let core = &pad.core;
    let project = core.radio_project(&alpha).unwrap();
    let call = |tool: &str, session: Option<&str>, args: Value| core.radio_tab(Agent::Claude, &project, tool, session, &args).0;
    let open = |id: u64| core.lock().terminal_list.iter().any(|view| view.id == id);
    let shell = |path: &str| core.open_terminal(path, false, Some((80, 24))).unwrap().id;

    let idle = shell(&alpha);
    let pinned = shell(&alpha);
    core.pin_terminal(pinned, true);
    let admin = shell(&alpha);
    core.mark_elevated(admin);
    let busy = shell(&alpha);
    // A Claude tab whose agent ended there, its session seen, its shell running a program now.
    let ended = shell(&alpha);
    let other = shell(&beta);
    let laps = core.open_terminal_apart(&alpha, &Launch::Shell { shell: None, elevated: false }, "lap".into(), Some((80, 24)), None).unwrap().id;
    {
        let mut inner = core.lock();
        inner.terminal_list.iter_mut().find(|view| view.id == ended).unwrap().kind = "claude".into();
        inner.tab_seen.insert(ended, (Agent::Claude, "5c2d0d8e-2f5b-4d7e-9a43-1f2b3c4d5e6f".into()));
    }
    let all = [idle, pinned, admin, busy, ended, other, laps];
    assert!(wait_for(|| all.iter().all(|id| !core.terminal_buffer(*id).data.is_empty() && !core.terminal_busy(*id, &mut None))), "the shells never came to rest");
    for id in [busy, ended] {
        core.write_terminal(id, "sleep 30\r");
    }
    assert!(wait_for(|| core.terminal_busy(busy, &mut None) && core.terminal_busy(ended, &mut None)), "the programs never started");

    // The list holds this project's tabs alone.
    let listed = call("tab_list", Some(CALLER), json!({})).text;
    for id in [idle, pinned, admin, busy, ended] {
        assert!(listed.contains(&format!("- tab {id}:")), "tab {id} is missing from the list:\n{listed}");
    }
    for id in [other, laps] {
        assert!(!listed.contains(&format!("- tab {id}:")), "tab {id} is listed:\n{listed}");
    }

    // Nothing is written to a shell, nor any session started in one that runs a program.
    let before = core.terminal_buffer(idle).data;
    call("tab_send", Some(CALLER), json!({ "tabs": [idle, ended, other, laps], "text": "echo PITWALL-MARK" }));
    assert!(call("tab_wake", Some(CALLER), json!({ "tab": ended })).error);
    assert!(call("tab_wake", Some(CALLER), json!({ "tab": idle })).error);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(core.terminal_buffer(idle).data, before, "a shell was written to");
    for id in [ended, other, laps] {
        let shown = core.terminal_buffer(id).data;
        assert!(!shown.contains("PITWALL-MARK") && !shown.contains("--resume"), "tab {id} was written to");
    }

    // No shell is opened, and the bounds hold before anything starts: four opens a minute, eight
    // of the radio's tabs standing in a project.
    let extra: Vec<u64> = (0..3).map(|_| shell(&alpha)).collect();
    let tabs = core.lock().terminal_list.len();
    assert!(call("tab_open", Some(CALLER), json!({ "kind": "shell" })).error);
    core.lock().radio.opens.insert(CALLER.into(), std::iter::repeat_n(Instant::now(), 4).collect());
    assert!(call("tab_open", Some(CALLER), json!({ "kind": "claude" })).error, "a fifth tab in a minute opened");
    core.lock().radio.opens.clear();
    for view in core.lock().terminal_list.iter_mut().filter(|view| view.path == alpha && !view.apart) {
        view.by = Some(Agent::Claude);
    }
    assert!(call("tab_open", Some(CALLER), json!({ "kind": "claude" })).error, "a ninth tab of the radio's opened");
    for view in core.lock().terminal_list.iter_mut() {
        view.by = None;
    }
    assert_eq!(core.lock().terminal_list.len(), tabs, "a tab was opened");
    for id in extra {
        core.close_terminal(id);
    }

    // Another project's tab, the laps' own and an administrator's take nothing.
    for id in [other, laps] {
        assert!(call("tab_pin", Some(CALLER), json!({ "tab": id })).error);
        assert!(call("tab_rename", Some(CALLER), json!({ "tab": id, "name": "taken" })).error);
    }
    assert!(call("tab_pin", Some(CALLER), json!({ "tab": admin })).error);
    assert!(core.lock().terminal_list.iter().all(|view| view.name != "taken" && (view.id == pinned || !view.pinned)));

    // A session Pitwall can't tell closes nothing.
    assert!(call("tab_close", None, json!({ "tabs": [idle] })).error);
    assert!(open(idle));

    // Of all asked, only the idle shell closes; the busy one with `force` alone.
    call("tab_close", Some(CALLER), json!({ "tabs": [other, laps, pinned, admin, busy, idle] }));
    assert!(wait_for(|| !open(idle)), "the idle shell stayed open");
    for id in [other, laps, pinned, admin, busy] {
        assert!(open(id), "tab {id} was closed");
    }
    call("tab_close", Some(CALLER), json!({ "tabs": [busy], "force": true }));
    assert!(wait_for(|| !open(busy)), "`force` left the busy shell open");

    for id in [pinned, admin, ended, other, laps] {
        core.close_terminal(id);
    }
}
