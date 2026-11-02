//! Pitwall's hook in Codex, as `hooks.rs` is in Claude Code: its entries in `<home>/hooks.json`
//! (never `config.toml`, never a trust of its own: Codex runs a new hook only once the user
//! trusted it in `/hooks`, and Pitwall never writes `hooks.state.*`), the script that runs them on
//! macOS and the app's own binary as `--codex-hook <Event>` elsewhere. What it writes of an event
//! is `event`, `session_id`, `turn_id`, `agent_id`, `cwd`, `source`, `tool_name` and `ppid`,
//! nothing else: never the prompt, a tool's input or output or Codex's message, which come on its
//! stdin and are skipped unread. It prints nothing (Codex would take a `SessionStart` hook's
//! output for context) and always ends well, so it can't affect Codex.
//!
//! The rules for `hooks.json` are `settings.json`'s: back the file up once, never touch a file
//! that isn't valid JSON, keep every other key and hook in place, add our entries once, remove
//! only ours. Codex untrusts a hook whose entry changed, so the entries and the script stay the
//! same, byte for byte, from one version of Pitwall to the next: a moved app is the only change
//! that asks the user for trust again.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::agent::HookState;
use crate::hooks::{installed_app, make_executable, scalar, shell_quote, stamp, write_event};
use crate::i18n::t;
use crate::registry::write_atomic;

const SCRIPT_NAME: &str = "codex-hook.sh";
/// The flag that runs the app as Codex's hook: `<app> --codex-hook <Event>`.
const HOOK_FLAG: &str = "--codex-hook";
/// How much of Codex's input hook mode reads. Values it doesn't keep are skipped as they stream
/// past, never held: `PostToolUse` carries the tool's output.
const INPUT_LIMIT: u64 = 64 << 20;
/// What Codex gives each run of the hook (seconds); hook mode itself gives up reading sooner.
const TIMEOUT_S: u64 = 5;
const READ_WITHIN: Duration = Duration::from_millis(4_000);

/// The events Pitwall listens to, with their matcher (a regular expression; `None`: the entry has
/// none and takes every one). A tool's matcher matches its name.
const EVENTS: [(&str, Option<&str>); 9] = [
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PermissionRequest", Some(".*")),
    ("PostToolUse", Some(".*")),
    ("Stop", None),
    ("Interrupt", None),
    ("SubagentStart", None),
    ("SubagentStop", None),
    ("SessionEnd", None),
];

/// The keys an event's file gets, in this order, each when Codex gives it a one-line value; then
/// `ppid`, the hook's parent (Codex's TUI under `--no-daemon`). The same list for every event, so
/// the script never has to change for one.
const KEYS: [&str; 6] = ["session_id", "turn_id", "agent_id", "cwd", "source", "tool_name"];

/// Writes the few fields Pitwall needs, one `key=value` per line, to a file in the events folder
/// and prints nothing. `plutil` reads the JSON on stdin; the input stays in memory, so the prompt,
/// messages and tool input never reach the disk. `@EVENTS@` is the events folder. It carries no
/// version: Codex untrusts a hook that changed.
const SCRIPT: &str = r#"#!/bin/sh
# Pitwall: Codex hook, run as `codex-hook.sh <event>`. Notes for the Pitwall app which session
# changed state and where; never the prompt, a message or a tool's input or output (of a tool
# only its name). Prints nothing and always exits 0, so it can't affect Codex.
dir=@EVENTS@
case "$1" in ''|*[!A-Za-z]*) exit 0 ;; esac
input=$(cat)
out="event=$1"
for key in session_id turn_id agent_id cwd source tool_name; do
  value=$(printf '%s' "$input" | /usr/bin/plutil -extract "$key" raw -o - - 2>/dev/null) || continue
  case "$value" in ''|*'
'*) continue ;; esac
  out="$out
$key=$value"
done
out="$out
ppid=$PPID"
[ -d "$dir" ] || mkdir -p "$dir" 2>/dev/null || exit 0
file="$dir/$$-${RANDOM:-0}"
{ printf '%s\n' "$out" > "$file.tmp" && mv -f "$file.tmp" "$file.event"; } 2>/dev/null
exit 0
"#;

