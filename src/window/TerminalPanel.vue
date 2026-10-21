<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import { shellRequest } from "../lib/flag";
import { t } from "../lib/i18n";
import { useNativeMenu } from "../lib/nativeMenu";
import { isMac } from "../lib/platform";
import {
  chosenTerminal,
  COLLAPSED_HEIGHT,
  COLLAPSE_MS,
  outputCollapsed,
  outputHeight,
  outputShown,
  terminalCollapsed,
  terminalHeight,
  terminalHistoryShown,
  terminalOrder,
  HISTORY_MIN,
  HISTORY_WIDTH,
  historyWidth,
  terminalRequest,
  sessionsTerminalCollapsed,
  sessionsTerminalHeight,
  trackTerminalCollapsed,
  trackTerminalHeight,
  trackTerminals,
  useResizer,
} from "../lib/panel";
import { statusLine } from "../lib/sessions";
import { MODEL_LABELS, MODELS, projectChoice } from "../lib/start";
import { api, now, sessions } from "../lib/store";
import { useTabReorder } from "../lib/tabReorder";
import { attach, fitView, focusView, measure } from "../lib/terminals";
import type { Project, StartChoice } from "../lib/types";
import RenameDialog from "./RenameDialog.vue";
import TerminalHistory from "./TerminalHistory.vue";

/** How long a `reveal-terminal` request waits for its project and tab to show up. */
const REQUEST_MS = 5_000;

/**
 * The selected project's terminals. `place`: the page they are on, the list's by default. The Track
 * page's is the same panel with one difference: it lies over the page's bottom instead of taking
 * its room from what is above it, so nothing there is laid out again (or scrolls in less room) as
 * it grows, and a press on what it lies over folds it away (`collapse`). The Sessions page's takes
 * its room under that page's list, as the list's does; the page moves it from session to session,
 * so it never takes the keyboard by itself. Each has a height and a collapsed state of its own
 * (lib/panel).
 */
const props = withDefaults(defineProps<{ project: Project; place?: "list" | "track" | "sessions" }>(), { place: "list" });

/** Room above the panel that always stays: the toolbar (56), and under it the list's table header
 * (38) or the Sessions page's filters (44). */
const PLACES = {
  list: { header: 94, height: terminalHeight, collapsed: terminalCollapsed },
  track: { header: 56, height: trackTerminalHeight, collapsed: trackTerminalCollapsed },
  sessions: { header: 100, height: sessionsTerminalHeight, collapsed: sessionsTerminalCollapsed },
};
const { header: HEADER, height, collapsed } = PLACES[props.place];

const panel = ref<HTMLElement | null>(null);
const viewport = ref<HTMLElement | null>(null);
const opening = ref(false);
/** The tab being renamed: double-click opens the rename dialog for it. */
const renaming = ref<{ id: number; name: string } | null>(null);

/** The project's terminals, in the order their tabs were dragged into (lib/panel), new ones last. */
const terminals = computed(() => {
  const list = props.project.terminals;
  const order = terminalOrder.value[props.project.path];
  if (!order) return list;
  const rank = new Map(order.map((id, i) => [id, i]));
  const unranked = Number.MAX_SAFE_INTEGER;
  return [...list].sort((a, b) => (rank.get(a.id) ?? unranked) - (rank.get(b.id) ?? unranked));
});

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
  () => (props.place === "list" && outputShown.value && !outputCollapsed.value ? outputHeight.value : null),
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

/* ---------- the history ---------- */

/** Below this width the panel has no room for the history beside the terminal: it lies over the
 * terminal's right side instead, and goes once a session is picked. */
const HISTORY_ROOM = 760;
const narrow = ref(false);
/** The narrow panel's history, over the terminal. */
const overlayOpen = ref(false);
/** Shown, as "History" says it: never while the panel is collapsed. The column beside the terminal
 * is kept through a collapse and comes back with the panel; the narrow panel's goes with it. */
const historyOpen = computed(() => !collapsed.value && (narrow.value ? overlayOpen.value : terminalHistoryShown.value));
/** The history beside the terminal; `sliding`: its room is still opening, closing or being
 * resized. */
