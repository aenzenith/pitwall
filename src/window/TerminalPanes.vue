<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import { t } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import {
  canAddPane,
  canSplit,
  evenBorder,
  kindOf,
  leaves,
  MAX_PANES,
  moveBorder,
  movePane,
  PANE_ACCELERATORS,
  PANE_KEYS,
  place,
  setPanes,
  shownIn,
  swapPanes,
  tellPanes,
  type Gutter,
  type PaneLayout,
  type Place,
  type PlacedPane,
  type Rect,
  type Side,
  type SplitDir,
} from "../lib/panes";
import { SHELL_LABELS, shellIcon } from "../lib/shells";
import { MODEL_LABELS, MODELS, projectChoice } from "../lib/start";
import { attach, fitView } from "../lib/terminals";
import type { Project, Shell, ShellChoice, StartChoice, TerminalView } from "../lib/types";

/**
 * The terminal panel's panes (lib/panes): each with a head (the terminal's name, Claude's state,
 * split right, split down, zoom, close) over its terminal, or empty with buttons to open one. The
 * borders between them drag (a border moves only the panes on either side of it, never below
 * their least), a pane is moved by dragging its head onto another, and a double click on a border
 * shares its room out equally.
 */
const props = defineProps<{
  project: Project;
  layout: PaneLayout;
  /** The project's terminals, in their tabs' order. */
  terminals: TerminalView[];
  live: Map<number, { phase: "waiting" | "working"; text: string }>;
  /** The panel is collapsed or still sliding: nothing is sized meanwhile. */
  hold: boolean;
  /** The history's room is moving: the views follow it, the shells hear the size it ends at. */
  quiet: boolean;
  opening: boolean;
  /** Where a session dragged in from the history, or a tab from the row, would go (TerminalPanel
   * works it out); `incomingMoves`: a tab, which moves there rather than opens. */
  incoming?: { key: string; side: Side | null } | null;
  incomingMoves?: boolean;
  /** Whether a point is over the panel's tab row: a pane dragged there goes back to being a tab. */
  rowAt?: (x: number, y: number) => boolean;
  /** The names the panes show (lib/tabs: `distinctNames`); a terminal's own without. */
  names?: Map<number, string>;
  /** The shells installed, on Windows (a split's menu offers them); none elsewhere. */
  shells?: Shell[];
}>();

const emit = defineEmits<{
  /** A new terminal, put there; `shell`, on Windows the shell of the list picked. */
  open: [place: Place, claude: boolean, start: StartChoice | null, shell: ShellChoice | null];
  close: [id: number];
  rename: [id: number];
  /** A pane picked: `keyboard`, the keyboard goes into its terminal. */
  focus: [key: string, keyboard: boolean];
  /** A pane taken out of the panes (its terminal stays open, waiting). */
  remove: [key: string];
  /** A pane dragged by its head over the tab row (`x`: where along it), or off it again. */
  overRow: [x: number | null];
  /** A pane let go on the tab row: its terminal goes back to being a tab there. */
  toRow: [key: string, x: number];
  /** The room the panes have. */
  room: [w: number, h: number];
}>();

const stage = ref<HTMLElement | null>(null);
const width = ref(0);
const height = ref(0);

/** The tree as the room lets it show (lib/panes: `shownIn`): too small for it, its panes dealt out
 * in fewer columns, or the focused one filling the room. */
const shown = computed(() => shownIn(props.layout.tree, width.value, height.value));
const placed = computed(() => place(shown.value.tree, width.value, height.value));
/** The pane that fills the room: the one zoomed, or squeezed, the one with the keyboard. */
const zoom = computed(() => {
  const keys = leaves(props.layout.tree).map((pane) => pane.key);
  if (props.layout.zoom && keys.includes(props.layout.zoom)) return props.layout.zoom;
  return shown.value.mode === "squeezed" && keys.includes(props.layout.focus) ? props.layout.focus : null;
});
/** Splits, moves to a side and borders only on the tree as it is: dealt out or squeezed, what the
 * panes look like isn't what they are. */
