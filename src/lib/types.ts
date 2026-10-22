// Mirrors the Rust `Snapshot` (src-tauri/src/core.rs).

import type { Language } from "./i18n";

export type Issue = { kind: "crashed" | "error" | "unresponsive"; text: string };

export type Turn = { kind: "finished" | "asking" | "permission"; at: number };

export type GitInfo = { branch: string; changes: number; ahead: number; behind: number };

export type CustomCommand = {
  id: string;
  name: string;
  command: string;
  /** A worker or watcher: restarted if it crashes. */
  keepRunning: boolean;
  /** Ask before running. */
  confirm: boolean;
  /** Started and stopped with the dev server. */
  withServer: boolean;
};

/** One of a project's other addresses: staging, production, the admin panel, the issue tracker… */
export type ProjectLink = { name: string; url: string };

/** Pitwall for VS Code in the chosen editor: its version when installed; where it installs from. */
export type ExtensionStatus = { version: string | null; marketplace: boolean };

/** The editors Settings offers (`settings.editor`). */
export type Editor = "vscode" | "vscode-insiders" | "cursor" | "windsurf";

/** Whether the system lets Pitwall post notifications (notify.rs: `access`): `notAsked` while its
 * question has no answer; `borrowed` from a dev build, which posts as another app whose
 * permission it is; `unknown` where the system isn't asked. */
export type NotifyAccess = "allowed" | "off" | "notAsked" | "borrowed" | "unknown";

/** An address the project names itself, offered as a link; `source`: `git`, `.env`, `package.json`. */
export type LinkSuggestion = { name: string; url: string; source: string };

export type ProjectSettings = {
  /** The folder inside the project its dev server and commands run in, written from the
   * project's folder (`apps/web`); unset: the project's own. */
  folder?: string;
  script?: string;
  port?: number;
  url?: string;
  commands?: CustomCommand[];
  links?: ProjectLink[];
  /** The model a Claude session started here takes; unset: the one Claude Code picks itself. */
  claudeModel?: ClaudeModel;
  /** The mode such a session starts in; unset: normal. */
  claudeMode?: ClaudeMode;
  /** The three letters the Track page and the mini tower write for it; unset: told from its name. */
  code?: string;
  /** Where it opens from each place, in place of Pitwall's choice (`Settings.open`); a place
   * left out takes Pitwall's. Read leniently (lib/open). */
  open?: Partial<OpenSettings>;
};

/** What opening a project opens: its Files window, its editor, or the project in Pitwall's list. */
export type OpenTarget = "files" | "editor" | "list";

/** Where a project opens from each place (Settings › Projects, lib/open): a double click or ↵ on
 * its row in the list (`files` or `editor`), a double click on it on the window's other pages, and
 * the menu bar's popover, the quick switcher and the mini tower. */
export type OpenSettings = { list: OpenTarget; pages: OpenTarget; outside: OpenTarget };

export type JobResult = { ok: boolean; code: number | null; stopped: boolean; finishedAt: number };

/** Who started a server whose command marked "ask first" didn't run with it: `user` (a click)
 * waits for the go-ahead; the others skipped it, each saying when. */
export type HeldBy = "user" | "launch" | "restore" | "link" | "claude" | "crash" | "noTerminal";

export type CommandView = CustomCommand & {
  status: "running" | "busy" | "ok" | "failed" | "idle";
  startedAt: number | null;
  result: JobResult | null;
  /** Marked "ask first", it started with the server and wasn't run: it waits for the user. */
  held: HeldBy | null;
};

export type Status = "running" | "stopped" | "crashed" | "busy";

/** A team: projects listed together, started and stopped together. A project is in one at most. */
export type Team = {
  id: string;
  name: string;
  /** Its projects' paths, in the order they are listed and started. */
  projects: string[];
  /** Folded in the list: its head alone. */
  collapsed: boolean;
  /** Its servers start when Pitwall does. */
  startAtLaunch: boolean;
};

/** How the app is drawn: as the system is, dark, or light. */
export type Theme = "system" | "dark" | "light";

