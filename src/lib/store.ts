import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, ref, watch } from "vue";

import type {
  BoardColumn,
  Card,
  DaySummary,
  DepReport,
  DepScanDetails,
  ExtensionStatus,
  Folder,
  Fuel,
  LinkSuggestion,
  NewImage,
  ProjectSettings,
  RestartPlan,
  SessionsView,
  Settings,
  Snapshot,
  TerminalView,
  UpdateView,
} from "./types";
import { dayName } from "./day";
import { trackSettings, type TrackSettings } from "./track";

/** The whole app state, pushed by the core on every change. */
export const snapshot = ref<Snapshot | null>(null);

/** Ticks so uptimes and "2 min ago" stay current; stands still while the window is hidden. */
export const now = ref(Date.now());

/** The day it is (`YYYY-MM-DD`, local), by that clock: a page left open over midnight follows it. */
export const today = computed(() => dayName(now.value));

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

/** Claude's plan limits and today's tokens: the Fuel page's, and its sidebar badge's. */
export const fuel = ref<Fuel | null>(null);

/**
 * The main window's: hears `fuel` (sent to this window only) and asks for the current state. A
 * core that doesn't answer leaves it null, which shows as loading.
 */
export async function connectFuel(): Promise<UnlistenFn> {
  let heard = 0;
  const unlisten = await getCurrentWindow().listen<Fuel>("fuel", (event) => {
    heard++;
    fuel.value = event.payload;
  });
  const before = heard;
  try {
    const state = await invoke<Fuel>("fuel_state");
    // An event that came in meanwhile is newer.
    if (heard === before) fuel.value = state;
  } catch {
    // Kept as loading; the next `fuel` event fills it.
  }
  return unlisten;
}

/** Every listed project's dependencies: the Garage page's, and its sidebar count's. Null
 * until the core answers. */
export const deps = ref<DepReport[] | null>(null);

/** Asks the core for the dependencies; one that doesn't answer (yet) leaves them as they were. */
export async function loadDeps(): Promise<void> {
  try {
    deps.value = await api.depsState();
  } catch {
    // Kept as it was (null shows as loading); the next `deps` event fills it.
  }
}

/**
 * The main window's: hears `deps` (sent to this window only, every project's report after each
 * change) and asks for the current state.
 */
export async function connectDeps(): Promise<UnlistenFn> {
  let heard = 0;
  const unlisten = await getCurrentWindow().listen<DepReport[]>("deps", (event) => {
    heard++;
    deps.value = event.payload;
  });
  const before = heard;
  try {
    const reports = await api.depsState();
    // An event that came in meanwhile is newer.
    if (heard === before) deps.value = reports;
  } catch {
    // Kept as loading; the next `deps` event fills it.
  }
  return unlisten;
}

/** Every Claude Code session today: the Sessions page's, and its sidebar count's. */
export const sessions = ref<SessionsView | null>(null);

/** Asks the core for the sessions; one that doesn't answer (yet) leaves them as they were. */
export async function loadSessions(): Promise<void> {
  try {
    sessions.value = await api.claudeSessions();
  } catch {
    // Kept as it was (null shows as loading); the next `sessions` event fills it.
  }
}

/**
 * The main window's: hears `sessions` (sent to this window only, on every change) and asks for
 * the current state.
 */
export async function connectSessions(): Promise<UnlistenFn> {
  let heard = 0;
  const unlisten = await getCurrentWindow().listen<SessionsView>("sessions", (event) => {
    heard++;
    sessions.value = event.payload;
  });
  const before = heard;
  try {
    const view = await api.claudeSessions();
    // An event that came in meanwhile is newer.
    if (heard === before) sessions.value = view;
  } catch {
    // Kept as loading; the next `sessions` event fills it.
  }
  return unlisten;
}

/** Every card on the board, all projects': the Board page's. Null until the core answers. */
export const board = ref<Card[] | null>(null);

/** `board` events heard so far: an answer asked for before the latest one is older than it. */
let boardHeard = 0;

/** Asks the core for the cards; one that doesn't answer (yet) leaves them as they were. */
export async function loadBoard(): Promise<void> {
  const before = boardHeard;
  try {
    const cards = await api.boardState();
    // An event that came in meanwhile is newer.
    if (boardHeard === before) board.value = cards;
  } catch {
    // Kept as it was (null shows as loading); the next `board` event fills it.
  }
}

/**
 * The main window's: hears `board` (sent to this window only, with every card after each change)
 * and asks for the current state.
 */
export async function connectBoard(): Promise<UnlistenFn> {
  const unlisten = await getCurrentWindow().listen<Card[]>("board", (event) => {
    boardHeard++;
    board.value = event.payload;
  });
  await loadBoard();
  return unlisten;
}

/** The app's own update: the brand row's version and button, and Settings › About. Null until the
 * core answers. */
export const update = ref<UpdateView | null>(null);

/**
 * The main window's: hears `update` (sent to this window only, on every change) and asks for the
 * current state. A core that doesn't answer leaves it null: no version, no button.
 */
