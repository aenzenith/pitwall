// The mini tower's arithmetic: which projects it lists, in which order, and what each says at its
// right end. Pure: the tower's page (tower/Tower.vue) draws from it.

import { duration } from "./day";
import { phase } from "./format";
import { t } from "./i18n";
import { towerOrder } from "./track";
import type { Project, TowerSettings } from "./types";

/** The tower's settings before the core has sent any, and for what an older core doesn't send. */
export const TOWER_DEFAULTS: TowerSettings = { on: false, form: "list", hideStopped: true, allSpaces: true, shortcut: false, shortcutKeys: "Ctrl+Alt+KeyK" };

export function towerSettings(stored: Partial<TowerSettings> | undefined): TowerSettings {
  return { ...TOWER_DEFAULTS, ...stored, form: stored?.form === "strip" ? "strip" : "list" };
}

/** Neither runs nor has Claude in it: what "hide stopped" leaves out. A crash stays. */
export function isIdle(project: Project): boolean {
  return project.status === "stopped" && !project.claude && !project.claudeWorking;
}

/**
 * The projects in the tower's order (who waits on you, Claude at work, what runs, the rest; the
 * Track page's), split in two: `shown`, and the `idle` ones left out while `hideStopped`.
 */
export function towerRows(projects: Project[], hideStopped: boolean): { shown: Project[]; idle: Project[] } {
  const ordered = towerOrder(projects);
  if (!hideStopped) return { shown: ordered, idle: [] };
  return { shown: ordered.filter((project) => !isIdle(project)), idle: ordered.filter(isIdle) };
}

/** What a row says at its right end, and in which ink: Claude's, a crash's, the plain one, or a
 * faint one for what stands still. */
export type TowerState = { text: string; tone: "claude" | "crash" | "plain" | "faint" };

/** How long since `at`, as short as the day's timeline writes it: `< 1 min`, `14 min`, `1 h 05 min`. */
function since(at: number, now: number): string {
  return duration(Math.max(1, now - at));
}

/** The session the project's waiting turn is of. */
function asker(project: Project): Project["claudeSessions"][number] | undefined {
  const waiting = project.claudeSessions.filter((session) => session.phase === "waiting");
  return waiting.find((session) => session.turn?.at === project.claude?.at) ?? waiting[0];
}

/**
 * What matters most about a project, in a few letters: what Claude waits on you with (a
 * permission's tool, else for how long), how long Claude has been at work (the hook tells; without
 * it the server speaks), a crash, a start or stop under way, the port, or that it stands still.
 */
export function towerState(project: Project, now: number): TowerState {
  const turn = project.claude;
  if (turn) {
    const what = t(({ permission: "tower.permission", asking: "tower.asking", finished: "tower.finished" } as const)[turn.kind]);
    const tool = turn.kind === "permission" ? asker(project)?.tool : undefined;
    return { text: `${what} · ${tool ?? since(turn.at, now)}`, tone: "claude" };
  }

  const began = project.claudeSessions.filter((session) => session.phase === "working" && session.since).map((session) => session.since as number);
  if (project.claudeWorking && began.length) return { text: since(Math.min(...began), now), tone: "claude" };

  if (project.status === "crashed") return { text: t("tower.crashed"), tone: "crash" };
  if (project.status === "busy") return { text: phase(project), tone: "plain" };
  if (project.status === "running") return { text: project.port ? `:${project.port}` : t("phase.starting"), tone: "plain" };
  return { text: t("server.stopped"), tone: "faint" };
}

/** The Claude session a row's button brings up: the one that waits on you, else the first at
 * work; null when Claude waits with a turn no session is listed for (the core finds it then). */
export function towerSession(project: Project): string | null {
  return (asker(project) ?? project.claudeSessions.find((session) => session.phase === "working"))?.id ?? null;
}
