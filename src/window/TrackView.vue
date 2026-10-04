<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import type { Inset } from "../lib/circuit";
import { t } from "../lib/i18n";
import { dragRegion } from "../lib/platform";
import { api, snapshot } from "../lib/store";
import { circuit, CIRCUITS, codes, towerOrder, trackSettings, type TrackSettings } from "../lib/track";
import type { Project } from "../lib/types";
import TrackCanvas from "./track/TrackCanvas.vue";
import TrackCard from "./track/TrackCard.vue";
import TrackOptions from "./track/TrackOptions.vue";
import TrackPicker from "./track/TrackPicker.vue";
import TrackPit from "./track/TrackPit.vue";
import TrackTower from "./track/TrackTower.vue";

/**
 * The Track page: every project on the website's night circuit. A running dev server is a green
 * light lapping it, Claude at work an orange one; Claude waiting on you stands at the pit wall, a
 * crashed server stands where it stopped, and a stopped project waits in the pit. The tower beside
 * the circuit lists the same projects for the keyboard, and the card under it acts on the one
 * selected.
 */
const emit = defineEmits<{ "open-project": [path: string] }>();

/** The tower and the card, from the stage's left edge: narrower in a narrow window, where the
 * circuit needs the room more. */
const COLUMN = 272;
const COLUMN_NARROW = 232;
const NARROW = 780;
const EDGE = 16;
/** The pit strip's height. */
const PIT = 50;

/* ---------- the page's settings ---------- */

const stored = computed(() => trackSettings(snapshot.value?.settings.track));
/** What was just picked: shown at once, the core keeps it for the next opening. */
const picked = ref<TrackSettings | null>(null);
const settings = computed(() => picked.value ?? stored.value);

function change(next: TrackSettings): void {
  picked.value = next;
  // An older core doesn't know the command: the choice then lasts as long as the page.
  void api.setTrack(next).catch(() => undefined);
}

/** `shuffle`'s pick for this opening. */
const dealt = CIRCUITS[Math.floor(Math.random() * CIRCUITS.length)];
const shown = computed(() => (settings.value.circuit === "shuffle" ? dealt : settings.value.circuit));
const course = computed(() => circuit(shown.value));
const straight = computed(() => shown.value === "straight");

/* ---------- the projects ---------- */

const listed = computed(() => snapshot.value?.projects ?? []);
const projects = computed(() => towerOrder(listed.value));
/** By the list's own order, so a project keeps its letters while the tower's order changes. */
const letters = computed(() => codes(listed.value));
const tags = computed(() => (settings.value.labels === "name" ? new Map(listed.value.map((project) => [project.path, project.name])) : letters.value));

const stoppedProjects = computed(() => projects.value.filter((project) => project.status === "stopped"));
const anyBusy = computed(() => listed.value.some((project) => project.status === "busy"));
const anyRunning = computed(() => listed.value.some((project) => project.status === "running"));
/** The straight gives every project a lane, the stopped ones too: it needs no pit. */
const pitShown = computed(() => settings.value.pit && !straight.value && listed.value.length > 0);

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

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

/* ---------- the stage ---------- */

const stage = ref<HTMLElement | null>(null);
const narrow = ref(false);
let observer: ResizeObserver | null = null;

onMounted(() => {
  if (!stage.value) return;
  observer = new ResizeObserver(([entry]) => (narrow.value = entry.contentRect.width < NARROW));
  observer.observe(stage.value);
});

onBeforeUnmount(() => observer?.disconnect());

const column = computed(() => (narrow.value ? COLUMN_NARROW : COLUMN));

/** The room kept round the circuit: the column on its left, its tags above and to the right, the
 * pit wall's tags and the pit under it. */
const inset = computed<Inset>(() => ({
  top: 48,
  right: narrow.value ? 44 : 60,
  bottom: (pitShown.value ? PIT + EDGE : 0) + (straight.value ? 40 : 76),
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
          <span v-if="snapshot.crashed" class="tally"><span class="dot crash" aria-hidden="true"></span>{{ t("track.crashed", { count: snapshot.crashed }) }}</span>
        </span>
      </div>
      <TrackPicker :picked="settings.circuit" :shown="shown" @pick="change({ ...settings, circuit: $event })" />
      <TrackOptions :settings="settings" @change="change" />
    </header>

    <div ref="stage" class="stage">
      <div class="sky" aria-hidden="true"></div>

      <TrackCanvas
        :circuit="course"
        :projects="projects"
        :tags="tags"
        :settings="settings"
        :selected="selectedPath"
        :hot="hot"
        :inset="inset"
        @select="selectedPath = $event"
        @open="emit('open-project', $event)"
      />

      <div v-if="listed.length" class="column" :style="{ width: `${column}px` }">
        <TrackTower
          :projects="projects"
          :codes="letters"
          :selected="selectedPath"
          @select="selectedPath = $event"
          @open="emit('open-project', $event)"
          @toggle="toggle"
          @hover="hot = $event"
        />
        <TrackCard v-if="selected" :project="selected" @open="emit('open-project', $event)" />
      </div>

      <TrackPit v-if="pitShown" class="pit-strip" :style="{ left: `${EDGE * 2 + column}px` }" :projects="stoppedProjects" :codes="letters" :selected="selectedPath" :busy="anyBusy" :running="anyRunning" @select="selectedPath = $event" />

      <p v-if="snapshot && !listed.length" class="none">{{ t("window.emptyLine1") }}</p>
    </div>
  </div>
</template>

<style scoped>
.track {
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

/* The night the circuit lies in: darker than the window, as the output panel is. */
.stage {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
  background: var(--bg-log);
}

/* A little light behind the circuit, as on the website. */
.sky {
  position: absolute;
  inset: 0;
  background: radial-gradient(ellipse 52% 60% at 64% 44%, #171b22 0%, rgba(11, 12, 15, 0) 100%);
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

.pit-strip {
  position: absolute;
  right: 16px;
  bottom: 16px;
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
