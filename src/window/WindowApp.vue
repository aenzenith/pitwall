<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Marquee from "../components/Marquee.vue";
import Spinner from "../components/Spinner.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeState, gitLine, meta } from "../lib/format";
import { isLow, isStale, left, percentText, sessionWindow } from "../lib/fuel";
import { searchProjects } from "../lib/fuzzy";
import { language, t } from "../lib/i18n";
import { filters, isFilter, type Filter, type Page, type View } from "../lib/pages";
import { onBoard } from "../lib/board";
import { cardRequest, outputRequest, terminalRequest } from "../lib/panel";
import { dragRegion, terminalChord } from "../lib/platform";
import { useReorder } from "../lib/reorder";
import { rowKeys } from "../lib/rows";
import { needsAttention } from "../lib/deps";
import { api, board, connectBoard, connectDeps, connectFuel, connectSessions, deps, fuel, loadBoard, loadSessions, now, sessions, snapshot, visible } from "../lib/store";
import type { Card, Project } from "../lib/types";
import BoardView from "./BoardView.vue";
import DayView from "./DayView.vue";
import DepsView from "./DepsView.vue";
import FuelView from "./FuelView.vue";
import ProjectDetail from "./ProjectDetail.vue";
import SessionsView from "./SessionsView.vue";
import TerminalPanel from "./TerminalPanel.vue";
import SettingsView from "./SettingsView.vue";
import TrackView from "./TrackView.vue";

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
 * page's settings button on Track's. */
const settingsTab = ref<"claude" | "track" | undefined>(undefined);

function openSettings(tab?: "claude" | "track"): void {
  settingsTab.value = tab;
  settingsOpen.value = true;
}
const query = ref("");
const selectedPath = ref<string | null>(null);
const tbody = ref<HTMLElement | null>(null);
const reorder = useReorder(tbody);

function select(path: string): void {
  if (reorder.isClick()) selectedPath.value = path;
}

const projects = computed(() => snapshot.value?.projects ?? []);
const anyBusy = computed(() => projects.value.some((p) => p.status === "busy"));
const current = computed(() => filters.find((f) => f.id === filter.value) ?? filters[0]);
const rows = computed(() => {
  const q = query.value.trim().toLowerCase();
  const shown = projects.value.filter((p) => current.value.test(p));
  // Searching: the quick switcher's fuzzy match and order (lib/fuzzy); else the list's own order.
  return q ? searchProjects(shown, q).map((match) => match.project) : shown;
});
const selected = computed(() => projects.value.find((p) => p.path === selectedPath.value) ?? null);

/** The list shows a project with a terminal open: its tabs tell Claude's state in them
 * (TerminalPanel), which the sessions hold. */
const listTabs = computed(() => view.value === "projects" && (selected.value?.terminals.length ?? 0) > 0);
watch(
  listTabs,
  (tabs) => {
    if (tabs && !sessionsLink) sessionsLink = connectSessions();
  },
  { immediate: true },
);

// Keep a selection while there is something to show.
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