export type Project = {
  path: string;
  name: string;
  favourite: boolean;
  status: Status;
  /** While busy: what is happening. */
  phase: "starting…" | "stopping…" | "restarting…" | null;
  port: number | null;
  url: string | null;
  startedAt: number | null;
  issue: Issue | null;
  /** The VS Code window that runs it, when it isn't this app. */
  owner: { id: string; title: string } | null;
  /** The editor window that has it open as a root folder. */
  openIn: string | null;
  claude: Turn | null;
  /** Claude is mid-turn here right now. */
  claudeWorking: boolean;
  /** Its Claude sessions that are working, or wait on you (notifications not read yet). `since`:
   * when a working one began, as the hook told it; `tool`: the tool a permission wait is for. */
  claudeSessions: Array<{ id: string; phase: "working" | "waiting"; turn: Turn | null; since?: number; tool?: string }>;
  git: GitInfo | null;
  script: string;
  /** The exact command the dev server runs: `npm run dev`, `pnpm dev`, `bun run dev`… */
  runCommand: string;
  settings: ProjectSettings;
  commands: CommandView[];
  /** Open terminals in this project. */
  terminals: TerminalView[];
  /** Nothing is told about it: no notification, no sound (Settings › Notifications). */
  muted: boolean;
};

export type Settings = {
  /** `system` or a language code. */
  language: "system" | Language;
  script: string;
  packageManager: string;
  editor: string;
  openUrlOnStart: boolean;
  notify: boolean;
  /** Notify when the session or weekly limit passes 90 %. */
  fuelAlert: boolean;
  /** The session's fuel left in the menu bar, after the counts. */
  fuelInBar: boolean;
  /** The limits' dials in the popover, in its foot. */
  fuelInPopover: boolean;
  /** Minutes between asking Anthropic for the limits: 1, 5 or 15. */
  fuelEvery: number;
  /** The count of running dev servers in the menu bar, in green. */
  serversInBar: boolean;
  /** The count of Claude sessions at work in the menu bar, in orange. */
  claudesInBar: boolean;
  /** Which of Pitwall's sounds each event plays (src-tauri/sounds); "" for none. */
  sounds: { claudeFinished: string; claudeAsking: string; serverCrashed: string; serverReady: string; commandDone: string };
  launchAtLogin: boolean;
  shortcut: boolean;
  /** Global-hotkey form: `Ctrl+Alt+KeyP`, `Alt+Space`. */
  shortcutKeys: string;
  /** Ask for new versions and download them without being asked (Settings › About still can). */
  autoUpdate: boolean;
  projects: Record<string, ProjectSettings>;
  /** The list order the user dragged into place. */
  order: string[];
  /** Searched by the quick switcher when no project matches. */
  projectsDir?: string;
  /** The board the Board page showed last: `all`, or a project's path. */
  boardScope?: string;
  /** How a project's board is drawn: `columns` (side by side) or `list` (one under the other). */
  boardView?: string;
  /** The Files window shows a Markdown or SVG file as its preview, until its eye is pressed again. */
  filesPreview?: boolean;
  /** The Track page's own settings (lib/track: TrackSettings), read leniently. */
  track?: { circuit?: string; labels?: string; motion?: string; glow?: boolean };
  /** How the app is drawn (lib/theme). */
  theme: Theme;
  /** The list's teams, in the order they are drawn; changed through the `team*` commands only. */
  teams?: Team[];
  /** What was open at the last quit, at the next start: asked for, put back, or left. */
  resume: "ask" | "always" | "never";
  /** The newest version whose notes were shown. */
  seenVersion?: string;
  /** The first run's steps were finished or put off. */
  onboarded: boolean;
  /** A project that appears in the projects folder is offered under the list. */
  suggestProjects: boolean;
  /** The projects folder's subfolders known so far; the core's own. */
  seenFolders?: string[];
  /** What is told, the quiet hours and the muted projects; changed through their own commands
   * only (lib/quiet). */
  notifications?: NotificationSettings;
  /** The team radio; changed through the `radio*` commands only. */
  radio?: RadioSettings;
  /** The chequered flag; changed through its own command only. */
  flag?: FlagSettings;
  /** The mini tower; changed through its own commands only. */
  tower?: TowerSettings;
  /** The command line and `pitwall://` links; changed through their own commands only. */
  cli?: CliSettings;
  /** Where a project opens from each place, unless it chose its own. Read leniently (lib/open). */
  open?: Partial<OpenSettings>;
};

/** The chequered flag: a quit from Pitwall's own menus shows what is still open first. */
export type FlagSettings = { atQuit: boolean };

