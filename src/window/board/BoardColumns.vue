<script setup lang="ts">
import { computed, ref } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import { COLUMN_LABELS, COLUMNS, columnCards, type BoardLayout, type CardStatus } from "../../lib/board";
import type { DropTarget, Ghost } from "../../lib/cardDrag";
import { t } from "../../lib/i18n";
import { useSlide } from "../../lib/slide";
import type { BoardColumn, Card, SessionRow } from "../../lib/types";
import BoardCard from "./BoardCard.vue";
import type { CardBusy } from "./context";

/**
 * One project's board: up next, with Claude, to review, done today. `layout`: the four side by
 * side, a lane each (`columns`), or one under the other, a card a row (`list`). While a card is
 * dragged (`dragging`), the column under the pointer shows where it would land (`target`), and its
 * cards slide aside to make that room; a card that changes places any other way has its
 * neighbours slide too.
 */
const props = defineProps<{
  path: string;
  layout: BoardLayout;
  cards: Card[];
  now: number;
  selected: string | null;
  statuses: Map<string, CardStatus | null>;
  rowOf: (card: Card) => SessionRow | null;
  busy: Record<string, CardBusy>;
  errors: Record<string, string>;
  dragging: string | null;
  target: DropTarget | null;
  ghost: Ghost | null;
}>();
const emit = defineEmits<{ add: [] }>();

type Entry = { key: string; card: Card | null };

/**
 * The dragged card's place among `cards`, a column's (the card itself left out); -1 when it isn't
 * over that column. Where the pointer is, in every column; only a card given to Claude (one from
 * another column) joins the end of Claude's, as the core puts it.
 */
function slotIndex(column: BoardColumn, cards: Card[]): number {
  if (!props.dragging || props.target?.column !== column) return -1;
  if (column === "claude" && !cards.some((card) => card.id === props.dragging)) return cards.length;
  return props.target.index;
}

/** A column's cards, with the empty place a dragged card would take. */
const columns = computed(() =>
  COLUMNS.map((column) => {
    const cards = columnCards(props.cards, props.path, column, props.now);
    const entries: Entry[] = [];
    const slotAt = slotIndex(column, cards);
    let at = 0;
    for (const card of cards) {
      if (card.id !== props.dragging) {
        if (at === slotAt) entries.push({ key: "slot", card: null });
        at++;
      }
      entries.push({ key: card.id, card });
    }
    if (slotAt >= at) entries.push({ key: "slot", card: null });
    return { column, cards, entries, count: cards.filter((card) => card.id !== props.dragging).length };
  }),
);

function label(column: BoardColumn): string {
  return column === "done" ? t("board.doneToday") : t(COLUMN_LABELS[column]);
}

/**
 * The dragged card's room where it would land: its own height to the fraction, so nothing shifts
 * as it is picked up or takes that place. A card from another column gets a done row's room in
 * the done column (the style's own: no height given).
 */
function slotHeight(column: BoardColumn, cards: Card[]): string | undefined {
  if (column === "done" && !cards.some((card) => card.id === props.dragging)) return undefined;
  return `${props.ghost?.height ?? 40}px`;
}

/* ---------- cards slide ---------- */

const root = ref<HTMLElement | null>(null);

/** Every column's cards and the place shown, in their order. A card is known by its column too:
 * it slides within its column, and just takes its place in a new one (a column cuts off what
 * leaves it). In a list the column's own box grows and shrinks with them (`data-slide-box`). */
const order = computed(() => columns.value.map((col) => col.entries.map((entry) => entry.key).join(" ")).join("\n"));
const slideKey = (column: BoardColumn, id: string): string => `${column} ${id}`;

useSlide(root, order);
</script>

