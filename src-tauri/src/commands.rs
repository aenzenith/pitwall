//! Everything the UI may do. The UI never spawns processes or touches files itself.

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::core::{Action, Origin, Snapshot};
use crate::i18n::t;
use crate::settings::{ProjectSettings, Settings, TrackSettings};
use crate::{apply_shortcut, tray, AppState, POPOVER, SWITCHER};
use tauri::Manager;

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Snapshot {
    state.core.snapshot()
}

/// The dev server's output, or a custom command's when `job` is its id.
#[tauri::command]
pub fn get_output(state: State<'_, AppState>, path: String, job: Option<String>) -> Vec<String> {
    match job {
        Some(id) => state.core.command_output(&path, &id),
        None => state.core.output(&path),
    }
}

#[tauri::command]
pub fn run_command(state: State<'_, AppState>, path: String, id: String) {
    let core = state.core.clone();
    std::thread::spawn(move || core.run_command(&path, &id));
}

#[tauri::command]
pub fn stop_command(state: State<'_, AppState>, path: String, id: String) {
    let core = state.core.clone();
    std::thread::spawn(move || core.stop_command(&path, &id));
}

/// A command that started with its server and waits for the user, skipped: nothing runs.
#[tauri::command]
pub fn skip_held(state: State<'_, AppState>, path: String, id: String) {
    state.core.skip_held(&path, &id);
}

/// The address "Open in browser" would open, to show it on the dev server row. Off the main
/// thread, as the project's `.env` is read for it.
#[tauri::command]
pub async fn resolve_address(app: AppHandle, path: String) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || {
        let local = core.snapshot().projects.into_iter().find(|p| p.path == path).and_then(|p| p.url);
        core.resolve_url(&path, local)
    })
    .await
    .ok()
    .flatten()
}

/// Folder picker; the chosen folder becomes a project. It opens in the projects folder when one
/// is set.
#[tauri::command]
pub fn pick_folder(app: AppHandle, state: State<'_, AppState>) {
    let core = state.core.clone();
    let mut dialog = app.dialog().file().set_title(t!("common.addProject"));

    if let Some(dir) = core.settings().projects_dir.filter(|dir| std::path::Path::new(dir).is_dir()) {
        dialog = dialog.set_directory(dir);
    }

    dialog.pick_folder(move |folder| {
        if let Some(path) = folder.and_then(|f| f.into_path().ok()) {
            let _ = core.add_project(&path.to_string_lossy());
        }
    });
}

/// Folder picker for the projects folder the quick switcher falls back to.
#[tauri::command]
pub fn pick_projects_dir(app: AppHandle, state: State<'_, AppState>) {
    let core = state.core.clone();

    app.dialog().file().set_title(t!("core.dialog.projectsFolder")).pick_folder(move |folder| {
        if let Some(path) = folder.and_then(|f| f.into_path().ok()) {
            core.set_projects_dir(Some(path.to_string_lossy().into_owned()));
        }
    });
}

#[tauri::command]
pub fn clear_projects_dir(state: State<'_, AppState>) {
    state.core.set_projects_dir(None);
}

#[tauri::command]
pub fn project_folders(state: State<'_, AppState>) -> Vec<crate::core::Folder> {
    state.core.project_folders()
}

/// Opens one of a project's links; a tab that already shows it comes forward. Only web and mail
/// addresses open.
#[tauri::command]
pub fn open_url(app: AppHandle, state: State<'_, AppState>, url: String) -> Result<(), String> {
    // A refused link leaves the popover where it is.
    if crate::links::normalize(&url).is_some() {
        step_aside(&app);
    }
    state.core.open_url(&url)
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) {
    crate::copy_text(&app, text);
}

/// Addresses the project names itself (git remote, `.env`, `package.json`), offered as links.
#[tauri::command]
pub async fn link_suggestions(app: AppHandle, path: String) -> Vec<crate::links::LinkSuggestion> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || crate::links::suggestions(&path, core.work_dir(&path).as_deref())).await.unwrap_or_default()
}

/// Whether `editor` (as in Settings) has Pitwall for VS Code.
#[tauri::command]
pub fn extension_status(editor: String) -> crate::extension::ExtensionStatus {
    crate::extension::status(&editor)
}

/// Where to get Pitwall for VS Code for `editor`: its page in the editor, or in the browser. An
/// editor's link that nothing here opens (an editor removed since) falls back to the browser.
#[tauri::command]
pub fn open_extension_page(app: AppHandle, editor: String) -> Result<(), String> {
    let page = crate::extension::page(&editor);
    let web = crate::extension::web_page(&editor);
    if app.opener().open_url(&page, None::<&str>).is_ok() || (page != web && app.opener().open_url(web, None::<&str>).is_ok()) {
        return Ok(());
    }
    Err(t!("core.error.extensionPage"))
}

/* ---------- the first run's last step: the editors here, and notifications ---------- */

/// Which of the editors Settings offers are on this machine, in Settings' order.
#[tauri::command]
pub async fn installed_editors() -> Result<Vec<&'static str>, String> {
    tauri::async_runtime::spawn_blocking(crate::extension::installed_editors).await.map_err(|error| error.to_string())
}

/// Whether the system lets Pitwall post notifications, asked of it now: `allowed`, `off`,
/// `notAsked`, `borrowed` (a dev build posts as another app, whose permission it is) or `unknown`.
#[tauri::command]
pub async fn notification_access() -> Result<&'static str, String> {
    tauri::async_runtime::spawn_blocking(crate::notify::access).await.map_err(|error| error.to_string())
}

/// "Send a test": one banner through the door (`Core::test_banner`) once the system lets Pitwall
/// post (macOS asks first when it never had an answer). What it allows then comes back.
#[tauri::command]
pub async fn notification_test(app: AppHandle) -> Result<&'static str, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    let tested = move || {
        if crate::notify::ask() {
            core.test_banner();
        }
        crate::notify::access()
    };
    tauri::async_runtime::spawn_blocking(tested).await.map_err(|error| error.to_string())
}

/// The system's notification settings, at the entry whose permission it is.
#[tauri::command]
pub fn notification_settings(app: AppHandle) -> Result<(), String> {
    let url = crate::notify::settings_url(&app.config().identifier).ok_or("no notification settings here")?;
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}

/// Settings' preview of a notification sound.
#[tauri::command]
pub fn play_sound(app: AppHandle, id: String) {
    crate::play_sound(&app, id);
}

/// The day's timeline (`YYYY-MM-DD`; today without one). It asks every project's git, so it
/// runs off the main thread.
#[tauri::command]
pub async fn day_summary(state: State<'_, AppState>, date: Option<String>) -> Result<crate::core::DaySummary, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.day_summary(date.as_deref())).await.map_err(|error| error.to_string())
}

