<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import Icon from "../components/Icon.vue";
import Marquee from "../components/Marquee.vue";
import MutedMark from "../components/MutedMark.vue";
import Spinner from "../components/Spinner.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeState, gitLine, meta } from "../lib/format";
import { isLow, isStale, left, percentText, sessionWindow } from "../lib/fuel";
import { searchProjects } from "../lib/fuzzy";
import { language, t } from "../lib/i18n";
import { filters, isFilter, type Filter, type Page, type View } from "../lib/pages";
import { onBoard } from "../lib/board";
import { headKey, hintKey, rowKey, useGroupReorder } from "../lib/groupReorder";
import { useReorder } from "../lib/reorder";
import { useNativeMenu, type MenuEntry, type MenuPoint } from "../lib/nativeMenu";
import { cardRequest, outputRequest, terminalRequest, trackTerminals } from "../lib/panel";
import { dragRegion, keys, terminalChord } from "../lib/platform";
import { rowKeys } from "../lib/rows";
import { needsAttention } from "../lib/deps";
import { api, board, connectBoard, connectDeps, connectFuel, connectSessions, connectUpdate, deps, fuel, loadBoard, loadSessions, now, sessions, snapshot, update, visible } from "../lib/store";
import { applyDrop, claudeNotes, createTeam, deleteTeam, foldTeam, leaveTeam, looseProjects, membersOf, moveToTeam, nudgeTeam, orderTeams, renameTeam, startTeam, stopTeam, teamAsk, teamOf, teams, teamState, type ClaudeNote } from "../lib/teams";
import type { Card, Project, Team } from "../lib/types";
import BoardView from "./BoardView.vue";
import DayView from "./DayView.vue";
import DepsView from "./DepsView.vue";
import FuelView from "./FuelView.vue";
import ProjectDetail from "./ProjectDetail.vue";
import ProjectSuggestions from "./ProjectSuggestions.vue";
import ResumeDialog from "./ResumeDialog.vue";
import SessionsView from "./SessionsView.vue";
import SetupView from "./SetupView.vue";
import TeamDetail from "./TeamDetail.vue";
import TerminalPanel from "./TerminalPanel.vue";
import SettingsView from "./SettingsView.vue";
import TrackView from "./TrackView.vue";
import UpdateDialog from "./UpdateDialog.vue";
import FlagDialog from "./FlagDialog.vue";
import WhatsNewDialog from "./WhatsNewDialog.vue";
import TeamDeleteDialog from "./team/TeamDeleteDialog.vue";
import TeamHead from "./team/TeamHead.vue";
import TeamNameDialog from "./team/TeamNameDialog.vue";

const filter = ref<Filter>("all");
/** The project list, the track they run on, the day's timeline, Claude's sessions, the board of
 * cards, Claude's fuel, or the projects' dependencies. */
const view = ref<"projects" | View>("projects");

/** The sidebar's "26% left" beside Fuel: the session's share left, red when low, grey when stale. */
const fuelBadge = computed(() => {
  const limits = fuel.value?.limits;
  const session = sessionWindow(fuel.value, now.value);
  if (!limits || !session) return null;
  return { text: t("fuel.left", { value: percentText(left(session.used), language.value) }), low: isLow(session.used), stale: isStale(limits) };
});

let unlistenFuel: (() => void) | null = null;
/** Each feed is connected once, on first need (kept as the promise, so a quick second opening
 * never listens twice), and let go with the window. */
let sessionsLink: Promise<UnlistenFn> | null = null;
let boardLink: Promise<UnlistenFn> | null = null;

// Fuel's data comes to this window from the start: the sidebar shows it on every page.
onMounted(async () => {
  unlistenFuel = await connectFuel();
});

/** The dependencies' feed: from the start too, for the sidebar's count. */
let depsLink: Promise<UnlistenFn> | null = null;
onMounted(() => {
  depsLink = connectDeps();
});
onBeforeUnmount(() => {
  void depsLink?.then((unlisten) => unlisten(), () => undefined);
});

/** The app's own update: from the start too, for the brand row's version and button. */
let updateLink: Promise<UnlistenFn> | null = null;
onMounted(() => {
  updateLink = connectUpdate();
});
onBeforeUnmount(() => {
  void updateLink?.then((unlisten) => unlisten(), () => undefined);
});

/** The brand row's button: a new version to install (`ready`), to fetch by hand (`available`), or
 * going in right now (`installing`, its spinner). */
const updateButton = computed(() => update.value?.status === "ready" || update.value?.status === "available" || update.value?.status === "installing");
const installing = computed(() => update.value?.status === "installing");
/** The update dialog: opened from the brand row's button and from Settings › About. */
const updateOpen = ref(false);

/** Beside Garage: how many listed projects need you (lib/deps: needsAttention). */
const depsBadge = computed(() => {
  const listed = new Set((snapshot.value?.projects ?? []).map((project) => project.path));
  return (deps.value ?? []).filter((report) => listed.has(report.path) && needsAttention(report)).length;
});

// The sessions' only once a page showing them (Sessions, Board, Track) first opens: until then the
// core doesn't work them out. The Sessions page asks for a fresh look itself on each opening; the
// board's cards follow their sessions, so the Board page's opening asks for both here; the Track
// page names a project's sessions on its card. The list needs them for its terminal tabs alone
// (`listTabs`).
watch(view, (shown) => {
  if (shown === "sessions" || shown === "board" || shown === "track") {
    if (!sessionsLink) sessionsLink = connectSessions();
    else if (shown !== "sessions") void loadSessions();
  }
  if (shown === "board") {
    if (!boardLink) boardLink = connectBoard();
    else void loadBoard();
  }
});

onBeforeUnmount(() => {
  unlistenFuel?.();
  void sessionsLink?.then((unlisten) => unlisten(), () => undefined);
  void boardLink?.then((unlisten) => unlisten(), () => undefined);
});

// The core sends `sessions` only while this window is on screen: back on screen, the page catches
// up if it is shown.
watch(visible, (on) => {
  if (!on) return;
  if (view.value === "sessions" || view.value === "board" || view.value === "track" || listTabs.value) void loadSessions();
  if (view.value === "board") void loadBoard();
});
const settingsOpen = ref(false);
/** Settings opens on this tab: the Sessions page's "Add hook" opens it on Claude's, the Track
 * page's settings button on Track's, the popover's "Quiet hours…" on Notifications'. */
const settingsTab = ref<"claude" | "track" | "notifications" | undefined>(undefined);

function openSettings(tab?: "claude" | "track" | "notifications"): void {
  settingsTab.value = tab;
  settingsOpen.value = true;
}

/** The first run's steps (SetupView), in place of the list and its details: once by themselves,
 * when the app starts with no project and they were never finished or put off, and again when
 * Settings asks for them. */
