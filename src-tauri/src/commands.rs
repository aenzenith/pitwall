//! Everything the UI may do. The UI never spawns processes or touches files itself.

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::core::{Action, Snapshot};
use crate::i18n::t;
use crate::settings::{ProjectSettings, Settings};
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

/// Folder picker; the chosen folder becomes a project.
#[tauri::command]
pub fn pick_folder(app: AppHandle, state: State<'_, AppState>) {
    let core = state.core.clone();

    app.dialog().file().set_title(t!("common.addProject")).pick_folder(move |folder| {
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
            let mut settings = core.settings();
            settings.projects_dir = Some(path.to_string_lossy().into_owned());
            core.set_settings(settings);
        }
    });
}

#[tauri::command]
pub fn clear_projects_dir(state: State<'_, AppState>) {
    let mut settings = state.core.settings();
    settings.projects_dir = None;
    state.core.set_settings(settings);
}

#[tauri::command]
pub fn project_folders(state: State<'_, AppState>) -> Vec<crate::core::Folder> {
    state.core.project_folders()
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

#[tauri::command]
pub fn open_claude(app: AppHandle, state: State<'_, AppState>, path: String) {
    step_aside(&app);
    state.core.open_claude(&path);
}

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

    // Project overrides, the list order, the shortcut keys and the projects folder have their
    // own commands.
    let mut settings = settings;
    settings.projects = before.projects;
    settings.order = before.order;
    settings.shortcut_keys = before.shortcut_keys;
    settings.projects_dir = before.projects_dir;
    state.core.set_settings(settings);
    Ok(())
}

/// New quick-switcher keys. If they can't be registered the old ones come back.
#[tauri::command]
pub fn set_shortcut(app: AppHandle, state: State<'_, AppState>, keys: String) -> Result<(), String> {
    let mut settings = state.core.settings();

    if let Err(error) = apply_shortcut(&app, true, &keys) {
        let _ = apply_shortcut(&app, settings.shortcut, &settings.shortcut_keys);
        return Err(error);
    }

    settings.shortcut = true;
    settings.shortcut_keys = keys;
    state.core.set_settings(settings);
    Ok(())
}

/// The quick switcher, from the popover's search button.
#[tauri::command]
pub fn show_switcher(app: AppHandle) {
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

#[tauri::command]
pub fn set_project_settings(state: State<'_, AppState>, path: String, settings: ProjectSettings) {
    state.core.set_project_settings(&path, settings);
}

#[tauri::command]
pub fn open_window(app: AppHandle) {
    tray::show_main(&app);
}

/// Esc in the popover: close it and go back to the app from before.
#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    tray::dismiss_popover(&app);
}

/// Esc in the switcher: close it and go back to the app from before.
#[tauri::command]
pub fn hide_switcher(app: AppHandle) {
    tray::dismiss_switcher(&app);
}

/// Before opening something (editor, Claude, a link): the popover and switcher get out of the
/// way. While Pitwall is the active app they go when the opened app takes focus, since hidden
/// right away Pitwall's own window would come up for a moment. When it isn't (the switcher,
/// opened by its shortcut), they go at once rather than linger while that app starts.
fn step_aside(app: &AppHandle) {
    let wait = tray::pitwall_active();
    for label in [POPOVER, SWITCHER] {
        if let Some(window) = app.get_webview_window(label) {
            if window.is_visible().unwrap_or(false) {
                if wait {
                    tray::hide_once_focus_moves(window, std::time::Duration::from_millis(1500));
                } else {
                    let _ = window.hide();
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

/// Opens one of the About links in the browser. The UI names a link, never an address.
#[tauri::command]
pub fn open_link(app: AppHandle, link: String) -> Result<(), String> {
    let url = match link.as_str() {
        "site" => "https://aenzenith.com",
        "coffee" => "https://buymeacoffee.com/aenzenith",
        _ => return Err(format!("Unknown link: {link}")),
    };
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
