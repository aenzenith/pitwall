<script setup lang="ts">
import { onBeforeUnmount, onBeforeUpdate, onMounted, onUpdated, ref, shallowRef, watch } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { clock } from "../../lib/day";
import { language, t } from "../../lib/i18n";
import { cardDraft, errorBlock, runEnd, segments, type FileRef, type Marker, type OutputLine, type Range, type Segment } from "../../lib/outputLines";
import type { Hit } from "../../lib/outputSearch";
import { api } from "../../lib/store";

/**
 * An output's lines as the pane shows them, newest last.
 *
 * - It follows the output: the newest line stays in view until you scroll up. Then it stands
 *   still, a pill counts what arrives meanwhile, and the end (scrolled to, or the pill) takes
 *   hold again.
 * - A marker of the core's is a rule with a word and its time; the lines before the newest
 *   "started" one (`runStart`) are of a run before and dimmed.
 * - An error's first line, and a file a line names, open a small menu at the line: the file in
 *   the editor (the core finds it, `path` being the project's folder), the error as a card.
 * - `ranges` marks what the search found in a line, `current` the match it stands on, which is
 *   scrolled into view when it changes.
 *
 * A line is drawn once: nothing about it changes after it arrived, so each row is kept as it is
 * (`v-memo`) while batches of new lines come in.
 */
const props = defineProps<{
  path: string;
  lines: OutputLine[];
  runStart: number;
  ranges: ((line: OutputLine) => Range[]) | null;
  current: Hit | null;
  /** What stands there while there is no line. */
  empty: string;
}>();
const emit = defineEmits<{ card: [draft: { title: string; note: string }] }>();

const scroller = ref<HTMLElement | null>(null);

/* ---------- following ---------- */

/** The newest line stays in view. */
const following = ref(true);
/** The lines that arrived since following stopped: counted as they come, for the oldest of them
 * scroll out of the output while it stands still. */
const fresh = ref(0);
/** The newest line counted into `fresh`. */
let counted = 0;
/** The newest line drawn so far. */
let drawn = 0;
/** Where following last left the view, to tell a scroll up that no event has told of yet. */
let pinnedTop = 0;

/** The newest line among `lines`. A rule is none, and may stand below lines newer than itself
 * (lib/outputLines' `oneMoment`). */
function newest(lines: OutputLine[]): number {
  for (let i = lines.length - 1; i >= 0; i--) if (!lines[i].marker) return lines[i].id;
  return 0;
}

watch(
  () => props.lines,
  (lines) => {
    if (following.value) return;
    for (let i = lines.length - 1; i >= 0 && (lines[i].marker || lines[i].id > counted); i--) if (!lines[i].marker) fresh.value++;
    counted = Math.max(counted, newest(lines));
  },
);

function setFollowing(on: boolean): void {
  if (on === following.value) return;
  // What was drawn when following stopped is seen; what comes after it is new.
  if (!on) [fresh.value, counted] = [0, drawn];
  following.value = on;
}

function toEnd(): void {
  const el = scroller.value;
  if (!el) return;
  el.scrollTop = el.scrollHeight;
  pinnedTop = el.scrollTop;
}

function atEnd(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 4;
}

function onScroll(): void {
  const el = scroller.value;
  if (!el) return;
  setFollowing(atEnd(el));
  // A menu goes with its line: out of view, it would only be there for Esc to close.
  if (menu.value && !inView(el, menu.value.id)) closeMenu();
}

function follow(): void {
  setFollowing(true);
  toEnd();
}

/** The line at the view's top while it stands still, and where it was: lines that scroll out
 * of the output above it must not move it. */
let held: { el: HTMLElement; top: number } | null = null;

/** The first row that reaches below the view's top and is of a line from `oldest` on (a rule
 * has no line: the next row that has one stands for it). */
function topRow(el: HTMLElement, oldest: number): HTMLElement | null {
  const rows = el.children;
  let low = 0;
  let high = rows.length - 1;
  while (low < high) {
    const mid = (low + high) >> 1;
    const row = rows[mid] as HTMLElement;
    if (row.offsetTop + row.offsetHeight > el.scrollTop) high = mid;
    else low = mid + 1;
  }
  while (low < rows.length - 1 && !(Number((rows[low] as HTMLElement).dataset.id) >= oldest)) low++;
  return (rows[low] as HTMLElement | undefined) ?? null;
}

