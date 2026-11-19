use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::chat::control::Always;

thread_local! {
    /// The `claude` a test's chats run: a stand-in script.
    pub(super) static PROGRAM: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// How long a test's chats are left alone before they sleep, and how often they are looked at.
    pub(super) static NAP: RefCell<Option<(Duration, Duration)>> = const { RefCell::new(None) };
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
/// `child` it leaves a `sleep` in its group, with `nomode` it refuses a mode. It writes into `log`
/// each start with its arguments, each rewind, each mode and each flag setting it is asked, and
/// each message it is sent.
///
/// With `sessions` in its folder (the path of Claude Code's sessions folder) it lists itself there
/// as Claude Code does, and takes itself out before it ends, a moment before; started on a
/// session it goes on with (`--resume`) it lists itself only a moment after its start, and first
/// says what a process says of itself as it starts (a hook of the session's start, its `init` in
/// the mode of its start, a state, a notice); a hook of the session's end as it ends.
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
FILE=
if [ -e "$DIR/sessions" ]; then
  FILE="$(cat "$DIR/sessions")/$$.json"
  entry() {
    printf '%s\n' '{"pid":'"$$"',"sessionId":"'"$sid"'","cwd":"/","entrypoint":"sdk-cli","status":"idle"}' > "$FILE"
  }
  if [ -n "$res" ]; then
    ( sleep 0.7; entry ) </dev/null >/dev/null 2>&1 &
    printf '%s\n' '{"type":"system","subtype":"hook_started","hook_event":"SessionStart","hook_name":"SessionStart:resume"}'
    printf '%s\n' '{"type":"system","subtype":"init","permissionMode":"auto","session_id":"'"$sid"'"}'
    state running
    state idle
    printf '%s\n' '{"type":"system","subtype":"informational","content":"resumed"}'
  else
    entry
  fi
fi
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"subtype":"initialize"'*) answer "$id" '{"commands":[],"models":[]}'; state idle ;;
    *'"subtype":"set_permission_mode"'*)
      if [ -e "$DIR/nomode" ]; then
        printf '%s\n' '{"type":"control_response","response":{"subtype":"error","request_id":"'"$id"'","error":"no such mode"}}'
      else
        echo "mode $line" >> "$LOG"; answer "$id" '{}'
      fi
      ;;
    *'"subtype":"rewind_files"'*) echo "rewind $line" >> "$LOG"; answer "$id" '{"canRewind":true,"filesChanged":[],"insertions":0,"deletions":0}' ;;
    *'"subtype":"get_context_usage"'*) answer "$id" '{}'; state running; state idle ;;
    *'"subtype":"get_usage"'*) answer "$id" '{}'; exit 0 ;;
    *'"subtype":"apply_flag_settings"'*) echo "flags $line" >> "$LOG"; answer "$id" '{}' ;;
    *'"type":"control_request"'*) answer "$id" '{}' ;;
    *'"type":"user"'*)
      echo "user $line" >> "$LOG"
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
if [ -n "$FILE" ]; then
  rm -f "$FILE"
  printf '%s\n' '{"type":"system","subtype":"hook_started","hook_event":"SessionEnd","hook_name":"SessionEnd"}'
  sleep 0.3
