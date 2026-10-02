mod browser;
mod claude;
mod commands;
mod core;
mod extension;
mod git;
mod hooks;
mod i18n;
mod links;
#[cfg(target_os = "macos")]
mod notify;
mod ports;
mod process;
mod registry;
mod resolve;
mod settings;
mod sound;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_opener::OpenerExt;

use crate::core::{Core, CoreConfig, CoreEvent};
use crate::i18n::t;

pub const POPOVER: &str = "popover";
pub const MAIN: &str = "main";
pub const SWITCHER: &str = "switcher";

pub struct AppState {
    pub core: Arc<Core>,
}

#[derive(Clone, serde::Serialize)]
struct TerminalData {
    id: u64,
    seq: u64,
    data: String,
}

/// Lines of output, batched per project and command (see `core::OUTPUT_BATCH`).
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
    }
}

/// A system notification; clicking it opens the project and counts the turn as seen.
#[cfg(target_os = "macos")]
fn notify(app: &AppHandle, path: String, title: String, body: String) {
    let image = claude_mark(app);
    notify::post(path, title, body, image.as_deref().map(std::path::Path::new));
}

/// Plays one of Pitwall's notification sounds (from a notification, or Settings' preview).
pub fn play_sound(app: &AppHandle, id: String) {
    let _ = app.run_on_main_thread(move || sound::play(&id));
}

/// Puts `text` on the clipboard.
pub fn copy_text(app: &AppHandle, text: String) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(move || {
        let board = objc2_app_kit::NSPasteboard::generalPasteboard();
        board.clearContents();
        // SAFETY: an AppKit constant, read on the main thread.
        let kind = unsafe { objc2_app_kit::NSPasteboardTypeString };
        board.setString_forType(&objc2_foundation::NSString::from_str(&text), kind);
    });

    #[cfg(not(target_os = "macos"))]
    let _ = (app, text);
}

#[cfg(not(target_os = "macos"))]
fn notify(_app: &AppHandle, _path: String, _title: String, _body: String) {}

/// Claude's spark beside the notification text. The API takes a file, so the bundled image is
/// written to the cache folder (again when it changed).
#[cfg(target_os = "macos")]
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

/// Running from an installed `.app`, not from `tauri dev`.
pub fn is_bundled() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS"))
}

/// Registers the quick-switcher shortcut (or none when off). An error means the keys are
/// invalid or another app already owns them.
pub fn apply_shortcut(app: &AppHandle, on: bool, keys: &str) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();

    if !on {
        return Ok(());
    }

    let shortcut = keys.parse::<Shortcut>().map_err(|error| t!("core.error.invalidShortcut", keys = keys, error = error))?;
    let result = shortcuts.register(shortcut).map_err(|error| t!("core.error.shortcutTaken", keys = keys, error = error));

    #[cfg(debug_assertions)]
    eprintln!("[pitwall] register {keys}: {result:?}");

    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Passed by the login item, so a launch at login stays in the menu bar.
const AUTOSTART_ARG: &str = "--autostart";

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    #[cfg(debug_assertions)]
                    eprintln!("[pitwall] shortcut {:?}", event.state());
                    if event.state() == ShortcutState::Pressed {
                        tray::toggle_switcher(app);
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

            #[cfg(target_os = "macos")]
            {
                let clicks = app.handle().clone();
                notify::start(is_bundled(), move |path| {
                    if let Some(state) = clicks.try_state::<AppState>() {
                        state.core.open_claude(&path);
                    }
                });
            }

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

            app.manage(AppState { core: Arc::clone(&core) });

            core.reap_orphans();
            core.record_app("start");

            // The login item points at whichever binary registered it; re-register from the
            // installed app so it never launches a stale or dev build.
            if is_bundled() && core.settings().launch_at_login {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
            }
            let settings = core.settings();
            let _ = apply_shortcut(app.handle(), settings.shortcut, &settings.shortcut_keys);
            tray::create(app.handle())?;
            #[cfg(target_os = "macos")]
            {
                if let Some(switcher) = app.get_webview_window(SWITCHER) {
                    tray::make_panel(&switcher);
                }
                for label in [POPOVER, SWITCHER] {
                    if let Some(window) = app.get_webview_window(label) {
                        tray::float_over_spaces(&window);
                    }
                }
            }

            // One loop for everything periodic: commands every second, heartbeat and Claude every
            // 5 s, health every 30 s.
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

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The popover behaves like a menu: it goes away when it loses focus.
            WindowEvent::Focused(false) if window.label() == POPOVER => tray::hide_popover(window.app_handle()),
            // The search goes away the moment it loses the keyboard: a click outside, another app.
            WindowEvent::Focused(_focused) if window.label() == SWITCHER => {
                #[cfg(debug_assertions)]
                eprintln!("[pitwall] switcher focused={_focused}");
                if !_focused {
                    let _ = window.hide();
                }
            }
            // Nor does it stay up behind Pitwall's window once that window has the keyboard.
            WindowEvent::Focused(true) if window.label() == MAIN => {
                if let Some(switcher) = window.app_handle().get_webview_window(SWITCHER) {
                    if switcher.is_visible().unwrap_or(false) {
                        let _ = switcher.hide();
                    }
                }
            }
            // Closing the window only hides it; the app lives in the menu bar.
            WindowEvent::CloseRequested { api, .. } if window.label() == MAIN => {
                api.prevent_close();
                let _ = window.hide();
                tray::set_dock_visible(window.app_handle(), false);
            }
            // Folders dropped on the window become projects.
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
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
            commands::day_summary,
            commands::play_sound,
            commands::open_terminal,
            commands::rename_terminal,
            commands::write_terminal,
            commands::resize_terminal,
            commands::close_terminal,
            commands::terminal_buffer,
            commands::set_favourite,
            commands::act,
            commands::start_all,
            commands::stop_all,
            commands::open_browser,
            commands::open_editor,
            commands::open_claude,
            commands::reveal_claude,
            commands::mark_seen,
            commands::set_settings,
            commands::set_project_settings,
            commands::reorder,
            commands::set_shortcut,
            commands::suspend_shortcut,
            commands::resume_shortcut,
            commands::show_switcher,
            commands::run_command,
            commands::stop_command,
            commands::resolve_address,
            commands::open_window,
            commands::hide_popover,
            commands::hide_switcher,
            commands::install_claude_hook,
            commands::uninstall_claude_hook,
            commands::open_link,
            commands::quit
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                state.core.dispose();
            }
        }
        // Opening the app again (Finder, Dock, Spotlight) while it runs brings the window up.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => tray::show_main(app),
        _ => {}
    });
}
