// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code's hook on Windows and Linux (`--claude-hook <Event>`): notes the event and
    // exits before anything of the app starts.
    if pitwall_lib::hook_mode(std::env::args_os().skip(1)) {
        return;
    }
    // Claude Code's MCP server (`--mcp`), the team radio's other end: it answers Claude Code
    // until that closes it, and nothing of the app starts either.
    if pitwall_lib::mcp_mode(std::env::args_os().skip(1)) {
        return;
    }

    pitwall_lib::run()
}