const setupOpen = ref(false);
let setupOffered = false;
watch(
  snapshot,
  (state) => {
    if (!state || setupOffered) return;
    setupOffered = true;
    if (!state.projects.length && state.settings.onboarded === false) setupOpen.value = true;
  },
  { immediate: true },
);

/** From Settings: the steps again, on the list's page. */
function openSetup(): void {
  settingsOpen.value = false;
  view.value = "projects";
  setupOpen.value = true;
}

/** What was open at the last quit, while that is still to be answered and wasn't closed in this
 * window; never over the first run's steps. */
const resumeClosed = ref(false);
const resumeOffer = computed(() => (resumeClosed.value || setupOpen.value ? null : (snapshot.value?.resume ?? null)));

/** This version's notes: once by themselves after an update (when the offer above is out of the
 * way), and whenever Settings asks for them. */
const whatsNewOpen = ref(false);
let whatsNewOffered = false;
watch(
  () => snapshot.value?.whatsNew === true && !resumeOffer.value && !setupOpen.value,
  (due) => {
    if (!due || whatsNewOffered) return;
    whatsNewOffered = true;
    whatsNewOpen.value = true;
  },
  { immediate: true },
);

const query = ref("");
const selectedPath = ref<string | null>(null);
/** The team whose own view the list shows (its sidebar entry), in place of a filter's. */
const teamFilter = ref<string | null>(null);
/** The team selected in the list: its details show in place of a project's. */
const selectedTeamId = ref<string | null>(null);
/** The projects in no team, folded under their head: for as long as the window lasts. */
const looseFolded = ref(false);
const tbody = ref<HTMLElement | null>(null);
/** Drag to reorder: a row in its group or into another, a team's head among the teams. A row
 * dropped under a folded head goes to that group's end. */
const drag = useGroupReorder(tbody, (change) => applyDrop(change, (group) => sections.value.some((section) => section.head && (section.team?.id ?? "") === group && section.folded)));

function selectProject(path: string): void {
  selectedPath.value = path;
  selectedTeamId.value = null;
}

function select(path: string): void {
  if (drag.isClick()) selectProject(path);
}

/** A click on a team's head selects the team: its details show. (A double click folds or
 * unfolds the group, as its arrow does: `fold`.) */
function pressHead(section: Section): void {
  if (drag.isClick() && section.team) selectedTeamId.value = section.team.id;
}

const projects = computed(() => snapshot.value?.projects ?? []);
const anyBusy = computed(() => projects.value.some((p) => p.status === "busy"));
const current = computed(() => filters.find((f) => f.id === filter.value) ?? filters[0]);
const shownTeam = computed(() => teams.value.find((team) => team.id === teamFilter.value) ?? null);
/** The toolbar's heading and the list's name: the team shown, else the filter. */
const heading = computed(() => shownTeam.value?.name ?? t(current.value.label));

/** The sidebar's teams, each with how many of its projects run. */
const teamNav = computed(() => teams.value.map((team) => ({ team, ...teamState(team) })));

/** The sidebar's team entries, dragged into order as the list's rows are (lib/reorder): the
 * teams' own order, the same one their heads are dragged into in the list. */
const teamList = ref<HTMLElement | null>(null);
const teamDrag = useReorder(teamList, (moved, beside, after) => {
  const ids = teams.value.map((team) => team.id).filter((id) => id !== moved);
  const at = ids.indexOf(beside);
  if (at < 0) return;
  ids.splice(after ? at + 1 : at, 0, moved);
  orderTeams(ids);
});

/** ⌥↑ and ⌥↓ on a sidebar entry move its team one place, the keyboard staying on it. */
function onTeamNavKey(event: KeyboardEvent, id: string): void {
  if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
  event.preventDefault();
  nudgeTeam(id, event.key === "ArrowUp" ? -1 : 1);
  void nextTick(() => Array.from(teamList.value?.querySelectorAll<HTMLElement>("[data-path]") ?? []).find((entry) => entry.dataset.path === id)?.focus());
}

/** The list is drawn in groups, each under its head: a team's own view, and every project while
 * there are teams. The other filters and a search stay flat, as with no team at all. */
const grouped = computed(() => query.value.trim() === "" && (shownTeam.value !== null || (filter.value === "all" && teams.value.length > 0)));

/** A stretch of the list: a group under its head (`team` null: the projects in no team), or the
 * whole flat list (`head` false). A folded group shows its head alone. */
type Section = { key: string; head: boolean; team: Team | null; rows: Project[]; folded: boolean; total: number; running: number; busy: boolean; claude: ClaudeNote[] };

const sections = computed<Section[]>(() => {
  if (!grouped.value) {
    const q = query.value.trim().toLowerCase();
    const pool = shownTeam.value ? membersOf(shownTeam.value) : projects.value.filter((p) => current.value.test(p));
    // Searching: the quick switcher's fuzzy match and order (lib/fuzzy); else the list's own order.
    const found = q ? searchProjects(pool, q).map((match) => match.project) : pool;
    return [{ key: "flat", head: false, team: null, rows: found, folded: false, total: found.length, running: 0, busy: false, claude: [] }];
  }

  const ofTeam = (team: Team, folded: boolean): Section => {
    const state = teamState(team);
    return { key: `team:${team.id}`, head: true, team, rows: membersOf(team), folded, total: state.total, running: state.running, busy: state.busy, claude: state.claude };
  };
  // Its own view shows a team's rows whether it is folded in the list or not.
  if (shownTeam.value) return [ofTeam(shownTeam.value, false)];

  const list = teams.value.map((team) => ofTeam(team, team.collapsed));
  const loose = looseProjects.value;
  if (loose.length) {
    list.push({ key: "loose", head: true, team: null, rows: loose, folded: looseFolded.value, total: loose.length, running: loose.filter((p) => p.status === "running").length, busy: false, claude: claudeNotes(loose) });
  }
  return list;
});

/** The rows drawn, top to bottom: a folded group's aren't. */
const rows = computed(() => sections.value.flatMap((section) => (section.folded ? [] : section.rows)));
/** Every project the list holds, a folded group's too. */
const listed = computed(() => sections.value.flatMap((section) => section.rows));
/** Something to draw: a flat list with a row, the groups while a project is listed, a team's own view always. */
const listShown = computed(() => (grouped.value ? shownTeam.value !== null || projects.value.length > 0 : rows.value.length > 0));
const selected = computed(() => projects.value.find((p) => p.path === selectedPath.value) ?? null);
/** The selected team, while its head is drawn. */
const selectedTeam = computed(() => sections.value.find((section) => section.head && section.team !== null && section.team.id === selectedTeamId.value)?.team ?? null);
/** The project whose details and terminals show: none while a team is selected. */
const shown = computed(() => (selectedTeam.value ? null : selected.value));

