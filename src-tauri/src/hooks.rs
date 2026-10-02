//! The Claude Code hook that reports a session's turns the moment they change: a prompt sent,
//! the turn over, a permission prompt or a question waiting, the session started or gone.
//! Installing it edits `~/.claude/settings.json`, so the rules are strict: back the file up
//! once, never touch a file that isn't valid JSON, keep every other key and hook in place, add
//! our entries once, and remove only our entries.
//!
//! On macOS the hook is a script, which reads Claude Code's JSON with `plutil`. Windows has no
//! `sh` and Linux no `plutil`, so there Claude Code runs the installed app itself in hook mode
//! (`<app> --claude-hook <Event>`, `hook_mode`), which writes the same file.

use std::collections::hash_map::RandomState;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::hash::BuildHasher;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};

use crate::i18n::t;
use crate::registry::write_atomic;

const SCRIPT_NAME: &str = "claude-hook.sh";
/// The first script, which saved the whole Notification input (message text included). Its
/// entries are replaced on install and removed on uninstall.
const LEGACY_SCRIPT_NAME: &str = "claude-notification.sh";
/// The flag that runs the app as the hook: `<app> --claude-hook <Event>`.
const HOOK_FLAG: &str = "--claude-hook";
/// How much of Claude Code's input hook mode reads. Values it doesn't keep are skipped as they
/// stream past, never held, so this only bounds the time: `PostToolUse` carries the tool's
/// output, which for an image runs to megabytes.
const INPUT_LIMIT: u64 = 64 << 20;

/// The events Pitwall listens to, with their matcher (`None`: the event takes none). A tool
/// call that finishes after a prompt answers it; `PreToolUse` only for the tools that ask.
/// `PermissionRequest` comes the moment a permission dialog opens, with the tool's name;
/// `Notification` reports the same dialog only 6 s later and stays for older Claude Codes.
const EVENTS: [(&str, Option<&str>); 8] = [
    ("SessionStart", Some("")),
    ("UserPromptSubmit", None),
    ("PreToolUse", Some("AskUserQuestion|ExitPlanMode")),
    ("PermissionRequest", Some("")),
    ("PostToolUse", Some("")),
    ("Notification", Some("")),
    ("Stop", None),
    ("SessionEnd", Some("")),
];

/// Writes the few fields Pitwall needs, one `key=value` per line, to a file in the events folder
/// and prints nothing. `plutil` reads the JSON on stdin; the input stays in memory, so the
/// prompt, messages and tool input never reach the disk. `@EVENTS@` is the events folder.
const SCRIPT: &str = r#"#!/bin/sh
# Pitwall: Claude Code hook, run as `claude-hook.sh <event>`. Notes for the Pitwall app which
# session changed state and where; never the prompt, a message or a tool's input (of a
# permission request only the tool's name). Prints nothing and always exits 0, so it can't
# affect Claude: a permission dialog stays Claude's own.
dir=@EVENTS@
input=$(cat)
case "$1" in
  PostToolUse) keys="session_id agent_id tool_name" ;;
  PermissionRequest) keys="session_id agent_id tool_name cwd transcript_path" ;;
  SessionEnd) keys="session_id" ;;
  SessionStart) keys="session_id source cwd transcript_path" ;;
  Notification) keys="session_id notification_type cwd transcript_path" ;;
  UserPromptSubmit|PreToolUse|Stop) keys="session_id cwd transcript_path" ;;
  *) exit 0 ;;
esac
out="event=$1"
for key in $keys; do
  value=$(printf '%s' "$input" | /usr/bin/plutil -extract "$key" raw -o - - 2>/dev/null) || continue
  case "$value" in ''|*'
'*) continue ;; esac
  out="$out
$key=$value"
done
[ -d "$dir" ] || mkdir -p "$dir" 2>/dev/null || exit 0
file="$dir/$$-${RANDOM:-0}"
{ printf '%s\n' "$out" > "$file.tmp" && mv -f "$file.tmp" "$file.event"; } 2>/dev/null
exit 0
"#;

/// Whether Pitwall's hook is in Claude's settings, and whether it is the current one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HookStatus {
    /// At least one Pitwall entry is in the settings.
    pub installed: bool,
    /// Installed, but events are missing, an old entry or script is left, the script is gone, or
    /// the entries run an app that isn't this one (moved, removed).
    pub outdated: bool,
}

/// Modification time and size of the settings file and the script, to read them only after a
/// change.
type Stamp = [Option<(SystemTime, u64)>; 2];

