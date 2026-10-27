mod accounts;
mod browser;
mod claude;
mod clock;
mod commands;
mod core;
mod deps;
mod extension;
mod fuel;
mod git;
mod hooks;
mod i18n;
mod links;
mod login_item;
// One API, one file per system: UserNotifications on macOS, toasts on Windows, D-Bus on Linux.
#[cfg_attr(target_os = "linux", path = "notify_linux.rs")]
#[cfg_attr(windows, path = "notify_windows.rs")]
mod notify;
mod ports;
mod process;
mod registry;
mod resolve;
mod sessions;
mod settings;
mod sound;
mod spend;
mod tray;
mod update;
mod usage;
mod whatsnew;
mod windows;
mod presence;
mod mcp;
mod flag;
mod shortcuts;
mod tower;
mod cli;
mod start;
mod files;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::ShortcutState;
use tauri_plugin_opener::OpenerExt;

use crate::core::{Core, CoreConfig, CoreEvent};

pub use crate::hooks::hook_mode;
pub use crate::mcp::mcp_mode;
pub use crate::cli::cli_mode;

pub const POPOVER: &str = "popover";
pub const MAIN: &str = "main";
pub const SWITCHER: &str = "switcher";

pub struct AppState {
    pub core: Arc<Core>,
    pub fuel: fuel::Fuel,
    pub sessions: sessions::Sessions,
    pub update: update::Update,
    /// The backup file picked last (`commands::backup_pick`): the one an import reads.
    pub backup: std::sync::Mutex<Option<PathBuf>>,
}

#[derive(Clone, serde::Serialize)]
struct TerminalData {
    id: u64,
    seq: u64,
    data: String,
}

/// A terminal for the main window to show (`reveal-terminal`).
#[derive(Clone, serde::Serialize)]
struct TerminalReveal {
    path: String,
    id: u64,
}

/// Lines of output, batched per project and command (see `core::output::OUTPUT_BATCH`).
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputBatch {
    path: String,
    /// Empty for the dev server, the command id otherwise.
    job: Option<String>,
    lines: Vec<String>,
}

/// VS Code-family storage folders the extension used before `~/.pitwall/`.
fn old_extension_storage() -> Vec<PathBuf> {
    let Some(config) = dirs::config_dir() else {
        return Vec::new();
    };

    let mut sources = Vec::new();

    for editor in ["Code", "Code - Insiders", "Cursor", "Windsurf"] {
        for id in ["aenzenith.pitwall-vscode", "aenzenith.pitwall"] {
            sources.push(config.join(editor).join("User").join("globalStorage").join(id));
        }
    }

    sources
}

fn handle_event(app: &AppHandle, event: CoreEvent) {
    match event {
        CoreEvent::State => {
            if let Some(state) = app.try_state::<AppState>() {
                let snapshot = state.core.snapshot();
                tray::refresh(app, &snapshot);
                let _ = app.emit("state", snapshot);
            }
        }
        // Only the window shows output.
        CoreEvent::Output { path, job, lines } => {
            let _ = app.emit_to(MAIN, "output", OutputBatch { path, job, lines });
        }
        CoreEvent::Open(url) => {
            let _ = app.opener().open_url(url, None::<&str>);
        }
        CoreEvent::Notify { path, title, body } => notify(app, path, title, body),
        CoreEvent::Sound(id) => play_sound(app, id),
        CoreEvent::Terminal { id, seq, data } => {
            let _ = app.emit_to(MAIN, "terminal", TerminalData { id, seq, data });
        }
        CoreEvent::TerminalExit { id } => {
            let _ = app.emit_to(MAIN, "terminal-exit", id);
        }
        // Held for the main window's page until it listens (`windows::FOR_MAIN`).
        CoreEvent::RevealTerminal { path, id } => {
            tray::show_main(app);
            let _ = app.emit("reveal-terminal", TerminalReveal { path, id });
        }
        // Only the window shows the board.
        CoreEvent::Board(cards) => {
            let _ = app.emit_to(MAIN, "board", cards);
        }
        CoreEvent::Deps(reports) => {
            let _ = app.emit_to(MAIN, "deps", reports);
        }
        // From a notification or the popover's list of what was missed: held for the main
        // window's page until it listens, as a terminal's is.
        CoreEvent::RevealProject { path, job } => {
            tray::show_main(app);
            let _ = match job {
                Some(job) => app.emit("reveal-output", serde_json::json!({ "path": path, "job": job })),
                None => app.emit("reveal-project", serde_json::json!({ "path": path })),
            };
        }
        CoreEvent::RevealPage(page) => {
            tray::show_main(app);
            let _ = app.emit("reveal-page", serde_json::json!({ "page": page }));
        }
    }
}

