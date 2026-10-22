<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import type { Inset } from "../lib/circuit";
import { t } from "../lib/i18n";
import { openInWindow } from "../lib/open";
import { atWork } from "../lib/pages";
import { terminalRequest, usePanelFollow } from "../lib/panel";
import { dragRegion } from "../lib/platform";
import { api, setTrack, snapshot, track as settings } from "../lib/store";
import { circuit, CIRCUITS, codes, towerOrder } from "../lib/track";
import type { Project } from "../lib/types";
import TrackCanvas from "./track/TrackCanvas.vue";
import TrackCard from "./track/TrackCard.vue";
import TrackLegend from "./track/TrackLegend.vue";
import TrackPicker from "./track/TrackPicker.vue";
import TrackTower from "./track/TrackTower.vue";

/**
 * The Track page: the favourites and the projects at work on a circuit. A running dev server is a
 * green light lapping it, each Claude session at work an orange one; one waiting on you has driven
 * to the pit wall and stands there, a crashed server stands where it stopped, and a stopped
 * project is off the circuit, in the tower only. The tower beside the circuit lists every one
 * drawn for the keyboard, and the card under it
 * acts on the one selected. That project's terminals are along the page's bottom. Its settings are
 * in Settings, under Track.
 */
const emit = defineEmits<{ "open-project": [path: string]; "open-settings": [] }>();

/** The tower and the card, from the stage's left edge: narrower in a narrow window, where the
 * circuit needs the room more. */
const COLUMN = 272;
const COLUMN_NARROW = 232;
const NARROW = 780;
const EDGE = 16;
/** The legend's room from the stage's bottom edge. */
const LEGEND_EDGE = 14;

/* ---------- the circuit ---------- */

/** `shuffle`'s pick for this opening. */
const dealt = CIRCUITS[Math.floor(Math.random() * CIRCUITS.length)];
const shown = computed(() => (settings.value.circuit === "shuffle" ? dealt : settings.value.circuit));
const course = computed(() => circuit(shown.value));
const straight = computed(() => shown.value === "straight");

/* ---------- the projects ---------- */

const all = computed(() => snapshot.value?.projects ?? []);
/** The favourites and the projects at work (lib/pages), always: one that only sits in the list
 * would make the tower longer for nothing. */
const listed = computed(() => all.value.filter((project) => project.favourite || atWork(project)));
const projects = computed(() => towerOrder(listed.value));
/** The crashed servers drawn: one that isn't drawn (no favourite) isn't counted over the circuit. */
const crashed = computed(() => listed.value.filter((project) => project.status === "crashed").length);
/** By the list's own order and from all of it, so a project keeps its letters while the tower's
 * order changes and others come and go. */
const letters = computed(() => codes(all.value));
const tags = computed(() => (settings.value.labels === "name" ? new Map(all.value.map((project) => [project.path, project.name])) : letters.value));

const selectedPath = ref<string | null>(null);
/** The project pointed at in the tower: its light brightens, its tag shows. */
const hot = ref<string | null>(null);
const selected = computed(() => listed.value.find((project) => project.path === selectedPath.value) ?? null);

// Keep a selection while there is something to show: at first whoever leads the tower.
watch(
  projects,
  (list) => {
    if (!list.some((project) => project.path === selectedPath.value)) selectedPath.value = list[0]?.path ?? null;
  },
  { immediate: true },
);

/** The first session of a project's that waits on you. */
function waitingSession(path: string): string | null {
  return listed.value.find((entry) => entry.path === path)?.claudeSessions.find((session) => session.phase === "waiting")?.id ?? null;
}

/** What Claude waits with, brought up where its session runs: its editor window or terminal; in
 * one of Pitwall's own terminals, its card on the board or the project's terminal panel
 * (WindowApp: `revealTerminal`). A light at the wall names its own session; the tower, the
 * project's first. */
function bring(path: string, session = waitingSession(path)): void {
  selectedPath.value = path;
  if (session) void api.revealClaude(path, session);
  else emit("open-project", path);
}

/** Return in the tower: a project whose Claude waits on you goes to that session, any other to the
 * project in the list. */