/// How Claude Code runs Pitwall's hook. The app is `None` in a dev build: its binary moves with
/// the build folder, so it can't be the hook.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Runner {
    /// macOS: the script, `~/.pitwall/hooks/claude-hook.sh <Event>`.
    Script,
    /// Linux: `'<app>' --claude-hook <Event>`, run by `sh -c`.
    Shell(Option<PathBuf>),
    /// Windows: the app with `args`, spawned without a shell. A command line would go to Git
    /// Bash, or to PowerShell where Git Bash isn't installed, and no quoting suits both.
    Exec(Option<PathBuf>),
}

impl Runner {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Runner::Script
        } else if cfg!(windows) {
            Runner::Exec(installed_app())
        } else {
            Runner::Shell(installed_app())
        }
    }

    /// The hook that runs `event`; none from a dev build.
    fn hook(&self, script: &Path, event: &str) -> Option<Value> {
        match self {
            Runner::Script => {
                let command = format!("{} {event}", shell_quote(&script.to_string_lossy()));
                Some(json!({ "type": "command", "command": command, "timeout": 10 }))
            }
            Runner::Shell(app) => app.as_deref().map(|app| app_hook(app, event, false)),
            Runner::Exec(app) => app.as_deref().map(|app| app_hook(app, event, true)),
        }
    }
}

/// The installed app's binary, which Claude Code runs on Windows and Linux; none from a dev
/// build (`is_bundled`). An AppImage runs from a new mount each launch: the image file itself is
/// the one to run.
fn installed_app() -> Option<PathBuf> {
    if !crate::is_bundled() {
        return None;
    }

    let image = std::env::var_os("APPIMAGE").filter(|_| cfg!(target_os = "linux"));
    image.map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

/// The hook that runs the app at `app` in hook mode for `event`: the path and `args`, spawned
/// without a shell (`exec`), or one command line for `sh -c`.
fn app_hook(app: &Path, event: &str, exec: bool) -> Value {
    let app = app.to_string_lossy();

    if exec {
        json!({ "type": "command", "command": app, "args": [HOOK_FLAG, event], "timeout": 10 })
    } else {
        json!({ "type": "command", "command": format!("{} {HOOK_FLAG} {event}", shell_quote(&app)), "timeout": 10 })
    }
}

/// A hook that runs Pitwall in hook mode, wherever its binary is (a moved install's entries are
/// still ours, to replace): the flag first in `args`, or in the command line.
fn runs_app(hook: &Value) -> bool {
    let command = hook.get("command").and_then(Value::as_str).unwrap_or_default();
    let first_arg = hook.get("args").and_then(Value::as_array).and_then(|args| args.first()).and_then(Value::as_str);
    let flagged = first_arg == Some(HOOK_FLAG) || command.contains(&format!(" {HOOK_FLAG} "));

    flagged && command.to_lowercase().contains("pitwall")
}

/// The only keys hook mode keeps from Claude Code's input. Every other value (the prompt, a
/// message, a tool's input and output) is skipped as it is read, never held.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HookInput {
    #[serde(deserialize_with = "scalar")]
    session_id: Option<String>,
    #[serde(deserialize_with = "scalar")]
    agent_id: Option<String>,
    #[serde(deserialize_with = "scalar")]
    cwd: Option<String>,
    #[serde(deserialize_with = "scalar")]
    transcript_path: Option<String>,
    #[serde(deserialize_with = "scalar")]
    notification_type: Option<String>,
    #[serde(deserialize_with = "scalar")]
    source: Option<String>,
    /// The tool of a permission request or a finished tool call, by name only (`Bash`, `Edit`);
    /// never its input or output.
    #[serde(deserialize_with = "scalar")]
    tool_name: Option<String>,
}

impl HookInput {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "session_id" => self.session_id.as_deref(),
            "agent_id" => self.agent_id.as_deref(),
            "cwd" => self.cwd.as_deref(),
            "transcript_path" => self.transcript_path.as_deref(),
            "notification_type" => self.notification_type.as_deref(),
            "source" => self.source.as_deref(),
            "tool_name" => self.tool_name.as_deref(),
            _ => None,
        }
    }
}

/// A string, or a number or boolean written out (as `plutil -extract … raw` does); anything
/// else is left out.
fn scalar<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => Some(text),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    })
}

