//! The Claude Code hook that reports a session's turns the moment they change: a prompt sent,
//! the turn over, a permission prompt or a question waiting, the session started or gone.
//! Installing it edits `~/.claude/settings.json`, so the rules are strict: back the file up
//! once, never touch a file that isn't valid JSON, keep every other key and hook in place, add
//! our entries once, and remove only our entries.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde_json::{json, Map, Value};

use crate::i18n::t;
use crate::registry::write_atomic;

const SCRIPT_NAME: &str = "claude-hook.sh";
/// The first script, which saved the whole Notification input (message text included). Its
/// entries are replaced on install and removed on uninstall.
const LEGACY_SCRIPT_NAME: &str = "claude-notification.sh";

/// The events Pitwall listens to, with their matcher (`None`: the event takes none). A tool
/// call that finishes after a prompt answers it; `PreToolUse` only for the tools that ask.
const EVENTS: [(&str, Option<&str>); 7] = [
    ("SessionStart", Some("")),
    ("UserPromptSubmit", None),
    ("PreToolUse", Some("AskUserQuestion|ExitPlanMode")),
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
# session changed state and where; never the prompt, a message or a tool's input. Prints
# nothing and always exits 0, so it can't affect Claude.
dir=@EVENTS@
input=$(cat)
case "$1" in
  PostToolUse) keys="session_id agent_id" ;;
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
    /// Installed, but events are missing, an old entry or script is left, or the script is gone.
    pub outdated: bool,
}

/// Modification time and size of the settings file and the script, to read them only after a
/// change.
type Stamp = [Option<(SystemTime, u64)>; 2];

pub struct ClaudeHook {
    settings_file: PathBuf,
    script_file: PathBuf,
    legacy_script: PathBuf,
    events_dir: PathBuf,
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

    /// The entry Pitwall wants under `event`.
    fn entry(&self, event: &str, matcher: Option<&str>) -> Value {
        let command = format!("{} {event}", shell_quote(&self.script_file.to_string_lossy()));
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        entry.insert("hooks".into(), json!([{ "type": "command", "command": command, "timeout": 10 }]));
        Value::Object(entry)
    }

