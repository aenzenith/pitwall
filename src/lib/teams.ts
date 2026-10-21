// Teams: projects listed together, started and stopped together (core/teams.rs). The core keeps
// them; a change here shows at once and the next snapshot confirms it.

import { computed, ref } from "vue";

import { parseKey, type Drop } from "./groupReorder";
import { t } from "./i18n";
import { api, snapshot } from "./store";
import type { Project, Team } from "./types";

/** The teams, in the order they are drawn. */
export const teams = computed<Team[]>(() => snapshot.value?.settings.teams ?? []);

/** The team a project is in, if any. */
export function teamOf(path: string): Team | null {
  return teams.value.find((team) => team.projects.includes(path)) ?? null;
}

/** A team's projects that are listed right now, in the team's order. */
export function membersOf(team: Team): Project[] {
  const listed = new Map((snapshot.value?.projects ?? []).map((project) => [project.path, project]));
  return team.projects.map((path) => listed.get(path)).filter((project) => project !== undefined);
}

/** The listed projects in no team, in the list's order. */
export const looseProjects = computed<Project[]>(() => {
  const taken = new Set(teams.value.flatMap((team) => team.projects));
  return (snapshot.value?.projects ?? []).filter((project) => !taken.has(project.path));
});

/** The start or stop asked for a team, until the core has answered the call. */
const asked = ref<Record<string, "start" | "stop">>({});

/** What Claude is at in some of a group's projects: waiting on you, or at work. */
export type ClaudeNote = { phase: "waiting" | "working"; text: string };

/**
 * What Claude is at in `projects`, said for the group as a whole (its rows may be folded away):
 * a note for each state one of them is in, what waits on you first (a permission, a question, a
 * finished turn), then Claude at work. A project counts once, by what its row shows.
 */
export function claudeNotes(projects: Project[]): ClaudeNote[] {
  const count = { permission: 0, asking: 0, finished: 0, working: 0 };
  for (const project of projects) {
    if (project.claude) count[project.claude.kind]++;
    else if (project.claudeWorking) count.working++;
  }

  return (["permission", "asking", "finished", "working"] as const)
    .filter((kind) => count[kind] > 0)
    .map((kind) => ({
      phase: kind === "working" ? "working" : "waiting",
      text: count[kind] === 1 ? t(`team.claude.${kind}`) : t(`team.claude.${kind}In`, { count: count[kind] }),
    }));
}

/**
 * How a team stands: how many of its projects are listed and how many of them run, whether a
 * start or a stop is under way (a project of its own busy with one, or the team's just asked
 * for), and what Claude is at in them.
 */
export type TeamState = { total: number; running: number; starting: boolean; stopping: boolean; busy: boolean; claude: ClaudeNote[] };

export function teamState(team: Team): TeamState {
  const members = membersOf(team);
  const busy = members.filter((project) => project.status === "busy");
  const stopping = asked.value[team.id] === "stop" || busy.some((project) => project.phase === "stopping…");
  const starting = asked.value[team.id] === "start" || busy.some((project) => project.phase !== "stopping…");
  return {
    total: members.length,
    running: members.filter((project) => project.status === "running").length,
    starting,
    stopping,
    busy: starting || stopping,
    claude: claudeNotes(members),
  };
}

/** "3 projects · 2 running": the line beside a team's name; the running part only when one runs. */
export function teamLine(total: number, running: number): string {
  const parts = [t("team.projects", { count: total })];
  if (running > 0) parts.push(t("team.running", { count: running }));
  return parts.join(" · ");
}

/* ---------- changes, shown at once ---------- */

/**
 * `listed` in their new order, with the paths of `old` that aren't among them kept: each stays
 * right after the one it followed (as core/projects.rs: merge_order), so a project that isn't
 * listed right now comes back where it was.
 */
function mergeOrder(old: string[], listed: string[]): string[] {
  const merged = [...listed];
  let anchor: string | null = null;
  for (const path of old) {
    if (!merged.includes(path)) merged.splice(anchor === null ? 0 : merged.indexOf(anchor) + 1, 0, path);
    anchor = path;
  }
  return merged;
}

