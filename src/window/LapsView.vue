<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { dayName } from "../lib/day";
import { showFiles } from "../lib/explorer";
import { t } from "../lib/i18n";
import { lapAsked, scheduled, towerOrder, towerPart, type LapPlace, type LapView } from "../lib/laps";
import { useNativeMenu, type MenuEntry, type MenuPoint } from "../lib/nativeMenu";
import { panelPath, usePanelFollow } from "../lib/panel";
import { dayLong } from "../lib/period";
import { dragRegion, isMac, isWindows } from "../lib/platform";
import { api, laps, loadLaps, snapshot, visible } from "../lib/store";
import DayOffDialog from "./laps/DayOffDialog.vue";
import LapCalendar from "./laps/LapCalendar.vue";
import LapDeleteDialog from "./laps/LapDeleteDialog.vue";
import LapDetail from "./laps/LapDetail.vue";
import LapEditDialog from "./laps/LapEditDialog.vue";
import LapResultDialog from "./laps/LapResultDialog.vue";
import LapTower from "./laps/LapTower.vue";

/**
 * Turlar (TN1): the jobs that run on a schedule while Pitwall runs. Left the tower, in the user's
 * order, counting down each second while the window shows; right the job picked, with its runs.
 * Takvim opens the month over it; Yeni iş, under the tower (on the empty page, in its middle),
 * the wide dialog: the head has no second one (the user's word, 2026-11-06). The panel follows
 * the job picked, its project or the folder it runs in; a run's own terminal shows in its dialog
 * alone. A right-click on a job in the tower picks it and opens its menu (`towerMenu`).
 */

const emit = defineEmits<{ openProject: [path: string] }>();

onMounted(() => void loadLaps());

const state = laps;
const elsewhere = computed(() => state.value?.elsewhere ?? false);
const ordered = computed(() => towerOrder(state.value?.laps ?? []));
const byId = computed(() => new Map((state.value?.laps ?? []).map((lap) => [lap.id, lap])));

/* ---------- the clock: each second, while the window shows ---------- */

const now = ref(Date.now());
let ticker: number | undefined;

function runClock(on: boolean): void {
  window.clearInterval(ticker);
  ticker = undefined;
  if (!on) return;
  now.value = Date.now();
  ticker = window.setInterval(() => (now.value = Date.now()), 1000);
}

watch(visible, runClock, { immediate: true });
onBeforeUnmount(() => window.clearInterval(ticker));

const today = computed(() => dayName(now.value));

/* ---------- the pick ---------- */

const selectedId = ref<string | null>(null);
/** A job just saved: picked before the core's next `laps` has it. */
let awaited: string | null = null;

watch(
  ordered,
  (list) => {
    if (selectedId.value && list.some((lap) => lap.id === selectedId.value)) {
      if (awaited === selectedId.value) awaited = null;
      return;
    }
    if (selectedId.value && selectedId.value === awaited) return;
    selectedId.value = list[0]?.id ?? null;
  },
  { immediate: true },
);

const selected = computed<LapView | null>(() => (selectedId.value ? (byId.value.get(selectedId.value) ?? null) : null));

// The panel shows the job's project or folder; never a run's tab, which is the laps' own.
usePanelFollow(() => (selected.value && selected.value.place.path ? { path: selected.value.place.path, tab: null } : null));

/* ---------- the dialogs ---------- */

const editing = ref<{ lap: LapView | null } | null>(null);
const result = ref<{ run: string; lap: string } | null>(null);
const removing = ref<LapView | null>(null);
const calendarOpen = ref(false);
const calendarButton = ref<HTMLButtonElement | null>(null);

/** A new job's place: the project the panel shows, else the picked job's, else the first listed. */
const newPlace = computed<LapPlace | null>(() => {
  const path = panelPath.value;
  if (path && snapshot.value?.projects.some((p) => p.path === path)) return { kind: "project", path };
  if (selected.value) return selected.value.place;
  return null;
});

/* ---------- read: a result opened is read ---------- */

/** Whether `run` of the job `lap` is unread as the laps' state has it: by its mark among the last
 * ten, else (an older one) by whether the job has any unread run, the core telling the rest. */
