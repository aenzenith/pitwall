//! What the window asks of Codex's accounts (`commands.rs` with `agent: "codex"`), and what it
//! hears back: `fuel` with every account (`fuel.rs`), `codex-account-login` while one is being
//! added (with the lines Codex prints, for "Show the terminal"), `codex-account-sync` while one's
//! MCP servers are brought level with the main account's. Claude's are `app.rs`'s, as before.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use tauri::{AppHandle, Emitter};

use super::app::{home, state, LoginEvent, NewFolder, PickedFolder, SyncEvent};
use super::codex::{check_dir, new_dir, prepare, shares_main, unlink};
use super::codex_sync;
use super::sync::{SyncReport, SyncServer};
use super::{check_name, listed, login, new_id, shown, Prepared, MAIN};
use crate::agent::Agent;
use crate::i18n::t;
use crate::settings::ClaudeAccount;
use crate::MAIN as MAIN_WINDOW;

/// Claude's folders, its main one's and each account's: no Codex account may have one, be inside
/// one or hold one (`check_dir`).
fn claude_folders(app: &AppHandle, home: &Path) -> Vec<PathBuf> {
    super::folders_of(super::main_dir(home), &state(app).core.accounts())
}

fn tell_login(app: &AppHandle, event: LoginEvent) {
    let _ = app.emit_to(MAIN_WINDOW, "codex-account-login", event);
}

fn tell_sync(app: &AppHandle, id: &str, state: &'static str, report: Option<SyncReport>) {
    let _ = app.emit_to(MAIN_WINDOW, "codex-account-sync", SyncEvent { id: id.to_string(), state, report });
}

fn main_home(app: &AppHandle) -> PathBuf {
    state(app).core.codex_main_home()
}

/// The account `id` as listed, when it is another than the main one.
fn other(app: &AppHandle, id: &str) -> Result<ClaudeAccount, String> {
    listed(&state(app).core.codex_accounts(), id).cloned().ok_or_else(|| t!("core.accounts.unknown"))
}

/// The account the Codex sessions Pitwall starts use from now on. Switched to another than the
/// main one, its MCP servers are brought level (its switch on) or looked at (off: `differs` when
/// the main one has more).
pub fn use_account(app: &AppHandle, id: &str) -> Result<(), String> {
    state(app).core.change_codex_accounts(|accounts| {
        if id == MAIN {
            accounts.in_use = None;
            return Ok(());
        }
        let account = listed(accounts, id).ok_or_else(|| t!("core.accounts.unknown"))?;
        if !Path::new(&account.dir).is_dir() {
            return Err(t!("core.accounts.missing", name = account.name));
        }
        accounts.in_use = Some(id.to_string());
        Ok(())
    })?;
    state(app).fuel.codex_changed(app);

    if id != MAIN {
        let account = other(app, id)?;
        let app = app.clone();
        thread::spawn(move || {
            if account.sync {
                run_sync(&app, &account);
            } else if let Ok(home) = home() {
                let report = codex_sync::plan(&home, &account.dir).report();
                if report.differs() {
                    tell_sync(&app, &account.id, "differs", Some(report));
                }
            }
        });
    }
    Ok(())
}

/// Syncs `account` now, telling the window as it goes.
fn run_sync(app: &AppHandle, account: &ClaudeAccount) -> SyncReport {
    tell_sync(app, &account.id, "syncing", None);
    let home = home().unwrap_or_default();
    // No Codex to write with (nor to list with): whatever was to be brought over wasn't.
    let report = codex_sync::sync(&home, &account.dir).unwrap_or_default();
    let report = SyncReport { servers: report.servers.into_iter().map(|server| SyncServer { state: if server.state == "missing" { "failed" } else { server.state }, ..server }).collect() };
    tell_sync(app, &account.id, if report.failed() { "failed" } else { "done" }, Some(report.clone()));
    report
}

/// "Sync" and "Try again": now, whatever its switch says.
pub fn sync_now(app: &AppHandle, id: &str) -> Result<SyncReport, String> {
    let account = other(app, id)?;
    Ok(run_sync(app, &account))
}