/// The keys an event's file gets, in the order the script writes them (its `case`); none for an
/// event Pitwall doesn't listen to.
fn event_keys(event: &str) -> Option<&'static [&'static str]> {
    let keys: &[&str] = match event {
        "PostToolUse" => &["session_id", "agent_id", "tool_name"],
        "PermissionRequest" => &["session_id", "agent_id", "tool_name", "cwd", "transcript_path"],
        "SessionEnd" => &["session_id"],
        "SessionStart" => &["session_id", "source", "cwd", "transcript_path"],
        "Notification" => &["session_id", "notification_type", "cwd", "transcript_path"],
        "UserPromptSubmit" | "PreToolUse" | "Stop" => &["session_id", "cwd", "transcript_path"],
        _ => return None,
    };
    Some(keys)
}

/// Writes the file for `event` into `dir` from Claude Code's JSON `input`, as the script does:
/// `event=<Event>`, then the event's keys that have a one-line value. Nothing for an event
/// Pitwall doesn't listen to; unreadable input still notes the event.
fn note_event(dir: &Path, event: &str, input: impl Read) -> Option<PathBuf> {
    let keys = event_keys(event)?;
    let input: HookInput = serde_json::from_reader(input.take(INPUT_LIMIT)).unwrap_or_default();
    let mut text = format!("event={event}\n");

    for key in keys {
        if let Some(value) = input.get(key).filter(|value| !value.is_empty() && !value.contains(['\n', '\r'])) {
            text.push_str(&format!("{key}={value}\n"));
        }
    }

    write_event(dir, &text).ok()
}

/// `<pid>-<random>.tmp`, renamed to `.event`: the app never reads half a file.
fn write_event(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;

    let name = format!("{}-{}", std::process::id(), RandomState::new().hash_one(SystemTime::now()) as u32);
    let temp = dir.join(format!("{name}.tmp"));
    let file = dir.join(format!("{name}.event"));
    let written = fs::write(&temp, text).and_then(|()| fs::rename(&temp, &file));

    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.map(|()| file)
}

/// Hook mode, Claude Code's hook on Windows and Linux: when `args` (after the program) are
/// `--claude-hook <Event>`, notes the event in `~/.pitwall/claude-events/` like the script does
/// and returns true, before anything of the app starts. Prints nothing and never fails, so it
/// can't affect Claude.
pub fn hook_mode(mut args: impl Iterator<Item = OsString>) -> bool {
    if args.next().as_deref() != Some(OsStr::new(HOOK_FLAG)) {
        return false;
    }

    let event = args.next().and_then(|event| event.into_string().ok()).unwrap_or_default();

    if let Some(home) = dirs::home_dir() {
        note_event(&home.join(".pitwall").join("claude-events"), &event, std::io::stdin().lock());
    }
    true
}

pub struct ClaudeHook {
    settings_file: PathBuf,
    script_file: PathBuf,
    legacy_script: PathBuf,
    events_dir: PathBuf,
    runner: Runner,
    cached: Mutex<Option<(Stamp, HookStatus)>>,
}