/** The mini tower: shown or not, a row per project or one row of codes, without the projects
 * that neither run nor wait, on every desktop; its own global shortcut and that shortcut's keys;
 * and where it was left (the core's own). */
export type TowerSettings = {
  on: boolean;
  form: "list" | "strip";
  hideStopped: boolean;
  allSpaces: boolean;
  shortcut: boolean;
  /** Global-hotkey form: `Ctrl+Alt+KeyK`. */
  shortcutKeys: string;
  place?: [number, number, number, number];
};

/** The command line: its line is open (the `pitwall` command is installed), and `pitwall://`
 * links are answered. */
export type CliSettings = { on: boolean; links: boolean };

/** An open project terminal. */
export type TerminalView = {
  id: number;
  path: string;
  name: string;
  kind: "shell" | "claude";
  /** Opened by a board card's moment, not by the user: it takes neither the view nor the keyboard. */
  unasked: boolean;
};

/** A subfolder of the projects folder. */
export type Folder = { name: string; path: string };

export type Snapshot = {
  /** Its place among the snapshots the core took: a window keeps the latest it has seen. */
  seq: number;
  projects: Project[];
  running: number;
  waiting: number;
  crashed: number;
  crashUnseen: boolean;
  /** The Claude Code Notification hook is installed. */
  claudeHook: boolean;
  /** Installed, but older than the current hook set (which reports Claude's state instantly). */
  claudeHookOutdated: boolean;
  /** The board's file couldn't be written: changes to its cards last only while the app runs. */
  boardUnsaved: boolean;
  settings: Settings;
  /** The language to speak: Settings' choice, else the system's. */
  language: Language;
  /** The system's language, for the "System" choice in Settings. */
  systemLanguage: Language;
  /** What was open when Pitwall last quit, while it is still to be answered (Settings' `resume`
   * is `ask`); null otherwise. */
  resume: ResumeOffer | null;
  /** This version's notes haven't been shown yet (after an update). */
  whatsNew: boolean;
  /** Projects that appeared in the projects folder, offered under the list. */
  suggestions: Candidate[];
  /** It is quiet now (a period the user started, or the quiet hours); null otherwise. */
  quiet: Quiet | null;
  /** What the user wasn't told meanwhile, newest first. */
  missed: Missed[];
  /** How many calls came over the team radio: when it moves, a session's calls are asked for again. */
  radioCalls?: number;
};

/* ---------- First run and the projects folder (discover_projects, add_projects) ---------- */

/** A folder of the projects folder that could be a project: it has the dev script. `kind`: what
 * it is built with, a proper name ("Laravel + Vite", "Next", "Vite"…); `changedAt`: when it was
 * last worked on (ms), null when that can't be told; `listed`: already in Pitwall. */
export type Candidate = { path: string; name: string; kind: string; changedAt: number | null; listed: boolean };

/** The projects folder looked through. `dir` null: none is set; `folders`: how many subfolders
 * it has in all. */
export type Discovery = { dir: string | null; folders: number; candidates: Candidate[] };

/* ---------- What's new (whats_new) ---------- */

/** One thing this version brought, in the language shown: something new, or a fix. */
export type ReleaseNote = { kind: "new" | "fix"; title: string; body: string | null };

/** This version's notes, written for each release in every language (src/whats-new.json).
 * `pending`: they haven't been shown yet; `date`: the release's day (`YYYY-MM-DD`), when it has
 * one. No `notes`: there are none for this build, and nothing is to be shown or offered. */
export type WhatsNew = { current: string; date: string | null; pending: boolean; notes: ReleaseNote[] };

/* ---------- Resume: what was open at the last quit (snapshot.resume) ---------- */

/** One project of the last quit: its dev server ran, the commands that ran (their names), and
 * how many terminal tabs it had. */
export type ResumeProject = { path: string; name: string; server: boolean; commands: string[]; terminals: number };

/** What was open when Pitwall last quit (`quitAt`, ms). */
export type ResumeOffer = { quitAt: number; projects: ResumeProject[] };

/* ---------- Backup and moved folders (backup_*, missing_projects, rehome_project) ---------- */

/** What a backup takes, or an import puts back. */
export type BackupParts = { settings: boolean; projects: boolean; board: boolean; activity: boolean };

/** A backup file just picked: what it holds. `app`: the version that wrote it. A part that is
 * missing from the file is 0 or false. */
