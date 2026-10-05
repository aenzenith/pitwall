<script setup lang="ts">
import { computed } from "vue";

import { language, t } from "../../lib/i18n";
import { keys, primary } from "../../lib/platform";
import { rowKeys } from "../../lib/rows";
import { SECTION_LABELS, sessionTitle, spendText, statusLine, type Layout } from "../../lib/sessions";
import type { SessionRow } from "../../lib/types";
import OriginLabel from "./OriginLabel.vue";

/**
 * The sessions, section by section, as one Tab stop (lib/rows): ↑/↓, Home and End move, ↵
 * brings the session up, Space marks its project seen, ⌘↵ (Ctrl+Enter) opens the project, ⌘C
 * (Ctrl+C) copies the command that resumes it. `hold` says when the pointer or the keyboard is on
 * the list, so the page keeps its order still meanwhile. `plain`: the rows without their tokens
 * (today's alone, so left out when the page looks back).
 */
const props = defineProps<{ layout: Layout; rows: Map<string, SessionRow>; selected: string | null; now: number; label: string; plain?: boolean }>();
const emit = defineEmits<{
  select: [id: string];
  bringUp: [id: string];
  markSeen: [id: string];
  openProject: [id: string];
  copy: [id: string];
  menu: [event: MouseEvent, id: string];
  hold: [on: boolean];
}>();

function onKey(event: KeyboardEvent): void {
  const target = event.target as HTMLElement;
  const row = target.closest<HTMLElement>("[data-path]");
  if (row && target === row && primary(event) && !event.shiftKey) {
    const id = row.dataset.path ?? "";
    if (event.key === "Enter") {
      event.preventDefault();
      emit("openProject", id);
      return;
    }
    if (event.code === "KeyC") {
      event.preventDefault();
      emit("copy", id);
      return;
    }
  }
  rowKeys(event, {
    move: (id) => emit("select", id),
    enter: (id) => emit("bringUp", id),
    space: (id) => emit("markSeen", id),
  });
}

let pointer = false;
let focus = false;

function hold(): void {
  emit("hold", pointer || focus);
}

function onPointer(on: boolean): void {
  pointer = on;
  hold();
}

function onFocusOut(event: FocusEvent): void {
  const to = event.relatedTarget as Node | null;
  if (to && (event.currentTarget as HTMLElement).contains(to)) return;
  focus = false;
  hold();
}

function onFocusIn(): void {
  focus = true;
  hold();
}

/** The sections as drawn: each session with its name, status and spend worked out once. */
const groups = computed(() =>
  props.layout.map((group) => ({
    section: group.section,
    // Looking back, the ended ones didn't end today.
    label: `${t(props.plain && group.section === "ended" ? "sessions.filter.endedPast" : SECTION_LABELS[group.section])} · ${group.ids.length}`,
    items: group.ids.flatMap((id) => {
      const row = props.rows.get(id);
      return row ? [{ id, row, title: sessionTitle(row), status: statusLine(row, props.now), spend: spendText(row.spend, language.value) }] : [];
    }),
  })),
);

/** Right-click: the row is selected first, so the menu is about what the details show. */
function onMenu(event: MouseEvent, id: string): void {
  event.preventDefault();
  emit("select", id);
  emit("menu", event, id);
}

const keyHint = (): string => t("sessions.rowKeys", { open: keys("mod+Enter"), copy: keys("mod+KeyC") });
</script>