fn stamp(file: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(file).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Single quotes for the shell, when the path needs them.
fn shell_quote(path: &str) -> String {
    if path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)) {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

impl ClaudeHook {
    pub fn new(claude_settings: PathBuf, registry_dir: &Path) -> Self {
        let hooks = registry_dir.join("hooks");
        Self {
            settings_file: claude_settings,
            script_file: hooks.join(SCRIPT_NAME),
            legacy_script: hooks.join(LEGACY_SCRIPT_NAME),
            events_dir: registry_dir.join("claude-events"),
            runner: Runner::current(),
            cached: Mutex::new(None),
        }
    }

    fn read_settings(&self) -> Result<Value, String> {
        match fs::read_to_string(&self.settings_file) {
            Ok(raw) if raw.trim().is_empty() => Ok(json!({})),
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(value @ Value::Object(_)) => Ok(value),
                _ => Err(t!("core.error.invalidJson", file = self.settings_file.display())),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
            Err(error) => Err(error.to_string()),
        }
    }

    fn script(&self) -> String {
        SCRIPT.replace("@EVENTS@", &shell_quote(&self.events_dir.to_string_lossy()))
    }

    /// The entry Pitwall wants under `event`; none from a dev build off macOS.
    fn entry(&self, event: &str, matcher: Option<&str>) -> Option<Value> {
        let hook = self.runner.hook(&self.script_file, event)?;
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        entry.insert("hooks".into(), Value::Array(vec![hook]));
        Some(Value::Object(entry))
    }

    /// Every entry Pitwall wants, by event; none from a dev build off macOS.
    fn wanted(&self) -> Option<Vec<(&'static str, Value)>> {
        EVENTS.iter().map(|(event, matcher)| self.entry(event, *matcher).map(|entry| (*event, entry))).collect()
    }

    /// An entry that runs Pitwall: one of our scripts, current or old, or the app in hook mode.
    fn is_ours(&self, entry: &Value) -> bool {
        let ours = [self.script_file.to_string_lossy(), self.legacy_script.to_string_lossy()];
        entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                let command = hook.get("command").and_then(Value::as_str).unwrap_or_default();
                ours.iter().any(|script| command.contains(script.as_ref())) || runs_app(hook)
            })
        })
    }

    fn status_of(&self, settings: &Value) -> HookStatus {
        let empty = Map::new();
        let hooks = settings.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
        let entries = |event: &str| hooks.get(event).and_then(Value::as_array).cloned().unwrap_or_default();

        let installed = hooks.keys().any(|event| entries(event).iter().any(|entry| self.is_ours(entry)));
        // A dev build can't say what the entries should run; the installed app judges them.
        let Some(wanted) = self.wanted() else {
            return HookStatus { installed, outdated: false };
        };
        let missing = wanted.iter().any(|(event, entry)| !entries(event).contains(entry));
        // Another script or binary (a moved app) is ours too, but not current.
        let stray = hooks.keys().any(|event| {
            let wanted = wanted.iter().find(|(name, _)| name == event).map(|(_, entry)| entry);
            entries(event).iter().any(|entry| self.is_ours(entry) && Some(entry) != wanted)
        });
        let script_current =
            self.runner != Runner::Script || fs::read_to_string(&self.script_file).is_ok_and(|script| script == self.script());

        HookStatus { installed, outdated: installed && (missing || stray || !script_current || self.legacy_script.exists()) }
    }

    /// Read again only when the settings file or the script changed.
    pub fn status(&self) -> HookStatus {
        let now: Stamp = [stamp(&self.settings_file), stamp(&self.script_file)];
        let mut cached = self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some((at, status)) = cached.as_ref() {
            if *at == now {
                return *status;
            }
        }

        let status = self.read_settings().map(|settings| self.status_of(&settings)).unwrap_or_default();
        *cached = Some((now, status));
        status
    }

    /// Adds every entry Pitwall wants, replacing ours that differ (an older install, a moved
    /// app), and writes the current script on macOS.
    pub fn install(&self) -> Result<(), String> {
        // A dev build's binary moves with the build folder: only the installed app adds the hook.
        let wanted = self.wanted().ok_or_else(|| t!("core.error.hookDev"))?;
        let mut settings = self.read_settings()?;

        // The hook script first: settings must never point at a missing script.
        if self.runner == Runner::Script {
            if let Some(dir) = self.script_file.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            write_atomic(&self.script_file, &self.script()).map_err(|e| e.to_string())?;
            make_executable(&self.script_file)?;
        }
        let _ = fs::create_dir_all(&self.events_dir);

        let root = settings.as_object_mut().ok_or("settings is not an object")?;
        let hooks = root.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
        let hooks = hooks.as_object_mut().ok_or("\"hooks\" in Claude settings is not an object")?;
        let mut changed = false;

        // Ours that aren't wanted (old script, moved app, events no longer used, a changed
        // entry) go.
        let mut emptied = Vec::new();
        for (event, entries) in hooks.iter_mut() {
            let wanted = wanted.iter().find(|(name, _)| name == event).map(|(_, entry)| entry);
            if let Some(entries) = entries.as_array_mut() {
                let before = entries.len();
                entries.retain(|entry| !self.is_ours(entry) || Some(entry) == wanted);

                if entries.len() != before {
                    changed = true;
                    if entries.is_empty() && wanted.is_none() {
                        emptied.push(event.clone());
                    }
                }
            }
        }
        for event in emptied {
            hooks.remove(&event);
        }

        for (event, wanted) in wanted {
            let entries = hooks.entry(event).or_insert_with(|| Value::Array(Vec::new()));
            let entries = entries.as_array_mut().ok_or_else(|| format!("\"hooks.{event}\" in Claude settings is not a list"))?;

            if !entries.contains(&wanted) {
                entries.push(wanted);
                changed = true;
            }
        }

        if changed {
            self.backup_once()?;
            self.write_settings(&settings)?;
        }

        let _ = fs::remove_file(&self.legacy_script);
        Ok(())
    }

    /// Removes every Pitwall entry from every event, and nothing else.
    pub fn uninstall(&self) -> Result<(), String> {
        let mut settings = self.read_settings()?;
        let mut changed = false;

        if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
            let mut emptied = Vec::new();

            for (event, entries) in hooks.iter_mut() {
                if let Some(entries) = entries.as_array_mut() {
                    let before = entries.len();
                    entries.retain(|entry| !self.is_ours(entry));

                    if entries.len() != before {
                        changed = true;
                        if entries.is_empty() {
                            emptied.push(event.clone());
                        }
                    }
                }
            }

            for event in emptied {
                hooks.remove(&event);
            }

            if hooks.is_empty() {
                settings.as_object_mut().map(|root| root.remove("hooks"));
            }
        }

        if changed {
            self.write_settings(&settings)?;
        }

        let _ = fs::remove_file(&self.script_file);
        let _ = fs::remove_file(&self.legacy_script);
        Ok(())
    }

    /// The file as it was before Pitwall first touched it.
    fn backup_once(&self) -> Result<(), String> {
        let backup = self.settings_file.with_extension("json.pitwall-backup");

        if self.settings_file.exists() && !backup.exists() {
            fs::copy(&self.settings_file, backup).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    fn write_settings(&self, settings: &Value) -> Result<(), String> {
        if let Some(dir) = self.settings_file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }

        let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        write_atomic(&self.settings_file, &format!("{json}\n")).map_err(|e| e.to_string())
    }
}

