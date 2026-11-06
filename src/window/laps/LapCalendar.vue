<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { clockText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import {
  intervalText,
  markTone,
  noonOf,
  reasonText,
  refusalText,
  resultLabel,
  runLength,
  weekdayLong,
  weekdayOf,
  weekdayShort,
  type CalendarDay,
  type DayRow,
  type LapRun,
  type LapView,
  type MarkTone,
  type RunMark,
  type RunResult,
} from "../../lib/laps";
import { dayLong } from "../../lib/period";
import { api, laps as lapsState } from "../../lib/store";
import DayOffDialog from "./DayOffDialog.vue";
import LapMark from "./LapMark.vue";

/**
 * The Takvim popover (TN2, TND1–3), under its button: the month, Monday first, each day's dots
 * (a past day's results in their colours, a day to come its planned runs as hollow dots, a day off
 * its mark), today orange, the day picked ringed. Under it the picked day's jobs: to come with
 * their switches (off asks how, in a dialog; one that is off says how and can be taken back), past
 * as their results (a click opens the run's result). A press outside or Esc closes it.
 */
const props = defineProps<{ laps: LapView[]; today: string; now: number; anchor: HTMLElement | null }>();
const emit = defineEmits<{ close: []; openRun: [lap: string, run: string] }>();

const pad = (n: number): string => String(n).padStart(2, "0");

const [startYear, startMonth] = props.today.split("-").map(Number);
const year = ref(startYear);
/** 1–12, as `lap_calendar` takes it. */
const month = ref(startMonth);
const picked = ref(props.today);

const days = ref<Map<string, CalendarDay>>(new Map());
const rows = ref<DayRow[]>([]);
const error = ref("");

const title = computed(() => new Date(year.value, month.value - 1, 1, 12).toLocaleDateString(language.value, { month: "long", year: "numeric" }));
const weekdays = computed(() => [1, 2, 3, 4, 5, 6, 7].map(weekdayShort));

type Cell = { date: string; day: number; dots: Array<MarkTone | "planned">; off: boolean };

const cells = computed<Array<Cell | null>>(() => {
  const first = `${year.value}-${pad(month.value)}-01`;
  const count = new Date(year.value, month.value, 0).getDate();
  const list: Array<Cell | null> = Array<null>(weekdayOf(first) - 1).fill(null);
  for (let day = 1; day <= count; day++) {
    const date = `${year.value}-${pad(month.value)}-${pad(day)}`;
    const known = days.value.get(date);
    const dots: Cell["dots"] = (known?.results ?? []).slice(0, 3).map(markTone);
    for (let i = 0; i < (known?.planned ?? 0) && dots.length < 3; i++) dots.push("planned");
    list.push({ date, day, dots, off: known?.off ?? false });
  }
  return list;
});

function shift(by: number): void {
  const at = new Date(year.value, month.value - 1 + by, 1);
  year.value = at.getFullYear();
  month.value = at.getMonth() + 1;
}

/* ---------- what the core says of the month and the day ---------- */

let askedMonth = 0;
async function loadMonth(): Promise<void> {
  const ask = ++askedMonth;
  const list = await api.lapCalendar(year.value, month.value).catch(() => null);
  if (ask !== askedMonth || !list) return;
  days.value = new Map(list.map((day) => [day.date, day]));
}

/** The kept runs of the day's jobs, for what a past row says of them; held while it shows. */
const runs = ref<Record<string, LapRun[]>>({});
const summaries = ref<Record<string, string>>({});

let askedDay = 0;
async function loadDay(): Promise<void> {
  const ask = ++askedDay;
  const date = picked.value;
  const list = await api.lapDay(date).catch(() => null);
  if (ask !== askedDay || !list) return;
  rows.value = list;
  const withRuns = list.filter((row) => row.runs.length).map((row) => row.lap);
  const read = await Promise.all(withRuns.map((id) => api.lapRuns(id).then((kept) => [id, kept] as const, () => [id, []] as const)));
  if (ask !== askedDay) return;
  runs.value = Object.fromEntries(read);
  const wanted = read.flatMap(([, kept]) => kept.filter((run) => dayOf(run.due) === date && run.result === "ok" && run.session && run.agent)).map((run) => run.id);
  if (!wanted.length) return;
  const said = await api.lapSummaries(wanted).catch(() => null);
  if (ask === askedDay && said) summaries.value = { ...summaries.value, ...said };
}

function dayOf(at: number): string {
  const d = new Date(at);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

watch([year, month], () => void loadMonth(), { immediate: true });
watch(picked, () => void loadDay(), { immediate: true });

// The jobs changed (a run, a save, a day off): the month and the day as they are now.
let again: ReturnType<typeof setTimeout> | undefined;
watch(
  () => lapsState.value,
  () => {
    clearTimeout(again);
    again = setTimeout(() => {
      void loadMonth();
      void loadDay();
    }, 300);
  },
);

/* ---------- the picked day ---------- */

const isToday = computed(() => picked.value === props.today);
const isPast = computed(() => picked.value < props.today);

const byId = computed(() => new Map(props.laps.map((lap) => [lap.id, lap])));

/** A row of results: one run, or the same job's runs that ended alike. */
type ResultRow = { key: string; lap: string; name: string; run: string; at: number; result: RunResult | null; text: string };

const resultRows = computed<ResultRow[]>(() => {
  if (!isPast.value && !isToday.value) return [];
  const list: ResultRow[] = [];
  for (const row of rows.value) {
    // Runs that ended alike: the same result for the same reason.
    const reasons = new Map((runs.value[row.lap] ?? []).map((run) => [run.id, run.reason]));
    const groups = new Map<string, RunMark[]>();
    for (const mark of row.runs) {
      const key = `${mark.result ?? "going"}:${reasons.get(mark.id) ?? ""}`;
      groups.set(key, [...(groups.get(key) ?? []), mark]);
    }
    for (const [key, marks] of groups) {
      const first = [...marks].sort((a, b) => a.due - b.due)[0];
      const kept = runs.value[row.lap]?.find((run) => run.id === first.id) ?? null;
      const lap = byId.value.get(row.lap);
      let text = kept ? (summaries.value[kept.id] ?? reasonText(kept, lap?.gate.script ?? "none") ?? (kept.endedAt !== null ? runLength(kept.endedAt - kept.startedAt) : "")) : resultLabel(first.result);
      if (marks.length > 1) text = t("laps.cal.together", { text: kept ? (reasonText(kept, lap?.gate.script ?? "none") ?? resultLabel(first.result)) : resultLabel(first.result), count: marks.length });
      list.push({ key: `${row.lap}:${key}`, lap: row.lap, name: row.name, run: first.id, at: first.due, result: first.result, text });
    }
  }
  return list.sort((a, b) => a.at - b.at);
});

/** A row with its switch: a job still to come that day. */
type SwitchRow = { lap: string; name: string; on: boolean; off: "day" | "weekday" | null; at: number | null; times: string; sub: string | null; weekly: boolean };

const switchRows = computed<SwitchRow[]>(() => {
  if (isPast.value) return [];
  const list: SwitchRow[] = [];
  for (const row of rows.value) {
    const lap = byId.value.get(row.lap);
    // The core's moments are those still to come when it was asked (today's after that moment),
    // the first ones; its count is all of them, `last` the last. Those passed since are taken off.
    const coming = row.at.filter((at) => at > props.now);
    const left = Math.max(0, row.count - (row.at.length - coming.length));
    const when = lap?.when;
    // Nothing more to come that day.
    if (!row.off && !left) continue;
    let times: string;
    if (when?.kind === "every") times = when.from || when.to ? `${when.from ?? "00:00"} – ${when.to ?? "24:00"}` : intervalText(when.minutes);
    else if (coming.length && left <= 3) times = coming.map(clock).join(" · ");
    else if (row.last !== null && left) times = `${clock(coming[0] ?? row.last)} – ${clock(row.last)}`;
    else times = "";
    let sub: string | null = null;
    if (!row.off && left > 1 && (when?.kind === "every" || left > 3)) sub = isToday.value ? t("laps.cal.left", { count: left }) : t("laps.cal.runs", { count: left });
    const weekly = !!when && when.kind !== "once" && when.kind !== "refill";
    list.push({ lap: row.lap, name: row.name, on: row.off === null, off: row.off, at: coming[0] ?? null, times, sub, weekly });
  }
  return list.sort((a, b) => (a.at ?? Number.MAX_SAFE_INTEGER) - (b.at ?? Number.MAX_SAFE_INTEGER));
});

const head = computed(() => {
  const day = dayLong(picked.value);
  return isToday.value ? t("laps.cal.today", { day }) : day;
});

const count = computed(() => {
  if (isPast.value) return t("laps.cal.runs", { count: rows.value.reduce((sum, row) => sum + row.runs.length, 0) });
  return t("laps.cal.jobs", { count: rows.value.length });
});

const nowClock = computed(() => clockText(props.now, language.value));

function clock(at: number): string {
  return clockText(at, language.value);
}

/* ---------- the switches ---------- */

/** The day off being asked about: its switch shows off meanwhile. */
const asking = ref<SwitchRow | null>(null);
const busy = ref<string | null>(null);

function flip(row: SwitchRow, event: Event): void {
  const on = (event.target as HTMLInputElement).checked;
  if (!on) {
    asking.value = row;
    return;
  }
  void takeBack(row);
}

/** "geri aç": the day off, or its weekday's, is taken back. */
async function takeBack(row: SwitchRow): Promise<void> {
  if (busy.value) return;
  busy.value = row.lap;
  error.value = "";
  try {
    await api.lapDayOn(row.lap, picked.value);
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    busy.value = null;
    void loadMonth();
    void loadDay();
  }
}

function asked(applied: boolean): void {
  asking.value = null;
  if (applied) {
    void loadMonth();
    void loadDay();
  }
}

function offText(row: SwitchRow): string {
  return row.off === "weekday" ? t("laps.cal.offWeekday", { weekday: weekdayLong(weekdayOf(picked.value)) }) : t("laps.cal.offDay");
}

/* ---------- closing ---------- */

const popover = ref<HTMLElement | null>(null);

function onPress(event: PointerEvent): void {
  const target = event.target as Node | null;
  if (!target) return;
  if (popover.value?.contains(target) || props.anchor?.contains(target)) return;
  // A dialog over it (a result, the day off) is its own.
  if (target instanceof Element && target.closest("dialog[open]")) return;
  emit("close");
}

function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape" || event.defaultPrevented) return;
  if (document.querySelector("dialog[open]")) return;
  event.preventDefault();
  emit("close");
}

onMounted(() => {
  document.addEventListener("pointerdown", onPress, true);
  window.addEventListener("keydown", onKey);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onPress, true);
  window.removeEventListener("keydown", onKey);
  clearTimeout(again);
});

