import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, ref, watch } from "vue";

import type {
  BackupInfo,
  BackupParts,
  BoardColumn,
  Card,
  DaySummary,
  ExportPreview,
  Discovery,
  ImportReport,
  MissingProject,
  Team,
  WhatsNew,
  DepReport,
  DepScanDetails,
  ExtensionStatus,
  Folder,
  Fuel,
  LinkSuggestion,
  NewImage,
  ProjectSettings,
  RadioCall,
  RadioState,
  RadioTool,
  RestartPlan,
  SessionsView,
  Settings,
  ShellChoice,
  SignalKind,
  Snapshot,
  Agent,
  AgentStartChoice,
  SyncReport,
  TerminalLayout,
  TerminalOpened,
  TerminalShells,
  TerminalView,
  UpdateView,
} from "./types";
import { dayName } from "./day";
import { trackSettings, type TrackSettings } from "./track";
import type { PhoneConfig, PhoneView } from "./types";
import type { CalendarDay, DayRow, Lap, LapKind, LapResult, LapRun, LapsState, LapTerminal, LapWhen } from "./laps";

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

/** Snapshots sent from different threads of the core may come in either order: one taken before
 * the one shown is dropped, or the page would go back to an older state (`seq`, snapshot.rs). */
function take(next: Snapshot): void {
  if (snapshot.value && next.seq < snapshot.value.seq) return;
  snapshot.value = next;
}

export async function connect(): Promise<void> {
  await listen<Snapshot>("state", (event) => take(event.payload));
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
  take(await invoke<Snapshot>("get_state"));
  runClock(visible.value);
}

/** Claude's plan limits and today's tokens: the Fuel page's, its sidebar badge's, the popover's
 * strip's and the mini tower's share. */
export const fuel = ref<Fuel | null>(null);

/**
 * The main window's, the popover's and the mini tower's: hears `fuel` (sent to those only) and
 * asks for the current state. A core that doesn't answer leaves it null, which shows as loading (in the
 * popover: no strip).
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

/** The laps (Turlar): every job with its last runs, and the sidebar's count. Null until the core
 * answers. */
export const laps = ref<LapsState | null>(null);

/** `laps` events heard so far: an answer asked for before the latest one is older than it. */
let lapsHeard = 0;

/** Asks the core for the laps; one that doesn't answer (yet) leaves them as they were. */
export async function loadLaps(): Promise<void> {
  const before = lapsHeard;
  try {
    const state = await api.lapsState();
    // An event that came in meanwhile is newer.
    if (lapsHeard === before) laps.value = state;
  } catch {
    // Kept as it was (null shows as loading); the next `laps` event fills it.
  }
}

/**
 * The main window's, from the start (the sidebar counts the jobs that failed): hears `laps` (sent
 * to this window only, after each change of what the page shows) and asks for the current state.
 */
export async function connectLaps(): Promise<UnlistenFn> {
  const unlisten = await getCurrentWindow().listen<LapsState>("laps", (event) => {
    lapsHeard++;
    laps.value = event.payload;
  });
  await loadLaps();
  return unlisten;
}

/** The app's own update: the brand row's version and button, and Settings › About. Null until the
 * core answers. */
export const update = ref<UpdateView | null>(null);