/// The Sessions page: every Claude session Pitwall knows of. From the first call on, the main
/// window also gets `sessions` (the same) whenever it changes.
#[tauri::command]
pub async fn claude_sessions(app: AppHandle) -> Result<crate::core::SessionsView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.sessions.ask(&app)
    })
    .await
    .map_err(|error| error.to_string())
}

/// The Sessions page looking back: the sessions of an earlier day (`YYYY-MM-DD`) or of every day
/// there is something kept of (`all`). It goes through Claude Code's log folders, so it runs off
/// the main thread.
#[tauri::command]
pub async fn claude_history(app: AppHandle, period: String) -> Result<crate::core::SessionsView, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.sessions_history(&period)).await.map_err(|error| error.to_string())
}

/// The terminal's history: the sessions of a listed project that open again in its terminals. It
/// goes through Claude Code's log folders, so it runs off the main thread.
#[tauri::command]
pub async fn claude_past(app: AppHandle, path: String) -> Result<Vec<crate::core::PastSession>, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.claude_past(&path)).await.map_err(|error| error.to_string())
}

/// A session of the terminal's history brought up where it runs, or opened again in a new tab of
/// the project's terminals once it has ended.
#[tauri::command]
pub async fn resume_claude(app: AppHandle, path: String, session: String) -> Result<crate::core::Resumed, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.resume_claude(&path, &session)).await.map_err(|error| error.to_string())
}

/// What Claude last said in a session, for its details (the Sessions page's, a board card's):
/// read from its log off the main thread, never kept.
#[tauri::command]
pub async fn claude_last_message(app: AppHandle, session: String) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.last_message(&session)).await.ok().flatten()
}

/* ---------- project terminals ---------- */

/// `start`: for a Claude tab, the model and the mode chosen from "New Claude"'s list; without
/// it, the project's own. `account`: for a Claude tab, the Claude account it runs with ("Start
/// with account ›"); without it, the one in use. `shell`: on Windows, the shell of the list a shell tab runs, by its
/// word (`Shell::parse`; any other is none, Command Prompt), never a path or a command line;
/// `elevated`, an administrator's, which waits for Windows' permission: off the main thread.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn open_terminal(
    app: AppHandle,
    path: String,
    claude: Option<bool>,
    cols: Option<u16>,
    rows: Option<u16>,
    start: Option<StartChoice>,
    shell: Option<String>,
    elevated: Option<bool>,
    account: Option<String>,
) -> Result<crate::core::TerminalOpened, String> {
    use crate::core::TerminalOpened;

    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || match (claude.unwrap_or(false), start) {
        (true, start) => core.open_claude_terminal_with(&path, start.as_ref().map(StartChoice::gated), cols.zip(rows), account.as_deref()).map(|view| TerminalOpened::Tab { view }),
        (false, _) => core.open_shell(&path, shell.as_deref().and_then(crate::core::Shell::parse), elevated.unwrap_or(false), cols.zip(rows)),
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The shells the terminal panel's "+" offers on Windows, and where an administrator's opens:
/// asked of the system now, nothing kept. None elsewhere.
#[tauri::command]
pub async fn terminal_shells() -> Result<crate::core::TerminalShells, String> {
    tauri::async_runtime::spawn_blocking(crate::core::installed_shells).await.map_err(|error| error.to_string())
}

/// Windows' settings for sudo (System › For developers), from the panel's line about an
/// administrator's shell that opened in a window of its own: that page, and nothing else.
#[tauri::command]
pub fn open_sudo_settings(app: AppHandle) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("no sudo settings here".into());
    }
    app.opener().open_url("ms-settings:developers", None::<&str>).map_err(|error| error.to_string())
}

/// The terminal panel said that the last quit's or update's administrator's tabs weren't put back.
#[tauri::command]
pub fn admin_not_restored_seen(state: State<'_, AppState>) {
    state.core.admin_not_restored_seen();
}

#[tauri::command]
pub fn rename_terminal(state: State<'_, AppState>, id: u64, name: String) {
    state.core.rename_terminal(id, &name);
}

/// The terminal panel's height for its panes, kept for the next start.
#[tauri::command]
pub fn set_panes_height(state: State<'_, AppState>, height: u32) {
    state.core.set_panes_height(height);
}

/// How the panel lays a project's terminals out; none: one at a time.
#[tauri::command]
pub fn set_terminal_layout(state: State<'_, AppState>, path: String, layout: Option<crate::core::TerminalLayout>) {
    state.core.set_terminal_layout(&path, layout);
}

#[tauri::command]
pub fn write_terminal(state: State<'_, AppState>, id: u64, data: String) {
    state.core.write_terminal(id, &data);
}

#[tauri::command]
pub fn resize_terminal(state: State<'_, AppState>, id: u64, cols: u16, rows: u16) {
    state.core.resize_terminal(id, cols, rows);
}

#[tauri::command]
pub fn close_terminal(state: State<'_, AppState>, id: u64) {
    state.core.close_terminal(id);
}

#[tauri::command]
pub fn terminal_buffer(state: State<'_, AppState>, id: u64) -> crate::core::TerminalBuffer {
    state.core.terminal_buffer(id)
}

/* ---------- the board ---------- */

#[tauri::command]
pub fn board_state(state: State<'_, AppState>) -> Vec<crate::core::Card> {
    state.core.board_state()
}

/// `images`: those pasted into the note, each a `data:` URL. Their files are written, so it runs
/// off the main thread.
#[tauri::command]
pub async fn board_add(app: AppHandle, path: String, title: String, note: String, images: Vec<crate::core::NewImage>) -> Result<crate::core::Card, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.board_add(&path, &title, &note, &images)).await.map_err(|error| error.to_string())?
}

/// `images`: those pasted into the note since the card was saved.
#[tauri::command]
pub async fn board_edit(app: AppHandle, id: String, title: String, note: String, images: Vec<crate::core::NewImage>) -> Result<(), String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.board_edit(&id, &title, &note, &images)).await.map_err(|error| error.to_string())?
}

/// One of a card's images (`[Image #n]` in its note) as a `data:` URL.
#[tauri::command]
pub async fn board_image(app: AppHandle, id: String, n: u32) -> Result<String, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.board_image(&id, n)).await.map_err(|error| error.to_string())?
}

/// `index`: the card's place among `column`'s cards, itself left out.
#[tauri::command]
pub fn board_move(state: State<'_, AppState>, id: String, column: String, index: usize) -> Result<(), String> {
    state.core.board_move(&id, &column, index)
}

