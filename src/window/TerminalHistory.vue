<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import ClaudeLogo from "../components/ClaudeLogo.vue";
import Highlight from "../components/Highlight.vue";
import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { clock, dayName } from "../lib/day";
import { fuzzy } from "../lib/fuzzy";
import { t } from "../lib/i18n";
import { periodLabel } from "../lib/period";
import { originView, statusLine } from "../lib/sessions";
import { api, now, sessions, snapshot, today } from "../lib/store";
import type { PastSession, Project, Resumed, SessionRow } from "../lib/types";

/**
 * The terminals' history (window/TerminalPanel): the project's Claude sessions that open again in
 * its terminals, the last heard of first, by day. A click brings one up where it runs, or opens it
 * again in a new tab once it has ended (`resume_claude`). Dragged onto the terminal, it opens where
 * it is let go (the panel works out where: `drag`, `drop`). `overlay`: a narrow panel's, over the
 * terminal's right side, gone once a session is picked.
 */
const props = defineProps<{ project: Project; overlay?: boolean }>();
const emit = defineEmits<{
  close: [];
  leave: [];
  /** A row dragged: its session and where the pointer is; none once the drag is called off. */
  drag: [drag: { id: string; name: string; x: number; y: number } | null];
  /** A dragged row let go: the panel opens its session where it was let go. */
  drop: [id: string];
}>();

/** The last list read per project, shown at once when the column comes back to it. */
const kept = new Map<string, PastSession[]>();

const list = ref<PastSession[] | null>(kept.get(props.project.path) ?? null);
const query = ref("");
/** The row the keyboard is on, by session id. */
const picked = ref<string | null>(null);
/** The session being brought up, while its tab opens. */
const opening = ref<string | null>(null);
/** What went wrong bringing a session up, by id, until the list is read again. */
const failed = ref<Record<string, Resumed>>({});
const field = ref<HTMLInputElement | null>(null);
const listbox = ref<HTMLElement | null>(null);

/** The project's sessions that are open now, by id: where they run and what Claude is at. */
const live = computed(() => {
  const open = new Map<string, SessionRow>();
  for (const row of sessions.value?.sessions ?? []) {
    if (row.phase !== "ended" && row.path === props.project.path) open.set(row.id, row);
  }
  return open;
});

let asked = 0;
async function load(): Promise<void> {
  const path = props.project.path;
  const ask = ++asked;
  try {
    const found = await api.claudePast(path);
    if (ask !== asked) return;
    kept.set(path, found);
    list.value = found;
    failed.value = {};
  } catch {
    if (ask === asked) list.value ??= [];
  }
}

watch(
  () => props.project.path,
  (path) => {
    list.value = kept.get(path) ?? null;
    query.value = "";
    picked.value = null;
    failed.value = {};
    void load();
  },
  { immediate: true },
);

// A session that starts or ends is read again with the rest: one that ended has its log by then.
watch(
  () => [...live.value.keys()].sort().join(" "),
  (ids, was) => {
    if (ids !== was) void load();
  },
);

/**
 * The list as read, under the project's open sessions it doesn't have yet (Claude Code writes a
 * session's log with its first message), the last heard of first: those that run in the
 * project's own folder, the one `claude --resume` finds them from.
 */
const shown = computed<PastSession[] | null>(() => {
  if (list.value === null) return null;
  const read = new Set(list.value.map((session) => session.id));
  const fresh = [...live.value.values()]
    .filter((row) => !read.has(row.id) && row.folder === props.project.path)
    .map((row) => ({ id: row.id, title: row.title, at: row.since ?? now.value }))
    .sort((a, b) => b.at - a.at);
  return [...fresh, ...list.value];
});

type Item = { session: PastSession; name: string; hits: Set<number> | null };

/** The sessions the search keeps, in the list's order, each with its name's matched letters; an
 * open one by the name it has now. */
const items = computed<Item[]>(() => {
  const q = query.value.trim();
  const out: Item[] = [];
  for (const session of shown.value ?? []) {
    const name = live.value.get(session.id)?.title ?? session.title ?? t("terminal.historyUntitled");
    if (!q) {
      out.push({ session, name, hits: null });
      continue;
    }
    const match = fuzzy(name, q);
    if (match) out.push({ session, name, hits: new Set(match.hits) });
  }
  return out;
});

/** The items by the day they were last heard of: today, yesterday, then dates. */
const days = computed(() => {
  const groups: Array<{ day: string; label: string; items: Item[] }> = [];
  for (const item of items.value) {
    const day = item.session.at === null ? "" : dayName(item.session.at);
    const last = groups[groups.length - 1];
    if (last?.day === day) last.items.push(item);
    else groups.push({ day, label: day ? periodLabel(day === today.value ? null : day, today.value) : "", items: [item] });
  }
  return groups;
});

