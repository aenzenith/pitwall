import { language, t, type Key } from "./i18n";
import type { GitInfo, Project } from "./types";

export function uptime(startedAt: number | null, now: number): string {
  if (!startedAt) {
    return "";
  }

  const minutes = Math.max(0, Math.floor((now - startedAt) / 60_000));
  const hours = Math.floor(minutes / 60);

  return hours > 0 ? t("time.upHours", { hours, minutes: String(minutes % 60).padStart(2, "0") }) : t("time.upMinutes", { minutes });
}

export function ago(at: number, now: number): string {
  const minutes = Math.floor((now - at) / 60_000);

  if (minutes < 1) {
    return t("time.justNow");
  }
  if (minutes < 60) {
    return t("time.minutesAgo", { count: minutes });
  }

  const hours = Math.floor(minutes / 60);

  return hours < 24 ? t("time.hoursAgo", { count: hours }) : new Date(at).toLocaleDateString(language.value);
}

const PHASES: Record<NonNullable<Project["phase"]>, Key> = {
  "starting…": "phase.starting",
  "stopping…": "phase.stopping",
  "restarting…": "phase.restarting",
};

/** What a busy project is doing: `starting…`, `stopping…`, `restarting…`. */
export function phase(project: Project): string {
  return t(project.phase ? PHASES[project.phase] : "phase.working");
}

/** The second line under a project name. */
export function meta(project: Project, now: number): string {
  if (project.status === "busy") {
    return phase(project);
  }

  if (project.status === "running") {
    const parts = [project.port ? `:${project.port}` : t("phase.starting"), uptime(project.startedAt, now)];

    if (project.issue) {
      parts.push(project.issue.text);
    }

    return parts.filter(Boolean).join(" · ");
  }

  if (project.issue) {
    return project.issue.text;
  }

  return project.openIn ? t("server.stoppedOpenIn", { window: project.openIn }) : t("server.stopped");
}

export function claudeLine(project: Project, now: number): string {
  if (!project.claude) {
    return "";
  }

  const what = t(({ finished: "claude.finished", asking: "claude.asking", permission: "claude.permission" } as const)[project.claude.kind]);

  return `${what} · ${ago(project.claude.at, now)}`;
}

/** `main · 3 changed · ↑1 ↓2`, or `` outside Git. */
export function gitLine(git: GitInfo | null): string {
  if (!git) return "";
  const parts = [git.branch];
  if (git.changes) parts.push(t("git.changed", { count: git.changes }));
  const sync = [git.ahead ? `↑${git.ahead}` : "", git.behind ? `↓${git.behind}` : ""].filter(Boolean).join(" ");
  if (sync) parts.push(sync);
  return parts.join(" · ");
}

export function claudeState(project: Project, now: number): string {
  if (project.claude) return claudeLine(project, now);
  return project.claudeWorking ? t("claude.working") : "";
}

const MODIFIER_SYMBOLS: Record<string, string> = { CTRL: "⌃", CONTROL: "⌃", ALT: "⌥", OPTION: "⌥", SHIFT: "⇧", SUPER: "⌘", CMD: "⌘", COMMAND: "⌘" };
const KEY_SYMBOLS: Record<string, string> = {
  SPACE: "Space", ENTER: "↩", TAB: "⇥", BACKSPACE: "⌫", DELETE: "⌦", ESCAPE: "⎋",
  ARROWUP: "↑", ARROWDOWN: "↓", ARROWLEFT: "←", ARROWRIGHT: "→",
  MINUS: "-", EQUAL: "=", COMMA: ",", PERIOD: ".", SLASH: "/", BACKSLASH: "\\", SEMICOLON: ";",
  QUOTE: "'", BACKQUOTE: "`", BRACKETLEFT: "[", BRACKETRIGHT: "]",
};

/** `Ctrl+Alt+KeyP` → `⌃⌥P`, in the usual macOS order (⌃⌥⇧⌘). */
export function shortcutLabel(keys: string): string {
  const parts = keys.split("+").map((part) => part.trim().toUpperCase()).filter(Boolean);
  const order = ["⌃", "⌥", "⇧", "⌘"];
  const mods = parts.filter((p) => p in MODIFIER_SYMBOLS).map((p) => MODIFIER_SYMBOLS[p]);
  const key = parts.find((p) => !(p in MODIFIER_SYMBOLS)) ?? "";
  const label = KEY_SYMBOLS[key] ?? key.replace(/^KEY/, "").replace(/^DIGIT/, "");
  return [...order.filter((m) => mods.includes(m)), label].join("");
}

const EDITOR_NAMES: Record<string, string> = { vscode: "VS Code", "vscode-insiders": "Insiders", cursor: "Cursor", windsurf: "Windsurf" };

export function editorName(editor: string | undefined): string {
  return EDITOR_NAMES[editor ?? "vscode"] ?? t("editor.fallback");
}