#[tauri::command]
pub fn board_delete(state: State<'_, AppState>, id: String) {
    state.core.board_delete(&id);
}

/// Gives a card to Claude. `mode` is `new` or `plan` (a new terminal tab of its project), or
/// `continue`: the session it holds goes on, and no tab opens for one that still runs. It
/// starts a shell and reads the process table, so it runs off the main thread. `start`: the
/// model and the mode chosen for a new session; without it, the project's own.
#[tauri::command]
pub async fn board_give(app: AppHandle, id: String, mode: String, cols: Option<u16>, rows: Option<u16>, start: Option<StartChoice>) -> Result<Option<crate::core::TerminalView>, String> {
    let give = crate::core::Give::parse(&mode).ok_or_else(|| format!("unknown mode: {mode}"))?;
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    let start = start.map(|start| start.gated());
    tauri::async_runtime::spawn_blocking(move || core.board_give_with(&id, give, start, cols.zip(rows))).await.map_err(|error| error.to_string())?
}

/// Every card of the folder `from` goes to the listed project `to`.
#[tauri::command]
pub fn board_rehome(state: State<'_, AppState>, from: String, to: String) -> Result<(), String> {
    state.core.board_rehome(&from, &to)
}

/// Hands a card to a Claude session that already runs.
#[tauri::command]
pub async fn board_link(app: AppHandle, id: String, session: String) -> Result<(), String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.board_link(&id, &session)).await.map_err(|error| error.to_string())?
}

/// A folder of the projects folder added from the quick switcher: listed, not starred.
#[tauri::command]
pub fn add_project(state: State<'_, AppState>, path: String) -> Result<(), String> {
    state.core.add_unstarred(&path)
}

#[tauri::command]
pub fn set_favourite(state: State<'_, AppState>, path: String, on: bool) {
    state.core.set_favourite(&path, on);
}

/// "Remove from Pitwall" on a row.
#[tauri::command]
pub fn remove_project(state: State<'_, AppState>, path: String) {
    state.core.remove_project(&path);
}

#[tauri::command]
pub fn act(state: State<'_, AppState>, path: String, action: String) -> Result<(), String> {
    let action = Action::parse(&action).ok_or_else(|| format!("unknown action: {action}"))?;
    state.core.act(&path, action, Origin::User);
    Ok(())
}

/// Off the main thread, as the project's `.env` is read for the address.
#[tauri::command]
pub fn open_browser(app: AppHandle, state: State<'_, AppState>, path: String) {
    step_aside(&app);
    let core = state.core.clone();
    std::thread::spawn(move || core.open_browser(&path));
}

#[tauri::command]
pub fn open_editor(app: AppHandle, state: State<'_, AppState>, path: String) {
    step_aside(&app);
    state.core.open_editor(&path);
}

/// A project whose Claude waits on you, opened from the popover or the switcher: its waiting
/// session comes up where it runs (`Core::open_claude`).
#[tauri::command]
pub fn open_claude(app: AppHandle, state: State<'_, AppState>, path: String) {
    step_aside(&app);
    state.core.open_claude(&path);
}

/// Brings a Claude session up where it runs: `path` is its project, or the folder it runs in when
/// that isn't listed.
#[tauri::command]
pub fn reveal_claude(app: AppHandle, state: State<'_, AppState>, path: String, session: String) {
    step_aside(&app);
    state.core.reveal_claude(&path, &session);
}

#[tauri::command]
pub fn mark_seen(state: State<'_, AppState>, path: String) {
    state.core.mark_seen(&path);
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    let before = state.core.settings();

    if settings.launch_at_login != before.launch_at_login {
        if settings.launch_at_login && !crate::is_bundled() {
            return Err(t!("core.error.launchAtLoginDev"));
        }

        let autostart = app.autolaunch();
        let result = if settings.launch_at_login { autostart.enable() } else { autostart.disable() };
        result.map_err(|error| error.to_string())?;
    }

    if settings.shortcut != before.shortcut {
        apply_shortcut(&app, settings.shortcut, &before.shortcut_keys)?;
    }

    let theme = (settings.theme != before.theme).then(|| settings.theme.clone());
    let suggest = settings.suggest_projects != before.suggest_projects;

    // What has a command of its own stays as the core holds it, whatever the form says.
    state.core.set_settings_form(settings);

    if let Some(theme) = theme {
        crate::apply_theme(&app, &theme);
    }
    if suggest {
        // It reads the projects folder: off the caller's thread.
        let core = state.core.clone();
        std::thread::spawn(move || core.refresh_suggestions());
    }
    Ok(())
}

/// New quick-switcher keys. If they can't be registered (another app holds them, or the mini
/// tower's shortcut does) the old ones come back. Either way the mini tower's shortcut, dropped
/// while the keys were recorded, is registered again.
#[tauri::command]
pub fn set_shortcut(app: AppHandle, state: State<'_, AppState>, keys: String) -> Result<(), String> {
    let taken = crate::shortcuts::take(&app, crate::shortcuts::Which::Switcher, &keys, &state.core.settings());
    if taken.is_ok() {
        state.core.set_shortcut_keys(keys);
    }
    crate::shortcuts::resume(&app, &state.core);
    taken
}

/// The quick switcher, from the popover's search button. Async, as is every command that can make
/// a window: made from a synchronous command, WebView2 deadlocks (wry#583).
#[tauri::command]
pub async fn show_switcher(app: AppHandle) {
    tray::toggle_switcher(&app);
}

/// While a new shortcut is being recorded, the current ones must not fire: neither the quick
/// switcher's nor the mini tower's, whichever is being recorded.
#[tauri::command]
pub fn suspend_shortcut(app: AppHandle) {
    crate::shortcuts::suspend(&app);
}

#[tauri::command]
pub fn resume_shortcut(app: AppHandle, state: State<'_, AppState>) {
    crate::shortcuts::resume(&app, &state.core);
}

#[tauri::command]
pub fn reorder(state: State<'_, AppState>, paths: Vec<String>) {
    state.core.reorder(paths);
}

/// The board picked on the Board page: `all`, or a project's path.
#[tauri::command]
pub fn set_board_scope(state: State<'_, AppState>, scope: String) {
    state.core.set_board_scope(scope);
}

/// How the Board page draws a project's board: `columns` or `list`.
#[tauri::command]
pub fn set_board_view(state: State<'_, AppState>, view: String) {
    state.core.set_board_view(view);
}

/// The list's "Only favourites and open", kept until it is turned off.
#[tauri::command]
pub fn set_favourites_only(state: State<'_, AppState>, on: bool) {
    state.core.set_favourites_only(on);
}

