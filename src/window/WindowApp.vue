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
import { connectAccounts } from "../lib/accounts";
import { claudeState, gitLine, meta } from "../lib/format";
import { isLow, isStale, left, percentText, sessionWindow } from "../lib/fuel";
import { searchProjects } from "../lib/fuzzy";
import { language, t } from "../lib/i18n";
import { filters, isFilter, openOrAtWork, type Filter, type Page, type View } from "../lib/pages";
import { onBoard } from "../lib/board";
import { headKey, hintKey, parseKey, rowKey, useGroupReorder } from "../lib/groupReorder";
import { useReorder } from "../lib/reorder";
import { useNativeMenu, type MenuEntry, type MenuPoint } from "../lib/nativeMenu";
import { cardRequest, COLLAPSED_HEIGHT, followProject, openInPanel, panelOver, panelPath, panelTyping, revealTab, showOutput, terminalCollapsed } from "../lib/panel";
import { closeChord, dragRegion, isMac, isWindows, keys, terminalChord } from "../lib/platform";
import { closeFiles, closeShownTab, filesWindows, frontFiles, minimizeFiles, raiseFiles, showFiles, toggleFiles, type FilesWin } from "../lib/explorer";
import { focusTerminal, listRequest, openInWindow, openTarget } from "../lib/open";
import { rowKeys } from "../lib/rows";
import { needsAttention } from "../lib/deps";
import { api, board, connectBoard, connectDeps, connectFuel, connectSessions, connectUpdate, deps, fuel, loadBoard, loadSessions, now, sessions, snapshot, update, visible } from "../lib/store";
import { applyDrop, claudeNotes, createTeam, deleteTeam, leaveTeam, looseProjects, membersOf, moveToTeam, nudgeTeam, orderTeams, renameTeam, startTeam, stopTeam, teamAsk, teamOf, teams, teamState } from "../lib/teams";
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
import FilesWindows from "./files/FilesWindows.vue";
import WhatsNewDialog from "./WhatsNewDialog.vue";
import TeamDeleteDialog from "./team/TeamDeleteDialog.vue";
import TeamHead from "./team/TeamHead.vue";
import TeamNameDialog from "./team/TeamNameDialog.vue";
import TitleBar from "./TitleBar.vue";

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

/** The accounts' syncs (lib/accounts), from the start too: a switch made in Settings is told on
 * the Fuel page. */
let accountsLink: Promise<UnlistenFn> | null = null;
onMounted(() => {
  accountsLink = connectAccounts();
});
onBeforeUnmount(() => {
  void accountsLink?.then((unlisten) => unlisten(), () => undefined);
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
// page names a project's sessions on its card. The terminal panel needs them for its tabs alone
// (`panelTabs`).
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
  if (view.value === "sessions" || view.value === "board" || view.value === "track" || panelTabs.value) void loadSessions();
  if (view.value === "board") void loadBoard();
});
const settingsOpen = ref(false);
/** Settings opens on this tab: the Sessions page's "Add hook" opens it on Claude's, the Track
 * page's settings button on Track's, the popover's "Quiet hours…" on Notifications'. The first
 * run's phone row opens it on Notifications' too, at the phone's part (`settingsPart`); the Fuel
 * page's account panel on Claude's, at its accounts (with the add dialog open for "Add account"). */
const settingsTab = ref<"claude" | "track" | "notifications" | undefined>(undefined);
type SettingsPart = "phone" | "accounts" | "addAccount";
const settingsPart = ref<SettingsPart | undefined>(undefined);