// Read before the rows change, written after: one layout per batch of lines.
onBeforeUpdate(() => {
  const el = scroller.value;
  held = null;
  if (!el) return;
  // Scrolled up since the last batch, and the scroll's own event still to come. Not a pane
  // that grew: that one is still at its end.
  if (following.value && el.scrollTop < pinnedTop - 4 && !atEnd(el)) setFollowing(false);
  if (following.value) return;
  // The lines are the new ones already, the rows still the old: a row whose line this batch
  // pushed out of the output can't be held, the first one that stays can.
  const row = topRow(el, props.lines[0]?.id ?? 0);
  if (row) held = { el: row, top: row.offsetTop };
});

onUpdated(() => {
  const el = scroller.value;
  drawn = newest(props.lines);
  if (!el) return;
  if (following.value) toEnd();
  else if (held?.el.isConnected && held.el.offsetTop !== held.top) el.scrollTop += held.el.offsetTop - held.top;
  held = null;
});

// The panel dragged smaller or brought back: the end stays the end.
let resized: ResizeObserver | null = null;
onMounted(() => {
  drawn = newest(props.lines);
  toEnd();
  if (!scroller.value) return;
  resized = new ResizeObserver(() => following.value && toEnd());
  resized.observe(scroller.value);
});

/* ---------- rows ---------- */

const CLAUDE = new Set<Marker["kind"]>(["claude-read", "claude-start", "claude-stop", "claude-restart"]);

function markerText(marker: Marker): string {
  const time = clock(marker.at);
  if (marker.kind === "ended") {
    const { how, code } = runEnd(marker);
    if (how === "crashed") return code ? t("output.marker.crashedCode", { code, time }) : t("output.marker.crashed", { time });
    if (how === "exited") return code ? t("output.marker.exitedCode", { code, time }) : t("output.marker.exited", { time });
    return how === "stopped" ? t("output.marker.stopped", { time }) : t("output.marker.ended", { time });
  }
  const text = { started: "output.marker.started", "claude-read": "output.marker.claudeRead", "claude-start": "output.marker.claudeStart", "claude-stop": "output.marker.claudeStop", "claude-restart": "output.marker.claudeRestart" } as const;
  return t(text[marker.kind], { time });
}

function rowClass(line: OutputLine): unknown[] {
  if (line.marker) return ["rule", { claude: CLAUDE.has(line.marker.kind) }];
  return ["line", line.kind, { error: line.level === "error", old: line.id < props.runStart }];
}

/** A line's text in the stretches it is drawn in: one, unless it names a file or the search
 * found something in it. */
function parts(line: OutputLine): Segment[] {
  const hits = props.ranges?.(line) ?? [];
  return line.ref || hits.length ? segments(line, hits) : [{ text: line.text, ref: false, hit: -1 }];
}

/* ---------- the menu at a line ---------- */

/** `file`: the file "Open in editor" opens, null on a line that names none. `card`: the line is
 * an error's. `missing`: the project has no such file, said in the menu's place for a moment. */
type LineMenu = { id: number; file: FileRef | null; card: boolean; busy: boolean; missing: boolean; above: boolean };

const menu = shallowRef<LineMenu | null>(null);
let missingTimer = 0;
/** The room a menu takes under its line, its buttons one over the other as a narrow pane has them. */
const MENU_ROOM = 72;

function closeMenu(): void {
  window.clearTimeout(missingTimer);
  menu.value = null;
}

/** Whether any of the line's row is in the view. */
function inView(el: HTMLElement, id: number): boolean {
  const row = el.querySelector<HTMLElement>(`[data-id="${id}"]`);
  return !!row && row.offsetTop + row.offsetHeight > el.scrollTop && row.offsetTop < el.scrollTop + el.clientHeight;
}

/** An error's first line opens its menu from anywhere on it; any other line from the file it names. */
function onClick(event: MouseEvent): void {
  const target = event.target as HTMLElement;
  if (target.closest(".menu")) return;
  const row = target.closest<HTMLElement>("[data-id]");
  const line = row && props.lines.find((line) => line.id === Number(row.dataset.id));
  // A press that selected text is no click on the line.
  if (!line || window.getSelection()?.isCollapsed === false || menu.value?.id === line.id) return closeMenu();

  const whole = line.level === "error";
  const onFile = !!target.closest(".ref");
  if (!whole && !onFile) return closeMenu();

  // Over the line where the view has no room under it and more above: the menu never makes the
  // output longer, so the line stays under the pointer.
  const [view, at] = [(event.currentTarget as HTMLElement).getBoundingClientRect(), row.getBoundingClientRect()];
  const under = view.bottom - at.bottom;
  const above = under < MENU_ROOM && at.top - view.top > under;

  // An error that names its file further down ("… from Checkout.vue:42:11" on the next line)
  // opens that one from its first line.
  const file = line.ref ?? (whole ? (errorBlock(props.lines, props.lines.indexOf(line)).find((part) => part.ref)?.ref ?? null) : null);

  window.clearTimeout(missingTimer);
  menu.value = { id: line.id, file, card: !!line.level, busy: false, missing: false, above };
}