export type BackupInfo = {
  file: string;
  createdAt: number;
  app: string;
  settings: boolean;
  /** Listed projects, and how many of them carry settings, commands or links. */
  projects: number;
  commands: number;
  links: number;
  teams: number;
  cards: number;
  images: number;
  /** Days of the timeline. */
  days: number;
};

/** What an import put back. */
export type ImportReport = { projects: number; cards: number; days: number; settings: boolean };

/** A project whose folder is gone, and what Pitwall still keeps for it. */
export type MissingProject = { path: string; name: string; commands: number; links: number; cards: number; listed: boolean };

/** A stretch of the day; Claude's name their session. */
export type DaySpan = { start: number; end: number; session?: string };

/** A Claude session's part in the day: when, how long it worked and waited on you, its turns. */
export type DaySession = { id: string; title: string | null; start: number; end: number; work: number; wait: number; turns: number };

export type DayCommit = { at: number; subject: string };

export type DayCommand = { name: string; runs: number; failed: number };

/** Claude's tokens in a day, and the share of the week's limit they took, in percent: null before
 * the limit was read that day. `estimated`: part of the share was worked out from the tokens. */
export type DaySpend = { tokens: number; week: number | null; estimated: boolean };

/** A project's part of them. */
export type ProjectSpend = { tokens: number; week: number | null };

export type DayProject = {
  path: string;
  name: string;
  work: DaySpan[];
  wait: DaySpan[];
  server: DaySpan[];
  crashes: number[];
  sessions: DaySession[];
  commits: DayCommit[];
  commands: DayCommand[];
  /** Null when nothing is known of its tokens that day. */
  spend: ProjectSpend | null;
};

/** One day: what Pitwall wrote down while it ran, and the commits made; its tokens, null for a day
 * before they were written down. */
export type DaySummary = { date: string; start: number; end: number; today: boolean; now: number; projects: DayProject[]; spend: DaySpend | null };

/* ---------- Fuel: Claude's plan limits (src-tauri/src/usage.rs) and today's spend (spend.rs) ---------- */

/** One limit. `five_hour` is the session, `seven_day` the week across models; then each model's
 * week (`seven_day_opus`, `seven_day_sonnet`…) and others the API adds. */
export type UsageWindow = {
  id: string;
  /** The model's name as the API writes it ("Fable 5"), for limits from its `limits` list. */
  name: string | null;
  /** Percent used, 0–100; above 100 when over the limit. */
  used: number;
  /** In ms; null while the window hasn't started. */
  resetsAt: number | null;
};

/** Extra Usage, the pay-as-you-go spend past the plan's limits; amounts in the currency's main unit. */
export type UsageExtra = {
  enabled: boolean;
  /** Spent this month. */
  usedCredits: number;
  /** The monthly cap; null without one. */
  monthlyLimit: number | null;
  /** Percent of the cap spent; null without a cap. */
  used: number | null;
  /** ISO 4217, e.g. `USD`. */
  currency: string | null;
};

export type UsageError = "network" | "timeout" | "forbidden" | "server" | "http" | "badResponse" | "credentials" | "keychain";

export type UsageState = {
  status: "ready" | "loading" | "signedOut" | "expired" | "rateLimited" | "error";
  /** The last good reading, kept through later failures. */
  usage: { windows: UsageWindow[]; extra: UsageExtra | null } | null;
  /** The plan Claude Code signed in with: `Pro`, `Max 5x`, `Team`… */
  plan: string | null;
  /** When `usage` was read, in ms. */
  fetchedAt: number | null;
  /** While rate limited: when the core asks again, in ms. */
  retryAt: number | null;
  /** With `error`: what went wrong. */
  error: UsageError | null;
};

export type TokenCounts = { input: number; output: number; cacheWrite: number; cacheRead: number; total: number };

/** Today's tokens from Claude Code's logs, priced as the API would charge them. */
export type SpendToday = {
  /** `YYYY-MM-DD`, local. */
  date: string;
  tokens: TokenCounts;
  /** Priced models only. */
  costUsd: number;
  /** The costliest first. */
  models: Array<{ model: string; tokens: TokenCounts; costUsd: number; priced: boolean }>;
  /** Used today without a known price: their tokens count, their cost doesn't. */
  unpricedModels: string[];
};

