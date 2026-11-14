use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::chat::control::Always;

thread_local! {
    /// The `claude` a test's chats run: a stand-in script.
    pub(super) static PROGRAM: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

const EMAIL: &str = "secret@example.com";
const ORG: &str = "Secret Org";

/// A stand-in for `claude -p --input-format stream-json`: answers `initialize` with an account
/// (its email and organisation), says it requires action and asks `cli-1` (with a suggestion)
/// and `cli-2`, which it withdraws at once; answers `get_status` with the rows that name the
/// account; asks `cli-3` once `cli-1` is answered. It writes into `log` that it started, every
/// answer to its requests it gets, and `eof` when its stdin ends.
const FAKE: &str = r#"#!/bin/sh
LOG='@LOG@'
echo start >> "$LOG"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"type":"control_response"'*)
      echo "answer $line" >> "$LOG"
      case "$line" in
        *'"request_id":"cli-1"'*)
          printf '%s\n' '{"type":"control_request","request_id":"cli-3","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm -rf build"}}}'
          ;;
      esac
      ;;
    *'"subtype":"initialize"'*)
      printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"'"$id"'","response":{"commands":[],"models":[{"value":"haiku"}],"account":{"email":"secret@example.com","organization":"Secret Org","subscriptionType":"max","apiProvider":"firstParty"}}}}'
      printf '%s\n' '{"type":"system","subtype":"session_state_changed","state":"requires_action"}'
      printf '%s\n' '{"type":"control_request","request_id":"cli-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"ls:*"}],"behavior":"allow","destination":"localSettings"}]}}'
      printf '%s\n' '{"type":"control_request","request_id":"cli-2","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"/tmp/x","content":"a"}}}'
      printf '%s\n' '{"type":"control_cancel_request","request_id":"cli-2"}'
      ;;
    *'"subtype":"get_status"'*)
      printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"'"$id"'","response":{"sections":[{"title":"Session","rows":[{"label":"Version","value":"2.1.325"},{"label":"Login method","value":"Claude Max account"},{"label":"Organization","value":"Secret Org"},{"label":"Email","value":"secret@example.com"},{"label":"Signed in as","value":"Secret Org (secret@example.com)"}]}]}}}'
      ;;
  esac
done
echo eof >> "$LOG"
"#;