/**
 * Shows the teams as they will be, and the list in their order, as the core sorts it: each
 * team's projects first, then the rest in the order dragged before (`settings.order`), then by name.
 */
function show(next: Team[]): void {
  const state = snapshot.value;
  if (!state) return;
  state.settings.teams = next;

  const place = new Map<string, number>();
  next.forEach((team, at) => {
    team.projects.forEach((path, i) => {
      if (!place.has(path)) place.set(path, at * 1e6 + i);
    });
  });
  const dragged = new Map(state.settings.order.map((path, i) => [path, i]));
  const rank = (project: Project): [number, number] => [place.get(project.path) ?? Infinity, dragged.get(project.path) ?? Infinity];
  state.projects = [...state.projects].sort((a, b) => {
    const [teamA, looseA] = rank(a);
    const [teamB, looseB] = rank(b);
    if (teamA !== teamB) return teamA < teamB ? -1 : 1;
    if (looseA !== looseB) return looseA < looseB ? -1 : 1;
    return looseA === Infinity ? a.name.toLowerCase().localeCompare(b.name.toLowerCase()) : 0;
  });
}

/** One team changed, the others as they are. */
function change(id: string, to: (team: Team) => Team): void {
  show(teams.value.map((team) => (team.id === id ? to(team) : team)));
}

/** A call the core may refuse (an older one doesn't know teams): what shows then lasts until its
 * next snapshot. */
function send(call: Promise<unknown>): Promise<unknown> {
  return call.catch(() => undefined);
}

/** A team's projects, in order: a reorder, one added (it leaves the team it was in) or one
 * dropped (it is in no team then). Its projects that aren't listed right now keep their place. */
export function setTeamProjects(id: string, listed: string[]): Promise<unknown> {
  const team = teams.value.find((entry) => entry.id === id);
  if (!team) return Promise.resolve();
  // Of its old order, what stays: the projects kept, and those not listed right now.
  const shown = new Set((snapshot.value?.projects ?? []).map((project) => project.path));
  const projects = mergeOrder(
    team.projects.filter((path) => !shown.has(path) || listed.includes(path)),
    listed,
  );
  show(teams.value.map((entry) => (entry.id === id ? { ...entry, projects } : { ...entry, projects: entry.projects.filter((path) => !projects.includes(path)) })));
  return send(api.teamProjects(id, projects));
}

/** The project goes to the end of a team. */
export function moveToTeam(path: string, id: string): void {
  const team = teams.value.find((entry) => entry.id === id);
  if (team) void setTeamProjects(id, [...membersOf(team).map((project) => project.path).filter((member) => member !== path), path]);
}

/** The project leaves its team; it stays listed. */
export function leaveTeam(path: string): Promise<unknown> {
  const team = teamOf(path);
  if (!team) return Promise.resolve();
  return setTeamProjects(
    team.id,
    membersOf(team)
      .map((project) => project.path)
      .filter((member) => member !== path),
  );
}

/** The projects in no team, in a new order. */
export function orderLoose(loose: string[]): Promise<unknown> {
  const state = snapshot.value;
  if (!state) return Promise.resolve();
  // The whole list as it shows: the core keeps the places of projects not listed right now.
  const paths = [...teams.value.flatMap((team) => membersOf(team).map((project) => project.path)), ...loose];
  state.settings.order = mergeOrder(state.settings.order, paths);
  show(teams.value);
  return send(api.reorder(paths));
}

export function orderTeams(ids: string[]): void {
  const place = (team: Team): number => {
    const at = ids.indexOf(team.id);
    return at < 0 ? Infinity : at;
  };
  show([...teams.value].sort((a, b) => place(a) - place(b)));
  void send(api.teamOrder(ids));
}

/** A team one place up or down among the teams: the keyboard's way to reorder them. */
export function nudgeTeam(id: string, by: -1 | 1): void {
  const ids = teams.value.map((team) => team.id);
  const at = ids.indexOf(id);
  const to = at + by;
  if (at < 0 || to < 0 || to >= ids.length) return;
  [ids[at], ids[to]] = [ids[to], ids[at]];
  orderTeams(ids);
}