/** The Fuel page's data, pushed to the main window and the popover as `fuel`. */
export type Fuel = { limits: UsageState; today: SpendToday | null };

/* ---------- Dependencies: lock files against what is installed (deps_state, event `deps`) ---------- */

/** A package whose installed version isn't the locked one; `installed` null: missing, `locked`
 * null: installed but no longer locked. */
export type DepPackage = { name: string; locked: string | null; installed: string | null };

/** One package manager's check. `ecosystem`: the language it belongs to (`npm`, `composer`,
 * `python`, `ruby`, `dotnet`, …); `manager`: the tool (`pnpm`, `uv`, `bundler`, …). `unknown`: no
 * lock file to compare with (not watched); `differing`: how many packages differ (`packages` may
 * list fewer). `installCommand`: what an install runs, null for a manager that resolves at build
 * time (nothing to install); `canScan`: it has an `outdated`/`audit` to run; `tool`: whether the
 * manager itself was found on this machine. */
export type DepCheck = {
  ecosystem: string;
  manager: string;
  state: "ok" | "install" | "unknown";
  packages: DepPackage[];
  differing: number;
  installCommand: string | null;
  canScan: boolean;
  tool: "ok" | "missing";
};

/** What the project asks of a runtime (`node`, `php`, `python`, …), what is active, and whether
 * it satisfies it (null: can't tell). */
export type DepRuntime = { name: string; required: string | null; active: string | null; ok: boolean | null };

/** The last `outdated`/`audit` run (network, on request only); counts null when it didn't get them. */
export type DepScan = { at: number; outdated: number | null; vulnerable: number | null; error: "offline" | "timeout" | "failed" | null; running: boolean };

/** A package an outdated report lists: the version installed, the newest the project's
 * constraint allows (where the manager tells) and the newest there is. */
export type DepOutdated = { name: string; installed: string | null; wanted: string | null; latest: string | null };

/** One advisory against a package, as far as the manager's audit tells. `direct`: the project
 * asks for the package itself; `affected`: the versions it is in, as the advisory writes them;
 * `via`: the vulnerable packages it comes with, for one with no advisory of its own; `fix`: what
 * fixes it, as the audit writes it (the patched versions, or the package to install);
 * `fixable`: the manager can fix it itself, null where the audit doesn't say. */
export type DepAdvisory = {
  package: string;
  installed: string | null;
  severity: "critical" | "high" | "moderate" | "low" | "info" | null;
  direct: boolean | null;
  title: string | null;
  url: string | null;
  affected: string | null;
  via: string | null;
  fix: string | null;
  fixable: boolean | null;
};

/** What a scan's counts are of (`deps_scan_details`, asked for when shown): the lists the scan
 * kept, at most 300 of each. A part is null when its command didn't run or gave no list. */
export type DepScanDetails = { outdated: DepOutdated[] | null; advisories: DepAdvisory[] | null };

/** One migration tool's (`laravel`, `django`, `rails`, `prisma`, …) migrations as the project's
 * database reports them (read-only; the tool connects, Pitwall never reads credentials).
 * `command`: what a migrate runs; `pending`: the names still to run, kept from the last good read
 * when a later one fails; `error`: why the last read got no list; `at`: when it was read;
 * `running`: a read is in flight; `database`: where the migrations would go, when known. */
export type DepMigrations = {
  tool: string;
  command: string;
  pending: string[];
  error: "unreachable" | "noTable" | "timeout" | "failed" | "toolMissing" | "noEnvironment" | null;
  at: number;
  running: boolean;
  database: { connection: string | null; name: string | null } | null;
};

/** One listed project's dependencies. `migrations`: one per migration tool found, usually none or
 * one; `scans`: by ecosystem. */
export type DepReport = {
  path: string;
  checks: DepCheck[];
  runtimes: DepRuntime[];
  migrations: DepMigrations[];
  framework: { name: string; version: string } | null;
  scans: Record<string, DepScan>;
  checkedAt: number;
  /** The ecosystem whose install runs now, if any. */
  installing: string | null;
};

/* ---------- Sessions: every Claude Code session today, live (claude_sessions, event `sessions`) ---------- */

/**
 * Where a session runs: one of Pitwall's own terminals (its id), a VS Code window's Claude Code
 * tab or terminal (the window's name), a terminal app (`iTerm2`, `Ghostty`… from an allowlist;
 * null when not named), something else (`entrypoint`: `sdk-ts`, `claude-desktop`…), or unknown
 * (no sessions folder, no process table: Windows).
 */