export async function connectUpdate(): Promise<UnlistenFn> {
  let heard = 0;
  const unlisten = await getCurrentWindow().listen<UpdateView>("update", (event) => {
    heard++;
    update.value = event.payload;
  });
  const before = heard;
  try {
    const state = await api.updateState();
    // An event that came in meanwhile is newer.
    if (heard === before) update.value = state;
  } catch {
    // Kept as it was; the next `update` event fills it.
  }
  return unlisten;
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
  /** Every Claude Code session today (the Sessions page). */
  claudeSessions: () => invoke<SessionsView>("claude_sessions"),
  /** The sessions of an earlier day (`YYYY-MM-DD`) or of every day kept (`all`), each as an ended
   * row: the Sessions page looking back. With `all`, a row's time is the days before today's; the
   * page adds today's from the live row. */
  claudeHistory: (period: string) => invoke<SessionsView>("claude_history", { period }),
  /** What Claude last said in a session (Markdown): read when asked, never kept. */
  lastMessage: (session: string) => invoke<string | null>("claude_last_message", { session }),
  /** Every listed project's dependencies (the Garage page). */
  depsState: () => invoke<DepReport[]>("deps_state"),
  /** Reads lock files and installed packages again: one project's, or every one's (`null`). */
  depsCheck: (path: string | null) => invoke("deps_check", { path }),
  depsInstall: (path: string, ecosystem: string) => invoke("deps_install", { path, ecosystem }),
  /** One migration tool's migrate (`laravel`, `django`, …). */
  depsMigrate: (path: string, tool: string) => invoke("deps_migrate", { path, tool }),
  /** `outdated` and `audit` for one ecosystem: needs the network. */
  depsScan: (path: string, ecosystem: string) => invoke("deps_scan", { path, ecosystem }),
  /** What the last scan of one ecosystem listed; null for one kept from before it was listed. */
  depsScanDetails: (path: string, ecosystem: string) => invoke<DepScanDetails | null>("deps_scan_details", { path, ecosystem }),
  markSeen: (path: string) => invoke("mark_seen", { path }),
  setSettings: (settings: Settings) => invoke("set_settings", { settings }),
  setProjectSettings: (path: string, settings: ProjectSettings) => invoke("set_project_settings", { path, settings }),
  reorder: (paths: string[]) => invoke("reorder", { paths }),
  /** The board picked on the Board page: `all`, or a project's path. */
  setBoardScope: (scope: string) => invoke("set_board_scope", { scope }),
  /** How a project's board is drawn: `columns` or `list`. */
  setBoardView: (view: string) => invoke("set_board_view", { view }),
  /** The Track page's own settings: its circuit, what the cars carry, how much moves. */
  setTrack: (track: TrackSettings) => invoke("set_track", { track }),
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
  /** `releases`: the release notes, and where a new version is downloaded by hand. */
  openLink: (link: "site" | "coffee" | "releases") => invoke("open_link", { link }),
  updateState: () => invoke<UpdateView>("update_state"),
  /** Asks for a new version now; the answer comes as an `update` event. */
  updateCheck: () => invoke("update_check"),
  /** What a restart for the update would cut off and bring back, as things stand now. */
  updatePlan: () => invoke<RestartPlan>("update_plan"),
  /** Installs the ready update and restarts: never answers when it works, rejects with a short
   * error id (`download`, `signature`, `install`) when it doesn't. */
  updateInstall: () => invoke("update_install"),
  /** Asks the core to read the limits again (`force`: past the background pace; it still throttles). */
  refreshFuel: (force: boolean) => invoke("refresh_fuel", { force }),
  /** The board: every project's cards (the Board page). */
  boardState: () => invoke<Card[]>("board_state"),
  /** `images`: those pasted into the note; the card takes the ones its note names (`[Image #n]`). */
  boardAdd: (path: string, title: string, note: string, images: NewImage[] = []) => invoke<Card>("board_add", { path, title, note, images }),
  /** `images`: those pasted since the card was saved; one its note no longer names is dropped. */
  boardEdit: (id: string, title: string, note: string, images: NewImage[] = []) => invoke("board_edit", { id, title, note, images }),
  /** One of a card's images, as a `data:` URL. */
  boardImage: (id: string, n: number) => invoke<string>("board_image", { id, n }),
  /** `index`: its place among the project's cards in `column`, counted without it. */
  boardMove: (id: string, column: BoardColumn, index: number) => invoke("board_move", { id, column, index }),
  boardDelete: (id: string) => invoke("board_delete", { id }),
  /** Starts a Claude session in a new Pitwall terminal with the card's text (`plan`: in plan mode).
   * `continue`: the session the card holds goes on instead; one that still runs opens no terminal (null). */
  boardGive: (id: string, mode: "new" | "plan" | "continue", size: { cols: number; rows: number } | null = null) =>
    invoke<TerminalView | null>("board_give", { id, mode, cols: size?.cols, rows: size?.rows }),
  boardLink: (id: string, session: string) => invoke("board_link", { id, session }),
  /** Every card of the folder `from` goes to the project `to`, each in its column. */
  boardRehome: (from: string, to: string) => invoke("board_rehome", { from, to }),
  quit: () => invoke("quit"),
};

/** The Track page's settings as just picked, on the page or in Settings: shown at once, the core
 * keeps them for the next opening. */
const trackPicked = ref<TrackSettings | null>(null);

/** The Track page's settings as they show. */
export const track = computed(() => trackPicked.value ?? trackSettings(snapshot.value?.settings.track));

export function setTrack(next: TrackSettings): void {
  trackPicked.value = next;
  // An older core doesn't know the command: the choice then lasts as long as the window.
  void api.setTrack(next).catch(() => undefined);
}