/// What a sync would do, writing nothing.
pub fn sync_plan(app: &AppHandle, id: &str) -> Result<SyncReport, String> {
    let account = other(app, id)?;
    Ok(codex_sync::plan(&home()?, &account.dir).report())
}

/// A new name; the main account's goes back to the catalog's when it is emptied.
pub fn rename(app: &AppHandle, id: &str, name: &str) -> Result<(), String> {
    state(app).core.change_codex_accounts(|accounts| {
        if id == MAIN {
            accounts.main_name = match name.trim() {
                "" => None,
                _ => Some(check_name(name, accounts, Some(MAIN))?),
            };
            return Ok(());
        }
        let name = check_name(name, accounts, Some(id))?;
        let account = accounts.others.iter_mut().find(|account| account.id == id).ok_or_else(|| t!("core.accounts.unknown"))?;
        account.name = name;
        Ok(())
    })?;
    state(app).fuel.codex_changed(app);
    Ok(())
}

/// Takes an account other than the main one off the list, and out of its home what Pitwall put
/// there: its links, its hook and (while Codex is on) the radio's entry. The home, its sessions and the sign-in in it stay. Refused while
/// Codex sessions run with it. The main one is in use afterwards if this one was.
pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
    if id == MAIN {
        return Err(t!("core.accounts.mainStays"));
    }
    let core = &state(app).core;
    if core.codex_account_runs(id) {
        let name = core.codex_accounts().others.into_iter().find(|account| account.id == id).map(|account| account.name).unwrap_or_default();
        return Err(t!("core.accounts.running.codex", name = name));
    }
    let account = core.change_codex_accounts(|accounts| {
        let at = accounts.others.iter().position(|account| account.id == id).ok_or_else(|| t!("core.accounts.unknown"))?;
        if accounts.in_use.as_deref() == Some(id) {
            accounts.in_use = None;
        }
        Ok(accounts.others.remove(at))
    })?;
    state(app).fuel.codex_changed(app);

    if Path::new(&account.dir).is_dir() {
        let _ = core.codex_account_hook(&account.dir, false);
        if core.settings().agents.codex.radio && core.agent_on(Agent::Codex) {
            let _ = core.codex_radio_home(Path::new(&account.dir), false);
        }
        unlink(&main_home(app), Path::new(&account.dir));
    }
    Ok(())
}

pub fn set_sync(app: &AppHandle, id: &str, on: bool) -> Result<(), String> {
    state(app).core.change_codex_accounts(|accounts| {
        let account = accounts.others.iter_mut().find(|account| account.id == id).ok_or_else(|| t!("core.accounts.unknown"))?;
        account.sync = on;
        Ok(())
    })?;
    state(app).fuel.codex_changed(app);
    Ok(())
}

/// The account's home, in Finder.
pub fn reveal(app: &AppHandle, id: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let dir = match id {
        MAIN => main_home(app),
        _ => PathBuf::from(other(app, id)?.dir),
    };
    if !dir.is_dir() {
        return Err(t!("core.accounts.folderMissing", folder = shown(&home()?, &dir.to_string_lossy())));
    }
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|error| error.to_string())
}

/// The home a new account named `name` gets.
pub fn folder(app: &AppHandle, name: &str) -> Result<NewFolder, String> {
    let home = home()?;
    let dir = new_dir(&home, name, &state(app).core.codex_accounts());
    Ok(NewFolder { shown: shown(&home, &dir.to_string_lossy()), exists: dir.is_dir() })
}

/// A folder picked in the system's dialog, checked as an account's home (`check_dir`).
pub fn pick_folder(app: &AppHandle) -> Result<Option<PickedFolder>, String> {
    use tauri_plugin_dialog::DialogExt;

    let home = home()?;
    let picked = app.dialog().file().set_title(t!("core.dialog.accountFolder")).set_directory(&home).blocking_pick_folder();
    let Some(path) = picked.and_then(|folder| folder.into_path().ok()) else {
        return Ok(None);
    };
    let path = path.to_string_lossy().into_owned();
    check_dir(&home, &main_home(app), &path, &state(app).core.codex_accounts(), None, true, &claude_folders(app, &home))?;
    Ok(Some(PickedFolder { shown: shown(&home, &path), path }))
}