function pick(id: Filter): void {
  filter.value = id;
  view.value = "projects";
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

/** Pressed from the keyboard, the button turns busy (disabled) and would drop the focus: the row keeps it. */
function toggleFrom(event: MouseEvent, project: Project): void {
  toggle(project);
  if (event.detail === 0) ((event.currentTarget as HTMLElement).closest("[data-path]") as HTMLElement | null)?.focus();
}

/** The table is one Tab stop: arrows move the selection, ↵ opens the editor, Space starts or stops. */
function onTableKey(event: KeyboardEvent): void {
  rowKeys(event, {
    move: (path) => (selectedPath.value = path),
    enter: (path) => void api.openEditor(path),
    space: (path) => {
      const project = rows.value.find((p) => p.path === path);
      if (project && project.status !== "busy") toggle(project);
    },
  });
}

/** ↓ in the search box goes on to the selected row. */
function focusSelected(): void {
  tbody.value?.querySelector<HTMLElement>('[data-path][tabindex="0"]')?.focus();
}

/** In full screen the traffic lights are gone, and so is the room kept for them. */
const fullscreen = ref(false);
let unlistenResize: UnlistenFn | null = null;

const terminalPanel = ref<InstanceType<typeof TerminalPanel> | null>(null);

/** ⌘T (Ctrl+Shift+T on Windows and Linux, lib/platform: terminalChord) opens a new terminal in the selected project. */
function onKey(event: KeyboardEvent): void {
  if (terminalChord(event) && event.code === "KeyT" && terminalPanel.value) {
    event.preventDefault();
    void terminalPanel.value.openTerminal();
  }
}

let unlistenReveal: UnlistenFn | null = null;
let unlistenProject: UnlistenFn | null = null;
let unlistenFuelReveal: UnlistenFn | null = null;
let unlistenTerminal: UnlistenFn | null = null;
let unlistenPage: UnlistenFn | null = null;

/** From the switcher (`/`): that page of the window. Settings opens over the page shown and "Add
 * project" asks for the folder; any other page closes Settings, which would cover it. */
function revealPage(page: Page): void {
  if (page === "settings") openSettings();
  else if (page === "add") void api.pickFolder();
  else {
    settingsOpen.value = false;
    if (isFilter(page)) pick(page);
    else view.value = page;
  }
}

/** From the switcher (⌘↵ on a link): select the project, even if a filter or search hid it. */
function revealProject(path: string): void {
  view.value = "projects";
  if (!rows.value.some((p) => p.path === path)) {
    filter.value = "all";
    query.value = "";
  }
  selectedPath.value = path;
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
 * tab (TerminalPanel takes the request). Either way the keyboard is in it.
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
    revealProject(path);
    terminalRequest.value = { path, id, at: Date.now() };
  }
}