function dayLabel(cell: Cell): string {
  return new Date(noonOf(cell.date)).toLocaleDateString(language.value, { day: "numeric", month: "long", weekday: "long" });
}
</script>

<template>
  <div ref="popover" class="calendar" role="dialog" :aria-label="t('laps.cal.title')">
    <div class="month">
      <div class="month-head">
        <span class="month-title">{{ title }}</span>
        <span class="grow"></span>
        <button type="button" class="icon-button" :aria-label="t('laps.cal.previous')" :title="t('laps.cal.previous')" @click="shift(-1)">
          <Icon name="back" :size="14" />
        </button>
        <button type="button" class="icon-button" :aria-label="t('laps.cal.next')" :title="t('laps.cal.next')" @click="shift(1)">
          <Icon name="chevron" :size="14" />
        </button>
      </div>
      <div class="grid">
        <span v-for="name in weekdays" :key="name" class="weekday">{{ name }}</span>
        <template v-for="(cell, i) in cells" :key="cell?.date ?? `blank-${i}`">
          <button
            v-if="cell"
            type="button"
            :class="['day', { today: cell.date === today, picked: cell.date === picked, past: cell.date < today }]"
            :aria-label="dayLabel(cell)"
            :aria-pressed="cell.date === picked"
            @click="picked = cell.date"
          >
            {{ cell.day }}
            <span class="dots" aria-hidden="true">
              <i v-for="(dot, d) in cell.dots" :key="d" :class="['dot', dot]"></i>
              <span v-if="cell.off" class="off-mark"><Icon name="minus" :size="9" /></span>
            </span>
          </button>
          <span v-else></span>
        </template>
      </div>
    </div>

    <div class="rule"></div>

    <div class="day-list">
      <div class="day-head">
        <!-- One label, so that the space before the count is the one inside the head. -->
        <span class="section-label">{{ head }} <span class="faint">· {{ count }}</span></span>
      </div>

      <p v-if="!resultRows.length && !switchRows.length" class="hint none">{{ t("laps.cal.empty") }}</p>

      <button v-for="row in resultRows" :key="row.key" type="button" class="row result-row" :disabled="row.result === null" @click="emit('openRun', row.lap, row.run)">
        <LapMark :result="row.result" :size="18" />
        <span class="row-text">
          <span class="row-name">{{ row.name }}</span>
          <span v-if="row.text" class="row-sub">{{ row.text }}</span>
        </span>
        <span class="row-time">{{ clock(row.at) }}</span>
        <Icon name="chevron" :size="12" class="row-go" />
      </button>

      <div v-if="isToday && resultRows.length && switchRows.length" class="now-line" aria-hidden="true">
        <span class="now-time">{{ nowClock }}</span>
        <span class="now-rule"></span>
      </div>

      <div v-for="row in switchRows" :key="row.lap" class="row switch-row">
        <input
          type="checkbox"
          role="switch"
          :checked="row.on && asking?.lap !== row.lap"
          :disabled="busy !== null"
          :aria-label="row.name"
          @change="flip(row, $event)"
        />
        <span class="row-text">
          <span class="row-name">{{ row.name }}</span>
          <span v-if="row.off" class="row-sub">
            {{ offText(row) }} ·
            <button type="button" class="link" :disabled="busy !== null" @click="takeBack(row)">{{ t("laps.cal.undo") }}</button>
          </span>
          <span v-else-if="row.sub" class="row-sub">{{ row.sub }}</span>
        </span>
        <span class="row-time">{{ row.times }}</span>
      </div>

      <p v-if="error" class="error pad" role="alert">{{ error }}</p>
    </div>

    <DayOffDialog v-if="asking" :lap="asking.lap" :name="asking.name" :date="picked" :at="asking.at" :weekly="asking.weekly" @close="asked" />
  </div>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.calendar {
  position: absolute;
  top: 52px;
  right: 14px;
  z-index: 30;
  width: 430px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-popover);
  overflow: hidden;
  animation: rise 0.14s ease-out;
}