/// A system notification; clicking it brings the project's waiting Claude session up where it
/// runs (without one, the project's editor window) and counts the turn as seen. One about a
/// server, a command or the Garage opens Pitwall's window instead (`Core::open_banner`), and
/// carries no mark of Claude's.
pub(crate) fn notify(app: &AppHandle, path: String, title: String, body: String) {
    let image = if core::plain_banner(&path) { None } else { claude_mark(app) };
    notify::post(path, title, body, image.as_deref().map(std::path::Path::new));
}

/// Plays one of Pitwall's notification sounds (from a notification, or Settings' preview).
pub fn play_sound(app: &AppHandle, id: String) {
    let _ = app.run_on_main_thread(move || sound::play(&id));
}

/// Puts `text` on the clipboard.
pub fn copy_text(app: &AppHandle, text: String) {
    let _ = app.clipboard().write_text(text);
}

/// Claude's mark (components/ClaudeLogo) beside the notification text. The APIs take a file, so the bundled image is
/// written to the cache folder (again when it changed).
fn claude_mark(app: &AppHandle) -> Option<String> {
    const MARK: &[u8] = include_bytes!("../icons/claude-mark.png");
    let dir = app.path().app_cache_dir().ok()?;
    let file = dir.join("claude-mark.png");

    if std::fs::metadata(&file).map(|meta| meta.len()).ok() != Some(MARK.len() as u64) {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&file, MARK).ok()?;
    }

    Some(file.to_string_lossy().into_owned())
}

/// Running as the installed app (an `.app`, Program Files, `/usr/bin`, an AppImage…), not from
/// `tauri dev`.
pub fn is_bundled() -> bool {
    let appimage = std::env::var_os("APPIMAGE").is_some_and(|path| !path.is_empty());
    std::env::current_exe().is_ok_and(|exe| process::installed_app(&exe, appimage))
}

/// Draws the windows' native side (menus, scroll bars, form controls) as Settings says:
/// `light`, `dark`, or as the system is. The pages follow the same setting themselves
/// (src/lib/theme.ts).
pub fn apply_theme(app: &AppHandle, theme: &str) {
    app.set_theme(match theme {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    });
}

/// Registers the quick-switcher shortcut (or none when off). An error means the keys are
/// invalid or another app already owns them. The mini tower's own shortcut stays as it is
/// (`shortcuts.rs`).
pub fn apply_shortcut(app: &AppHandle, on: bool, keys: &str) -> Result<(), String> {
    shortcuts::apply(app, shortcuts::Which::Switcher, on, keys)
}

/// `pitwall://` links that came before the app was set up, until it is: in memory only.
fn early_links() -> std::sync::MutexGuard<'static, Vec<String>> {
    static EARLY: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    EARLY.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A `pitwall://` link the system handed over (`Core::open_link`): it does one of its four