/** The Sessions page's "Open a Claude terminal": one, in the project selected in the list. */
async function openClaudeTerminal(): Promise<void> {
  view.value = "projects";
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
  unlistenPage = await listen<{ page: Page }>("reveal-page", (event) => revealPage(event.payload.page));
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
      </div>
      <nav :aria-label="t('window.filters')" class="nav">
        <button
          v-for="f in filters"
          :key="f.id"
          type="button"
          :class="{ on: view === 'projects' && filter === f.id }"
          :aria-current="view === 'projects' && filter === f.id ? 'page' : undefined"
          @click="pick(f.id)"
        >
          <Icon :name="f.icon" /> {{ t(f.label) }}
          <span :class="['count', { hot: f.id === 'waiting' && count(f.id) > 0 }]">{{ count(f.id) }}</span>
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
      <div class="grow"></div>
      <div class="nav">
        <button type="button" @click="api.pickFolder()"><Icon name="plus" /> {{ t("window.addProject") }}</button>
        <button type="button" :class="{ on: settingsOpen }" @click="openSettings()"><Icon name="settings" /> {{ t("common.settings") }}</button>
      </div>
    </aside>

    <DayView v-if="view === 'day'" />
    <TrackView v-else-if="view === 'track'" @open-project="revealProject" @open-settings="openSettings('track')" />
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

    <template v-else>
      <main class="main">
        <!-- Its heading and free space drag the window; the search box and buttons stay clickable. -->
        <div class="toolbar" :data-tauri-drag-region="dragRegion">
          <div class="heading">{{ t(current.label) }}</div>
          <label class="sr" for="search">{{ t("common.searchProjects") }}</label>
          <input id="search" v-model="query" type="search" :placeholder="t('common.searchProjects')" @keydown.down.prevent="focusSelected" />
          <Spinner v-if="anyBusy" class="busy-spin" />
          <button type="button" class="control" :disabled="!projects.length || anyBusy" @click="api.startAll()">{{ t("common.startAll") }}</button>
          <button type="button" class="control" :disabled="!snapshot?.running || anyBusy" @click="api.stopAll()">{{ t("common.stopAll") }}</button>
        </div>

        <span id="rows-keys" class="sr-only">{{ t("common.rowKeys") }}</span>
        <!-- One Tab stop: the selected row takes the focus; ↑/↓ move it, ←/→ reach its buttons. -->
        <div v-if="rows.length" class="table" role="grid" :aria-label="t(current.label)" aria-describedby="rows-keys">
          <div class="thead" role="row">
            <span role="columnheader">{{ t("window.col.project") }}</span>
            <span role="columnheader">{{ t("window.col.branch") }}</span>
            <span role="columnheader">{{ t("window.col.server") }}</span>
            <span role="columnheader">{{ t("window.col.claude") }}</span>
            <span role="columnheader" :aria-label="t('window.col.actions')"></span>
          </div>
          <div ref="tbody" :class="['tbody', reorder.listClass()]" role="rowgroup" @keydown="onTableKey">
            <div
              v-for="project in rows"
              :key="project.path"
              role="row"
              :aria-selected="project.path === selectedPath"
              :tabindex="project.path === selectedPath ? 0 : -1"
              :data-path="project.path"
              :class="['trow', { selected: project.path === selectedPath }, reorder.rowClass(project.path)]"
              :style="reorder.rowStyle(project.path)"
              :title="t('window.rowTitle')"
              @pointerdown="reorder.down($event, project.path)"
              @click="select(project.path)"
              @dblclick="api.openEditor(project.path)"
            >
              <span class="cell-project" role="gridcell">
                <StatusIcon :project="project" ring="var(--bg-app)" />
                <span class="names">
                  <span class="name">
                    <span class="name-text">{{ project.name }}</span>
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

        <!-- The selected project's terminals, level with the output panel beside it. -->
        <TerminalPanel v-if="selected" ref="terminalPanel" :project="selected" />
      </main>

      <ProjectDetail v-if="selected" :project="selected" />
      <!-- Nothing to show: the panel stays blank but keeps its place, so the layout never jumps.
           The middle panel already shows the glyph and what to do. -->
      <section v-else class="detail-empty" :aria-label="t('window.projectDetails')"></section>
    </template>

    <SettingsView v-if="settingsOpen" :start-tab="settingsTab" @close="settingsOpen = false" />
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
  color: #d7dae0;
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
  color: #b4bac3;
  text-align: left;
}

.nav button:hover {
  background: #1f2228;
}

.nav button.on {
  background: #23272e;
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

.grow {
  flex-grow: 1;
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
  background: #2c3039;
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
  border-bottom: 1px solid #1e2127;
}

:lang(zh) .thead,
:lang(ja) .thead {
  letter-spacing: 0;
}

/* Fills the panel down to the terminals, so a row can be dragged anywhere in it (dropped in the
   empty space it slides up to the last place) and its shadow is never cut. The same room on
   every side; a dragged row keeps it from the edges too (lib/reorder). */
.tbody {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 10px;
}

.trow {
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
}

/* The keyboard's row: the ring inside, so neighbours and the list's edge never cut it. */
.trow:focus-visible {
  outline-offset: -2px;
}

.trow:hover {
  background: #1b1e24;
}

.trow.selected {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px #2c313a;
}

/* Drag to reorder: the dragged row lifts and follows the pointer, the others slide aside to
   show where it lands, and on release it slides into place (lib/reorder). */
.tbody.reordering .trow {
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
    0 14px 22px -14px rgba(0, 0, 0, 0.8),
    inset 0 0 0 1px #2c313a;
}

.tbody.reordering .trow.dragging {
  transition: none;
}

.tbody.reordering .trow.dragging.settling {
  transition: transform 160ms ease-out;
}

.tbody.frozen .trow {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .tbody.reordering .trow,
  .tbody.reordering .trow.dragging.settling {
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
  background: #1c1f25;
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
  color: #c7ccd3;
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
  color: #c7ccd3;
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
  background: #262a33;
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