export type SessionOrigin =
  | { kind: "pitwall"; terminal: number }
  | { kind: "vscodeTab"; window: string }
  | { kind: "vscodeTerminal"; window: string }
  | { kind: "terminal"; app: string | null }
  /** `claude-vscode`: a Claude Code tab in an editor window without the extension. */
  | { kind: "other"; entrypoint: string | null }
  | { kind: "unknown" };

/** A stretch of a session's day: Claude working, or waiting on you. */
export type SessionSpan = { start: number; end: number; kind: "work" | "wait" };

export type SessionRow = {
  /** The session id (its log's name, a UUID). */
  id: string;
  /** The listed project it runs in; null for a folder outside the listed projects. */
  path: string | null;
  /** The folder it runs in. */
  folder: string;
  /** The project's name, or the folder's. */
  project: string;
  /** Its name (`custom-title`, else `ai-title`); never message text. */
  title: string | null;
  /** `idle`: open, nothing going on and nothing unseen; `ended`: its process is gone. */
  phase: "working" | "waiting" | "idle" | "ended";
  /** While waiting: on what, since when (a permission wait the hook didn't name has no `tool`). */
  turn: Turn | null;
  /** A permission wait: the tool's name (`Bash`); never its input. */
  tool: string | null;
  /** In this phase since (an ended one: when it ended); null when not known. */
  since: number | null;
  /** Its process runs; null when that can't be told. */
  running: boolean | null;
  origin: SessionOrigin;
  /** Its part in today: time worked and waited on you, turns, and when; listed projects only. */
  today: { work: number; wait: number; turns: number; spans: SessionSpan[] } | null;
  /** Today's tokens (subagents' included), priced as the API would; `priced`: every model had a price. */
  spend: { tokens: TokenCounts; costUsd: number; priced: boolean; models: string[] } | null;
  /** Subagents it ran today. */
  subagents: number;
};

/**
 * The Sessions page, its rows in the core's order (waiting, working oldest first, idle, ended
 * newest first). `hook`: Claude Code's hook is installed (live state, permission waits);
 * `unlisted`: the rows in folders outside the listed projects (`path` null). Sent as `sessions`
 * to the main window once the page has asked for it, while the window is on screen; times that
 * only grow (today's) come again at most every 30 s.
 */
export type SessionsView = { now: number; hook: boolean; sessions: SessionRow[]; unlisted: number };

/** A session in the terminal's history (`claude_past`): its name and when it was last heard of. */
export type PastSession = { id: string; title: string | null; at: number | null };

/** What a click on a session in the terminal's history did (`resume_claude`): its tab shown, brought
 * up where it runs, opened again in a new tab; no log of it in the project's folder, or no telling
 * whether it still runs (left alone). */
export type Resumed = "shown" | "elsewhere" | "opened" | "missing" | "unsure";

/* ---------- Board: cards of work per project, given to Claude (board_state, event `board`) ---------- */

/** Up next, with Claude, to review, done. */
export type BoardColumn = "queued" | "claude" | "review" | "done";

/**
 * A card: a piece of work written for a project. Its place in its column is its place among the
 * project's cards there, in the board's order. `session` and `terminal`: the Claude session it was
 * given to (the core links it), and the Pitwall terminal that runs it; times in ms. `images`: those
 * pasted into its note, `[Image #n]` in its text.
 */
export type Card = {
  id: string;
  path: string;
  title: string;
  note: string;
  column: BoardColumn;
  createdAt: number;
  movedAt: number;
  session: string | null;
  terminal: number | null;
  givenAt: number | null;
  images: CardImage[];
  /** The model and the mode its last new session was started with; unset before the first. */
  start?: ClaudeStart | null;
  /** Up next and told to wait: the core gives it at its moment. */
  schedule?: CardSchedule | null;
};

/** An image a card holds: its number in the note, and what its file is. */
export type CardImage = { n: number; kind: "png" | "jpeg" | "gif" | "webp" };

/** An image pasted into a card's note, handed to the core with the card's text: its bytes as a
 * `data:` URL. */
export type NewImage = { n: number; data: string };

/* ---------- Update: the app's own new version (update_state, event `update`) ---------- */