fi
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
fn end_chat(core: &Arc<Core>, id: u64) {
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

/// What the stand-in wrote into its log since its start number `start` (from 1), by `prefix`.
fn since_start(log: &Path, start: usize, prefix: &str) -> Vec<String> {
    let text = log_of(log);
    let mut seen = 0;
    text.lines().filter(|line| {
        seen += usize::from(line.starts_with("start "));
        seen >= start && line.starts_with(prefix)
    }).map(str::to_string).collect()
}

/// A chat left alone sleeps: its process is ended only once nothing of it runs (here a `!` run
/// holds it up), and nobody is told: no `chat-exit`, no state but idle, the tab and its session
/// held as before. What is sent to it asleep reaches the process it wakes with exactly once and
/// in the order sent, after the control the chat had taken, on the session it had.
#[test]
fn a_chat_asleep_wakes_for_a_message_and_loses_none() {
    let tmp = tempfile::tempdir().unwrap();
    let (events, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, _) = quiet_project(tmp.path(), &core);
    NAP.with(|nap| *nap.borrow_mut() = Some((Duration::from_millis(400), Duration::from_millis(40))));

    let id = ready_chat(&core, &path, ChatOpen::default());
    let session = session_of(&core, id);
    let logs = tmp.path().join("claude").join("projects").join("-paddock");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join(format!("{session}.jsonl")), "{\"type\":\"user\"}\n").unwrap();
    let pid = core.chat_shells().into_iter().find(|(_, tab, _)| *tab == id).map(|(pid, _, _)| pid).unwrap();
    core.chat_control(id, &ChatControl::SetEffort { level: "high".into() }).unwrap();

    core.chat_shell(id, "sleep 1.5").unwrap();
    thread::sleep(Duration::from_millis(1000));
    assert!(process::group_alive(pid), "it slept while a run of its was going");
    wait_for("its sleep", || !process::group_alive(pid));

    assert_eq!(state_of(&core, id), Some(ChatState::Idle));
    assert_eq!(core.chat_view(id).and_then(|view| view.chat).map(|chat| chat.state), Some(ChatState::Idle));
    assert_eq!(core.chat_alive(id), Some(true), "its session is no longer the tab's");
    assert!(matches!(core.liveness(&session), Liveness::Running { .. }), "its session reads as ended");
    assert_eq!(core.chat_truths().len(), 1);
    assert_eq!(starts(&log).len(), 1);

    core.chat_send(id, ChatOutgoing::text("the first".into())).unwrap();
    core.chat_send(id, ChatOutgoing::text("the second".into())).unwrap();
    wait_for("both messages", || since_start(&log, 2, "user ").len() >= 2);
    wait_for("the turns' end", || state_of(&core, id) == Some(ChatState::Idle) && core.chat_buffer(id, 0).unwrap().frames.iter().filter(|frame| frame["type"] == "result").count() == 2);
    let woke = starts(&log);
    assert_eq!(woke.len(), 2, "{woke:?}");
    assert!(woke[1].contains(&format!("--resume {session}")), "{}", woke[1]);
    let said = since_start(&log, 2, "");
    let at = |word: &str| said.iter().position(|line| line.contains(word)).unwrap_or_else(|| panic!("{word} in {said:?}"));
    assert!(at("\"effortLevel\":\"high\"") < at("the first") && at("the first") < at("the second"), "{said:?}");
    assert_eq!(since_start(&log, 1, "user ").len(), 2, "each message once: {said:?}");

    let frames = core.chat_buffer(id, 0).unwrap().frames;
    let told = |state: &str| frames.iter().filter(|frame| frame["kind"] == "state" && frame["state"] == state).count();
    assert_eq!((told("starting"), told("exited")), (1, 0), "the tab was told of the sleep or the wake");
    assert!(!frames.iter().any(|frame| frame["kind"] == "unsent" || frame["kind"] == "error"));
    assert!(!events.lock().unwrap().iter().any(|event| event.contains("\"reason\"")), "an exit was announced");
    core.dispose();
}

/// A chat asleep whose process doesn't start again says so as any failed start does, and the
/// message that woke it comes back for the composer.
#[test]
fn a_wake_that_fails_hands_the_message_back() {
    let tmp = tempfile::tempdir().unwrap();
    let (events, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, dir) = quiet_project(tmp.path(), &core);
    NAP.with(|nap| *nap.borrow_mut() = Some((Duration::from_millis(200), Duration::from_millis(40))));

    let id = ready_chat(&core, &path, ChatOpen::default());
    let pid = core.chat_shells().into_iter().find(|(_, tab, _)| *tab == id).map(|(pid, _, _)| pid).unwrap();
    wait_for("its sleep", || !process::group_alive(pid) && core.chat_shells().iter().any(|(shell, tab, _)| *tab == id && *shell != pid));
    assert!(!events.lock().unwrap().iter().any(|event| event.contains("\"reason\"")));

    fs::write(dir.join("die"), "").unwrap();
    core.chat_send(id, ChatOutgoing::text("words not to lose".into())).unwrap();
    wait_for("the failed wake", || state_of(&core, id) == Some(ChatState::Exited));
    let frames = core.chat_buffer(id, 0).unwrap().frames;
    let unsent = frames.iter().find(|frame| frame["kind"] == "unsent").expect("the message is handed back");
    assert_eq!(unsent["text"], "words not to lose");
    wait_for("its exit", || events.lock().unwrap().iter().any(|event| event.contains("\"reason\":\"startFailed\"")));
    assert_eq!(starts(&log).len(), 2);
    assert_eq!(core.chat_alive(id), Some(false));

    // And it starts again as any exited chat does.
    fs::remove_file(dir.join("die")).unwrap();
    core.chat_restart(id).unwrap();
    wait_for("the restart", || state_of(&core, id) == Some(ChatState::Idle));
    core.dispose();
}