/// Whether the Files window shows a Markdown or SVG file as its preview, kept until it is turned off.
#[tauri::command]
pub fn set_files_preview(state: State<'_, AppState>, on: bool) {
    state.core.set_files_preview(on);
}

/// The Track page's own settings: its circuit, what the cars carry, how much moves.
#[tauri::command]
pub fn set_track(state: State<'_, AppState>, track: TrackSettings) {
    state.core.set_track(track);
}

#[tauri::command]
pub fn set_project_settings(state: State<'_, AppState>, path: String, settings: ProjectSettings) {
    state.core.set_project_settings(&path, settings);
}

/// The folder typed for the project at `path` to run in, as its settings keep it
/// (`ProjectSettings::folder`; `""` for the project's own), or why it can't be.
#[tauri::command]
pub async fn work_folder(path: String, folder: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || work_folder_within(&path, &folder)).await.map_err(|error| error.to_string())?
}

/// Asks for that folder, the project's own shown first: as `work_folder` gives it, or None
/// when the dialog was cancelled.
#[tauri::command]
pub async fn pick_work_folder(app: AppHandle, path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file().set_title(t!("core.dialog.workFolder")).set_directory(&path);
        let Some(picked) = dialog.blocking_pick_folder().and_then(|folder| folder.into_path().ok()) else {
            return Ok(None);
        };
        work_folder_within(&path, &picked.to_string_lossy()).map(Some)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn work_folder_within(path: &str, folder: &str) -> Result<String, String> {
    match crate::settings::folder_within(std::path::Path::new(path), folder) {
        Ok(folder) => Ok(folder.unwrap_or_default()),
        Err(crate::settings::FolderRefused::Missing) => Err(t!("core.error.workFolderMissing", folder = folder.trim())),
        Err(crate::settings::FolderRefused::Outside) => Err(t!("core.error.workFolderOutside")),
    }
}

#[tauri::command]
pub async fn open_window(app: AppHandle) {
    tray::show_main(&app);
}

/// A window's page is up and listening; a window made on first use comes up now.
#[tauri::command]
pub fn window_ready(window: tauri::WebviewWindow) {
    crate::windows::ready(window.app_handle(), window.label());
}

/// The Fuel page: Claude's plan limits and today's tokens, as last read.
#[tauri::command]
pub fn fuel_state(state: State<'_, AppState>) -> crate::fuel::FuelView {
    state.fuel.view()
}

/// The Fuel page opened, or its refresh button: read again (`force` skips Settings' wait, not
/// the minute between two requests).
#[tauri::command]
pub fn refresh_fuel(app: AppHandle, state: State<'_, AppState>, force: bool) {
    state.fuel.refresh(&app, force, state.core.settings().fuel_every_ms());
}

/* ---------- Claude's accounts ---------- */

/// The account the Claude sessions Pitwall starts use from now on (`accounts::MAIN` for the main
/// one).
#[tauri::command]
pub fn account_use(app: AppHandle, id: String) -> Result<(), String> {
    crate::accounts::app::use_account(&app, &id)
}

#[tauri::command]
pub fn account_rename(app: AppHandle, id: String, name: String) -> Result<(), String> {
    crate::accounts::app::rename(&app, &id, &name)
}

/// Takes an account off the list; Claude Code's CLI takes the radio's entry out, so off the main
/// thread.
#[tauri::command]
pub async fn account_remove(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::accounts::app::remove(&app, &id)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn account_set_sync(app: AppHandle, id: String, on: bool) -> Result<(), String> {
    crate::accounts::app::set_sync(&app, &id, on)
}

#[tauri::command]
pub fn account_reveal(app: AppHandle, id: String) -> Result<(), String> {
    crate::accounts::app::reveal(&app, &id)
}

/// The folder a new account of this name gets.
#[tauri::command]
pub fn account_folder(app: AppHandle, name: String) -> Result<crate::accounts::app::NewFolder, String> {
    crate::accounts::app::folder(&app, &name)
}

/// A folder picked for an account: the system's dialog waits for the user, so off the main thread.
#[tauri::command]
pub async fn account_pick_folder(app: AppHandle) -> Result<Option<crate::accounts::app::PickedFolder>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::accounts::app::pick_folder(&app)).await.map_err(|error| error.to_string())?
}

/// Starts adding an account; `account-login` tells how it goes.
#[tauri::command]
pub fn account_login(app: AppHandle, name: String, dir: Option<String>, shared: bool, sync: bool) -> Result<(), String> {
    crate::accounts::app::login(&app, &name, dir, shared, sync)
}

#[tauri::command]
pub fn account_login_cancel() {
    crate::accounts::app::login_cancel();
}

/// An account's MCP servers brought level with the main account's now, through Claude Code's CLI.
#[tauri::command]
pub async fn account_sync(app: AppHandle, id: String) -> Result<crate::accounts::sync::SyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::accounts::app::sync_now(&app, &id)).await.map_err(|error| error.to_string())?
}

/// What a sync would do, writing nothing.
#[tauri::command]
pub async fn account_sync_plan(app: AppHandle, id: String) -> Result<crate::accounts::sync::SyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::accounts::app::sync_plan(&app, &id)).await.map_err(|error| error.to_string())?
}

/* ---------- dependencies ---------- */

/// The Dependencies page: every listed project's report, in list order. From then on the main
/// window also gets `deps` (the same) whenever it changes. A project not checked yet is checked
/// first, so it runs off the main thread.
#[tauri::command]
pub async fn deps_state(state: State<'_, AppState>) -> Result<Vec<crate::deps::DepReport>, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.deps_state()).await.map_err(|error| error.to_string())
}

/// Checks one project (`path`) or every listed one again, with the login shell asked anew for
/// the runtimes' versions and the managers' executables; resolves once done (the reports come
/// as `deps`). A project's pending migrations are read again too (its migration tools' status
/// commands) on a thread of their own: `running` in its `migrations` till then.
#[tauri::command]
pub async fn deps_check(state: State<'_, AppState>, path: Option<String>) -> Result<(), String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.deps_check(path.as_deref())).await.map_err(|error| error.to_string())
}

/// Installs a project's packages of one ecosystem (`npm`, `composer`: the check's
/// `installCommand`) as its job `deps:<ecosystem>`; the output is that job's (`get_output`),
/// stopped with `stop_command`.
#[tauri::command]
pub async fn deps_install(state: State<'_, AppState>, path: String, ecosystem: String) -> Result<(), String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.deps_install(&path, &ecosystem)).await.map_err(|error| error.to_string())?
}

/// A migration tool's forward-only migrate command, exactly (`laravel`: `php artisan migrate`;
/// the report's `migrations[].command`), as the job `deps:migrate:<tool>`: only from the user's
/// confirm.
#[tauri::command]
pub async fn deps_migrate(state: State<'_, AppState>, path: String, tool: String) -> Result<(), String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.deps_migrate(&path, &tool)).await.map_err(|error| error.to_string())?
}