/// How Codex runs Pitwall's hook. The app is `None` in a dev build: its binary moves with the
/// build folder, so it can't be the hook.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Runner {
    /// macOS: the script, `~/.pitwall/hooks/codex-hook.sh <Event>`.
    Script,
    /// Elsewhere: `<app> --codex-hook <Event>`, one command line (Codex's handlers take no
    /// arguments of their own): single quotes for `sh`, double quotes on Windows.
    App(Option<PathBuf>),
}

impl Runner {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Runner::Script
        } else {
            Runner::App(installed_app())
        }
    }

    /// The command line that runs `event`; none from a dev build off macOS.
    fn command(&self, script: &Path, event: &str) -> Option<String> {
        match self {
            Runner::Script => Some(format!("{} {event}", shell_quote(&script.to_string_lossy()))),
            Runner::App(app) => app.as_deref().map(|app| format!("{} {HOOK_FLAG} {event}", quote_app(&app.to_string_lossy()))),
        }
    }
}

/// The app's path as a command line's first word.
fn quote_app(path: &str) -> String {
    if cfg!(windows) {
        let plain = path.chars().all(|c| c.is_ascii_alphanumeric() || "\\/:._-".contains(c));
        if plain {
            path.to_string()
        } else {
            format!("\"{path}\"")
        }
    } else {
        shell_quote(path)
    }
}

/// A command that runs Pitwall as Codex's hook, wherever its binary is (a moved install's entries
/// are still ours, to replace).
fn runs_app(command: &str) -> bool {
    command.contains(&format!(" {HOOK_FLAG} ")) && command.to_lowercase().contains("pitwall")
}

/// The only keys hook mode keeps from Codex's input. Every other value (the prompt, a tool's input
/// and output, Codex's last message) is skipped as it is read, never held.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HookInput {
    #[serde(deserialize_with = "scalar")]
    session_id: Option<String>,
    #[serde(deserialize_with = "scalar")]
    turn_id: Option<String>,
    #[serde(deserialize_with = "scalar")]
    agent_id: Option<String>,
    #[serde(deserialize_with = "scalar")]
    cwd: Option<String>,
    /// Why a session started: `startup` | `resume` | `clear` | `compact` | `fork`.
    #[serde(deserialize_with = "scalar")]
    source: Option<String>,
    /// The tool of a permission request or of a finished call, by name only.
    #[serde(deserialize_with = "scalar")]
    tool_name: Option<String>,
}

impl HookInput {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "session_id" => self.session_id.as_deref(),
            "turn_id" => self.turn_id.as_deref(),
            "agent_id" => self.agent_id.as_deref(),
            "cwd" => self.cwd.as_deref(),
            "source" => self.source.as_deref(),
            "tool_name" => self.tool_name.as_deref(),
            _ => None,
        }
    }
}

/// An event's name as the script lets it through: letters only.
fn is_event(event: &str) -> bool {
    !event.is_empty() && event.chars().all(|c| c.is_ascii_alphabetic())
}

/// The file for `event` from Codex's JSON `input`, as the script writes it: `event=<Event>`, the
/// keys that have a one-line value, then `ppid`. Only the first JSON value is read; unreadable
/// input still notes the event.
fn event_text(event: &str, input: impl Read, ppid: Option<u32>) -> Option<String> {
    if !is_event(event) {
        return None;
    }
    let mut stream = serde_json::Deserializer::from_reader(input.take(INPUT_LIMIT));
    let input = HookInput::deserialize(&mut stream).unwrap_or_default();
    let mut text = format!("event={event}\n");

    for key in KEYS {
        if let Some(value) = input.get(key).filter(|value| !value.is_empty() && !value.contains(['\n', '\r'])) {
            text.push_str(&format!("{key}={value}\n"));
        }
    }
    if let Some(ppid) = ppid {
        text.push_str(&format!("ppid={ppid}\n"));
    }
    Some(text)
}

/// Notes `event` in `dir`, reading `input` for a few seconds at most: Codex gives the hook five.
fn note_event(dir: &Path, event: &str, input: impl Read + Send + 'static, ppid: Option<u32>) -> Option<PathBuf> {
    let event = event.to_string();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(event_text(&event, input, ppid));
    });
    let text = receiver.recv_timeout(READ_WITHIN).ok().flatten()?;
    write_event(dir, &text).ok()
}

