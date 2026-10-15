<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import { shellRequest } from "../lib/flag";
import { t } from "../lib/i18n";
import { keys, TERMINAL_MOD } from "../lib/platform";
import {
  chosenTerminal,
  COLLAPSED_HEIGHT,
  COLLAPSE_MS,
  outputCollapsed,
  outputHeight,
  outputShown,
  terminalCollapsed,
  terminalHeight,
  terminalRequest,
  trackTerminalCollapsed,
  trackTerminalHeight,
  trackTerminals,
  useResizer,
} from "../lib/panel";
import { statusLine } from "../lib/sessions";
import { MODEL_LABELS, MODELS, projectChoice } from "../lib/start";
import { api, now, sessions } from "../lib/store";
import { attach, fitView, focusView, measure } from "../lib/terminals";
import type { Project, StartChoice } from "../lib/types";
import RenameDialog from "./RenameDialog.vue";

/** How long a `reveal-terminal` request waits for its project and tab to show up. */
const REQUEST_MS = 5_000;

/**
 * The selected project's terminals. `track`: the Track page's, the same panel with one difference:
 * it lies over the page's bottom instead of taking its room from what is above it, so nothing there
 * is laid out again (or scrolls in less room) as it grows. Its height and collapsed state are that
 * page's own (lib/panel), and a press on what it lies over folds it away (`collapse`).
 */
const props = defineProps<{ project: Project; track?: boolean }>();

/** Room above the panel that always stays: the toolbar (56), and under it the list's table header
 * (38). */
const HEADER = props.track ? 56 : 94;
const height = props.track ? trackTerminalHeight : terminalHeight;
const collapsed = props.track ? trackTerminalCollapsed : terminalCollapsed;

const panel = ref<HTMLElement | null>(null);
const viewport = ref<HTMLElement | null>(null);
const opening = ref(false);
/** The tab being renamed: double-click opens the rename dialog for it. */
const renaming = ref<{ id: number; name: string } | null>(null);

const terminals = computed(() => props.project.terminals);

/** Claude in a tab's terminal, by the terminal's id: at work, or waiting on you (a turn finished
 * and not looked at yet, a question, a permission prompt). Nothing for a tab with neither. */
const live = computed(() => {
  const states = new Map<number, { phase: "waiting" | "working"; text: string }>();
  for (const row of sessions.value?.sessions ?? []) {
    if (row.origin.kind !== "pitwall" || (row.phase !== "waiting" && row.phase !== "working")) continue;
    // Two in one terminal (one left running in the background): the one that waits on you tells.
    if (row.phase === "working" && states.get(row.origin.terminal)?.phase === "waiting") continue;
    states.set(row.origin.terminal, { phase: row.phase, text: statusLine(row, now.value).text });
  }
  return states;
});
const active = computed(() => {
  const id = chosenTerminal.value[props.project.path];
  return terminals.value.find((t) => t.id === id) ?? terminals.value[terminals.value.length - 1] ?? null;
});

const resizer = useResizer(
  height,
  () => (panel.value?.parentElement?.clientHeight ?? 0) - HEADER,
  () => (!props.track && outputShown.value && !outputCollapsed.value ? outputHeight.value : null),
);
const dragging = resizer.dragging;

/** Collapsed, only the tab row shows; the chosen height comes back on expanding. */
const shownHeight = computed(() => (collapsed.value ? COLLAPSED_HEIGHT : height.value));
/** While the panel animates, the shell keeps its size: no resize per frame, no squeeze. */
const animating = ref(false);
let settle: ReturnType<typeof setTimeout> | undefined;

function toggleCollapsed(): void {
  collapsed.value = !collapsed.value;
  animating.value = true;
  clearTimeout(settle);
  settle = setTimeout(() => {
    animating.value = false;
    if (!collapsed.value && active.value) fitView(active.value.id);
  }, COLLAPSE_MS + 40);
}

/**
 * The Track page's panel folds away on a press on the stage behind it (TrackView), its tab row
 * left. Not while a tab is opening: its shell starts at the open panel's size.
 */
function collapse(): void {
  if (!collapsed.value && !opening.value) toggleCollapsed();
}