async function openFile(): Promise<void> {
  const at = menu.value;
  if (!at?.file || at.busy) return;
  menu.value = { ...at, busy: true };

  const opened = await api.openReference(props.path, at.file.file, at.file.line, at.file.column).catch(() => false);
  // Closed, or another line's, meanwhile.
  if (menu.value?.id !== at.id) return;
  if (opened) return closeMenu();

  menu.value = { ...at, busy: false, missing: true };
  missingTimer = window.setTimeout(closeMenu, 2600);
}

function toCard(): void {
  const at = props.lines.findIndex((line) => line.id === menu.value?.id);
  closeMenu();
  if (at >= 0) emit("card", cardDraft(errorBlock(props.lines, at)));
}

function onDocumentDown(event: PointerEvent): void {
  if (!(event.target instanceof Node && scroller.value?.contains(event.target))) closeMenu();
}

function onDocumentKey(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.stopPropagation();
  closeMenu();
}

function unlisten(): void {
  document.removeEventListener("pointerdown", onDocumentDown, true);
  document.removeEventListener("keydown", onDocumentKey, true);
}

// Only while a menu is open: a press outside the output or Esc closes it.
watch(
  () => menu.value !== null,
  (shown) => {
    unlisten();
    if (!shown) return;
    document.addEventListener("pointerdown", onDocumentDown, true);
    document.addEventListener("keydown", onDocumentKey, true);
  },
);

// Its line scrolled out of the output, or the filter took it away.
watch(
  () => props.lines,
  (lines) => {
    if (menu.value && !lines.some((line) => line.id === menu.value?.id)) closeMenu();
  },
);

/* ---------- the search's place ---------- */

/** Room kept clear around the match it stands on: a line above, the pill's height below. */
const MATCH_ROOM = { top: 8, bottom: 40 };

// The match the search stands on comes into view, by this pane's own scroll alone. One above
// the end takes the pane there and it stands still, as after a scroll up.
watch(
  () => props.current,
  (_, before) => {
    const el = scroller.value;
    const mark = el?.querySelector("mark.on");
    if (!el || !mark) return;
    // Not a step of the user's: the match it stood on scrolled out of the output and the next
    // one took over. A pane that follows stays at its end.
    if (following.value && before && !props.lines.some((line) => line.id === before.id)) return;

    const [box, at] = [el.getBoundingClientRect(), mark.getBoundingClientRect()];
    if (at.top < box.top + MATCH_ROOM.top) el.scrollTop -= box.top + MATCH_ROOM.top - at.top;
    else if (at.bottom > box.bottom - MATCH_ROOM.bottom) el.scrollTop += at.bottom - (box.bottom - MATCH_ROOM.bottom);
    setFollowing(atEnd(el));
  },
  { flush: "post" },
);

onBeforeUnmount(() => {
  resized?.disconnect();
  window.clearTimeout(missingTimer);
  unlisten();
});

defineExpose({
  /** The keyboard to the lines (arrows and Page keys scroll them). */
  focus: () => scroller.value?.focus({ preventScroll: true }),
});
</script>

<template>
  <div class="log">
    <div ref="scroller" class="lines selectable" role="tabpanel" tabindex="0" @scroll.passive="onScroll" @click="onClick">
      <template v-if="lines.length">
        <span
          v-for="line in lines"
          :key="line.id"
          v-memo="[line.id < runStart, menu?.id === line.id && menu, ranges, current?.id === line.id ? current.k : -1, language]"
          :data-id="line.marker ? undefined : line.id"
          :class="rowClass(line)"
        >
          <template v-if="line.marker"><i></i><ClaudeLogo v-if="CLAUDE.has(line.marker.kind)" :size="11" color="var(--claude)" /><span class="what">{{ markerText(line.marker) }}</span><i></i></template>
          <template v-else>
            <template v-for="(part, k) in parts(line)" :key="k"><mark v-if="part.hit >= 0" :class="{ on: current?.id === line.id && current.k === part.hit, ref: part.ref }">{{ part.text }}</mark><span v-else-if="part.ref" class="ref">{{ part.text }}</span><template v-else>{{ part.text }}</template></template>
            <span v-if="menu?.id === line.id" :class="['menu', { above: menu.above }]">
              <span v-if="menu.missing" class="missing" role="status">{{ t("output.notInProject") }}</span>
              <template v-else>
                <button v-if="menu.file" type="button" :disabled="menu.busy" @click="openFile">
                  <Spinner v-if="menu.busy" :size="11" />
                  <Icon v-else name="editor" :size="13" />
                  {{ t("common.openInEditor") }}
                </button>
                <button v-if="menu.card" type="button" @click="toCard">
                  <Icon name="board" :size="13" />
                  {{ t("output.toCard") }}
                </button>
              </template>
            </span>
          </template>
        </span>
      </template>
      <span v-else class="dim">{{ empty }}</span>
    </div>
    <button v-if="!following" type="button" class="pill" @click="follow">
      {{ fresh ? t("output.pausedNew", { count: fresh }) : t("output.paused") }}
      <Icon name="chevron-down" :size="12" />
    </button>
  </div>