/// The network scan (outdated, vulnerable) of one ecosystem of a project; its `running` shows
/// in the reports until it ends.
#[tauri::command]
pub async fn deps_scan(state: State<'_, AppState>, path: String, ecosystem: String) -> Result<(), String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.deps_scan(&path, &ecosystem)).await.map_err(|error| error.to_string())?
}

/// What a project's last scan of one ecosystem listed: the outdated packages and the advisories
/// its counts are of. `null` for a scan kept from before they were listed.
#[tauri::command]
pub async fn deps_scan_details(state: State<'_, AppState>, path: String, ecosystem: String) -> Result<Option<crate::deps::ScanDetails>, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.deps_scan_details(&path, &ecosystem)).await.map_err(|error| error.to_string())
}

/* ---------- teams ---------- */

/// A new team, last in the list, with these projects (each leaves the team it was in).
#[tauri::command]
pub fn team_create(state: State<'_, AppState>, name: String, projects: Vec<String>) -> Result<crate::settings::Team, String> {
    state.core.team_create(&name, projects)
}

#[tauri::command]
pub fn team_rename(state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    state.core.team_rename(&id, &name)
}

/// The team goes; its projects stay listed, in no team.
#[tauri::command]
pub fn team_delete(state: State<'_, AppState>, id: String) {
    state.core.team_delete(&id);
}

/// The team's projects, in order: a reorder, one added or one dropped.
#[tauri::command]
pub fn team_projects(state: State<'_, AppState>, id: String, projects: Vec<String>) {
    state.core.team_projects(&id, projects);
}

/// The teams' own order.
#[tauri::command]
pub fn team_order(state: State<'_, AppState>, ids: Vec<String>) {
    state.core.team_order(ids);
}

#[tauri::command]
pub fn team_collapse(state: State<'_, AppState>, id: String, collapsed: bool) {
    state.core.team_collapse(&id, collapsed);
}

/// Whether the team's servers start when Pitwall does.
#[tauri::command]
pub fn team_launch(state: State<'_, AppState>, id: String, on: bool) {
    state.core.team_launch(&id, on);
}

/// Starts the team's projects that don't run, one after the other.
#[tauri::command]
pub fn team_start(state: State<'_, AppState>, id: String) {
    state.core.team_start(&id);
}

#[tauri::command]
pub fn team_stop(state: State<'_, AppState>, id: String) {
    state.core.team_stop(&id);
}

/* ---------- the first run and the projects folder ---------- */

/// The projects folder's subfolders that have the dev script. It reads every one's
/// `package.json`, so it runs off the main thread.
#[tauri::command]
pub async fn discover_projects(state: State<'_, AppState>) -> Result<crate::core::Discovery, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.discover_projects()).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn add_projects(state: State<'_, AppState>, paths: Vec<String>) -> Result<(), String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.add_projects(&paths)).await.map_err(|error| error.to_string())
}

/// A project offered under the list isn't offered again.
#[tauri::command]
pub fn dismiss_suggestion(state: State<'_, AppState>, path: String) {
    state.core.dismiss_suggestion(&path);
}

/// The first run's steps are over: finished or put off.
#[tauri::command]
pub fn set_onboarded(state: State<'_, AppState>) {
    state.core.set_onboarded();
}

/* ---------- what's new, and what was open at the last quit ---------- */

/// This version's notes (`src/whats-new.json`), in the language spoken.
#[tauri::command]
pub fn whats_new(state: State<'_, AppState>) -> crate::whatsnew::WhatsNew {
    state.core.whats_new()
}

#[tauri::command]
pub fn whats_new_seen(state: State<'_, AppState>) {
    state.core.whats_new_seen();
}

/// Puts back what was open at the last quit (`always`: from now on without asking). It opens
/// shells, so it runs off the main thread.
#[tauri::command]
pub async fn resume_restore(app: AppHandle, always: bool) -> Result<(), String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.resume_restore(always)).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resume_dismiss(state: State<'_, AppState>) {
    state.core.resume_dismiss();
}

/* ---------- backup, and projects whose folder moved ---------- */