function open(path: string): void {
  if (waitingSession(path)) bring(path);
  else emit("open-project", path);
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

/* ---------- the terminals ---------- */

// The terminal panel under the page (TerminalPanel) shows the project selected in the tower.
usePanelFollow(() => (selectedPath.value ? { path: selectedPath.value, tab: null } : null));

// A Claude session in one of Pitwall's terminals was brought up (WindowApp: `revealTerminal`): its
// project is selected in the tower, as the panel shows its tab.
watch(
  terminalRequest,
  (request) => {
    if (request && listed.value.some((project) => project.path === request.path)) selectedPath.value = request.path;
  },
  { immediate: true },
);

/* ---------- the stage ---------- */

const stage = ref<HTMLElement | null>(null);
const narrow = ref(false);
let observer: ResizeObserver | null = null;

/** What the lights mean, along the stage's bottom: one line, or more in a narrow window. */
const legend = ref<{ $el: HTMLElement } | null>(null);
const legendHeight = ref(0);
let legendObserver: ResizeObserver | null = null;
/** The legend is measured: the circuit is first laid out with its room known, so nothing moves once
 * it is drawn. */
const measured = ref(false);

onMounted(() => {
  if (!stage.value) return;
  observer = new ResizeObserver(([entry]) => (narrow.value = entry.contentRect.width < NARROW));
  observer.observe(stage.value);
  narrow.value = stage.value.clientWidth < NARROW;
  // As the observer will measure it: the same number, to the fraction.
  legendHeight.value = legend.value?.$el.getBoundingClientRect().height ?? 0;
  measured.value = true;
});

// The legend comes and goes with the projects.
watch(
  legend,
  (shown) => {
    legendObserver?.disconnect();
    if (!shown) {
      legendHeight.value = 0;
      return;
    }
    legendObserver ??= new ResizeObserver(([entry]) => (legendHeight.value = entry.contentRect.height));
    legendObserver.observe(shown.$el);
  },
  { flush: "post" },
);

onBeforeUnmount(() => {
  observer?.disconnect();
  legendObserver?.disconnect();
});

const column = computed(() => (narrow.value ? COLUMN_NARROW : COLUMN));

/** The room kept round the circuit: the column on its left, its tags above and to the right, the
 * pit wall's tags under it, and the legend under those. */
const inset = computed<Inset>(() => ({
  top: 48,
  right: narrow.value ? 44 : 60,
  bottom: (straight.value ? 40 : 76) + (legendHeight.value ? legendHeight.value + LEGEND_EDGE : 0),
  left: (listed.value.length ? EDGE + column.value : 0) + (narrow.value ? 24 : 36),
}));
</script>

<template>
  <div class="track">
    <!-- Its heading and free space drag the window; the pickers stay clickable. -->
    <header class="bar" :data-tauri-drag-region="dragRegion">
      <div class="heading">
        <span class="title">{{ t("track.title") }}</span>
        <span v-if="snapshot" class="subtitle">
          <span class="tally"><span class="dot run" aria-hidden="true"></span>{{ t("popover.running", { count: snapshot.running }) }}</span>
          <span v-if="snapshot.waiting" class="tally"><span class="dot claude" aria-hidden="true"></span>{{ t("popover.waiting", { count: snapshot.waiting }) }}</span>
          <span v-if="crashed" class="tally"><span class="dot crash" aria-hidden="true"></span>{{ t("track.crashed", { count: crashed }) }}</span>
        </span>
      </div>
      <TrackPicker :picked="settings.circuit" :shown="shown" @pick="setTrack({ ...settings, circuit: $event })" />
      <button type="button" class="options" :aria-label="t('track.options')" :title="t('track.options')" @click="emit('open-settings')">
        <Icon name="settings" :size="16" />
      </button>
    </header>

    <div ref="stage" class="stage">
      <div class="sky" aria-hidden="true"></div>

      <TrackCanvas
        v-if="measured"
        :circuit="course"
        :projects="projects"
        :tags="tags"
        :settings="settings"
        :selected="selectedPath"
        :hot="hot"
        :inset="inset"
        @select="selectedPath = $event"
        @open="openInWindow($event, 'pages')"
        @bring="bring"
      />

      <div v-if="listed.length" class="column" :style="{ width: `${column}px` }">
        <TrackTower
          :projects="projects"
          :codes="letters"
          :selected="selectedPath"
          @select="selectedPath = $event"
          @open="open"
          @toggle="toggle"
          @hover="hot = $event"
        />
        <TrackCard v-if="selected" :project="selected" @open="emit('open-project', $event)" />
      </div>

      <TrackLegend v-if="listed.length" ref="legend" class="legend" :style="{ left: `${EDGE * 2 + column}px`, bottom: `${LEGEND_EDGE}px` }" />

      <p v-if="snapshot && !all.length" class="none">{{ t("window.emptyLine1") }}</p>
    </div>

  </div>
</template>

<style scoped>
.track {
  position: relative;
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

/* As tall as the projects toolbar, so switching never moves the title. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
}

.subtitle {
  display: flex;
  gap: 12px;
  min-width: 0;
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-muted);
  overflow: hidden;
}

.tally {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  white-space: nowrap;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.dot.run {
  background: var(--run);
}

.dot.claude {
  background: var(--claude);
}

.dot.crash {
  background: var(--crash);
}

/* The page's settings open in Settings: a quiet button, as the other pages' are. */
.options {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.options:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* The ground the circuit lies in, a shade under the window's: the night in the dark theme, a
   pale day in the light one (--bg-stage, styles/tokens.css). The circuit takes the look that goes
   with it (lib/circuit: `Look`), and what is laid over the stage, the terminals too, follows the
   theme as the bar above does. */
.stage {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
  background: var(--bg-stage);
}

/* A little light behind the circuit, as on the website: a lighter patch of the stage's ground. */
.sky {
  position: absolute;
  inset: 0;
  background: radial-gradient(ellipse 52% 60% at 64% 44%, var(--stage-sky) 0%, transparent 100%);
}

/* The tower, and the selected project's card under it: the tower gives way and scrolls. */
.column {
  position: absolute;
  left: 16px;
  top: 16px;
  bottom: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  pointer-events: none;
}

.column > * {
  pointer-events: auto;
}

/* Under the circuit, in the room beside the column. */
.legend {
  position: absolute;
  right: 16px;
}

.none {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  padding: 0 24px;
  color: var(--text-subtle);
  text-align: center;
}
</style>