const beside = computed(() => terminalHistoryShown.value && !narrow.value && !collapsed.value);
const sliding = ref(false);
/** A few frames have passed since the panel came: until then the history takes its place (the
 * panel's width, measured) without moving, so coming to the page shows it as it was. */
const settled = ref(false);
/** What the terminal keeps beside the history, however wide that is dragged; and the margins
 * around the two (20 left, 12 between, 20 right). */
const TERMINAL_ROOM = 320;
const MARGINS = 52;
const panelWidth = ref(0);
/** The history's width as the panel allows it: what it was dragged to, the terminal's room kept. */
const widest = computed(() => Math.max(HISTORY_MIN, panelWidth.value - TERMINAL_ROOM - MARGINS));
const historyShown = computed(() => Math.min(historyWidth.value, widest.value));
/** The room opens in this long; closing, it waits as long again for the list to fade (CSS). */
const SLIDE_MS = COLLAPSE_MS + 120;
let slid: ReturnType<typeof setTimeout> | undefined;
const side = ref<InstanceType<typeof TerminalHistory> | null>(null);
const over = ref<InstanceType<typeof TerminalHistory> | null>(null);

// While the history's room opens or closes, the terminal follows its width frame by frame; the
// shell is told the size it ends at.
watch([beside, historyShown], () => {
  if (!settled.value) return;
  sliding.value = true;
  clearTimeout(slid);
  slid = setTimeout(() => {
    sliding.value = false;
    if (!collapsed.value && !animating.value && active.value) fitView(active.value.id);
  }, SLIDE_MS + 40);
});

/** "History": the column beside the terminal, or a narrow panel's over it; shown, the keyboard
 * goes to its search, and a collapsed panel comes up with it. Hidden, back to the terminal. */
async function toggleHistory(): Promise<void> {
  const open = !historyOpen.value;
  // The panel comes up first: coming up, it would put away a history over the terminal.
  if (open && collapsed.value) {
    toggleCollapsed();
    await nextTick();
  }
  if (narrow.value) overlayOpen.value = open;
  else terminalHistoryShown.value = open;
  if (!open) return backToTerminal();
  await nextTick();
  (narrow.value ? over : side).value?.focus();
}

function closeOverlay(): void {
  overlayOpen.value = false;
  backToTerminal();
}

function backToTerminal(): void {
  if (active.value) focusView(active.value.id);
}

/* The history's width: its handle, the gap between it and the terminal, drags it (the pointer
   moving left widens it); ←/→ step it; a double click puts the default back. */
let widthDrag: { x: number; width: number } | null = null;
const resizing = ref(false);

function setWidth(width: number): void {
  historyWidth.value = Math.round(Math.max(HISTORY_MIN, Math.min(widest.value, width)));
}

function startWidth(event: PointerEvent): void {
  widthDrag = { x: event.clientX, width: historyShown.value };
  resizing.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

function moveWidth(event: PointerEvent): void {
  if (widthDrag) setWidth(widthDrag.width + widthDrag.x - event.clientX);
}

function endWidth(): void {
  widthDrag = null;
  resizing.value = false;
}

function nudgeWidth(event: KeyboardEvent): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  setWidth(historyShown.value + (event.key === "ArrowLeft" ? 24 : -24));
}

function resetWidth(): void {
  historyWidth.value = HISTORY_WIDTH;
}

/** A press on the terminal puts away the history lying over it. */
function pressScreen(): void {
  seeFinished(active.value?.id);
  overlayOpen.value = false;
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
    if (props.place === "track") trackTerminals.add(view.id);
    asked = view.id;
    chosenTerminal.value = { ...chosenTerminal.value, [props.project.path]: view.id };
  } finally {
    opening.value = false;
  }
}

function show(id: number): void {
  unasked.delete(id);
  asked = id;
  chosenTerminal.value = { ...chosenTerminal.value, [props.project.path]: id };
}

/**
 * What Claude finished in a tab counts as seen once the tab is looked at: picked (a click, the
 * keyboard) or pressed in, the panel open. Every session of its project stops waiting on you, as
 * on a board picked by hand. Not while one asks something there (a question, a permission
 * prompt): seen, it would no longer say so.
 */