/// Asks where to save and writes a backup of the chosen parts there. The file's path, or None
/// when the dialog was cancelled. The dialog waits for the user, so it runs off the main thread.
#[tauri::command]
pub async fn backup_export(app: AppHandle, parts: crate::core::BackupParts) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut dialog = app
            .dialog()
            .file()
            .set_title(t!("core.dialog.backupSave"))
            .set_file_name(format!("Pitwall {}.pitwall-backup", crate::clock::day_name(crate::registry::now_ms())))
            .add_filter(t!("core.dialog.backupFilter"), &["pitwall-backup"]);
        if let Some(downloads) = dirs::download_dir().filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(downloads);
        }

        let Some(file) = dialog.blocking_save_file().and_then(|file| file.into_path().ok()) else {
            return Ok(None);
        };
        state.core.backup_write(&file, parts)?;
        Ok(Some(file.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Asks for a backup file and says what it holds; None when the dialog was cancelled. The file
/// is the one `backup_import` reads: the page never names a path.
#[tauri::command]
pub async fn backup_pick(app: AppHandle) -> Result<Option<crate::core::BackupInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let picked = app.dialog().file().set_title(t!("core.dialog.backupOpen")).add_filter(t!("core.dialog.backupFilter"), &["pitwall-backup", "json"]).blocking_pick_file();
        let Some(file) = picked.and_then(|file| file.into_path().ok()) else {
            return Ok(None);
        };

        let info = state.core.backup_read(&file)?;
        *state.backup.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(file);
        Ok(Some(info))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Puts the chosen parts of the file picked last back (`core/backup.rs`). Imported settings take
/// hold at once: the shortcut is registered again and the theme drawn.
#[tauri::command]
pub async fn backup_import(app: AppHandle, parts: crate::core::BackupParts) -> Result<crate::core::ImportReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let picked = || state.backup.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let file = picked().clone().ok_or_else(|| t!("core.error.backupNoFile"))?;
        // A file that failed stays the one picked, for another try.
        let report = state.core.backup_import(&file, parts)?;
        picked().take();

        if report.settings {
            let settings = state.core.settings();
            // Keys another machine could register may be taken here: the switcher then has none
            // until new ones are recorded in Settings. Should they be the mini tower's keys here,
            // the switcher has them and the tower's shortcut is switched off (`shortcuts::resume`).
            crate::shortcuts::resume(&app, &state.core);
            crate::apply_theme(&app, &settings.theme);
            // The projects folder may be another one now.
            state.core.projects_dir_changed();
        } else if report.projects > 0 {
            // The projects it listed are no longer offered under the list.
            state.core.refresh_suggestions();
        }
        Ok(report)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Projects whose folder is gone, with what is still kept for them.
#[tauri::command]
pub async fn missing_projects(state: State<'_, AppState>) -> Result<Vec<crate::core::MissingProject>, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.missing_projects()).await.map_err(|error| error.to_string())
}

/// Asks for the folder a project is in now and hands it everything kept for `from`. The new
/// path, or None when the dialog was cancelled.
#[tauri::command]
pub async fn rehome_project(app: AppHandle, from: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut dialog = app.dialog().file().set_title(t!("core.dialog.rehome"));
        if let Some(dir) = state.core.settings().projects_dir.filter(|dir| std::path::Path::new(dir).is_dir()) {
            dialog = dialog.set_directory(dir);
        }

        let Some(to) = dialog.blocking_pick_folder().and_then(|folder| folder.into_path().ok()) else {
            return Ok(None);
        };
        let to = to.to_string_lossy().into_owned();
        state.core.rehome_project(&from, &to)?;
        Ok(Some(to))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Drops what is kept for a project whose folder is gone (its cards stay on the board).
#[tauri::command]
pub fn forget_project(state: State<'_, AppState>, path: String) -> Result<(), String> {
    state.core.forget_project(&path)
}

/// Esc in the popover: close it and go back to the app from before.
#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    tray::hide_popover(&app);
}

/// Esc in the switcher: close it and go back to the app from before.
#[tauri::command]
pub fn hide_switcher(app: AppHandle) {
    tray::dismiss_switcher(&app);
}

/// Before opening something (editor, Claude, a link): the popover and switcher get out of the
/// way. While Pitwall is the active app they go when the opened app takes focus, since hidden
/// right away Pitwall's own window would come up for a moment. When it isn't (both are panels
/// that leave the app you are in in front), they go at once rather than linger while that app
/// starts.
fn step_aside(app: &AppHandle) {
    let wait = tray::pitwall_active();
    for label in [POPOVER, SWITCHER] {
        if let Some(window) = app.get_webview_window(label) {
            if window.is_visible().unwrap_or(false) {
                if wait {
                    tray::hide_once_focus_moves(window, std::time::Duration::from_millis(1500));
                } else {
                    let _ = crate::windows::hide(&window);
                }
            }
        }
    }
}

/// Adds the Notification hook to `~/.claude/settings.json` (backed up first).
#[tauri::command]
pub fn install_claude_hook(state: State<'_, AppState>) -> Result<(), String> {
    state.core.install_claude_hook()
}

#[tauri::command]
pub fn uninstall_claude_hook(state: State<'_, AppState>) -> Result<(), String> {
    state.core.uninstall_claude_hook()
}

/* ---------- Pitwall's own update ---------- */

#[tauri::command]
pub fn update_state(state: State<'_, AppState>) -> crate::update::UpdateView {
    state.update.view()
}

/// "Check now" in Settings; the answer arrives as `update` events.
#[tauri::command]
pub fn update_check(app: AppHandle, state: State<'_, AppState>) {
    state.update.check(&app);
}

/// What a restart for the update would cut off and bring back, for the dialog that asks. It
/// reads the process table, so it runs off the main thread.
#[tauri::command]
pub async fn update_plan(app: AppHandle) -> Result<crate::core::RestartPlan, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.restart_plan()).await.map_err(|error| error.to_string())
}

/// Installs the waiting update and restarts. Answers only when that failed, with the step.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.update.install(&app).map_err(str::to_string)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Opens one of the About links in the browser. The UI names a link, never an address.
#[tauri::command]
pub fn open_link(app: AppHandle, link: String) -> Result<(), String> {
    let url = match link.as_str() {
        "site" => "https://aenzenith.com",
        "coffee" => "https://buymeacoffee.com/aenzenith",
        "releases" => "https://github.com/aenzenith/pitwall/releases/latest",
        _ => return Err(format!("Unknown link: {link}")),
    };
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}

/// The popover's quit button. With something still open it brings the window up with the
/// chequered flag's dialog first, while Settings says so (`flag::request_quit`).
#[tauri::command]
pub fn quit(app: AppHandle) {
    crate::flag::request_quit(&app);
}

/* ---------- the phone's channel (core/relay.rs) ---------- */

/// The channel as Settings shows it. The Telegram token never comes along: only whether one is
/// stored.
#[tauri::command]
pub fn phone_config(state: State<'_, AppState>) -> crate::core::PhoneView {
    state.core.phone_view()
}

/// Settings' form, kept. An empty Telegram token keeps the stored one.
#[tauri::command]
pub fn phone_set(state: State<'_, AppState>, config: crate::core::PhoneConfig) -> Result<crate::core::PhoneView, String> {
    state.core.phone_set(config)
}

/// "Send a test": answers once the channel did, with the error's text when it refused or
/// wasn't reached.
#[tauri::command]
pub async fn phone_test(app: AppHandle) -> Result<(), String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.phone_test()).await.map_err(|error| error.to_string())?
}

/// A fresh ntfy topic, made here.
#[tauri::command]
pub fn phone_topic(state: State<'_, AppState>) -> String {
    state.core.phone_topic()
}

/* ---------- notifications: quiet, what was missed, muted projects ---------- */

/// Quiet from now on: `hour` (for an hour), `morning` (until tomorrow morning) or `open` (until
/// it is switched off).
#[tauri::command]
pub fn quiet_start(state: State<'_, AppState>, span: String) -> Result<(), String> {
    let span = crate::core::QuietFor::parse(&span).ok_or_else(|| format!("unknown span: {span}"))?;
    state.core.quiet_start(span);
    Ok(())
}

#[tauri::command]
pub fn quiet_end(state: State<'_, AppState>) {
    state.core.quiet_end();
}

/// One of quiet's options: `questions` (questions and permissions still come through) or
/// `sounds` (the sounds are held too).
#[tauri::command]
pub fn quiet_option(state: State<'_, AppState>, option: String, on: bool) -> Result<(), String> {
    state.core.set_quiet_option(&option, on)
}

/// The quiet hours: on or off, and from when to when (`HH:MM`).
#[tauri::command]
pub fn quiet_hours(state: State<'_, AppState>, on: bool, from: String, to: String) -> Result<(), String> {
    state.core.set_quiet_hours(on, &from, &to)
}

/// Whether an event brings a notification: `claude`, `serverCrashed`, `command`, `longTurn`,
/// `deps` or `fuel`.
#[tauri::command]
pub fn notify_event(state: State<'_, AppState>, event: String, on: bool) -> Result<(), String> {
    state.core.set_notification(&event, on)
}

