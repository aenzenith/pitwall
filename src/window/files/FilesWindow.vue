<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import StatusIcon from "../../components/StatusIcon.vue";
import { absolutePath, changesOf, closeFiles, COMMIT_TAB, forgetFiles, minimizeFiles, peekFiles, raiseFiles, refreshFiles, type FilesWin, type Rect } from "../../lib/explorer";
import { gitLine } from "../../lib/format";
import { t } from "../../lib/i18n";
import { useNativeMenu, type MenuEntry, type MenuPoint } from "../../lib/nativeMenu";
import { isMac, isWindows } from "../../lib/platform";
import { api, snapshot, visible } from "../../lib/store";
import FileTree from "./FileTree.vue";
import FileViewer from "./FileViewer.vue";
import SourceControl from "./SourceControl.vue";

/**
 * A Files window: floats over the app's own window, never in the way of what is under it but
 * where it lies; moved by its bar, sized from any edge, filling the app at a double click on its
 * bar, folded into its entry in the sidebar. A click in it brings it in front of the others.
 * Read only: what is written goes to the editor.
 */
const props = defineProps<{ win: FilesWin }>();
const win = props.win;

const project = computed(() => snapshot.value?.projects.find((p) => p.path === win.path) ?? null);
const files = computed(() => peekFiles(win.path));
const shown = computed(() => win.open && !win.minimized && !!project.value && !!files.value);

// A project taken off the list takes its files' window along.
watch(
  () => snapshot.value?.projects.map((p) => p.path).join("\n"),
  () => {
    if (snapshot.value && !project.value) forgetFiles(win.path);
  },
);

const revealLabel = computed(() => t(isMac ? "files.reveal.mac" : isWindows ? "files.reveal.windows" : "files.reveal.linux"));

/* ---------- where it floats ---------- */

const MIN_W = 520;
const MIN_H = 320;
/** Where the app's sidebar ends: the window first opens beside it. */
const SIDE = 232;

/** Where it first opens: beside the sidebar, centred, each window a step from the one before. */
function fallback(): Rect {
  const w = Math.min(960, Math.max(MIN_W, innerWidth - SIDE - 80));
  const h = Math.min(640, Math.max(MIN_H, innerHeight - 110));
  const step = 28 * win.cascade;
  const x = Math.max(SIDE + 16, Math.round((innerWidth - SIDE - w) / 2) + SIDE);
  const y = Math.max(40, Math.round((innerHeight - h) / 2));
  return { x: Math.min(x + step, Math.max(x, innerWidth - w - 8)), y: Math.min(y + step, Math.max(y, innerHeight - h - 8)), w, h };
}

/** Kept on screen: its bar always within reach. */
function clamp(rect: Rect): Rect {
  const w = Math.min(Math.max(rect.w, MIN_W), innerWidth - 16);
  const h = Math.min(Math.max(rect.h, MIN_H), innerHeight - 16);
  return { w, h, x: Math.min(Math.max(rect.x, 120 - w), innerWidth - 120), y: Math.min(Math.max(rect.y, 0), innerHeight - 44) };
}

const rect = computed(() => win.rect ?? fallback());
/** The app's window, for the frame that fills it. */
const viewport = reactive({ w: innerWidth, h: innerHeight });
/** Filled or not, always the same four sides, so a double click can slide them (`zooming`). */
const frame = computed<Rect>(() => (win.maximized ? { x: SIDE + 8, y: 8, w: viewport.w - SIDE - 16, h: viewport.h - 16 } : rect.value));
const style = computed(() => ({ left: `${frame.value.x}px`, top: `${frame.value.y}px`, width: `${frame.value.w}px`, height: `${frame.value.h}px` }));

/** The bar's double click: the window fills the app beside the sidebar, or goes back, its sides
 * sliding there (a real resize: the content is laid out again, never stretched). */
