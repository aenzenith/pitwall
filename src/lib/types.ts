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

export type ProjectSettings = { script?: string; port?: number; url?: string; commands?: CustomCommand[] };

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