#[tauri::command]
pub fn mute_project(state: State<'_, AppState>, path: String, muted: bool) {
    state.core.mute_project(&path, muted);
}

/// A line of the popover's list of what was missed: what a click on its notification would
/// bring up, and the line goes.
#[tauri::command]
pub fn missed_open(app: AppHandle, state: State<'_, AppState>, path: String, kind: crate::core::SignalKind) {
    // Claude's come up where they run; the rest in Pitwall's window, which hides the popover.
    if kind.is_claudes() {
        step_aside(&app);
    }
    state.core.missed_open(&path, kind);
}

#[tauri::command]
pub fn missed_clear(state: State<'_, AppState>) {
    state.core.missed_clear();
}

/* ---------- the team radio: Pitwall as Claude Code's MCP server ---------- */

/// The tools there are, for their rows in Settings: the core's own list, answered at once.
#[tauri::command]
pub fn radio_tools() -> Vec<crate::core::RadioTool> {
    crate::core::radio_tools()
}

/// The radio as Claude Code has it. It asks Claude Code's CLI, so it runs off the main thread.
#[tauri::command]
pub async fn radio_state(app: AppHandle) -> Result<crate::core::RadioState, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.radio_state()).await.map_err(|error| error.to_string())
}

/// Turns the radio on (Pitwall is added to Claude Code's MCP servers) or off (it is taken out).
#[tauri::command]
pub async fn radio_set(app: AppHandle, on: bool) -> Result<crate::core::RadioState, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.radio_set(on)).await.map_err(|error| error.to_string())?
}

/// One tool's switch.
#[tauri::command]
pub fn radio_tool(state: State<'_, AppState>, name: String, on: bool) -> Result<(), String> {
    state.core.radio_tool(&name, on)
}

/// Today's calls of a session over the radio, the newest first.
#[tauri::command]
pub fn radio_calls(state: State<'_, AppState>, session: String) -> Vec<crate::core::RadioCall> {
    state.core.radio_calls(&session)
}

/* ---------- the output pane ---------- */

/// Opens the file an output line names (`file`, as it was printed) in the editor at `line`:
/// only a file of the project at `path` (`Core::open_reference`). False when there is none.
/// `own_folder`: the Files window's, a path from the project's own folder. It may look through
/// the project's folder, so it runs off the main thread.
#[tauri::command]
pub async fn open_reference(app: AppHandle, path: String, file: String, line: u32, column: Option<u32>, own_folder: bool) -> bool {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.open_reference(&path, &file, line, column, own_folder)).await.unwrap_or(false)
}

/* ---------- the Files window: a listed project's folders and files, read only (core/explorer.rs) ---------- */

/// The folder `dir` of the project at `path` (`""` its root), at most `limit` entries.
#[tauri::command]
pub async fn explorer_list(app: AppHandle, path: String, dir: String, limit: usize) -> Option<crate::core::Listing> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_list(&path, &dir, limit)).await.ok().flatten()
}

/// A file of the project, as the window shows it: its text, or why not.
#[tauri::command]
pub async fn explorer_read(app: AppHandle, path: String, file: String) -> Option<crate::core::FileView> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_read(&path, &file)).await.ok().flatten()
}

/// A picture a Markdown file names, for its preview: a data URL, or nothing (`core/explorer.rs`).
#[tauri::command]
pub async fn explorer_image(app: AppHandle, path: String, file: String) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_image(&path, &file)).await.ok().flatten()
}

/// The Git state of the project's paths; nothing when it isn't a Git work tree.
#[tauri::command]
pub async fn explorer_status(app: AppHandle, path: String) -> Option<Vec<crate::core::GitMark>> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_status(&path)).await.ok().flatten()
}

/// What changed in a file of the project since the last commit.
#[tauri::command]
pub async fn explorer_diff(app: AppHandle, path: String, file: String) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_diff(&path, &file)).await.ok().flatten()
}

/// The project's files for the window's search.
#[tauri::command]
pub async fn explorer_files(app: AppHandle, path: String) -> Option<crate::core::FileIndex> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_files(&path)).await.ok().flatten()
}

/// The history of the project's branch and its upstream, `limit` commits after `skip`.
#[tauri::command]
pub async fn explorer_log(app: AppHandle, path: String, skip: usize, limit: usize) -> Option<crate::core::History> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_log(&path, skip, limit)).await.ok().flatten()
}

/// The files a commit of the project changed.
#[tauri::command]
pub async fn explorer_commit(app: AppHandle, path: String, id: String) -> Option<Vec<crate::core::CommitFile>> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_commit(&path, &id)).await.ok().flatten()
}

/// What a commit of the project changed in one file (and, renamed, where it came from).
#[tauri::command]
pub async fn explorer_commit_diff(app: AppHandle, path: String, id: String, file: String, from: Option<String>) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.explorer_commit_diff(&path, &id, &file, from.as_deref())).await.ok().flatten()
}

/// A file or folder of the project shown in Finder (Explorer, the file manager).
#[tauri::command]
pub async fn explorer_reveal(app: AppHandle, path: String, file: String) -> bool {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    let Ok(Some(found)) = tauri::async_runtime::spawn_blocking(move || core.explorer_path(&path, &file)).await else {
        return false;
    };
    tauri_plugin_opener::reveal_item_in_dir(found).is_ok()
}

/* ---------- the chequered flag: what is still open (core/flag.rs, flag.rs) ---------- */

/// What is still open, and whether a quit waits on the dialog's answer. `fresh`: the projects'
/// Git state is read again first (as the dialog opens), so it runs off the main thread.
#[tauri::command]
pub async fn flag_state(app: AppHandle, fresh: bool) -> Result<crate::core::FlagState, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || {
        if fresh {
            core.flag_refresh(crate::flag::FRESH);
        }
        core.flag_state()
    })
    .await
    .map_err(|error| error.to_string())
}

/// Whether a quit from Pitwall's own menus shows what is still open first: Settings › General's
/// switch and the dialog's box.
#[tauri::command]
pub fn flag_set(state: State<'_, AppState>, on: bool) {
    state.core.set_flag_at_quit(on);
}

/// The dialog is up for the quit that was held.
#[tauri::command]
pub fn flag_shown() {
    crate::flag::shown();
}

/// "Cancel" on a held quit (Esc, a click outside, the dialog gone): Pitwall stays, and the next
/// quit asks again.
#[tauri::command]
pub fn flag_keep(state: State<'_, AppState>) {
    state.core.quit_dropped();
}