/**
 * The main window's and the popover's: hears `update` (the window's on every change and the
 * download's progress, the popover's on every change of state) and asks for the current state. A
 * core that doesn't answer leaves it null: no version, no button.
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
  pickFolder: () => invoke("pick_folder"),
  /** A folder added from the quick switcher: listed, with no star. */
  addProject: (path: string) => invoke("add_project", { path }),
  pickProjectsDir: () => invoke("pick_projects_dir"),
  clearProjectsDir: () => invoke("clear_projects_dir"),
  projectFolders: () => invoke<Folder[]>("project_folders"),
  daySummary: (date: string | null = null) => invoke<DaySummary>("day_summary", { date }),
  /** The Today page's CSV export of the days `from`..`to` (`YYYY-MM-DD`): what it would hold; the
   *  core's save dialog and the file's path once written (null: cancelled); that file in Finder. */
  dayExportPreview: (from: string, to: string) => invoke<ExportPreview>("day_export_preview", { from, to }),
  dayExport: (from: string, to: string) => invoke<string | null>("day_export", { from, to }),
  dayExportReveal: () => invoke<boolean>("day_export_reveal"),
  playSound: (id: string) => invoke("play_sound", { id }),
  openUrl: (url: string) => invoke("open_url", { url }),
  copyText: (text: string) => invoke("copy_text", { text }),
  linkSuggestions: (path: string) => invoke<LinkSuggestion[]>("link_suggestions", { path }),
  extensionStatus: (editor: string) => invoke<ExtensionStatus>("extension_status", { editor }),
  openExtensionPage: (editor: string) => invoke("open_extension_page", { editor }),
  /** Which of the editors Settings offers are on this machine, in Settings' order. */
  installedEditors: () => invoke<import("./types").Editor[]>("installed_editors"),
  /** Whether the system lets Pitwall post notifications, asked of it now. */
  notificationAccess: () => invoke<import("./types").NotifyAccess>("notification_access"),
  /** One test banner, once the system lets Pitwall post (macOS asks first when it never had an
   * answer, and this waits for it); what it allows then. */
  notificationTest: () => invoke<import("./types").NotifyAccess>("notification_test"),
  /** The system's notification settings, at the entry whose permission it is. */
  notificationSettings: () => invoke("notification_settings"),
  /** `start`: for a Claude tab, the model and the mode picked from "New Claude"'s list; without
   * it, the project's own. `shell`: on Windows, the shell of the list a shell tab runs, and
   * whether an administrator's; without it, Command Prompt there and the user's own shell
   * elsewhere. `account`: the Claude account it runs with ("Start with account"); without it, the
   * one in use. */
  openTerminal: (
    path: string,
    claude = false,
    size: { cols: number; rows: number } | null = null,
    start: AgentStartChoice | null = null,
    shell: ShellChoice | null = null,
    account: string | null = null,
  ) =>
    invoke<TerminalOpened>("open_terminal", {
      path,
      claude,
      cols: size?.cols,
      rows: size?.rows,
      start: start ?? undefined,
      shell: shell?.shell,
      elevated: shell?.elevated,
      account: account ?? undefined,
    }),
  /** The shells "+" offers on Windows (none elsewhere), asked of the system now. */
  terminalShells: () => invoke<TerminalShells>("terminal_shells"),
  /** Windows' settings for sudo (System › For developers). */
  openSudoSettings: () => invoke("open_sudo_settings"),
  /** The panel said that the last quit's or update's administrator's tabs weren't put back. */
  adminNotRestoredSeen: () => invoke("admin_not_restored_seen"),
  renameTerminal: (id: number, name: string) => invoke("rename_terminal", { id, name }),
  writeTerminal: (id: number, data: string) => invoke("write_terminal", { id, data }),
  resizeTerminal: (id: number, cols: number, rows: number) => invoke("resize_terminal", { id, cols, rows }),
  closeTerminal: (id: number) => invoke("close_terminal", { id }),
  /** The terminal panel's height for its panes, kept for the next start. */
  setPanesHeight: (height: number) => invoke("set_panes_height", { height }),
  /** How the panel lays a project's terminals out; null: one at a time. */
  setTerminalLayout: (path: string, layout: TerminalLayout | null) => invoke("set_terminal_layout", { path, layout }),
  terminalBuffer: (id: number) => invoke<{ data: string; seq: number }>("terminal_buffer", { id }),
  setFavourite: (path: string, on: boolean) => invoke("set_favourite", { path, on }),
  /** "Remove from Pitwall": no added project and no favourite any more. */
  removeProject: (path: string) => invoke("remove_project", { path }),
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
  /** The terminal's history: the project's sessions that open again in its terminals, the last
   * heard of first. */
  claudePast: (path: string) => invoke<import("./types").PastSession[]>("claude_past", { path }),
  /** A session of the terminal's history: brought up where it runs, or opened again in a new tab
   * once it has ended (`reveal-terminal` shows it). */
  resumeClaude: (path: string, session: string) => invoke<import("./types").Resumed>("resume_claude", { path, session }),
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
  /** One session's tab looked at: its finished turn is seen, though a prompt elsewhere in its project holds the project's look back. */
  markSessionSeen: (id: string) => invoke("mark_session_seen", { id }),
  setSettings: (settings: Settings) => invoke("set_settings", { settings }),
  setProjectSettings: (path: string, settings: ProjectSettings) => invoke("set_project_settings", { path, settings }),
  /** The folder typed for a project to run in as its settings keep it (`""`: its own); rejects with why it can't be. */
  workFolder: (path: string, folder: string) => invoke<string>("work_folder", { path, folder }),
  /** The same, picked in a dialog; null when it was cancelled. */
  pickWorkFolder: (path: string) => invoke<string | null>("pick_work_folder", { path }),
  reorder: (paths: string[]) => invoke("reorder", { paths }),
  /** The board picked on the Board page: `all`, or a project's path. */
  setBoardScope: (scope: string) => invoke("set_board_scope", { scope }),
  /** How a project's board is drawn: `columns` or `list`. */
  setBoardView: (view: string) => invoke("set_board_view", { view }),
  setFilesPreview: (on: boolean) => invoke("set_files_preview", { on }),
  /** The list's "Only favourites and open". */
  setFavouritesOnly: (on: boolean) => invoke("set_favourites_only", { on }),
  /** The Track page's own settings: its circuit, what the cars carry, how much moves. */
  setTrack: (track: TrackSettings) => invoke("set_track", { track }),
  setShortcut: (keys: string) => invoke("set_shortcut", { keys }),
  suspendShortcut: () => invoke("suspend_shortcut"),
  resumeShortcut: () => invoke("resume_shortcut"),
  showSwitcher: () => invoke("show_switcher"),
  output: (path: string, job: string | null = null) => invoke<string[]>("get_output", { path, job }),
  runCommand: (path: string, id: string) => invoke("run_command", { path, id }),
  stopCommand: (path: string, id: string) => invoke("stop_command", { path, id }),
  /** A command that started with its server and waits for the user, skipped: nothing runs. */
  skipHeld: (path: string, id: string) => invoke("skip_held", { path, id }),
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

  /* Claude accounts (`fuel.accounts`; events `account-login`, `account-sync`: lib/accounts). */
  /** New Claude sessions Pitwall starts use it from now on; with its sync on a sync starts, with it
   * off what differs is told (`account-sync`). Rejects with a message. */
  accountUse: (id: string) => invoke("account_use", { id }),
  /** Any account's, the main one's too (an empty name gives it back its default). */
  accountRename: (id: string, name: string) => invoke("account_rename", { id, name }),
  /** Not the main one: Pitwall takes back what it added to the folder; the folder and its
   * sign-in stay. */
  accountRemove: (id: string) => invoke("account_remove", { id }),
  /** "Sync when switching". */
  accountSetSync: (id: string, on: boolean) => invoke("account_set_sync", { id, on }),
  /** Its folder in Finder. */
  accountReveal: (id: string) => invoke("account_reveal", { id }),
  /** The folder a new account of this name gets; `exists`: one is there already, used as it is. */
  accountFolder: (name: string) => invoke<{ shown: string; exists: boolean }>("account_folder", { name }),
  /** The system's folder dialog: null when cancelled; rejects with a message for a folder that
   * can't be an account's. */
  accountPickFolder: () => invoke<{ path: string; shown: string } | null>("account_pick_folder"),
  /** Starts adding one: validates (rejects with a message), then Claude Code's own sign-in opens
   * in the browser and goes on as `account-login` events. `dir`: a `path` from
   * `accountPickFolder`, null for the new folder by its name. */
  accountLogin: (name: string, dir: string | null, shared: boolean, sync: boolean) => invoke("account_login", { name, dir, shared, sync }),
  /** Stops the sign-in under way: nothing is added. */
  accountLoginCancel: () => invoke("account_login_cancel"),
  /** Brings its MCP servers level with the main account's now ("Sync", "Try again"). */
  accountSync: (id: string) => invoke<SyncReport>("account_sync", { id }),
  /** What differs, writing nothing ("Details"). */
  accountSyncPlan: (id: string) => invoke<SyncReport>("account_sync_plan", { id }),
  /** When each account's sign-in ends, as Claude Code says it (from 2.1.318; none before): only
   * the accounts it answered for, kept by the core for 10 minutes. */
  claudeLoginEnds: () => invoke<import("./types").LoginEnd[]>("claude_login_ends"),
  /** Claude Code's own sign-in again for the account (the main one too), in the browser; it goes
   * on as `account-renew` events and is stopped by `accountLoginCancel`. Rejects with a message. */
  accountRenew: (id: string) => invoke("account_renew", { id }),
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
   * `continue`: the session the card holds goes on instead; one that still runs opens no terminal (null).
   * `start`: the model and the mode chosen for a new session; without it, the project's own. */
  boardGive: (id: string, mode: "new" | "plan" | "continue", size: { cols: number; rows: number } | null = null, start: AgentStartChoice | null = null) =>
    invoke<TerminalView | null>("board_give", { id, mode, cols: size?.cols, rows: size?.rows, start: start ?? undefined }),
  boardLink: (id: string, session: string) => invoke("board_link", { id, session }),
  /** Every card of the folder `from` goes to the project `to`, each in its column. */
  boardRehome: (from: string, to: string) => invoke("board_rehome", { from, to }),

  /* Teams (core/teams.rs). Every change comes back in the next snapshot's `settings.teams`. */
  /** A new team, last in the list, with these projects (each leaves the team it was in). */
  teamCreate: (name: string, projects: string[] = []) => invoke<Team>("team_create", { name, projects }),
  teamRename: (id: string, name: string) => invoke("team_rename", { id, name }),
  /** The team goes; its projects stay listed, in no team. */
  teamDelete: (id: string) => invoke("team_delete", { id }),
  /** The team's projects, in order: reorders them, adds one (it leaves the team it was in) or
   * drops one (it is in no team then). */
  teamProjects: (id: string, projects: string[]) => invoke("team_projects", { id, projects }),
  /** The teams' own order. */
  teamOrder: (ids: string[]) => invoke("team_order", { ids }),
  /** Whether its servers start when Pitwall does. */
  teamLaunch: (id: string, on: boolean) => invoke("team_launch", { id, on }),
  /** Starts its projects that don't run, one after the other in the team's order: the next
   * once the one before answers on its address. */
  teamStart: (id: string) => invoke("team_start", { id }),
  teamStop: (id: string) => invoke("team_stop", { id }),

  /* The first run and the projects folder (core/discover.rs). */
  /** The projects folder's subfolders that have the dev script. */
  discoverProjects: () => invoke<Discovery>("discover_projects"),
  addProjects: (paths: string[]) => invoke("add_projects", { paths }),
  /** A project offered under the list isn't offered again. */
  dismissSuggestion: (path: string) => invoke("dismiss_suggestion", { path }),
  /** The first run's steps are over (finished or put off). */
  setOnboarded: () => invoke("set_onboarded"),

  /* What's new (whatsnew.rs). */
  whatsNew: () => invoke<WhatsNew>("whats_new"),
  /** The notes were shown: they don't come up by themselves again. */
  whatsNewSeen: () => invoke("whats_new_seen"),

  /* What was open at the last quit (core/resume.rs): `snapshot.resume`. */
  /** Puts it back. `always`: from now on without asking (Settings' `resume`). */
  resumeRestore: (always = false) => invoke("resume_restore", { always }),
  /** Starts empty; the offer is gone. */
  resumeDismiss: () => invoke("resume_dismiss"),

  /* Backup and moved folders (core/backup.rs). Files are picked and written by the core. */
  /** Asks where to save and writes the backup there. Resolves with the file, or null when the
   * dialog was cancelled. */
  backupExport: (parts: BackupParts) => invoke<string | null>("backup_export", { parts }),
  /** Asks for a backup file and reads what it holds; null when the dialog was cancelled. The
   * core remembers the file for `backupImport`. */
  backupPick: () => invoke<BackupInfo | null>("backup_pick"),
  /** Puts the picked file's parts back (what is here is kept beside it first). */
  backupImport: (parts: BackupParts) => invoke<ImportReport>("backup_import", { parts }),
  /** Projects whose folder is gone, with what is still kept for them. */
  missingProjects: () => invoke<MissingProject[]>("missing_projects"),
  /** Asks for the folder's new place and moves everything kept for `from` there. Resolves
   * with the new path, or null when the dialog was cancelled. */
  rehomeProject: (from: string) => invoke<string | null>("rehome_project", { from }),
  /** Drops what is kept for a project whose folder is gone (its cards stay on the board). */
  forgetProject: (path: string) => invoke("forget_project", { path }),

  quit: () => invoke("quit"),

  /* The phone's channel (core/relay.rs). Its settings are no part of the snapshot. */
  /** The channel as it is set. */
  phoneConfig: () => invoke<PhoneView>("phone_config"),
  /** Keeps the form; resolves with the channel as stored. */
  phoneSet: (config: PhoneConfig) => invoke<PhoneView>("phone_set", { config }),
  /** Sends a test message and waits for the channel's answer; rejects with why it didn't arrive. */
  phoneTest: () => invoke("phone_test"),
  /** A fresh ntfy topic. */
  phoneTopic: () => invoke<string>("phone_topic"),

  /* Notifications (core/signals.rs): `snapshot.quiet`, `snapshot.missed`, a project's `muted`. */
  /** Quiet from now on: for an hour, until tomorrow morning, or until it is switched off. */
  quietStart: (span: "hour" | "morning" | "open") => invoke("quiet_start", { span }),
  quietEnd: () => invoke("quiet_end"),
  /** `questions`: questions and permissions still come through; `sounds`: the sounds are held too. */
  quietOption: (option: "questions" | "sounds", on: boolean) => invoke("quiet_option", { option, on }),
  /** The quiet hours: on or off, from when to when (`HH:MM`). */
  quietHours: (on: boolean, from: string, to: string) => invoke("quiet_hours", { on, from, to }),
  /** Whether an event brings a notification. */
  notifyEvent: (event: "claude" | "serverCrashed" | "command" | "longTurn" | "deps" | "fuel", on: boolean) => invoke("notify_event", { event, on }),
  /** After how many minutes one turn is told of. */
  notifyLongTurn: (minutes: number) => invoke("notify_long_turn", { minutes }),
  muteProject: (path: string, muted: boolean) => invoke("mute_project", { path, muted }),
  /** Opens what a missed notification was about; its line goes. `agent`: whose line it is. */
  missedOpen: (path: string, kind: SignalKind, agent: import("./types").Agent = "claude") => invoke("missed_open", { path, kind, agent }),
  missedClear: () => invoke("missed_clear"),

  /* The team radio (core/radio): Claude Code asks Pitwall. */
  /** The tools there are, in the core's order: answered at once. */
  radioTools: () => invoke<RadioTool[]>("radio_tools"),
  /** The radio as Claude Code has it; the core asks its CLI, so it takes a moment. */
  radioState: () => invoke<RadioState>("radio_state"),
  /** Turns it on (Pitwall is added to Claude Code's MCP servers) or off (it is taken out). */
  radioSet: (on: boolean) => invoke<RadioState>("radio_set", { on }),
  radioTool: (name: string, on: boolean) => invoke("radio_tool", { name, on }),
  /** Today's calls of a session, the newest first. */
  radioCalls: (session: string) => invoke<RadioCall[]>("radio_calls", { session }),

  /* The output pane (core/reference.rs). */
  /** Opens the file an output line names (`file`, as printed) in the editor, at its line. The
   * core finds it, in the project's folder only: false when the project has no such file.
   * `ownFolder`: the Files window's, a path from the project's own folder, never its working folder. */
  openReference: (path: string, file: string, line: number, column: number | null, ownFolder = false) =>
    invoke<boolean>("open_reference", { path, file, line, column, ownFolder }),
  /** The Files window (lib/explorer): a folder of the project (`""` its root), at most `limit` entries. */
  explorerList: (path: string, dir: string, limit: number) => invoke<import("./explorer").Listing | null>("explorer_list", { path, dir, limit }),
  /** A file of the project: its text, or why it isn't shown; null when it can't be read. */
  explorerRead: (path: string, file: string) => invoke<import("./explorer").FileView | null>("explorer_read", { path, file }),
  /** A picture a Markdown file names, as a `data:` URL; null when it isn't one of the project's. */
  explorerImage: (path: string, file: string) => invoke<string | null>("explorer_image", { path, file }),
  /** The Git state of the project's paths; null when it isn't a Git work tree. */
  explorerStatus: (path: string) => invoke<import("./explorer").GitMark[] | null>("explorer_status", { path }),
  /** What changed in a file since the last commit, as `git diff` writes it. */
  explorerDiff: (path: string, file: string) => invoke<string | null>("explorer_diff", { path, file }),
  /** The project's files, for the window's search. */
  explorerFiles: (path: string) => invoke<{ files: string[]; partial: boolean } | null>("explorer_files", { path }),
  /** The history of the project's branch and its upstream, `limit` commits after `skip`. */
  explorerLog: (path: string, skip: number, limit: number) => invoke<import("./explorer").History | null>("explorer_log", { path, skip, limit }),
  /** The files a commit of the project changed. */
  explorerCommit: (path: string, id: string) => invoke<import("./explorer").CommitFile[] | null>("explorer_commit", { path, id }),
  /** What a commit changed in one file (`from`: where a renamed one was). */
  explorerCommitDiff: (path: string, id: string, file: string, from: string | null) => invoke<string | null>("explorer_commit_diff", { path, id, file, from }),
  /** A file or folder of the project shown in Finder. */
  explorerReveal: (path: string, file: string) => invoke<boolean>("explorer_reveal", { path, file }),
  /* The chequered flag (core/flag.rs): what is still open, before a quit and from Today's bar. */
  /** What is open, and whether a quit waits on the dialog. `fresh`: the projects' Git state is
   * read again first, so the answer takes a moment. */
  flagState: (fresh = false) => invoke<import("./types").FlagState>("flag_state", { fresh }),
  /** Whether a quit from Pitwall's own menus shows what is still open first. */
  flagSet: (on: boolean) => invoke("flag_set", { on }),
  /** The dialog is up for the quit that was held. */
  flagShown: () => invoke("flag_shown"),
  /** The held quit is given up: Pitwall stays, and the next quit asks again. */
  flagKeep: () => invoke("flag_keep"),
  /** The held quit goes on. */
  flagQuit: () => invoke("flag_quit"),
  /** Every server and every running command stops; each shows in the state until it is down. */
  flagStopAll: () => invoke("flag_stop_all"),
  /** The project shown in the list, for the new shell tab its terminal panel then opens. */
  flagTerminal: (path: string) => invoke("flag_terminal", { path }),
  /** The window's Board page. */
  flagBoard: () => invoke("flag_board"),

  /* The mini tower (tower.rs). Every change comes back in the next snapshot's `settings.tower`. */
  /** Shows or hides it. */
  towerShow: (on: boolean) => invoke("tower_show", { on }),
  /** `hideStopped`: without the projects that neither run nor have Claude in them; `allSpaces`:
   * on every desktop; `shortcut`: its global shortcut (rejects when its keys are taken). */
  towerOption: (option: "hideStopped" | "allSpaces" | "shortcut", on: boolean) => invoke("tower_option", { option, on }),
  towerForm: (form: "list" | "strip") => invoke("tower_form", { form }),
  /** New keys for its shortcut, which is on from then; rejects when they can't be registered. */
  towerShortcut: (keys: string) => invoke("tower_shortcut", { keys }),
  /** The tower's own page: how large its content is. Its window takes that size and comes up. */
  towerFit: (width: number, height: number) => invoke("tower_fit", { width, height }),
  /* The command line (core/cli): the `pitwall` command and pitwall:// links. */
  /** Where the `pitwall` command is; the core asks the shell for its PATH, so it takes a moment. */
  cliState: () => invoke<import("./types").CliState>("cli_state"),
  /** Puts the command on the shell's PATH, as a link to this app. */
  cliInstall: () => invoke<import("./types").CliState>("cli_install"),
  /** Takes it out again. */
  cliRemove: () => invoke<import("./types").CliState>("cli_remove"),
  /** Whether `pitwall://` links are answered. */
  cliLinks: (on: boolean) => invoke("cli_links", { on }),
  /** Puts the link that does `action` to the project at `path` on the clipboard. */
  cliCopyLink: (path: string, action: import("./types").LinkAction) => invoke("cli_copy_link", { path, action }),
  /** The listed project looked at last (the seen book's latest look), or null: where the quick
   * switcher's `+` offers a new card first. */
  lastLooked: () => invoke<string | null>("last_looked"),
  /** Tells a card up next to wait until the 5-hour session refills (the core's own reading of
   * that moment): the core gives it then, with `start`. */
  boardSchedule: (id: string, start: AgentStartChoice) => invoke("board_schedule", { id, start }),
  /** The card waits no longer; it stays up next. */
  boardUnschedule: (id: string) => invoke("board_unschedule", { id }),
  /** "Give now" on a card that waits: given at once with what was chosen for it; null when it
   * no longer waits (the core gave it a moment ago). */
  boardGiveNow: (id: string, size: { cols: number; rows: number } | null = null) => invoke<TerminalView | null>("board_give_now", { id, cols: size?.cols, rows: size?.rows }),
  /** Pitwall's hook in Codex's `hooks.json`; Codex runs it once the user trusted it in `/hooks`. */
  installCodexHook: () => invoke("install_codex_hook"),
  uninstallCodexHook: () => invoke("uninstall_codex_hook"),
  /** "Check": what Codex itself says of Pitwall's lines in `hooks.json`, trusted and on out of
   * all; once it runs them all, the hook reads On. */
  checkCodexHook: () => invoke<{ trusted: number; total: number }>("check_codex_hook"),
  /** The terminal's history of Codex: the project's sessions `codex resume` finds. */
  codexPast: (path: string) => invoke<import("./types").PastSession[]>("codex_past", { path }),
  /** A Codex session of the terminal's history brought up where it runs, or opened again in a new
   * tab (`codex resume`) once it has ended, as `resumeClaude` is Claude's. */
  resumeCodex: (path: string, session: string) => invoke<import("./types").Resumed>("resume_codex", { path, session }),
  /* An agent's accounts (Codex's: `fuel.codex.accounts`; events `codex-account-login`,
     `codex-account-sync`: lib/accounts). With "claude" these are the calls above. */
  agentAccountUse: (agent: import("./types").Agent, id: string) => invoke("account_use", { id, agent }),
  agentAccountRename: (agent: import("./types").Agent, id: string, name: string) => invoke("account_rename", { id, name, agent }),
  agentAccountRemove: (agent: import("./types").Agent, id: string) => invoke("account_remove", { id, agent }),
  agentAccountSetSync: (agent: import("./types").Agent, id: string, on: boolean) => invoke("account_set_sync", { id, on, agent }),
  agentAccountReveal: (agent: import("./types").Agent, id: string) => invoke("account_reveal", { id, agent }),
  agentAccountFolder: (agent: import("./types").Agent, name: string) => invoke<{ shown: string; exists: boolean }>("account_folder", { name, agent }),
  agentAccountPickFolder: (agent: import("./types").Agent) => invoke<{ path: string; shown: string } | null>("account_pick_folder", { agent }),
  /** Codex's sign-in is `codex login` with the account's own `CODEX_HOME`. */
  agentAccountLogin: (agent: import("./types").Agent, name: string, dir: string | null, shared: boolean, sync: boolean) => invoke("account_login", { name, dir, shared, sync, agent }),
  agentAccountSync: (agent: import("./types").Agent, id: string) => invoke<import("./types").AgentSyncReport>("account_sync", { id, agent }),
  agentAccountSyncPlan: (agent: import("./types").Agent, id: string) => invoke<import("./types").AgentSyncReport>("account_sync_plan", { id, agent }),
  /** Turns an agent on or off at the user's word; a look finds it again for nothing after. */
  agentSetOn: (agent: import("./types").Agent, on: boolean) => invoke("agent_set_on", { agent, on }),
  /** Where an agent's fuel shows (not Claude's: Settings' form has it); one left out stays. */
  agentSetFuel: (agent: import("./types").Agent, where: { bar?: boolean; popover?: boolean }) => invoke("agent_set_fuel", { agent, bar: where.bar ?? null, popover: where.popover ?? null }),
  /** The user was told that an agent was found and turned on: the notice closes for good. */
  agentTold: (agent: import("./types").Agent) => invoke("agent_told", { agent }),
  /** Looks for the agents' home folders again; true when that changed what is found. */
  lookForAgents: () => invoke<boolean>("look_for_agents"),
  /** Each agent's CLI version and home folder, for Settings › Agents. */
  agentDetails: () => invoke<import("./types").AgentDetail[]>("agent_details"),
  /** Codex's team radio, as `radioState` / `radioSet` are Claude's. */
  codexRadioState: () => invoke<RadioState>("codex_radio_state"),
  codexRadioSet: (on: boolean) => invoke<RadioState>("codex_radio_set", { on }),
  /* The laps (Turlar): the jobs, their runs and the calendar (core/laps; event `laps`). */
  lapsState: () => invoke<LapsState>("laps_state"),
  /** Saves a job (a new one when its id is empty), checked whole by the core: the job saved, or
   * why not. */
  lapSave: (lap: Lap) => invoke<Lap>("lap_save", { lap }),
  lapDelete: (id: string) => invoke("lap_delete", { id }),
  /** The job's switch (an ended job comes back on). */
  lapSwitch: (id: string, on: boolean) => invoke("lap_switch", { id, on }),
  /** Duraklat (true) / Sürdür (false). */
  lapPause: (id: string, paused: boolean) => invoke("lap_pause", { id, paused }),
  /** Şimdi çalıştır: a run now, off the schedule. */
  lapRunNow: (id: string) => invoke("lap_run_now", { id }),
  /** Bu seferi atla: the next moment becomes a skipped run. */
  lapSkipNext: (id: string) => invoke("lap_skip_next", { id }),
  lapStop: (id: string) => invoke("lap_stop", { id }),
  /** A day off: `date` alone, or (`every`) its weekday from now on. */
  lapDayOff: (id: string, date: string, every: boolean) => invoke("lap_day_off", { id, date, every }),
  /** "geri aç": `date`'s day off, or its weekday's, is taken back. */
  lapDayOn: (id: string, date: string) => invoke("lap_day_on", { id, date }),
  /** The next four moments of `when` for a job of `kind`, or why it is refused. */
  /** The next moments of `when`; a refill's as the reading of `account` (null: the one in use) has them. */
  lapPreview: (when: LapWhen, kind: LapKind, agent: Agent, account: string | null) => invoke<number[]>("lap_preview", { when, kind, agent, account }),
  lapCalendar: (year: number, month: number) => invoke<CalendarDay[]>("lap_calendar", { year, month }),
  lapDay: (date: string) => invoke<DayRow[]>("lap_day", { date }),
  /** A job's kept runs, newest first. */
  lapRuns: (id: string) => invoke<LapRun[]>("lap_runs", { id }),
  /** The first line of what the agent last said in each of `runs`, read now, by run. */
  lapSummaries: (runs: string[]) => invoke<Record<string, string>>("lap_summaries", { runs }),
  /** A run's kept script output and what its agent last said, read now. */
  lapResult: (run: string) => invoke<LapResult>("lap_result", { run }),
  /** A run's terminal for its dialog: the laps' own tab its agent runs in, its session opened
   * again in a new one once it has ended (Oturumu aç), or why there is none. */
  lapTerminal: (run: string, size: { cols: number; rows: number } | null = null) => invoke<LapTerminal>("lap_terminal", { run, cols: size?.cols, rows: size?.rows }),
  /** The run's dialog let go of its terminal: closed by the core once its agent has left. */
  lapTerminalLeft: (id: number) => invoke("lap_terminal_left", { id }),
  /** A run's result was opened: it is read from now on (one read already, or gone, is no error). */
  lapRead: (run: string) => invoke("lap_read", { run }),
  /** The jobs in the order the tower was dragged into: those named first, as named, the rest
   * after them as they stood. */
  lapReorder: (ids: string[]) => invoke("lap_reorder", { ids }),
  /** "Klasör seç…" for a job in a folder: the folder picked (absolute, and as shown), or null. */
  lapPickFolder: () => invoke<{ path: string; shown: string } | null>("lap_pick_folder"),
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