/** In a flat list drawn while there are teams, a row is dragged among its own team's rows only
 * (lib/groupReorder: `data-zone`); with the heads drawn, or with no team, it goes anywhere. */
function zone(project: Project): string | undefined {
  return grouped.value || !teams.value.length ? undefined : (teamOf(project.path)?.id ?? "");
}

/** The list shows a project with a terminal open: its tabs tell Claude's state in them
 * (TerminalPanel), which the sessions hold. */
const listTabs = computed(() => view.value === "projects" && (shown.value?.terminals.length ?? 0) > 0);
watch(
  listTabs,
  (tabs) => {
    if (tabs && !sessionsLink) sessionsLink = connectSessions();
  },
  { immediate: true },
);

// Keep a selection while there is something to show. A project folded away under its head stays
// selected: only one the list holds no more gives way.
watch(
  listed,
  (list) => {
    if (!list.some((p) => p.path === selectedPath.value)) selectedPath.value = rows.value[0]?.path ?? list[0]?.path ?? null;
  },
  { immediate: true },
);

function count(id: Filter): number {
  const f = filters.find((x) => x.id === id);
  return f ? projects.value.filter(f.test).length : 0;
}

// A team that is gone takes its own view with it: the list is back on its filter.
watch(teams, (list) => {
  if (teamFilter.value && !list.some((team) => team.id === teamFilter.value)) teamFilter.value = null;
});

function pick(id: Filter): void {
  filter.value = id;
  teamFilter.value = null;
  view.value = "projects";
  // The list itself, not the setup steps in its place.
  setupOpen.value = false;
}

/** A team's sidebar entry: the list shows that team alone, selected until a row is clicked. */
function pickTeam(id: string): void {
  teamFilter.value = id;
  query.value = "";
  view.value = "projects";
  setupOpen.value = false;
  selectedTeamId.value = id;
}

/** A head's arrow. Folding a team away with the selected project in it selects the team. */
function fold(section: Section): void {
  if (!section.team) {
    looseFolded.value = !looseFolded.value;
    return;
  }
  if (!section.folded && !selectedTeam.value && section.rows.some((p) => p.path === selectedPath.value)) selectedTeamId.value = section.team.id;
  foldTeam(section.team.id, !section.folded);
}

/** The toolbar's Start all and Stop all are the shown team's in its own view, else everyone's. */
const shownState = computed(() => (shownTeam.value ? teamState(shownTeam.value) : null));
const acting = computed(() => shownState.value?.busy ?? anyBusy.value);

function startAll(): void {
  if (shownTeam.value) startTeam(shownTeam.value.id);
  else void api.startAll();
}

function stopAll(): void {
  if (shownTeam.value) stopTeam(shownTeam.value.id);
  else void api.stopAll();
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

/** Pressed from the keyboard, the button turns busy (disabled) and would drop the focus: the row keeps it. */
function toggleFrom(event: MouseEvent, project: Project): void {
  toggle(project);
  if (event.detail === 0) ((event.currentTarget as HTMLElement).closest("[data-path]") as HTMLElement | null)?.focus();
}

/** The line the keyboard is on, while it is in the list. */
const keyLine = ref<string | null>(null);
/** The list's one Tab stop: the line the keyboard is on; coming in, the selected team's head or
 * the selected row, and the first line while neither is drawn. */
const tabStop = computed(() => {
  const drawn = sections.value.flatMap((section) => [...(section.head ? [headKey(section.team?.id ?? "")] : []), ...(section.folded ? [] : section.rows.map((p) => rowKey(p.path)))]);
  const wanted = [keyLine.value, selectedTeam.value && headKey(selectedTeam.value.id), selectedPath.value && rowKey(selectedPath.value)];
  return wanted.find((key) => key && drawn.includes(key)) ?? drawn[0] ?? null;
});

function onListFocus(event: FocusEvent): void {
  const line = (event.target as HTMLElement).closest<HTMLElement>("[data-path], [data-group]");
  keyLine.value = line ? (line.dataset.path !== undefined ? rowKey(line.dataset.path) : headKey(line.dataset.group ?? "")) : null;
}

function onListBlur(event: FocusEvent): void {
  if (!tbody.value?.contains(event.relatedTarget as Node | null)) keyLine.value = null;
}

/** The list's rows and heads, top to bottom. */
function lines(): HTMLElement[] {
  return Array.from(tbody.value?.querySelectorAll<HTMLElement>(":scope > [data-path], :scope > [data-group]") ?? []);
}

/**
 * On a group's head: ←/→ reach its buttons, ↑/↓ go to the line above or below (a row is selected,
 * as the rows' own arrows do), and ⌥↑/⌥↓ move the team among the teams. True when the key was
 * the head's.
 */
function onHeadKey(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement;
  const head = target.closest<HTMLElement>("[data-group]");
  if (!head || event.metaKey || event.ctrlKey) return false;
  const id = head.dataset.group ?? "";

  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    if (event.altKey) return false;
    event.preventDefault();
    // The head, then its enabled buttons left to right, as a row's.
    const stops = [head, ...Array.from(head.querySelectorAll<HTMLElement>("button:not(:disabled)"))];
    stops[Math.min(stops.length - 1, Math.max(0, stops.indexOf(target) + (event.key === "ArrowRight" ? 1 : -1)))]?.focus();
    return true;
  }
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return false;

  event.preventDefault();
  const by = event.key === "ArrowDown" ? 1 : -1;
  if (event.altKey) {
    if (!id || shownTeam.value) return true;
    nudgeTeam(id, by);
    // The head moved in the page: the keyboard stays on it.
    void nextTick(() => lines().find((line) => line.dataset.group === id)?.focus());
    return true;
  }

  const all = lines();
  const next = all[all.indexOf(head) + by];
  if (!next) return true;
  if (next.dataset.path !== undefined) selectProject(next.dataset.path);
  next.focus();
  next.scrollIntoView({ block: "nearest" });
  return true;
}

/** The list is one Tab stop: arrows go along its rows and heads (a row is selected), ↵ opens the
 * editor, Space starts or stops. */
function onTableKey(event: KeyboardEvent): void {
  if (onHeadKey(event)) return;
  rowKeys(event, {
    move: selectProject,
    enter: (path) => void api.openEditor(path),
    space: (path) => {
      const project = rows.value.find((p) => p.path === path);
      if (project && project.status !== "busy") toggle(project);
    },
  });
}

/** ↓ in the search box goes on to the list's Tab stop. */
function focusSelected(): void {
  tbody.value?.querySelector<HTMLElement>('[tabindex="0"]')?.focus();
}

/* ---------- teams: menus and the small dialogs ---------- */

const menu = useNativeMenu();