fn fake_claude(dir: &Path, log: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let program = dir.join("claude");
    fs::write(&program, FAKE.replace("@LOG@", &log.to_string_lossy())).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn core_in(tmp: &Path, sink: Sink) -> Arc<Core> {
    Core::new(
        CoreConfig {
            registry_dir: tmp.join("registry"),
            claude_dir: tmp.join("claude").join("projects"),
            codex_home: tmp.join("codex"),
            settings_file: tmp.join("config").join("settings.json"),
            claude_settings: tmp.join("claude").join("settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        },
        sink,
    )
}

/// Every chat event the core sent, as the window would get it.
fn heard() -> (Arc<Mutex<Vec<String>>>, Sink) {
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let into = Arc::clone(&heard);
    let sink: Sink = Arc::new(move |event| {
        let text = match event {
            CoreEvent::Chat { id, seq, frames } => json!({ "id": id, "seq": seq, "frames": frames }).to_string(),
            CoreEvent::ChatExit { id, code, reason, stderr } => json!({ "id": id, "code": code, "reason": reason, "stderr": stderr }).to_string(),
            _ => return,
        };
        into.lock().unwrap().push(text);
    });
    (heard, sink)
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn pending(core: &Core, id: u64) -> Vec<String> {
    core.chat_buffer(id, 0).map(|buffer| buffer.pending.into_iter().map(|pending| pending.request_id).collect()).unwrap_or_default()
}

fn log_of(log: &Path) -> String {
    fs::read_to_string(log).unwrap_or_default()
}

/// A listed project with its chats allowed, a stand-in `claude` and its log.
fn project(tmp: &Path, core: &Core) -> (String, PathBuf) {
    let path = tmp.join("paddock");
    fs::create_dir_all(&path).unwrap();
    let path = path.to_string_lossy().into_owned();
    core.add_project(&path).unwrap();
    let log = tmp.join("fake.log");
    let program = fake_claude(&tmp.join("bin"), &log);
    PROGRAM.with(|own| *own.borrow_mut() = Some(program));
    (path, log)
}

/// Tests 3 and 5: no card of the CLI's is answered but by `chat_answer` naming it, a withdrawn
/// one and one answered already never are, "always" sends only a copy of the request's own
/// suggestion placed where the user picked, and a closed tab answers nothing: the CLI's card ends
/// with its stdin. The account's email and organisation, in the `initialize` reply and in a
/// control's reply, reach no event, no reply and no snapshot.
#[test]
fn a_card_is_answered_by_the_user_alone_and_nothing_names_the_account() {
    let tmp = tempfile::tempdir().unwrap();
    let (events, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log) = project(tmp.path(), &core);

    assert_eq!(core.open_chat(&path, ChatOpen::default()).unwrap_err(), UNTRUSTED);
    core.chat_trust(&path).unwrap();
    let id = core.open_chat(&path, ChatOpen::default()).unwrap().id;

    wait_for("the cards", || {
        let buffer = core.chat_buffer(id, 0).unwrap();
        buffer.state == ChatState::RequiresAction && buffer.frames.iter().any(|frame| frame["type"] == "control_cancel_request")
    });
    assert_eq!(pending(&core, id), ["cli-1"], "the withdrawn card is no card");
    assert!(!log_of(&log).contains("answer "), "nothing answered unasked: {}", log_of(&log));

    let deny = ChatAnswer::Deny { message: String::new(), interrupt: false };
    assert!(core.chat_answer(id, "cli-2", &deny).is_err(), "a withdrawn card is never answered");
    let always = ChatAnswer::Allow { input: None, always: Some(Always { suggestions: vec![0], destination: "userSettings".into() }), mode: None };
    core.chat_answer(id, "cli-1", &always).unwrap();
    assert!(core.chat_answer(id, "cli-1", &always).is_err(), "a card is answered once");

    let status = core.chat_control(id, &ChatControl::Status).unwrap().to_string();
    assert!(status.contains("2.1.325"), "{status}");
    for secret in [EMAIL, ORG, "Organization"] {
        assert!(!status.contains(secret), "{secret} in {status}");
    }

    wait_for("the answer", || log_of(&log).contains("\"cli-1\""));
    let answers: Vec<Value> = log_of(&log).lines().filter_map(|line| line.strip_prefix("answer ")).map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(answers.len(), 1, "{answers:?}");
    let response = &answers[0]["response"];
    assert_eq!(response["request_id"], "cli-1");
    assert_eq!(response["response"]["updatedInput"], json!({ "command": "ls" }));
    assert_eq!(
        response["response"]["updatedPermissions"],
        json!([{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "ls:*" }], "behavior": "allow", "destination": "userSettings" }])
    );

    wait_for("the next card", || pending(&core, id) == ["cli-3"]);
    core.close_terminal(id);
    wait_for("the end of stdin", || log_of(&log).contains("eof"));
    assert!(!log_of(&log).contains("cli-3\""), "a closed tab answers nothing: {}", log_of(&log));
    assert!(core.chat_buffer(id, 0).is_err());

    wait_for("the chat's last event", || events.lock().unwrap().iter().any(|event| event.contains("\"reason\":\"closed\"")));
    let said = events.lock().unwrap().join("\n");
    assert!(said.contains("\"subscriptionType\":\"max\""), "the info frame came");
    let snapshot = serde_json::to_string(&core.snapshot()).unwrap();
    for secret in [EMAIL, ORG] {
        assert!(!said.contains(secret), "{secret} reached an event");
        assert!(!snapshot.contains(secret), "{secret} reached the snapshot");
    }
    core.dispose();
}

/// Test 7: the quit's and the update's records name a chat by its session id and its kind alone,
/// nothing it said; a chat that sent nothing isn't written; and putting either record back while
/// the chat still runs its session opens nothing a second time.
#[test]
fn a_chat_is_recorded_by_its_session_alone_and_never_opened_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log) = project(tmp.path(), &core);
    core.chat_trust(&path).unwrap();
    // Claude Code's running sessions can be read: this one isn't among them.
    fs::create_dir_all(tmp.path().join("claude").join("sessions")).unwrap();

    let spoke = core.open_chat(&path, ChatOpen::default()).unwrap();
    let quiet = core.open_chat(&path, ChatOpen::default()).unwrap();
    wait_for("both chats", || log_of(&log).matches("start").count() == 2);
    let words = "what the user wrote in the chat";
    core.chat_send(spoke.id, ChatOutgoing::text(words.into())).unwrap();
    let session = spoke.chat.as_ref().and_then(|chat| chat.session.clone()).unwrap();
    let silent = quiet.chat.as_ref().and_then(|chat| chat.session.clone()).unwrap();
    // Its log, written a moment ago.
    let logs = tmp.path().join("claude").join("projects").join("-paddock");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join(format!("{session}.jsonl")), "{\"type\":\"user\"}\n").unwrap();

    core.save_last_session();
    core.save_restore("9.9.9", false).unwrap();
    for file in ["last-session.json", "restore.json"] {
        let record = fs::read_to_string(tmp.path().join("config").join(file)).unwrap();
        assert!(record.contains(&session) && record.contains("\"kind\":\"chat\""), "{file}: {record}");
        assert!(!record.contains(&silent), "{file}: a chat that sent nothing is in it");
        assert!(!record.contains(words), "{file}: what was said is in it");
    }

    let tabs = || core.snapshot().projects.iter().map(|project| project.terminals.len()).sum::<usize>();
    assert_eq!(tabs(), 2);
    core.resume_start(false);
    core.resume_restore(false);
    assert!(core.restore().is_some());
    assert_eq!(tabs(), 2, "nothing opened a second time");
    assert_eq!(log_of(&log).matches("start").count(), 2, "no second process");

    core.dispose();
    wait_for("the ends of stdin", || log_of(&log).matches("eof").count() == 2);
}

