//! What an uninstall takes out (`<app> --uninstall-cleanup`, which the Windows uninstaller runs
//! before it deletes the app: `src-tauri/windows/hooks.nsh`): Pitwall's hooks from Claude Code's
//! settings and Codex's `hooks.json`, the main account's and every other one's, and the team
//! radio's entry from their MCP servers while it is on. On Windows the hooks run the app itself,
//! so once it is gone every Claude session would stop on a program that isn't there.
//!
//! Only Pitwall's entries go, as **Remove hook** and **Turn radio off** take them out
//! (`ClaudeHook::take_out`, `CodexHook::take_out`, Claude Code's and Codex's own CLIs); nothing else
//! is read of Claude's or Codex's but those files, and nothing of Pitwall's own is deleted. It
//! prints nothing and always ends well, within `LIMIT`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use super::radio::register;
use crate::agent::Agent;
use crate::codex_hooks::CodexHook;
use crate::hooks::ClaudeHook;
use crate::settings::Settings;

/// The flag that runs the app as the uninstaller's cleanup.
const FLAG: &str = "--uninstall-cleanup";
/// How long an uninstall waits for it at most. The hooks go first, at once; the radio's CLIs
/// after them, each of which may take a while.
const LIMIT: Duration = Duration::from_secs(60);
/// The app's identifier (`tauri.conf.json`): its settings folder's name, as Tauri's
/// `app_config_dir` names it.
const IDENTIFIER: &str = "com.aenzenith.pitwall";

/// Uninstall mode: when `args` (after the program) are `--uninstall-cleanup`, takes Pitwall out of
/// Claude Code and Codex and returns true, before anything of the app starts.
pub fn uninstall_mode(mut args: impl Iterator<Item = OsString>) -> bool {
    if args.next().as_deref() != Some(OsStr::new(FLAG)) {
        return false;
    }
    let Some(home) = dirs::home_dir() else {
        return true;
    };
    let settings = dirs::config_dir().map(|config| config.join(IDENTIFIER).join("settings.json"));

    // What outlasts the limit is left: the process ends, and the CLIs it started with it.
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        clean(&home, settings.as_deref());
        let _ = done.send(());
    });
    let _ = finished.recv_timeout(LIMIT);
    true
}

/// Takes Pitwall's hooks out of Claude's main account and every other one `settings_file` lists,
/// and out of Codex's main home and its accounts' homes; then the team radio, through each
/// agent's CLI, where the settings say it is on. What one of them says holds none of the others
/// back.
pub(crate) fn clean(home: &Path, settings_file: Option<&Path>) {
    let settings = settings_file.map(Settings::load).unwrap_or_default();
    let registry = home.join(".pitwall");
    let codex_home = Agent::Codex.home_in(home);
    let claude_accounts: Vec<&str> = settings.accounts.others.iter().map(|account| account.dir.as_str()).filter(|dir| Path::new(dir).is_dir()).collect();
    let codex_accounts: Vec<PathBuf> =
        settings.agents.codex.accounts.others.iter().map(|account| PathBuf::from(&account.dir)).filter(|dir| dir.is_dir() && *dir != codex_home).collect();

    // The hooks first: Claude Code and Codex run them at every turn.
    for dir in &claude_accounts {
        let _ = ClaudeHook::new(Path::new(dir).join("settings.json"), &registry).take_out();
    }
    let _ = ClaudeHook::new(home.join(".claude").join("settings.json"), &registry).uninstall();
    for dir in &codex_accounts {
        let _ = CodexHook::new(dir, &registry).take_out();
    }
    let _ = CodexHook::new(&codex_home, &registry).uninstall();

    // Then the radio's entry, where Pitwall put it: `claude mcp remove`, `codex mcp remove`.
    if settings.radio.on {
        let _ = register::remove_in(None);
        for dir in &claude_accounts {
            let _ = register::remove_in(Some(dir));
        }
    }
    if settings.agents.codex.radio {
        let _ = register::codex_remove(None);
        for dir in &codex_accounts {
            let _ = register::codex_remove(Some(dir));
        }
    }
}
