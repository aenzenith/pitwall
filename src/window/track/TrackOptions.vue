<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { t, type Key } from "../../lib/i18n";
import type { TrackSettings } from "../../lib/track";

/** The page's own settings, under their button in the bar: what the cars carry, how much moves,
 * the pit and the glow; and what the lights mean. A change shows on the circuit at once. */
const props = defineProps<{ settings: TrackSettings }>();
const emit = defineEmits<{ change: [settings: TrackSettings] }>();

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
const LEGEND: Array<{ mark: string; text: Key }> = [
  { mark: "server", text: "track.legend.server" },
  { mark: "claude", text: "track.legend.claude" },
  { mark: "waiting", text: "track.legend.waiting" },
  { mark: "crashed", text: "track.legend.crashed" },
];

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const panel = ref<HTMLElement | null>(null);
const open = ref(false);
const place = ref<Record<string, string>>({});
/** The system asks for less motion: the lights stand still whatever is picked here. */
const reducedMotion = ref(false);
let motionQuery: MediaQueryList | null = null;

function set<K extends keyof TrackSettings>(key: K, value: TrackSettings[K]): void {
  if (props.settings[key] !== value) emit("change", { ...props.settings, [key]: value });
}

function show(): void {
  const box = button.value?.getBoundingClientRect();
  if (!box || open.value) return;
  place.value = {
    top: `${box.bottom + 4}px`,
    right: `${window.innerWidth - box.right}px`,
    maxHeight: `${Math.max(160, window.innerHeight - box.bottom - 16)}px`,
  };
  open.value = true;
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => panel.value?.querySelector<HTMLElement>("button, input")?.focus());
}

function hide(): void {
  open.value = false;
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

/** Closed with the keyboard: it goes back to the button. */
function close(): void {
  hide();
  button.value?.focus();
}

function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

function onMotion(): void {
  reducedMotion.value = motionQuery?.matches ?? false;
}

onMounted(() => {
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  motionQuery.addEventListener("change", onMotion);
  onMotion();
});

onBeforeUnmount(() => {
  hide();
  motionQuery?.removeEventListener("change", onMotion);
});
</script>

<template>
  <div ref="root" class="topt">
    <button
      ref="button"
      type="button"
      class="topt-button"
      aria-haspopup="dialog"
      :aria-expanded="open"
      :aria-label="t('track.options')"
      :title="open ? undefined : t('track.options')"
      @click="open ? hide() : show()"
    >
      <Icon name="settings" :size="14" />
    </button>

    <div v-if="open" ref="panel" class="topt-panel" role="dialog" :aria-label="t('track.options')" :style="place" @keydown.esc.stop.prevent="close">
      <div class="topt-title">{{ t("track.options") }}</div>

      <div class="topt-row stack">
        <span class="topt-text">
          <span id="track-labels" class="topt-name">{{ t("track.labels") }}</span>
          <span class="topt-hint">{{ t("track.labelsHint") }}</span>
        </span>
        <span class="seg" role="group" aria-labelledby="track-labels">
          <button v-for="item in LABELS" :key="item.id" type="button" :aria-pressed="settings.labels === item.id" @click="set('labels', item.id)">{{ t(item.name) }}</button>
        </span>
      </div>

      <div class="topt-row stack">
        <span class="topt-text">
          <span id="track-motion" class="topt-name">{{ t("track.motion") }}</span>
          <span class="topt-hint">{{ t(reducedMotion ? "track.motion.reduced" : "track.motionHint") }}</span>
        </span>
        <span class="seg" role="group" aria-labelledby="track-motion">
          <button v-for="item in MOTIONS" :key="item.id" type="button" :aria-pressed="settings.motion === item.id" @click="set('motion', item.id)">{{ t(item.name) }}</button>
        </span>
      </div>

      <label class="topt-row">
        <span class="topt-text">
          <span class="topt-name">{{ t("track.pitToggle") }}</span>
          <span class="topt-hint">{{ t("track.pitToggleHint") }}</span>
        </span>
        <input type="checkbox" role="switch" :checked="settings.pit" @change="set('pit', ($event.target as HTMLInputElement).checked)" />
      </label>

      <label class="topt-row">
        <span class="topt-text">
          <span class="topt-name">{{ t("track.glow") }}</span>
          <span class="topt-hint">{{ t("track.glowHint") }}</span>
        </span>
        <input type="checkbox" role="switch" :checked="settings.glow" @change="set('glow', ($event.target as HTMLInputElement).checked)" />
      </label>

      <ul class="topt-legend" :aria-label="t('track.legend')">
        <li v-for="item in LEGEND" :key="item.mark"><span :class="['mark', item.mark]" aria-hidden="true"></span>{{ t(item.text) }}</li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.topt {
  flex-shrink: 0;
  display: flex;
}

.topt-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: #c7ccd3;
}

.topt-button:hover,
.topt-button[aria-expanded="true"] {
  background: #2c3039;
}

/* Over the page, under its button; in a short window it scrolls. */
.topt-panel {
  position: fixed;
  z-index: 60;
  width: 320px;
  max-width: calc(100vw - 16px);
  display: flex;
  flex-direction: column;
  padding: 14px 16px 12px;
  overflow-y: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-panel);
  background: var(--bg-panel);
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
}

.topt-title {
  padding-bottom: 12px;
  font-weight: 600;
}

.topt-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 0;
  border-top: 1px solid #1e2127;
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

/* What the lights mean. */
.topt-legend {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin: 0;
  padding: 12px 0 2px;
  border-top: 1px solid #1e2127;
  list-style: none;
  font-size: 12px;
  color: var(--text-subtle);
}

.topt-legend li {
  display: flex;
  align-items: center;
  gap: 8px;
}

.mark {
  flex-shrink: 0;
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

/* Moving lights are drawn as a short trail; the ones that stand, as a dot. */
.mark.server,
.mark.claude {
  width: 14px;
  height: 4px;
  margin: 0 -3px;
  border-radius: 2px;
}

.mark.server {
  background: linear-gradient(90deg, transparent, var(--run));
}

.mark.claude {
  background: linear-gradient(90deg, transparent, var(--claude));
}

.mark.waiting {
  background: var(--claude);
}

.mark.crashed {
  background: var(--crash);
}
</style>
