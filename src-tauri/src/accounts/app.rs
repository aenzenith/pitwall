//! What the window asks of Claude's accounts (`commands.rs`), and what it hears back: `fuel`
//! with every account (`fuel.rs`), `account-login` while one is being added, `account-sync`
//! while one's MCP servers are brought level with the main account's.

use std::path::{Path, PathBuf};
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::sync::{self, SyncReport, SyncServer};
use super::{check_dir, check_name, listed, login, new_dir, new_id, prepare, shown, unlink, Prepared, MAIN};
use crate::i18n::t;
use crate::settings::ClaudeAccount;
use crate::{AppState, MAIN as MAIN_WINDOW};

/// A sign-in under way, as the window hears of it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LoginEvent {
    /// The browser was asked to open; `url`, its page, once Claude printed it.
    Waiting { url: Option<String> },
    /// Added, as `id`, with what was set up for it.
    Done {
        id: String,
        /// `shared` | `installed` | `failed` | `none`
        hooks: &'static str,
        /// `added` | `failed` | `off`
        radio: &'static str,
        mcp: Option<SyncReport>,
    },
    /// Not added: `closed`, `cli` (no Claude Code) or `error`.
    Failed { reason: &'static str, error: Option<String> },
}

/// A sync, as the window hears of it.
#[derive(Debug, Clone, Serialize)]
pub struct SyncEvent {
    id: String,
    /// `syncing` | `done` | `failed` | `differs`
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<SyncReport>,
}

/// The folder a new account gets, as the add dialog shows it.
#[derive(Debug, Clone, Serialize)]
pub struct NewFolder {
    shown: String,
    exists: bool,
}

/// A folder picked for an account.
#[derive(Debug, Clone, Serialize)]
pub struct PickedFolder {
    path: String,
    shown: String,
}

fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| t!("core.accounts.unknown"))
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// The listed projects: a sync brings their own servers over too.
fn listed_projects(app: &AppHandle) -> Vec<String> {
    state(app).core.snapshot().projects.into_iter().map(|project| project.path).collect()
}

fn tell_login(app: &AppHandle, event: LoginEvent) {
    let _ = app.emit_to(MAIN_WINDOW, "account-login", event);
}

fn tell_sync(app: &AppHandle, id: &str, state: &'static str, report: Option<SyncReport>) {
    let _ = app.emit_to(MAIN_WINDOW, "account-sync", SyncEvent { id: id.to_string(), state, report });
}

/// The account `id` as listed, when it is another than the main one.
fn other(app: &AppHandle, id: &str) -> Result<ClaudeAccount, String> {
    listed(&state(app).core.accounts(), id).cloned().ok_or_else(|| t!("core.accounts.unknown"))
}

