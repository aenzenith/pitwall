//! The Claude Code Notification hook that reports permission prompts and questions, which the
//! session logs don't record. Installing it edits `~/.claude/settings.json`, so the rules are
//! strict: back the file up once, never touch a file that isn't valid JSON, keep every other key
//! and hook in place, add our entry once, and remove only our entry.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::i18n::t;
use crate::registry::write_atomic;

/// Marker in the command path that identifies our hook entry.
const SCRIPT_NAME: &str = "claude-notification.sh";

/// Saves the hook input for the app and prints nothing. `async` in settings keeps it off
/// Claude's path; its output is discarded for Notification events anyway.
const SCRIPT: &str = r#"#!/bin/sh
# Pitwall: Claude Code Notification hook. Saves the event for the Pitwall app.
dir="$HOME/.pitwall/claude-events"
mkdir -p "$dir" || exit 0
file="$dir/$(date +%s)-$$"
cat > "$file.tmp" && mv "$file.tmp" "$file.json"
exit 0
"#;

pub struct ClaudeHook {
    settings_file: PathBuf,
    script_file: PathBuf,
}

impl ClaudeHook {
    pub fn new(claude_settings: PathBuf, registry_dir: &Path) -> Self {
        Self { settings_file: claude_settings, script_file: registry_dir.join("hooks").join(SCRIPT_NAME) }
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

    fn is_ours(entry: &Value) -> bool {
        entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| hooks.iter().any(|hook| hook.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(SCRIPT_NAME))))
    }

    pub fn installed(&self) -> bool {
        self.read_settings()
            .ok()
            .and_then(|settings| settings.pointer("/hooks/Notification").and_then(Value::as_array).cloned())
            .is_some_and(|entries| entries.iter().any(Self::is_ours))
            && self.script_file.exists()
    }

    pub fn install(&self) -> Result<(), String> {
        let mut settings = self.read_settings()?;

        // The hook script first: settings must never point at a missing script.
        if let Some(dir) = self.script_file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        write_atomic(&self.script_file, SCRIPT).map_err(|e| e.to_string())?;
        make_executable(&self.script_file)?;

        let root = settings.as_object_mut().ok_or("settings is not an object")?;
        let hooks = root.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
        let hooks = hooks.as_object_mut().ok_or("\"hooks\" in Claude settings is not an object")?;
        let entries = hooks.entry("Notification").or_insert_with(|| Value::Array(Vec::new()));
        let entries = entries.as_array_mut().ok_or("\"hooks.Notification\" in Claude settings is not a list")?;

        if !entries.iter().any(Self::is_ours) {
            entries.push(json!({
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": self.script_file.to_string_lossy(),
                    "async": true
                }]
            }));
        }

        self.backup_once()?;
        self.write_settings(&settings)
    }

    pub fn uninstall(&self) -> Result<(), String> {
        let mut settings = self.read_settings()?;
        let mut changed = false;

        if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
            if let Some(entries) = hooks.get_mut("Notification").and_then(Value::as_array_mut) {
                let before = entries.len();
                entries.retain(|entry| !Self::is_ours(entry));
                changed = entries.len() != before;

                if entries.is_empty() {
                    hooks.remove("Notification");
                }
            }

            if hooks.is_empty() {
                settings.as_object_mut().map(|root| root.remove("hooks"));
            }
        }

        if changed {
            self.write_settings(&settings)?;
        }

        let _ = fs::remove_file(&self.script_file);
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

    const EXISTING: &str = r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(ls:*)"] },
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }],
    "Notification": [{ "matcher": "idle_prompt", "hooks": [{ "type": "command", "command": "afplay ping.aiff" }] }]
  },
  "statusLine": { "type": "command", "command": "status.sh" }
}"#;

    #[test]
    fn install_keeps_everything_else_and_adds_one_entry() {
        let (_tmp, hook) = setup(Some(EXISTING));

        hook.install().unwrap();
        hook.install().unwrap();

        let settings = read(&hook);
        let keys: Vec<&String> = settings.as_object().unwrap().keys().collect();

        assert_eq!(keys, ["model", "permissions", "hooks", "statusLine"], "key order kept");
        assert_eq!(settings["permissions"]["allow"][0], "Bash(ls:*)");
        assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");

        let notification = settings["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(notification.len(), 2, "the user's hook plus exactly one of ours");
        assert_eq!(notification[0]["hooks"][0]["command"], "afplay ping.aiff");
        assert_eq!(notification[1]["hooks"][0]["async"], true);
        assert!(hook.installed());
        assert!(hook.script_file.exists());
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
    fn uninstall_removes_only_our_entry() {
        let (_tmp, hook) = setup(Some(EXISTING));

        hook.install().unwrap();
        hook.uninstall().unwrap();

        let settings = read(&hook);
        let notification = settings["hooks"]["Notification"].as_array().unwrap();

        assert_eq!(notification.len(), 1);
        assert_eq!(notification[0]["hooks"][0]["command"], "afplay ping.aiff");
        assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert!(!hook.installed());
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
        assert!(hook.installed());

        hook.uninstall().unwrap();
        assert_eq!(read(&hook), json!({}));
    }

    #[test]
    fn a_permission_prompt_waits_until_the_session_moves_on() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = tmp.path().join("pitwall");
        let events = registry.join("claude-events");
        let transcript = tmp.path().join("session.jsonl");
        let project = "/Users/me/projects/paddock";

        fs::create_dir_all(&events).unwrap();
        fs::write(registry.join("claude-seen.json"), r#"{"since":0,"paths":{}}"#).unwrap();
        fs::write(&transcript, "{}\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

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

        // The user answers; Claude writes to the session log again.
        std::thread::sleep(std::time::Duration::from_millis(1_600));
        fs::write(&transcript, "{}\n{}\n").unwrap();

        assert!(watch.scan(&paths).waiting.is_empty());
        assert!(!event.exists(), "answered events are cleaned up");
    }
}