@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
}

.month {
  flex-shrink: 0;
  padding: 14px 14px 10px;
}

.month-head {
  display: flex;
  align-items: center;
  margin-bottom: 8px;
}

.month-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-strong);
}

.grid {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 3px;
  text-align: center;
}

.weekday {
  padding-bottom: 4px;
  font-size: 11px;
  color: var(--text-faint);
}

.day {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 3px;
  height: 42px;
  padding: 0;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text);
  font-size: 13px;
  font-variant-numeric: tabular-nums;
}

.day.past {
  color: var(--text-muted);
}

.day:hover:not(.today):not(.picked) {
  background: var(--bg-row-hover);
}

.day.today {
  background: var(--claude);
  color: var(--on-claude);
  font-weight: 600;
}

.day.picked {
  box-shadow: inset 0 0 0 1.5px var(--text-strong);
  font-weight: 600;
}

.day.picked:not(.today) {
  background: var(--bg-lit);
  color: var(--text-strong);
}

.dots {
  display: flex;
  align-items: center;
  gap: 2px;
  height: 6px;
}

.dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  box-sizing: border-box;
}

.dot.ok {
  background: var(--run);
}

.dot.bad {
  background: var(--crash);
}

.dot.grey {
  background: var(--claude-idle);
}

.dot.going {
  background: var(--claude);
}