/// things or nothing at all, and nothing of it is logged. The window comes up when it asks.
fn open_link(app: &AppHandle, url: &str) {
    match app.try_state::<AppState>() {
        Some(state) => {
            if state.core.open_link(url) {
                tray::show_main(app);
            }
        }
        None => early_links().push(url.to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Passed by the login item, so a launch at login stays in the menu bar.
const AUTOSTART_ARG: &str = "--autostart";

pub fn run() {
    // Started from inside a Claude session (a dev run), the app is no part of it.
    process::leave_claude_session();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    #[cfg(debug_assertions)]
                    eprintln!("[pitwall] shortcut {:?}", event.state());
                    // The switcher's or the mini tower's: `shortcuts` tells which.
                    if event.state() == ShortcutState::Pressed {
                        shortcuts::fired(app, shortcut);
                    }
                })
                .build(),
        )
        .setup(|app| {
            // Menu bar app: no Dock icon until the window is opened.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let home = dirs::home_dir().ok_or("no home folder")?;
            let registry_dir = home.join(".pitwall");

            // Bring favourites and seen state over from the extension's old storage, once.
            registry::adopt_old_storage(&registry_dir, &old_extension_storage());

            let clicks = app.handle().clone();
            notify::start(is_bundled(), move |path| {
                // The Fuel page's notification opens that page.
                if path == fuel::NOTIFICATION_ID {
                    tray::show_main(&clicks);
                    let _ = clicks.emit("reveal-fuel", ());
                } else if path == update::NOTIFICATION_ID || path == core::TEST_BANNER {
                    tray::show_main(&clicks);
                } else if let Some(state) = clicks.try_state::<AppState>() {
                    state.core.open_banner(&path);
                }
            });

            let handle = app.handle().clone();
            let legacy = old_extension_storage();
            let core = Core::new(
                CoreConfig {
                    registry_dir,
                    claude_dir: home.join(".claude").join("projects"),
                    settings_file: app.path().app_config_dir()?.join("settings.json"),
                    claude_settings: home.join(".claude").join("settings.json"),
                    legacy_registries: legacy,
                    title: "Pitwall".into(),
                },
                Arc::new(move |event| handle_event(&handle, event)),
            );

            let fuel = fuel::Fuel::new(app.handle(), home.join(".claude").join("projects"));
            let update = update::Update::new(app.handle());
            app.manage(AppState { core: Arc::clone(&core), fuel, sessions: sessions::Sessions::default(), update, backup: std::sync::Mutex::new(None) });
            fuel::Fuel::start(app.handle());
            sessions::Sessions::start(app.handle());
            update::Update::start(app.handle());

            core.reap_orphans();
            core.record_app("start");
            // The team radio's endpoint, when it is on.
            core.radio_start();
            // The command line's line, when the `pitwall` command is installed.
            core.cli_start();
            // After a restart for an update: the servers, tabs and Claude sessions it closed.
            let restored = core.restore();
            // After a quit: what was open, put back or offered as Settings says.
            core.resume_start(restored.is_some());
            // Not after that restart: what ran came back, and what was stopped stays stopped.
            if restored.is_none() {
                core.start_launch_teams();
            }

            // The login item points at whichever binary registered it; re-register from the
            // installed app so it never launches a stale or dev build. An agent written before it
            // named Pitwall's bundle gets the name here too (`login_item`).
            if is_bundled() && core.settings().launch_at_login {
                let _ = login_item::set(app.handle(), true);
            }
            let settings = core.settings();
            // The quick switcher's and the mini tower's, each on its own.
            shortcuts::resume(app.handle(), &core);
            apply_theme(app.handle(), &settings.theme);
            // The popover and the switcher never bring Pitwall to the front, not even while they
            // are made (`tray::without_activating`).
            #[cfg(target_os = "macos")]
            tray::guard_activation();
            // The popover opens from the icon. Linux delivers no clicks on it (the tray has a
            // menu there instead), so it is made only on macOS and Windows.
            #[cfg(not(target_os = "linux"))]
            let _ = windows::get_or_create(app.handle(), POPOVER);
            tray::create(app.handle())?;
            // ⌘Q asks what is still open first, as the popover's quit button does (`flag.rs`).
            #[cfg(target_os = "macos")]
            flag::own_quit_item(app.handle());
            #[cfg(target_os = "macos")]
            tray::hide_popover_on_outside_click(app.handle());
            // The main window and the switcher are made on first use (`windows`).
            windows::setup(app.handle());
            // The mini tower comes back as it was left: shown or not, and where.
            tower::sync(app.handle());

            // One loop for everything periodic: commands and peers every second, heartbeat and
            // Claude every 5 s, health every 30 s.
            thread::spawn(move || {
                let mut tick: u64 = 0;
                loop {
                    core.tick(tick);
                    tick += 1;
                    thread::sleep(Duration::from_secs(1));
                }
            });

            // Opened by hand, the window comes up; at login the app stays in the menu bar.
            if !std::env::args().any(|arg| arg == AUTOSTART_ARG) {
                tray::show_main(app.handle());
            }
            if let Some(restored) = &restored {
                update::restored(app.handle(), restored);
            }
            // A link that started the app came before there was anything to answer it.
            for url in std::mem::take(&mut *early_links()) {
                open_link(app.handle(), &url);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The popover behaves like a menu: it goes away when it loses focus (and, on macOS, at
            // any click outside Pitwall: `tray::hide_popover_on_outside_click`).
            WindowEvent::Focused(false) if window.label() == POPOVER => tray::hide_popover(window.app_handle()),
            // It sizes itself to its content; above a taskbar it grows upwards, away from the icon.
            #[cfg(windows)]
            WindowEvent::Resized(_) if window.label() == POPOVER => tray::keep_popover_by_icon(window.app_handle()),
            // The search goes away the moment it loses the keyboard: a click outside, another app.
            WindowEvent::Focused(_focused) if window.label() == SWITCHER => {
                #[cfg(debug_assertions)]
                eprintln!("[pitwall] switcher focused={_focused}");
                if !_focused {
                    let _ = window.hide();
                    windows::tell(window.app_handle(), SWITCHER, false);
                }
            }
            // Nor does it stay up behind Pitwall's window once that window has the keyboard.
            WindowEvent::Focused(true) if window.label() == MAIN => {
                if let Some(switcher) = window.app_handle().get_webview_window(SWITCHER) {
                    if switcher.is_visible().unwrap_or(false) {
                        let _ = windows::hide(&switcher);
                    }
                }
            }
            // Closing the window only hides it; the app lives in the menu bar.
            WindowEvent::CloseRequested { api, .. } if window.label() == MAIN => {
                api.prevent_close();
                let _ = window.hide();
                windows::tell(window.app_handle(), MAIN, false);
                tray::set_dock_visible(window.app_handle(), false);
            }
            // Where the mini tower is left is where it comes back (`tower.rs`).
            WindowEvent::Moved(_) | WindowEvent::Resized(_) if window.label() == tower::LABEL => tower::frame_changed(window.app_handle()),
            // Folders dropped on the window become projects. On the window only: the mini tower
            // stands over everything else, where a folder on its way somewhere may be let go.
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) if window.label() == MAIN => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    for path in paths.iter().filter(|p| p.is_dir()) {
                        let _ = state.core.add_project(&path.to_string_lossy());
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::get_output,
            commands::pick_folder,
            commands::add_project,
            commands::pick_projects_dir,
            commands::clear_projects_dir,
            commands::project_folders,
            commands::open_url,
            commands::copy_text,
            commands::link_suggestions,
            commands::extension_status,
            commands::open_extension_page,
            commands::installed_editors,
            commands::notification_access,
            commands::notification_test,
            commands::notification_settings,
            commands::day_summary,
            commands::claude_sessions,
            commands::claude_history,
            commands::claude_past,
            commands::resume_claude,
            commands::claude_last_message,
            commands::play_sound,
            commands::open_terminal,
            commands::terminal_shells,
            commands::open_sudo_settings,
            commands::admin_not_restored_seen,
            commands::rename_terminal,
            commands::set_terminal_layout,
            commands::set_panes_height,
            commands::write_terminal,
            commands::resize_terminal,
            commands::close_terminal,
            commands::terminal_buffer,
            commands::board_state,
            commands::board_add,
            commands::board_edit,
            commands::board_image,
            commands::board_move,
            commands::board_delete,
            commands::board_rehome,
            commands::board_give,
            commands::board_link,
            commands::set_favourite,
            commands::remove_project,
            commands::act,
            commands::open_browser,
            commands::open_editor,
            commands::open_claude,
            commands::reveal_claude,
            commands::mark_seen,
            commands::set_settings,
            commands::set_project_settings,
            commands::work_folder,
            commands::pick_work_folder,
            commands::reorder,
            commands::set_board_scope,
            commands::set_board_view,
            commands::set_favourites_only,
            commands::set_files_preview,
            commands::set_track,
            commands::set_shortcut,
            commands::suspend_shortcut,
            commands::resume_shortcut,
            commands::show_switcher,
            commands::run_command,
            commands::stop_command,
            commands::skip_held,
            commands::resolve_address,
            commands::open_window,
            commands::hide_popover,
            commands::hide_switcher,
            commands::window_ready,
            commands::fuel_state,
            commands::refresh_fuel,
            commands::account_use,
            commands::account_rename,
            commands::account_remove,
            commands::account_set_sync,
            commands::account_reveal,
            commands::account_folder,
            commands::account_pick_folder,
            commands::account_login,
            commands::account_login_cancel,
            commands::account_sync,
            commands::account_sync_plan,
            commands::deps_state,
            commands::deps_check,
            commands::deps_install,
            commands::deps_migrate,
            commands::deps_scan,
            commands::deps_scan_details,
            commands::install_claude_hook,
            commands::uninstall_claude_hook,
            commands::update_state,
            commands::update_check,
            commands::update_plan,
            commands::update_install,
            commands::open_link,
            commands::team_create,
            commands::team_rename,
            commands::team_delete,
            commands::team_projects,
            commands::team_order,
            commands::team_collapse,
            commands::team_launch,
            commands::team_start,
            commands::team_stop,
            commands::discover_projects,
            commands::add_projects,
            commands::dismiss_suggestion,
            commands::set_onboarded,
            commands::whats_new,
            commands::whats_new_seen,
            commands::resume_restore,
            commands::resume_dismiss,
            commands::backup_export,
            commands::backup_pick,
            commands::backup_import,
            commands::missing_projects,
            commands::rehome_project,
            commands::forget_project,
            commands::quit,
            commands::phone_config,
            commands::phone_set,
            commands::phone_test,
            commands::phone_topic,
            commands::quiet_start,
            commands::quiet_end,
            commands::quiet_option,
            commands::quiet_hours,
            commands::notify_event,
            commands::mute_project,
            commands::missed_open,
            commands::missed_clear,
            commands::radio_tools,
            commands::radio_state,
            commands::radio_set,
            commands::radio_tool,
            commands::radio_calls,
            commands::open_reference,
            commands::explorer_list,
            commands::explorer_read,
            commands::explorer_image,
            commands::explorer_status,
            commands::explorer_diff,
            commands::explorer_files,
            commands::explorer_reveal,
            commands::explorer_log,
            commands::explorer_commit,
            commands::explorer_commit_diff,
            commands::flag_state,
            commands::flag_set,
            commands::flag_shown,
            commands::flag_keep,
            commands::flag_quit,
            commands::flag_stop_all,
            commands::flag_terminal,
            commands::flag_board,
            commands::tower_show,
            commands::tower_option,
            commands::tower_form,
            commands::tower_shortcut,
            commands::tower_fit,
            commands::cli_state,
            commands::cli_install,
            commands::cli_remove,
            commands::cli_links,
            commands::cli_copy_link,
            commands::last_looked,
            commands::board_schedule,
            commands::board_unschedule,
            commands::board_give_now
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                // Where the mini tower was left, should that not be written yet.
                tower::flush(&state.core);
                // What is open, for the next start; then every server goes.
                state.core.save_last_session();
                state.core.dispose();
                // An update that waits goes in on the way out; a restart for it took it already.
                state.update.install_on_quit();
            }
        }
        // Opening the app again (Finder, Dock, Spotlight) while it runs brings the window up; the
        // reopen macOS sends behind a click on a notification doesn't (the click went elsewhere).
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if !notify::reopened_by_click() {
                tray::show_main(app);
            }
        }
        // `pitwall://` links, from whatever opened them: each is the core's to answer or not.
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            for url in urls {
                open_link(app, url.as_str());
            }
        }
        _ => {}
    });
}
