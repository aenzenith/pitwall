<script setup lang="ts">
import { computed, nextTick, ref } from "vue";

import ClaudeDot from "../../components/ClaudeDot.vue";
import Icon from "../../components/Icon.vue";
import { parseKey, rowKey, useGroupReorder, type Drop } from "../../lib/groupReorder";
import { t } from "../../lib/i18n";
import { countdown, dayOffTag, markTone, phaseShort, phaseText, phaseTone, resultLabel, runMoment, scheduled, towerPart, type LapView, type RunMark } from "../../lib/laps";

/**
 * The tower (TN1): the jobs on the schedule in the user's order, numbered; Kalan counting down
 * each second (a run in progress says what it is at instead; the next moment of all in the laps'
 * orange within the hour); "Son 10" the last ten results as squares, the newest at the right,
 * each opening that run's result; a run in progress opens its dialog on its terminal from its
 * words. The paused jobs and those that are off at the foot, dimmed. A row is picked whole; the
 * arrows move the pick. A row dragged moves among the rows of its own part (`move`), as the
 * project list's rows do. An orange dot beside a job's name: a result of it not opened yet. Under
 * the rows, a faint Yeni iş, which never moves.
 */
const props = defineProps<{ laps: LapView[]; selected: string | null; now: number; today: string; elsewhere: boolean }>();
const emit = defineEmits<{ select: [id: string]; openRun: [lap: string, run: string]; new: []; move: [moved: string, beside: string, after: boolean] }>();

type Row = {
  lap: LapView;
  /** Its place in the order, for a job on the schedule; null for one paused or off. */
  place: number | null;
  tag: string | null;
  /** Kalan: the countdown, or what the run in progress is at (a short word for it: the column is
   * narrow; `full`, its whole words, as the tooltip); `why`, a longer word for a missing one. */
  left: { text: string; live: "waiting" | "working" | null; soon: boolean; why?: string; full?: string };
  /** What stands in Son 10's place for a job off the schedule. */
  state: string | null;
  squares: Array<RunMark | null>;
};

/** The next moment of all the jobs on the schedule that don't run now: its row's countdown is the
 * laps' orange within the hour. */
const soonest = computed(() => Math.min(...props.laps.filter((lap) => scheduled(lap) && !lap.running && lap.nextAt !== null).map((lap) => lap.nextAt as number)));

const rows = computed<Row[]>(() => {
  let place = 0;
  return props.laps.map((lap) => {
    const on = scheduled(lap);
    if (on) place++;
    const last = lap.last.slice(-10);
    const squares: Array<RunMark | null> = [...Array<null>(10 - last.length).fill(null), ...last];
    let left: Row["left"] = { text: "—", live: null, soon: false };
    if (lap.running) left = { text: phaseShort(lap.running, lap), full: phaseText(lap.running, lap), live: phaseTone(lap.running), soon: false };
    else if (on && lap.nextAt !== null) {
      const ms = lap.nextAt - props.now;
      left = { text: countdown(ms), live: null, soon: lap.nextAt === soonest.value && ms < 3_600_000 };
    } else if (on && lap.when.kind === "refill" && lap.noReading) left = { text: t("popover.fuel.unread"), live: null, soon: false, why: t("laps.reason.noReading") };
    let state: string | null = null;
    if (lap.paused === "failures") state = t("laps.paused.failures", { count: Math.max(lap.streak, lap.pauseAfter ?? 0) });
    else if (lap.paused === "user") state = t("laps.paused.user");
    else if (!lap.on) state = t("laps.off");
    return { lap, place: on ? place : null, tag: dayOffTag(lap, props.today), left, state, squares };
  });
});

const list = ref<HTMLElement | null>(null);

/** Drag to reorder, as the project list (lib/groupReorder): a row among the rows of its part
 * alone (`data-zone`), held at its part's edge when the pointer goes beyond it. Not while
 * another Pitwall holds the laps: the core would refuse the order. */
const drag = useGroupReorder(list, (change: Drop) => emit("move", parseKey(change.moved).id, parseKey(change.beside).id, change.after));

function press(event: PointerEvent, id: string): void {
  if (!props.elsewhere) drag.down(event, rowKey(id));
}

function pick(id: string): void {
  if (drag.isClick()) emit("select", id);
}