fn asleep(core: &Core, id: u64) -> bool {
    core.chat(id).is_some_and(|tab| {
        let shared = tab.lock();
        shared.rest == Rest::Asleep && !shared.running
    })
}

/// A chat's session runs on through its sleep and its wake, at every moment and in its tab: also
/// while its process ends (Claude Code's entry gone before the end is taken in) and while the
/// next one starts (its entry not written yet). One reading of it as ended would take a board
/// card of it to review and tell the user of work nobody did. The tab is told nothing of what the
/// process says as it ends and as it starts, the mode the chat was in is asked of the process it
/// wakes with before the message goes, and a control that finds the chat just falling asleep is
/// asked again of the process it wakes with.
#[test]
fn a_chats_session_runs_on_through_its_sleep_and_its_wake() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, dir) = quiet_project(tmp.path(), &core);
    fs::write(dir.join("sessions"), tmp.path().join("claude").join("sessions").to_string_lossy().as_bytes()).unwrap();
    // Longer than the woken stand-in takes to list itself.
    NAP.with(|nap| *nap.borrow_mut() = Some((Duration::from_millis(1500), Duration::from_millis(40))));

    let id = ready_chat(&core, &path, ChatOpen::default());
    let session = session_of(&core, id);
    let listed = |pid: u32| matches!(core.liveness(&session), Liveness::Running { lineage, .. } if lineage.first() == Some(&pid) && lineage.len() > 2);
    let pid = core.chat_shells().into_iter().find(|(_, tab, _)| *tab == id).map(|(pid, _, _)| pid).unwrap();
    wait_for("Claude Code's entry", || listed(pid));
    core.chat_control(id, &ChatControl::SetMode { mode: "plan".into() }).unwrap();

    // Every reading from here on: running, in this tab.
    let (done, lost) = (Arc::new(std::sync::atomic::AtomicBool::new(false)), Arc::new(Mutex::new(Vec::<String>::new())));
    let reader = {
        let (core, session, done, lost) = (Arc::clone(&core), session.clone(), Arc::clone(&done), Arc::clone(&lost));
        thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                match core.liveness(&session) {
                    Liveness::Running { lineage, .. } if core.pitwall_terminal(&lineage).map(|(tab, _)| tab) == Some(id) => {}
                    other => lost.lock().unwrap().push(format!("{other:?}")),
                }
                thread::sleep(Duration::from_millis(5));
            }
        })
    };

    wait_for("its sleep", || asleep(&core, id));
    core.chat_send(id, ChatOutgoing::text("the words".into())).unwrap();
    wait_for("the message", || since_start(&log, 2, "user ").len() == 1);
    let woken = core.chat_shells().into_iter().find(|(_, tab, _)| *tab == id).map(|(pid, _, _)| pid).unwrap();
    wait_for("the woken process's entry", || listed(woken));

    // As `watch_rest` lets it go, just after a control found it awake.
    let tab = core.chat(id).unwrap();
    let asked = std::cell::Cell::new(0);
    let status = core.chat_needing(&tab, || {
        if asked.replace(asked.get() + 1) == 0 {
            tab.lock().rest = Rest::Falling;
            end_process(&tab, true);
        }
        core.chat_control_now(&tab, &ChatControl::Status)
    });
    assert!(status.is_ok() && asked.get() == 2, "{status:?} after {} tries", asked.get());

    done.store(true, std::sync::atomic::Ordering::Relaxed);
    reader.join().unwrap();
    assert!(lost.lock().unwrap().is_empty(), "its session read as not running: {:?}", lost.lock().unwrap());

    let said = since_start(&log, 2, "");
    let at = |word: &str| said.iter().position(|line| line.contains(word)).unwrap_or_else(|| panic!("{word} in {said:?}"));
    assert!(at("\"mode\":\"plan\"") < at("the words"), "{said:?}");
    let buffer = core.chat_buffer(id, 0).unwrap();
    let told = |state: &str| buffer.frames.iter().filter(|frame| frame["kind"] == "state" && frame["state"] == state).count();
    assert_eq!(told("running"), 1, "a state of the wake reached the tab");
    let of_the_wake = |frame: &&Value| frame["type"] == "system" && (frame["subtype"].as_str().is_some_and(|subtype| subtype.starts_with("hook_")) || frame["subtype"] == "informational" || frame["permissionMode"] == "auto");
    assert_eq!(buffer.frames.iter().filter(of_the_wake).count(), 0, "what the process said as it ended or started reached the tab");
    assert_eq!(buffer.info.and_then(|info| info.permission_mode).as_deref(), Some("plan"));
    core.dispose();
}

