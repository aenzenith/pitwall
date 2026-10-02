<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeMark from "../components/ClaudeMark.vue";
import Icon from "../components/Icon.vue";
import { t } from "../lib/i18n";
import { keys, TERMINAL_MOD } from "../lib/platform";
import {
  COLLAPSED_HEIGHT,
  COLLAPSE_MS,
  outputCollapsed,
  outputHeight,
  outputShown,
  terminalCollapsed,
  terminalHeight,
  useResizer,
} from "../lib/panel";
import { api } from "../lib/store";
import { attach, fitView, focusView, measure } from "../lib/terminals";
import type { Project } from "../lib/types";
import RenameDialog from "./RenameDialog.vue";

/** Room above the panel that always stays: the toolbar (56) and the table header (38). */
const HEADER = 94;

const props = defineProps<{ project: Project }>();

const panel = ref<HTMLElement | null>(null);
const viewport = ref<HTMLElement | null>(null);
/** The tab shown, per project, so coming back to a project shows the same one. */
const chosen = ref<Record<string, number>>({});
const opening = ref(false);
/** The tab being renamed: double-click opens the rename dialog for it. */
const renaming = ref<{ id: number; name: string } | null>(null);

const terminals = computed(() => props.project.terminals);
const active = computed(() => {
  const id = chosen.value[props.project.path];
  return terminals.value.find((t) => t.id === id) ?? terminals.value[terminals.value.length - 1] ?? null;
});

const resizer = useResizer(
  terminalHeight,
  () => (panel.value?.parentElement?.clientHeight ?? 0) - HEADER,
  () => (outputShown.value && !outputCollapsed.value ? outputHeight.value : null),
);
const dragging = resizer.dragging;

/** Collapsed, only the tab row shows; the chosen height comes back on expanding. */
const shownHeight = computed(() => (terminalCollapsed.value ? COLLAPSED_HEIGHT : terminalHeight.value));
/** While the panel animates, the shell keeps its size: no resize per frame, no squeeze. */
const animating = ref(false);
let settle: ReturnType<typeof setTimeout> | undefined;

function toggleCollapsed(): void {
  terminalCollapsed.value = !terminalCollapsed.value;
  animating.value = true;
  clearTimeout(settle);
  settle = setTimeout(() => {
    animating.value = false;
    if (!terminalCollapsed.value && active.value) fitView(active.value.id);
  }, COLLAPSE_MS + 40);
}

/** The handle doesn't drag a collapsed panel; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!terminalCollapsed.value) resizer.start(event);
}

/** A new tab: the shell, or with `claude` a Claude Code session. */
async function openTerminal(claude = false): Promise<void> {
  if (opening.value) return;
  opening.value = true;
  try {
    // A new terminal is meant to be seen, and its shell starts at the open panel's size.
    if (terminalCollapsed.value) {
      toggleCollapsed();
      await new Promise((resolve) => setTimeout(resolve, COLLAPSE_MS + 60));
    }
    const size = viewport.value ? measure(viewport.value) : null;
    const view = await api.openTerminal(props.project.path, claude, size);
    chosen.value = { ...chosen.value, [props.project.path]: view.id };
  } finally {
    opening.value = false;
  }
}

function show(id: number): void {
  chosen.value = { ...chosen.value, [props.project.path]: id };
}

function close(id: number): void {
  void api.closeTerminal(id);
}

function rename(name: string): void {
  if (renaming.value) void api.renameTerminal(renaming.value.id, name);
}

/** Saved or not, the keyboard goes back to the terminal. */
function endRename(): void {
  const id = renaming.value?.id;
  renaming.value = null;
  if (id !== undefined) focusView(id);
}

const tablist = ref<HTMLElement | null>(null);
/** An arrow key picked the tab: the keyboard stays on the tabs instead of going to the shell. */
let stayOnTabs = false;
/** A tab closed from the keyboard: once it is gone, the focus goes to the tab shown next. */
let refocus: number | null = null;

function focusTab(id: number | undefined): void {
  if (id !== undefined) tablist.value?.querySelector<HTMLElement>(`[data-tab="${id}"]`)?.focus();
}

/**
 * The tabs are one Tab stop: ←/→ (Home, End) pick the next tab and keep the focus on the tabs;
 * ↵ and Space go into the shown terminal; Delete closes the tab, F2 renames it.
 */