function unread(lap: string, run: string): boolean {
  const view = byId.value.get(lap);
  if (!view) return false;
  const mark = view.last.find((m) => m.id === run);
  return mark ? mark.unread : view.unread > 0;
}

/** The runs asked to be read: a run read stays read, so each is asked once. */
const reading = new Set<string>();

function read(run: string): void {
  if (reading.has(run)) return;
  reading.add(run);
  void api.lapRead(run).catch(() => reading.delete(run));
}

/** A run's dialog: its result, or, while it goes, its terminal. */
function openRun(lap: string, run: string): void {
  result.value = { run, lap };
  if (unread(lap, run)) read(run);
}

// A run asked for from outside the page (a lap's banner, lib/laps: `lapAsked`): its job picked and
// its dialog open, once the jobs are here.
watch(
  [lapAsked, state],
  ([run, now]) => {
    if (!run || !now) return;
    lapAsked.value = null;
    const lap = now.laps.find((one) => one.running?.run === run || one.last.some((mark) => mark.id === run));
    if (!lap) return;
    selectedId.value = lap.id;
    openRun(lap.id, run);
  },
  { immediate: true },
);

// A run whose result shows that ends meanwhile (the user watched it) is read at once.
watch(
  () => (result.value && byId.value.get(result.value.lap)?.last.some((mark) => mark.id === result.value?.run && mark.unread) ? result.value.run : null),
  (run) => {
    if (run) read(run);
  },
);

function saved(id: string): void {
  awaited = id;
  selectedId.value = id;
  void loadLaps();
}

function deleted(): void {
  void loadLaps();
}

/* ---------- the tower's right-click menu ---------- */

const detail = ref<InstanceType<typeof LapDetail> | null>(null);
const menu = useNativeMenu();
/** "Bir günü izinli yap": the day picked in the menu, asked about as the calendar asks it. */
const dayOff = ref<{ lap: string; name: string; date: string; at: number | null; weekly: boolean } | null>(null);

const revealKey = isMac ? "laps.menu.reveal.mac" : isWindows ? "laps.menu.reveal.windows" : "laps.menu.reveal.linux";

/** The days of the coming week the job still runs on and that aren't off yet, each with its first
 * moment still to come, as the calendar's day rows have them (`lap_day`). */
async function runDays(lap: LapView): Promise<Array<{ date: string; at: number }>> {
  if (!scheduled(lap) || lap.nextAt === null) return [];
  const from = new Date();
  const dates = Array.from({ length: 7 }, (_, i) => dayName(new Date(from.getFullYear(), from.getMonth(), from.getDate() + i, 12).getTime()));
  const rows = await Promise.all(dates.map((date) => api.lapDay(date).then((list) => list.find((row) => row.lap === lap.id) ?? null, () => null)));
  const moment = Date.now();
  return dates.flatMap((date, i) => {
    const row = rows[i];
    const first = row && row.off === null ? row.at.find((at) => at > moment) : undefined;
    return first === undefined ? [] : [{ date, at: first }];
  });
}

/** A job's menu: it is picked first, so the detail beside shows it (a held job's script too, which
 * Sürdür lets run). What writes is dimmed while another Pitwall holds the laps. */