const zooming = ref(false);
let zoomTimer: number | undefined;
function toggleMax(): void {
  clearTimeout(zoomTimer);
  zooming.value = !matchMedia("(prefers-reduced-motion: reduce)").matches;
  win.maximized = !win.maximized;
  zoomTimer = window.setTimeout(() => (zooming.value = false), 260);
}

type Edge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
const EDGES: Edge[] = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];
let drag: { kind: "move" | Edge; x: number; y: number; from: Rect } | null = null;

function begin(event: PointerEvent, kind: "move" | Edge): void {
  if (event.button !== 0 || win.maximized) return;
  if (kind === "move" && (event.target as HTMLElement).closest("button")) return;
  drag = { kind, x: event.clientX, y: event.clientY, from: { ...rect.value } };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  event.preventDefault();
}

function move(event: PointerEvent): void {
  if (!drag) return;
  const dx = event.clientX - drag.x;
  const dy = event.clientY - drag.y;
  const r = { ...drag.from };
  if (drag.kind === "move") {
    r.x += dx;
    r.y += dy;
  } else {
    if (drag.kind.includes("e")) r.w = Math.max(MIN_W, drag.from.w + dx);
    if (drag.kind.includes("s")) r.h = Math.max(MIN_H, drag.from.h + dy);
    if (drag.kind.includes("w")) {
      r.w = Math.max(MIN_W, drag.from.w - dx);
      r.x = drag.from.x + drag.from.w - r.w;
    }
    if (drag.kind.includes("n")) {
      r.h = Math.max(MIN_H, drag.from.h - dy);
      r.y = drag.from.y + drag.from.h - r.h;
    }
  }
  win.rect = clamp(r);
}

function end(): void {
  drag = null;
}

const onResize = () => {
  viewport.w = innerWidth;
  viewport.h = innerHeight;
  if (win.rect) win.rect = clamp(win.rect);
};

/* ---------- the tree's width ---------- */

let split: { x: number; from: number } | null = null;

function beginSplit(event: PointerEvent): void {
  split = { x: event.clientX, from: win.tree };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  event.preventDefault();
}

function moveSplit(event: PointerEvent): void {
  if (split) win.tree = Math.min(Math.max(split.from + event.clientX - split.x, 160), rect.value.w - 300);
}

function splitKey(event: KeyboardEvent): void {
  const by = event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
  if (!by) return;
  event.preventDefault();
  win.tree = Math.min(Math.max(win.tree + by, 160), rect.value.w - 300);
}

/* ---------- what is shown, read again when it may have changed ---------- */

let timer: number | undefined;
function refreshSoon(): void {
  if (!shown.value || !files.value) return;
  clearTimeout(timer);
  const target = files.value;
  timer = window.setTimeout(() => void refreshFiles(target), 250);
}

// Git's state of the project moved (a save, a commit, a checkout): its marks and files again.
watch(() => JSON.stringify(project.value?.git ?? null), refreshSoon);
watch(visible, (on) => on && refreshSoon());
watch(shown, (on, was) => on && was === false && refreshSoon());

onMounted(() => {
  addEventListener("resize", onResize);
  addEventListener("focus", refreshSoon);
});
onBeforeUnmount(() => {
  removeEventListener("resize", onResize);
  removeEventListener("focus", refreshSoon);
  clearTimeout(timer);
  clearTimeout(zoomTimer);
});

/* ---------- menus and keys ---------- */

const menu = useNativeMenu();

/** A file's or a folder's menu; a tab's puts its own lines first (a commit's tab has those alone). */
function itemMenu(at: MenuPoint, file: string, dir: boolean, head: MenuEntry[] = []): void {
  const path = win.path;
  const entries: MenuEntry[] = [...head];
  if (file.startsWith(COMMIT_TAB)) return void menu.popup(at, entries);
  if (entries.length) entries.push("separator");
  if (!dir) entries.push({ text: t("common.openInEditor"), action: () => void api.openReference(path, file, 1, null) });
  entries.push({ text: revealLabel.value, action: () => void api.explorerReveal(path, file) });
  entries.push("separator", { text: t("files.copyPath"), action: () => void api.copyText(absolutePath(path, file)) }, { text: t("files.copyRelative"), action: () => void api.copyText(file) });
  void menu.popup(at, entries);
}