/** Right-click on a row: the project goes to another team, to a new one, or out of its own. */
function rowMenu(event: MouseEvent, project: Project): void {
  selectProject(project.path);
  const own = teamOf(project.path);
  const entries: MenuEntry[] = teams.value.filter((team) => team.id !== own?.id).map((team) => ({ text: t("team.moveTo", { team: team.name }), action: () => moveToTeam(project.path, team.id) }));
  entries.push({ text: t("team.newMenu"), action: () => (teamAsk.value = { kind: "create", projects: [project.path], show: false }) });
  if (own) entries.push("separator", { text: t("team.leave"), action: () => void leaveTeam(project.path) });
  entries.push("separator", { text: t(project.muted ? "notifications.unmute" : "notifications.mute"), action: () => void api.muteProject(project.path, !project.muted) });
  // While `pitwall://` links are answered: the project's own, to copy (Raycast, Shortcuts, scripts).
  // Not for a name another listed project shares: a link on it does nothing.
  const named = projects.value.filter((other) => other.name === project.name).length;
  if (snapshot.value?.settings.cli?.links && named === 1) entries.push("separator", { text: t("cli.copyLink"), items: linkEntries(project.path) });
  void menu.popup(event, entries);
}

/** A project's `pitwall://` links, each copied by its line of the row's menu. */
function linkEntries(path: string): MenuEntry[] {
  const copy = (action: import("../lib/types").LinkAction) => () => void api.cliCopyLink(path, action);
  return [
    { text: t("common.start"), action: copy("start") },
    { text: t("common.restart"), action: copy("restart") },
    { text: t("common.stop"), action: copy("stop") },
    "separator",
    { text: t("cli.link.show"), action: copy("show") },
  ];
}

/** Right-click on a team, on its head in the list or its entry in the sidebar. */
function teamMenu(at: MenuPoint, team: Team): void {
  const state = teamState(team);
  void menu.popup(at, [
    state.running
      ? { text: t("team.stop"), enabled: !state.busy, action: () => stopTeam(team.id) }
      : { text: t("team.start"), enabled: !state.busy && state.total > 0, action: () => startTeam(team.id) },
    "separator",
    { text: t("team.renameMenu"), action: () => (teamAsk.value = { kind: "rename", team }) },
    { text: t("team.delete"), action: () => (teamAsk.value = { kind: "delete", team }) },
  ]);
}

/** What a new team can take in: the listed projects in no team, in the list's order, with the
 * ones it was asked for (a row's "New team…") first and ticked, whatever team those are in. */
const teamChoices = computed(() => {
  const asked = teamAsk.value?.kind === "create" ? teamAsk.value.projects : [];
  const loose = looseProjects.value.filter((project) => !asked.includes(project.path));
  return [...projects.value.filter((project) => asked.includes(project.path)), ...loose].map((project) => ({ path: project.path, name: project.name }));
});

/** A name was given: a team is renamed, or made with the projects ticked. A team made from the
 * sidebar is shown on its own; one made from a row's menu is selected where the list stands. */
async function saveTeamName(name: string, projects: string[]): Promise<void> {
  const ask = teamAsk.value;
  if (ask?.kind === "rename") renameTeam(ask.team.id, name);
  if (ask?.kind !== "create") return;
  const team = await createTeam(name, projects);
  if (!team) return;
  if (ask.show) pickTeam(team.id);
  else selectedTeamId.value = team.id;
}

function removeAskedTeam(): void {
  if (teamAsk.value?.kind === "delete") deleteTeam(teamAsk.value.team.id);
}

/** In full screen the traffic lights are gone, and so is the room kept for them. */
const fullscreen = ref(false);
let unlistenResize: UnlistenFn | null = null;

const terminalPanel = ref<InstanceType<typeof TerminalPanel> | null>(null);
const trackPage = ref<InstanceType<typeof TrackView> | null>(null);

/** ⌘T (Ctrl+Shift+T on Windows and Linux, lib/platform: terminalChord) opens a new terminal in the
 * selected project: the list's, or on the Track page the tower's. */
function onKey(event: KeyboardEvent): void {
  const terminals = terminalPanel.value ?? trackPage.value;
  if (terminalChord(event) && event.code === "KeyT" && terminals) {
    event.preventDefault();
    void terminals.openTerminal();
  }
}

let unlistenReveal: UnlistenFn | null = null;
let unlistenProject: UnlistenFn | null = null;
let unlistenFuelReveal: UnlistenFn | null = null;
let unlistenTerminal: UnlistenFn | null = null;
let unlistenPage: UnlistenFn | null = null;

/** From the switcher (`/`): that page of the window. Settings opens over the page shown and "Add
 * project" asks for the folder; any other page closes Settings, which would cover it. */
function revealPage(page: Page, tab?: "notifications"): void {
  if (page === "settings") openSettings(tab);
  else if (page === "add") void api.pickFolder();
  else {
    settingsOpen.value = false;
    if (isFilter(page)) pick(page);
    else view.value = page;
  }
}

/** From the switcher (⌘↵ on a link) or a notification: select the project, even if a filter or
 * search hid it. Settings, which would cover it, closes. */
function revealProject(path: string): void {
  settingsOpen.value = false;
  view.value = "projects";
  if (!rows.value.some((p) => p.path === path)) {
    filter.value = "all";
    teamFilter.value = null;
    query.value = "";
    // Folded away under its head: that group opens.
    const team = teamOf(path);
    if (team?.collapsed) foldTeam(team.id, false);
    else if (!team) looseFolded.value = false;
  }
  selectProject(path);
}

/** From the switcher (⌘↵ on a command): the project, with that command's output showing. */
function revealOutput(path: string, job: string): void {
  revealProject(path);
  outputRequest.value = { path, job };
}

/**
 * A Claude session in one of Pitwall's terminals brought up, from whatever page (or the popover,
 * the switcher, a notification): it shows where its work was started. Given to Claude from a
 * card: that card on its project's board, its terminal open in the card's details (BoardView
 * takes the request). Opened in a project's terminal panel: the project, its panel open on that
 * tab (TerminalPanel takes the request), on the page it was opened from: the Track page, else the
 * list. Either way the keyboard is in it.
 */