/// A chat asleep starts no process it may not: its session runs no more once its tab is closed
/// (nothing would ever end it), a chat whose project is no longer listed stays without one, and
/// one whose process doesn't take the mode the chat was in sends it no message, which would run
/// in the mode of its start. Each message comes back for the composer.
#[test]
fn a_chat_asleep_wakes_only_where_and_as_it_stood() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let (path, log, dir) = quiet_project(tmp.path(), &core);
    NAP.with(|nap| *nap.borrow_mut() = Some((Duration::from_millis(200), Duration::from_millis(40))));
    let handed_back = |id: u64, words: &str| {
        wait_for("the failed wake", || state_of(&core, id) == Some(ChatState::Exited));
        let frames = core.chat_buffer(id, 0).unwrap().frames;
        let unsent = frames.iter().find(|frame| frame["kind"] == "unsent").expect("the message is handed back");
        assert_eq!(unsent["text"], words);
        assert!(!log_of(&log).contains(words), "it went to a process: {}", log_of(&log));
    };

    let (closed, moded, unlisted) = (ready_chat(&core, &path, ChatOpen::default()), ready_chat(&core, &path, ChatOpen::default()), ready_chat(&core, &path, ChatOpen::default()));
    core.chat_control(moded, &ChatControl::SetMode { mode: "plan".into() }).unwrap();
    wait_for("their sleep", || [closed, moded, unlisted].iter().all(|id| asleep(&core, *id)));
    // Counted from their sleep: on a slow machine a chat may fall asleep before its mode comes,
    // and be woken for it, a start of its own.
    let slept = starts(&log).len();

    let session = session_of(&core, closed);
    assert!(matches!(core.liveness(&session), Liveness::Running { .. }));
    core.close_terminal(closed);
    assert!(!core.running_cached().unwrap().contains_key(&session), "a closed chat's session still runs");

    fs::write(dir.join("nomode"), "").unwrap();
    core.chat_send(moded, ChatOutgoing::text("words for plan mode".into())).unwrap();
    handed_back(moded, "words for plan mode");
    assert_eq!(starts(&log).len(), slept + 1, "the closed chat started, or the wake started more than one process");

    core.remove_project(&path);
    assert!(!core.is_listed(&path));
    core.chat_send(unlisted, ChatOutgoing::text("words for a listed project".into())).unwrap();
    handed_back(unlisted, "words for a listed project");
    assert_eq!(starts(&log).len(), slept + 1, "a process started in a project that isn't listed");
    core.dispose();
}