const tree = ref<InstanceType<typeof FileTree> | null>(null);
const viewer = ref<InstanceType<typeof FileViewer> | null>(null);

function onKey(event: KeyboardEvent): void {
  const mod = isMac ? event.metaKey : event.ctrlKey;
  if (!mod || event.altKey || event.defaultPrevented) return;
  if (event.code === "KeyP" && !event.shiftKey) {
    event.preventDefault();
    win.side = "files";
    void nextTick(() => tree.value?.focusSearch());
  } else if (event.code === "KeyF" && !event.shiftKey) {
    event.preventDefault();
    viewer.value?.openFind();
  } else if (event.code === "KeyV" && event.shiftKey && !(event.target as HTMLElement).closest("input, textarea")) {
    // VS Code's keys for a Markdown file's preview; here an SVG's too.
    event.preventDefault();
    viewer.value?.togglePreview();
  } else if (event.code === "KeyA" && !event.shiftKey && !(event.target as HTMLElement).closest("input, textarea, .selectable")) {
    // On a tab or the tree: nothing to select. The page's own Select All would take the text of
    // the app's window behind (the code and the preview take ⌘A first, for their own text).
    event.preventDefault();
  }
}

/* ---------- the side tabs: Files and Source control, as VS Code's activity bar ---------- */

/** What Git sees changed, on the Source control tab's badge. */
const changeCount = computed(() => (files.value ? changesOf(files.value).length : 0));

/** A tab shows its panel; the one shown, pressed again, hides it (the file takes the room). */
function pickSide(side: "files" | "scm"): void {
  win.side = win.side === side ? null : side;
}

/* ---------- motion: it opens and closes in place, and folds into its sidebar entry and back ---------- */

const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
/** The window comes back out of its sidebar entry (a click on it, the button, ⇧⌘E), not opened anew. */
let fromSide = false;

watch(
  () => win.minimized,
  (now, was) => {
    if (was && !now && win.open) fromSide = true;
  },
  { flush: "sync" },
);

/** Its entry in the sidebar, where it folds into and grows out of. */
function entry(): DOMRect {
  const drawn = document.querySelector<HTMLElement>(`.files-nav [data-files="${CSS.escape(win.path)}"]`);
  return drawn?.offsetWidth ? drawn.getBoundingClientRect() : new DOMRect(16, innerHeight / 2, SIDE - 32, 32);
}

/** The window's own box drawn as `to`: moved and scaled from its top left corner. */
function toward(from: DOMRect, to: DOMRect): string {
  return `translate(${to.left - from.left}px, ${to.top - from.top}px) scale(${to.width / from.width}, ${to.height / from.height})`;
}

