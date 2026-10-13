<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { depJobName, depJobs } from "../../lib/deps";
import { uptime } from "../../lib/format";
import { t } from "../../lib/i18n";
import { plainText, shownLines } from "../../lib/outputLines";
import { useOutputSearch } from "../../lib/outputSearch";
import type { OutputLine } from "../../lib/outputStream";
import { outputCollapsed, outputErrorsOnly, outputShown, type useResizer } from "../../lib/panel";
import { primary } from "../../lib/platform";
import { api, now } from "../../lib/store";
import { tabKey } from "../../lib/tabs";
import type { Project } from "../../lib/types";
import OutputLog from "./OutputLog.vue";
import OutputSearch from "./OutputSearch.vue";

type Resizer = ReturnType<typeof useResizer>;

/**
 * The output over the details' lower part: a handle, a tab per output (the dev server, then every
 * command that ran in this session), the shown one's tools (search, errors only, copy) and its
 * lines (OutputLog).
 *
 * `tab`: the shown output, `null` for the dev server, otherwise a command id. `runStart`: where
 * its newest run begins (lib/outputStream). `height`: the panel's height as shown (collapsed or
 * not). `resizer`: the handle's drag and keys (lib/panel). `card`: an error of the output is to
 * become a card, with this title and note.
 */
const props = defineProps<{ project: Project; tab: string | null; lines: OutputLine[]; runStart: number; height: number; resizer: Resizer }>();
const emit = defineEmits<{ show: [id: string | null]; card: [draft: { title: string; note: string }] }>();

const log = ref<InstanceType<typeof OutputLog> | null>(null);

const runsHere = computed(() => props.project.status !== "stopped" && !props.project.owner);
/** While the handle is dragged, the panel follows the pointer without animating. */
const dragging = computed(() => props.resizer.dragging.value);

/** Tabs: the dev server, then every command that ran in this session, then the Garage
 * page's installs and migrations that did (lib/deps) and the core doesn't list as commands. */
const tabs = computed(() => {
  const commands = (props.project.commands ?? []).filter((c) => c.status !== "idle" || c.result).map((c) => ({ id: c.id as string | null, name: c.name }));
  const listed = new Set((props.project.commands ?? []).map((c) => c.id));
  const depTabs = (depJobs.value[props.project.path] ?? [])
    .filter((job) => !listed.has(job))
    .map((job) => ({ id: job as string | null, name: depJobName(props.project.path, job) ?? job }));
  return [{ id: null as string | null, name: t("common.devServer") }, ...commands, ...depTabs];
});
/** The one tab in the Tab order: the shown one, else the dev server's. */
const activeTab = computed(() => (tabs.value.some((item) => item.id === props.tab) ? props.tab : null));

/** ←/→, Home and End move between the output tabs. */
function onTabKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, tabs.value.length, at);
  if (next !== null) emit("show", tabs.value[next].id);
}

function toggleOutput(): void {
  outputCollapsed.value = !outputCollapsed.value;
}

/** The handle doesn't drag a collapsed panel; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!outputCollapsed.value) props.resizer.start(event);
}

/* ---------- what the pane shows ---------- */

/** The output shown: a project's server, or one of its commands. Each has its own filter. */
const pane = computed(() => `${props.project.path}\u001f${props.tab ?? ""}`);

const errorsOnly = computed({
  get: () => outputErrorsOnly.value[pane.value] ?? false,
  set: (on) => (outputErrorsOnly.value = { ...outputErrorsOnly.value, [pane.value]: on }),
});
/** The errors of the run that is on: those of a run before are no longer the server's state. */
const errorCount = computed(() => props.lines.reduce((count, line) => count + (line.level === "error" && line.id >= props.runStart ? 1 : 0), 0));
const shown = computed(() => shownLines(props.lines, errorsOnly.value));

const empty = computed(() => {
  if (props.lines.length) return t("output.noErrors");
  if (props.tab !== null) return t("detail.noOutput");
  return t(props.project.owner ? "detail.noServerOutput" : "detail.notRunning");
});