/// A quieter stand-in: answers `initialize` and goes idle; runs a turn for each message (a
/// `/clear` resets the conversation, and the process goes on under `CLEARED`); answers a rewind
/// as possible; starts a turn by itself on `get_context_usage` (as a background task's
/// notification would); ends on `get_usage`. With `die` in its folder it ends at once, with
/// `child` it leaves a `sleep` in its group. It writes into `log` each start with its arguments
/// and each rewind it is asked.
const QUIET: &str = r#"#!/bin/sh
LOG='@LOG@'
DIR='@DIR@'
echo "start $*" >> "$LOG"
[ -e "$DIR/die" ] && exit 3
if [ -e "$DIR/child" ]; then sleep 60 </dev/null >/dev/null 2>&1 & fi
sid= res= prev=
for arg in "$@"; do
  [ "$prev" = "--session-id" ] && sid=$arg
  [ "$prev" = "--resume" ] && res=$arg
  prev=$arg
done
[ -n "$sid" ] || sid=$res
answer() {
  printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"'"$1"'","response":'"$2"'}}'
}
state() {
  printf '%s\n' '{"type":"system","subtype":"session_state_changed","state":"'"$1"'","session_id":"'"$sid"'"}'
}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"subtype":"initialize"'*) answer "$id" '{"commands":[],"models":[]}'; state idle ;;
    *'"subtype":"rewind_files"'*) echo "rewind $line" >> "$LOG"; answer "$id" '{"canRewind":true,"filesChanged":[],"insertions":0,"deletions":0}' ;;
    *'"subtype":"get_context_usage"'*) answer "$id" '{}'; state running; state idle ;;
    *'"subtype":"get_usage"'*) answer "$id" '{}'; exit 0 ;;
    *'"type":"control_request"'*) answer "$id" '{}' ;;
    *'"type":"user"'*)
      state running
      case "$line" in
        *'/clear'*)
          printf '%s\n' '{"type":"conversation_reset","new_conversation_id":"11111111-2222-4333-8444-555555555555","session_id":"'"$sid"'"}'
          sid='@CLEARED@'
          printf '%s\n' '{"type":"system","subtype":"init","cwd":"/","session_id":"'"$sid"'"}'
          ;;
      esac
      printf '%s\n' '{"type":"result","subtype":"success","session_id":"'"$sid"'"}'
      state idle
      ;;
  esac