/// "Quit anyway": the quit that was held goes on.
#[tauri::command]
pub fn flag_quit(app: AppHandle) {
    app.exit(0);
}

/// "Stop all" in the dialog: every server and every running command.
#[tauri::command]
pub fn flag_stop_all(state: State<'_, AppState>) {
    state.core.flag_stop_all();
}

/// A listed project shown in the window's list, for the new shell tab its terminal panel then
/// opens. Async, as every command that brings a window up.
#[tauri::command]
pub async fn flag_terminal(app: AppHandle, path: String) -> Result<(), String> {
    app.state::<AppState>().core.flag_terminal(&path)
}

/// The window's Board page. Async, as every command that brings a window up.
#[tauri::command]
pub async fn flag_board(app: AppHandle) {
    app.state::<AppState>().core.flag_board();
}

/* ---------- the mini tower (tower.rs) ---------- */

/// Shows or hides the mini tower. Async, as is every command that can make a window.
#[tauri::command]
pub async fn tower_show(app: AppHandle, on: bool) -> Result<(), String> {
    app.state::<AppState>().core.set_tower_option("on", on)?;
    crate::tower::sync(&app);
    Ok(())
}

/// One of the tower's other switches: `hideStopped` (projects that neither run nor have Claude
/// in them are left out), `allSpaces` (it shows on every desktop) or `shortcut` (its global
/// shortcut; an error when the keys can't be registered, and the switch stays off).
#[tauri::command]
pub fn tower_option(app: AppHandle, state: State<'_, AppState>, option: String, on: bool) -> Result<(), String> {
    match option.as_str() {
        "shortcut" => crate::shortcuts::apply(&app, crate::shortcuts::Which::Tower, on, &state.core.settings().tower.shortcut_keys)?,
        "hideStopped" | "allSpaces" => {}
        _ => return Err(format!("unknown option: {option}")),
    }

    state.core.set_tower_option(&option, on)?;
    crate::tower::sync(&app);
    Ok(())
}

/// How the tower is drawn: `list` (a row per project) or `strip` (one row of codes).
#[tauri::command]
pub fn tower_form(state: State<'_, AppState>, form: String) -> Result<(), String> {
    state.core.set_tower_form(&form)
}

/// New keys for the tower's shortcut, which is on from then. If they can't be registered
/// (another app holds them, or the quick switcher's shortcut does) the old ones come back.
/// Either way the quick switcher's, dropped while the keys were recorded, is registered again.
#[tauri::command]
pub fn tower_shortcut(app: AppHandle, state: State<'_, AppState>, keys: String) -> Result<(), String> {
    let taken = crate::shortcuts::take(&app, crate::shortcuts::Which::Tower, &keys, &state.core.settings());
    if taken.is_ok() {
        state.core.set_tower_shortcut_keys(keys);
    }
    crate::shortcuts::resume(&app, &state.core);
    taken
}

/// The tower's page has drawn: how large its content is. Its window takes that size and, when
/// it is to be shown and isn't yet, comes up. Only the tower's own page may say so.
#[tauri::command]
pub fn tower_fit(window: tauri::WebviewWindow, width: f64, height: f64) {
    if window.label() == crate::tower::LABEL {
        crate::tower::fit(&window, width, height);
    }
}

/* ---------- the command line: the `pitwall` command and `pitwall://` links (core/cli) ---------- */

/// Where the `pitwall` command is. It asks the shell for its PATH, so it runs off the main thread.
#[tauri::command]
pub async fn cli_state(app: AppHandle) -> Result<crate::core::CliState, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.cli_state()).await.map_err(|error| error.to_string())
}

/// Puts the command on the shell's PATH, as a link to this app, and says where it is.
#[tauri::command]
pub async fn cli_install(app: AppHandle) -> Result<crate::core::CliState, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.cli_install()).await.map_err(|error| error.to_string())?
}

/// Takes the command out again: only a link that leads to this app.
#[tauri::command]
pub async fn cli_remove(app: AppHandle) -> Result<crate::core::CliState, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.cli_remove()).await.map_err(|error| error.to_string())?
}

/// Whether `pitwall://` links are answered.
#[tauri::command]
pub fn cli_links(state: State<'_, AppState>, on: bool) {
    state.core.cli_links(on);
}

/// Puts the link that does `action` (`start` | `stop` | `restart` | `show`) to the listed
/// project at `path` on the clipboard.
#[tauri::command]
pub fn cli_copy_link(app: AppHandle, state: State<'_, AppState>, path: String, action: String) -> Result<(), String> {
    let link = state.core.link_text(&path, &action).ok_or_else(|| format!("unknown link: {action}"))?;
    crate::copy_text(&app, link);
    Ok(())
}

/// The quick switcher's `+`: the listed project looked at last, which a new card is offered
/// for first; none when no listed project was ever looked at.
#[tauri::command]
pub async fn last_looked(app: AppHandle) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.last_looked()).await.ok().flatten()
}

/// The model and the mode the window chose for a Claude session, as it names them. Neither is
/// used as it comes: `gated` keeps only the values of the two fixed lists (`start.rs`).
#[derive(serde::Deserialize)]
pub struct StartChoice {
    model: Option<String>,
    mode: Option<String>,
}

impl StartChoice {
    fn gated(&self) -> crate::start::Start {
        crate::start::Start::of(self.model.as_deref(), self.mode.as_deref())
    }
}

/// Tells a card up next to wait until the 5-hour session refills, as the last reading of
/// Claude's limits says: the core gives it then, in its turn among the cards that wait for the
/// refill too, with the model and in the mode of `start` (without it, the project's own). The
/// moment is the core's own reading, never the window's.
#[tauri::command]
pub fn board_schedule(state: State<'_, AppState>, id: String, start: Option<StartChoice>) -> Result<(), String> {
    let at = state.fuel.refill().ok_or_else(|| t!("core.start.noRefill"))?;
    let start = match start {
        Some(start) => start.gated(),
        None => state.core.board_start(&id).ok_or_else(|| format!("no such card: {id}"))?,
    };
    state.core.board_schedule(&id, at, true, start)
}

/// The card waits no longer; it stays up next.
#[tauri::command]
pub fn board_unschedule(state: State<'_, AppState>, id: String) {
    state.core.board_unschedule(&id);
}

/// "Give now" on a card that waits: given at once with what was chosen for it, unless it waits
/// no longer (`None`). It starts a shell, so it runs off the main thread.
#[tauri::command]
pub async fn board_give_now(app: AppHandle, id: String, cols: Option<u16>, rows: Option<u16>) -> Result<Option<crate::core::TerminalView>, String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.board_give_now(&id, cols.zip(rows))).await.map_err(|error| error.to_string())?
}