#[cfg(unix)]
fn make_executable(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn make_executable(_file: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The macOS script, on every system.
    fn setup(content: Option<&str>) -> (tempfile::TempDir, ClaudeHook) {
        setup_with(content, |_| Runner::Script)
    }

    fn setup_with(content: Option<&str>, runner: impl Fn(&Path) -> Runner) -> (tempfile::TempDir, ClaudeHook) {
        let tmp = tempfile::tempdir().unwrap();
        let settings = tmp.path().join("claude").join("settings.json");

        if let Some(content) = content {
            fs::create_dir_all(settings.parent().unwrap()).unwrap();
            fs::write(&settings, content).unwrap();
        }

        let mut hook = ClaudeHook::new(settings, &tmp.path().join("pitwall"));
        hook.runner = runner(tmp.path());
        (tmp, hook)
    }

    /// Where the tests' app is installed: a folder with a space.
    fn app(tmp: &Path) -> PathBuf {
        tmp.join("Pitwall App").join("pitwall")
    }

    /// One setup per way the hook runs: the script (macOS), the app by `sh -c` (Linux) and the
    /// app without a shell (Windows).
    fn every_runner(content: Option<&str>) -> Vec<(tempfile::TempDir, ClaudeHook)> {
        vec![
            setup(content),
            setup_with(content, |tmp| Runner::Shell(Some(app(tmp)))),
            setup_with(content, |tmp| Runner::Exec(Some(app(tmp)))),
        ]
    }

    fn read(hook: &ClaudeHook) -> Value {
        serde_json::from_str(&fs::read_to_string(&hook.settings_file).unwrap()).unwrap()
    }

    /// Another tool's hooks on the same events as ours.
    const EXISTING: &str = r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(ls:*)"] },
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }],
    "Notification": [{ "matcher": "idle_prompt", "hooks": [{ "type": "command", "command": "afplay ping.aiff" }] }],
    "PostToolUse": [{ "matcher": "", "hooks": [{ "type": "command", "command": "node ~/.other/claude-hook.js" }] }],
    "PreCompact": [{ "hooks": [{ "type": "command", "command": "backup.sh" }] }]
  },
  "statusLine": { "type": "command", "command": "status.sh" }
}"#;

    fn foreign(settings: &Value) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for (event, entries) in settings["hooks"].as_object().unwrap() {
            for entry in entries.as_array().unwrap() {
                let command = entry["hooks"][0]["command"].as_str().unwrap();
                if !command.contains("pitwall") {
                    found.push((event.clone(), command.to_string()));
                }
            }
        }
        found.sort();
        found
    }

    fn ours(settings: &Value, event: &str) -> usize {
        settings["hooks"][event].as_array().map_or(0, |entries| {
            entries.iter().filter(|entry| entry["hooks"][0]["command"].as_str().unwrap().contains("pitwall")).count()
        })
    }

    #[test]
    fn install_keeps_everything_else_and_adds_each_entry_once() {
        for (_tmp, hook) in every_runner(Some(EXISTING)) {
            let before = foreign(&serde_json::from_str(EXISTING).unwrap());

            hook.install().unwrap();
            hook.install().unwrap();

            let settings = read(&hook);
            let keys: Vec<&String> = settings.as_object().unwrap().keys().collect();

            assert_eq!(keys, ["model", "permissions", "hooks", "statusLine"], "key order kept");
            assert_eq!(settings["permissions"]["allow"][0], "Bash(ls:*)");
            assert_eq!(foreign(&settings), before, "other tools' hooks stay, on the same events too");
            assert_eq!(settings["hooks"]["Notification"][0]["hooks"][0]["command"], "afplay ping.aiff", "and in their place");
            for (event, _) in EVENTS {
                assert_eq!(ours(&settings, event), 1, "exactly one of ours on {event}");
            }
            assert_eq!(hook.status(), HookStatus { installed: true, outdated: false });
            assert_eq!(hook.script_file.exists(), hook.runner == Runner::Script, "the script only where it runs");
        }
    }

    #[test]
    fn a_moved_app_is_outdated_and_its_entries_replaced() {
        for (tmp, mut hook) in every_runner(Some(EXISTING)).into_iter().skip(1) {
            hook.install().unwrap();

            // Installed again elsewhere: the entries still run the old binary.
            let moved = tmp.path().join("Moved").join("pitwall");
            hook.runner = match hook.runner {
                Runner::Shell(_) => Runner::Shell(Some(moved.clone())),
                _ => Runner::Exec(Some(moved.clone())),
            };
            assert_eq!(hook.status_of(&read(&hook)), HookStatus { installed: true, outdated: true });

            hook.install().unwrap();

            let settings = read(&hook);
            assert_eq!(foreign(&settings), foreign(&serde_json::from_str(EXISTING).unwrap()));
            for (event, _) in EVENTS {
                assert_eq!(ours(&settings, event), 1, "the old entry on {event} is replaced, not kept beside the new one");
            }
            assert!(!settings.to_string().contains("Pitwall App"));
            assert_eq!(hook.status_of(&settings), HookStatus { installed: true, outdated: false });

            // A dev build's binary moves with the build folder: it never becomes the hook.
            hook.runner = Runner::Exec(None);
            let before = fs::read_to_string(&hook.settings_file).unwrap();
            assert!(hook.install().is_err());
            assert_eq!(fs::read_to_string(&hook.settings_file).unwrap(), before);
            assert_eq!(hook.status_of(&settings), HookStatus { installed: true, outdated: false });

            // It still removes the hook.
            hook.uninstall().unwrap();
            assert_eq!(read(&hook)["hooks"], serde_json::from_str::<Value>(EXISTING).unwrap()["hooks"]);
        }
    }

    #[test]
    fn an_old_install_is_outdated_and_upgraded_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = tmp.path().join("pitwall");
        let legacy = registry.join("hooks").join(LEGACY_SCRIPT_NAME);
        let settings_file = tmp.path().join("settings.json");
        let mut old: Value = serde_json::from_str(EXISTING).unwrap();
        old["hooks"]["Notification"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "matcher": "", "hooks": [{ "type": "command", "command": legacy.to_string_lossy(), "async": true }] }));
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "#!/bin/sh\n").unwrap();
        fs::write(&settings_file, serde_json::to_string_pretty(&old).unwrap()).unwrap();
        let mut hook = ClaudeHook::new(settings_file, &registry);
        hook.runner = Runner::Script;

        assert_eq!(hook.status(), HookStatus { installed: true, outdated: true });

        hook.install().unwrap();

        let settings = read(&hook);
        assert_eq!(foreign(&settings), foreign(&old));
        assert_eq!(ours(&settings, "Notification"), 1, "the old entry is replaced, not kept beside the new one");
        assert!(!settings.to_string().contains(LEGACY_SCRIPT_NAME));
        assert!(!legacy.exists());
        assert_eq!(hook.status(), HookStatus { installed: true, outdated: false });

        // An install from before an event was added (`PermissionRequest`) is outdated too.
        let mut settings = read(&hook);
        settings["hooks"].as_object_mut().unwrap().remove("PermissionRequest");
        hook.write_settings(&settings).unwrap();
        assert!(hook.status().outdated);
        hook.install().unwrap();
        assert_eq!(hook.status(), HookStatus { installed: true, outdated: false });

        // A script left from an older version is outdated too, though every entry is there.
        fs::write(&hook.script_file, "#!/bin/sh\ncat > \"$HOME/.pitwall/claude-events/x.json\"\n").unwrap();
        assert!(hook.status().outdated);
    }

    #[test]
    fn the_original_is_backed_up_once() {
        for (_tmp, hook) in every_runner(Some(EXISTING)) {
            let backup = hook.settings_file.with_extension("json.pitwall-backup");

            hook.install().unwrap();
            hook.uninstall().unwrap();
            hook.install().unwrap();

            assert_eq!(fs::read_to_string(backup).unwrap(), EXISTING);
        }
    }

    #[test]
    fn uninstall_removes_only_our_entries() {
        for (_tmp, hook) in every_runner(Some(EXISTING)) {
            hook.install().unwrap();
            hook.uninstall().unwrap();

            let settings = read(&hook);
            let keys: Vec<&String> = settings["hooks"].as_object().unwrap().keys().collect();

            assert_eq!(settings["hooks"], serde_json::from_str::<Value>(EXISTING).unwrap()["hooks"], "exactly what was there");
            assert_eq!(keys, ["Stop", "Notification", "PostToolUse", "PreCompact"]);
            assert_eq!(hook.status(), HookStatus::default());
            assert!(!hook.script_file.exists());
        }
    }

    /// A path with a space and a quote, as Claude Code runs it on each system.
    #[test]
    fn the_app_path_survives_each_shell() {
        // Windows: no shell, so the path and each argument go as they are.
        let windows = Path::new(r"C:\Program Files\Pit'wall\Pitwall.exe");
        let hook = app_hook(windows, "Stop", true);
        assert_eq!(hook["command"], r"C:\Program Files\Pit'wall\Pitwall.exe");
        assert_eq!(hook["args"], json!(["--claude-hook", "Stop"]));
        assert!(runs_app(&hook));

        // Linux: `sh -c` sees the path as one word.
        let hook = app_hook(Path::new("/opt/My Apps/Pit'wall/pitwall"), "Stop", false);
        assert_eq!(hook["command"], r"'/opt/My Apps/Pit'\''wall/pitwall' --claude-hook Stop");
        assert!(hook.get("args").is_none());
        assert!(runs_app(&hook));

        #[cfg(unix)]
        {
            let tmp = tempfile::tempdir().unwrap();
            let app = tmp.path().join("My Apps").join("Pit'wall").join("pitwall");
            let hook = app_hook(&app, "Stop", false);
            fs::create_dir_all(app.parent().unwrap()).unwrap();
            fs::write(&app, "#!/bin/sh\nprintf '%s|' \"$@\"\n").unwrap();
            make_executable(&app).unwrap();
            let output = std::process::Command::new("sh").arg("-c").arg(hook["command"].as_str().unwrap()).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&output.stdout), "--claude-hook|Stop|");
        }
    }

    #[test]
    fn a_broken_settings_file_is_never_touched() {
        let broken = "{ \"model\": \"opus\", ";
        let (_tmp, hook) = setup(Some(broken));

        assert!(hook.install().is_err());
        assert!(hook.uninstall().is_err());
        assert_eq!(fs::read_to_string(&hook.settings_file).unwrap(), broken);
        assert!(!hook.settings_file.with_extension("json.pitwall-backup").exists());
    }

    #[test]
    fn a_missing_settings_file_is_created_and_cleaned_up() {
        let (_tmp, hook) = setup(None);

        hook.install().unwrap();
        assert!(hook.status().installed);

        hook.uninstall().unwrap();
        assert_eq!(read(&hook), json!({}));
    }

    /// Runs the installed script for `event` with `input` on stdin; returns what it wrote.
    #[cfg(target_os = "macos")]
    fn run_script(hook: &ClaudeHook, event: &str, input: &Value) -> String {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let before: Vec<PathBuf> = fs::read_dir(&hook.events_dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        let mut child = Command::new(&hook.script_file)
            .arg(event)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();

        assert!(output.status.success());
        assert!(output.stdout.is_empty() && output.stderr.is_empty(), "the hook prints nothing");

        let written: Vec<PathBuf> =
            fs::read_dir(&hook.events_dir).unwrap().flatten().map(|e| e.path()).filter(|p| !before.contains(p)).collect();
        assert_eq!(written.len(), 1, "one file per event, nothing else left behind");
        assert_eq!(written[0].extension().unwrap(), "event");
        let text = fs::read_to_string(&written[0]).unwrap();
        fs::remove_file(&written[0]).unwrap();
        text
    }

    /// What the hook writes for `event` from `input`: hook mode's file (Windows, Linux), which on
    /// macOS must be the script's to the byte.
    fn noted(hook: &ClaudeHook, event: &str, input: &Value) -> String {
        #[cfg(target_os = "macos")]
        let script = run_script(hook, event, input);

        let file = note_event(&hook.events_dir, event, input.to_string().as_bytes()).unwrap();
        assert_eq!(file.extension().unwrap(), "event");
        let text = fs::read_to_string(&file).unwrap();
        fs::remove_file(&file).unwrap();
        assert_eq!(fs::read_dir(&hook.events_dir).unwrap().count(), 0, "nothing else left behind");

        #[cfg(target_os = "macos")]
        assert_eq!(text, script, "hook mode writes what the script writes");
        text
    }

    #[test]
    fn the_hook_never_writes_the_prompt_or_a_message() {
        let (_tmp, hook) = setup(None);
        hook.install().unwrap();
        let secret = "SECRET \"text\" with = signs";
        let base = |event: &str| {
            json!({
                "session_id": "9bb85e86", "transcript_path": "/Users/me/.claude/projects/-p/9bb85e86.jsonl",
                "cwd": "/Users/me/my project", "permission_mode": "default", "hook_event_name": event,
            })
        };

        let mut prompt = base("UserPromptSubmit");
        prompt["prompt"] = secret.into();
        let written = noted(&hook, "UserPromptSubmit", &prompt);
        assert_eq!(
            written,
            "event=UserPromptSubmit\nsession_id=9bb85e86\ncwd=/Users/me/my project\ntranscript_path=/Users/me/.claude/projects/-p/9bb85e86.jsonl\n"
        );

        let mut notification = base("Notification");
        notification["notification_type"] = "permission_prompt".into();
        notification["message"] = secret.into();
        notification["title"] = secret.into();
        let written = noted(&hook, "Notification", &notification);
        assert!(written.contains("notification_type=permission_prompt\n"));
        assert!(!written.contains("SECRET"));

        let mut tool = base("PostToolUse");
        tool["agent_id"] = "a1".into();
        tool["tool_name"] = "Bash".into();
        tool["tool_input"] = json!({ "command": secret, "session_id": "nested" });
        tool["tool_response"] = json!({ "stdout": secret, "stderr": null });
        let written = noted(&hook, "PostToolUse", &tool);
        assert_eq!(written, "event=PostToolUse\nsession_id=9bb85e86\nagent_id=a1\ntool_name=Bash\n");

        let mut stop = base("Stop");
        stop["last_assistant_message"] = secret.into();
        assert!(!noted(&hook, "Stop", &stop).contains("SECRET"));

        // A permission dialog: the tool's name only, never what it would run or the
        // suggestions built from it.
        let mut permission = base("PermissionRequest");
        permission["agent_id"] = "a1".into();
        permission["tool_name"] = "Bash".into();
        permission["tool_input"] = json!({ "command": secret, "description": secret, "tool_name": "nested" });
        permission["permission_suggestions"] = json!([{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": secret }] }]);
        assert_eq!(
            noted(&hook, "PermissionRequest", &permission),
            "event=PermissionRequest\nsession_id=9bb85e86\nagent_id=a1\ntool_name=Bash\ncwd=/Users/me/my project\ntranscript_path=/Users/me/.claude/projects/-p/9bb85e86.jsonl\n"
        );

        // A tool's output runs to megabytes (an image read); the event still gets its keys.
        tool["tool_response"] = json!({ "file": { "base64": "A".repeat(3 << 20) } });
        assert_eq!(noted(&hook, "PostToolUse", &tool), "event=PostToolUse\nsession_id=9bb85e86\nagent_id=a1\ntool_name=Bash\n");

        // Events Pitwall doesn't listen to leave nothing.
        assert!(note_event(&hook.events_dir, "PreCompact", prompt.to_string().as_bytes()).is_none());
        assert_eq!(fs::read_dir(&hook.events_dir).unwrap().count(), 0);
    }

    #[test]
    fn a_legacy_permission_event_waits_until_the_session_moves_on() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = tmp.path().join("pitwall");
        let events = registry.join("claude-events");
        let transcript = tmp.path().join("session.jsonl");
        let project = "/Users/me/projects/paddock";

        fs::create_dir_all(&events).unwrap();
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        fs::write(&transcript, "{}\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Written by the first hook script: Claude Code's whole input.
        let event = events.join("1-1.json");
        fs::write(
            &event,
            json!({
                "session_id": "s",
                "transcript_path": transcript,
                "cwd": format!("{project}/app"),
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt",
                "message": "Claude needs your permission to use Bash"
            })
            .to_string(),
        )
        .unwrap();

        let mut watch = crate::claude::ClaudeWatch::new(tmp.path().join("no-claude"), &registry);
        let paths = vec![project.to_string()];

        let waiting = watch.scan(&paths).waiting;
        assert_eq!(waiting[project].kind, crate::claude::TurnKind::Permission);
        assert!(!event.exists(), "its message text doesn't stay on disk");

        // The user answers; Claude writes to the session log again.
        std::thread::sleep(std::time::Duration::from_millis(1_600));
        fs::write(&transcript, "{}\n{}\n").unwrap();

        assert!(watch.scan(&paths).waiting.is_empty());
    }
}