/** ↑/↓ (and Home/End) move the pick, the keyboard going with it. */
function onKey(event: KeyboardEvent): void {
  const at = props.laps.findIndex((lap) => lap.id === props.selected);
  let to = at;
  if (event.key === "ArrowDown") to = Math.min(props.laps.length - 1, at + 1);
  else if (event.key === "ArrowUp") to = Math.max(0, at - 1);
  else if (event.key === "Home") to = 0;
  else if (event.key === "End") to = props.laps.length - 1;
  else return;
  event.preventDefault();
  const next = props.laps[to];
  if (!next) return;
  emit("select", next.id);
  void nextTick(() => list.value?.querySelector<HTMLElement>(`[data-lap="${next.id}"]`)?.focus({ preventScroll: false }));
}

function squareLabel(mark: RunMark): string {
  return `${resultLabel(mark.result)} · ${runMoment(mark.due)}`;
}
</script>

<template>
  <div class="tower">
    <div class="tower-head" aria-hidden="true">
      <span class="section-label">{{ t("laps.tower.place") }}</span>
      <span class="section-label">{{ t("laps.tower.job") }}</span>
      <span class="section-label left-head">{{ t("laps.tower.left") }}</span>
      <span class="section-label last-head">{{ t("laps.tower.last") }}</span>
    </div>
    <!-- Its rows are its own children, one under the other: the drag (lib/groupReorder) counts on it. -->
    <div ref="list" :class="['tower-rows', drag.listClass()]" role="listbox" :aria-label="t('laps.title')" @keydown="onKey">
      <div
        v-for="row in rows"
        :key="row.lap.id"
        :data-lap="row.lap.id"
        :data-path="row.lap.id"
        :data-zone="towerPart(row.lap)"
        :class="['tower-row', { on: row.lap.id === selected, dim: row.place === null }, drag.lineClass(rowKey(row.lap.id))]"
        :style="drag.lineStyle(rowKey(row.lap.id))"
        role="option"
        :aria-selected="row.lap.id === selected"
        :tabindex="row.lap.id === selected ? 0 : -1"
        @pointerdown="press($event, row.lap.id)"
        @click="pick(row.lap.id)"
      >
        <span v-if="row.place !== null" class="place">{{ row.place }}</span>
        <span v-else class="place idle"><Icon :name="row.lap.paused ? 'pause' : 'minus'" :size="11" /></span>

        <span class="job">
          <span class="name" :title="row.lap.name">{{ row.lap.name }}</span>
          <span v-if="row.lap.unread" class="unread" role="img" :aria-label="t('laps.unread', { count: row.lap.unread })" :title="t('laps.unread', { count: row.lap.unread })"></span>
          <span v-if="row.tag" class="pill tag">{{ row.tag }}</span>
        </span>

        <!-- A run in progress: its words open its dialog, on its terminal; the row still picks. -->
        <span
          v-if="row.lap.running"
          :class="['left', 'live', 'open']"
          role="button"
          data-no-drag
          :title="`${row.left.full}\n${t('laps.runs.terminal')}`"
          @click.stop="emit('openRun', row.lap.id, row.lap.running.run)"
        >
          <ClaudeDot :live="{ phase: row.left.live ?? 'working', text: row.left.text }" />
          <span class="left-text">{{ row.left.text }}</span>
        </span>
        <span v-else :class="['left', { soon: row.left.soon, live: row.left.live, said: row.left.why }]" :title="row.left.why ?? row.left.full">
          <ClaudeDot v-if="row.left.live" :live="{ phase: row.left.live, text: row.left.text }" />
          <span class="left-text">{{ row.left.text }}</span>
        </span>

        <span v-if="row.state" class="last"><span class="pill">{{ row.state }}</span></span>
        <span v-else class="last squares">
          <template v-for="(mark, i) in row.squares" :key="mark?.id ?? `none-${i}`">
            <button
              v-if="mark"
              type="button"
              tabindex="-1"
              :class="['square', markTone(mark.result)]"
              data-no-drag
              :aria-label="squareLabel(mark)"
              :title="squareLabel(mark)"
              @click.stop="mark.result === null ? emit('select', row.lap.id) : emit('openRun', row.lap.id, mark.id)"
            ></button>
            <span v-else class="square none" aria-hidden="true"></span>
          </template>
        </span>
      </div>
    </div>
    <!-- The next row's place: a new job, as the head's Yeni iş. -->
    <button type="button" class="tower-new" @click="emit('new')">
      <span class="new-plus"><Icon name="plus" :size="13" /></span>
      <span class="new-text">{{ t("laps.edit.new") }}</span>
    </button>
  </div>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.tower {
  flex: 1 1 560px;
  min-width: 0;
  max-width: 620px;
  display: flex;
  flex-direction: column;
  padding: 10px 8px 0 18px;
  container-type: inline-size;
}

.tower-head,
.tower-row,
.tower-new {
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr) 104px 142px;
  align-items: center;
  padding: 0 12px;
}

.tower-head {
  height: 30px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--line);
}