/// A chat's process is let go only when every condition holds: one dataset over what keeps it.
#[test]
fn a_chat_sleeps_only_when_nothing_of_it_runs_waits_or_is_owed() {
    let rested = || {
        let mut shared = Shared::new(new_uuid(), true);
        shared.running = true;
        shared.ready = true;
        shared.state = ChatState::Idle;
        shared
    };
    assert!(rested().may_rest(Duration::ZERO));
    assert!(!rested().may_rest(Duration::from_secs(60)), "something happened in it a moment ago");
    let (reply, _) = mpsc::channel();
    let waiting = || Waiting { uuid: new_uuid(), line: String::new(), text: String::new(), carry: Vec::new() };
    type Keep = Box<dyn FnOnce(&mut Shared)>;
    let keeps: Vec<(&str, Keep)> = vec![
        ("a turn under way", Box::new(|shared| shared.state = ChatState::Running)),
        ("a card open", Box::new(|shared| shared.state = ChatState::RequiresAction)),
        ("starting", Box::new(|shared| shared.state = ChatState::Starting)),
        ("not ready", Box::new(|shared| shared.ready = false)),
        ("exited", Box::new(|shared| shared.running = false)),
        ("a card of a background subagent", Box::new(|shared| shared.pending.push(Pending { id: "cli-1".into(), request: Value::Null, at: 0 }))),
        ("a request awaiting its reply", Box::new(move |shared| shared.replies = HashMap::from([("req".to_string(), reply)]))),
        ("a message held", Box::new(move |shared| shared.held.push(waiting()))),
        ("a `!` run", Box::new(|shared| shared.shells = HashMap::from([("sh_1".to_string(), 1)]))),
        ("a background task", Box::new(|shared| shared.background = HashSet::from(["task".to_string()]))),
        ("a message streaming", Box::new(|shared| shared.streams = HashMap::from([(String::new(), "msg".to_string())]))),
        ("a conversation being reset", Box::new(|shared| shared.reset = true)),
        ("what only its process holds", Box::new(|shared| shared.pinned = true)),
        ("being closed", Box::new(|shared| shared.closing = true)),
        ("its tab closed", Box::new(|shared| shared.gone = true)),
        ("asleep already", Box::new(|shared| shared.rest = Rest::Asleep)),
    ];
    for (what, keep) in keeps {
        let mut shared = rested();
        keep(&mut shared);
        assert!(!shared.may_rest(Duration::ZERO), "it slept with {what}");
    }

    // What the frames tell of work its process still holds.
    let mut shared = rested();
    note_work(&mut shared, "system", "background_tasks_changed", &json!({ "tasks": [{ "task_id": "a" }, { "task_id": "b" }] }));
    note_work(&mut shared, "system", "task_notification", &json!({ "task_id": "a", "status": "completed" }));
    assert!(!shared.may_rest(Duration::ZERO), "a background task still runs");
    note_work(&mut shared, "system", "task_updated", &json!({ "task_id": "b", "patch": { "status": "killed" } }));
    assert!(shared.may_rest(Duration::ZERO));
    note_work(&mut shared, "system", "task_started", &json!({ "task_id": "c", "is_backgrounded": true }));
    assert!(!shared.may_rest(Duration::ZERO));
    note_work(&mut shared, "system", "background_tasks_changed", &json!({ "tasks": [] }));
    assert!(shared.may_rest(Duration::ZERO));
    note_work(&mut shared, "assistant", "", &json!({ "message": { "content": [{ "type": "tool_use", "name": "ScheduleWakeup", "input": {} }] } }));
    assert!(!shared.may_rest(Duration::ZERO), "it set itself a wakeup only its process knows of");

    // A chat that talks with another session waits for what that one writes to its process.
    for tool in ["SendMessage", "ListAgents"] {
        let mut shared = rested();
        note_work(&mut shared, "assistant", "", &json!({ "message": { "content": [{ "type": "tool_use", "name": tool, "input": {} }] } }));
        assert!(!shared.may_rest(Duration::ZERO), "{tool}: another session may write to it");
    }
}

/// A stand-in that streams: answers `initialize` and goes idle; for each message it starts a
/// turn and, a moment later, says forty partial frames and nothing more, and for a message saying `last` it ends
/// right after them.
const STREAMS: &str = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"subtype":"initialize"'*)
      printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"'"$id"'","response":{"commands":[],"models":[]}}}'
      printf '%s\n' '{"type":"system","subtype":"session_state_changed","state":"idle"}'
      ;;
    *'"type":"control_request"'*)
      printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"'"$id"'","response":{}}}'
      ;;
    *'"type":"user"'*)
      printf '%s\n' '{"type":"system","subtype":"session_state_changed","state":"running"}'
      sleep 0.3
      i=0
      while [ $i -lt 40 ]; do
        printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"word '"$i"'"}}}'
        i=$((i+1))
      done
      case "$line" in *last*) exit 0 ;; esac
      ;;
  esac