function winEnter(el: Element, done: () => void): void {
  const growing = fromSide;
  fromSide = false;
  if (still()) return done();
  const box = el.getBoundingClientRect();
  const motion = growing
    ? el.animate(
        [
          { transformOrigin: "0 0", transform: toward(box, entry()), opacity: 0 },
          { transformOrigin: "0 0", transform: toward(box, entry()), opacity: 0.6, offset: 0.15 },
          { transformOrigin: "0 0", transform: "none", opacity: 1 },
        ],
        { duration: 280, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
      )
    : el.animate(
        [
          { transform: "scale(0.96)", opacity: 0 },
          { transform: "none", opacity: 1 },
        ],
        { duration: 180, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
      );
  motion.onfinish = done;
  motion.oncancel = done;
}

function winLeave(el: Element, done: () => void): void {
  if (still()) return done();
  const box = el.getBoundingClientRect();
  const motion =
    win.minimized && win.open
      ? el.animate(
          [
            { transformOrigin: "0 0", transform: "none", opacity: 1 },
            { transformOrigin: "0 0", transform: toward(box, entry()), opacity: 0.5, offset: 0.85 },
            { transformOrigin: "0 0", transform: toward(box, entry()), opacity: 0 },
          ],
          { duration: 260, easing: "cubic-bezier(0.5, 0, 0.75, 0)", fill: "forwards" },
        )
      : el.animate(
          [
            { transform: "none", opacity: 1 },
            { transform: "scale(0.97)", opacity: 0 },
          ],
          { duration: 140, easing: "ease-in", fill: "forwards" },
        );
  motion.onfinish = done;
  motion.oncancel = done;
}
</script>

<template>
  <!-- `appear`: a new window is mounted already shown, and opens with the same motion. -->
  <Transition :css="false" appear @enter="winEnter" @leave="winLeave">
    <section
      v-if="shown && project && files"
      :class="['files-win', { max: win.maximized, zooming }]"
      :style="[style, { zIndex: win.z }]"
      :aria-label="t('files.window', { name: project.name })"
      @keydown="onKey"
      @pointerdown.capture="raiseFiles(win)"
      @focusin="raiseFiles(win)"
    >
      <header class="bar" @pointerdown="begin($event, 'move')" @pointermove="move" @pointerup="end" @pointercancel="end" @dblclick="toggleMax">
        <StatusIcon :project="project" ring="var(--bg-files-bar)" />
        <span class="title">{{ project.name }}</span>
        <span class="sub">{{ t("files.title") }}</span>
        <span v-if="project.git" class="branch">{{ gitLine(project.git) }}</span>
        <span class="grow"></span>
        <button type="button" class="icon small" :aria-label="t('files.minimize')" :title="t('files.minimize')" @click="minimizeFiles(win)"><Icon name="minus" :size="14" /></button>
        <button type="button" class="icon small" :aria-label="t('common.close')" :title="t('common.close')" @click="closeFiles(win.path)"><Icon name="close" :size="13" /></button>
      </header>

      <div class="body">
        <nav class="activity" :aria-label="t('scm.sideTabs')">
          <button type="button" :class="['side-tab', { on: win.side === 'files' }]" :aria-pressed="win.side === 'files'" :aria-label="t('files.title')" :title="t('files.title')" @click="pickSide('files')">
            <Icon name="files" :size="19" />
          </button>
          <button type="button" :class="['side-tab', { on: win.side === 'scm' }]" :aria-pressed="win.side === 'scm'" :aria-label="t('scm.title')" :title="t('scm.title')" @click="pickSide('scm')">
            <Icon name="source-control" :size="19" />
            <span v-if="changeCount" class="badge" aria-hidden="true">{{ changeCount > 99 ? "99+" : changeCount }}</span>
          </button>
        </nav>
        <FileTree v-if="win.side === 'files'" ref="tree" :files="files" :style="{ width: `${win.tree}px` }" class="tree" @menu="(at, file, dir) => itemMenu(at, file, dir)" />
        <SourceControl v-else-if="win.side === 'scm'" :files="files" :project="project" :style="{ width: `${win.tree}px` }" v-model:split="win.scmSplit" class="tree" @menu="(event, file) => itemMenu(event, file, false)" />
        <div
          v-if="win.side"
          class="split"
          role="separator"
          aria-orientation="vertical"
          tabindex="0"
          :aria-label="t('files.resizeTree')"
          :aria-valuenow="win.tree"
          @pointerdown="beginSplit"
          @pointermove="moveSplit"
          @pointerup="split = null"
          @pointercancel="split = null"
          @keydown="splitKey"
        ></div>
        <FileViewer ref="viewer" :files="files" :reveal-label="revealLabel" @menu="(event, file, head) => itemMenu(event, file, false, head)" />
      </div>

      <template v-if="!win.maximized">
        <span v-for="edge in EDGES" :key="edge" :class="['edge', edge]" aria-hidden="true" @pointerdown="begin($event, edge)" @pointermove="move" @pointerup="end" @pointercancel="end"></span>
      </template>
    </section>
  </Transition>

</template>

<style scoped src="../detail/detail.css"></style>
<style scoped>
.files-win {
  --bg-files-bar: var(--bg-panel);
  --bg-files-activity: var(--bg-code-tabs);
  position: fixed;
  pointer-events: auto;
  display: flex;
  flex-direction: column;
  border-radius: 12px;
  background: var(--bg-log);
  box-shadow:
    0 0 0 1px var(--line-dialog),
    var(--shadow-dialog);
  /* Not `hidden`: that makes the window a scroll box of the few pixels its edges reach out, which
     a `scrollIntoView` inside would scroll. */
  overflow: clip;
}

.bar {
  height: 38px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 6px 0 14px;
  background: var(--bg-files-bar);
  border-bottom: 1px solid var(--line-soft);
  cursor: grab;
  white-space: nowrap;
}

.files-win.zooming {
  transition:
    left 240ms cubic-bezier(0.2, 0.8, 0.2, 1),
    top 240ms cubic-bezier(0.2, 0.8, 0.2, 1),
    width 240ms cubic-bezier(0.2, 0.8, 0.2, 1),
    height 240ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.files-win.max .bar {
  cursor: default;
}

.title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-strong);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sub {
  font-size: 12px;
  color: var(--text-subtle);
}

.branch {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-left: 4px;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-subtle);
}

.grow {
  flex: 1;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
}

.tree {
  flex-shrink: 0;
  border-right: 1px solid var(--line-soft);
}

/* VS Code's activity bar: a tab on a ground of its own when shown, never an edge of colour. */
.activity {
  width: 46px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 8px 0;
  background: var(--bg-files-activity);
  border-right: 1px solid var(--line-soft);
}

.side-tab {
  position: relative;
  width: 36px;
  height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-faint);
}