async function revealTerminal(path: string, id: number): Promise<void> {
  settingsOpen.value = false;
  // The board's cards, to know whether one runs in that terminal.
  if (!boardLink) boardLink = connectBoard();
  await boardLink.then(
    () => undefined,
    () => undefined,
  );
  const cards = (board.value ?? []).filter((entry) => onBoard(entry, null, now.value));
  // The session that runs there: as the sessions' list tells it, else as the card started in that
  // terminal holds it (the list comes only once a page that shows it has opened).
  const session = sessions.value?.sessions.find((row) => row.origin.kind === "pitwall" && row.origin.terminal === id)?.id ?? cards.find((entry) => entry.terminal === id)?.session ?? null;
  // A session several cards hold works for the one given last (core/board.rs: `follow_board`).
  const card = cards
    .filter((entry) => entry.terminal === id || (session !== null && entry.session === session))
    .reduce<Card | null>((last, entry) => (!last || (entry.givenAt ?? 0) >= (last.givenAt ?? 0) ? entry : last), null);
  if (card) {
    cardRequest.value = { id: card.id, path: card.path };
    view.value = "board";
  } else {
    // Opened from the Track page's panel: there (the page selects the project in its tower).
    if (trackTerminals.has(id)) view.value = "track";
    else revealProject(path);
    terminalRequest.value = { path, id, at: Date.now() };
  }
}

/** The Sessions page's "Open a Claude terminal": one, in the project selected in the list. */
async function openClaudeTerminal(): Promise<void> {
  view.value = "projects";
  // The project's terminals show under the list: a team's details give way to its own.
  selectedTeamId.value = null;
  await nextTick();
  void terminalPanel.value?.openTerminal(true);
}

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  unlistenReveal = await listen<{ path: string; job: string }>("reveal-output", (event) => revealOutput(event.payload.path, event.payload.job));
  unlistenProject = await listen<{ path: string }>("reveal-project", (event) => revealProject(event.payload.path));
  // A click on the 90 % notification: the Fuel page.
  unlistenFuelReveal = await listen("reveal-fuel", () => (view.value = "fuel"));
  // Bringing up a Claude session that runs in one of Pitwall's terminals: that terminal.
  unlistenTerminal = await listen<{ path: string; id: number }>("reveal-terminal", (event) => void revealTerminal(event.payload.path, event.payload.id));
  // `/` in the switcher: one of the sidebar's pages.
  unlistenPage = await listen<{ page: Page; tab?: "notifications" }>("reveal-page", (event) => revealPage(event.payload.page, event.payload.tab));
  // Listening now: on its first opening the window comes up, with what the switcher sent meanwhile.
  void api.windowReady();
  const win = getCurrentWindow();
  const check = async (): Promise<void> => {
    fullscreen.value = await win.isFullscreen();
  };
  await check();
  unlistenResize = await win.onResized(() => void check());
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  unlistenReveal?.();
  unlistenProject?.();
  unlistenFuelReveal?.();
  unlistenTerminal?.();
  unlistenPage?.();
  unlistenResize?.();
});

function server(project: Project): string {
  return meta(project, now.value);
}
</script>