/** The handle doesn't drag a collapsed panel; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!collapsed.value) resizer.start(event);
}

/** A new tab: the shell, or with `claude` a Claude Code session (`start`: with the model and in
 * the mode picked from its list; without one, as the project's settings say). */
async function openTerminal(claude = false, start: StartChoice | null = null): Promise<void> {
  if (opening.value) return;
  opening.value = true;
  try {
    // A new terminal is meant to be seen, and its shell starts at the open panel's size.
    if (collapsed.value) {
      toggleCollapsed();
      await new Promise((resolve) => setTimeout(resolve, COLLAPSE_MS + 60));
    }
    const size = viewport.value ? measure(viewport.value) : null;
    const view = await api.openTerminal(props.project.path, claude, size, start);
    if (props.track) trackTerminals.add(view.id);
    chosenTerminal.value = { ...chosenTerminal.value, [props.project.path]: view.id };
  } finally {
    opening.value = false;
  }
}

function show(id: number): void {
  unasked.delete(id);
  chosenTerminal.value = { ...chosenTerminal.value, [props.project.path]: id };
}

/**
 * A tab that came by itself while this panel was up: one the core opened at a board card's
 * moment (`unasked`). The tab in sight stays in sight and the keyboard stays where it is: whoever
 * is typing here goes on typing where they were. With no tab in sight, the one that came shows,
 * without the keyboard, and only that one is noted here, until it is shown. From then on it is a
 * tab like any other: clicked, brought up, or shown in the place of one that closed, it takes the
 * keyboard. A tab the user asked for (put back, a card given by hand) is shown as any other.
 */
const unasked = new Set<number>();

watch(
  () => [props.project.path, terminals.value] as const,
  ([path, list], [wasPath, was]) => {
    if (path !== wasPath) return unasked.clear();
    const came = list.filter((term) => !was.some((old) => old.id === term.id));
    // One this panel is opening is asked for, and so is any the core doesn't call unasked.
    if (!came.length || opening.value || !came.every((term) => term.unasked)) return;
    const chosen = chosenTerminal.value[path];
    if (came.some((term) => term.id === chosen)) return;
    const shown = was.find((term) => term.id === chosen) ?? was[was.length - 1];
    // No tab was in sight: the last one is the one shown now.
    if (!shown) {
      unasked.add(list[list.length - 1].id);
      return;
    }
    if (shown.id !== chosen && list.some((term) => term.id === shown.id)) chosenTerminal.value = { ...chosenTerminal.value, [path]: shown.id };
  },
  // Before the shown tab is worked out again.
  { flush: "sync" },
);

/* ---------- "New Claude"'s list ---------- */

/** How "New Claude" starts a session here when nothing is picked: the project's own settings. */
const defaults = computed(() => projectChoice(props.project));

type ClaudeEntry = { key: string; label: string; checked: boolean; line: boolean; start: StartChoice };

/** The models (the project's own checked), then plan mode: each starts a session at once, the
 * rest as the project's settings say. */
const claudeEntries = computed<ClaudeEntry[]>(() => [
  ...MODELS.map((model) => ({ key: model, label: t(MODEL_LABELS[model]), checked: defaults.value.model === model, line: false, start: { model, mode: defaults.value.mode } })),
  { key: "plan", label: t("start.newClaudePlan"), checked: defaults.value.mode === "plan", line: true, start: { model: defaults.value.model, mode: "plan" as const } },
]);

const claudeSplit = ref<HTMLElement | null>(null);
const claudeMore = ref<HTMLButtonElement | null>(null);
const claudeMenu = ref<HTMLElement | null>(null);
const claudeOpen = ref(false);
/** The line under the pointer or the keyboard. */
const claudeAt = ref(0);
const claudePlace = ref<Record<string, string>>({});

/** Under the button, their right edges level; over it where the window has no room below (the
 * panel lies along its bottom). Drawn in the page: a checked line beside plain ones, as the
 * board picker's list. */