.side-tab:hover {
  color: var(--text-soft);
}

.side-tab.on {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

.badge {
  position: absolute;
  top: 3px;
  right: 1px;
  min-width: 15px;
  height: 15px;
  box-sizing: border-box;
  padding: 0 4px;
  border-radius: 8px;
  background: var(--line-selected);
  box-shadow: 0 0 0 2px var(--bg-files-activity);
  font-size: 9.5px;
  font-weight: 700;
  line-height: 15px;
  color: var(--text-strong);
}

.split {
  width: 5px;
  margin: 0 -3px 0 -2px;
  flex-shrink: 0;
  z-index: 1;
  cursor: col-resize;
}

.split:focus-visible {
  outline-offset: -2px;
}

.edge {
  position: absolute;
  z-index: 3;
}

.edge.n,
.edge.s {
  left: 10px;
  right: 10px;
  height: 6px;
  cursor: ns-resize;
}

.edge.e,
.edge.w {
  top: 10px;
  bottom: 10px;
  width: 6px;
  cursor: ew-resize;
}

.edge.n {
  top: -3px;
}

.edge.s {
  bottom: -3px;
}

.edge.e {
  right: -3px;
}

.edge.w {
  left: -3px;
}

.edge.ne,
.edge.nw,
.edge.se,
.edge.sw {
  width: 12px;
  height: 12px;
}

.edge.ne {
  top: -4px;
  right: -4px;
  cursor: nesw-resize;
}

.edge.sw {
  bottom: -4px;
  left: -4px;
  cursor: nesw-resize;
}

.edge.nw {
  top: -4px;
  left: -4px;
  cursor: nwse-resize;
}

.edge.se {
  bottom: -4px;
  right: -4px;
  cursor: nwse-resize;
}
</style>