/// The hook's parent: the process Codex ran it from.
#[cfg(unix)]
fn own_parent() -> Option<u32> {
    Some(std::os::unix::process::parent_id()).filter(|pid| *pid > 1)
}

#[cfg(not(unix))]
fn own_parent() -> Option<u32> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let me = Pid::from_u32(std::process::id());
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), false, ProcessRefreshKind::nothing());
    system.process(me).and_then(|process| process.parent()).map(|parent| parent.as_u32()).filter(|pid| *pid > 0)
}

/// The nearest `codex` process of `pid` and its parents, at this moment: a shell Codex ran the
/// hook in that doesn't hand itself over to the command (Windows' never do) ends with the hook,
/// so the parent told is Codex's own. Their names and executables only, never a command line.
fn codex_above(pid: u32) -> Option<u32> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let mut system = System::new();
    let mut at = pid;
    for _ in 0..32 {
        if at <= 1 {
            return None;
        }
        let id = Pid::from_u32(at);
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[id]), false, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
        let process = system.process(id)?;
        let name = process.name().to_string_lossy().into_owned();
        let program = process.exe().map(|exe| exe.to_string_lossy().into_owned()).unwrap_or_default();
        if crate::process::is_codex(&crate::process::ProcessInfo { parent: 0, line: String::new(), name, program }) {
            return Some(at);
        }
        at = process.parent()?.as_u32();
    }
    None
}

/// Hook mode, Codex's hook on Windows and Linux: when `args` (after the program) are
/// `--codex-hook <Event>`, notes the event in `~/.pitwall/codex-events/` like the script does and
/// returns true, before anything of the app starts. Prints nothing and never fails, so it can't
/// affect Codex.
pub fn codex_hook_mode(mut args: impl Iterator<Item = OsString>) -> bool {
    if args.next().as_deref() != Some(OsStr::new(HOOK_FLAG)) {
        return false;
    }

    let event = args.next().and_then(|event| event.into_string().ok()).unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        let parent = own_parent().map(|parent| codex_above(parent).unwrap_or(parent));
        note_event(&home.join(".pitwall").join("codex-events"), &event, std::io::stdin(), parent);
    }
    true
}

/// Where Pitwall's hook stands in one Codex home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CodexHookStatus {
    /// `Missing`, or `Waiting` once it is in `hooks.json`: whether Codex runs it (`Working`) only
    /// the events tell, which `Core` weighs (`CodexWatch::hook_state`).
    pub state: HookState,
    /// Installed, but not this version's entries (or a script of an app that moved).
    pub outdated: bool,
}

/// Modification time and size of `hooks.json` and the script, to read them only after a change.
type Stamp = [Option<(SystemTime, u64)>; 2];

/// Pitwall's hook in the Codex home it was made for (the main one, or an account's: CX5 makes one
/// per account).
pub struct CodexHook {
    /// `<home>/hooks.json`, the only file of Codex's it writes.
    hooks_file: PathBuf,
    /// The script on macOS, `~/.pitwall/hooks/codex-hook.sh`: one for every home.
    script_file: PathBuf,
    /// Where its events go (`~/.pitwall/codex-events`).
    events_dir: PathBuf,
    runner: Runner,
    cached: Mutex<Option<(Stamp, CodexHookStatus)>>,
    /// `~/.pitwall/codex-hook-heard.json`, Pitwall's own: which `hooks.json` (its path and when it
    /// was last written) an event was heard after, so that a trusted hook reads at work after a
    /// restart of Pitwall's. Nothing of a session goes into it.
    heard_file: PathBuf,
    /// That file as last read or written (`None` before the first read).
    heard_for: Mutex<Option<Option<u64>>>,
}

/// `codex-hook-heard.json`: the `hooks.json` an event was heard after, and when it had changed.
#[derive(serde::Serialize, Deserialize)]
struct Heard {
    hooks: PathBuf,
    changed: u64,
}