/** The app's own update: what Settings › About and the sidebar's brand row show. */
export type UpdateView = {
  /** The installed version. */
  current: string;
  /** `off`: this build never asks (a dev run, no signing key). `available`: a new version exists
   *  but this install can't replace itself (a package manager's, a read-only place). */
  status: "off" | "idle" | "checking" | "downloading" | "ready" | "available" | "installing" | "failed";
  /** The new version: from `downloading` on, and with `available` and `failed`. */
  version: string | null;
  /** While `downloading`: bytes so far, and in all when the server said. */
  downloaded: number;
  total: number | null;
  /** When it last got an answer (ms since epoch). */
  checkedAt: number | null;
  /** The last ask got no answer (offline, GitHub down). Never a failure state of its own. */
  offline: boolean;
  /** With `failed`: `download` | `signature` | `install`. */
  error: string | null;
  /** A ready update is also installed when the app quits (not on Windows). */
  onQuit: boolean;
};

/** What a restart for the update would do to what is open; for the dialog. */
export type RestartPlan = {
  /** Cut off for good. `claude`: a Claude turn at work in one of Pitwall's tabs (its session is
   *  reopened, the turn is lost). `shell`: a command running in a shell tab. `command`: a
   *  running one-off custom command. `name` is the tab's or the command's name. */
  cut: Array<{ kind: "claude" | "shell" | "command"; project: string; name: string }>;
  /** Project names whose dev server is started again. */
  servers: string[];
  /** Project names of the Claude sessions opened again, one entry per session. */
  claude: string[];
  /** Tabs opened again, and in how many projects. */
  terminals: number;
  terminalProjects: number;
};

/* ---------- The phone's channel (core/relay.rs: phone_config, phone_set) ---------- */

export type PhoneChannel = "ntfy" | "telegram" | "webhook";

/** The moment something may go to the phone: while the screen is locked, after five minutes
 *  without input (or locked), or whenever. */
export type PhoneWhen = "locked" | "idle" | "always";

/** The events offered, each with its box. */
export type PhoneEvents = { asked: boolean; permission: boolean; finished: boolean; crashed: boolean; fuel: boolean; card: boolean };

/** The channel as Settings sends it. Each channel keeps its address while another is picked. */
export type PhoneConfig = {
  on: boolean;
  channel: PhoneChannel;
  ntfyServer: string;
  ntfyTopic: string;
  /** A new token; empty keeps the one stored. */
  telegramToken: string;
  telegramChat: string;
  webhookUrl: string;
  when: PhoneWhen;
  events: PhoneEvents;
};

/** The channel as the core holds it. */
export type PhoneView = Omit<PhoneConfig, "telegramToken"> & {
  /** A Telegram token is stored. The token itself never comes. */
  telegramToken: boolean;
  /** The moments this system can tell, in the order they are offered. */
  moments: PhoneWhen[];
  /** What a permission wait in the first listed project would send, word for word. */
  sample: string;
  /** Why the last message didn't arrive; null once one did. */
  failure: string | null;
};

/* ---------- Notifications: quiet, what was missed, muted projects (core/signals.rs) ---------- */

/** What happened, as the core names it. */
export type SignalKind =
  | "claude-asking"
  | "claude-permission"
  | "claude-finished"
  | "claude-long-turn"
  | "server-crashed"
  | "command-done"
  | "command-failed"
  | "deps-waiting"
  | "fuel-high"
  | "card-review";

/** Something the user wasn't told while it was quiet. `path` and `name` are empty for what is
 * about no project (the fuel alert); `detail`: a permission's tool, a command's name, the fuel
 * alert's line. */
export type Missed = { kind: SignalKind; path: string; name: string; detail: string | null; at: number };

/** Quiet now: until when (ms; null: until it is switched off), and whether the quiet hours made it so. */
export type Quiet = { until: number | null; hours: boolean };

/** Settings › Notifications. Claude's own switch is `Settings.notify`. */
export type NotificationSettings = {
  serverCrashed: boolean;
  command: boolean;
  longTurn: boolean;
  deps: boolean;
  /** The muted projects' paths. */
  muted?: string[];
  /** Every day from `from` to `to` (`HH:MM`), when `on`. */
  quietHours: { on: boolean; from: string; to: string };
  /** While quiet, Claude's questions and permission prompts still come through. */
  questionsThrough: boolean;
  /** While quiet, the sounds are held too. */
  soundsToo: boolean;
};