const asIs = computed(() => shown.value.mode === "as-is" && !zoom.value);

function nameOf(id: number): string {
  return props.names?.get(id) ?? views.value.get(id)?.name ?? "";
}
const views = computed(() => new Map(props.terminals.map((term) => [term.id, term])));

function box(rect: Rect): Record<string, string> {
  return { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.w}px`, height: `${rect.h}px` };
}

/** A zoomed pane fills the room. The others are taken out of the page meanwhile (not merely
 * hidden): xterm stops drawing a terminal it can't see, and draws it whole again once it can. */
function paneStyle(pane: PlacedPane): Record<string, string> {
  if (!zoom.value) return box(pane);
  return pane.leaf.key === zoom.value ? box({ x: 0, y: 0, w: width.value, h: height.value }) : { display: "none" };
}

/** A pane out of sight while another fills the room: never sized meanwhile. */
function outOfSight(key: string): boolean {
  return !!zoom.value && zoom.value !== key;
}

const defaults = computed(() => projectChoice(props.project));

/* ---------- the terminals in their panes ---------- */

const hosts = new Map<string, HTMLElement>();
/** The terminal last put into each pane's host. */
const attached = new Map<string, number>();
let hostObserver: ResizeObserver | null = null;
/** A border is being dragged (`start`: where it was as the pointer took it at `from`): the views
 * follow it, the shells hear the size it ends at. */
const bordering = ref<{ split: string; at: number; dir: SplitDir; from: number; start: number } | null>(null);

function hostRef(key: string, el: Element | null): void {
  const old = hosts.get(key);
  if (old === el) return;
  if (old) hostObserver?.unobserve(old);
  if (el instanceof HTMLElement) {
    hosts.set(key, el);
    hostObserver?.observe(el);
    void nextTick(attachAll);
  } else {
    hosts.delete(key);
    attached.delete(key);
  }
}

function attachAll(): void {
  for (const pane of leaves(props.layout.tree)) {
    const host = hosts.get(pane.key);
    if (pane.terminal === null || !host) continue;
    if (attached.get(pane.key) === pane.terminal && host.firstElementChild) continue;
    attach(pane.terminal, host, !props.hold, false);
    attached.set(pane.key, pane.terminal);
  }
}

function fitAll(tell: boolean): void {
  if (props.hold) return;
  for (const pane of leaves(props.layout.tree)) if (pane.terminal !== null && !outOfSight(pane.key)) fitView(pane.terminal, tell);
}

watch(
  () => leaves(props.layout.tree).map((pane) => `${pane.key}:${pane.terminal}`).join(),
  () => void nextTick(attachAll),
);
watch(
  () => props.hold,
  (held) => {
    if (!held) void nextTick(() => fitAll(true));
  },
);
watch(
  () => props.quiet,
  (moving) => {
    if (!moving) fitAll(true);
  },
);
// Zoomed or back: the pane's size changed.
watch(zoom, () => void nextTick(() => fitAll(true)));

/* ---------- picking a pane ---------- */

function press(key: string, event: PointerEvent): void {
  if (event.button !== 0 && event.button !== 2) return;
  const head = (event.target as HTMLElement).closest(".head");
  const onButton = (event.target as HTMLElement).closest("button");
  if (key !== props.layout.focus || (head && !onButton)) emit("focus", key, !!head && !onButton);
}

function focusIn(key: string): void {
  if (key !== props.layout.focus) emit("focus", key, false);
}

/* ---------- a pane's buttons and menus ---------- */

const menu = useNativeMenu();

function rectOf(key: string): PlacedPane | undefined {
  return placed.value.panes.find((pane) => pane.leaf.key === key);
}

/** The pane can be split that way, with a shell beside it (`claude`: a Claude session), and the
 * panes may take one more. */
function splits(key: string, dir: SplitDir, claude = false): boolean {
  const rect = rectOf(key);
  return asIs.value && !!rect && canAddPane(props.layout.tree) && canSplit(rect, dir, [kindOf(rect.leaf), claude ? "claude" : "shell"]);
}

function splitTitle(key: string, dir: SplitDir): string {
  if (!canAddPane(props.layout.tree)) return t("panes.tooMany", { count: MAX_PANES });
  if (!splits(key, dir)) return t(dir === "row" ? "panes.tooNarrow" : "panes.tooShort");
  return `${t(dir === "row" ? "panes.splitRight" : "panes.splitDown")} (${dir === "row" ? PANE_KEYS.right : PANE_KEYS.down})`;
}

function split(key: string, dir: SplitDir, claude = false, start: StartChoice | null = null, shell: ShellChoice | null = null): void {
  if (!splits(key, dir, claude) || props.opening) return;
  emit("open", { kind: "split", key, dir }, claude, start, shell);
}

/** A split button's other choices, on its right-click as on "+": a shell (on Windows each shell
 * installed, Command Prompt the one the button opens), Claude as the project's settings say, or
 * Claude with a model or plan mode of its own. */
function splitMenu(event: MouseEvent, key: string, dir: SplitDir): void {
  if (!splits(key, dir)) return;
  const { model, mode } = defaults.value;
  // A Claude session needs more room than a shell: with too little for it, its lines are off.
  const claude = splits(key, dir, true);
  const accelerator = dir === "row" ? PANE_ACCELERATORS.right : PANE_ACCELERATORS.down;
  const shellLines: MenuEntry[] = props.shells?.length
    ? [...props.shells.map((shell) => ({ text: t(SHELL_LABELS[shell]), accelerator: shell === "cmd" ? accelerator : undefined, action: () => split(key, dir, false, null, { shell, elevated: false }) })), "separator"]
    : [{ text: t("panes.shell"), accelerator, action: () => split(key, dir) }];
  void menu.popup(event, [
    ...shellLines,
    { text: t("panes.claude"), enabled: claude, action: () => split(key, dir, true) },
    "separator",
    {
      text: t("terminal.claudeWith"),
      enabled: claude,
      items: [
        ...MODELS.map((each) => ({ text: t(MODEL_LABELS[each]), checked: model === each, action: () => split(key, dir, true, { model: each, mode }) })),
        "separator",
        { text: t("start.newClaudePlan"), checked: mode === "plan", action: () => split(key, dir, true, { model, mode: "plan" }) },
      ],
    },
  ]);
}

function toggleZoom(key: string): void {
  setPanes(props.project.path, { ...props.layout, focus: key, zoom: zoom.value === key ? null : key });
  emit("focus", key, true);
}

/** A pane's head, right-clicked: split, zoom, take it out of the panes, close its terminal. */
function paneMenu(event: MouseEvent, key: string, id: number): void {
  const zoomed = zoom.value === key;
  const entries: MenuEntry[] = [
    { text: t("panes.splitRight"), accelerator: PANE_ACCELERATORS.right, enabled: splits(key, "row"), action: () => split(key, "row") },
    { text: t("panes.splitDown"), accelerator: PANE_ACCELERATORS.down, enabled: splits(key, "col"), action: () => split(key, "col") },
    "separator",
    { text: t(zoomed ? "panes.unzoom" : "panes.zoom"), accelerator: PANE_ACCELERATORS.zoom, action: () => toggleZoom(key) },
    { text: t("panes.detach"), action: () => emit("remove", key) },
    "separator",
    { text: t("common.close"), accelerator: PANE_ACCELERATORS.close, action: () => emit("close", id) },
  ];
  void menu.popup(event, entries);
}

/* ---------- the borders ---------- */

/** One column, or one row, of the terminal's type: what an arrow key moves a border by. */
const STEP = { row: 7.2, col: 18 };

function startBorder(event: PointerEvent, gutter: Gutter): void {
  if (event.button !== 0) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  bordering.value = { split: gutter.split, at: gutter.at, dir: gutter.dir, from: gutter.dir === "row" ? event.clientX : event.clientY, start: gutter.dir === "row" ? gutter.x : gutter.y };
}

/** The border goes where the pointer takes it, in the panes as they are now: a pane that went
 * meanwhile (its terminal closed) stays gone. Past its panes' least, it waits for the pointer. */
function dragBorder(event: PointerEvent): void {
  const drag = bordering.value;
  if (!drag) return;
  const gutter = placed.value.gutters.find((each) => each.split === drag.split && each.at === drag.at);
  if (!gutter) return;
  const wanted = drag.start + (drag.dir === "row" ? event.clientX : event.clientY) - drag.from;
  const tree = moveBorder(props.layout.tree, drag.split, drag.at, wanted - (drag.dir === "row" ? gutter.x : gutter.y), placed.value);
  if (tree) setPanes(props.project.path, { ...props.layout, tree }, false);
}

function endBorder(): void {
  if (!bordering.value) return;
  bordering.value = null;
  tellPanes(props.project.path);
  fitAll(true);
}

function nudgeBorder(event: KeyboardEvent, gutter: Gutter): void {
  const keys: Record<string, number> = gutter.dir === "row" ? { ArrowLeft: -1, ArrowRight: 1 } : { ArrowUp: -1, ArrowDown: 1 };
  const way = keys[event.key];
  if (!way) return;
  event.preventDefault();
  const tree = moveBorder(props.layout.tree, gutter.split, gutter.at, way * STEP[gutter.dir] * (event.shiftKey ? 10 : 1), placed.value);
  if (tree) setPanes(props.project.path, { ...props.layout, tree });
}

function evenOut(gutter: Gutter): void {
  setPanes(props.project.path, { ...props.layout, tree: evenBorder(props.layout.tree, gutter.split, gutter.at) });
}

/* ---------- moving a pane by its head ---------- */

type Drop = { key: string; side: Side | null };
const moving = ref<{ key: string; name: string; x: number; y: number; started: boolean; drop: Drop | null; row: boolean } | null>(null);
let moveStart: { x: number; y: number } | null = null;
/** How far the pointer goes before a press on a head becomes a drag. */
const DRAG_FROM = 5;

function startMove(event: PointerEvent, key: string, name: string): void {
  if (event.button !== 0 || (event.target as HTMLElement).closest("button") || zoom.value) return;
  moveStart = { x: event.clientX, y: event.clientY };
  moving.value = { key, name, x: event.clientX, y: event.clientY, started: false, drop: null, row: false };
  window.addEventListener("pointermove", dragMove);
  window.addEventListener("pointerup", endMove);
  window.addEventListener("pointercancel", cancelMove);
  window.addEventListener("keydown", escMove, true);
}

/** Where a pane let go here would go: into the middle of another, they change places; onto its
 * edge, beside it there, when that pane has the room to be split that way. */
function dropAt(x: number, y: number, from: string): Drop | null {
  const room = stage.value?.getBoundingClientRect();
  if (!room) return null;
  const px = x - room.left;
  const py = y - room.top;
  const target = placed.value.panes.find((pane) => pane.leaf.key !== from && px >= pane.x && px < pane.x + pane.w && py >= pane.y && py < pane.y + pane.h);
  if (!target) return null;
  if (target.leaf.terminal === null || !asIs.value) return { key: target.leaf.key, side: null };
  const rx = (px - target.x) / target.w;
  const ry = (py - target.y) / target.h;
  if (rx > 0.3 && rx < 0.7 && ry > 0.3 && ry < 0.7) return { key: target.leaf.key, side: null };
  const near: [Side, number][] = [
    ["left", rx],
    ["right", 1 - rx],
    ["top", ry],
    ["bottom", 1 - ry],
  ];
  const side = near.sort((a, b) => a[1] - b[1])[0][0];
  return { key: target.leaf.key, side: canSplit(target, side === "left" || side === "right" ? "row" : "col") ? side : null };
}

function dragMove(event: PointerEvent): void {
  const drag = moving.value;
  if (!drag || !moveStart) return;
  if (!drag.started && Math.hypot(event.clientX - moveStart.x, event.clientY - moveStart.y) < DRAG_FROM) return;
  if (!drag.started) document.body.classList.add("grabbing");
  // Over the tab row it goes back to being a tab; elsewhere, beside or in another pane.
  const row = !!props.rowAt?.(event.clientX, event.clientY);
  moving.value = { ...drag, started: true, x: event.clientX, y: event.clientY, drop: row ? null : dropAt(event.clientX, event.clientY, drag.key), row };
  if (row || drag.row) emit("overRow", row ? event.clientX : null);
}

function endMove(): void {
  const drag = moving.value;
  stopMove();
  if (drag?.started && drag.row) return emit("toRow", drag.key, drag.x);
  if (!drag?.started || !drag.drop) return;
  const { key, side } = drag.drop;
  const tree = side ? movePane(props.layout.tree, drag.key, key, side) : swapPanes(props.layout.tree, drag.key, key);
  setPanes(props.project.path, { ...props.layout, tree, focus: drag.key });
  void nextTick(() => fitAll(true));
}

function cancelMove(): void {
  stopMove();
}

function escMove(event: KeyboardEvent): void {
  if (event.key !== "Escape" || !moving.value?.started) return;
  event.preventDefault();
  event.stopPropagation();
  stopMove();
}

function stopMove(): void {
  if (moving.value?.row) emit("overRow", null);
  moving.value = null;
  moveStart = null;
  document.body.classList.remove("grabbing");
  window.removeEventListener("pointermove", dragMove);
  window.removeEventListener("pointerup", endMove);
  window.removeEventListener("pointercancel", cancelMove);
  window.removeEventListener("keydown", escMove, true);
}

/** The pane being moved holds a Claude session: its ghost says so. */
const movingClaude = computed(() => {
  const key = moving.value?.key;
  const terminal = key ? leaves(props.layout.tree).find((pane) => pane.key === key)?.terminal : null;
  return terminal != null && views.value.get(terminal)?.kind === "claude";
});

const DROP_LABELS: Record<Side, "panes.moveLeft" | "panes.moveRight" | "panes.moveUp" | "panes.moveDown"> = { left: "panes.moveLeft", right: "panes.moveRight", top: "panes.moveUp", bottom: "panes.moveDown" };
const OPEN_LABELS: Record<Side, "panes.openLeft" | "panes.openRight" | "panes.openAbove" | "panes.openBelow"> = { left: "panes.openLeft", right: "panes.openRight", top: "panes.openAbove", bottom: "panes.openBelow" };

/** Where something let go would go, and what that does: a pane moved by its head, or a session
 * dragged in from the history (it opens there). */
const dropping = computed<{ key: string; side: Side | null; label: string } | null>(() => {
  const move = moving.value?.started ? moving.value.drop : null;
  if (move) return { ...move, label: t(move.side ? DROP_LABELS[move.side] : "panes.swap") };
  const come = props.incoming;
  if (come) return { ...come, label: t(come.side ? (props.incomingMoves ? DROP_LABELS : OPEN_LABELS)[come.side] : "panes.openHere") };
  return null;
});

/* ---------- the room ---------- */

let stageObserver: ResizeObserver | null = null;

onMounted(() => {
  hostObserver = new ResizeObserver((entries) => {
    if (props.hold) return;
    for (const entry of entries) {
      const key = (entry.target as HTMLElement).dataset.pane;
      const pane = key ? leaves(props.layout.tree).find((each) => each.key === key) : undefined;
      // Taken out of the page (another fills the room): nothing to size it to.
      if (!entry.contentRect.width || !entry.contentRect.height) continue;
      if (pane?.terminal != null) fitView(pane.terminal, !bordering.value && !props.quiet && !moving.value?.started);
    }
  });
  for (const host of hosts.values()) hostObserver.observe(host);
  stageObserver = new ResizeObserver(() => {
    width.value = stage.value?.clientWidth ?? 0;
    height.value = stage.value?.clientHeight ?? 0;
    emit("room", width.value, height.value);
  });
  if (stage.value) stageObserver.observe(stage.value);
  attachAll();
});

onBeforeUnmount(() => {
  hostObserver?.disconnect();
  stageObserver?.disconnect();
  stopMove();
});

/** For the panel: the panes' places as shown, how the tree shows, a pane's room, and where
 * something let go at a point would go. */
defineExpose({ placed, shown, rectOf, dropAt });
</script>

<template>
  <div ref="stage" :class="['panes', { bordering: !!bordering }]">
    <div
      v-for="pane in placed.panes"
      :key="pane.leaf.key"
      :class="['pane', { on: pane.leaf.key === layout.focus, empty: pane.leaf.terminal === null, elevated: pane.leaf.terminal !== null && views.get(pane.leaf.terminal)?.elevated, lifted: moving?.started && moving.key === pane.leaf.key, targeted: dropping?.key === pane.leaf.key }]"
      :style="paneStyle(pane)"
      @pointerdown.capture="press(pane.leaf.key, $event)"
      @focusin="focusIn(pane.leaf.key)"
    >
      <template v-if="pane.leaf.terminal !== null">
        <div
          class="head"
          :title="t('terminal.renameHint')"
          @pointerdown="startMove($event, pane.leaf.key, nameOf(pane.leaf.terminal))"
          @dblclick="emit('rename', pane.leaf.terminal)"
          @contextmenu.prevent="paneMenu($event, pane.leaf.key, pane.leaf.terminal)"
        >
          <ClaudeLogo v-if="views.get(pane.leaf.terminal)?.kind === 'claude'" :size="11" />
          <Icon v-else :name="shellIcon(views.get(pane.leaf.terminal)?.shell)" :size="11" />
          <span class="name">{{ nameOf(pane.leaf.terminal) }}</span>
          <span v-if="views.get(pane.leaf.terminal)?.elevated" class="admin"><Icon name="shield" :size="9" />{{ t("terminal.adminBadge") }}</span>
          <ClaudeDot v-if="live.has(pane.leaf.terminal)" :live="live.get(pane.leaf.terminal)" />
          <span v-if="live.get(pane.leaf.terminal)?.phase === 'waiting'" class="wait">{{ live.get(pane.leaf.terminal)?.text }}</span>
          <span class="grow"></span>
          <span class="tools">
            <button
              type="button"
              class="tool"
              :class="{ off: !splits(pane.leaf.key, 'row') }"
              :aria-disabled="!splits(pane.leaf.key, 'row') || opening"
              :aria-label="t('panes.splitRight')"
              :title="splitTitle(pane.leaf.key, 'row')"
              @click="split(pane.leaf.key, 'row')"
              @contextmenu.prevent.stop="splitMenu($event, pane.leaf.key, 'row')"
            >
              <Icon name="split-right" :size="12" />
            </button>
            <button
              type="button"
              class="tool"
              :class="{ off: !splits(pane.leaf.key, 'col') }"
              :aria-disabled="!splits(pane.leaf.key, 'col') || opening"
              :aria-label="t('panes.splitDown')"
              :title="splitTitle(pane.leaf.key, 'col')"
              @click="split(pane.leaf.key, 'col')"
              @contextmenu.prevent.stop="splitMenu($event, pane.leaf.key, 'col')"
            >
              <Icon name="split-down" :size="12" />
            </button>
            <button
              type="button"
              class="tool"
              :aria-pressed="zoom === pane.leaf.key"
              :aria-label="t(zoom === pane.leaf.key ? 'panes.unzoom' : 'panes.zoom')"
              :title="`${t(zoom === pane.leaf.key ? 'panes.unzoom' : 'panes.zoom')} (${PANE_KEYS.zoom})`"
              @click="toggleZoom(pane.leaf.key)"
            >
              <Icon :name="zoom === pane.leaf.key ? 'minimize' : 'maximize'" :size="12" />
            </button>
          </span>
          <button type="button" class="x" tabindex="-1" :aria-label="t('common.close')" :title="t('common.close')" @click.stop="emit('close', pane.leaf.terminal)" @dblclick.stop>×</button>
        </div>
        <div :ref="(el) => hostRef(pane.leaf.key, el as Element | null)" class="host" :data-pane="pane.leaf.key"></div>
      </template>
      <div v-else class="cell" :aria-label="t('panes.empty')" role="group">
        <button type="button" class="control" :disabled="opening" @click="emit('open', { kind: 'cell', key: pane.leaf.key }, false, null, null)">
          <Icon name="terminal" :size="12" /> {{ t("terminal.newTab") }}
        </button>
        <button type="button" class="control" :disabled="opening" @click="emit('open', { kind: 'cell', key: pane.leaf.key }, true, null, null)">
          <ClaudeLogo :size="12" /> {{ t("terminal.newClaude") }}
        </button>
      </div>
      <div v-if="dropping?.key === pane.leaf.key" :class="['drop', dropping.side ?? 'swap']">
        <span>{{ dropping.label }}</span>
      </div>
    </div>
    <template v-if="asIs">
      <div
        v-for="gutter in placed.gutters"
        :key="`${gutter.split}:${gutter.at}`"
        :class="['gutter', gutter.dir, { held: bordering?.split === gutter.split && bordering.at === gutter.at }]"
        :style="box(gutter)"
        role="separator"
        tabindex="0"
        :aria-orientation="gutter.dir === 'row' ? 'vertical' : 'horizontal'"
        :aria-label="t('panes.border')"
        @pointerdown="startBorder($event, gutter)"
        @pointermove="dragBorder"
        @pointerup="endBorder"
        @pointercancel="endBorder"
        @keydown="nudgeBorder($event, gutter)"
        @dblclick="evenOut(gutter)"
      ></div>
    </template>
    <div v-if="moving?.started" class="ghost" :style="{ left: `${moving.x + 12}px`, top: `${moving.y + 10}px` }" aria-hidden="true">
      <ClaudeLogo v-if="movingClaude" :size="11" />
      <Icon v-else name="terminal" :size="11" />
      {{ moving.name }}
    </div>
  </div>
</template>

<style scoped>
.panes {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
}

.pane {
  position: absolute;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  border-radius: 8px;
  border: 1px solid var(--line-inset);
  background: var(--bg-log);
  overflow: clip;
}

.pane.on {
  border-color: var(--line-heavy);
}

/* An administrator's shell: its border and its head's mark in the warning's colours, apart from
   the ordinary shells beside it. */
.pane.elevated {
  border-color: var(--warn-line);
}

.pane.elevated.on {
  border-color: color-mix(in srgb, var(--warn) 55%, var(--warn-line));
}

.admin {
  flex-shrink: 0;
  height: 15px;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 0 5px;
  border-radius: 4px;
  border: 1px solid var(--warn-line);
  background: var(--warn-bg);
  font-size: 10.5px;
  font-weight: 500;
  color: var(--warn-text);
}

.admin :deep(svg) {
  color: var(--warn);
}

.pane.lifted {
  opacity: 0.45;
}

/* The head: on a ground of its own, off the terminal's, so stacked terminals never run together. */
.head {
  container-type: inline-size;
  flex-shrink: 0;
  height: 22px;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 3px 0 9px;
  border-bottom: 1px solid var(--line-soft);
  background: var(--bg-pane-head);
  font-size: 12px;
  color: var(--text-faint);
  cursor: grab;
}

body.grabbing .head {
  cursor: grabbing;
}

.pane.on .head {
  color: var(--text-muted);
}

.name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-faint);
}

.pane.on .name {
  color: var(--text-lit);
}

/* What Claude waits for (a permission's tool): only while the head has room for it. */
.wait {
  flex-shrink: 0;
  font-size: 11.5px;
  color: var(--claude-text);
  white-space: nowrap;
}

@container (max-width: 380px) {
  .wait {
    display: none;
  }
}

.grow {
  flex-grow: 1;
}

/* Split and zoom: on the pane with the keyboard, and on any other the pointer is over. */
.tools {
  display: flex;
  gap: 6px;
  opacity: 0;
}

.pane.on .tools,
.pane:hover .tools,
.tools:focus-within {
  opacity: 1;
}

.tool {
  width: 20px;
  height: 20px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text-subtle);
}

.tool:hover:not(.off) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Off, but still under the pointer: its tooltip says why (a disabled button shows none in WebKit). */
.tool.off {
  opacity: 0.35;
  cursor: default;
}

.x {
  width: 18px;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: center;
  font-size: 13px;
  line-height: 1;
  border-radius: 4px;
  color: var(--line-heavy);
}

.pane.on .x {
  color: var(--text-faint);
}

.x:hover {
  color: var(--text-strong);
  background: var(--bg-icon-hover);
}

.host {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  margin: 6px 4px 6px 10px;
  overflow: clip;
}

.host :deep(.terminal-host),
.host :deep(.xterm) {
  height: 100%;
}

.host :deep(.xterm-viewport) {
  background-color: var(--bg-log) !important;
}

/* An empty pane: what can go into it. */
.pane.empty {
  border-style: dashed;
  border-color: var(--line-dialog);
  background: transparent;
}

.pane.empty.on {
  border-color: var(--line-heavy);
}

.cell {
  flex: 1 1 auto;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  align-content: center;
  gap: 6px;
  padding: 8px;
}

/* As the empty terminal's buttons. */
.control {
  height: 28px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 0 11px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12.5px;
  color: var(--text-soft);
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control:disabled {
  opacity: 0.4;
  cursor: default;
}

/* Where a pane being dragged would go. */
.drop {
  position: absolute;
  inset: 26px 4px 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 7px;
  border: 1.5px dashed var(--line-heavy);
  background: color-mix(in srgb, var(--text-muted) 10%, transparent);
  pointer-events: none;
}

.drop.left {
  right: 50%;
}

.drop.right {
  left: 50%;
}

.drop.top {
  bottom: 50%;
}

.drop.bottom {
  top: calc(50% + 11px);
}

.pane.empty .drop {
  inset: 4px;
}

/* An empty pane a pane is about to go into: what could open in it gives way to where it goes. */
.pane.empty.targeted .cell {
  visibility: hidden;
}

.drop span {
  padding: 3px 9px;
  border-radius: 6px;
  background: var(--bg-lit);
  font-size: 12px;
  color: var(--text-strong);
  white-space: nowrap;
}

/* A border: the room between two panes, its handle showing while the pointer is over it. */
.gutter {
  position: absolute;
  outline: none;
  z-index: 1;
}

.gutter.row {
  cursor: col-resize;
}

.gutter.col {
  cursor: row-resize;
}

.gutter::after {
  content: "";
  position: absolute;
  border-radius: 2px;
  background: transparent;
}

.gutter.row::after {
  left: 1.5px;
  top: 50%;
  width: 3px;
  height: 36px;
  margin-top: -18px;
}

.gutter.col::after {
  top: 1.5px;
  left: 50%;
  width: 36px;
  height: 3px;
  margin-left: -18px;
}

.gutter:hover::after {
  background: var(--line-heavy);
}

.gutter.held::after {
  background: var(--text-faint);
}

.gutter:focus-visible::after {
  background: var(--focus-ring);
}

.panes.bordering {
  cursor: inherit;
}

.ghost {
  position: fixed;
  z-index: 70;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 22px;
  padding: 0 8px;
  border-radius: 6px;
  background: var(--bg-lit);
  box-shadow: var(--shadow-menu);
  font-size: 12px;
  color: var(--text-strong);
  white-space: nowrap;
  pointer-events: none;
}
</style>
