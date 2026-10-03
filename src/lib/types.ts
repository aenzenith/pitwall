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

/** An address the project names itself, offered as a link; `source`: `git`, `.env`, `package.json`. */
export type LinkSuggestion = { name: string; url: string; source: string };

export type ProjectSettings = { script?: string; port?: number; url?: string; commands?: CustomCommand[]; links?: ProjectLink[] };

export type JobResult = { ok: boolean; code: number | null; stopped: boolean; finishedAt: number };

export type CommandView = CustomCommand & {
  status: "running" | "busy" | "ok" | "failed" | "idle";
  startedAt: number | null;
  result: JobResult | null;
};

export type Status = "running" | "stopped" | "crashed" | "busy";

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
  /** Its Claude sessions that are working, or wait on you (notifications not read yet). */
  claudeSessions: Array<{ id: string; phase: "working" | "waiting"; turn: Turn | null }>;
  git: GitInfo | null;
  script: string;
  /** The exact command the dev server runs: `npm run dev`, `pnpm dev`, `bun run dev`… */
  runCommand: string;
  settings: ProjectSettings;
  commands: CommandView[];
  /** Open terminals in this project. */
  terminals: TerminalView[];
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
  /** Which of Pitwall's sounds each event plays (src-tauri/sounds); "" for none. */
  sounds: { claudeFinished: string; claudeAsking: string; serverCrashed: string; serverReady: string; commandDone: string };
  launchAtLogin: boolean;
  shortcut: boolean;
  /** Global-hotkey form: `Ctrl+Alt+KeyP`, `Alt+Space`. */
  shortcutKeys: string;
  projects: Record<string, ProjectSettings>;
  /** The list order the user dragged into place. */
  order: string[];
  /** Searched by the quick switcher when no project matches. */
  projectsDir?: string;
  /** The board the Board page showed last: `all`, or a project's path. */
  boardScope?: string;
  /** How a project's board is drawn: `columns` (side by side) or `list` (one under the other). */
  boardView?: string;
};

/** An open project terminal. */
export type TerminalView = { id: number; path: string; name: string; kind: "shell" | "claude" };

/** A subfolder of the projects folder. */
export type Folder = { name: string; path: string };

export type Snapshot = {
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
};

/** A stretch of the day; Claude's name their session. */
export type DaySpan = { start: number; end: number; session?: string };

/** A Claude session's part in the day: when, how long it worked and waited on you, its turns. */
export type DaySession = { id: string; title: string | null; start: number; end: number; work: number; wait: number; turns: number };

export type DayCommit = { at: number; subject: string };

export type DayCommand = { name: string; runs: number; failed: number };

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
};

/** One day: what Pitwall wrote down while it ran, and the commits made. */
export type DaySummary = { date: string; start: number; end: number; today: boolean; now: number; projects: DayProject[] };

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

/** The Fuel page's data, pushed to the main window as `fuel`. */
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
  error: "unreachable" | "noTable" | "timeout" | "failed" | "toolMissing" | null;
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
};

/** An image a card holds: its number in the note, and what its file is. */
export type CardImage = { n: number; kind: "png" | "jpeg" | "gif" | "webp" };

/** An image pasted into a card's note, handed to the core with the card's text: its bytes as a
 * `data:` URL. */
export type NewImage = { n: number; data: string };
