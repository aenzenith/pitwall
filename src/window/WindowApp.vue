<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Marquee from "../components/Marquee.vue";
import Spinner from "../components/Spinner.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeState, gitLine, meta } from "../lib/format";
import { t, type Key } from "../lib/i18n";
import { outputRequest } from "../lib/panel";
import { useReorder } from "../lib/reorder";
import { api, now, snapshot } from "../lib/store";
import type { Project } from "../lib/types";
import ProjectDetail from "./ProjectDetail.vue";
import TerminalPanel from "./TerminalPanel.vue";
import SettingsView from "./SettingsView.vue";

type Filter = "all" | "running" | "favourites" | "waiting";

const filters: Array<{ id: Filter; label: Key; test: (p: Project) => boolean }> = [
  { id: "all", label: "window.filter.all", test: () => true },
  { id: "running", label: "window.filter.running", test: (p) => p.status === "running" },
  { id: "favourites", label: "window.filter.favourites", test: (p) => p.favourite },
  { id: "waiting", label: "window.filter.waiting", test: (p) => p.claude !== null },
];

const filter = ref<Filter>("all");
const settingsOpen = ref(false);
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
  return projects.value.filter((p) => current.value.test(p) && (!q || p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q)));
});
const selected = computed(() => projects.value.find((p) => p.path === selectedPath.value) ?? null);

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
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

/** In full screen the traffic lights are gone, and so is the room kept for them. */
const fullscreen = ref(false);
let unlistenResize: UnlistenFn | null = null;

const terminalPanel = ref<InstanceType<typeof TerminalPanel> | null>(null);

/** ⌘T opens a new terminal in the selected project. */
function onKey(event: KeyboardEvent): void {
  if (event.metaKey && event.code === "KeyT" && terminalPanel.value) {
    event.preventDefault();
    void terminalPanel.value.openTerminal();
  }
}

let unlistenReveal: UnlistenFn | null = null;

/** From the switcher (⌘↵ on a command): select the project, even if a filter or search hid it. */
function revealOutput(path: string, job: string): void {
  if (!rows.value.some((p) => p.path === path)) {
    filter.value = "all";
    query.value = "";
  }
  selectedPath.value = path;
  outputRequest.value = { path, job };
}

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  unlistenReveal = await listen<{ path: string; job: string }>("reveal-output", (event) => revealOutput(event.payload.path, event.payload.job));
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
  unlistenResize?.();
});

function server(project: Project): string {
  return meta(project, now.value);
}
</script>

<template>
  <div :class="['shell', { fullscreen }]">
    <aside class="sidebar">
      <!-- "deep": the logo and name drag the window too; so does the strip under the traffic lights. -->
      <div class="brand" data-tauri-drag-region="deep">
        <PitwallGlyph :size="30" lamps="var(--run)" />
        <span>Pitwall</span>
      </div>
      <nav :aria-label="t('window.filters')" class="nav">
        <button
          v-for="f in filters"
          :key="f.id"
          type="button"
          :class="{ on: filter === f.id }"
          @click="pick(f.id)"
        >
          {{ t(f.label) }}
          <span :class="['count', { hot: f.id === 'waiting' && count(f.id) > 0 }]">{{ count(f.id) }}</span>
        </button>
      </nav>
      <div class="grow"></div>
      <div class="nav">
        <button type="button" @click="api.pickFolder()"><Icon name="plus" /> {{ t("window.addProject") }}</button>
        <button type="button" :class="{ on: settingsOpen }" @click="settingsOpen = true"><Icon name="settings" /> {{ t("common.settings") }}</button>
      </div>
    </aside>

    <main class="main">
      <!-- Its heading and free space drag the window; the search box and buttons stay clickable. -->
      <div class="toolbar" data-tauri-drag-region="deep">
        <div class="heading">{{ t(current.label) }}</div>
        <label class="sr" for="search">{{ t("common.searchProjects") }}</label>
        <input id="search" v-model="query" type="search" :placeholder="t('common.searchProjects')" />
        <Spinner v-if="anyBusy" class="busy-spin" />
        <button type="button" class="control" :disabled="!projects.length || anyBusy" @click="api.startAll()">{{ t("common.startAll") }}</button>
        <button type="button" class="control" :disabled="!snapshot?.running || anyBusy" @click="api.stopAll()">{{ t("common.stopAll") }}</button>
      </div>

      <div v-if="rows.length" class="table">
        <div class="thead">
          <span>{{ t("window.col.project") }}</span>
          <span>{{ t("window.col.branch") }}</span>
          <span>{{ t("window.col.server") }}</span>
          <span>{{ t("window.col.claude") }}</span>
          <span></span>
        </div>
        <div ref="tbody" :class="['tbody', reorder.listClass()]">
          <div
            v-for="project in rows"
            :key="project.path"
            :data-path="project.path"
            :class="['trow', { selected: project.path === selectedPath }, reorder.rowClass(project.path)]"
            :style="reorder.rowStyle(project.path)"
            :title="t('window.rowTitle')"
            @pointerdown="reorder.down($event, project.path)"
            @click="select(project.path)"
            @dblclick="api.openEditor(project.path)"
          >
            <button type="button" class="cell-project" :aria-label="t('window.showName', { name: project.name })" @click.stop="select(project.path)">
              <StatusIcon :project="project" ring="var(--bg-app)" />
              <span class="names">
                <span class="name">
                  <span class="name-text">{{ project.name }}</span>
                  <span v-if="project.terminals.length" class="term-badge" :title="t('window.openTerminals', { count: project.terminals.length })">>_ {{ project.terminals.length }}</span>
                </span>
                <span class="path">{{ project.path }}</span>
              </span>
            </button>
            <span :class="['cell-git', { dirty: project.git?.changes }]">{{ gitLine(project.git) || "—" }}</span>
            <span :class="['cell-server', { bad: project.status === 'crashed', on: project.status === 'running' }]"><Marquee :text="server(project)" /></span>
            <span :class="['cell-claude', { hot: project.claude, live: !project.claude && project.claudeWorking }]"><Marquee :text="claudeState(project, now) || '—'" /></span>
            <span class="cell-actions" data-no-drag @click.stop @dblclick.stop>
              <button
                type="button"
                class="icon"
                :aria-label="t(project.favourite ? 'window.unfavouriteName' : 'window.favouriteName', { name: project.name })"
                :title="t(project.favourite ? 'window.favourite' : 'window.addFavourite')"
                @click="api.setFavourite(project.path, !project.favourite)"
              >
                <Icon :name="project.favourite ? 'star-filled' : 'star'" :size="14" />
              </button>
              <button
                type="button"
                class="icon"
                :disabled="project.status === 'busy'"
                :aria-label="t(project.status === 'running' ? 'common.stopName' : 'common.startName', { name: project.name })"
                @click="toggle(project)"
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

    <SettingsView v-if="settingsOpen" @close="settingsOpen = false" />
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

/* No traffic lights: the brand moves up, level with the toolbar heading. */
.shell.fullscreen .brand {
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
  padding: 1px 7px;
  border-radius: 9px;
  background: var(--claude);
  color: #0c0d10;
  font-weight: 600;
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
  border: 0;
  background: transparent;
  text-align: left;
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