function onTabKey(event: KeyboardEvent, id: number): void {
  const list = terminals.value;
  const at = list.findIndex((term) => term.id === id);
  let to: number | undefined;
  switch (event.key) {
    case "ArrowRight":
      to = list[(at + 1) % list.length]?.id;
      break;
    case "ArrowLeft":
      to = list[(at - 1 + list.length) % list.length]?.id;
      break;
    case "Home":
      to = list[0]?.id;
      break;
    case "End":
      to = list[list.length - 1]?.id;
      break;
    case "Enter":
    case " ":
      event.preventDefault();
      show(id);
      focusView(id);
      return;
    case "Delete":
    case "Backspace":
      event.preventDefault();
      refocus = id;
      stayOnTabs = true;
      close(id);
      return;
    case "F2": {
      event.preventDefault();
      const term = list[at];
      if (term) renaming.value = { id: term.id, name: term.name };
      return;
    }
    default:
      return;
  }
  event.preventDefault();
  if (to === undefined) return;
  if (to !== active.value?.id) stayOnTabs = true;
  show(to);
  focusTab(to);
}

// The shown terminal goes into the viewport; the others keep running out of sight. It takes the
// keyboard, unless the arrows on the tabs are picking it.
watch(
  () => active.value?.id,
  async (id) => {
    const focus = !stayOnTabs;
    stayOnTabs = false;
    await nextTick();
    if (id !== undefined && viewport.value) attach(id, viewport.value, !terminalCollapsed.value && !animating.value, focus);
    if (!focus) focusTab(id);
  },
  { immediate: true },
);

// A tab closed from the keyboard: the focus goes to the tab shown in its place (or, with none
// left, to "New tab").
watch(terminals, async (list) => {
  if (refocus === null || list.some((term) => term.id === refocus)) return;
  refocus = null;
  stayOnTabs = false;
  await nextTick();
  if (active.value) focusTab(active.value.id);
  else panel.value?.querySelector<HTMLElement>(".action")?.focus();
});

// No terminal opens on its own: a project starts with "No terminal in …" until you open one.

let observer: ResizeObserver | null = null;

onMounted(() => {
  observer = new ResizeObserver(() => {
    if (active.value && !terminalCollapsed.value && !animating.value) fitView(active.value.id);
  });
  if (viewport.value) observer.observe(viewport.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  clearTimeout(settle);
});

defineExpose({ openTerminal });
</script>

<template>
  <section
    ref="panel"
    :class="['term-panel', { animated: !dragging, collapsed: terminalCollapsed }]"
    :style="{ height: `${shownHeight}px` }"
    :aria-label="t('terminal.panel', { name: project.name })"
  >
    <div
      class="grip"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('terminal.resize')"
      tabindex="0"
      :aria-valuenow="Math.round(shownHeight)"
      @pointerdown="startDrag"
      @pointermove="resizer.move"
      @pointerup="resizer.end"
      @pointercancel="resizer.end"
      @keydown="resizer.nudge"
    ></div>
    <div class="tabs">
      <span :class="['where', { divided: terminals.length }]" :title="project.path">{{ project.name }}</span>
      <span id="terminal-tab-keys" class="sr-only">{{ t("terminal.tabKeys") }}</span>
      <!-- Each tab and its close button side by side, never one inside the other; the list holds
           tabs only, so the close button is the mouse's (the keyboard closes with Delete). One Tab
           stop: the shown tab; ←/→ move. Double-click (or F2) renames. -->
      <div v-if="terminals.length" ref="tablist" class="tablist" role="tablist" :aria-label="t('terminal.tabs')">
        <div
          v-for="term in terminals"
          :key="term.id"
          :class="['tab', { on: active?.id === term.id }]"
          role="none"
          :title="t('terminal.renameHint')"
          @click="show(term.id)"
          @dblclick="renaming = { id: term.id, name: term.name }"
        >
          <div
            role="tab"
            class="tab-label"
            :data-tab="term.id"
            :tabindex="active?.id === term.id ? 0 : -1"
            :aria-selected="active?.id === term.id"
            aria-keyshortcuts="Delete F2"
            aria-describedby="terminal-tab-keys"
            @keydown="onTabKey($event, term.id)"
          >
            <ClaudeMark v-if="term.kind === 'claude'" :size="11" />
            <Icon v-else name="terminal" :size="11" />
            <span>{{ term.name }}</span>
          </div>
          <button
            type="button"
            class="x"
            tabindex="-1"
            aria-hidden="true"
            :title="t('common.close')"
            @click.stop="close(term.id)"
            @dblclick.stop
          >×</button>
        </div>
      </div>
      <!-- Left: the project and its tabs. Right: new tab buttons and collapse. -->
      <span class="grow"></span>
      <button type="button" class="action" :title="t('terminal.newTabTitle', { shortcut: keys(`${TERMINAL_MOD}+T`) })" :disabled="opening" @click="openTerminal()">
        <Icon name="plus" :size="13" /> {{ t("terminal.newTab") }}
      </button>
      <button type="button" class="action" :title="t('terminal.newClaudeTitle')" :disabled="opening" @click="openTerminal(true)">
        <ClaudeMark :size="12" /> {{ t("terminal.newClaude") }}
      </button>
      <button
        type="button"
        :class="['icon', 'collapse', { up: terminalCollapsed }]"
        :aria-expanded="!terminalCollapsed"
        :aria-label="t(terminalCollapsed ? 'terminal.expand' : 'terminal.collapse')"
        :title="t(terminalCollapsed ? 'common.expand' : 'common.collapse')"
        @click="toggleCollapsed"
      >
        <Icon name="chevron" :size="13" />
      </button>
    </div>
    <div class="screen">
      <div ref="viewport" class="viewport"></div>
      <div v-if="!active" class="empty">
        <span>{{ t("terminal.none", { name: project.name }) }}</span>
      </div>
    </div>

    <RenameDialog v-if="renaming" :name="renaming.name" :project-name="project.name" @save="rename" @close="endRename" />
  </section>
</template>

<style scoped>
/* Pinned under the list with its own height (lib/panel). */
.term-panel {
  flex-shrink: 0;
  min-height: 0;
  max-height: calc(100% - 94px);
  overflow: clip;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--line);
  box-shadow: 0 -12px 24px rgba(0, 0, 0, 0.22);
}