/* ---------- The team radio: Claude Code asks Pitwall (radio_tools, radio_state, radio_set, radio_tool, radio_calls) ---------- */

/** The radio's own settings: on or off, and each tool's switch by the tool's name. */
export type RadioSettings = { on: boolean; tools: Record<string, boolean> };

/** One tool of the radio, as the core lists it: it `acts` or only reads. Its switch is the
 * settings' (`RadioSettings.tools`). */
export type RadioTool = { name: string; acts: boolean };

/** The radio as Claude Code has it. `unsupported`: not on this system. `missing`: Claude Code's CLI
 * wasn't found. `sessions`: Claude Code sessions with a line to Pitwall right now. */
export type RadioState = { status: "on" | "off" | "unsupported"; missing: boolean; sessions: number };

/** What came of a call over the radio, for its session's list: the server as it was answered,
 * lines of output read, a card added, a note left on the session's card, a saved command run. */
export type RadioResult =
  | { kind: "status"; status: Status; port: number | null; issue: string | null }
  | { kind: "lines"; count: number }
  | { kind: "card"; title: string }
  | { kind: "noted" }
  | { kind: "ran"; name: string }
  | { kind: "refused" };

/** One call a session made over the radio. */
export type RadioCall = { at: number; tool: string; result: RadioResult };

/* ---------- The chequered flag: what is still open (flag_state) ---------- */

/** A project with something of Git's left: `count` changes to commit, or commits to push. */
export type FlagGit = { path: string; name: string; branch: string; count: number };

/** A Claude session that waits on you: with what (`tool`: a permission prompt's tool, by name
 * alone), and since when (ms). */
export type FlagWait = { path: string; name: string; session: string; kind: Turn["kind"]; tool: string | null; since: number };

/** A project's cards in "To review". */
export type FlagReview = { path: string; name: string; cards: number };

/** A project's cards that wait for their moment: how many, and the earliest moment (ms); null
 * while one of them waits its turn in the refill's line. */
export type FlagScheduled = { path: string; name: string; cards: number; at: number | null };

/** What still runs: the projects whose dev server does, the commands (each with its `project`'s
 * name), and whether some of it is being stopped right now. */
export type FlagRunning = { servers: Array<{ path: string; name: string }>; commands: Array<{ path: string; project: string; id: string; name: string }>; stopping: boolean };

/** What is still open, part by part; a part with nothing in it is empty. */
export type Flag = { commit: FlagGit[]; push: FlagGit[]; claude: FlagWait[]; review: FlagReview[]; scheduled: FlagScheduled[]; running: FlagRunning };

/** The core's answer: what is open, and whether a quit was held and waits on the dialog. */
export type FlagState = { open: Flag; asked: boolean };

/* ---------- The command line: the `pitwall` command and pitwall:// links (cli_state, cli_install, cli_remove, cli_links, cli_copy_link) ---------- */

/** The `pitwall` command as Settings › General shows it. `unsupported`: not on this system.
 * `path`: where the command is, none while it isn't installed. */
export type CliState = { status: "ready" | "unsupported"; path: string | null };

/** What a `pitwall://` link asks for. */
export type LinkAction = "start" | "stop" | "restart" | "show";

/** A model by the alias Claude Code's `--model` takes for the newest one of its name. */
export type ClaudeModel = "fable" | "opus" | "sonnet" | "haiku";

/** The mode a Claude session starts in: Claude Code's own, plan first, or edits made without asking. */
export type ClaudeMode = "normal" | "plan" | "acceptEdits";

/** How a Claude session starts, as the core keeps it: no model is Claude Code's own choice, no
 * mode is normal. */
export type ClaudeStart = { model?: ClaudeModel; mode?: ClaudeMode };

/** How a Claude session is to start, as the window says it to the core (the core keeps only
 * values of its two lists): `model` null is Claude Code's own choice. */
export type StartChoice = { model: ClaudeModel | null; mode: ClaudeMode };

/** A card told to wait: when it is given (ms), whether that moment is the 5-hour session's
 * refill (it then follows the reading), and what was chosen for it. `line`: its refill came while
 * a card before it was given or still with Claude, and it waits its turn (`at` is no moment then). */
export type CardSchedule = { at: number; refill: boolean; start: ClaudeStart; line?: boolean };
