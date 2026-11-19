//! The board over the radio: what a call never does to a card that isn't its own to change (the
//! user's, another project's, another session's, one that waits), the columns it never moves a
//! card into or out of, and a card a session takes going with the turn that took it.

use super::*;
use crate::core::board::Column;

/// The user's cards and their words are theirs: the radio deletes none of them, never replaces
/// their title or note, and only adds to a note, signed. A card the radio added becomes the
/// user's once they edit and save it in the window; moved there, it stays the radio's. A card of
/// another project is no card
/// here (refused in the very words an unknown id gets), a card with another session is that
/// session's, a card that waits for its moment stays as it is (an edit or a move would call the
/// user's wait off), and no card goes into or out of an agent's column or review. What the radio
/// may do it does: its own cards are rewritten and deleted, a user's card moves between "Up
/// next" and "Done".
#[test]
fn the_radio_changes_no_card_that_isnt_its_own() {
    let pad = paddock(true);
    let (alpha, beta) = (pad.project("alpha"), pad.project("beta"));
    pad.core.radio_tool("board_delete", true).unwrap();
    let other_session = "9bb85e86-f618-4861-9858-03ec8fc36c28";

    let id_of = |title: &str| pad.core.board_state().into_iter().find(|card| card.title == title).unwrap().id;
    let radios = |cwd: &str, title: &str| {
        let reply = pad.call(cwd, "board_add", json!({ "title": title, "note": "the radio's words" }));
        assert!(!reply.error, "{}", reply.text);
        id_of(title)
    };
    let users = |title: &str| pad.core.board_add(&alpha, title, "the user's words", &[]).unwrap().id;
    let card = |id: &str| pad.core.board_state().into_iter().find(|card| card.id == id).unwrap();

    let mine = users("The user's card");
    let mine_done = users("The user's done card");
    pad.core.board_move(&mine_done, "done", 0).unwrap();
    let mine_held = users("The user's card with another session");
    pad.core.board_link(&mine_held, other_session).unwrap();
    let mine_waits = users("The user's card that waits");
    pad.core.board_schedule(&mine_waits, now_ms() + 3_600_000, false, crate::start::Start::default()).unwrap();

    let radio = radios(&alpha, "The radio's card");
    let radio_done = radios(&alpha, "The radio's done card");
    pad.core.board_move(&radio_done, "done", 0).unwrap();
    let held = radios(&alpha, "The radio's card with another session");
    pad.core.board_link(&held, other_session).unwrap();
    let review = radios(&alpha, "The radio's card in review");
    pad.core.board_move(&review, "review", 0).unwrap();
    let waits = radios(&alpha, "The radio's card that waits");
    pad.core.board_schedule(&waits, now_ms() + 3_600_000, false, crate::start::Start::default()).unwrap();
    let beyond = radios(&beta, "Beta's card");
    let edited = radios(&alpha, "Edited in the window");
    pad.core.board_edit(&edited, "Rewritten by the user", "the user's words now", &[]).unwrap();
    let moved = radios(&alpha, "The radio's moved card");
    pad.core.board_move(&moved, "done", 0).unwrap();
    pad.core.board_move(&moved, "queued", 0).unwrap();

    let before = pad.core.board_state();
    assert!(before.iter().filter(|card| card.title.starts_with("The radio's") || card.title == "Beta's card").all(|card| card.by == Some(Agent::Claude)));
    assert!(before.iter().filter(|card| card.title.starts_with("The user's")).all(|card| card.by.is_none()));

    // Another project's card is no card here: the same answer as an id that never was.
    let unknown = pad.call(&alpha, "board_delete", json!({ "card": "no-such-card" }));
    assert_eq!(pad.call(&alpha, "board_delete", json!({ "card": beyond })).text, unknown.text);
    assert_eq!(pad.call(&alpha, "board_edit", json!({ "card": beyond, "note": "x" })).text, unknown.text);

    let cases: Vec<(&str, Value)> = vec![
        ("board_delete", json!({ "card": mine })),
        ("board_delete", json!({ "card": mine_done })),
        ("board_delete", json!({ "card": beyond })),
        ("board_delete", json!({ "card": held })),
        ("board_delete", json!({ "card": review })),
        ("board_delete", json!({ "card": waits })),
        ("board_delete", json!({ "card": edited })),
        ("board_edit", json!({ "card": mine, "title": "Renamed by the radio" })),
        ("board_edit", json!({ "card": mine, "title": "Renamed by the radio", "note": "more" })),
        ("board_edit", json!({ "card": mine, "note": "all new", "mode": "replace" })),
        ("board_edit", json!({ "card": edited, "title": "Renamed by the radio" })),
        ("board_edit", json!({ "card": edited, "note": "all new", "mode": "replace" })),
        ("board_edit", json!({ "card": mine_waits, "note": "more" })),
        ("board_edit", json!({ "card": mine_held, "note": "more" })),
        ("board_edit", json!({ "card": held, "note": "all new", "mode": "replace" })),
        ("board_edit", json!({ "card": waits, "title": "Renamed by the radio" })),
        ("board_edit", json!({ "card": beyond, "title": "Renamed by the radio" })),
        ("board_move", json!({ "card": held, "column": "queued" })),
        ("board_move", json!({ "card": mine_held, "column": "done" })),
        ("board_move", json!({ "card": review, "column": "done" })),
        ("board_move", json!({ "card": review, "column": "queued", "index": 0 })),
        ("board_move", json!({ "card": mine, "column": "review" })),
        ("board_move", json!({ "card": mine, "column": "claude" })),
        ("board_move", json!({ "card": mine_done, "column": "review" })),
        ("board_move", json!({ "card": waits, "column": "done" })),
        ("board_move", json!({ "card": beyond, "column": "done" })),
    ];
    for (tool, args) in cases {
        let reply = pad.call(&alpha, tool, args.clone());
        assert!(reply.error, "{tool} {args}: {}", reply.text);
        assert_eq!(pad.core.board_state(), before, "{tool} {args} changed the board");
    }

    // The user's card: a note added under theirs, signed as board_note signs, the rest kept.
    let reply = pad.call(&alpha, "board_edit", json!({ "card": mine, "note": "Found the cause." }));
    assert!(!reply.error, "{}", reply.text);
    let noted = card(&mine);
    assert_eq!((noted.title.as_str(), noted.column), ("The user's card", Column::Queued));
    let (theirs, added) = noted.note.split_once("\n\n").unwrap();
    let (signature, text) = added.split_once('\n').unwrap();
    assert_eq!((theirs, text), ("the user's words", "Found the cause."));
    assert!(signature.starts_with("Claude · ") && signature.len() == "Claude · 2026-11-19 14:02".len(), "{signature}");

    // The card the user rewrote takes a signed note too.
    let reply = pad.call(&alpha, "board_edit", json!({ "card": edited, "note": "Found it." }));
    assert!(!reply.error, "{}", reply.text);
    let noted = card(&edited);
    assert!(noted.title == "Rewritten by the user" && noted.note.starts_with("the user's words now\n\nClaude · "), "{noted:?}");

    // The radio's own card is rewritten whole.
    let reply = pad.call(&alpha, "board_edit", json!({ "card": radio, "title": "Rewritten", "note": "new words", "mode": "replace" }));
    assert!(!reply.error, "{}", reply.text);
    assert_eq!((card(&radio).title.as_str(), card(&radio).note.as_str()), ("Rewritten", "new words"));

    // A user's card moves to "Done" and back to the top of "Up next".
    assert!(!pad.call(&alpha, "board_move", json!({ "card": mine, "column": "done" })).error);
    assert_eq!(card(&mine).column, Column::Done);
    assert!(!pad.call(&alpha, "board_move", json!({ "card": mine, "column": "queued", "index": 0 })).error);
    let queued: Vec<String> = pad.core.board_state().into_iter().filter(|card| card.path == alpha && card.column == Column::Queued).map(|card| card.id).collect();
    assert_eq!(queued.first(), Some(&mine));

    // The radio's cards in "Up next" and "Done" are deleted, and nothing else is.
    let deleted = [&radio, &radio_done, &moved];
    for id in deleted {
        let reply = pad.call(&alpha, "board_delete", json!({ "card": id }));
        assert!(!reply.error, "{}", reply.text);
    }
    let mut left: Vec<String> = pad.core.board_state().into_iter().map(|card| card.id).collect();
    let mut kept: Vec<String> = before.iter().map(|card| card.id.clone()).filter(|id| !deleted.contains(&id)).collect();
    left.sort();
    kept.sort();
    assert_eq!(left, kept);
    for card in pad.core.board_state().into_iter().filter(|card| card.id != mine && card.id != edited) {
        let was = before.iter().find(|was| was.id == card.id).unwrap();
        assert_eq!((&card.title, &card.note, card.column, &card.session, card.schedule), (&was.title, &was.note, was.column, &was.session, was.schedule));
    }
}