/* Collapsing and expanding slide (COLLAPSE_MS in lib/panel); a drag follows the pointer. */
.term-panel.animated {
  transition: height 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.term-panel.collapsed .grip {
  cursor: default;
}

.term-panel.collapsed .grip::after {
  opacity: 0;
}

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
  background: #2f343d;
}

.grip:hover::after {
  background: #4a515c;
}

/* The handle's own focus: the bar in the focus ring's colour. */
.grip:focus-visible::after {
  background: var(--focus-ring);
}

/* A fixed height: renaming a tab (its input) never pushes the terminal down. */
.tabs {
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 12px 0 14px;
  min-width: 0;
}

.tablist {
  display: flex;
  align-items: center;
  gap: 2px;
}

/* The tab and its close button read as one: the hover, the colour and the hand are the pair's. */
.tab {
  height: 22px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 6px;
  border-radius: 6px;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
  cursor: pointer;
}

.tab-label {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

/* The keyboard's tab: the ring goes round the pair, as the tab looks like one piece. */
.tab-label:focus-visible {
  outline: none;
}

.tab:has(> .tab-label:focus-visible) {
  outline: 2px solid var(--focus-ring);
  outline-offset: -1px;
}

.tab:hover {
  color: var(--text-muted);
}

.tab.on {
  color: #fff;
}

.x {
  width: 14px;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: center;
  font-size: 13px;
  line-height: 1;
  color: transparent;
  border-radius: 4px;
}

.tab:hover .x,
.tab.on .x {
  color: var(--text-faint);
}

.x:hover {
  color: var(--text-strong) !important;
  background: #262a33;
}

.icon {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.icon:hover:not(:disabled) {
  background: #262a33;
  color: var(--text-strong);
}

.grow {
  flex-grow: 1;
}

/* New tab buttons: a terminal, or a Claude Code session. */
.action {
  height: 24px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.action:hover:not(:disabled) {
  background: #262a33;
  color: var(--text-strong);
}

/* The project, first in the row, set off from its tabs. */
.where {
  flex-shrink: 0;
  max-width: 180px;
  margin-right: 6px;
  padding: 0 10px 0 6px;
  border-right: 1px solid transparent;
  font-size: 12px;
  font-weight: 600;
  line-height: 14px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The divider separates the project from its tabs; with no tabs there is nothing to set off. */
.where.divided {
  border-right-color: var(--line);
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

.screen {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  margin: 6px 20px 20px;
  padding: 8px 4px 8px 10px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid #1f2228;
  display: flex;
  overflow: clip;
  transition: opacity 160ms ease;
}

.term-panel.collapsed .screen {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .term-panel.animated,
  .screen,
  .collapse :deep(svg) {
    transition: none;
  }
}

/* Nothing inside the terminal can scroll it or anything around it. */
.viewport {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow: clip;
}

.viewport :deep(.terminal-host),
.viewport :deep(.xterm) {
  height: 100%;
}

/* xterm paints its scroll area black; below the last row it would show as a strip. */
.viewport :deep(.xterm-viewport) {
  background-color: var(--bg-log) !important;
}

/* Over the viewport, so the viewport always keeps its size and can be measured. */
.empty {
  position: absolute;
  inset: 0;
  border-radius: 8px;
  background: var(--bg-log);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-subtle);
}
</style>