/* ---------- search ---------- */

const { open: searching, query, ranges, count: matches, position, current, step, close } = useOutputSearch(shown);
const field = ref<InstanceType<typeof OutputSearch> | null>(null);
/** What had the keyboard when the search opened: closing gives it back. */
let before: HTMLElement | null = null;

async function openSearch(): Promise<void> {
  if (!searching.value) before = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  outputCollapsed.value = false;
  searching.value = true;
  await nextTick();
  field.value?.focus();
}

/** The marks go with the query; the keyboard goes back where it was, else to the lines. */
function closeSearch(): void {
  close();
  const back = before;
  before = null;
  if (back?.isConnected) back.focus({ preventScroll: true });
  else log.value?.focus();
}

/** ⌘F while the keyboard is anywhere in the panel (its lines take it with a click), and Esc
 * there closes the search: from its own buttons as from its field, else the keyboard stays. */
function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape" && searching.value) {
    event.preventDefault();
    event.stopPropagation();
    if (event.target instanceof Node && field.value?.$el.contains(event.target)) return closeSearch();
    close();
    before = null;
    return;
  }
  if (!primary(event) || event.shiftKey || event.altKey || event.key.toLowerCase() !== "f") return;
  event.preventDefault();
  void openSearch();
}

// Collapsed, there is no line to search in.
watch(outputCollapsed, (collapsed) => collapsed && close());

/* ---------- copy ---------- */

const copied = ref(false);
let copiedTimer = 0;

/** What the pane shows, as text. */
function copyAll(): void {
  void api.copyText(plainText(shown.value));
  copied.value = true;
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = false), 1200);
}

// The terminal panel snaps to the output only while the output is there.
outputShown.value = true;
onBeforeUnmount(() => {
  outputShown.value = false;
  window.clearTimeout(copiedTimer);
});
</script>

<template>
  <div :class="['out', { animated: !dragging, collapsed: outputCollapsed }]" :style="{ height: `${height}px` }" @keydown="onKey">
    <div
      class="grip"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('detail.resizeOutput')"
      tabindex="0"
      :aria-valuenow="Math.round(height)"
      @pointerdown="startDrag"
      @pointermove="resizer.move"
      @pointerup="resizer.end"
      @pointercancel="resizer.end"
      @keydown="resizer.nudge"
    ></div>
    <div class="tabs" role="tablist" :aria-label="t('detail.output')">
      <button
        v-for="(item, i) in tabs"
        :key="item.id ?? 'server'"
        type="button"
        role="tab"
        :aria-selected="tab === item.id"
        :tabindex="item.id === activeTab ? 0 : -1"
        :class="['tab', { on: tab === item.id }]"
        @click="emit('show', item.id)"
        @keydown="onTabKey($event, i)"
      >
        {{ item.name }}
      </button>
      <span class="grow"></span>
      <span v-if="tab === null && project.status === 'running' && runsHere" class="up">{{ uptime(project.startedAt, now) }}</span>
      <template v-if="!outputCollapsed">
        <button type="button" :class="['tool', { on: searching }]" :aria-pressed="searching" :aria-label="t('output.search')" :title="t('output.search')" @click="searching ? closeSearch() : openSearch()">
          <Icon name="search" :size="13" />
        </button>
        <button
          type="button"
          :class="['tool', { on: errorsOnly }]"
          :aria-pressed="errorsOnly"
          :aria-label="errorCount ? `${t('output.errorsOnly')} (${errorCount})` : t('output.errorsOnly')"
          :title="t('output.errorsOnly')"
          @click="errorsOnly = !errorsOnly"
        >
          <Icon name="filter" :size="13" />
          <span v-if="errorCount" class="errors">{{ errorCount }}</span>
        </button>
        <button type="button" class="tool" :disabled="!shown.length" :aria-label="t('output.copyAll')" :title="t(copied ? 'output.copied' : 'output.copyAll')" @click="copyAll">
          <Icon :name="copied ? 'check' : 'copy'" :size="13" />
        </button>
      </template>
      <button
        type="button"
        :class="['collapse', { up: outputCollapsed }]"
        :aria-expanded="!outputCollapsed"
        :aria-label="t(outputCollapsed ? 'detail.expandOutput' : 'detail.collapseOutput')"
        :title="t(outputCollapsed ? 'common.expand' : 'common.collapse')"
        @click="toggleOutput"
      >
        <Icon name="chevron" :size="13" />
      </button>
    </div>
    <OutputSearch v-if="searching" ref="field" v-model:query="query" :position="position" :count="matches" @step="step" @close="closeSearch" />
    <!-- One per output: each starts at its end, following. Collapsed, it is out of the keyboard's way. -->
    <OutputLog ref="log" :key="pane" class="log" :inert="outputCollapsed" :path="project.path" :lines="shown" :run-start="runStart" :ranges="ranges" :current="current" :empty="empty" @card="emit('card', $event)" />
  </div>
