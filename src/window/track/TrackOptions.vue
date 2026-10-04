<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";

import { t, type Key } from "../../lib/i18n";
import { setTrack, track } from "../../lib/store";
import type { TrackSettings } from "../../lib/track";

/** The Track page's own settings, in Settings under Track: what the cars carry, how much moves
 * and the glow. A change shows on the circuit at once, and goes to the core by its own command
 * (`set_track`). */
const LABELS: Array<{ id: TrackSettings["labels"]; name: Key }> = [
  { id: "code", name: "track.labels.code" },
  { id: "name", name: "track.labels.name" },
  { id: "hover", name: "track.labels.hover" },
];
const MOTIONS: Array<{ id: TrackSettings["motion"]; name: Key }> = [
  { id: "full", name: "track.motion.full" },
  { id: "calm", name: "track.motion.calm" },
  { id: "still", name: "track.motion.still" },
];
/** The system asks for less motion: the lights stand still whatever is picked here. */
const reducedMotion = ref(false);
let motionQuery: MediaQueryList | null = null;

function set<K extends keyof TrackSettings>(key: K, value: TrackSettings[K]): void {
  if (track.value[key] !== value) setTrack({ ...track.value, [key]: value });
}

function onMotion(): void {
  reducedMotion.value = motionQuery?.matches ?? false;
}

onMounted(() => {
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  motionQuery.addEventListener("change", onMotion);
  onMotion();
});

onBeforeUnmount(() => motionQuery?.removeEventListener("change", onMotion));
</script>

<template>
  <div class="topt">
    <div class="topt-row stack">
      <span class="topt-text">
        <span id="track-labels" class="topt-name">{{ t("track.labels") }}</span>
        <span class="topt-hint">{{ t("track.labelsHint") }}</span>
      </span>
      <span class="seg" role="group" aria-labelledby="track-labels">
        <button v-for="item in LABELS" :key="item.id" type="button" :aria-pressed="track.labels === item.id" @click="set('labels', item.id)">{{ t(item.name) }}</button>
      </span>
    </div>

    <div class="topt-row stack">
      <span class="topt-text">
        <span id="track-motion" class="topt-name">{{ t("track.motion") }}</span>
        <span class="topt-hint">{{ t(reducedMotion ? "track.motion.reduced" : "track.motionHint") }}</span>
      </span>
      <span class="seg" role="group" aria-labelledby="track-motion">
        <button v-for="item in MOTIONS" :key="item.id" type="button" :aria-pressed="track.motion === item.id" @click="set('motion', item.id)">{{ t(item.name) }}</button>
      </span>
    </div>

    <label class="topt-row">
      <span class="topt-text">
        <span class="topt-name">{{ t("track.glow") }}</span>
        <span class="topt-hint">{{ t("track.glowHint") }}</span>
      </span>
      <input type="checkbox" role="switch" :checked="track.glow" @change="set('glow', ($event.target as HTMLInputElement).checked)" />
    </label>
  </div>
</template>

<style scoped>
.topt {
  display: flex;
  flex-direction: column;
}

.topt-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 0;
  border-top: 1px solid var(--line);
}

/* The first one sits under the tab's own room. */
.topt-row:first-child {
  padding-top: 0;
  border-top: 0;
}

.topt-row.stack {
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}

.topt-text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.topt-name {
  font-weight: 500;
}

.topt-hint {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-subtle);
}

/* One of a few: the picked one lit. A long language wraps to a second row rather than cutting a name. */
.seg {
  display: inline-flex;
  flex-wrap: wrap;
  max-width: 100%;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-input);
}

.seg button {
  height: 24px;
  padding: 0 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
}

.seg button:hover:not([aria-pressed="true"]) {
  color: var(--text);
}

.seg button[aria-pressed="true"] {
  background: #2a2f37;
  color: var(--text-strong);
}

/* A small switch, like the Sessions page's: no state colour, it isn't a state. */
.topt-row input {
  appearance: none;
  -webkit-appearance: none;
  position: relative;
  flex-shrink: 0;
  width: 26px;
  height: 14px;
  margin: 0;
  border-radius: 7px;
  background: #3a3f48;
  transition: background 0.15s;
}

.topt-row input::before {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--bg-app);
  transition: transform 0.15s;
}

.topt-row input:checked {
  background: var(--text-muted);
}

.topt-row input:checked::before {
  transform: translateX(12px);
}
</style>