.dot.planned {
  border: 1.2px solid var(--text-faint);
}

.day.today .dot.planned {
  border-color: currentColor;
}

.off-mark {
  display: inline-flex;
  margin-left: 1px;
  color: var(--claude-text);
}

.off-mark :deep(svg) {
  stroke-width: 3;
}

.day.today .off-mark {
  color: currentColor;
}

.rule {
  flex-shrink: 0;
  height: 1px;
  margin: 0 14px;
  background: var(--line);
}

.day-list {
  min-height: 0;
  overflow-y: auto;
  padding-bottom: 8px;
}

.day-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 12px 14px 4px;
}

.faint {
  color: var(--text-faint);
}

.none {
  padding: 6px 14px 4px;
}

.row {
  display: flex;
  align-items: center;
  gap: 11px;
  width: 100%;
  min-height: 40px;
  padding: 0 14px;
  border: 0;
  background: transparent;
  text-align: left;
}

.result-row:hover:not(:disabled) {
  background: var(--bg-row-hover);
}

.result-row:disabled {
  cursor: default;
}

.row-text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.row-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  color: var(--text);
}

.row-sub {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  color: var(--text-faint);
}

.row-time {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  color: var(--text-faint);
  white-space: nowrap;
}

.row-go {
  flex-shrink: 0;
  color: var(--text-faint);
}

.link {
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-link);
  font: inherit;
  text-decoration: underline;
}

.now-line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 14px 2px;
}

.now-time {
  display: flex;
  align-items: center;
  height: 16px;
  padding: 0 5px;
  border-radius: 4px;
  background: var(--claude);
  color: var(--on-claude);
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 600;
}

.now-rule {
  flex-grow: 1;
  height: 1.5px;
  background: var(--claude);
  opacity: 0.6;
}

.pad {
  padding: 6px 14px 0;
}

@media (prefers-reduced-motion: reduce) {
  .calendar {
    animation: none;
  }
}
</style>