done
"#;

/// The session the quiet stand-in goes on under after a `/clear`.
const CLEARED: &str = "3f2b6c1e-8d4a-4f7b-9c1d-2a5e6b7c8d9e";

/// A listed project with its chats allowed and the quiet stand-in: the project, the stand-in's
/// log and its folder.
fn quiet_project(tmp: &Path, core: &Core) -> (String, PathBuf, PathBuf) {
    let path = tmp.join("paddock");
    fs::create_dir_all(&path).unwrap();
    let path = path.to_string_lossy().into_owned();
    core.add_project(&path).unwrap();
    core.chat_trust(&path).unwrap();
    // Claude Code's running sessions can be read: the chats' aren't among them.
    fs::create_dir_all(tmp.join("claude").join("sessions")).unwrap();
    let (dir, log) = (tmp.join("bin"), tmp.join("quiet.log"));
    fs::create_dir_all(&dir).unwrap();
    let program = dir.join("claude");
    let script = QUIET.replace("@LOG@", &log.to_string_lossy()).replace("@DIR@", &dir.to_string_lossy()).replace("@CLEARED@", CLEARED);
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    PROGRAM.with(|own| *own.borrow_mut() = Some(program));
    (path, log, dir)
}

/// Each start of the stand-in, its arguments.
fn starts(log: &Path) -> Vec<String> {
    log_of(log).lines().filter_map(|line| line.strip_prefix("start ")).map(str::to_string).collect()
}

fn state_of(core: &Core, id: u64) -> Option<ChatState> {
    core.chat_buffer(id, 0).ok().map(|buffer| buffer.state)
}

fn session_of(core: &Core, id: u64) -> String {
    core.chat_view(id).and_then(|view| view.chat?.session).unwrap()
}

/// Opens a chat as `open` says and waits for its process to be ready.
fn ready_chat(core: &Arc<Core>, path: &str, open: ChatOpen) -> u64 {
    let id = core.open_chat(path, open).unwrap().id;
    wait_for("the chat to be ready", || state_of(core, id) == Some(ChatState::Idle));
    id
}

/// Ends the chat's process (the stand-in's `get_usage`) and waits for its end.
fn end_chat(core: &Core, id: u64) {
    let _ = core.chat_control(id, &ChatControl::Usage);
    wait_for("the chat's end", || state_of(core, id) == Some(ChatState::Exited));
}