/// A session takes a card for the work it does now: the turn under way when it took it is the
/// card's, so the card goes to review once that turn ends, not one turn later (a card linked to a
/// busy session waits for the next), whether it ends with a finished turn or the user's Esc. The
/// end of an earlier turn, still what was last read of the session, is not the card's. In review
/// it stays when the session works on, for the session is a general one, not the card's own (a
/// card the user links to it again follows it back, as a given card does). A lap's session takes
/// no card, nor does a call whose session can't be told, nor a session that holds one already.
#[cfg(unix)]
#[test]
fn a_taken_card_goes_with_the_turn_that_took_it() {
    use std::os::unix::process::CommandExt;

    use crate::claude::TurnKind;
    use crate::core::laps::LapRun;

    let pad = paddock(true);
    let alpha = pad.project("alpha");
    pad.core.radio_tool("board_take", true).unwrap();

    // Two Claude Code sessions in alpha: one at work for the user, one a lap's run holds.
    let session = "9bb85e86-f618-4861-9858-000000000001";
    let lap = "9bb85e86-f618-4861-9858-000000000002";
    let sessions = pad.tmp.path().join("claude").join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let claude = |id: &str| {
        let child = std::process::Command::new("/bin/sleep").arg0("claude").arg("30").spawn().unwrap();
        let file = json!({ "pid": child.id(), "sessionId": id, "cwd": alpha, "startedAt": now_ms() });
        fs::write(sessions.join(format!("{}.json", child.id())), file.to_string()).unwrap();
        child
    };
    let (own, laps) = (claude(session), claude(lap));
    assert!(process::seen_with(own.id(), "claude") && process::seen_with(laps.id(), "claude"));
    pad.core.laps.lock().unwrap().runs.entry("l-1".into()).or_default().push(LapRun { session: Some(lap.into()), ..Default::default() });

    let take = |pid: u32, args: Value| pad.core.radio_call(&Caller { cwd: alpha.clone(), ppid: pid }, "board_take", &args);
    // Where the session stands, as the hook tells it (`interrupted`: its turn was, with Esc); then
    // the board follows it.
    let session_is = |phase: SessionPhase, turn: Option<(TurnKind, u64)>, interrupted: bool| {
        {
            let mut inner = pad.core.lock();
            inner.scanned.retain(|state| state.id != session);
            let turn = turn.map(|(kind, at)| Turn { kind, at });
            inner.scanned.push(SessionState { id: session.into(), agent: Agent::Claude, path: alpha.clone(), folder: None, title: None, phase, turn, since: Some(now_ms()), hooked: true, ended: false, tool: None, subagents: 0, helping: 0, interrupted });
        }
        pad.core.follow_board();
    };
    let card = |title: &str| pad.core.board_add(&alpha, title, "", &[]).unwrap().id;
    let (first, second, third) = (card("First"), card("Second"), card("Third"));
    let column = |id: &str| pad.core.board_state().into_iter().find(|card| card.id == id).unwrap().column;

    // No session to tell, or a lap's: nothing is taken.
    let reply = pad.call(&alpha, "board_take", json!({}));
    assert!(reply.error && reply.text.contains("can't tell which session"), "{}", reply.text);
    let reply = take(laps.id(), json!({ "card": first }));
    assert!(reply.error && reply.text.contains("laps"), "{}", reply.text);
    assert_eq!((column(&first), column(&second)), (Column::Queued, Column::Queued));

    // Taken at work, the top card is the session's; one at a time.
    session_is(SessionPhase::Working, None, false);
    let reply = take(own.id(), json!({}));
    assert!(!reply.error, "{}", reply.text);
    let taken = pad.core.board_state().into_iter().find(|card| card.id == first).unwrap();
    assert_eq!((taken.column, taken.session.as_deref()), (Column::Claude, Some(session)));
    pad.core.follow_board();
    assert_eq!(column(&first), Column::Claude, "it stays while the turn runs");
    let reply = take(own.id(), json!({ "card": second }));
    assert!(reply.error && reply.text.contains("already"), "{}", reply.text);
    assert_eq!(column(&second), Column::Queued);
    // The turn that took it ends: to review, now.
    session_is(SessionPhase::Waiting, Some((TurnKind::Finished, now_ms() + 1_000)), false);
    assert_eq!(column(&first), Column::Review);

    // What was last read of the session is the end of the turn before: not the card's end.
    session_is(SessionPhase::Waiting, Some((TurnKind::Finished, now_ms() - 60_000)), false);
    let reply = take(own.id(), json!({ "card": second }));
    assert!(!reply.error, "{}", reply.text);
    pad.core.follow_board();
    assert_eq!(column(&second), Column::Claude, "an earlier turn's end is not the card's");
    session_is(SessionPhase::Waiting, Some((TurnKind::Finished, now_ms() + 1_000)), false);
    assert_eq!(column(&second), Column::Review);

    // At work again, the session leaves the card it took in review, where the user looks at it:
    // its next turn is other work. So the next take needs no board_note first. A turn the user
    // interrupts (Esc, which tells no time) ends the card taken in it too.
    session_is(SessionPhase::Working, None, false);
    assert_eq!(column(&second), Column::Review, "a taken card stays in review when its session works again");
    assert!(!take(own.id(), json!({ "card": third })).error);
    session_is(SessionPhase::Idle, None, true);
    assert_eq!((column(&second), column(&third)), (Column::Review, Column::Review));

    // Linked to the session again by the user, the card is a given one: it comes back with its
    // session as a card given from the board does.
    pad.core.board_link(&third, session).unwrap();
    session_is(SessionPhase::Working, None, false);
    session_is(SessionPhase::Waiting, Some((TurnKind::Finished, now_ms() + 1_000)), false);
    assert_eq!(column(&third), Column::Review);
    session_is(SessionPhase::Working, None, false);
    assert_eq!(column(&third), Column::Claude);

    for mut child in [own, laps] {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// While the board's file can't be written, a card the file says waits is given by nobody
/// (`a_waiting_card_is_not_given_by_hand_while_the_board_cant_be_written` in `board.rs`): once
/// given here, the file would still say it waits, and its moment would give it a second time. A
/// take over the radio is a give to the calling session, so it refuses that card too, by its id
/// as from the top of "Up next".
#[test]
fn a_card_the_file_says_waits_is_not_taken_while_the_board_cant_be_written() {
    let pad = paddock(true);
    let alpha = pad.project("alpha");
    let project = pad.core.radio_project(&alpha).unwrap();
    let session = "9bb85e86-f618-4861-9858-0000000000a1";
    let file = pad.tmp.path().join("config").join("board.json");

    let card = pad.core.board_add(&alpha, "Waits for its moment", "", &[]).unwrap().id;
    pad.core.board_schedule(&card, now_ms() + 3_600_000, false, crate::start::Start::default()).unwrap();
    assert!(fs::read_to_string(&file).unwrap().contains("schedule"));
    // The disk refuses the board's file; the wait called off lands in memory only.
    fs::remove_file(&file).unwrap();
    fs::create_dir_all(file.join("in the way")).unwrap();
    pad.core.board_unschedule(&card);

    // The window's give refuses it, as its own test holds.
    assert!(pad.core.board_give_with(&card, crate::core::board::Give::New, None, None).is_err());

    for args in [json!({ "card": card }), json!({})] {
        let (reply, _) = pad.core.radio_board_take(Agent::Claude, &project, Some(session), &args);
        let column = pad.core.board_state().into_iter().find(|known| known.id == card).unwrap().column;
        assert!(reply.error && column == Column::Queued, "{args}: taken while the file says it waits: {} ({column:?})", reply.text);
    }
}

/// A session in one of the laps' own tabs (`apart`) that no run holds yet (a Codex lap's before
/// its run has tied it, a session started again in a lap's tab) is the laps' layer
/// (`radio_lap_layer`): the lap tools refuse it, and it takes no card either.
#[cfg(unix)]
#[test]
fn a_session_in_a_laps_tab_takes_no_card() {
    use crate::core::terminal::Launch;

    let pad = paddock(true);
    let alpha = pad.project("alpha");
    pad.core.radio_tool("board_take", true).unwrap();
    pad.core.radio_tool("lap_add", true).unwrap();
    let card = pad.core.board_add(&alpha, "The user's card", "", &[]).unwrap().id;

    // A tab of the laps' own, and under its shell a stand-in for Claude Code.
    let shell = Launch::Shell { shell: None, elevated: false };
    let tab = pad.core.open_terminal_apart(&alpha, &shell, "nightly lap".into(), Some((80, 24)), None).unwrap().id;
    let pid_file = pad.tmp.path().join("claude.pid");
    let line = format!("/bin/sh -c 'echo $$ > {}; sleep 30; :' claude &\n", pid_file.display());
    let until = std::time::Instant::now() + Duration::from_secs(20);
    let mut typed = std::time::Instant::now() - Duration::from_secs(5);
    let pid: u32 = loop {
        if let Some(pid) = fs::read_to_string(&pid_file).ok().and_then(|text| text.trim().parse().ok()) {
            break pid;
        }
        assert!(std::time::Instant::now() < until, "the stand-in never started");
        if typed.elapsed() > Duration::from_secs(3) {
            pad.core.write_terminal(tab, &line);
            typed = std::time::Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(process::seen_with(pid, "claude"));
    let session = "9bb85e86-f618-4861-9858-0000000000b2";
    let sessions = pad.tmp.path().join("claude").join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join(format!("{pid}.json")), json!({ "pid": pid, "sessionId": session, "cwd": alpha, "startedAt": now_ms() }).to_string()).unwrap();
    assert!(!pad.core.lap_sessions().contains(session), "no run holds it");

    let caller = Caller { cwd: alpha.clone(), ppid: pid };
    let job = json!({ "name": "bred", "kind": "agent", "prompt": "Again", "when": { "kind": "daily", "at": "09:00" } });
    let reply = pad.core.radio_call(&caller, "lap_add", &job);
    assert!(reply.error && reply.text.contains("one of Pitwall's laps"), "the lap tools call it a lap's: {}", reply.text);

    let reply = pad.core.radio_call(&caller, "board_take", &json!({}));
    let taken = pad.core.board_state().into_iter().find(|known| known.id == card).unwrap();
    pad.core.close_terminal(tab);
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    assert!(reply.error && taken.column == Column::Queued, "a session of the laps' layer took the card: {} ({:?}, {:?})", reply.text, taken.column, taken.session);
}