function openSettings(tab?: "claude" | "track" | "notifications", part?: SettingsPart): void {
  settingsTab.value = tab;
  settingsPart.value = part;
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
/** The projects in no team, shown on their own (the last entry under the sidebar's teams). */
const looseShown = ref(false);
/** The team selected in the list: its details show in place of a project's. */
const selectedTeamId = ref<string | null>(null);
const tbody = ref<HTMLElement | null>(null);
/** Drag to reorder: a row in its group or into another, a team's head among the teams. */
const drag = useGroupReorder(tbody, applyDrop);
/** The team the pointer is on, on its head or one of its rows: its lines light up and its head
 * shows the team's start or stop. */
const litTeam = ref<string | null>(null);

function onListPointer(event: PointerEvent): void {
  const line = (event.target as HTMLElement).closest<HTMLElement>("[data-team], [data-group]");
  litTeam.value = line?.dataset.team || line?.dataset.group || null;
}

/** A team's row (or its empty line) while a drag is on: the row moves as the drag says, the
 * team's line drawn beside it only as its head does (`--tie-shift`). The line stands still while
 * the team's rows trade places, and goes with the team when the team is carried. */
function teamLineStyle(key: string, team: Team): Record<string, string> {
  return { ...drag.lineStyle(key), "--tie-shift": `${drag.offset(headKey(team.id)) - drag.offset(key)}px` };
}

/** A row away from its piece of the line while a project is dragged: the one in hand, or one
 * slid aside to make room. Its branch draws back as a selected row's does, so it doesn't reach
 * into whatever row is passing its place. */
function offLine(key: string, team: Team): boolean {
  const dragged = drag.dragging.value;
  if (!dragged || parseKey(dragged).kind !== "row") return false;
  return key === dragged || drag.offset(key) !== drag.offset(headKey(team.id));
}

function selectProject(path: string): void {
  selectedPath.value = path;
  selectedTeamId.value = null;
}

function select(path: string): void {
  if (drag.isClick()) selectProject(path);
}

/** A click on a team's head selects the team: its details show. */
function pressHead(section: Section): void {
  if (drag.isClick() && section.team) selectedTeamId.value = section.team.id;
}

const projects = computed(() => snapshot.value?.projects ?? []);
const current = computed(() => filters.find((f) => f.id === filter.value) ?? filters[0]);
const shownTeam = computed(() => teams.value.find((team) => team.id === teamFilter.value) ?? null);
/** The toolbar's heading and the list's name: the team shown, the projects in no team, else the filter. */
const heading = computed(() => shownTeam.value?.name ?? t(looseShown.value ? "team.loose" : current.value.label));
/** What the list draws from before a search and "Only favourites and open": the team shown, the
 * projects in no team, else the filter's. */
const pool = computed(() => (shownTeam.value ? membersOf(shownTeam.value) : looseShown.value ? looseProjects.value : projects.value.filter((p) => current.value.test(p))));

/** The sidebar's teams, each with how many of its projects run. */
const teamNav = computed(() => teams.value.map((team) => ({ team, ...teamState(team) })));
/** The sidebar's last entry under the teams, the projects in no team: counted as a team is. */
const looseNav = computed(() => ({
  total: looseProjects.value.length,
  running: looseProjects.value.filter((project) => project.status === "running").length,
  claude: claudeNotes(looseProjects.value),
}));

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

/** "Only favourites and open" (the toolbar's switch, kept in the settings): the favourites, and
 * another project only while it is open or at work. */
const favouritesOnly = computed(() => snapshot.value?.settings.favouritesOnly === true);

function drawn(project: Project): boolean {
  return !favouritesOnly.value || project.favourite || openOrAtWork(project);
}

/** The switch turned: drawn at once, kept by the core. */
function setFavouritesOnly(on: boolean): void {
  if (snapshot.value) snapshot.value.settings.favouritesOnly = on;
  void api.setFavouritesOnly(on);
}

/** The list is drawn in groups, each under its head: a team's own view, and every project while
 * there are teams. The other filters, the projects in no team and a search stay flat, as with no
 * team at all. */
const grouped = computed(() => query.value.trim() === "" && (shownTeam.value !== null || (!looseShown.value && filter.value === "all" && teams.value.length > 0)));

/** A stretch of the list: a group under its head (`team` null: the projects in no team, whose
 * head draws nothing), or the whole flat list (`head` false). Groups never fold. */
type Section = { key: string; head: boolean; team: Team | null; rows: Project[]; total: number; running: number; busy: boolean };

const sections = computed<Section[]>(() => {
  if (!grouped.value) {
    const q = query.value.trim().toLowerCase();
    const listed = pool.value.filter(drawn);
    // Searching: the quick switcher's fuzzy match and order (lib/fuzzy); else the list's own order.
    const found = q ? searchProjects(listed, q).map((match) => match.project) : listed;
    return [{ key: "flat", head: false, team: null, rows: found, total: found.length, running: 0, busy: false }];
  }

  const ofTeam = (team: Team): Section => {
    const state = teamState(team);
    return { key: `team:${team.id}`, head: true, team, rows: membersOf(team).filter(drawn), total: state.total, running: state.running, busy: state.busy };
  };
  if (shownTeam.value) return [ofTeam(shownTeam.value)];

  // A team none of whose rows the switch leaves is left out whole; one with no project yet stays.
  const list = teams.value.map(ofTeam).filter((section) => section.rows.length || !section.total);
  const loose = looseProjects.value.filter(drawn);
  if (loose.length) list.push({ key: "loose", head: true, team: null, rows: loose, total: loose.length, running: 0, busy: false });
  return list;
});

/** The rows drawn, top to bottom. */
const rows = computed(() => sections.value.flatMap((section) => section.rows));
/** Something to draw: a flat list with a row, the groups while a project is listed and one of them
 * is drawn, a team's own view always. */
const listShown = computed(() => (grouped.value ? shownTeam.value !== null || (projects.value.length > 0 && sections.value.length > 0) : rows.value.length > 0));
/** The projects "Only favourites and open" leaves out of what the filter or the team shows. */
const hiddenCount = computed(() => {
  if (!favouritesOnly.value) return 0;
  return pool.value.filter((p) => !drawn(p)).length;
});
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

/**
 * The terminal panel's project (TerminalPanel, under every page): the one the pages followed last
 * or the panel picked (lib/panel: `panelPath`), while it is listed; else the list's selection, else
 * the first project. None before there is a project, and none for the first run's steps.
 */
const panelProject = computed(() => projects.value.find((p) => p.path === panelPath.value) ?? selected.value ?? projects.value[0] ?? null);
/** The panel is drawn: the page keeps its tab row's height free for it, and no more. */
const panelShown = computed(() => panelProject.value !== null && !(setupOpen.value && view.value === "projects"));

// The list's selection is the panel's project, on the Projects page: picked there, its terminal
// takes the keyboard (the panel open), as it always did; coming back to the page, the panel shows
// it again without.
watch(
  () => shown.value?.path,
  (path) => {
    if (path && view.value === "projects") followProject(path, true);
  },
  { immediate: true },
);
watch(view, (page) => {
  if (page === "projects" && shown.value) followProject(shown.value.path);
});

/** The panel shows a project with a terminal open: its tabs tell Claude's state in them
 * (TerminalPanel), which the sessions hold. */
const panelTabs = computed(() => (panelProject.value?.terminals.length ?? 0) > 0);
watch(
  panelTabs,
  (tabs) => {
    if (tabs && !sessionsLink) sessionsLink = connectSessions();
  },
  { immediate: true },
);

// Keep a selection while there is something to show: only a project the list holds no more gives way.
watch(
  rows,
  (list) => {
    if (!list.some((p) => p.path === selectedPath.value)) selectedPath.value = list[0]?.path ?? null;
  },
  { immediate: true },
);

function count(id: Filter): number {
  const f = filters.find((x) => x.id === id);
  return f ? projects.value.filter(f.test).length : 0;
}

// A team that is gone takes its own view with it, and the last team the projects in no team's
// view: the list is back on its filter.
watch(teams, (list) => {
  if (teamFilter.value && !list.some((team) => team.id === teamFilter.value)) teamFilter.value = null;
  if (!list.length) looseShown.value = false;
});

function pick(id: Filter): void {
  filter.value = id;
  teamFilter.value = null;
  looseShown.value = false;
  view.value = "projects";
  // The list itself, not the setup steps in its place.
  setupOpen.value = false;
}

/** A team's sidebar entry: the list shows that team alone, selected until a row is clicked. */
function pickTeam(id: string): void {
  teamFilter.value = id;
  looseShown.value = false;
  query.value = "";
  view.value = "projects";
  setupOpen.value = false;
  selectedTeamId.value = id;
}

/** The sidebar's entry for the projects in no team: the list shows them alone, flat. */
function pickLoose(): void {
  teamFilter.value = null;
  looseShown.value = true;
  query.value = "";
  view.value = "projects";
  setupOpen.value = false;
  selectedTeamId.value = null;
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
  const drawn = sections.value.flatMap((section) => [...(section.team ? [headKey(section.team.id)] : []), ...section.rows.map((p) => rowKey(p.path))]);
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

/** The list's rows and teams' heads, top to bottom (where the projects in no team begin, nothing
 * takes the keyboard). */
function lines(): HTMLElement[] {
  return Array.from(tbody.value?.querySelectorAll<HTMLElement>(':scope > [data-path], :scope > [data-group]:not([data-group=""])') ?? []);
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

/** A double click or ↵ on a row: the project's Files window or its editor, as its settings or
 * Pitwall's say (lib/open). */
function openRow(path: string): void {
  openInWindow(path, "list");
}

/** The list is one Tab stop: arrows go along its rows and heads (a row is selected), ↵ opens the
 * project as a double click does, Space starts or stops. */
function onTableKey(event: KeyboardEvent): void {
  if (onHeadKey(event)) return;
  rowKeys(event, {
    move: selectProject,
    enter: openRow,
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

/** Right-click on a row: the project's files or its editor; the project goes to another team, to a new one, or out of its own. */
function rowMenu(event: MouseEvent, project: Project): void {
  selectProject(project.path);
  const own = teamOf(project.path);
  const entries: MenuEntry[] = [
    { text: t("files.show"), accelerator: "CmdOrCtrl+Shift+E", action: () => toggleFiles(project.path) },
    { text: t("common.openInEditor"), action: () => void api.openEditor(project.path) },
    "separator",
  ];
  entries.push(...teams.value.filter((team) => team.id !== own?.id).map((team) => ({ text: t("team.moveTo", { team: team.name }), action: () => moveToTeam(project.path, team.id) })));
  entries.push({ text: t("team.newMenu"), action: () => (teamAsk.value = { kind: "create", projects: [project.path], show: false }) });
  if (own) entries.push("separator", { text: t("team.leave"), action: () => void leaveTeam(project.path) });
  entries.push("separator", { text: t(project.muted ? "notifications.unmute" : "notifications.mute"), action: () => void api.muteProject(project.path, !project.muted) });
  // Out of the list, star and all; a VS Code window that has it open, or a server of it that runs, still shows it.
  if (project.added || project.favourite) entries.push("separator", { text: t("window.removeProject"), action: () => void api.removeProject(project.path) });
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

/** The Files windows open, as the sidebar lists them under Garage: each by its project's name. */
const openFiles = computed(() =>
  filesWindows
    .filter((win) => win.open)
    .map((win) => ({ win, name: snapshot.value?.projects.find((p) => p.path === win.path)?.name ?? win.path.slice(win.path.lastIndexOf("/") + 1) })),
);

/** A Files window's sidebar entry pressed: a folded window comes up, the one in front folds back,
 * one that is up behind another comes in front. */
function pressEntry(win: FilesWin): void {
  if (win.minimized) showFiles(win.path);
  else if (frontFiles() === win) minimizeFiles(win);
  else raiseFiles(win);
}

/** Right-click on a Files window's sidebar entry: fold it there, or close it. */
function filesMenu(at: MenuPoint, win: FilesWin): void {
  void menu.popup(at, [
    { text: t("files.minimize"), enabled: !win.minimized, action: () => minimizeFiles(win) },
    { text: t("common.close"), action: () => closeFiles(win.path) },
  ]);
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

/** A row's title: what a double click on it opens (lib/open; the list never opens a project in itself). */
const ROW_TITLES = { terminal: "window.rowTitleTerminal", files: "window.rowTitleFiles", editor: "window.rowTitle", list: "window.rowTitle" } as const;

/** ⌘T (Ctrl+Shift+T on Windows and Linux, lib/platform: terminalChord) opens a new terminal in the
 * terminal panel's project, on whatever page; ⌘W closes what is in front (`closeFront`). */
function onKey(event: KeyboardEvent): void {
  const terminals = terminalPanel.value;
  if (terminalChord(event) && event.code === "KeyT" && terminals) {
    event.preventDefault();
    void terminals.openTerminal();
  } else if (closeChord(event)) {
    closeFront(event);
  } else if ((isMac ? event.metaKey : event.ctrlKey) && event.shiftKey && !event.altKey && event.code === "KeyE" && shown.value) {
    // ⇧⌘E: the selected project's files, in their floating window (files/FilesWindow).
    event.preventDefault();
    toggleFiles(shown.value.path);
  }
}

/**
 * ⌘W (lib/platform: closeChord) closes one thing at a time, what is in front: in a Files window the
 * tab shown, with none left the window (one with the keyboard in it takes the key itself,
 * files/FilesWindow); in the terminal panel, open, the tab shown, with none left the panel, folded.
 * The panel goes first when the keyboard is in it, else the Files window in front. With neither up
 * the key is left to the Window menu's Close Window (macOS): the window closes, Pitwall stays in the
 * menu bar. So it is while a dialog is up, as before. A held key closes nothing more.
 */
function closeFront(event: KeyboardEvent): void {
  if (event.defaultPrevented || document.querySelector("dialog[open]")) return;
  const terminals = terminalPanel.value;
  const panel = terminals && !terminalCollapsed.value ? terminals : null;
  const files = frontFiles();
  if (!event.repeat && !panel && !files) return;
  event.preventDefault();
  if (event.repeat) return;
  if (panel && (panelTyping.value || !files)) panel.closeShown();
  else if (files) closeShownTab(files);
}

let unlistenReveal: UnlistenFn | null = null;
let unlistenProject: UnlistenFn | null = null;
let unlistenFiles: UnlistenFn | null = null;
let unlistenFuelReveal: UnlistenFn | null = null;
let unlistenTerminal: UnlistenFn | null = null;
let unlistenFocus: UnlistenFn | null = null;
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

/** From the switcher (⌘↵ on a link) or a notification: select the project, even if a filter,
 * search or "Only favourites and open" hid it. Settings, which would cover it, closes. */
function revealProject(path: string): void {
  settingsOpen.value = false;
  view.value = "projects";
  if (!rows.value.some((p) => p.path === path)) {
    filter.value = "all";
    teamFilter.value = null;
    looseShown.value = false;
    query.value = "";
    const project = projects.value.find((p) => p.path === path);
    if (project && !drawn(project)) setFavouritesOnly(false);
  }
  selectProject(path);
}

/** From outside the window: the project's Files window, over the page shown; Settings, which
 * would cover it, closes. */
function revealFiles(path: string): void {
  settingsOpen.value = false;
  showFiles(path);
}

// A page's double click where its project opens in the list (lib/open).
watch(listRequest, (asked) => {
  if (asked) revealProject(asked.path);
});

/** From the switcher (⌘↵ on a command) or the Garage page: that command's output in the terminal
 * panel, open, on the page shown. */
function revealOutput(path: string, job: string): void {
  settingsOpen.value = false;
  showOutput(path, job);
}

/**
 * A Claude session in one of Pitwall's terminals brought up, from whatever page (or the popover,
 * the switcher, a notification): the terminal panel shows its project and tab, open, the keyboard
 * in it (TerminalPanel takes the request), on the page shown. On the Board page, given to Claude
 * from a card, that card is selected too (BoardView takes the request).
 */
async function revealTerminal(path: string, id: number): Promise<void> {
  settingsOpen.value = false;
  revealTab(path, id);
  if (view.value !== "board") return;
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
  if (card) cardRequest.value = { id: card.id, path: card.path };
}

/** The Sessions page's "Open a Claude terminal" with no project of its own: one, in the terminal
 * panel, for the project selected in the list. */
function openClaudeTerminal(): void {
  const project = selected.value ?? panelProject.value;
  if (project) openInPanel(project.path, true);
}

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  unlistenReveal = await listen<{ path: string; job: string }>("reveal-output", (event) => revealOutput(event.payload.path, event.payload.job));
  unlistenProject = await listen<{ path: string }>("reveal-project", (event) => revealProject(event.payload.path));
  // Opened from the popover, the switcher or the mini tower where Settings › Projects says Files.
  unlistenFiles = await listen<{ path: string }>("reveal-files", (event) => revealFiles(event.payload.path));
  // A click on the 90 % notification: the Fuel page.
  unlistenFuelReveal = await listen("reveal-fuel", () => (view.value = "fuel"));
  // Bringing up a Claude session that runs in one of Pitwall's terminals: that terminal.
  unlistenTerminal = await listen<{ path: string; id: number }>("reveal-terminal", (event) => void revealTerminal(event.payload.path, event.payload.id));
  // Opened from outside the window where Settings › Projects says its terminal (lib/open).
  unlistenFocus = await listen<{ path: string }>("focus-terminal", (event) => {
    settingsOpen.value = false;
    focusTerminal(event.payload.path);
  });
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
  unlistenFiles?.();
  unlistenFuelReveal?.();
  unlistenTerminal?.();
  unlistenFocus?.();
  unlistenPage?.();
  unlistenResize?.();
});

/** How many of the project's commands started with its server and wait for the user. */
function heldCount(project: Project): number {
  return project.commands.filter((command) => command.held).length;
}

function server(project: Project): string {
  return meta(project, now.value);
}
</script>

<template>
  <div :class="['shell', { fullscreen }]">
    <!-- Windows: Pitwall's own title bar across the top, the brand in it (the system's is off). -->
    <TitleBar
      v-if="isWindows"
      class="title-row"
      :version="update?.current ?? null"
      :next="update?.version ?? null"
      :update-button="updateButton"
      :installing="installing"
      @update="updateOpen = true"
    />
    <aside class="sidebar">
      <!-- "deep": the logo and name drag the window too; so does the strip under the traffic lights.
           macOS only (lib/platform: dragRegion): Linux has the system's title bar, Windows its own. -->
      <div v-if="!isWindows" class="brand" :data-tauri-drag-region="dragRegion">
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
            :class="{ on: view === 'projects' && !shownTeam && !looseShown && filter === f.id }"
            :aria-current="view === 'projects' && !shownTeam && !looseShown && filter === f.id ? 'page' : undefined"
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
          <!-- The projects in no team, last and never dragged: there while there is a team. -->
          <button
            v-if="teams.length"
            type="button"
            :class="{ on: view === 'projects' && looseShown }"
            :aria-current="view === 'projects' && looseShown ? 'page' : undefined"
            @click="pickLoose"
          >
            <Icon name="user" /> <span class="nav-label">{{ t("team.loose") }}</span>
            <ClaudeDot v-if="looseNav.claude.length" :live="{ phase: looseNav.claude[0].phase, text: looseNav.claude.map((note) => note.text).join(' · ') }" />
            <span :class="['team-count', { live: looseNav.running > 0 }]" :aria-label="t('team.runningOf', { running: looseNav.running, total: looseNav.total })">{{ looseNav.running }}/{{ looseNav.total }}</span>
          </button>
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
        <!-- The Files windows open, under Garage: a click brings a folded one up and in front, and
             folds one that is up back into its entry (files/FilesWindow); closed, it leaves the list. -->
        <nav v-if="openFiles.length" :aria-label="t('files.openWindows')" class="nav files-nav">
          <button
            v-for="entry in openFiles"
            :key="entry.win.path"
            type="button"
            :data-files="entry.win.path"
            :aria-label="t('files.window', { name: entry.name })"
            :title="t('files.window', { name: entry.name })"
            @click="pressEntry(entry.win)"
            @contextmenu.prevent="filesMenu($event, entry.win)"
          >
            <Icon name="folder" /> <span class="nav-label">{{ entry.name }}</span>
            <!-- → folded away (a click brings it up), ← up (a click folds it here). -->
            <Icon name="arrow-right" :size="13" :class="['files-go', { up: !entry.win.minimized }]" />
          </button>
        </nav>
      </div>
      <div class="nav foot">
        <button type="button" @click="api.pickFolder()"><Icon name="plus" /> {{ t("window.addProject") }}</button>
        <button type="button" :class="{ on: settingsOpen }" @click="openSettings()"><Icon name="settings" /> {{ t("common.settings") }}</button>
      </div>
    </aside>

    <!-- The page shown, and along its bottom the terminal panel, the same on every page: the page
         keeps the panel's tab row free (`COLLAPSED_HEIGHT`); open, the panel lies over it. -->
    <div class="pages">
    <div class="page" :style="{ marginBottom: panelShown ? `${COLLAPSED_HEIGHT}px` : undefined, '--panel-over': `${panelOver}px` }">
    <DayView v-if="view === 'day'" />
    <TrackView v-else-if="view === 'track'" @open-project="revealProject" @open-settings="openSettings('track')" />
    <FuelView v-else-if="view === 'fuel'" :terminal-project="panelShown ? (panelProject?.path ?? null) : null" @open-settings="(part) => openSettings('claude', part)" />
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
    <SetupView v-else-if="setupOpen" :covered="settingsOpen" @done="setupOpen = false" @open-phone="openSettings('notifications', 'phone')" />

    <template v-else>
      <main class="main">
        <!-- Its heading and free space drag the window; the search box and buttons stay clickable. -->
        <div class="toolbar" :data-tauri-drag-region="dragRegion">
          <div class="heading">{{ heading }}</div>
          <!-- Every project, or the favourites and those open or at work: the one shown is lit. -->
          <div class="list-scope" role="group" :aria-label="t('window.favouritesOnly')">
            <button type="button" :class="{ on: !favouritesOnly }" :aria-pressed="!favouritesOnly" @click="setFavouritesOnly(false)">{{ t("window.scopeAll") }}</button>
            <button type="button" :class="{ on: favouritesOnly }" :aria-pressed="favouritesOnly" :title="t('window.favouritesOnly')" @click="setFavouritesOnly(true)">
              <Icon :name="favouritesOnly ? 'star-filled' : 'star'" :size="13" class="scope-icon" /><span class="scope-text">{{ t("window.scopeOpen") }}</span>
            </button>
          </div>
          <label class="list-search">
            <Icon name="search" :size="13" />
            <input id="search" v-model="query" type="search" :placeholder="t('common.searchProjects')" :aria-label="t('common.searchProjects')" @keydown.down.prevent="focusSelected" />
          </label>
        </div>

        <span id="rows-keys" class="sr-only">
          {{ t("common.rowKeys") }}
          <template v-if="grouped">{{ t("team.headKeys", { up: keys("Alt+ArrowUp"), down: keys("Alt+ArrowDown") }) }}</template>
        </span>
        <!-- The list is one Tab stop: the selected row (or team's head) takes the focus; ↑/↓ move
             it along rows and heads, ←/→ reach a line's buttons. -->
        <div v-if="listShown" class="table" role="grid" :aria-label="heading" aria-describedby="rows-keys">
          <div :class="['thead', { grouped }]" role="row">
            <span role="columnheader">{{ t("window.col.project") }}</span>
            <span role="columnheader">{{ t("window.col.branch") }}</span>
            <span role="columnheader">{{ t("window.col.server") }}</span>
            <span role="columnheader">{{ t("window.col.claude") }}</span>
            <span role="columnheader" :aria-label="t('window.col.actions')"></span>
          </div>
          <!-- Heads, rows and the line of an empty team are all the list's own children, one under
               the other: the keyboard (lib/rows) and the drag (lib/groupReorder) count on it. -->
          <div
            ref="tbody"
            :class="['tbody', { grouped }, drag.listClass()]"
            role="rowgroup"
            @keydown="onTableKey"
            @focusin="onListFocus"
            @focusout="onListBlur"
            @pointerover="onListPointer"
            @pointerleave="litTeam = null"
          >
            <template v-for="section in sections" :key="section.key">
              <TeamHead
                v-if="section.head"
                :team="section.team"
                :total="section.total"
                :running="section.running"
                :busy="section.busy"
                :selected="section.team !== null && section.team.id === selectedTeam?.id"
                :lit="section.team !== null && section.team.id === litTeam"
                :tab-stop="tabStop === headKey(section.team?.id ?? '')"
                :class="drag.lineClass(headKey(section.team?.id ?? ''))"
                :style="drag.lineStyle(headKey(section.team?.id ?? ''))"
                @pointerdown="section.team && drag.down($event, headKey(section.team.id))"
                @contextmenu="section.team && teamMenu($event, section.team)"
                @select="pressHead(section)"
              />
              <!-- A team's rows carry its line: the axis down their left and a branch to each dot,
                   the last one turning in round (`branch`, `last`); the projects in no team, none. -->
              <div
                v-for="(project, at) in section.rows"
                :key="project.path"
                role="row"
                :aria-selected="project.path === shown?.path"
                :tabindex="tabStop === rowKey(project.path) ? 0 : -1"
                :data-path="project.path"
                data-picks-project
                :data-zone="zone(project)"
                :data-team="section.team?.id"
                :class="[
                  'trow',
                  {
                    selected: project.path === shown?.path,
                    branch: section.team !== null,
                    last: at === section.rows.length - 1,
                    lit: section.team !== null && section.team.id === litTeam,
                    chosen: section.team !== null && section.team.id === selectedTeam?.id,
                    'off-line': section.team !== null && offLine(rowKey(project.path), section.team),
                  },
                  drag.lineClass(rowKey(project.path)),
                ]"
                :style="section.team ? teamLineStyle(rowKey(project.path), section.team) : drag.lineStyle(rowKey(project.path))"
                :title="t(ROW_TITLES[openTarget(project.path, 'list')])"
                @pointerdown="drag.down($event, rowKey(project.path))"
                @click="select(project.path)"
                @dblclick="openRow(project.path)"
                @contextmenu="rowMenu($event, project)"
              >
                <span class="cell-project" role="gridcell">
                  <StatusIcon :project="project" ring="var(--bg-app)" />
                  <span class="names">
                    <span class="name">
                      <span class="name-text">{{ project.name }}</span>
                      <MutedMark v-if="project.muted" />
                      <span v-if="heldCount(project)" class="held-badge" role="img" :aria-label="t('detail.held.count', { count: heldCount(project) })" :title="t('detail.held.count', { count: heldCount(project) })">
                        <Icon name="clock" :size="10" /><span class="badge-count">{{ heldCount(project) }}</span>
                      </span>
                      <span v-if="project.terminals.length" class="term-badge" role="img" :aria-label="t('window.openTerminals', { count: project.terminals.length })" :title="t('window.openTerminals', { count: project.terminals.length })">>_ <span class="badge-count">{{ project.terminals.length }}</span></span>
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
                :data-team="section.team.id"
                :class="['ghint', { lit: section.team.id === litTeam, chosen: section.team.id === selectedTeam?.id }, drag.lineClass(hintKey(section.team.id))]"
                :style="teamLineStyle(hintKey(section.team.id), section.team)"
              >
                <span role="gridcell" aria-colspan="5">{{ t(section.total ? "window.favouritesHidden" : shownTeam ? "team.noMembers" : "team.empty") }}</span>
              </div>
            </template>
          </div>
        </div>

        <section v-else class="empty">
          <PitwallGlyph :size="44" lamps="var(--idle-ring)" />
          <template v-if="hiddenCount && !query.trim()">
            <p>{{ t("window.favouritesHidden") }}</p>
          </template>
          <template v-else-if="looseShown && !pool.length">
            <p>{{ t("team.pickNone") }}</p>
          </template>
          <template v-else-if="projects.length">
            <p>{{ t("window.noMatch") }}</p>
          </template>
          <template v-else>
            <p>{{ t("window.emptyLine1") }}<br />{{ t("window.emptyLine2") }}</p>
            <button type="button" class="control" @click="api.pickFolder()">{{ t("common.addProject") }}</button>
          </template>
        </section>

        <!-- Projects that appeared in the projects folder, offered under the list. -->
        <ProjectSuggestions />
      </main>

      <!-- A selected team's details take the pane; else the selected project's. -->
      <TeamDetail v-if="selectedTeam" :team="selectedTeam" />
      <ProjectDetail v-else-if="selected" :project="selected" />
      <!-- Nothing to show: the panel stays blank but keeps its place, so the layout never jumps.
           The middle panel already shows the glyph and what to do. -->
      <section v-else class="detail-empty" :aria-label="t('window.projectDetails')"></section>
    </template>
    </div>

    <!-- The panel's project's outputs and terminals; not before there is a project, nor over the
         first run's steps. -->
    <TerminalPanel v-if="panelShown && panelProject" ref="terminalPanel" :project="panelProject" />
    </div>

    <!-- The selected project's files, floating over whatever page is up (⇧⌘E, the details' button). -->
    <FilesWindows />

    <SettingsView v-if="settingsOpen" :start-tab="settingsTab" :start-part="settingsPart" @close="settingsOpen = false" @open-update="updateOpen = true" @open-setup="openSetup" @open-whats-new="whatsNewOpen = true" />
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

/* Right of the sidebar: the page, and along its bottom the terminal panel (TerminalPanel). */
.pages {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

/* The page, less the panel's tab row; its own parts lie side by side, as they did in the
   window. */
.page {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  overflow: clip;
}

/* Windows: the title bar over the sidebar and the pages, one ground with the sidebar; the pages
   lie in it like a sheet, their edge along the top and the left, rounded where the two meet. The
   sidebar's first row is level with a page's heading. */
:root[data-platform="windows"] .shell {
  display: grid;
  grid-template: auto minmax(0, 1fr) / auto minmax(0, 1fr);
  background: var(--bg-sidebar);
}

.title-row {
  grid-column: 1 / -1;
}

:root[data-platform="windows"] .sidebar {
  padding-top: 11px;
  border-right: 0;
}

:root[data-platform="windows"] .pages {
  border-top: 1px solid var(--line);
  border-left: 1px solid var(--line);
  border-top-left-radius: 10px;
  background: var(--bg-app);
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

/* The Files windows open, under the pages. */
.files-nav {
  margin-top: 12px;
}

.files-go {
  flex-shrink: 0;
  margin-left: auto;
  color: var(--text-subtle);
  transition: transform 200ms ease;
}

.files-go.up {
  transform: rotate(180deg);
}

@media (prefers-reduced-motion: reduce) {
  .files-go {
    transition: none;
  }
}

.files-nav button:hover .files-go {
  color: var(--text-strong);
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
  .today,
  .files-nav {
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
  container-type: inline-size;
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

/* A narrow list gives the heading's room and then the search box's to the list's two choices,
   whose names are never cut. */
.heading {
  font-size: 15px;
  font-weight: 600;
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The search box, as the Board page's: its glass, then the field. */
.list-search {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 220px;
  min-width: 64px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text-subtle);
  cursor: text;
}

.list-search svg {
  flex-shrink: 0;
}

.list-search input {
  -webkit-appearance: none;
  appearance: none;
  flex-grow: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.list-search:focus-within {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}

.list-search input::placeholder {
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

/* All, or the favourites and those open: the Board page's columns-or-list trough, with words. */
.list-scope {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  height: 30px;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
}

.list-scope button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 24px;
  padding: 0 10px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font-size: 12.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.list-scope button:hover {
  color: var(--text);
}

.list-scope button.on {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

/* In the light theme, the segmented control the settings have: a grey trough, the one chosen white. */
:root[data-theme="light"] .list-scope {
  background: var(--bg-seg);
}

:root[data-theme="light"] .list-scope button.on {
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
}

.scope-icon {
  display: none;
}

/* Without room for its words once the heading and the search box have given theirs, the second
   choice is its star alone (still its tooltip, and read out). */
@container (max-width: 409px) {
  .list-scope button:last-child {
    width: 30px;
    padding: 0;
  }

  .scope-icon {
    display: block;
  }

  .scope-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
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

/* A head as narrow as its column cuts its name, as the rows do. */
.thead > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

/* In groups the rows stand 20px further in, for their team's line (`.trow.branch`). */
.thead.grouped {
  padding-left: 40px;
}

/* Fills the panel down to the terminals, so a row can be dragged anywhere in it (dropped in the
   empty space it slides up to the last place) and its shadow is never cut. The same room on
   every side; a dragged row keeps it from the edges too (lib/groupReorder). */
.tbody {
  container: project-rows / inline-size;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 10px;
  /* Room under the end for the terminal panel lying over the page (WindowApp: `--panel-over`). */
  padding-bottom: calc(10px + var(--panel-over, 0px));
  scroll-padding-bottom: var(--panel-over, 0px);
}
/* Its scrollbar ends where the panel begins: the list keeps its size, only the bar's track is
   shorter (WindowApp: `--panel-over`). */
.tbody::-webkit-scrollbar-track {
  margin-bottom: var(--panel-over, 0px);
}


/* In groups, each head brings its own room above it (team/TeamHead). */
.tbody.grouped {
  padding-top: 0;
}

.trow {
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
}

/* In groups every row stands 20px in: room for its team's line, at x 8 of the list (the head
   draws its top), or for nothing (the projects in no team). */
.tbody.grouped > :is(.trow, .ghint) {
  margin-left: 20px;
}

/* A team's line through its rows: the axis down the row's left (-13..-11, the head's x 8) and a
   square branch to the dot, stopping 4px short of it (the dot's middle is at 18). The last row's
   axis turns into its branch round. Faint at rest, a step up while the pointer is on the team,
   bright while it is selected (the head's colours: team/TeamHead). */
.trow.branch,
.ghint {
  --tie: var(--team-line);
  position: relative;
}

:is(.trow.branch, .ghint).lit {
  --tie: var(--team-line-hover);
}

:is(.trow.branch, .ghint).chosen {
  --tie: var(--team-line-selected);
}

.trow.branch::before,
.trow.branch::after {
  content: "";
  position: absolute;
  pointer-events: none;
  transform: translateY(var(--tie-shift, 0px));
}

.trow.branch::before {
  left: -13px;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--tie);
}

.trow.branch::after {
  left: -11px;
  top: calc(50% - 1px);
  width: 20px;
  height: 2px;
  background: var(--tie);
}

.trow.branch.last::before {
  bottom: auto;
  width: 22px;
  height: calc(50% + 1px);
  box-sizing: border-box;
  border-left: 2px solid var(--tie);
  border-bottom: 2px solid var(--tie);
  border-bottom-left-radius: 10px;
  background: none;
}

.trow.branch.last::after {
  content: none;
}

/* A row on its own ground (selected, or under the pointer while nothing is dragged), and a row
   away from its piece of the line while a project is dragged (`off-line`: the one in hand, one
   slid aside): its branch stops 4px short of the ground's edge (x -4) instead of running in to
   the dot, drawing back and out again as that comes and goes. */
.trow.branch::after,
.trow.branch.last::before {
  transition:
    width 140ms ease-out,
    border-bottom-left-radius 140ms ease-out;
}

@media (prefers-reduced-motion: reduce) {
  .trow.branch::after,
  .trow.branch.last::before {
    transition: none;
  }
}

.trow.branch.selected::after,
.trow.branch.off-line::after,
.tbody:not(.reordering) > .trow.branch:hover::after {
  width: 7px;
}

.trow.branch.last.selected::before,
.trow.branch.last.off-line::before,
.tbody:not(.reordering) > .trow.branch.last:hover::before {
  width: 9px;
  border-bottom-left-radius: 9px;
}

/* While a drag is on, a team's line stands still where its head is (`--tie-shift`:
   teamLineStyle): each piece takes back its row's move at the row's own pace, so the two cancel
   out, and the row in hand leaves its piece in its place. A carried row follows the pointer at
   once and settles at the pace it settles at. Only the branches' length moves. */
.tbody.reordering > .trow.branch::before,
.tbody.reordering > .trow.branch::after,
.tbody.reordering > .ghint::before {
  transition:
    transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1),
    width 140ms ease-out,
    border-bottom-left-radius 140ms ease-out;
}

.tbody.reordering > .trow.branch.carried::before,
.tbody.reordering > .trow.branch.carried::after,
.tbody.reordering > .ghint.carried::before {
  transition:
    width 140ms ease-out,
    border-bottom-left-radius 140ms ease-out;
}

.tbody.reordering > .trow.branch.carried.settling::before,
.tbody.reordering > .trow.branch.carried.settling::after,
.tbody.reordering > .ghint.carried.settling::before {
  transition:
    transform 160ms ease-out,
    width 140ms ease-out,
    border-bottom-left-radius 140ms ease-out;
}

/* The new order takes over: nothing moves, the branches still reach out again. */
.tbody.frozen > .trow.branch::before,
.tbody.frozen > .trow.branch::after,
.tbody.frozen > .ghint::before {
  transition:
    width 140ms ease-out,
    border-bottom-left-radius 140ms ease-out !important;
}

@media (prefers-reduced-motion: reduce) {
  .tbody.reordering > .trow.branch::before,
  .tbody.reordering > .trow.branch::after,
  .tbody.reordering > .ghint::before,
  .tbody.frozen > .trow.branch::before,
  .tbody.frozen > .trow.branch::after,
  .tbody.frozen > .ghint::before {
    transition: none !important;
  }
}

/* The line of a team with no project yet: its head's line comes down dashed and turns toward
   the words, where a row's dot would be. The words, not the line, are cut short. */
.ghint {
  display: flex;
  align-items: center;
  height: 32px;
  padding: 0 10px 0 14px;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.ghint > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ghint::before {
  content: "";
  position: absolute;
  pointer-events: none;
  transform: translateY(var(--tie-shift, 0px));
  left: -13px;
  top: 0;
  width: 22px;
  height: calc(50% + 1px);
  box-sizing: border-box;
  border-left: 2px dashed var(--tie);
  border-bottom: 2px dashed var(--tie);
  border-bottom-left-radius: 10px;
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

/* Commands that started with the server and wait for the user: a clock and how many, in the
   colours of an "asks first" tag. */
.held-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  padding: 1px 6px;
  border-radius: 6px;
  background: var(--btn-claude-bg);
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 400;
  white-space: nowrap;
  color: var(--claude-soft);
}

/* A narrow list: the badges are their icons alone (their counts stay in their tooltips, and read
   out) before they leave a name beside both of them less than 48 px. */
@container project-rows (max-width: 651px) {
  .badge-count {
    display: none;
  }
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