impl CodexHook {
    pub fn new(codex_home: &Path, registry_dir: &Path) -> Self {
        Self {
            hooks_file: codex_home.join("hooks.json"),
            script_file: registry_dir.join("hooks").join(SCRIPT_NAME),
            events_dir: registry_dir.join("codex-events"),
            runner: Runner::current(),
            cached: Mutex::new(None),
            heard_file: registry_dir.join("codex-hook-heard.json"),
            heard_for: Mutex::new(None),
        }
    }

    /// When the `hooks.json` an event was last heard after had changed, as written down; none for
    /// another home's file or before any.
    fn heard_for(&self) -> Option<u64> {
        let mut known = self.heard_for.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *known.get_or_insert_with(|| {
            let heard: Heard = serde_json::from_str(&fs::read_to_string(&self.heard_file).ok()?).ok()?;
            (heard.hooks == self.hooks_file).then_some(heard.changed)
        })
    }

    /// Whether an event was heard after `hooks.json` as it is now (`changed_at`), in this run or an
    /// earlier one: Codex trusts it, unless it changed since.
    pub fn heard_since_changed(&self) -> bool {
        self.changed_at().is_some_and(|changed| self.heard_for() == Some(changed))
    }

    /// Writes down that an event was heard after `hooks.json` as it is now; once per change.
    pub fn note_heard(&self) {
        let Some(changed) = self.changed_at() else {
            return;
        };
        if self.heard_for() == Some(changed) {
            return;
        }
        let heard = Heard { hooks: self.hooks_file.clone(), changed };
        let written = serde_json::to_string(&heard).ok().is_some_and(|json| write_atomic(&self.heard_file, &json).is_ok());
        if written {
            *self.heard_for.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Some(changed));
        }
    }

    fn read_hooks(&self) -> Result<Value, String> {
        match fs::read_to_string(&self.hooks_file) {
            Ok(raw) if raw.trim().is_empty() => Ok(json!({})),
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(value @ Value::Object(_)) => Ok(value),
                _ => Err(t!("core.error.invalidJson", file = self.hooks_file.display())),
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
        let command = self.runner.command(&self.script_file, event)?;
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        entry.insert("hooks".into(), json!([{ "type": "command", "command": command, "timeout": TIMEOUT_S }]));
        Some(Value::Object(entry))
    }

    /// Every entry Pitwall wants, by event; none from a dev build off macOS.
    fn wanted(&self) -> Option<Vec<(&'static str, Value)>> {
        EVENTS.iter().map(|(event, matcher)| self.entry(event, *matcher).map(|entry| (*event, entry))).collect()
    }

    /// A handler that runs Pitwall: the script, or the app in hook mode.
    fn is_our_hook(&self, hook: &Value) -> bool {
        let command = hook.get("command").and_then(Value::as_str).unwrap_or_default();
        command.contains(self.script_file.to_string_lossy().as_ref()) || runs_app(command)
    }

    /// An entry that runs Pitwall: one of its handlers is ours.
    fn is_ours(&self, entry: &Value) -> bool {
        entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| hooks.iter().any(|hook| self.is_our_hook(hook)))
    }

    /// An entry that runs nothing, left where one of Pitwall's was.
    fn inert() -> Value {
        json!({ "hooks": [] })
    }

    /// An entry whose handlers are all Pitwall's.
    fn is_wholly_ours(&self, entry: &Value) -> bool {
        entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| !hooks.is_empty() && hooks.iter().all(|hook| self.is_our_hook(hook)))
    }

    /// Takes Pitwall's handlers out of `entries`, but for `keep` (the entry wanted, kept whole). An
    /// entry that holds someone else's handlers too keeps those; one left with none goes, or stays
    /// inert while entries after it stay: Codex trusts a hook by its place, so theirs keep theirs.
    /// Whether anything changed.
    fn strip_ours(&self, entries: &mut Vec<Value>, keep: Option<&Value>) -> bool {
        let mut changed = false;
        let mut later = false;
        for at in (0..entries.len()).rev() {
            let entry = &mut entries[at];
            if !self.is_ours(entry) || Some(&*entry) == keep {
                later = true;
                continue;
            }
            changed = true;
            if let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                hooks.retain(|hook| !self.is_our_hook(hook));
                if !hooks.is_empty() {
                    later = true;
                    continue;
                }
            }
            if later {
                *entry = Self::inert();
            } else {
                entries.remove(at);
            }
        }
        changed
    }

    fn status_of(&self, file: &Value) -> CodexHookStatus {
        let empty = Map::new();
        let hooks = file.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
        let entries = |event: &str| hooks.get(event).and_then(Value::as_array).cloned().unwrap_or_default();

        let installed = hooks.keys().any(|event| entries(event).iter().any(|entry| self.is_ours(entry)));
        let state = if installed { HookState::Waiting } else { HookState::Missing };
        // A dev build can't say what the entries should run; the installed app judges them.
        let Some(wanted) = self.wanted() else {
            return CodexHookStatus { state, outdated: false };
        };
        let missing = wanted.iter().any(|(event, entry)| !entries(event).contains(entry));
        let stray = hooks.keys().any(|event| {
            let wanted = wanted.iter().find(|(name, _)| name == event).map(|(_, entry)| entry);
            entries(event).iter().any(|entry| self.is_ours(entry) && Some(entry) != wanted)
        });
        let script_current =
            self.runner != Runner::Script || fs::read_to_string(&self.script_file).is_ok_and(|script| script == self.script());

        CodexHookStatus { state, outdated: installed && (missing || stray || !script_current) }
    }

    /// Not installed, or installed (and whether as this version wants): read again only when
    /// `hooks.json` or the script changed.
    pub fn status(&self) -> CodexHookStatus {
        let now: Stamp = [stamp(&self.hooks_file), stamp(&self.script_file)];
        let mut cached = self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some((at, status)) = cached.as_ref() {
            if *at == now {
                return *status;
            }
        }

        let status = self.read_hooks().map(|file| self.status_of(&file)).unwrap_or_default();
        *cached = Some((now, status));
        status
    }

    /// When `hooks.json` was last written: Codex untrusts a changed hook, so sessions begun since
    /// tell whether the user trusted it again.
    pub fn changed_at(&self) -> Option<u64> {
        stamp(&self.hooks_file).map(|(at, _)| crate::claude::system_ms(at))
    }

    /// Adds every entry Pitwall wants, replacing ours that differ (a moved app) in their place, and
    /// writes the script on macOS; an invalid file is left alone.
    pub fn install(&self) -> Result<(), String> {
        // A dev build's binary moves with the build folder: only the installed app adds the hook.
        let wanted = self.wanted().ok_or_else(|| t!("core.error.hookDev"))?;
        let mut file = self.read_hooks()?;

        // The script first: `hooks.json` must never point at a missing script.
        if self.runner == Runner::Script {
            if let Some(dir) = self.script_file.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            if fs::read_to_string(&self.script_file).ok().as_deref() != Some(self.script().as_str()) {
                write_atomic(&self.script_file, &self.script()).map_err(|e| e.to_string())?;
            }
            make_executable(&self.script_file)?;
        }
        let _ = fs::create_dir_all(&self.events_dir);

        let shape = || t!("core.codex.hooksShape", file = self.hooks_file.display());
        let root = file.as_object_mut().ok_or_else(shape)?;
        let hooks = root.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
        let hooks = hooks.as_object_mut().ok_or_else(shape)?;
        let mut changed = false;

        let mut emptied = Vec::new();
        for (event, entries) in hooks.iter_mut() {
            let wanted = wanted.iter().find(|(name, _)| name == event).map(|(_, entry)| entry);
            if let Some(entries) = entries.as_array_mut() {
                // The entry wanted takes the place of one of ours that differs (a moved app's).
                if let Some(wanted) = wanted.filter(|wanted| !entries.contains(wanted)) {
                    if let Some(stale) = entries.iter_mut().find(|entry| self.is_wholly_ours(entry)) {
                        *stale = wanted.clone();
                        changed = true;
                    }
                }
                if self.strip_ours(entries, wanted) {
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
            let entries = entries.as_array_mut().ok_or_else(shape)?;
            if !entries.contains(&wanted) {
                // Into the place a removal left inert, else after the others.
                match entries.iter_mut().find(|entry| **entry == Self::inert()) {
                    Some(inert) => *inert = wanted,
                    None => entries.push(wanted),
                }
                changed = true;
            }
        }

        if changed {
            self.backup_once()?;
            self.write_hooks(&file)?;
        }
        Ok(())
    }

    /// Takes Pitwall's entries out, and the script once no other home could still run it.
    pub fn uninstall(&self) -> Result<(), String> {
        self.take_out()?;
        let _ = fs::remove_file(&self.script_file);
        Ok(())
    }

    /// Takes Pitwall's entries out of this `hooks.json` only, nothing else of it; the script stays
    /// for the other homes'.
    pub fn take_out(&self) -> Result<(), String> {
        let mut file = self.read_hooks()?;
        let mut changed = false;

        if let Some(hooks) = file.get_mut("hooks").and_then(Value::as_object_mut) {
            let mut emptied = Vec::new();
            for (event, entries) in hooks.iter_mut() {
                if let Some(entries) = entries.as_array_mut() {
                    if self.strip_ours(entries, None) {
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
                file.as_object_mut().map(|root| root.remove("hooks"));
            }
        }

        if changed {
            // A file Pitwall made (no copy of the user's was kept) that holds nothing more goes.
            let made = !self.hooks_file.with_extension("json.pitwall-backup").exists();
            if made && file.as_object().is_some_and(Map::is_empty) && fs::symlink_metadata(&self.hooks_file).is_ok_and(|meta| meta.is_file()) {
                return fs::remove_file(&self.hooks_file).map_err(|e| e.to_string());
            }
            self.write_hooks(&file)?;
        }
        Ok(())
    }

    /// The file as it was before Pitwall first touched it.
    fn backup_once(&self) -> Result<(), String> {
        let backup = self.hooks_file.with_extension("json.pitwall-backup");
        if self.hooks_file.exists() && !backup.exists() {
            fs::copy(&self.hooks_file, backup).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Where the file is written: a link (to a dotfiles folder) is written through, so it stays a
    /// link.
    fn hooks_target(&self) -> PathBuf {
        match fs::read_link(&self.hooks_file) {
            Ok(target) => fs::canonicalize(&self.hooks_file)
                .unwrap_or_else(|_| self.hooks_file.parent().map_or_else(|| target.clone(), |dir| dir.join(&target))),
            Err(_) => self.hooks_file.clone(),
        }
    }

    fn write_hooks(&self, file: &Value) -> Result<(), String> {
        let target = self.hooks_target();
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
        write_atomic(&target, &format!("{json}\n")).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(tmp: &Path) -> PathBuf {
        tmp.join("Applications").join("Pitwall.app").join("Contents").join("MacOS").join("pitwall")
    }

    /// The home's hook as the installed app would write it, with each runner.
    fn every_runner(tmp: &Path) -> Vec<CodexHook> {
        [Runner::Script, Runner::App(Some(app(tmp)))]
            .into_iter()
            .map(|runner| CodexHook { runner, ..CodexHook::new(&tmp.join("codex"), &tmp.join("registry")) })
            .collect()
    }

    fn read(hook: &CodexHook) -> Value {
        serde_json::from_str(&fs::read_to_string(&hook.hooks_file).unwrap()).unwrap()
    }

    fn ours(file: &Value, hook: &CodexHook, event: &str) -> usize {
        file["hooks"][event].as_array().map_or(0, |entries| entries.iter().filter(|entry| hook.is_ours(entry)).count())
    }

    /// `hooks.json` holds the user's own hooks (Codex runs them once trusted, by their place in
    /// the file) and Codex's own `config.toml` the trust: an install that loses one of them, adds
    /// an entry twice or writes into `config.toml` can't be taken back.
    #[test]
    fn hooks_json_keeps_the_users_hooks_and_nothing_else_is_written() {
        let users = json!({
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "say done", "timeout": 3 }] }],
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "~/bin/guard" }] }]
            },
            "note": "the user's"
        });
        let config = "model = \"gpt-5\"\n[hooks.state.\"x:stop:0:0\"]\ntrusted_hash = \"abc\"\n";

        for index in 0..2 {
            let tmp = tempfile::tempdir().unwrap();
            let hook = every_runner(tmp.path()).remove(index);
            let home = tmp.path().join("codex");
            fs::create_dir_all(&home).unwrap();
            fs::write(home.join("config.toml"), config).unwrap();
            let original = serde_json::to_string_pretty(&users).unwrap();
            fs::write(&hook.hooks_file, &original).unwrap();

            hook.install().unwrap();
            hook.install().unwrap();
            let file = read(&hook);
            assert_eq!(file["note"], "the user's");
            assert_eq!(file["hooks"]["Stop"][0], users["hooks"]["Stop"][0], "the user's own entry keeps its place");
            assert_eq!(file["hooks"]["PreToolUse"], users["hooks"]["PreToolUse"]);
            for (event, _) in EVENTS {
                assert_eq!(ours(&file, &hook, event), 1, "{event} once");
            }
            assert_eq!(hook.status(), CodexHookStatus { state: HookState::Waiting, outdated: false });
            assert_eq!(fs::read_to_string(home.join("hooks.json.pitwall-backup")).unwrap(), original);

            // A moved app: its entries are ours still, replaced in place of being added.
            if let Runner::App(_) = hook.runner {
                let moved = CodexHook { runner: Runner::App(Some(tmp.path().join("Other").join("pitwall"))), ..CodexHook::new(&home, &tmp.path().join("registry")) };
                assert!(moved.status().outdated);
                moved.install().unwrap();
                let file = read(&moved);
                for (event, _) in EVENTS {
                    assert_eq!(ours(&file, &moved, event), 1, "{event} once after the move");
                }
                assert!(!file.to_string().contains(&app(tmp.path()).to_string_lossy().into_owned()));
            }

            hook.uninstall().unwrap();
            let file = read(&hook);
            assert_eq!(file, users, "only Pitwall's entries go");
            assert_eq!(hook.status().state, HookState::Missing);
            assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), config, "config.toml is never written");
            let mut names: Vec<String> = fs::read_dir(&home).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            assert_eq!(names, ["config.toml", "hooks.json", "hooks.json.pitwall-backup"]);
        }
    }

    /// Codex trusts a hook by its place (`<file>:<event>:<group>:<handler>`): a user's entry added
    /// after Pitwall's that moves up when ours is replaced or taken out stops running, without a
    /// word, until the user trusts it again.
    #[test]
    fn the_users_later_entries_keep_their_place_when_ours_are_replaced_or_taken_out() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex");
        fs::create_dir_all(&home).unwrap();
        let registry = tmp.path().join("registry");
        let users = json!({ "hooks": [{ "type": "command", "command": "say done", "timeout": 3 }] });
        let place = |file: &Value| file["hooks"]["Stop"].as_array().and_then(|entries| entries.iter().position(|entry| *entry == users));

        let hook = CodexHook { runner: Runner::App(Some(app(tmp.path()))), ..CodexHook::new(&home, &registry) };
        hook.install().unwrap();
        let mut file = read(&hook);
        file["hooks"]["Stop"].as_array_mut().unwrap().push(users.clone());
        fs::write(&hook.hooks_file, serde_json::to_string_pretty(&file).unwrap()).unwrap();

        let moved = CodexHook { runner: Runner::App(Some(tmp.path().join("Other").join("pitwall"))), ..CodexHook::new(&home, &registry) };
        moved.install().unwrap();
        let file = read(&moved);
        assert_eq!(place(&file), Some(1), "after a move: {file}");
        assert!(moved.is_ours(&file["hooks"]["Stop"][0]));
        assert_eq!(ours(&file, &moved, "Stop"), 1);

        moved.uninstall().unwrap();
        let file = read(&moved);
        assert_eq!(file["hooks"]["Stop"], json!([{ "hooks": [] }, users]), "an inert entry holds our place");
        assert_eq!(file["hooks"].as_object().unwrap().keys().collect::<Vec<_>>(), ["Stop"], "with nothing after ours, ours go");
        assert_eq!(moved.status().state, HookState::Missing);

        moved.install().unwrap();
        let file = read(&moved);
        assert_eq!(place(&file), Some(1), "installed again: {file}");
        assert_eq!(file["hooks"]["Stop"].as_array().unwrap().len(), 2, "into the inert entry's place");
        assert_eq!(moved.status(), CodexHookStatus { state: HookState::Waiting, outdated: false });
    }

    #[test]
    fn an_invalid_hooks_json_is_never_touched() {
        let tmp = tempfile::tempdir().unwrap();
        for hook in every_runner(tmp.path()) {
            fs::create_dir_all(hook.hooks_file.parent().unwrap()).unwrap();
            fs::write(&hook.hooks_file, "{ \"hooks\": { oops").unwrap();
            assert!(hook.install().is_err());
            assert!(hook.uninstall().is_err());
            assert_eq!(fs::read_to_string(&hook.hooks_file).unwrap(), "{ \"hooks\": { oops");
            assert!(!hook.hooks_file.with_extension("json.pitwall-backup").exists());
        }
    }

    const SECRET: &str = "SECRET-7f3a";

    /// Codex's input for `event`, with the marker in every value Pitwall must never write.
    fn input(event: &str) -> Value {
        json!({
            "session_id": "019a-s", "turn_id": "019a-t", "transcript_path": format!("/tmp/{SECRET}.jsonl"),
            "cwd": "/Users/test/projects/demo", "hook_event_name": event, "model": SECRET, "permission_mode": SECRET,
            "source": "startup", "tool_name": "Bash", "tool_use_id": SECRET, "agent_id": "agent-1", "agent_type": SECRET,
            "prompt": format!("fix it {SECRET}"),
            "tool_input": { "command": format!("rm -rf {SECRET}"), "description": SECRET },
            "tool_response": { "stdout": SECRET, "exit_code": 0 },
            "last_assistant_message": format!("done {SECRET}"),
            "stop_hook_active": false, "reason": SECRET,
        })
    }

    /// What hook mode writes is all that leaves Codex's input; a prompt, a command or a message
    /// on disk can't be taken back.
    #[test]
    fn hook_mode_writes_only_the_listed_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("events");

        for (event, _) in EVENTS {
            let raw = input(event).to_string();
            let file = note_event(&dir, event, std::io::Cursor::new(raw.into_bytes()), Some(4242)).unwrap();
            let text = fs::read_to_string(&file).unwrap();
            assert!(!text.contains(SECRET), "{event}: {text}");
            let keys: Vec<&str> = text.lines().map(|line| line.split_once('=').unwrap().0).collect();
            assert_eq!(keys, ["event", "session_id", "turn_id", "agent_id", "cwd", "source", "tool_name", "ppid"], "{event}");
            assert!(text.ends_with("ppid=4242\n"));
            fs::remove_file(file).unwrap();
        }

        // A name that isn't a word notes nothing; input that isn't JSON still notes the event.
        assert!(note_event(&dir, "../x", std::io::Cursor::new(Vec::new()), None).is_none());
        let file = note_event(&dir, "Stop", std::io::Cursor::new(b"not json".to_vec()), None).unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "event=Stop\n");
    }

    /// On macOS the script is the hook: it writes what hook mode writes, nothing more.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_script_writes_what_hook_mode_writes() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let tmp = tempfile::tempdir().unwrap();
        let hook = CodexHook { runner: Runner::Script, ..CodexHook::new(&tmp.path().join("codex"), &tmp.path().join("registry")) };
        hook.install().unwrap();

        for event in ["SessionStart", "PermissionRequest", "Stop"] {
            let raw = input(event).to_string();
            let mut child = Command::new(&hook.script_file).arg(event).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(raw.as_bytes()).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert!(output.stdout.is_empty(), "the hook prints nothing");

            let written: Vec<PathBuf> = fs::read_dir(&hook.events_dir).unwrap().flatten().map(|entry| entry.path()).collect();
            assert_eq!(written.len(), 1);
            let text = fs::read_to_string(&written[0]).unwrap();
            fs::remove_file(&written[0]).unwrap();

            let ppid = std::process::id();
            assert_eq!(text, event_text(event, std::io::Cursor::new(raw.into_bytes()), Some(ppid)).unwrap(), "{event}");
        }
    }
}