async function showClaudeMenu(): Promise<void> {
  if (claudeOpen.value || opening.value) return;
  claudeAt.value = Math.max(0, claudeEntries.value.findIndex((entry) => entry.checked));
  claudePlace.value = { visibility: "hidden" };
  claudeOpen.value = true;
  window.addEventListener("pointerdown", onClaudeOutside, true);
  window.addEventListener("blur", hideClaudeMenu);
  window.addEventListener("resize", hideClaudeMenu);
  await nextTick();
  const box = claudeSplit.value?.getBoundingClientRect();
  const list = claudeMenu.value;
  if (!box || !list) return hideClaudeMenu();
  const height = list.getBoundingClientRect().height;
  const below = box.bottom + 6;
  const top = below + height <= window.innerHeight - 8 ? below : Math.max(8, box.top - 6 - height);
  claudePlace.value = { top: `${top}px`, right: `${Math.max(8, window.innerWidth - box.right)}px` };
  // Once it is in its place and in sight: until then it takes no focus.
  await nextTick();
  claudeMenu.value?.focus();
}

function hideClaudeMenu(): void {
  claudeOpen.value = false;
  window.removeEventListener("pointerdown", onClaudeOutside, true);
  window.removeEventListener("blur", hideClaudeMenu);
  window.removeEventListener("resize", hideClaudeMenu);
}

/** A press anywhere else closes it; one on its own button is that button's to answer. */
function onClaudeOutside(event: PointerEvent): void {
  const target = event.target instanceof Node ? event.target : null;
  if (!(target && (claudeMenu.value?.contains(target) || claudeMore.value?.contains(target)))) hideClaudeMenu();
}

function pickClaude(entry: ClaudeEntry): void {
  hideClaudeMenu();
  void openTerminal(true, entry.start);
}

/** In the list: arrows, Home and End move, ↵ or Space starts the session, Esc and Tab close (the
 * keyboard goes back to the button); a shortcut closes it and stays the page's. */
function onClaudeKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hideClaudeMenu();
  const last = claudeEntries.value.length - 1;
  switch (event.key) {
    case "ArrowDown":
      claudeAt.value = Math.min(last, claudeAt.value + 1);
      break;
    case "ArrowUp":
      claudeAt.value = Math.max(0, claudeAt.value - 1);
      break;
    case "Home":
      claudeAt.value = 0;
      break;
    case "End":
      claudeAt.value = last;
      break;
    case "Enter":
    case " ":
      pickClaude(claudeEntries.value[claudeAt.value]);
      break;
    case "Escape":
    case "Tab":
      hideClaudeMenu();
      claudeMore.value?.focus();
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

/** A tab asked for by a click, Return or Space: shown, and a collapsed panel comes up with it. */
function open(id: number): void {
  show(id);
  if (collapsed.value) toggleCollapsed();
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

/** More tabs than the row holds even narrowed: the one shown comes into sight. */
function showTab(id: number | undefined): void {
  if (id !== undefined) tablist.value?.querySelector(`[data-tab="${id}"]`)?.closest(".tab")?.scrollIntoView({ block: "nearest", inline: "nearest" });
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
      open(id);
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
// keyboard, unless the arrows on the tabs are picking it, or the panel is collapsed: a terminal
// out of sight leaves the keyboard where it is.
watch(
  () => active.value?.id,
  async (id) => {
    const onTabs = stayOnTabs;
    stayOnTabs = false;
    await nextTick();
    // A tab that came by itself is shown without the keyboard.
    const quiet = id !== undefined && unasked.delete(id);
    showTab(id);
    if (id !== undefined && viewport.value) attach(id, viewport.value, !collapsed.value && !animating.value, !onTabs && !collapsed.value && !quiet);
    if (onTabs) focusTab(id);
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

// A Claude session in one of these terminals was brought up (`reveal-terminal`, lib/panel:
// terminalRequest): its tab, the panel open, the keyboard in the shell. A request waits a moment
// for its project and tab to arrive, then lapses.
watch(
  () => [terminalRequest.value, props.project.path, terminals.value] as const,
  async ([request, path, list]) => {
    if (!request) return;
    if (Date.now() - request.at > REQUEST_MS) {
      terminalRequest.value = null;
      return;
    }
    if (request.path !== path || !list.some((term) => term.id === request.id)) return;
    terminalRequest.value = null;
    if (collapsed.value) toggleCollapsed();
    const shown = active.value?.id === request.id;
    show(request.id);
    // Already the shown tab: the watch on it won't run, so the keyboard goes there from here.
    if (shown) {
      await nextTick();
      focusView(request.id);
    }
  },
  { immediate: true },
);

// The chequered flag's "Open in terminal" (lib/flag: shellRequest): a new shell tab of this
// project, opened as "New tab" opens one. The list's panel only, once it is on the page (its
// size is the tab's); a request waits a moment for its project to be the one shown, then lapses.
watch(
  () => [shellRequest.value, props.project.path] as const,
  async ([request, path]) => {
    if (!request || props.track) return;
    if (Date.now() - request.at > REQUEST_MS) {
      shellRequest.value = null;
      return;
    }
    if (request.path !== path) return;
    shellRequest.value = null;
    await nextTick();
    void openTerminal();
  },
  { immediate: true },
);

// No terminal opens on its own: a project starts with "No terminal in …" until you open one.

let observer: ResizeObserver | null = null;
/** A row that narrows (the window, the panel) keeps the shown tab in sight too. */
let tabsObserver: ResizeObserver | null = null;
watch(tablist, (list, old) => {
  if (old) tabsObserver?.unobserve(old);
  if (list) tabsObserver?.observe(list);
});

onMounted(() => {
  observer = new ResizeObserver(() => {
    if (active.value && !collapsed.value && !animating.value) fitView(active.value.id);
  });
  if (viewport.value) observer.observe(viewport.value);
  tabsObserver = new ResizeObserver(() => showTab(active.value?.id));
  if (tablist.value) tabsObserver.observe(tablist.value);
});

onBeforeUnmount(() => {
  observer?.disconnect();
  tabsObserver?.disconnect();
  clearTimeout(settle);
  hideClaudeMenu();
});

defineExpose({ openTerminal, collapse });
</script>

<template>
  <section
    ref="panel"
    :class="['term-panel', { animated: !dragging, collapsed, over: track }]"
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
          @click="open(term.id)"
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
            <ClaudeLogo v-if="term.kind === 'claude'" :size="11" />
            <Icon v-else name="terminal" :size="11" />
            <span class="tab-name">{{ term.name }}</span>
            <ClaudeDot v-if="live.has(term.id)" :live="live.get(term.id)" />
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
      <button type="button" class="action new-tab" :title="t('terminal.newTabTitle', { shortcut: keys(`${TERMINAL_MOD}+T`) })" :disabled="opening" @click="openTerminal()">
        <Icon name="plus" :size="13" /> <span class="action-text">{{ t("terminal.newTab") }}</span>
      </button>
      <!-- One button in two parts: a session as the project starts them, and the list of others. -->
      <span ref="claudeSplit" class="split">
        <button type="button" class="action new-claude" :title="t('terminal.newClaudeTitle')" :disabled="opening" @click="openTerminal(true)">
          <ClaudeLogo :size="12" /> <span class="action-text">{{ t("terminal.newClaude") }}</span>
        </button>
        <button
          ref="claudeMore"
          type="button"
          :class="['action', 'more', { open: claudeOpen }]"
          aria-haspopup="menu"
          :aria-expanded="claudeOpen"
          :aria-label="t('start.newClaudeOptions')"
          :title="claudeOpen ? undefined : t('start.newClaudeOptions')"
          :disabled="opening"
          @click="claudeOpen ? hideClaudeMenu() : showClaudeMenu()"
          @keydown.down.prevent="showClaudeMenu"
          @keydown.up.prevent="showClaudeMenu"
        >
          <Icon name="chevron-down" :size="11" />
        </button>
      </span>
      <button
        type="button"
        :class="['icon', 'collapse', { up: collapsed }]"
        :aria-expanded="!collapsed"
        :aria-label="t(collapsed ? 'terminal.expand' : 'terminal.collapse')"
        :title="t(collapsed ? 'common.expand' : 'common.collapse')"
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

    <!-- "New Claude"'s list. A press on a line leaves the keyboard in the list. -->
    <ul
      v-if="claudeOpen"
      ref="claudeMenu"
      class="claude-menu"
      role="menu"
      tabindex="-1"
      :aria-label="t('start.newClaudeOptions')"
      :aria-activedescendant="`claude-menu-${claudeAt}`"
      :style="claudePlace"
      @keydown="onClaudeKey"
      @mousedown.prevent
    >
      <template v-for="(entry, i) in claudeEntries" :key="entry.key">
        <li v-if="entry.line" class="claude-menu-line" role="separator"></li>
        <li
          :id="`claude-menu-${i}`"
          :class="['claude-menu-item', { on: i === claudeAt }]"
          :role="entry.line ? 'menuitemcheckbox' : 'menuitemradio'"
          :aria-checked="entry.checked"
          @mousemove="claudeAt = i"
          @click="pickClaude(entry)"
        >
          <span class="claude-menu-check"><Icon v-if="entry.checked" name="check" :size="13" /></span>
          <span class="claude-menu-name">{{ entry.label }}</span>
        </li>
      </template>
    </ul>

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
  box-shadow: var(--shadow-rise);
}

/* The Track page's: over the page's bottom, on a ground of its own, up to the page's bar. */
.term-panel.over {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 2;
  max-height: calc(100% - 56px);
  background: var(--bg-app);
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
  background: var(--line-dialog);
}

.grip:hover::after {
  background: var(--line-heavy);
}

/* The handle's own focus: the bar in the focus ring's colour. */
.grip:focus-visible::after {
  background: var(--focus-ring);
}

/* A fixed height: renaming a tab (its input) never pushes the terminal down. */
.tabs {
  container-type: inline-size;
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 12px 0 14px;
  min-width: 0;
}

/* More tabs than the row holds: they narrow, their names cut (well before the project's is), and
   the buttons on the right stay where they are; the one shown keeps in sight (one tab's room at
   least). */
.tablist {
  flex: 0 100000 auto;
  min-width: 56px;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow: hidden;
}

/* The tab and its close button read as one: the hover, the colour and the hand are the pair's. */
.tab {
  min-width: 56px;
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
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.tab-name {
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab-label > :not(.tab-name),
.x {
  flex-shrink: 0;
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
  color: var(--text-lit);
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
  background: var(--bg-icon-hover);
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
  background: var(--bg-icon-hover);
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
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* "New Claude" and the arrow of its list: one button in two parts, side by side. */
.split {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
}

.split > .action:first-child {
  padding-right: 5px;
  border-radius: 6px 0 0 6px;
}

.action.more {
  width: 18px;
  padding: 0;
  justify-content: center;
  border-radius: 0 6px 6px 0;
}

.action.more.open {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* A narrow row: "New tab" is its plus alone, and narrower still "New Claude" its mark (still their
   tooltips, and read out). */
@container (max-width: 455px) {
  .action.new-tab {
    flex-shrink: 0;
    width: 24px;
    padding: 0;
    justify-content: center;
  }

  .new-tab .action-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

@container (max-width: 359px) {
  .split > .action.new-claude {
    width: 24px;
    padding: 0;
    justify-content: center;
  }

  .new-claude .action-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

/* Its list, over the page wherever the panel's edges are. */
.claude-menu {
  position: fixed;
  z-index: 60;
  box-sizing: border-box;
  width: max-content;
  min-width: 210px;
  max-width: calc(100vw - 16px);
  margin: 0;
  padding: 5px;
  list-style: none;
  border: 1px solid var(--line-dialog);
  border-radius: var(--radius-card);
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
}

/* The keyboard's place is the lit line, not a ring round the list. */
.claude-menu:focus-visible {
  outline: none;
}

.claude-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 30px;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: pointer;
}

.claude-menu-item.on {
  background: var(--bg-lit);
}

/* The project's own: a check, in a place every line keeps so the names line up. */
.claude-menu-check {
  flex-shrink: 0;
  display: inline-flex;
  width: 14px;
  color: var(--run);
}

.claude-menu-line {
  height: 1px;
  margin: 4px 8px;
  background: var(--line);
}

/* The project, first in the row, set off from its tabs; its name is cut only once they have no
   more room to give. */
.where {
  flex-shrink: 1;
  min-width: 48px;
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
  border: 1px solid var(--line-inset);
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