done
"#;

/// The frames of a chat the panel doesn't show are held back from the window, never lost: every
/// frame reaches it once, numbered one after another (a gap makes the window read the chat
/// again, a frame twice would be drawn twice), at once when the tab is shown, and the process's
/// end comes after its last frames. What tells the tab row of the chat (its state) isn't held.
#[test]
fn a_chat_not_shown_loses_no_frame_and_ends_after_them() {
    let tmp = tempfile::tempdir().unwrap();
    let (heard, sink) = heard();
    let core = core_in(tmp.path(), sink);
    let path = tmp.path().join("paddock");
    fs::create_dir_all(&path).unwrap();
    let path = path.to_string_lossy().into_owned();
    core.add_project(&path).unwrap();
    core.chat_trust(&path).unwrap();
    let program = tmp.path().join("bin").join("claude");
    fs::create_dir_all(program.parent().unwrap()).unwrap();
    fs::write(&program, STREAMS).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    PROGRAM.with(|own| *own.borrow_mut() = Some(program));

    // Longer than the test: only being shown, a frame that doesn't wait or the end sends them.
    core.shown.set_hold(Duration::from_secs(600));
    core.set_tabs_shown("main", Vec::new());
    let id = ready_chat(&core, &path, ChatOpen::default());

    // The last frame the window was sent, every event checked: its frames follow the last one's.
    let sent = || {
        let mut last = 0;
        for event in heard.lock().unwrap().iter().map(|text| serde_json::from_str::<Value>(text).unwrap()).filter(|event| event.get("frames").is_some()) {
            let (seq, frames) = (event["seq"].as_u64().unwrap(), event["frames"].as_array().unwrap().len() as u64);
            assert_eq!(seq - frames, last, "frames out of their place");
            last = seq;
        }
        last
    };
    let held = || core.chat_buffer(id, 0).unwrap().seq;
    let words = || core.chat_buffer(id, 0).unwrap().frames.iter().filter(|frame| frame["type"] == "stream_event").count();

    // Hidden: the turn's start is told (the tab row's dot), what it streams waits.
    core.chat_send(id, ChatOutgoing::text("go".into())).unwrap();
    wait_for("the stream", || words() == 40);
    thread::sleep(Duration::from_millis(200));
    assert!(sent() < held(), "a hidden chat's stream wasn't held");
    let told: Vec<Value> = heard.lock().unwrap().iter().map(|text| serde_json::from_str(text).unwrap()).collect();
    assert!(told.iter().any(|event| event["frames"].as_array().is_some_and(|frames| frames.iter().any(|frame| frame["kind"] == "state" && frame["state"] == "running"))), "the turn's start waited");

    // Shown, in another window: at once, whatever the hold.
    core.set_tabs_shown("terminals", vec![id]);
    let deadline = Instant::now() + Duration::from_secs(2);
    while sent() < held() {
        assert!(Instant::now() < deadline, "being shown didn't send what was held");
        thread::sleep(Duration::from_millis(10));
    }
    // What the first window says late hides no tab the other shows; that one gone, it is hidden.
    core.set_tabs_shown("main", Vec::new());
    assert!(core.shown.has(id), "a window hid a tab another one shows");

    // Hidden again, and the process ends mid-stream: its last frames first, then its end.
    core.tabs_window_gone("terminals");
    assert!(!core.shown.has(id));
    core.chat_send(id, ChatOutgoing::text("last".into())).unwrap();
    wait_for("the chat's end", || heard.lock().unwrap().iter().any(|text| text.contains("\"reason\"")));
    assert_eq!(sent(), held(), "the end overtook the last frames");
    assert!(heard.lock().unwrap().last().unwrap().contains("\"reason\""), "something came after the end");
    assert_eq!(words(), 80, "the core read the whole of both streams");

    core.dispose();
}