/// Starts adding an account: `dir`, a folder picked (`pick_folder`), or none for the new one its
/// name gives. The home is set up and Codex's sign-in started (`codex login` with it); the
/// account is added once that is done (`finish`), and the home put back as it was if it isn't.
pub fn login(app: &AppHandle, name: &str, dir: Option<String>, shared: bool, sync: bool) -> Result<(), String> {
    let home = home()?;
    let main = main_home(app);
    let (accounts, claude) = (state(app).core.codex_accounts(), claude_folders(app, &home));
    let name = check_name(name, &accounts, None)?;
    let dir = match dir {
        Some(dir) => {
            check_dir(&home, &main, &dir, &accounts, None, true, &claude)?;
            dir
        }
        None => {
            let dir = new_dir(&home, &name, &accounts).to_string_lossy().into_owned();
            check_dir(&home, &main, &dir, &accounts, None, false, &claude)?;
            dir
        }
    };
    if login::busy() {
        return Err(t!("core.accounts.loginBusy"));
    }
    let prepared = Arc::new(prepare(&main, Path::new(&dir), shared)?);
    tell_login(app, LoginEvent::Waiting { url: None });

    let (on_url, on_line, on_end) = (app.clone(), app.clone(), app.clone());
    let (folder, kept) = (dir.clone(), Arc::clone(&prepared));
    let started = login::start_codex(
        &dir,
        move |url| tell_login(&on_url, LoginEvent::Waiting { url: Some(url) }),
        move |line| tell_login(&on_line, LoginEvent::Line { line }),
        move |ended| {
            let undo = || prepared.undo(Path::new(&folder));
            match ended {
                login::Ended::Done => finish(&on_end, ClaudeAccount { id: String::new(), name, dir: folder.clone(), shared, sync }, &prepared),
                login::Ended::Closed(error) => {
                    undo();
                    tell_login(&on_end, LoginEvent::Failed { reason: "closed", error });
                }
                login::Ended::Missing => {
                    undo();
                    tell_login(&on_end, LoginEvent::Failed { reason: "cli", error: None });
                }
                login::Ended::Cancelled => undo(),
            }
        },
    );
    if let Err(error) = started {
        kept.undo(Path::new(&dir));
        return Err(error);
    }
    Ok(())
}

/// The sign-in is done: the account is added, then Pitwall's hook put into its own `hooks.json`
/// (when the main home has it; Codex asks for its trust in that account), and the window told,
/// with the MCP servers a sync would bring over when it is switched to, and Pitwall among its MCP
/// servers while Codex's radio is on (`codex_radio_home`, as the radio puts it in every home).
/// Stopped meanwhile by the user, nothing is added.
fn finish(app: &AppHandle, mut account: ClaudeAccount, prepared: &Prepared) {
    if login::cancelled() {
        prepared.undo(Path::new(&account.dir));
        return;
    }
    let core = &state(app).core;
    let (home, main) = (home().unwrap_or_default(), main_home(app));
    account.shared = shares_main(&main, Path::new(&account.dir));
    let claude = claude_folders(app, &home);
    let added = core.change_codex_accounts(|accounts| {
        // Checked again: the list may have changed during the sign-in.
        account.name = check_name(&account.name, accounts, None)?;
        check_dir(&home, &main, &account.dir, accounts, None, true, &claude)?;
        account.id = new_id(accounts);
        accounts.others.push(account.clone());
        Ok(())
    });
    if let Err(error) = added {
        prepared.undo(Path::new(&account.dir));
        tell_login(app, LoginEvent::Failed { reason: "error", error: Some(error) });
        return;
    }
    state(app).fuel.codex_changed(app);

    let hooks = match core.codex_main_hook_installed() {
        false => "none",
        true => match core.codex_account_hook(&account.dir, true) {
            Ok(true) => "installed",
            Ok(false) | Err(_) => "failed",
        },
    };
    let radio = match core.settings().agents.codex.radio && core.agent_on(Agent::Codex) {
        false => "off",
        true if core.codex_radio_home(Path::new(&account.dir), true).is_ok() => "added",
        true => "failed",
    };
    let mcp = Some(codex_sync::plan(&home, &account.dir).report());
    tell_login(app, LoginEvent::Done { id: account.id.clone(), hooks, radio, mcp });
}
