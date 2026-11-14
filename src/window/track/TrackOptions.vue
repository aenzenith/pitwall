<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { t, type Key } from "../../lib/i18n";
import { setTrack, track } from "../../lib/store";
import type { TrackSettings } from "../../lib/track";

/**
 * The Track page's own settings, behind the settings button in its bar (in Settings, under Track,
 * until 2026-11-14): what the cars carry, how much moves and the glow, in a popover under the
 * button. A change shows on the circuit at once, and goes to the core by its own command
 * (`set_track`).
 *
 * It opens on a click on the button, or Return or Space on it; it closes on the button again, on
 * Esc, on a press outside it, when the keyboard goes elsewhere and when the window loses the
 * front or changes size. Opened with the keyboard, the keyboard goes to its first control, and
 * back to the button as Esc closes it.
 */
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

/* ---------- the popover ---------- */

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const pop = ref<HTMLElement | null>(null);

const open = ref(false);
/** Under the button, their right edges level. */
const place = ref<Record<string, string>>({});

/** `keyed`: opened with the keyboard, which goes to its first control. */
function show(keyed: boolean): void {
  const box = button.value?.getBoundingClientRect();
  if (!box || open.value) return;
  place.value = {
    top: `${box.bottom + 4}px`,
    right: `${window.innerWidth - box.right}px`,
    maxHeight: `${Math.max(160, window.innerHeight - box.bottom - 16)}px`,
  };
  open.value = true;
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  if (keyed) void nextTick(() => pop.value?.querySelector<HTMLElement>("button, input")?.focus());
}

function hide(): void {
  if (!open.value) return;
  open.value = false;
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("keydown", onKey, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

/** A click's `detail` is 0 when Return or Space pressed the button. */
function onClick(event: MouseEvent): void {
  if (open.value) hide();
  else show(event.detail === 0);
}

/** A press anywhere else closes it; one on the button is the button's own to answer. */
function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

/** Esc closes it and nothing else: the page's own answer to the key is not wanted. */
function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape" || event.isComposing) return;
  event.preventDefault();
  event.stopPropagation();
  const inside = !!root.value?.contains(document.activeElement);
  hide();
  if (inside) button.value?.focus();
}

/** The keyboard went to a control outside it. */
function onFocusOut(event: FocusEvent): void {
  const next = event.relatedTarget;
  if (next instanceof Node && !root.value?.contains(next)) hide();
}

onMounted(() => {
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  motionQuery.addEventListener("change", onMotion);
  onMotion();
});

onBeforeUnmount(() => {
  motionQuery?.removeEventListener("change", onMotion);
  hide();
});
</script>

<template>
  <div ref="root" class="topt-root" @focusout="onFocusOut">
    <button
      ref="button"
      type="button"
      class="topt-button"
      aria-haspopup="dialog"
      :aria-expanded="open"
      :aria-label="t('track.options')"
      :title="open ? undefined : t('track.options')"
      @click="onClick"
    >
      <Icon name="settings" :size="16" />
    </button>

    <Transition name="topt-pop">
      <div v-if="open" ref="pop" class="topt" role="dialog" :aria-label="t('track.options')" :style="place">
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
    </Transition>
  </div>
</template>

<style scoped src="../settings/settings.css"></style>
<style scoped>
.topt-root {
  flex-shrink: 0;
  display: flex;
}

/* Quiet, as the circuit picker beside it. */
.topt-button {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.topt-button:hover,
.topt-button[aria-expanded="true"] {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Over the page, under the button; in a short window it scrolls. */
.topt {
  position: fixed;
  z-index: 60;
  width: 320px;
  max-width: calc(100vw - 16px);
  display: flex;
  flex-direction: column;
  padding: 14px 16px;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-panel);
  background: var(--bg-panel);
  box-shadow: var(--shadow-popover);
  transform-origin: top right;
}

/* It opens out of the button, from the corner under it, and fades away. */
.topt-pop-enter-active {
  transition:
    opacity 0.16s ease-out,
    transform 0.16s cubic-bezier(0.2, 0.8, 0.2, 1);
}

.topt-pop-leave-active {
  transition: opacity 0.1s ease-in;
  pointer-events: none;
}

.topt-pop-enter-from {
  opacity: 0;
  transform: translateY(-6px) scale(0.97);
}

.topt-pop-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .topt-pop-enter-active,
  .topt-pop-leave-active {
    transition: none;
  }
}

.topt-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 0;
  border-top: 1px solid var(--line);
}

.topt-row:first-child {
  padding-top: 0;
  border-top: 0;
}

.topt-row:last-child {
  padding-bottom: 0;
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

/* The segments are Settings' own (settings/settings.css); here a long language wraps to a second
   row rather than cutting a name. */
.seg {
  flex-wrap: wrap;
}
</style>