.left-head {
  text-align: right;
  padding-right: 14px;
}

/* As tall as its rows, up to the room there is: the new job's row comes right under them. */
.tower-rows {
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 0 0;
}

/* The line between rows is straight and its own, not the row's border, which its rounded corners
   would bend; it gives way beside a row that is lit (picked, under the pointer, carried), whose
   rounded ground stands alone. */
.tower-row {
  position: relative;
  height: 58px;
  border-radius: 7px;
  cursor: pointer;
}

.tower-row::after {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 1px;
  background: var(--line-soft);
  pointer-events: none;
}

.tower-row.on::after,
.tower-row:has(+ .tower-row.on)::after,
.tower-row.dragging::after,
.tower-rows:not(.reordering) .tower-row:hover::after,
.tower-rows:not(.reordering) .tower-row:has(+ .tower-row:hover)::after {
  background: transparent;
}

.tower-row:hover:not(.on, .dragging) {
  background: var(--bg-row-hover);
}

.tower-row.on {
  background: var(--bg-selected);
}

.tower-row:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.tower-row.dim {
  opacity: 0.55;
}

.tower-row.dim.on {
  opacity: 0.8;
}

/* Drag to reorder, as the project list's rows (lib/groupReorder): the row lifts and follows the
   pointer, the others of its part slide aside to show where it lands, and on release it slides
   into place. */
.tower-rows.reordering .tower-row {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.tower-rows.reordering .tower-row:hover:not(.on, .dragging) {
  background: transparent;
}

/* The shadow falls below the row only: sideways it would be cut at the list's edges. */
.tower-row.dragging {
  z-index: 2;
  background: var(--bg-selected);
  box-shadow:
    0 14px 22px -14px var(--shade-drag),
    inset 0 0 0 1px var(--line-selected);
}

.tower-rows.reordering .carried {
  transition: none;
}

.tower-rows.reordering .carried.settling {
  transition: transform 160ms ease-out;
}

.tower-rows.frozen .tower-row {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .tower-rows.reordering .tower-row,
  .tower-rows.reordering .carried.settling {
    transition: none;
  }
}

.place {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: 5px;
  background: var(--bg-lit);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 13px;
  font-weight: 500;
}

.tower-row.on .place:not(.idle) {
  background: var(--text-strong);
  color: var(--bg-app);
}

.place.idle {
  background: transparent;
  border: 1px solid var(--line-strong);
  color: var(--text-muted);
}

.job {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13.5px;
  font-weight: 500;
  color: var(--text-strong);
}

.tower-row.dim .name {
  font-weight: 400;
  color: var(--text-soft);
}

/* A result not opened yet: the laps' orange. */
.unread {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--claude);
}

/* Faint until the pointer or the keyboard comes: a row of the list, not a button of its own. */
.tower-new {
  flex-shrink: 0;
  height: 58px;
  margin: 0 0 12px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-faint);
  font-size: 13px;
  text-align: left;
}

.tower-new:hover {
  background: var(--bg-row-hover);
  color: var(--text-soft);
}

.tower-new:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
  color: var(--text-soft);
}

.new-plus {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
}

.new-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.left {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  min-width: 0;
  padding-right: 14px;
  font-family: var(--font-mono);
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  color: var(--text);
}

.left-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.left.soon {
  color: var(--claude-text);
}

/* Why there is no countdown, in words. */
.left.said {
  font-family: var(--font);
  font-size: 12px;
  color: var(--text-soft);
}

/* A run in progress: its words, in the agent's colour. */
.left.live {
  font-family: var(--font);
  font-size: 12px;
  color: var(--claude-text);
}

.left.open:hover .left-text {
  text-decoration: underline;
  text-underline-offset: 2px;
}

.last {
  display: flex;
  align-items: center;
  min-width: 0;
}

.squares {
  gap: 2px;
}

.square {
  flex-shrink: 0;
  width: 11px;
  height: 18px;
  padding: 0;
  border: 0;
  border-radius: 2px;
}

.square.ok {
  background: var(--run);
}

.square.bad {
  background: var(--crash);
}

.square.grey {
  background: var(--claude-idle);
}

.square.going {
  background: var(--claude);
}

.square.none {
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line);
}

button.square:hover {
  box-shadow:
    0 0 0 2px var(--bg-app),
    0 0 0 3.5px var(--text-strong);
}

/* Narrow: the squares go first, then Kalan's room shrinks. */
@container (max-width: 470px) {
  .tower-head,
  .tower-row,
  .tower-new {
    grid-template-columns: 38px minmax(0, 1fr) 96px 0;
  }

  .last,
  .last-head {
    display: none;
  }
}
</style>