async function towerMenu(id: string, at: MenuPoint): Promise<void> {
  if (!byId.value.has(id)) return;
  selectedId.value = id;
  const days = await runDays(byId.value.get(id)!);
  // The job may have gone while its days were asked.
  const lap = byId.value.get(id);
  if (!lap) return;
  const off = elsewhere.value;
  const fail = (reason: unknown): void => detail.value?.fail(reason);
  const kept = (act: (now: LapView) => void) => () => {
    const now = byId.value.get(id);
    if (now) act(now);
  };
  const done = [...lap.last].reverse().find((mark) => mark.result !== null);
  const project = lap.place.kind === "project" ? (snapshot.value?.projects.find((p) => p.path === lap.place.path) ?? null) : null;
  const part = ordered.value.filter((other) => towerPart(other) === towerPart(lap));
  const top = part[0];
  const bottom = part[part.length - 1];
  const weekly = lap.when.kind !== "once" && lap.when.kind !== "refill";
  const running = lap.running;

  const entries: MenuEntry[] = [
    running ? { text: t("common.stop"), enabled: !off, action: () => detail.value?.stop() } : { text: t("laps.runNow"), enabled: !off, action: () => detail.value?.runNow() },
  ];
  if (running) entries.push({ text: t("laps.runs.terminal"), action: () => openRun(id, running.run) });
  entries.push({ text: t("laps.menu.lastResult"), enabled: done !== undefined, action: () => done && openRun(id, done.id) });

  entries.push(
    "separator",
    { text: t("laps.skipNext"), enabled: scheduled(lap) && lap.nextAt !== null && !off, action: () => void api.lapSkipNext(id).catch(fail) },
    lap.paused !== null
      ? { text: t("laps.resume"), enabled: !off, action: () => void api.lapPause(id, false).catch(fail) }
      : !lap.on
        ? { text: t("laps.switchOn"), enabled: !off, action: () => void api.lapSwitch(id, true).catch(fail) }
        : { text: t("laps.pause"), enabled: !off, action: () => void api.lapPause(id, true).catch(fail) },
    days.length && !off
      ? {
          text: t("laps.menu.dayOff"),
          items: days.map((day) => ({
            text: day.date === today.value ? t("laps.cal.today", { day: dayLong(day.date) }) : dayLong(day.date),
            action: () => (dayOff.value = { lap: id, name: lap.name, date: day.date, at: day.at, weekly }),
          })),
        }
      : { text: t("laps.menu.dayOff"), enabled: false },
  );

  entries.push(
    "separator",
    { text: t("common.edit"), action: kept((now) => (editing.value = { lap: now })) },
    { text: t("laps.menu.duplicate"), enabled: !off, action: () => void api.lapDuplicate(id).then((copy) => saved(copy.id), fail) },
    { text: t("laps.menu.top"), enabled: !off && top !== undefined && top.id !== id, action: () => top && move(id, top.id, false) },
    { text: t("laps.menu.bottom"), enabled: !off && bottom !== undefined && bottom.id !== id, action: () => bottom && move(id, bottom.id, true) },
  );

  entries.push("separator");
  if (project) {
    entries.push({ text: t("laps.menu.project"), action: () => emit("openProject", project.path) }, { text: t("files.show"), action: () => showFiles(project.path) });
  }
  entries.push({ text: t("common.openInEditor"), action: () => void api.openEditor(lap.place.path) }, { text: t(revealKey), action: () => void api.lapReveal(id).catch(fail) });

  if (lap.unread) entries.push("separator", { text: t("laps.menu.readAll"), enabled: !off, action: () => void api.lapReadAll(id).catch(fail) });

  entries.push("separator", { text: t("laps.delete"), enabled: !off, action: kept((now) => (removing.value = now)) });
  await menu.popup(at, entries);
}

/* ---------- the order: dragged in the tower ---------- */

/** A job dragged beside another (`after`: under it), in its own part of the tower: the jobs' own
 * order with it taken out and put back there, which the tower draws part by part. Shown at once;
 * the core's next state confirms it, and when it refuses, the order it has stands. */
function move(moved: string, beside: string, after: boolean): void {
  const now = state.value;
  if (!now || moved === beside) return;
  const order = now.laps.map((lap) => lap.id).filter((id) => id !== moved);
  const at = order.indexOf(beside);
  if (at < 0 || order.length === now.laps.length) return;
  order.splice(after ? at + 1 : at, 0, moved);

  const jobs = byId.value;
  state.value = { ...now, laps: order.map((id) => jobs.get(id)).filter((lap) => lap !== undefined) };
  void api.lapReorder(order).catch(() => void loadLaps());
}
</script>

