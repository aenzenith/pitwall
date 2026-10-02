// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code's hook on Windows and Linux (`--claude-hook <Event>`): notes the event and
    // exits before anything of the app starts.
    if pitwall_lib::hook_mode(std::env::args_os().skip(1)) {
        return;
    }

    pitwall_lib::run()
}