</template>

<style scoped>
/* Pinned to the bottom with the height you gave it; grown, it covers the sections. */
.out {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  overflow: clip;
  background: var(--bg-detail);
  border-top: 1px solid var(--line);
  box-shadow: var(--shadow-rise);
}

/* Collapsing and expanding slide (COLLAPSE_MS in lib/panel); a drag follows the pointer. */
.out.animated {
  transition: height 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.out.collapsed .grip {
  cursor: default;
}

.out.collapsed .grip::after {
  opacity: 0;
}

.out.collapsed .log {
  opacity: 0;
}

.grow {
  flex-grow: 1;
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse,
.tool {
  position: relative;
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.collapse:hover,
.tool:hover:not(:disabled),
.tool.on {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.tool:disabled {
  opacity: 0.45;
}

/* How many errors the output holds, on the corner of the button that shows them alone: the
   button keeps its width. Inside the row's height, which cuts what leaves it. */
.errors {
  position: absolute;
  top: 0;
  right: -4px;
  min-width: 13px;
  height: 13px;
  padding: 0 3px;
  box-sizing: border-box;
  border-radius: 7px;
  background: var(--crash-bg);
  box-shadow: 0 0 0 1px var(--crash-line);
  font-size: 9px;
  font-weight: 600;
  line-height: 13px;
  font-variant-numeric: tabular-nums;
  text-align: center;
  color: var(--crash-text);
  pointer-events: none;
}

.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

@media (prefers-reduced-motion: reduce) {
  .out.animated,
  .log,
  .collapse :deep(svg) {
    transition: none;
  }
}

/* Handle and tabs keep their size; only the log gives way when the panel is short. */
.grip {
  flex-shrink: 0;
  height: 9px;
  margin: 0 20px;
  cursor: row-resize;
  position: relative;
  outline: none;
}

.grip::after {
  content: "";
  position: absolute;
  left: 50%;
  top: 3px;
  width: 36px;
  height: 3px;
  margin-left: -18px;
  border-radius: 2px;
  background: var(--line-dialog);
}

.grip:hover::after,
.grip:focus-visible::after {
  background: var(--line-heavy);
}

/* Output tabs: the active one is white, the rest faded; nothing more. */
.tabs {
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 12px 0 14px;
  overflow: hidden;
}

/* With more tabs than there is room for, the names give way and the tools stay. */
.tab {
  min-width: 0;
  padding: 4px 6px;
  border: 0;
  background: transparent;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab:hover {
  color: var(--text-muted);
}

.tab.on {
  color: var(--text-lit);
}

.up {
  flex-shrink: 0;
  white-space: nowrap;
  padding-right: 6px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

/* The lines' box (OutputLog) takes the room that is left. */
.log {
  flex: 1 1 auto;
  min-height: 0;
  transition: opacity 160ms ease;
  margin: 6px 20px 20px;
  box-sizing: border-box;
}
</style>
