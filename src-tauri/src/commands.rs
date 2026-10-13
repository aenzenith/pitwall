//! Everything the UI may do. The UI never spawns processes or touches files itself.

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::core::{Action, Snapshot};
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

/// The address "Open in browser" would open, to show it on the dev server row.
#[tauri::command]
pub fn resolve_address(state: State<'_, AppState>, path: String) -> Option<String> {
    let local = state.core.snapshot().projects.into_iter().find(|p| p.path == path).and_then(|p| p.url);
    state.core.resolve_url(&path, local)
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
pub async fn link_suggestions(path: String) -> Vec<crate::links::LinkSuggestion> {
    tauri::async_runtime::spawn_blocking(move || crate::links::suggestions(&path)).await.unwrap_or_default()
}

/// Whether `editor` (as in Settings) has Pitwall for VS Code.
#[tauri::command]
pub fn extension_status(editor: String) -> crate::extension::ExtensionStatus {
    crate::extension::status(&editor)
}

/// Where to get Pitwall for VS Code for `editor`: its page in the editor, or the release.
#[tauri::command]
pub fn open_extension_page(app: AppHandle, editor: String) -> Result<(), String> {
    app.opener().open_url(crate::extension::page(&editor), None::<&str>).map_err(|error| error.to_string())
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

/// What Claude last said in a session, for its details (the Sessions page's, a board card's):
/// read from its log off the main thread, never kept.
#[tauri::command]
pub async fn claude_last_message(app: AppHandle, session: String) -> Option<String> {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.last_message(&session)).await.ok().flatten()
}

/* ---------- project terminals ---------- */

#[tauri::command]
pub fn open_terminal(
    state: State<'_, AppState>,
    path: String,
    claude: Option<bool>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<crate::core::TerminalView, String> {
    state.core.open_terminal(&path, claude.unwrap_or(false), cols.zip(rows))
}

#[tauri::command]
pub fn rename_terminal(state: State<'_, AppState>, id: u64, name: String) {
    state.core.rename_terminal(id, &name);
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
/// starts a shell and reads the process table, so it runs off the main thread.
#[tauri::command]
pub async fn board_give(app: AppHandle, id: String, mode: String, cols: Option<u16>, rows: Option<u16>) -> Result<Option<crate::core::TerminalView>, String> {
    let give = crate::core::Give::parse(&mode).ok_or_else(|| format!("unknown mode: {mode}"))?;
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.board_give(&id, give, cols.zip(rows))).await.map_err(|error| error.to_string())?
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

#[tauri::command]
pub fn add_project(state: State<'_, AppState>, path: String) -> Result<(), String> {
    state.core.add_project(&path)
}

#[tauri::command]
pub fn set_favourite(state: State<'_, AppState>, path: String, on: bool) {
    state.core.set_favourite(&path, on);
}

#[tauri::command]
pub fn act(state: State<'_, AppState>, path: String, action: String) -> Result<(), String> {
    let action = Action::parse(&action).ok_or_else(|| format!("unknown action: {action}"))?;
    state.core.act(&path, action);
    Ok(())
}

#[tauri::command]
pub fn start_all(state: State<'_, AppState>) {
    state.core.start_all();
}

#[tauri::command]
pub fn stop_all(state: State<'_, AppState>) {
    state.core.stop_all();
}

#[tauri::command]
pub fn open_browser(app: AppHandle, state: State<'_, AppState>, path: String) {
    step_aside(&app);
    state.core.open_browser(&path);
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

/// New quick-switcher keys. If they can't be registered the old ones come back.
#[tauri::command]
pub fn set_shortcut(app: AppHandle, state: State<'_, AppState>, keys: String) -> Result<(), String> {
    if let Err(error) = apply_shortcut(&app, true, &keys) {
        let settings = state.core.settings();
        let _ = apply_shortcut(&app, settings.shortcut, &settings.shortcut_keys);
        return Err(error);
    }

    state.core.set_shortcut_keys(keys);
    Ok(())
}

/// The quick switcher, from the popover's search button. Async, as is every command that can make
/// a window: made from a synchronous command, WebView2 deadlocks (wry#583).
#[tauri::command]
pub async fn show_switcher(app: AppHandle) {
    tray::toggle_switcher(&app);
}

/// While a new shortcut is being recorded, the current one must not fire.
#[tauri::command]
pub fn suspend_shortcut(app: AppHandle) {
    let _ = apply_shortcut(&app, false, "");
}

#[tauri::command]
pub fn resume_shortcut(app: AppHandle, state: State<'_, AppState>) {
    let settings = state.core.settings();
    let _ = apply_shortcut(&app, settings.shortcut, &settings.shortcut_keys);
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

/// The Track page's own settings: its circuit, what the cars carry, how much moves.
#[tauri::command]
pub fn set_track(state: State<'_, AppState>, track: TrackSettings) {
    state.core.set_track(track);
}

#[tauri::command]
pub fn set_project_settings(state: State<'_, AppState>, path: String, settings: ProjectSettings) {
    state.core.set_project_settings(&path, settings);
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
            // until new ones are recorded in Settings.
            let _ = apply_shortcut(&app, settings.shortcut, &settings.shortcut_keys);
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

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
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

/* ---------- the output pane ---------- */

/// Opens the file an output line names (`file`, as it was printed) in the editor at `line`:
/// only a file of the project at `path` (`Core::open_reference`). False when there is none.
/// It may look through the project's folder, so it runs off the main thread.
#[tauri::command]
pub async fn open_reference(app: AppHandle, path: String, file: String, line: u32, column: Option<u32>) -> bool {
    let core = std::sync::Arc::clone(&app.state::<AppState>().core);
    tauri::async_runtime::spawn_blocking(move || core.open_reference(&path, &file, line, column)).await.unwrap_or(false)
}
