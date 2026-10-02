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