<template>
  <main class="laps">
    <!-- Its heading and free space drag the window; the buttons stay clickable. -->
    <header class="laps-bar" :data-tauri-drag-region="dragRegion">
      <div class="heading">
        <span class="title">{{ t("laps.title") }}</span>
        <span class="subtitle">{{ t("laps.subtitle") }}</span>
      </div>
      <span v-if="elsewhere" class="elsewhere" :title="t('laps.elsewhere')"><Icon name="info" :size="12" />{{ t("laps.elsewhere") }}</span>
      <span class="grow"></span>
      <button
        ref="calendarButton"
        type="button"
        :class="['control', { pressed: calendarOpen }]"
        aria-haspopup="dialog"
        :aria-expanded="calendarOpen"
        :disabled="!state"
        @click="calendarOpen = !calendarOpen"
      >
        <Icon name="calendar" :size="13" />{{ t("laps.cal.title") }}<Icon name="chevron-down" :size="12" class="faint" />
      </button>
    </header>

    <!-- Over the page, not in its head: the head drags the window. -->
    <LapCalendar
      v-if="calendarOpen && state"
      :laps="state.laps"
      :today="today"
      :now="now"
      :anchor="calendarButton"
      @close="calendarOpen = false"
      @open-run="openRun"
    />

    <div v-if="!state" class="center"><Spinner :size="16" /></div>

    <div v-else-if="!ordered.length" class="center empty">
      <p class="empty-text">{{ t("laps.empty") }}</p>
      <button type="button" class="control primary" @click="editing = { lap: null }"><Icon name="plus" :size="13" />{{ t("laps.edit.new") }}</button>
    </div>

    <div v-else class="body">
      <LapTower
        :laps="ordered"
        :selected="selectedId"
        :now="now"
        :today="today"
        :elsewhere="elsewhere"
        @select="(id) => (selectedId = id)"
        @open-run="openRun"
        @new="editing = { lap: null }"
        @move="move"
        @menu="(id, at) => void towerMenu(id, at)"
      />
      <LapDetail
        v-if="selected"
        ref="detail"
        :lap="selected"
        :now="now"
        :elsewhere="elsewhere"
        @edit="editing = { lap: selected }"
        @remove="removing = selected"
        @open-run="(run) => selected && openRun(selected.id, run)"
      />
    </div>

    <footer v-if="state && ordered.length" class="legend">
      <span class="key"><i class="square ok"></i>{{ t("laps.result.ok") }}</span>
      <span class="key"><i class="square bad"></i>{{ t("laps.result.failed") }}</span>
      <span class="key"><i class="square grey"></i>{{ t("laps.legend.grey") }}</span>
      <span class="grow"></span>
      <span class="legend-note"><Icon name="moon" :size="11" />{{ t("laps.legend.note") }}</span>
    </footer>

    <LapEditDialog
      v-if="editing"
      :lap="editing.lap"
      :place="newPlace"
      :elsewhere="elsewhere"
      @saved="saved"
      @deleted="deleted"
      @close="editing = null"
    />
    <LapResultDialog v-if="result" :run="result.run" :lap="byId.get(result.lap) ?? null" :elsewhere="elsewhere" @close="result = null" />
    <DayOffDialog v-if="dayOff" :lap="dayOff.lap" :name="dayOff.name" :date="dayOff.date" :at="dayOff.at" :weekly="dayOff.weekly" @close="dayOff = null" />
    <LapDeleteDialog v-if="removing" :id="removing.id" :name="removing.name" :running="removing.running !== null" @deleted="deleted" @close="removing = null" />
  </main>
</template>

<style scoped src="./laps/laps.css"></style>

<style scoped>
.laps {
  position: relative;
  flex-grow: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

.laps-bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

/* The title, and the subtitle on the line under it, as the other pages' heads have them. */
.heading {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.subtitle {
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.elsewhere {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: 12px;
  color: var(--text-muted);
}

.control.pressed {
  background: var(--bg-control-on);
}

.faint {
  color: var(--text-faint);
}

.center {
  flex: 1 1 auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  color: var(--text-faint);
}

.empty-text {
  margin: 0;
  max-width: 420px;
  text-align: center;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-muted);
}

.body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
}

.legend {
  flex-shrink: 0;
  height: 34px;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 0 20px;
  border-top: 1px solid var(--line);
  font-size: 11.5px;
  color: var(--text-faint);
  white-space: nowrap;
}

.key {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.square {
  width: 8px;
  height: 12px;
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

.legend-note {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