/// The account in use from now on. Switched to another than the main one, its MCP servers are
/// brought level (its switch on) or looked at (off: `differs` when the main one has more).
pub fn use_account(app: &AppHandle, id: &str) -> Result<(), String> {
    let core = &state(app).core;
    core.change_accounts(|accounts| {
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
    state(app).fuel.accounts_changed(app);

    if id != MAIN {
        let account = other(app, id)?;
        let app = app.clone();
        thread::spawn(move || {
            if account.sync {
                run_sync(&app, &account);
            } else if let Ok(home) = home() {
                let report = sync::plan(&home, &account.dir, &listed_projects(&app)).report();
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
    let listed = listed_projects(app);
    let report = match sync::sync(&home, &account.dir, &listed) {
        Some(report) => report,
        // No Claude Code to write with: whatever was to be brought over wasn't.
        None => {
            let planned = sync::plan(&home, &account.dir, &listed).report();
            let servers = planned.servers.into_iter().map(|server| SyncServer { state: if server.state == "missing" { "failed" } else { server.state }, ..server }).collect();
            SyncReport { servers }
        }
    };
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
    Ok(sync::plan(&home()?, &account.dir, &listed_projects(app)).report())
}

/// A new name; the main account's goes back to the catalog's when it is emptied.
pub fn rename(app: &AppHandle, id: &str, name: &str) -> Result<(), String> {
    state(app).core.change_accounts(|accounts| {
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
    state(app).fuel.accounts_changed(app);
    Ok(())
}

/// Takes an account other than the main one off the list, and out of its folder what Pitwall put
/// there: its links, its hook (in settings of its own) and the radio's entry. The folder and the
/// sign-in in it stay. The main one is in use afterwards if this one was.
pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
    if id == MAIN {
        return Err(t!("core.accounts.mainStays"));
    }
    let core = &state(app).core;
    // Its sessions keep writing into the folders its links lead to: taken away under them, their
    // logs would go on in folders of its own, apart from the rest.
    if core.claudes_by_account().get(id).is_some_and(|count| *count > 0) {
        let name = core.accounts().others.into_iter().find(|account| account.id == id).map(|account| account.name).unwrap_or_default();
        return Err(t!("core.accounts.running", name = name));
    }
    let account = core.change_accounts(|accounts| {
        let at = accounts.others.iter().position(|account| account.id == id).ok_or_else(|| t!("core.accounts.unknown"))?;
        if accounts.in_use.as_deref() == Some(id) {
            accounts.in_use = None;
        }
        Ok(accounts.others.remove(at))
    })?;
    state(app).fuel.accounts_changed(app);

    if Path::new(&account.dir).is_dir() {
        let _ = core.account_hook(&account.dir, false);
        if core.settings().radio.on {
            let _ = core.radio_account(&account.dir, false);
        }
        unlink(&home()?, Path::new(&account.dir));
    }
    Ok(())
}

pub fn set_sync(app: &AppHandle, id: &str, on: bool) -> Result<(), String> {
    state(app).core.change_accounts(|accounts| {
        let account = accounts.others.iter_mut().find(|account| account.id == id).ok_or_else(|| t!("core.accounts.unknown"))?;
        account.sync = on;
        Ok(())
    })?;
    state(app).fuel.accounts_changed(app);
    Ok(())
}

/// The account's folder, in Finder.
pub fn reveal(app: &AppHandle, id: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let dir = match id {
        MAIN => super::main_dir(&home()?),
        _ => PathBuf::from(other(app, id)?.dir),
    };
    if !dir.is_dir() {
        return Err(t!("core.accounts.folderMissing", folder = shown(&home()?, &dir.to_string_lossy())));
    }
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|error| error.to_string())
}

/// The folder a new account named `name` gets.
pub fn folder(app: &AppHandle, name: &str) -> Result<NewFolder, String> {
    let home = home()?;
    let dir = new_dir(&home, name, &state(app).core.accounts());
    Ok(NewFolder { shown: shown(&home, &dir.to_string_lossy()), exists: dir.is_dir() })
}

/// A folder picked in the system's dialog, checked as an account's (`check_dir`).
pub fn pick_folder(app: &AppHandle) -> Result<Option<PickedFolder>, String> {
    use tauri_plugin_dialog::DialogExt;

    let home = home()?;
    let picked = app.dialog().file().set_title(t!("core.dialog.accountFolder")).set_directory(&home).blocking_pick_folder();
    let Some(path) = picked.and_then(|folder| folder.into_path().ok()) else {
        return Ok(None);
    };
    let path = path.to_string_lossy().into_owned();
    check_dir(&home, &path, &state(app).core.accounts(), None, true)?;
    Ok(Some(PickedFolder { shown: shown(&home, &path), path }))
}

/// Starts adding an account: `dir`, a folder picked (`pick_folder`), or none for the new one its
/// name gives. The folder is set up and Claude Code's sign-in started; the account is added once
/// that is done (`finish`), and the folder put back as it was if it isn't.
pub fn login(app: &AppHandle, name: &str, dir: Option<String>, shared: bool, sync: bool) -> Result<(), String> {
    let home = home()?;
    let accounts = state(app).core.accounts();
    let name = check_name(name, &accounts, None)?;
    let dir = match dir {
        Some(dir) => {
            check_dir(&home, &dir, &accounts, None, true)?;
            dir
        }
        None => {
            let dir = new_dir(&home, &name, &accounts).to_string_lossy().into_owned();
            check_dir(&home, &dir, &accounts, None, false)?;
            dir
        }
    };
    if login::busy() {
        return Err(t!("core.accounts.loginBusy"));
    }
    let prepared = std::sync::Arc::new(prepare(&home, Path::new(&dir), shared)?);
    tell_login(app, LoginEvent::Waiting { url: None });

    let (on_url, on_end) = (app.clone(), app.clone());
    let (folder, kept) = (dir.clone(), std::sync::Arc::clone(&prepared));
    let started = login::start(
        &dir,
        move |url| tell_login(&on_url, LoginEvent::Waiting { url: Some(url) }),
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

/// The sign-in is done: the account is added, then what Pitwall needs in it is set up (the hook
/// in settings of its own, the radio's entry), and the window told, with the MCP servers a sync
/// would bring over when it is switched to (nothing is copied yet: its switch may still go off).
/// Stopped meanwhile by the user, nothing is added.
fn finish(app: &AppHandle, mut account: ClaudeAccount, prepared: &Prepared) {
    if login::cancelled() {
        prepared.undo(Path::new(&account.dir));
        return;
    }
    let core = &state(app).core;
    let home = home().unwrap_or_default();
    // Shared as made: a folder that had a settings file of its own keeps it, hook and all.
    account.shared = core.shares_main_settings(&account.dir);
    let added = core.change_accounts(|accounts| {
        // Checked again: the list may have changed during the sign-in.
        account.name = check_name(&account.name, accounts, None)?;
        check_dir(&home, &account.dir, accounts, None, true)?;
        account.id = new_id(accounts);
        accounts.others.push(account.clone());
        Ok(())
    });
    if let Err(error) = added {
        prepared.undo(Path::new(&account.dir));
        tell_login(app, LoginEvent::Failed { reason: "error", error: Some(error) });
        return;
    }
    state(app).fuel.accounts_changed(app);

    let hooks = match (account.shared, core.main_hook_installed()) {
        (true, _) => "shared",
        (false, false) => "none",
        (false, true) => match core.account_hook(&account.dir, true) {
            Ok(true) => "installed",
            Ok(false) => "shared",
            Err(_) => "failed",
        },
    };
    let radio = match core.settings().radio.on {
        false => "off",
        true if core.radio_account(&account.dir, true).is_ok() => "added",
        true => "failed",
    };
    let mcp = Some(sync::plan(&home, &account.dir, &listed_projects(app)).report());
    tell_login(app, LoginEvent::Done { id: account.id.clone(), hooks, radio, mcp });
}

/// Stops the sign-in under way; nothing is added.
pub fn login_cancel() {
    login::cancel();
}