    /// An entry that runs one of our scripts, current or old.
    fn is_ours(&self, entry: &Value) -> bool {
        let ours = [self.script_file.to_string_lossy(), self.legacy_script.to_string_lossy()];
        entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command").and_then(Value::as_str).is_some_and(|command| ours.iter().any(|script| command.contains(script.as_ref())))
            })
        })
    }

    fn status_of(&self, settings: &Value) -> HookStatus {
        let empty = Map::new();
        let hooks = settings.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
        let entries = |event: &str| hooks.get(event).and_then(Value::as_array).cloned().unwrap_or_default();

        let installed = hooks.keys().any(|event| entries(event).iter().any(|entry| self.is_ours(entry)));
        let missing = EVENTS.iter().any(|(event, matcher)| !entries(event).contains(&self.entry(event, *matcher)));
        let stray = hooks.keys().any(|event| {
            let wanted = EVENTS.iter().find(|(name, _)| name == event).map(|(name, matcher)| self.entry(name, *matcher));
            entries(event).iter().any(|entry| self.is_ours(entry) && Some(entry) != wanted.as_ref())
        });
        let script_current = fs::read_to_string(&self.script_file).is_ok_and(|script| script == self.script());

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

    /// Adds every entry Pitwall wants, replacing ours that differ (an older install), and
    /// writes the current script.
    pub fn install(&self) -> Result<(), String> {
        let mut settings = self.read_settings()?;

        // The hook script first: settings must never point at a missing script.
        if let Some(dir) = self.script_file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        write_atomic(&self.script_file, &self.script()).map_err(|e| e.to_string())?;
        make_executable(&self.script_file)?;
        let _ = fs::create_dir_all(&self.events_dir);

        let root = settings.as_object_mut().ok_or("settings is not an object")?;
        let hooks = root.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
        let hooks = hooks.as_object_mut().ok_or("\"hooks\" in Claude settings is not an object")?;
        let mut changed = false;

        // Ours that aren't wanted (old script, events no longer used, a changed entry) go.
        let mut emptied = Vec::new();
        for (event, entries) in hooks.iter_mut() {
            let wanted = EVENTS.iter().find(|(name, _)| name == event).map(|(name, matcher)| self.entry(name, *matcher));
            if let Some(entries) = entries.as_array_mut() {
                let before = entries.len();
                entries.retain(|entry| !self.is_ours(entry) || Some(entry) == wanted.as_ref());

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

        for (event, matcher) in EVENTS {
            let wanted = self.entry(event, matcher);
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

    fn setup(content: Option<&str>) -> (tempfile::TempDir, ClaudeHook) {
        let tmp = tempfile::tempdir().unwrap();
        let settings = tmp.path().join("claude").join("settings.json");

        if let Some(content) = content {
            fs::create_dir_all(settings.parent().unwrap()).unwrap();
            fs::write(&settings, content).unwrap();
        }

        let hook = ClaudeHook::new(settings, &tmp.path().join("pitwall"));
        (tmp, hook)
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
        let (_tmp, hook) = setup(Some(EXISTING));
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
        assert!(hook.script_file.exists());
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
        let hook = ClaudeHook::new(settings_file, &registry);

        assert_eq!(hook.status(), HookStatus { installed: true, outdated: true });

        hook.install().unwrap();

        let settings = read(&hook);
        assert_eq!(foreign(&settings), foreign(&old));
        assert_eq!(ours(&settings, "Notification"), 1, "the old entry is replaced, not kept beside the new one");
        assert!(!settings.to_string().contains(LEGACY_SCRIPT_NAME));
        assert!(!legacy.exists());
        assert_eq!(hook.status(), HookStatus { installed: true, outdated: false });

        // A script left from an older version is outdated too, though every entry is there.
        fs::write(&hook.script_file, "#!/bin/sh\ncat > \"$HOME/.pitwall/claude-events/x.json\"\n").unwrap();
        assert!(hook.status().outdated);
    }

    #[test]
    fn the_original_is_backed_up_once() {
        let (_tmp, hook) = setup(Some(EXISTING));
        let backup = hook.settings_file.with_extension("json.pitwall-backup");

        hook.install().unwrap();
        hook.uninstall().unwrap();
        hook.install().unwrap();

        assert_eq!(fs::read_to_string(backup).unwrap(), EXISTING);
    }

    #[test]
    fn uninstall_removes_only_our_entries() {
        let (_tmp, hook) = setup(Some(EXISTING));

        hook.install().unwrap();
        hook.uninstall().unwrap();

        let settings = read(&hook);
        let keys: Vec<&String> = settings["hooks"].as_object().unwrap().keys().collect();

        assert_eq!(settings["hooks"], serde_json::from_str::<Value>(EXISTING).unwrap()["hooks"], "exactly what was there");
        assert_eq!(keys, ["Stop", "Notification", "PostToolUse", "PreCompact"]);
        assert_eq!(hook.status(), HookStatus::default());
        assert!(!hook.script_file.exists());
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
        fs::read_to_string(&written[0]).unwrap()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_script_never_writes_the_prompt_or_a_message() {
        let (_tmp, hook) = setup(None);
        hook.install().unwrap();
        let secret = "SECRET \"text\"\nwith = signs";
        let base = |event: &str| {
            json!({
                "session_id": "9bb85e86", "transcript_path": "/Users/me/.claude/projects/-p/9bb85e86.jsonl",
                "cwd": "/Users/me/my project", "permission_mode": "default", "hook_event_name": event,
            })
        };

        let mut prompt = base("UserPromptSubmit");
        prompt["prompt"] = secret.into();
        let written = run_script(&hook, "UserPromptSubmit", &prompt);
        assert_eq!(
            written,
            "event=UserPromptSubmit\nsession_id=9bb85e86\ncwd=/Users/me/my project\ntranscript_path=/Users/me/.claude/projects/-p/9bb85e86.jsonl\n"
        );

        let mut notification = base("Notification");
        notification["notification_type"] = "permission_prompt".into();
        notification["message"] = secret.into();
        notification["title"] = secret.into();
        let written = run_script(&hook, "Notification", &notification);
        assert!(written.contains("notification_type=permission_prompt\n"));
        assert!(!written.contains("SECRET"));

        let mut tool = base("PostToolUse");
        tool["agent_id"] = "a1".into();
        tool["tool_input"] = json!({ "command": secret, "session_id": "nested" });
        tool["tool_response"] = json!({ "stdout": secret, "stderr": null });
        let written = run_script(&hook, "PostToolUse", &tool);
        assert_eq!(written, "event=PostToolUse\nsession_id=9bb85e86\nagent_id=a1\n");

        let mut stop = base("Stop");
        stop["last_assistant_message"] = secret.into();
        assert!(!run_script(&hook, "Stop", &stop).contains("SECRET"));
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
