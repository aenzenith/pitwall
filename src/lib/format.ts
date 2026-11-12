import { computed } from "vue";

import { agentInfo, agentOn, multiAgent } from "./agents";
import { language, t, type Key } from "./i18n";
import { keys } from "./platform";
import { snapshot } from "./store";
import type { Agent, CommandView, GitInfo, HeldBy, Project, Turn } from "./types";

/** Every agent but Claude: each one's sentences are its own (`<key>.codex`). */
type OtherAgent = Exclude<Agent, "claude">;

/** A key whose sentence names Claude and that every other agent has written whole beside it:
 * `core.notify.finished` and `core.notify.finished.codex`. A key without them isn't one, so the
 * type check fails where a catalog misses an agent's sentence. */
export type AgentKey = { [K in Key]: `${K}.${OtherAgent}` extends Key ? K : never }[Key];

/** The key of a sentence that names `agent`: Claude's own, as it always was, else the same key
 * with the agent's id after a dot. Languages bend a name ("Claude'a", "Codex'e"), so it is never a
 * parameter. */
export function agentKey(key: AgentKey, agent: Agent): Key {
  // A payload without its agent (none is Claude) never asks for a key no catalog has.
  return !agent || agent === "claude" ? key : (`${key}.${agent}` as Key);
}

/** The places that could name an agent do, unless Claude is the only one on (each then looks as it
 * always did): with more than one on, or with Claude turned off and another on alone. Every
 * surface that names an agent asks this, never `multiAgent` (which says whether there is a
 * choice between agents). */
export const namesAgent = computed(() => multiAgent.value || !agentOn("claude"));

/** A key whose sentence names Claude and that has an `.agents` sentence for every agent beside it
 * (`notifications.claude` and `notifications.claude.agents`): a label that covers them all. */
export type AgentsKey = { [K in Key]: `${K}.agents` extends Key ? K : never }[Key];

/** `key` as Claude alone has it, its `.agents` sentence once the places name agents. */
export function agentsKey(key: AgentsKey): Key {
  return namesAgent.value ? (`${key}.agents` as Key) : key;
}

/** Settings › Agents: the Claude tab is the Agents tab once another agent is on this Mac, or ever
 * was turned on or off here (its id stays `claude`, so what opens Claude's tab opens it); until
 * then it is Claude's as it always was. What names that tab (`Settings › Claude`) follows it. */
export const agentsTab = computed(() => {
  const said = snapshot.value?.settings.agents?.codex;
  return !!agentInfo("codex")?.found || said?.on !== undefined || !!said?.told || snapshot.value?.settings.agents?.claude?.on === false;
});

/** The agents in the order a count names them: Claude first, as the core lists them. */
export const AGENT_ORDER: readonly Agent[] = ["claude", "codex"];

type LiveSession = Project["claudeSessions"][number];

/** The session a project's row speaks of (`claudeState`): the one that waits with the project's
 * newest turn, else one at work; null when neither is listed. */
export function rowSession(project: Project): LiveSession | null {
  const sessions = project.claudeSessions;
  const turn = project.claude;
  if (turn) {
    const waiting = sessions.filter((session) => session.phase === "waiting");
    return waiting.find((session) => session.turn?.at === turn.at && session.turn.kind === turn.kind) ?? waiting[0] ?? null;
  }
  return project.claudeWorking ? (sessions.find((session) => session.phase === "working") ?? null) : null;
}

/** Whose state a project's row shows: the agent of `rowSession`, Claude's when the turn was read
 * with no session listed for it; null while no agent waits or works there. */
export function rowAgent(project: Project): Agent | null {
  if (!project.claude && !project.claudeWorking) return null;
  return rowSession(project)?.agent ?? "claude";
}

/** How long a command's run that ended well keeps its ✓ and its output's tab. */
export const DONE_FOR_MS = 5 * 60_000;

/**
 * A command's state as shown: a run that ended well (a one-off done, a worker stopped) is done (✓)
 * for a few minutes after it ended, then idle again, and its output's tab goes from the terminal
 * panel (TerminalPanel). A failure (✕) stays until the next run.
 */
export function commandStatus(command: CommandView, at: number): CommandView["status"] {
  return commandFaded(command, at) ? "idle" : command.status;
}

/** Its run ended well longer ago than DONE_FOR_MS. */
export function commandFaded(command: CommandView, at: number): boolean {
  return command.status !== "running" && command.status !== "busy" && !!command.result?.ok && at - command.result.finishedAt > DONE_FOR_MS;
}

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

/** As `ago`, but a day or more as days: "3 days ago" (a hook's last event, which may be days old). */
export function agoDays(at: number, now: number): string {
  const days = Math.floor((now - at) / 86_400_000);
  return days > 0 ? t("time.daysAgo", { count: days }) : ago(at, now);
}

/** The site of the user guide. */
const GUIDE = "https://pitwall.aenzenith.com";

/** A section of the user guide, in the app's language: English at `/docs`, the others under
 * their own prefix (Chinese as `zh`). Opened in the browser, only when clicked (`open_url`). */
export function guideUrl(page: string, section: string): string {
  const lang = language.value === "zh-Hans" ? "zh" : language.value;
  return `${GUIDE}${lang === "en" ? "" : `/${lang}`}/docs/${page}#${section}`;
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
  return project.claude ? turnLine(project.claude, now) : "";
}

/** Why a command marked "ask first" waits beside its server: it waits for the go-ahead after a
 * click, and was skipped when anything else started the server. */
export function heldLine(held: HeldBy): string {
  return held === "user" ? t("detail.held.user") : `${t(`detail.held.${held}`)} · ${t("detail.tagAsksFirst")}`;
}

/** An address without its scheme and trailing slash, as a link row shows it. */
export function bareUrl(url: string): string {
  return url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "").replace(/\/$/, "");
}

/** What Claude waits on you with, and since when: `Finished · 2 min ago`. */
export function turnLine(turn: Turn, now: number): string {
  const what = t(({ finished: "claude.finished", asking: "claude.asking", permission: "claude.permission" } as const)[turn.kind]);

  return `${what} · ${ago(turn.at, now)}`;
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

/** A global shortcut (`Ctrl+Alt+KeyP`) as this platform writes it: `⌃⌥P` on macOS, `Ctrl+Alt+P` on
 * Windows and Linux (lib/platform: keys). On macOS Return and Escape read ↩ and ⎋ here, as in
 * menus. */
export function shortcutLabel(hotkey: string): string {
  return keys(hotkey, { ENTER: "↩", ESCAPE: "⎋" });
}

/** A download's size as the language writes it, one decimal: `6.1 MB`, `6,1 MB` (10⁶ bytes each). */
export function megabytes(bytes: number): string {
  return new Intl.NumberFormat(language.value, { style: "unit", unit: "megabyte", minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(bytes / 1_000_000);
}

/** Why an update didn't go in, by the core's error id: what came down can't be trusted (`download`,
 * `signature`), or it couldn't be put in place (`install`, and anything else). */
export function updateError(id: string | null): string {
  return t(id === "download" || id === "signature" ? "update.error.download" : "update.error.install");
}

const EDITOR_NAMES: Record<string, string> = { vscode: "VS Code", "vscode-insiders": "Insiders", cursor: "Cursor", windsurf: "Windsurf" };

export function editorName(editor: string | undefined): string {
  return EDITOR_NAMES[editor ?? "vscode"] ?? t("editor.fallback");
}