function seeFinished(id: number | undefined): void {
  if (id === undefined || collapsed.value) return;
  const rows = sessions.value?.sessions ?? [];
  const finished = rows.find((row) => row.origin.kind === "pitwall" && row.origin.terminal === id && row.phase === "waiting" && row.turn?.kind === "finished");
  if (!finished) return;
  const path = finished.path ?? finished.folder;
  const asks = rows.some((row) => (row.path ?? row.folder) === path && row.phase === "waiting" && !!row.turn && row.turn.kind !== "finished");
  if (!asks) void api.markSeen(path).catch(() => undefined);
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

/**
 * The tab asked for here, until it shows: clicked, picked, opened from "+" or brought up
 * (`terminalRequest`). On the Sessions page only that one takes the keyboard; a tab the page shows
 * for the session selected in its list comes up without it, so the list keeps its arrow keys.
 */
let asked: number | null = null;

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

/* ---------- "+": a new tab, or a Claude session ---------- */

/** How "New Claude" starts a session here when nothing is picked: the project's own settings. */
const defaults = computed(() => projectChoice(props.project));

const menu = useNativeMenu();

/** The "+" menu, under it: a shell tab, a Claude session as the project's settings say, and in
 * a submenu the models (the project's own checked) and plan mode, each starting a session at
 * once with that one change, the rest as the project's settings say. */
function addMenu(event: Event): void {
  if (opening.value) return;
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const { model, mode } = defaults.value;
  void menu.popup({ clientX: box.left, clientY: box.bottom + 4 }, [
    { text: t("terminal.newTab"), accelerator: isMac ? "Cmd+T" : "Ctrl+Shift+T", action: () => void openTerminal() },
    { text: t("terminal.newClaude"), action: () => void openTerminal(true) },
    "separator",
    {
      text: t("terminal.claudeWith"),
      items: [
        ...MODELS.map((each) => ({ text: t(MODEL_LABELS[each]), checked: model === each, action: () => void openTerminal(true, { model: each, mode }) })),
        "separator",
        { text: t("start.newClaudePlan"), checked: mode === "plan", action: () => void openTerminal(true, { model, mode: "plan" }) },
      ],
    },
  ]);
}

/** A double click on the tab row's empty part opens a Claude tab, as "New Claude" does. A tab's
 * own double click renames it, and the project's name and the buttons keep theirs. */
function newClaudeHere(event: MouseEvent): void {
  const target = event.target as HTMLElement;
  if (target === event.currentTarget || target === tablist.value || target.classList.contains("grow")) void openTerminal(true);
}


/** A tab asked for by a click, Return or Space: shown, and a collapsed panel comes up with it. The
 * tab already shown takes the keyboard here, the watch on the shown tab not running for it: left
 * on the tab's label, a Backspace would close the tab. */
function open(id: number): void {
  const shown = active.value?.id === id;
  show(id);
  if (collapsed.value) toggleCollapsed();
  seeFinished(id);
  if (shown) focusView(id);
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

/** Tabs dragged into another order keep it, per project, for as long as Pitwall runs. */
const tabDrag = useTabReorder(tablist, (ids) => {
  terminalOrder.value = { ...terminalOrder.value, [props.project.path]: ids };
});

/* A tab opens by growing to its width and closes by shrinking away, the gap beside it too; the
   tabs and the buttons after it follow it frame by frame. Not as the panel comes, nor for a
   project's tabs when another project is picked: those are there at once. */
const TAB_IN_MS = 260;
const TAB_OUT_MS = 200;
/** Even at both ends: a tab is seen growing from nothing, not appearing nearly whole. */
const EASE = "cubic-bezier(0.4, 0, 0.2, 1)";

function animateTab(tab: HTMLElement, opening: boolean, done: () => void): void {
  if (!settled.value || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return done();
  const style = getComputedStyle(tab);
  const row = tab.parentElement;
  const gap = row && row.children.length > 1 ? parseFloat(getComputedStyle(row).columnGap) || 0 : 0;
  const full = { width: `${tab.getBoundingClientRect().width}px`, minWidth: "0px", paddingLeft: style.paddingLeft, paddingRight: style.paddingRight, marginRight: "0px", opacity: 1 };
  const none = { width: "0px", minWidth: "0px", paddingLeft: "0px", paddingRight: "0px", marginRight: `${-gap}px`, opacity: 0 };
  // The row's room for one tab at least would show at once: it waits for its first tab.
  if (opening && row?.children.length === 1) row.style.minWidth = "0px";
  tab.style.overflow = "hidden";
  if (!opening) tab.style.pointerEvents = "none";
  const motion = tab.animate(opening ? [none, full] : [full, none], { duration: opening ? TAB_IN_MS : TAB_OUT_MS, easing: EASE, fill: opening ? "none" : "forwards" });
  const end = (): void => {
    tab.style.overflow = "";
    if (row) row.style.minWidth = "";
    done();
  };
  motion.onfinish = end;
  motion.oncancel = end;
}

function tabEnter(el: Element, done: () => void): void {
  animateTab(el as HTMLElement, true, done);
}

function tabLeave(el: Element, done: () => void): void {
  animateTab(el as HTMLElement, false, done);
}
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
  seeFinished(to);
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
    const wanted = id !== undefined && asked === id;
    asked = null;
    await nextTick();
    // A tab that came by itself is shown without the keyboard; on the Sessions page, so is any
    // tab not asked for here.
    const quiet = (id !== undefined && unasked.delete(id)) || (props.place === "sessions" && !wanted);
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
    if (!request || props.place !== "list") return;
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
/** How wide the panel is: room for the history beside the terminal or not. */
let panelObserver: ResizeObserver | null = null;
/** A row that narrows (the window, the panel) keeps the shown tab in sight too. */
let tabsObserver: ResizeObserver | null = null;
watch(tablist, (list, old) => {
  if (old) tabsObserver?.unobserve(old);
  if (list) tabsObserver?.observe(list);
});

onMounted(() => {
  observer = new ResizeObserver(() => {
    if (active.value && !collapsed.value && !animating.value) fitView(active.value.id, !sliding.value);
  });
  if (viewport.value) observer.observe(viewport.value);
  tabsObserver = new ResizeObserver(() => showTab(active.value?.id));
  if (tablist.value) tabsObserver.observe(tablist.value);
  const measure = (): void => {
    panelWidth.value = panel.value?.clientWidth ?? 0;
    narrow.value = panelWidth.value < HISTORY_ROOM;
  };
  panelObserver = new ResizeObserver(measure);
  if (panel.value) panelObserver.observe(panel.value);
  measure();
  requestAnimationFrame(() => requestAnimationFrame(() => (settled.value = true)));
});

// Wide again, the column is as it was; narrow again or collapsed, the history over the terminal
// waits for its button.
watch([narrow, collapsed], () => {
  overlayOpen.value = false;
});

onBeforeUnmount(() => {
  observer?.disconnect();
  tabsObserver?.disconnect();
  panelObserver?.disconnect();
  clearTimeout(settle);
  clearTimeout(slid);
});

defineExpose({ openTerminal, collapse });
</script>

<template>
  <section
    ref="panel"
    :class="['term-panel', { animated: !dragging, collapsed, over: place === 'track', paged: place === 'sessions' }]"
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
    <div class="tabs" @dblclick="newClaudeHere">
      <span class="where" :title="project.path">{{ project.name }}</span>
      <span id="terminal-tab-keys" class="sr-only">{{ t("terminal.tabKeys") }}</span>
      <!-- Each tab and its close button side by side, never one inside the other; the list holds
           tabs only, so the close button is the mouse's (the keyboard closes with Delete). One Tab
           stop: the shown tab; ←/→ move. Double-click (or F2) renames; a middle click closes; a drag
           moves it. -->
      <div ref="tablist" :class="['tablist', { some: terminals.length }, tabDrag.listClass()]" role="tablist" :aria-label="t('terminal.tabs')">
        <TransitionGroup :key="project.path" :css="false" @enter="tabEnter" @leave="tabLeave">
        <div
          v-for="term in terminals"
          :key="term.id"
          :class="['tab', { on: active?.id === term.id }, tabDrag.tabClass(term.id)]"
          :style="tabDrag.tabStyle(term.id)"
          :data-tab-id="term.id"
          role="none"
          :title="t('terminal.renameHint')"
          @pointerdown="tabDrag.down($event, term.id)"
          @click="open(term.id)"
          @dblclick="renaming = { id: term.id, name: term.name }"
          @mousedown.middle.prevent
          @mouseup.middle="close(term.id)"
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
            data-no-drag
            :title="t('common.close')"
            @click.stop="close(term.id)"
            @dblclick.stop
          >×</button>
        </div>
        </TransitionGroup>
      </div>
      <!-- The project, its tabs and right after them the buttons that open one; at the right end,
           the history and collapse. -->
      <span :class="['divider', { shown: terminals.length }]" aria-hidden="true"></span>
      <!-- A shell tab, a Claude session, or one with a model or mode of its own: a menu under it. -->
      <button
        type="button"
        class="icon add"
        aria-haspopup="menu"
        aria-keyshortcuts="ArrowDown"
        :aria-label="t('terminal.add')"
        :title="t('terminal.add')"
        :disabled="opening"
        @click="addMenu"
        @contextmenu.prevent="addMenu"
        @keydown.down.prevent="addMenu"
      >
        <Icon name="plus" :size="13" />
      </button>
      <span class="grow"></span>
      <button
        type="button"
        :class="['action', 'history-toggle', { on: historyOpen }]"
        :aria-pressed="historyOpen"
        :title="t('terminal.historyTitle')"
        @click="toggleHistory"
      >
        <Icon name="clock" :size="13" /> <span class="action-text">{{ t("terminal.history") }}</span>
      </button>
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
    <div class="body" :style="{ '--history-width': `${historyShown}px` }">
      <div class="screen">
        <div ref="viewport" class="viewport" @pointerdown="pressScreen"></div>
        <div v-if="!active" class="empty" @pointerdown="overlayOpen = false">
          <span>{{ t("terminal.none", { name: project.name }) }}</span>
        </div>
      </div>
      <!-- The history's room beside the terminal: the panel's right margin alone while it is
           hidden; shown, it grows to the left and the terminal gives up that width as it goes. -->
      <div :class="['side', { open: beside, resizing, still: !settled }]" :style="beside ? { width: `${historyShown + 32}px` } : undefined">
        <Transition name="fade">
          <div
            v-if="beside"
            class="width-grip"
            role="separator"
            aria-orientation="vertical"
            tabindex="0"
            :aria-label="t('terminal.historyResize')"
            :aria-valuenow="historyShown"
            @pointerdown="startWidth"
            @pointermove="moveWidth"
            @pointerup="endWidth"
            @pointercancel="endWidth"
            @keydown="nudgeWidth"
            @dblclick="resetWidth"
          ></div>
        </Transition>
        <Transition name="fade">
          <TerminalHistory v-if="beside" ref="side" :project="project" @leave="backToTerminal" />
        </Transition>
      </div>
      <Transition name="pop">
        <TerminalHistory v-if="narrow && overlayOpen" ref="over" :project="project" overlay @close="closeOverlay" @leave="backToTerminal" />
      </Transition>
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
  box-shadow: var(--shadow-rise);
}

/* The Sessions page's: up to its filters, under its bar. Not `.sessions`: that page's own root
   class, whose scoped rule reaches this root too, would make it grow. */
.term-panel.paged {
  max-height: calc(100% - 100px);
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
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow: hidden;
}

.tablist.some {
  min-width: 56px;
}

/* Drag to reorder (lib/tabReorder): the tab lifts and follows the pointer, the others slide
   aside, and on release it slides into place. */
.tablist.reordering .tab {
  transition: translate 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.tablist.reordering .tab:hover:not(.on, .dragging) {
  color: var(--text-faint);
}

.tab.dragging {
  position: relative;
  z-index: 2;
  background: var(--bg-lit);
  color: var(--text-strong);
  transition: none;
}

.tablist.reordering .tab.dragging.settling {
  transition: translate 160ms ease-out;
}

.tablist.frozen .tab {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .tablist.reordering .tab,
  .tablist.reordering .tab.dragging.settling {
    transition: none;
  }
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

/* A button with its label: "History". */
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

/* "History" keeps its mark whatever the tabs take ("New tab" and "New Claude" are marks alone). */
.action.history-toggle {
  flex-shrink: 0;
}

/* "History" while its list shows. */
.action.on,
.action.on:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* A narrow row: "History" is its clock alone (still its tooltip, and read out). */
@container (max-width: 455px) {
  .action.history-toggle {
    flex-shrink: 0;
    width: 24px;
    padding: 0;
    justify-content: center;
  }

  .history-toggle .action-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

/* The project, first in the row, set off from its tabs; its name is cut only once they have no
   more room to give. */
.where {
  flex-shrink: 1;
  min-width: 48px;
  max-width: 180px;
  margin-right: 6px;
  padding: 0 10px 0 6px;
  border-right: 1px solid var(--line);
  font-size: 12px;
  font-weight: 600;
  line-height: 14px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The tabs set off from the buttons after them, as the project is from its tabs (its right
   border); with no tabs there is nothing to set off, and it comes and goes with the first. */
.divider {
  flex-shrink: 0;
  width: 0;
  height: 14px;
  margin: 0 -2px 0 0;
  background: var(--line);
  opacity: 0;
  transition:
    width 260ms cubic-bezier(0.4, 0, 0.2, 1),
    margin 260ms cubic-bezier(0.4, 0, 0.2, 1),
    opacity 200ms ease;
}

.divider.shown {
  width: 1px;
  margin: 0 6px 0 8px;
  opacity: 1;
}

@media (prefers-reduced-motion: reduce) {
  .divider {
    transition: none;
  }
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

/* The terminal, and the history beside it or over it. */
.body {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
}

.screen {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  margin: 6px 0 20px 20px;
  padding: 8px 4px 8px 10px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid var(--line-inset);
  display: flex;
  overflow: clip;
  transition: opacity 160ms ease;
}

/* Opening, the room comes first and the list fades in once it is there; closing, the list fades
   first and then the room goes. */
.side {
  position: relative;
  flex-shrink: 0;
  width: 20px;
  overflow: hidden;
  transition:
    width 220ms cubic-bezier(0.2, 0.8, 0.2, 1) 120ms,
    opacity 160ms ease;
}

/* Open, the gap, the history (`historyShown`) and the panel's right margin: set on the element. */
.side.open {
  transition-delay: 0ms;
}

/* A drag follows the pointer; coming to the page, an open history is there without moving. */
.side.resizing,
.side.still,
.side.still .fade-enter-active,
.side.still .fade-leave-active {
  transition: none;
}

/* The history's handle: the gap beside the terminal, with a bar like the panel's handle. */
.width-grip {
  position: absolute;
  top: 6px;
  bottom: 20px;
  left: 0;
  width: 12px;
  cursor: col-resize;
  outline: none;
}

.width-grip::after {
  content: "";
  position: absolute;
  top: 50%;
  left: 4px;
  width: 3px;
  height: 36px;
  margin-top: -18px;
  border-radius: 2px;
  background: var(--line-dialog);
}

.width-grip:hover::after,
.side.resizing .width-grip::after {
  background: var(--line-heavy);
}

.width-grip:focus-visible::after {
  background: var(--focus-ring);
}

.fade-enter-active {
  transition: opacity 160ms ease 220ms;
}

.fade-leave-active {
  transition: opacity 120ms ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

/* A narrow panel's history comes in over the terminal from its right edge. */
.pop-enter-active,
.pop-leave-active {
  transition:
    opacity 160ms ease,
    transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateX(12px);
}

/* Collapsed, the tab row alone: nothing of the terminal or the history shows under it. */
.term-panel.collapsed .screen,
.term-panel.collapsed .side,
.term-panel.collapsed .history {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .term-panel.animated,
  .screen,
  .side,
  .fade-enter-active,
  .fade-leave-active,
  .pop-enter-active,
  .pop-leave-active,
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