/// Test 8, the gate's other half: a rewind changes files only right after a dry run of it.
/// Whatever may have changed the files since asks for the dry run again: a message the user sent,
/// a turn the CLI began by itself, a new process.
#[test]
fn a_rewind_goes_only_on_a_dry_run_nothing_came_after() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, _) = quiet_project(tmp.path(), &core);
    let id = ready_chat(&core, &path, ChatOpen::default());
    let message = new_uuid();
    let rewind = |dry_run: bool| core.chat_control(id, &ChatControl::RewindFiles { message: message.clone(), dry_run });
    let turns = || core.chat_buffer(id, 0).unwrap().frames.iter().filter(|frame| frame["kind"] == "state" && frame["state"] == "running").count();

    rewind(true).unwrap();
    rewind(false).expect("a rewind right after its dry run goes");

    let ways: [(&str, &dyn Fn()); 3] = [
        ("a message sent", &|| {
            core.chat_send(id, ChatOutgoing::text("go on".into())).unwrap();
        }),
        ("a turn the CLI began", &|| {
            let before = turns();
            core.chat_control(id, &ChatControl::ContextUsage).unwrap();
            wait_for("the CLI's turn", || turns() > before);
        }),
        ("a new process", &|| {
            end_chat(&core, id);
            core.chat_restart(id).unwrap();
            wait_for("the new process", || state_of(&core, id) == Some(ChatState::Idle));
        }),
    ];
    for (way, happen) in ways {
        rewind(true).unwrap();
        happen();
        assert_eq!(rewind(false).unwrap_err(), t!("chatCore.error.rewindUnchecked"), "after {way}, a rewind went on the dry run before it");
    }
    assert_eq!(log_of(&log).matches("\"dry_run\":false").count(), 1, "the one rewind that went: {}", log_of(&log));
    core.dispose();
}

/// Test 6, a process left behind: what a chat's Claude leaves in its process group (an MCP
/// server, a command it ran in the background) goes with it, whether it ended by itself or its
/// tab was closed.
#[test]
fn what_a_chats_claude_started_ends_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, _, dir) = quiet_project(tmp.path(), &core);
    fs::write(dir.join("child"), "").unwrap();

    for closed in [false, true] {
        let id = ready_chat(&core, &path, ChatOpen::default());
        let pid = core.chat_shells().into_iter().find(|(_, tab, _)| *tab == id).map(|(pid, _, _)| pid).unwrap();
        assert!(process::group_alive(pid));
        if closed {
            core.close_terminal(id);
        } else {
            end_chat(&core, id);
        }
        wait_for("its group to be gone", || !process::group_alive(pid));
    }
    core.dispose();
}

/// Test 7's other half, a session lost: a chat starts again on the session it holds. A message
/// that was only held (the process ended before it was ready) went nowhere: it is handed back,
/// and the chat starts its session anew rather than going on with one that has no log; a fork no
/// message went to is made again from its source, never a blank session under its id.
#[test]
fn a_chat_starts_again_on_the_session_it_holds() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, dir) = quiet_project(tmp.path(), &core);

    fs::write(dir.join("die"), "").unwrap();
    let prompt = ChatOutgoing::text("the first words".into());
    let id = core.open_chat(&path, ChatOpen { prompt: Some(prompt), ..ChatOpen::default() }).unwrap().id;
    wait_for("the failed start", || state_of(&core, id) == Some(ChatState::Exited));
    let frames = core.chat_buffer(id, 0).unwrap().frames;
    let unsent = frames.iter().find(|frame| frame["kind"] == "unsent").expect("the held message is handed back");
    assert_eq!(unsent["text"], "the first words");
    assert!(frames.iter().any(|frame| frame["kind"] == "error" && frame["reason"] == "unsent"));
    fs::remove_file(dir.join("die")).unwrap();
    let session = session_of(&core, id);
    core.chat_restart(id).unwrap();
    wait_for("the restart", || starts(&log).len() == 2);
    let again = starts(&log).pop().unwrap();
    assert!(again.contains(&format!("--session-id {session}")) && !again.contains("--resume"), "{again}");

    let from = new_uuid();
    let logs = tmp.path().join("claude").join("projects").join("-paddock");
    fs::create_dir_all(&logs).unwrap();
    let line = |kind: &str, text: &str| json!({ "type": kind, "uuid": new_uuid(), "sessionId": from, "message": { "role": kind, "content": [{ "type": "text", "text": text }] } }).to_string();
    fs::write(logs.join(format!("{from}.jsonl")), format!("{}\n{}\n", line("user", "hi"), line("assistant", "hello"))).unwrap();
    let fork = ready_chat(&core, &path, ChatOpen { fork: Some((from.clone(), None)), ..ChatOpen::default() });
    let copy = session_of(&core, fork);
    end_chat(&core, fork);
    let before = starts(&log).len();
    core.chat_restart(fork).unwrap();
    wait_for("the fork's restart", || starts(&log).len() > before);
    let again = starts(&log).pop().unwrap();
    assert!(again.contains(&format!("--resume {from} --fork-session --session-id {copy}")), "{again}");
    core.dispose();
}