<template>
  <!-- As a list the whole board scrolls; as columns each of them does. -->
  <div ref="root" :class="['bcols', layout]" :data-column-body="layout === 'list' ? '' : undefined">
    <section v-for="(col, x) in columns" :key="col.column" :class="['bcol', col.column]" :data-column="col.column" :data-colx="x" :aria-labelledby="`bcol-${col.column}`">
      <div class="bcol-top">
        <h2 :id="`bcol-${col.column}`" class="bcol-head">
          <ClaudeLogo v-if="col.column === 'claude'" :size="12" />
          <span class="bcol-name">{{ label(col.column) }}</span>
          <span class="bcol-count">{{ col.count }}</span>
        </h2>
        <button v-if="col.column === 'queued'" type="button" class="badd" :title="t('board.addShortTitle')" :aria-label="t('board.addShortTitle')" @click="emit('add')">
          <Icon name="plus" :size="12" />
          <span class="badd-text">{{ t("board.addShort") }}</span>
        </button>
      </div>
      <div :class="['bcol-body', { none: !col.entries.length }]" :data-column-body="layout === 'columns' ? '' : undefined" data-slide-box>
        <div class="bcol-list" role="list" :aria-labelledby="`bcol-${col.column}`">
          <template v-for="entry in col.entries" :key="entry.key">
            <div v-if="!entry.card" :class="['bslot', col.column]" :style="{ height: slotHeight(col.column, col.cards) }" data-slot aria-hidden="true"></div>
            <BoardCard
              v-else
              :data-slide="slideKey(col.column, entry.card.id)"
              :card="entry.card"
              :variant="col.column === 'done' ? 'done' : layout === 'list' ? 'row' : 'full'"
              :status="statuses.get(entry.card.id) ?? null"
              :row="rowOf(entry.card)"
              :selected="entry.card.id === selected"
              :busy="busy[entry.card.id] ?? null"
              :error="errors[entry.card.id] ?? ''"
              :lifted="entry.card.id === dragging"
              draggable
            />
          </template>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
/* Four lanes side by side, each as tall as the board; a long one scrolls on its own. */
.bcols {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  grid-template-rows: minmax(0, 1fr);
  padding: 0 8px;
}

/* The line under the lanes' heads, the board's whole width: level with the one under the
   details' head (100px: the bar's 56 and a head's 44). */
.bcols.columns::before {
  content: "";
  position: absolute;
  top: 43px;
  left: 0;
  right: 0;
  height: 1px;
  background: var(--line);
}

.bcol {
  min-height: 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.columns .bcol + .bcol {
  border-left: 1px solid #1e2127;
}

.bcol-top {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  height: 44px;
  padding: 0 12px;
}

.bcol-head {
  flex: 1 1 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  margin: 0;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
  white-space: nowrap;
}

:lang(zh) .bcol-head,
:lang(ja) .bcol-head {
  letter-spacing: 0;
}

.bcol-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bcol-count {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-weight: 400;
  letter-spacing: 0;
}

/* Only the lane scrolls, never the page. */
.bcol-body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  overflow-x: hidden;
  overflow-y: auto;
}

.bcol-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.bcol.done .bcol-list {
  gap: 2px;
}

/* As a list: the same four one under the other, a head's cards right under it and a line the
   board's whole width over each head but the first (the bar's own is over that one); the list
   scrolls as one. */
.bcols.list {
  display: flex;
  flex-direction: column;
  padding: 0 0 12px;
  overflow-x: hidden;
  overflow-y: auto;
}

.list .bcol {
  flex-shrink: 0;
}

.list .bcol-top {
  padding: 0 20px;
}

.list .bcol + .bcol .bcol-top {
  border-top: 1px solid var(--line);
}

.list .bcol-body {
  flex: none;
  gap: 2px;
  padding: 0 12px 8px;
  overflow: visible;
}

/* With no card it takes no room: the next head's line follows this head at once. */
.list .bcol-body.none {
  padding-bottom: 0;
}

.list .bcol-list {
  gap: 2px;
}

/* A done row ends where the rows above it do. */
.list .bcard.done {
  padding-left: 8px;
  padding-right: 8px;
}

/* Where a dragged card would land. */
.bslot {
  flex-shrink: 0;
  border: 1px dashed var(--line-strong);
  border-radius: var(--radius-card);
  background: rgba(255, 255, 255, 0.02);
}

.list .bslot {
  border-radius: var(--radius-control);
}

/* A done row's room: its padding and one line of its text. */
.bslot.done {
  padding: 8px 6px;
  border-radius: var(--radius-control);
  font-size: 12.5px;
}

.bslot.done::before {
  content: "\200b";
}

.bslot.claude {
  border-color: var(--claude-line);
  background: var(--claude-bg);
}

/* At the right end of Up next's head, in both layouts: a card is written from there. It looks as
   the "Add" in a project's Links and Commands heads does (detail.css: text-button). */
.badd {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  height: 24px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.badd:hover {
  background: #262a33;
  color: var(--text-strong);
}

.badd svg {
  flex-shrink: 0;
}

.badd-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