// A search picks its best place to start: the first it keeps.
watch(query, (q) => {
  picked.value = q.trim() ? (items.value[0]?.session.id ?? null) : null;
});

function liveDot(row: SessionRow): { phase: "waiting" | "working"; text: string } | null {
  return row.phase === "waiting" || row.phase === "working" ? { phase: row.phase, text: statusLine(row, now.value).text } : null;
}

/** Where an open session runs, as the row says it: in a tab here, or its place elsewhere. */
function where(row: SessionRow): string {
  return row.origin.kind === "pitwall" ? t("terminal.historyInTab") : originView(row.origin, props.project.name, snapshot.value?.settings.editor).short;
}

async function open(id: string): Promise<Resumed | null> {
  if (opening.value) return null;
  picked.value = id;
  opening.value = id;
  try {
    const result = await api.resumeClaude(props.project.path, id);
    if (result === "missing" || result === "unsure") failed.value = { ...failed.value, [id]: result };
    else if (props.overlay) emit("close");
    return result;
  } catch {
    failed.value = { ...failed.value, [id]: "unsure" };
    return "unsure";
  } finally {
    opening.value = null;
  }
}

/* A row dragged onto the terminal: the session opens where it is let go. One that runs somewhere
   else is never opened a second time, so it has nowhere to go here: it is only clicked. */

/** How far the pointer goes before a press on a row becomes a drag. */
const DRAG_FROM = 5;
let press: { id: string; name: string; x: number; y: number; started: boolean } | null = null;
/** A drag just ended: the click that may follow it opens nothing. */
let dragged = false;

function startDrag(event: PointerEvent, item: Item): void {
  if (event.button !== 0 || opening.value) return;
  const row = live.value.get(item.session.id);
  if (row && row.origin.kind !== "pitwall") return;
  press = { id: item.session.id, name: item.name, x: event.clientX, y: event.clientY, started: false };
  window.addEventListener("pointermove", dragMove);
  window.addEventListener("pointerup", dragEnd);
  window.addEventListener("pointercancel", dragCancel);
  window.addEventListener("keydown", dragKey, true);
}

function dragMove(event: PointerEvent): void {
  if (!press) return;
  if (!press.started) {
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_FROM) return;
    press.started = true;
    document.body.classList.add("grabbing");
  }
  emit("drag", { id: press.id, name: press.name, x: event.clientX, y: event.clientY });
}

function dragEnd(): void {
  const was = press;
  stopDrag();
  if (!was?.started) return;
  dragged = true;
  window.setTimeout(() => (dragged = false));
  emit("drop", was.id);
}

function dragCancel(): void {
  const started = press?.started;
  stopDrag();
  if (started) emit("drag", null);
}

function dragKey(event: KeyboardEvent): void {
  if (event.key !== "Escape" || !press?.started) return;
  event.preventDefault();
  event.stopPropagation();
  dragCancel();
}

function stopDrag(): void {
  press = null;
  document.body.classList.remove("grabbing");
  window.removeEventListener("pointermove", dragMove);
  window.removeEventListener("pointerup", dragEnd);
  window.removeEventListener("pointercancel", dragCancel);
  window.removeEventListener("keydown", dragKey, true);
}

onBeforeUnmount(stopDrag);

function clickRow(id: string): void {
  if (!dragged) void open(id);
}

function rowId(id: string): string {
  return `past-${id}`;
}

function showPicked(): void {
  void nextTick(() => {
    if (picked.value) listbox.value?.querySelector(`#${CSS.escape(rowId(picked.value))}`)?.scrollIntoView({ block: "nearest" });
  });
}

/** ↑/↓ move over the rows, ↵ brings the one picked (the first, with none) up; Esc empties the
 * search, then leaves: the overlay closes, the column hands the keyboard back to the terminal. */
function onKey(event: KeyboardEvent): void {
  const list = items.value;
  const at = list.findIndex((item) => item.session.id === picked.value);
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      picked.value = list[Math.min(at + 1, list.length - 1)]?.session.id ?? null;
      showPicked();
      return;
    case "ArrowUp":
      event.preventDefault();
      picked.value = list[Math.max(at - 1, 0)]?.session.id ?? null;
      showPicked();
      return;
    case "Enter": {
      event.preventDefault();
      const id = picked.value ?? list[0]?.session.id;
      if (id) void open(id);
      return;
    }
    case "Escape":
      event.preventDefault();
      if (query.value) query.value = "";
      else if (props.overlay) emit("close");
      else emit("leave");
      return;
  }
}

/** Without scrolling anything: beside the terminal, its room is still opening. */
function focus(): void {
  field.value?.focus({ preventScroll: true });
}

defineExpose({ focus, open });
</script>