export function launchTeam(id: string, on: boolean): void {
  change(id, (team) => ({ ...team, startAtLaunch: on }));
  void send(api.teamLaunch(id, on));
}

export function renameTeam(id: string, name: string): void {
  change(id, (team) => ({ ...team, name }));
  void send(api.teamRename(id, name));
}

export function deleteTeam(id: string): void {
  show(teams.value.filter((team) => team.id !== id));
  void send(api.teamDelete(id));
}

/** A new team, last in the list, with these projects. Null when the core refused it. */
export async function createTeam(name: string, projects: string[] = []): Promise<Team | null> {
  try {
    const team = await api.teamCreate(name, projects);
    // The snapshot that brings it may still be on its way.
    if (!teams.value.some((entry) => entry.id === team.id)) {
      show([...teams.value.map((entry) => ({ ...entry, projects: entry.projects.filter((path) => !team.projects.includes(path)) })), team]);
    }
    return team;
  } catch {
    return null;
  }
}

function ask(id: string, what: "start" | "stop", call: Promise<unknown>): void {
  asked.value = { ...asked.value, [id]: what };
  void send(call).then(() => {
    const rest = { ...asked.value };
    delete rest[id];
    asked.value = rest;
  });
}

/** Starts the team's projects that don't run, one after the other in its order. */
export function startTeam(id: string): void {
  ask(id, "start", api.teamStart(id));
}

export function stopTeam(id: string): void {
  ask(id, "stop", api.teamStop(id));
}

/* ---------- a drop in the list (lib/groupReorder) ---------- */

/**
 * A row dropped in a list drawn without heads (a filter, a search): it takes its place beside
 * the row it traded with, among its own team's projects or among those in no team.
 */
function placeBeside(path: string, beside: string, after: boolean): void {
  const team = teamOf(path);
  const order = (team ? membersOf(team) : looseProjects.value).map((project) => project.path).filter((member) => member !== path);
  const at = order.indexOf(beside);
  if (at < 0) return;
  order.splice(after ? at + 1 : at, 0, path);
  void (team ? setTeamProjects(team.id, order) : orderLoose(order));
}

/** A row dropped in the list drawn in groups: it belongs to the head above it now, at the place it was dropped. */
function placeUnder(path: string, keys: string[]): void {
  let group = "";
  let to: string | null = null;
  const rows: Record<string, string[]> = {};
  for (const key of keys) {
    const { kind, id } = parseKey(key);
    if (kind === "head") group = id;
    if (kind !== "row") continue;
    (rows[group] ??= []).push(id);
    if (id === path) to = group;
  }
  if (to === null) return;

  const from = teamOf(path);
  const target = teams.value.find((team) => team.id === to);
  if (target) {
    void setTeamProjects(target.id, rows[to]);
    return;
  }

  // Into the projects in no team: out of its team first, then to its place among them.
  const loose = rows[""] ?? [];
  if (from) {
    const left = from.projects.filter((member) => member !== path);
    show(teams.value.map((team) => (team.id === from.id ? { ...team, projects: left } : team)));
    void send(api.teamProjects(from.id, left));
  }
  void orderLoose(loose);
}

/** What a drop in the project list changes: the teams' order (a head), or a project's place and team (a row). */
export function applyDrop(change: Drop): void {
  const moved = parseKey(change.moved);
  if (moved.kind === "head") {
    orderTeams(change.keys.map(parseKey).filter((key) => key.kind === "head" && key.id !== "").map((key) => key.id));
    return;
  }
  if (moved.kind !== "row") return;
  if (change.keys.some((key) => parseKey(key).kind === "head")) placeUnder(moved.id, change.keys);
  else placeBeside(moved.id, parseKey(change.beside).id, change.after);
}

/* ---------- the small dialogs ---------- */

/** A dialog about a team, asked for from the list, the sidebar or the team's details; the window
 * shows it. `create`: a new team with these projects; `show`: the list goes to it afterwards. */
export type TeamAsk = { kind: "create"; projects: string[]; show: boolean } | { kind: "rename"; team: Team } | { kind: "delete"; team: Team };

export const teamAsk = ref<TeamAsk | null>(null);