<template>
  <span id="session-keys" class="sr-only">{{ keyHint() }}</span>
  <div :class="['grid', { plain }]" role="grid" :aria-label="label" aria-describedby="session-keys">
    <div
      class="rows"
      role="rowgroup"
      @keydown="onKey"
      @pointerenter="onPointer(true)"
      @pointerleave="onPointer(false)"
      @focusin="onFocusIn"
      @focusout="onFocusOut"
    >
      <template v-for="group in groups" :key="group.section">
        <div class="section" role="row">
          <span role="columnheader" class="section-label">{{ group.label }}</span>
        </div>
        <div
          v-for="item in group.items"
          :key="item.id"
          role="row"
          :data-path="item.id"
          :tabindex="item.id === selected ? 0 : -1"
          :aria-selected="item.id === selected"
          :class="['srow', group.section, { selected: item.id === selected }]"
          :title="t('sessions.rowTitle')"
          @click="emit('select', item.id)"
          @dblclick="emit('bringUp', item.id)"
          @contextmenu="onMenu($event, item.id)"
        >
          <span :class="['mark', item.row.phase]" aria-hidden="true"></span>
          <span class="names" role="gridcell">
            <span :class="['title', { untitled: !item.row.title }]">{{ item.title }}</span>
            <span class="meta">
              <span class="project">{{ item.row.project }}</span>
              <!-- Where an ended one ran, when that is known: of an earlier day's it isn't. -->
              <template v-if="item.row.phase !== 'ended' || item.row.origin.kind !== 'unknown'">
                <span aria-hidden="true">·</span>
                <OriginLabel :origin="item.row.origin" :project="item.row.project" />
              </template>
            </span>
          </span>
          <span :class="['status', item.status.tone]" role="gridcell">{{ item.status.text }}</span>
          <span class="spend" role="gridcell">{{ item.spend }}</span>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* Its width sets the rows' layout: narrower, the spend and then the status's own column go. */
.grid {
  container-type: inline-size;
}

.rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.section {
  padding: 12px 12px 6px;
}

.section:first-child {
  padding-top: 4px;
}

.srow {
  display: grid;
  /* Fixed outer columns, so every row's status starts at the same place. */
  grid-template-columns: 16px minmax(0, 1.35fr) minmax(0, 1fr) var(--spend-column, 156px);
  grid-template-areas: "mark names status spend";
  gap: 12px;
  align-items: center;
  padding: 8px 12px;
  border-radius: 8px;
  cursor: pointer;
}

.srow:hover {
  background: #1b1e24;
}

.srow.selected {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px #2c313a;
}

/* The keyboard's row: the ring inside, so neighbours and the list's edge never cut it. */
.srow:focus-visible {
  outline-offset: -2px;
}

/* Claude's state, by shape as well as colour: a filled dot waits on you, a breathing ring works,
   a small grey ring is open or ended. */
.mark {
  grid-area: mark;
  justify-self: center;
  width: 9px;
  height: 9px;
  box-sizing: border-box;
  border-radius: 50%;
}

.mark.waiting {
  background: var(--claude);
}

.mark.working {
  border: 1.5px solid var(--claude);
  animation: breathe 1.6s ease-in-out infinite;
}

.mark.idle,
.mark.ended {
  width: 7px;
  height: 7px;
  border: 1.5px solid var(--idle-ring);
}

@keyframes breathe {
  50% {
    opacity: 0.35;
  }
}

@media (prefers-reduced-motion: reduce) {
  .mark.working {
    animation: none;
  }
}

.names {
  grid-area: names;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.title {
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.title.untitled {
  color: var(--text-muted);
}

.srow.ended .title {
  color: var(--text-subtle);
}

.meta {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.project {
  flex-shrink: 1;
  min-width: 0;
  max-width: 60%;
  overflow: hidden;
  text-overflow: ellipsis;
}

.meta .origin {
  flex-shrink: 1;
  overflow: hidden;
}

.status {
  grid-area: status;
  min-width: 0;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.status.hot {
  color: var(--claude-text);
}

.status.live {
  color: #c7ccd3;
}

.status.quiet {
  color: var(--text-subtle);
}

.spend {
  grid-area: spend;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Without the tokens, their column's room goes to the status. */
.plain {
  --spend-column: 0px;
}

.plain .spend {
  display: none;
}

/* Narrower: no spend (the details have it). */
@container (max-width: 540px) {
  .srow {
    grid-template-columns: 16px minmax(0, 1.2fr) minmax(0, 1fr);
    grid-template-areas: "mark names status";
  }

  .spend {
    display: none;
  }
}

/* Narrowest: the status goes under the name. */
@container (max-width: 380px) {
  .srow {
    grid-template-columns: 16px minmax(0, 1fr);
    grid-template-areas:
      "mark names"
      ". status";
    row-gap: 3px;
    column-gap: 10px;
  }

  .mark {
    align-self: start;
    margin-top: 5px;
  }
}
</style>