<template>
  <div :class="['history', { overlay }]" :role="overlay ? 'dialog' : undefined" :aria-label="t('terminal.historyTitle')">
    <label class="search">
      <Icon name="search" :size="13" />
      <input
        ref="field"
        v-model="query"
        type="text"
        role="combobox"
        spellcheck="false"
        autocomplete="off"
        aria-autocomplete="list"
        aria-expanded="true"
        :aria-controls="`past-${project.path}`"
        :aria-activedescendant="picked ? rowId(picked) : undefined"
        :aria-label="t('terminal.historySearch')"
        :placeholder="t('terminal.historySearch')"
        @keydown="onKey"
      />
    </label>
    <div :id="`past-${project.path}`" ref="listbox" class="rows" role="listbox" :aria-label="t('terminal.historyTitle')">
      <div v-if="shown === null" class="note"><Spinner :size="13" /></div>
      <div v-else-if="!shown.length" class="note">{{ t("terminal.historyEmpty") }}</div>
      <div v-else-if="!items.length" class="note">{{ t("terminal.historyNoMatch", { query: query.trim() }) }}</div>
      <template v-for="group in days" :key="group.day">
        <div v-if="group.label" class="day" role="presentation">{{ group.label }}</div>
        <div
          v-for="item in group.items"
          :id="rowId(item.session.id)"
          :key="item.session.id"
          :class="['row', { picked: picked === item.session.id, failed: failed[item.session.id] }]"
          role="option"
          :aria-selected="picked === item.session.id"
          :title="item.name"
          @pointerdown="startDrag($event, item)"
          @click="clickRow(item.session.id)"
        >
          <ClaudeLogo :size="11" :class="{ dim: !live.has(item.session.id) }" />
          <span class="name"><Highlight v-if="item.hits" :text="item.name" :hits="item.hits" /><template v-else>{{ item.name }}</template></span>
          <Spinner v-if="opening === item.session.id" :size="12" />
          <span v-else-if="failed[item.session.id]" class="error">{{ t(failed[item.session.id] === "missing" ? "terminal.historyMissing" : "terminal.historyUnsure") }}</span>
          <span v-else-if="live.get(item.session.id)" class="where">
            <span class="where-text">{{ where(live.get(item.session.id)!) }}</span>
            <ClaudeDot :live="liveDot(live.get(item.session.id)!)" />
          </span>
          <span v-else-if="item.session.at !== null" class="time">{{ clock(item.session.at) }}</span>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* Beside the terminal, its height, on the panel's own ground in the room the panel makes for it
   (its left edge going with that room's as it grows); a narrow panel's lies over the terminal's
   right side, on a ground of its own. */
.history {
  position: absolute;
  top: 6px;
  bottom: 20px;
  left: 12px;
  width: var(--history-width, 296px);
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: hidden;
}
.history.overlay {
  left: auto;
  right: 20px;
  z-index: 1;
  width: min(var(--history-width, 296px), calc(100% - 80px));
  padding: 10px 8px 8px;
  border-radius: 8px;
  background: var(--bg-dialog);
  border: 1px solid var(--line-dialog);
  box-shadow: var(--shadow-menu);
}
.search {
  height: 30px;
  flex-shrink: 0;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  margin-bottom: 4px;
  border-radius: 7px;
  background: var(--bg-input);
  border: 1px solid var(--line-dialog);
  color: var(--text-faint);
  cursor: text;
}
/* No ring when it has the keyboard (the user's word, 2026-10-22): the caret is enough. */
.search input {
  flex: 1 1 auto;
  min-width: 0;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--text);
}
.search input::placeholder {
  color: var(--text-faint);
}
.rows {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}
.note {
  padding: 28px 12px;
  display: flex;
  justify-content: center;
  text-align: center;
  font-size: 12.5px;
  color: var(--text-subtle);
}
.day {
  padding: 10px 8px 3px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-faint);
  white-space: nowrap;
}
.day:first-child {
  padding-top: 4px;
}
:lang(zh) .day,
:lang(ja) .day {
  letter-spacing: 0;
}
.row {
  height: 28px;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 8px;
  border-radius: 6px;
  cursor: pointer;
}
.row:hover {
  background: var(--bg-row-hover);
}
.row.picked {
  background: var(--bg-seg-on);
}
.row .dim {
  opacity: 0.55;
}
.name {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 12.5px;
  color: var(--text-soft);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.row:hover .name,
.row.picked .name {
  color: var(--text-strong);
}
.time {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-faint);
  white-space: nowrap;
}
.where {
  flex-shrink: 1;
  min-width: 0;
  max-width: 45%;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11.5px;
  color: var(--text-faint);
}
.where-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.error {
  flex-shrink: 1;
  min-width: 0;
  max-width: 55%;
  font-size: 11.5px;
  color: var(--crash-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
