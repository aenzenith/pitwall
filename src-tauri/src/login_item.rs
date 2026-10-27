//! Launch at login. On macOS Pitwall writes its launch agent itself, so that the agent names the
//! app's bundle (`AssociatedBundleIdentifiers`): the autostart plugin leaves that list empty, and
//! System Settings' Login Items then shows the agent under the signing certificate's owner, with
//! no icon, instead of Pitwall. The agent is the plugin's own file
//! (`~/Library/LaunchAgents/Pitwall.plist`: the same label, program and argument), so one written
//! before is rewritten in place. Elsewhere the plugin's entry as it is.

use tauri::AppHandle;

/// Turns launch at login on or off.
pub fn set(app: &AppHandle, on: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = {
        let agent = agent(app)?;
        if on { agent.enable() } else { agent.disable() }
    };
    #[cfg(not(target_os = "macos"))]
    let result = {
        use tauri_plugin_autostart::ManagerExt;
        let autostart = app.autolaunch();
        if on { autostart.enable() } else { autostart.disable() }
    };
    result.map_err(|error| error.to_string())
}

/// The agent as the plugin would write it, plus the bundle it belongs to. Its program is the
/// executable inside the bundle, links followed: the Team ID System Settings matches against the
/// bundle's is that executable's.
#[cfg(target_os = "macos")]
fn agent(app: &AppHandle) -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().and_then(|path| path.canonicalize()).map_err(|error| error.to_string())?;
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name(&app.package_info().name)
        .set_app_path(&exe.display().to_string())
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .set_args(&[crate::AUTOSTART_ARG])
        .set_bundle_identifiers(&[app.config().identifier.as_str()])
        .build()
        .map_err(|error| error.to_string())
}