<template>
  <div :class="['shell', { fullscreen }]">
    <aside class="sidebar">
      <!-- "deep": the logo and name drag the window too; so does the strip under the traffic lights.
           macOS only (lib/platform: dragRegion): elsewhere the window has its own title bar. -->
      <div class="brand" :data-tauri-drag-region="dragRegion">
        <PitwallGlyph :size="30" lamps="var(--run)" />
        <span>Pitwall</span>
        <!-- Across from the logo: the installed version, and at the far right the way to a new one.
             The version drags the window with the rest; the button takes its own click (Tauri
             leaves a button out of a drag region, as it does the toolbar's). -->
        <span v-if="update" class="brand-end">
          <span class="version">{{ update.current }}</span>
          <button
            v-if="updateButton"
            type="button"
            class="update"
            :disabled="installing"
            :aria-busy="installing"
            :aria-label="installing ? t('about.update.installing', { version: update.version ?? '' }) : t('update.button')"
            :title="installing ? undefined : t('update.button')"
            @click="updateOpen = true"
          >
            <Spinner v-if="installing" />
            <Icon v-else name="download" :size="14" />
          </button>
        </span>
      </div>
      <!-- The filters, the teams and the pages: in a window too short for them all, they scroll
           between the brand and the foot. -->
      <div class="side-scroll">
        <nav :aria-label="t('window.filters')" class="nav">
          <button
            v-for="f in filters"
            :key="f.id"
            type="button"
            :class="{ on: view === 'projects' && !shownTeam && filter === f.id }"
            :aria-current="view === 'projects' && !shownTeam && filter === f.id ? 'page' : undefined"
            @click="pick(f.id)"
          >
            <Icon :name="f.icon" /> {{ t(f.label) }}
            <span :class="['count', { hot: f.id === 'waiting' && count(f.id) > 0 }]">{{ count(f.id) }}</span>
          </button>
        </nav>
        <!-- The teams: an entry each, the list's view of that team alone. Its label and "+" are
             always here, so the first team can be made. -->
        <nav :aria-label="t('team.section')" class="nav team-nav">
          <div class="team-nav-head">
            <span class="section-label">{{ t("team.section") }}</span>
            <button type="button" class="team-add" :aria-label="t('team.new')" :title="t('team.new')" @click="teamAsk = { kind: 'create', projects: [], show: true }">
              <Icon name="plus" :size="13" />
            </button>
          </div>
          <!-- Dragged into order, or moved with ⌥↑ and ⌥↓; a press that doesn't travel is a click. -->
          <div ref="teamList" :class="['team-list', teamDrag.listClass()]">
            <button
              v-for="entry in teamNav"
              :key="entry.team.id"
              type="button"
              :data-path="entry.team.id"
              :class="[{ on: view === 'projects' && shownTeam?.id === entry.team.id }, teamDrag.rowClass(entry.team.id)]"
              :style="teamDrag.rowStyle(entry.team.id)"
              :aria-current="view === 'projects' && shownTeam?.id === entry.team.id ? 'page' : undefined"
              aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
              @pointerdown="teamDrag.down($event, entry.team.id)"
              @click="teamDrag.isClick() && pickTeam(entry.team.id)"
              @keydown="onTeamNavKey($event, entry.team.id)"
              @contextmenu="teamMenu($event, entry.team)"
            >
              <Icon name="users" /> <span class="nav-label">{{ entry.team.name }}</span>
              <!-- What Claude is at in the team, what waits on you first: its rows may be folded away. -->
              <ClaudeDot v-if="entry.claude.length" :live="{ phase: entry.claude[0].phase, text: entry.claude.map((note) => note.text).join(' · ') }" />
              <span :class="['team-count', { live: entry.running > 0 }]" :aria-label="t('team.runningOf', { running: entry.running, total: entry.total })">{{ entry.running }}/{{ entry.total }}</span>
            </button>
          </div>
        </nav>
        <div class="nav today">
          <button type="button" :class="{ on: view === 'day' }" :aria-current="view === 'day' ? 'page' : undefined" @click="view = 'day'"><Icon name="calendar" /> {{ t("day.nav") }}</button>
          <button type="button" :class="{ on: view === 'sessions' }" :aria-current="view === 'sessions' ? 'page' : undefined" @click="view = 'sessions'">
            <Icon name="sparkles" /> <span class="nav-label">{{ t("sessions.nav") }}</span>
          </button>
          <button type="button" :class="{ on: view === 'board' }" :aria-current="view === 'board' ? 'page' : undefined" @click="view = 'board'">
            <Icon name="board" /> <span class="nav-label">{{ t("board.nav") }}</span>
          </button>
          <button type="button" :class="{ on: view === 'track' }" :aria-current="view === 'track' ? 'page' : undefined" @click="view = 'track'"><Icon name="flag" /> {{ t("track.nav") }}</button>
          <button type="button" :class="{ on: view === 'fuel' }" :aria-current="view === 'fuel' ? 'page' : undefined" @click="view = 'fuel'">
            <Icon name="fuel" /> <span class="nav-label">{{ t("fuel.nav") }}</span>
            <span v-if="fuelBadge" :class="['fuel-badge', { low: fuelBadge.low, stale: fuelBadge.stale }]">{{ fuelBadge.text }}</span>
          </button>
          <button type="button" :class="{ on: view === 'deps' }" :aria-current="view === 'deps' ? 'page' : undefined" @click="view = 'deps'">
            <Icon name="tools" /> <span class="nav-label">{{ t("deps.nav") }}</span>
            <span v-if="depsBadge" class="deps-badge" :aria-label="t('deps.group.attention', { count: depsBadge })">{{ depsBadge }}</span>
          </button>
        </div>
      </div>
      <div class="nav foot">
        <button type="button" @click="api.pickFolder()"><Icon name="plus" /> {{ t("window.addProject") }}</button>
        <button type="button" :class="{ on: settingsOpen }" @click="openSettings()"><Icon name="settings" /> {{ t("common.settings") }}</button>
      </div>
    </aside>

    <DayView v-if="view === 'day'" />
    <TrackView v-else-if="view === 'track'" ref="trackPage" @open-project="revealProject" @open-settings="openSettings('track')" />
    <FuelView v-else-if="view === 'fuel'" />
    <SessionsView
      v-else-if="view === 'sessions'"
      :claude-project="selected?.name ?? null"
      @open-project="revealProject"
      @open-settings="openSettings('claude')"
      @open-claude="openClaudeTerminal"
    />
    <BoardView v-else-if="view === 'board'" :project-path="selected?.path ?? null" @open-settings="openSettings('claude')" />
    <DepsView v-else-if="view === 'deps'" @open-project="revealProject" @open-output="revealOutput" />
    <!-- The first run's steps, in place of the list and its details. -->
    <SetupView v-else-if="setupOpen" @done="setupOpen = false" />

    <template v-else>
      <main class="main">
        <!-- Its heading and free space drag the window; the search box and buttons stay clickable. -->
        <div class="toolbar" :data-tauri-drag-region="dragRegion">
          <div class="heading">{{ heading }}</div>
          <label class="sr" for="search">{{ t("common.searchProjects") }}</label>
          <input id="search" v-model="query" type="search" :placeholder="t('common.searchProjects')" @keydown.down.prevent="focusSelected" />
          <Spinner v-if="acting" class="busy-spin" />
          <button type="button" class="control" :disabled="!(shownState ? shownState.total : projects.length) || acting" @click="startAll">{{ t("common.startAll") }}</button>
          <button type="button" class="control" :disabled="!(shownState ? shownState.running : snapshot?.running) || acting" @click="stopAll">{{ t("common.stopAll") }}</button>
        </div>

        <span id="rows-keys" class="sr-only">
          {{ t("common.rowKeys") }}
          <template v-if="grouped">{{ t("team.headKeys", { up: keys("Alt+ArrowUp"), down: keys("Alt+ArrowDown") }) }}</template>
        </span>
        <!-- The list is one Tab stop: the selected row (or team's head) takes the focus; ↑/↓ move
             it along rows and heads, ←/→ reach a line's buttons. -->
        <div v-if="listShown" class="table" role="grid" :aria-label="heading" aria-describedby="rows-keys">
          <div class="thead" role="row">
            <span role="columnheader">{{ t("window.col.project") }}</span>
            <span role="columnheader">{{ t("window.col.branch") }}</span>
            <span role="columnheader">{{ t("window.col.server") }}</span>
            <span role="columnheader">{{ t("window.col.claude") }}</span>
            <span role="columnheader" :aria-label="t('window.col.actions')"></span>
          </div>
          <!-- Heads, rows and the line of an empty team are all the list's own children, one under
               the other: the keyboard (lib/rows) and the drag (lib/groupReorder) count on it. -->
          <div ref="tbody" :class="['tbody', { grouped }, drag.listClass()]" role="rowgroup" @keydown="onTableKey" @focusin="onListFocus" @focusout="onListBlur">
            <template v-for="section in sections" :key="section.key">
              <TeamHead
                v-if="section.head"
                :team="section.team"
                :total="section.total"
                :running="section.running"
                :claude="section.claude"
                :busy="section.busy"
                :folded="section.folded"
                :selected="section.team !== null && section.team.id === selectedTeam?.id"
                :foldable="!shownTeam"
                :tab-stop="tabStop === headKey(section.team?.id ?? '')"
                :class="drag.lineClass(headKey(section.team?.id ?? ''))"
                :style="drag.lineStyle(headKey(section.team?.id ?? ''))"
                @pointerdown="section.team && drag.down($event, headKey(section.team.id))"
                @contextmenu="section.team && teamMenu($event, section.team)"
                @select="pressHead(section)"
                @fold="fold(section)"
              />
              <template v-if="!section.folded">
                <div
                  v-for="project in section.rows"
                  :key="project.path"
                  role="row"
                  :aria-selected="project.path === shown?.path"
                  :tabindex="tabStop === rowKey(project.path) ? 0 : -1"
                  :data-path="project.path"
                  :data-zone="zone(project)"
                  :class="['trow', { selected: project.path === shown?.path }, drag.lineClass(rowKey(project.path))]"
                  :style="drag.lineStyle(rowKey(project.path))"
                  :title="t('window.rowTitle')"
                  @pointerdown="drag.down($event, rowKey(project.path))"
                  @click="select(project.path)"
                  @dblclick="api.openEditor(project.path)"
                  @contextmenu="rowMenu($event, project)"
                >
                  <span class="cell-project" role="gridcell">
                    <StatusIcon :project="project" ring="var(--bg-app)" />
                    <span class="names">
                      <span class="name">
                        <span class="name-text">{{ project.name }}</span>
                        <MutedMark v-if="project.muted" />
                        <span v-if="project.terminals.length" class="term-badge" role="img" :aria-label="t('window.openTerminals', { count: project.terminals.length })" :title="t('window.openTerminals', { count: project.terminals.length })">>_ {{ project.terminals.length }}</span>
                      </span>
                      <span class="path">{{ project.path }}</span>
                    </span>
                  </span>
                  <span :class="['cell-git', { dirty: project.git?.changes }]" role="gridcell">{{ gitLine(project.git) || "—" }}</span>
                  <span :class="['cell-server', { bad: project.status === 'crashed', on: project.status === 'running' }]" role="gridcell"><Marquee :text="server(project)" /></span>
                  <span :class="['cell-claude', { hot: project.claude, live: !project.claude && project.claudeWorking }]" role="gridcell"><Marquee :text="claudeState(project, now) || '—'" /></span>
                  <span class="cell-actions" role="gridcell" data-no-drag @click.stop @dblclick.stop>
                    <button
                      type="button"
                      class="icon"
                      tabindex="-1"
                      :aria-label="t(project.favourite ? 'window.unfavouriteName' : 'window.favouriteName', { name: project.name })"
                      :title="t(project.favourite ? 'window.favourite' : 'window.addFavourite')"
                      @click="api.setFavourite(project.path, !project.favourite)"
                    >
                      <Icon :name="project.favourite ? 'star-filled' : 'star'" :size="14" />
                    </button>
                    <button
                      type="button"
                      class="icon"
                      tabindex="-1"
                      :disabled="project.status === 'busy'"
                      :aria-label="t(project.status === 'running' ? 'common.stopName' : 'common.startName', { name: project.name })"
                      @click="toggleFrom($event, project)"
                    >
                      <Spinner v-if="project.status === 'busy'" />
                      <Icon v-else :name="project.status === 'running' ? 'stop' : 'play'" :size="14" />
                    </button>
                  </span>
                </div>
                <!-- A team with no project yet: one line under its head says how one gets in; in the
                     team's own view, where there is no row to drag, only that it has none. -->
                <div
                  v-if="section.team && !section.rows.length"
                  role="row"
                  :data-hint="section.team.id"
                  :class="['ghint', drag.lineClass(hintKey(section.team.id))]"
                  :style="drag.lineStyle(hintKey(section.team.id))"
                >
                  <span role="gridcell" aria-colspan="5">{{ t(shownTeam ? "team.noMembers" : "team.empty") }}</span>
                </div>
              </template>
            </template>
          </div>
        </div>

        <section v-else class="empty">
          <PitwallGlyph :size="44" lamps="var(--idle-ring)" />
          <template v-if="projects.length">
            <p>{{ t("window.noMatch") }}</p>
          </template>
          <template v-else>
            <p>{{ t("window.emptyLine1") }}<br />{{ t("window.emptyLine2") }}</p>
            <button type="button" class="control" @click="api.pickFolder()">{{ t("common.addProject") }}</button>
          </template>
        </section>

        <!-- Projects that appeared in the projects folder, offered under the list. -->
        <ProjectSuggestions />

        <!-- The selected project's terminals, level with the output panel beside it. -->
        <TerminalPanel v-if="shown" ref="terminalPanel" :project="shown" />
      </main>

      <!-- A selected team's details take the pane; else the selected project's. -->
      <TeamDetail v-if="selectedTeam" :team="selectedTeam" />
      <ProjectDetail v-else-if="selected" :project="selected" />
      <!-- Nothing to show: the panel stays blank but keeps its place, so the layout never jumps.
           The middle panel already shows the glyph and what to do. -->
      <section v-else class="detail-empty" :aria-label="t('window.projectDetails')"></section>
    </template>

    <SettingsView v-if="settingsOpen" :start-tab="settingsTab" @close="settingsOpen = false" @open-update="updateOpen = true" @open-setup="openSetup" @open-whats-new="whatsNewOpen = true" />
    <!-- Opened from Settings › About too: the later dialog comes up over that one. -->
    <UpdateDialog v-if="updateOpen && update" :update="update" @close="updateOpen = false" />
    <!-- What was open at the last quit, asked once; then this version's notes, which Settings
         › About opens too. -->
    <ResumeDialog v-if="resumeOffer" :offer="resumeOffer" @close="resumeClosed = true" />
    <WhatsNewDialog v-if="whatsNewOpen" @close="whatsNewOpen = false" />
    <FlagDialog />
    <!-- Naming a team (a new one, or a rename) and deleting one, asked for from the sidebar, the
         list or the team's details (lib/teams: teamAsk). -->
    <TeamNameDialog v-if="teamAsk?.kind === 'create'" :name="null" :choices="teamChoices" :picked="teamAsk.projects" @save="saveTeamName" @close="teamAsk = null" />
    <TeamNameDialog v-else-if="teamAsk?.kind === 'rename'" :name="teamAsk.team.name" @save="saveTeamName" @close="teamAsk = null" />
    <TeamDeleteDialog v-else-if="teamAsk?.kind === 'delete'" :team="teamAsk.team" @remove="removeAskedTeam" @close="teamAsk = null" />
  </div>
</template>

<style scoped>
.shell {
  height: 100%;
  display: flex;
  overflow: clip;
}

.sidebar {
  width: 232px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 0 10px 14px;
  background: var(--bg-sidebar);
  border-right: 1px solid var(--line);
}

/* No traffic lights: the brand moves up, level with the toolbar heading. So it is on Windows and
   Linux, whose title bar sits above the window's content rather than over it. */
.shell.fullscreen .brand,
:root:not([data-platform="mac"]) .brand {
  padding-top: 13px;
}

/* Reaches the window's top edge, room for the traffic lights included, and the sidebar's full
   width, so the whole corner drags the window. */
.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0 -10px;
  padding: 44px 18px 18px;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-brand);
}