</template>

<style scoped>
/* The box of output; its lines scroll inside it, the pill stays at its foot. */
.log {
  position: relative;
  display: flex;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid var(--line-inset);
  overflow: hidden;
}

/* Its own place is kept by hand while it stands still (`held`): the browser's anchoring would
   move it a second time. Rows are whole pixels high (20 for 12 px text, where 1.7 gave 20.4), so
   the lines that leave above the view take a whole number with them and what is read stays put. */
.lines {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  box-sizing: border-box;
  padding: 12px;
  display: flex;
  flex-direction: column;
  overflow: auto;
  overflow-anchor: none;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 20px;
  color: var(--text-log);
}

.lines:focus-visible {
  outline-offset: -2px;
  border-radius: 8px;
}

.line {
  position: relative;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.cmd {
  color: var(--text);
}

.sys {
  color: var(--claude-text);
}

/* A run before: its lines step back, an error among them stays one. */
.old {
  color: var(--text-faint);
}

.error {
  color: var(--crash-text);
  cursor: pointer;
}

.dim {
  color: var(--text-subtle);
}

.ref {
  cursor: pointer;
}

.ref:hover {
  text-decoration: underline;
  text-underline-offset: 3px;
}

/* What the search found. No padding: a mark must not move the text it is in. */
mark {
  border-radius: 2px;
  background: var(--find-bg);
  box-shadow: 0 0 0 1px var(--find-bg);
  color: inherit;
}

mark.on {
  background: var(--find-on-bg);
  box-shadow: 0 0 0 1px var(--find-on-bg);
  color: var(--find-on-text);
}

/* A marker: a hairline either side of a word. Not part of the output, so not of a selection. */
.rule {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 3px 0;
  font-family: var(--font);
  font-size: 11px;
  line-height: 18px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.rule,
.rule *,
.menu,
.menu * {
  -webkit-user-select: none;
  user-select: none;
}

.rule i {
  flex: 1 1 0;
  min-width: 10px;
  height: 1px;
  background: var(--line);
}

.rule .what {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rule.claude {
  color: var(--claude-text);
}

/* Under its line, over the ones that follow; in a narrow pane its buttons take a row each. */
.menu {
  position: absolute;
  left: 0;
  top: 100%;
  z-index: 1;
  max-width: 100%;
  display: inline-flex;
  flex-wrap: wrap;
  gap: 2px;
  margin-top: 3px;
  padding: 4px;
  border-radius: 8px;
  background: var(--bg-control);
  border: 1px solid var(--line-dialog);
  box-shadow: var(--shadow-tip);
  font-family: var(--font);
  font-size: 12px;
  line-height: 1;
  white-space: nowrap;
  color: var(--text-soft);
  cursor: default;
}

.menu.above {
  top: auto;
  bottom: 100%;
  margin: 0 0 3px;
}

.menu button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 9px;
  border: 0;
  border-radius: 6px;
  background: transparent;
}

.menu button:hover:not(:disabled) {
  background: var(--bg-control-hover);
  color: var(--text);
}

.missing {
  display: inline-flex;
  align-items: center;
  height: 26px;
  padding: 0 9px;
  color: var(--text-muted);
}

.pill {
  position: absolute;
  left: 50%;
  bottom: 10px;
  transform: translateX(-50%);
  max-width: calc(100% - 16px);
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-dialog);
  border-radius: 13px;
  background: var(--bg-control);
  box-shadow: var(--shadow-tip);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text);
}

.pill:hover {
  background: var(--bg-control-hover);
}
</style>
