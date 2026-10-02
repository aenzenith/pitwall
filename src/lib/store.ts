import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ref, watch } from "vue";

import type { DaySummary, ExtensionStatus, Folder, LinkSuggestion, ProjectSettings, Settings, Snapshot, TerminalView } from "./types";

/** The whole app state, pushed by the core on every change. */
export const snapshot = ref<Snapshot | null>(null);

/** Ticks so uptimes and "2 min ago" stay current; stands still while the window is hidden. */
export const now = ref(Date.now());

/**
 * This window is on screen. It starts as the window says (`isVisible`, as windows start hidden
 * and the core's first `visibility` event comes with the first show); after that, whichever says
 * so last wins: the core's `visibility` event (sent as it shows or hides the window) or the
 * page's own `visibilitychange`.
 */
export const visible = ref(true);

let ticker: number | undefined;

function runClock(on: boolean): void {
  window.clearInterval(ticker);
  ticker = undefined;
  if (!on) return;
  now.value = Date.now();
  ticker = window.setInterval(() => (now.value = Date.now()), 10_000);
}

watch(visible, runClock);

export async function connect(): Promise<void> {
  await listen<Snapshot>("state", (event) => {
    snapshot.value = event.payload;
  });
  // This window's own: a plain listen() would also hear what the core sends the other windows.
  const win = getCurrentWindow();
  let told = 0;
  const tell = (on: boolean): void => {
    told++;
    visible.value = on;
  };
  await win.listen<{ visible: boolean }>("visibility", (event) => tell(event.payload.visible));
  document.addEventListener("visibilitychange", () => tell(!document.hidden));
  // The starting state, unless an event already said otherwise while it was asked for.
  const before = told;
  try {
    const shown = await win.isVisible();
    if (told === before) visible.value = shown;
  } catch {
    // Kept as shown: better a clock that ticks unseen than one that stands still on screen.
  }
  snapshot.value = await invoke<Snapshot>("get_state");
  runClock(visible.value);
}

export type Action = "start" | "stop" | "restart";

export const api = {
  act: (path: string, action: Action) => invoke("act", { path, action }),
  startAll: () => invoke("start_all"),
  stopAll: () => invoke("stop_all"),
  pickFolder: () => invoke("pick_folder"),
  addProject: (path: string) => invoke("add_project", { path }),
  pickProjectsDir: () => invoke("pick_projects_dir"),
  clearProjectsDir: () => invoke("clear_projects_dir"),
  projectFolders: () => invoke<Folder[]>("project_folders"),
  daySummary: (date: string | null = null) => invoke<DaySummary>("day_summary", { date }),
  playSound: (id: string) => invoke("play_sound", { id }),
  openUrl: (url: string) => invoke("open_url", { url }),
  copyText: (text: string) => invoke("copy_text", { text }),
  linkSuggestions: (path: string) => invoke<LinkSuggestion[]>("link_suggestions", { path }),
  extensionStatus: (editor: string) => invoke<ExtensionStatus>("extension_status", { editor }),
  openExtensionPage: (editor: string) => invoke("open_extension_page", { editor }),
  openTerminal: (path: string, claude = false, size: { cols: number; rows: number } | null = null) =>
    invoke<TerminalView>("open_terminal", { path, claude, cols: size?.cols, rows: size?.rows }),
  renameTerminal: (id: number, name: string) => invoke("rename_terminal", { id, name }),
  writeTerminal: (id: number, data: string) => invoke("write_terminal", { id, data }),
  resizeTerminal: (id: number, cols: number, rows: number) => invoke("resize_terminal", { id, cols, rows }),
  closeTerminal: (id: number) => invoke("close_terminal", { id }),
  terminalBuffer: (id: number) => invoke<{ data: string; seq: number }>("terminal_buffer", { id }),
  setFavourite: (path: string, on: boolean) => invoke("set_favourite", { path, on }),
  openBrowser: (path: string) => invoke("open_browser", { path }),
  openEditor: (path: string) => invoke("open_editor", { path }),
  openClaude: (path: string) => invoke("open_claude", { path }),
  revealClaude: (path: string, session: string) => invoke("reveal_claude", { path, session }),
  markSeen: (path: string) => invoke("mark_seen", { path }),
  setSettings: (settings: Settings) => invoke("set_settings", { settings }),
  setProjectSettings: (path: string, settings: ProjectSettings) => invoke("set_project_settings", { path, settings }),
  reorder: (paths: string[]) => invoke("reorder", { paths }),
  setShortcut: (keys: string) => invoke("set_shortcut", { keys }),
  suspendShortcut: () => invoke("suspend_shortcut"),
  resumeShortcut: () => invoke("resume_shortcut"),
  showSwitcher: () => invoke("show_switcher"),
  output: (path: string, job: string | null = null) => invoke<string[]>("get_output", { path, job }),
  runCommand: (path: string, id: string) => invoke("run_command", { path, id }),
  stopCommand: (path: string, id: string) => invoke("stop_command", { path, id }),
  resolveAddress: (path: string) => invoke<string | null>("resolve_address", { path }),
  openWindow: () => invoke("open_window"),
  hidePopover: () => invoke("hide_popover"),
  hideSwitcher: () => invoke("hide_switcher"),
  /** The page is listening: a window made on first use comes up. */
  windowReady: () => invoke("window_ready"),
  installClaudeHook: () => invoke("install_claude_hook"),
  uninstallClaudeHook: () => invoke("uninstall_claude_hook"),
  openLink: (link: "site" | "coffee") => invoke("open_link", { link }),
  quit: () => invoke("quit"),
};