/// Test 7's other half, a session doubled or lost: after a `/clear` the chat holds the session
/// its process goes on under (a record names it, nothing else opens it), and a chat never starts
/// again on a session another tab where Claude may run it holds.
#[test]
fn a_chat_holds_its_session_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, _) = quiet_project(tmp.path(), &core);

    let id = ready_chat(&core, &path, ChatOpen::default());
    let before = session_of(&core, id);
    core.chat_send(id, ChatOutgoing::text("/clear".into())).unwrap();
    wait_for("the new session", || session_of(&core, id) == CLEARED);
    core.chat_send(id, ChatOutgoing::text("after the clear".into())).unwrap();
    core.save_last_session();
    let record = fs::read_to_string(tmp.path().join("config").join("last-session.json")).unwrap();
    assert!(record.contains(CLEARED) && !record.contains(&before), "{record}");
    {
        let cache = core.sessions_cache();
        assert_eq!(cache.reopened.get(CLEARED), Some(&id));
        assert_ne!(cache.reopened.get(&before), Some(&id));
    }

    let held = ready_chat(&core, &path, ChatOpen::default());
    let session = session_of(&core, held);
    end_chat(&core, held);
    // The terminals' history opened it in another tab since, where Claude may run it.
    core.sessions_cache().reopened.insert(session.clone(), id);
    let before = starts(&log).len();
    assert_eq!(core.chat_restart(held).unwrap_err(), ELSEWHERE);
    assert_eq!(starts(&log).len(), before, "no second process");
    core.dispose();
}

/// Settings › Claude › Sohbet's start mode turns a Normal start (nothing named, a project's or a
/// card's Normal) to auto and nothing else: a start that names its mode keeps it, and no word a
/// settings file or a backup holds starts anything but auto or Claude Code's default.
#[test]
fn the_settings_start_mode_takes_only_a_normal_start() {
    let named = |mode| ChatStart { mode, ..ChatStart::default() };
    for (word, start, wanted) in [
        ("auto", named(ChatMode::Default), ChatMode::Auto),
        ("auto", named(ChatMode::Plan), ChatMode::Plan),
        ("auto", named(ChatMode::AcceptEdits), ChatMode::AcceptEdits),
        ("default", named(ChatMode::Default), ChatMode::Default),
        ("bypassPermissions", named(ChatMode::Default), ChatMode::Default),
        ("dontAsk", named(ChatMode::Default), ChatMode::Default),
        ("plan", named(ChatMode::Default), ChatMode::Default),
        ("", named(ChatMode::Default), ChatMode::Default),
    ] {
        assert_eq!(with_mode(start, chat_start_mode(word)).mode, wanted, "{word} over {:?}", start.mode);
    }
    // A chat starts in auto until the user says otherwise (the user's word, 14 Nov), and Normal
    // when they did.
    let mut settings = crate::settings::ChatSettings::default();
    let args = chat_args(with_mode(ChatStart::default(), chat_start_mode(&settings.mode)), &ChatSession::New(new_uuid()), false, false, None).unwrap();
    assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "auto"]), "{args:?}");
    settings.mode = "default".into();
    let args = chat_args(with_mode(ChatStart::default(), chat_start_mode(&settings.mode)), &ChatSession::New(new_uuid()), false, false, None).unwrap();
    assert!(!args.iter().any(|arg| arg == "auto"), "{args:?}");
}