/* A long version gives way; the button beside it never does. */
.brand-end {
  margin-left: auto;
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.version {
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 400;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The same box with the arrow or the spinner in it. */
.update {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text);
}

.update:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav button {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 34px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 13px;
  color: var(--text-nav);
  text-align: left;
}

.nav button:hover {
  background: var(--bg-nav-hover);
}

.nav button.on {
  background: var(--bg-lit);
  color: var(--text-strong);
}

.count {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
}

.count.hot {
  color: var(--claude);
  font-weight: 700;
}

.today {
  margin-top: 12px;
}

/* The teams, between the filters and the pages. */
.team-nav {
  margin-top: 12px;
}

.team-nav-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  height: 26px;
  padding: 0 4px 0 10px;
}

.team-nav-head .section-label {
  flex-grow: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The entries themselves, apart from the label's row: the box a dragged one stays in. */
.team-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

/* Drag to reorder, as the list's rows (lib/reorder): the entry lifts and follows the pointer,
   the others slide aside, and on release it slides into place. */
.team-list.reordering button {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.team-list.reordering button:hover:not(.on, .dragging) {
  background: transparent;
}

.team-list button.dragging {
  position: relative;
  z-index: 2;
  background: var(--bg-lit);
  color: var(--text-strong);
  box-shadow: 0 10px 18px -12px var(--shade-drag);
  transition: none;
}

.team-list.reordering button.dragging.settling {
  transition: transform 160ms ease-out;
}

.team-list.frozen button {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .team-list.reordering button,
  .team-list.reordering button.dragging.settling {
    transition: none;
  }
}

/* Smaller than a sidebar entry: it sits on the label's line. */
.nav .team-add {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
  justify-content: center;
  padding: 0;
  border-radius: 6px;
  color: var(--text-subtle);
}

.nav .team-add:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* How many of the team's projects run, of how many: green once one does. */
.team-count {
  flex-shrink: 0;
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.team-count.live {
  color: var(--run);
}

/* A long name gives way; the badge beside it never does. */
.nav-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Claude's: the session's share left, beside Fuel. */
.fuel-badge {
  flex-shrink: 0;
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--claude-text);
  white-space: nowrap;
}


.fuel-badge.low {
  color: var(--crash-text);
}

.fuel-badge.stale {
  color: var(--text-subtle);
}

/* The projects whose dependencies need you, beside Garage: the attention yellow. */
.deps-badge {
  flex-shrink: 0;
  margin-left: auto;
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--warn);
}

/* All the room between the brand and the foot. Too short for what it holds (a small window,
   many teams), it scrolls: its bar takes the sidebar's own padding, and what it holds keeps its
   width (the sidebar's 232px less its padding and line), so nothing shifts when the bar comes.
   The room around is for the keyboard's ring, which the scrolling would cut. */
.side-scroll {
  flex: 1 1 auto;
  min-height: 0;
  margin: -3px -10px 0 -3px;
  padding: 3px 0 3px 3px;
  overflow-x: hidden;
  overflow-y: auto;
}

.side-scroll > .nav {
  width: 211px;
}

.foot {
  flex-shrink: 0;
  padding-top: 6px;
}

/* The shortest window (560px): the sidebar's parts close up, each entry as tall as ever, so that
   with no team all of them show without scrolling. */
@media (max-height: 600px) {
  .sidebar {
    padding-bottom: 8px;
  }

  .brand {
    padding-bottom: 8px;
  }

  .team-nav,
  .today {
    margin-top: 6px;
  }

  .team-nav-head {
    height: 22px;
  }

  .foot {
    padding-top: 2px;
  }
}

.main {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

.toolbar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.heading {
  font-size: 15px;
  font-weight: 600;
  flex-grow: 1;
}

.busy-spin {
  color: var(--text-muted);
}

.sr {
  position: absolute;
  left: -9999px;
}

input[type="search"] {
  width: 220px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

input[type="search"]::placeholder {
  color: var(--text-faint);
}

.control {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}


.control:disabled {
  opacity: 0.4;
  cursor: default;
}

.table {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex-grow: 1;
}

.thead,
.trow {
  display: grid;
  grid-template-columns: minmax(0, 1.3fr) minmax(0, 1fr) minmax(0, 1fr) minmax(0, 0.9fr) 76px;
  gap: 12px;
  align-items: center;
}

/* Fixed height: the details header (toolbar 56 + 38 = 94px) lines up with this line. */
.thead {
  flex-shrink: 0;
  height: 38px;
  padding: 0 30px 0 20px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
  border-bottom: 1px solid var(--line-soft);
}

:lang(zh) .thead,
:lang(ja) .thead {
  letter-spacing: 0;
}

/* Fills the panel down to the terminals, so a row can be dragged anywhere in it (dropped in the
   empty space it slides up to the last place) and its shadow is never cut. The same room on
   every side; a dragged row keeps it from the edges too (lib/groupReorder). */
.tbody {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 10px;
}

/* In groups, each head brings its own room above it (team/TeamHead): the first sits as far from
   the table's head line as a first row does. */
.tbody.grouped {
  padding-top: 4px;
}

.trow {
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
}

/* The line of a team with no project yet, under its head, level with the team's name. */
.ghint {
  padding: 4px 10px 8px 31px;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The keyboard's row: the ring inside, so neighbours and the list's edge never cut it. */
.trow:focus-visible {
  outline-offset: -2px;
}

.trow:hover {
  background: var(--bg-row-hover);
}

.trow.selected {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

/* Drag to reorder: the dragged row lifts and follows the pointer, the others slide aside to
   show where it lands, and on release it slides into place (lib/groupReorder). A team's head
   does the same among the heads, and carries its rows along. */
.tbody.reordering :is(.trow, .ghead, .ghint) {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.tbody.reordering .trow:hover:not(.selected, .dragging) {
  background: transparent;
}

/* The shadow falls below the row only: sideways it would be cut at the list's edges. */
.trow.dragging {
  position: relative;
  z-index: 2;
  background: var(--bg-selected);
  box-shadow:
    0 14px 22px -14px var(--shade-drag),
    inset 0 0 0 1px var(--line-selected);
}

/* What a head carries, and the head itself: over the lines they pass, which don't show through. */
.tbody.reordering .carried:not(.trow.dragging) {
  position: relative;
  z-index: 2;
  background: var(--bg-app);
}

.tbody.reordering .carried {
  transition: none;
}

.tbody.reordering .carried.settling {
  transition: transform 160ms ease-out;
}

.tbody.frozen :is(.trow, .ghead, .ghint) {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .tbody.reordering :is(.trow, .ghead, .ghint),
  .tbody.reordering .carried.settling {
    transition: none;
  }
}

.cell-project {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  padding: 2px 0;
}

.names {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

/* Name and terminal badge on one line; a long name gives way, the badge never does. */
.name {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 13px;
  font-weight: 500;
}

.name-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.term-badge {
  flex-shrink: 0;
  padding: 1px 6px;
  border-radius: 6px;
  background: var(--bg-well);
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 400;
  color: var(--text-muted);
}

.path {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cell-git,
.cell-server,
.cell-claude {
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-subtle);
}

.cell-server {
  font-family: var(--font-mono);
}

.cell-server.on {
  color: var(--text-soft);
}

.cell-server.bad {
  color: var(--crash-text);
}

.cell-claude.hot {
  color: var(--claude-text);
}

.cell-claude.live {
  color: var(--text-muted);
}

.cell-git {
  font-family: var(--font-mono);
}

.cell-git.dirty {
  color: var(--text-soft);
}

.cell-actions {
  display: flex;
  justify-content: flex-end;
  gap: 2px;
}

.icon {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.icon:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.icon:disabled {
  opacity: 0.6;
}

.empty {
  flex-grow: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 16px;
  text-align: center;
  color: var(--text-muted);
}

.empty p {
  margin: 0;
  line-height: 1.6;
}

.detail-empty {
  width: 380px;
  flex-shrink: 0;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
}
</style>
