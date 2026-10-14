<script setup lang="ts">
// Settings › General › Mini tower. Its own: saved by its own commands, never with the form.
//
// Whether the mini tower shows, its form, what it leaves out, on which desktops (macOS), and its
// global shortcut: on or off, and the keys. Every value is the snapshot's (`settings.tower`); a
// change goes to the core at once and the tower follows it there.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { shortcutLabel } from "../../lib/format";
import { t, type Key } from "../../lib/i18n";
import { isMac, keys, shortcutModifiers } from "../../lib/platform";
import { recordingEnded, recordingStarts } from "../../lib/shortcutRecording";
import { api, snapshot } from "../../lib/store";
import { towerSettings } from "../../lib/tower";
import type { TowerSettings } from "../../lib/types";

const FORMS: Array<{ id: TowerSettings["form"]; name: Key }> = [
  { id: "list", name: "tower.form.list" },
  { id: "strip", name: "tower.form.strip" },
];

const tower = computed(() => towerSettings(snapshot.value?.settings.tower));
/** Why the shortcut wasn't taken: its keys belong to another app, or to the quick switcher. */
const error = ref("");

/** A switch as just clicked goes to the core; one the core refuses goes back to what is stored. */
async function flip(event: Event, send: (on: boolean) => Promise<unknown>, stored: () => boolean): Promise<void> {
  const box = event.target as HTMLInputElement;
  error.value = "";
  try {
    await send(box.checked);
  } catch (reason) {
    box.checked = stored();
    error.value = String(reason);
  }
}

function setForm(form: TowerSettings["form"]): void {
  if (tower.value.form !== form) void api.towerForm(form);
}

/* ---------- the shortcut's keys, recorded as the quick switcher's are (SettingsView) ---------- */

const root = ref<HTMLElement | null>(null);
const recording = ref(false);
/** What the keyboard is sending right now, shown live while recording. */
const held = ref("");
/** When Escape last ended a recording: the dialog's own answer to that same key is not wanted. */
let escapedAt = -Infinity;

function modifiers(event: KeyboardEvent): string[] {
  const mods: string[] = [];
  if (event.ctrlKey) mods.push("Ctrl");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");
  if (event.metaKey) mods.push("Super");
  return mods;
}

function startRecording(): void {
  // One recording at a time: the quick switcher's, if under way, ends here.
  recordingStarts(stopRecording);
  error.value = "";
  held.value = "";
  recording.value = true;
  // Neither shortcut may fire while new keys are typed: they would never reach this window.
  void api.suspendShortcut();
  window.addEventListener("keydown", onRecordKey, true);
  window.addEventListener("keyup", onRecordKeyUp, true);
}

function stopRecording(): void {
  recordingEnded(stopRecording);
  recording.value = false;
  held.value = "";
  window.removeEventListener("keydown", onRecordKey, true);
  window.removeEventListener("keyup", onRecordKeyUp, true);
}

function cancelRecording(): void {
  stopRecording();
  void api.resumeShortcut();
}

function onRecordKeyUp(event: KeyboardEvent): void {
  held.value = shortcutLabel(modifiers(event).join("+"));
}

async function onRecordKey(event: KeyboardEvent): Promise<void> {
  event.preventDefault();
  event.stopPropagation();
  const mods = modifiers(event);

  if (event.key === "Escape" && !mods.length) {
    escapedAt = performance.now();
    cancelRecording();
    return;
  }

  // Only modifiers so far: show them and wait for the key.
  if (/^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/.test(event.code)) {
    held.value = shortcutLabel(mods.join("+"));
    return;
  }

  const combo = [...mods, event.code].join("+");
  held.value = shortcutLabel(combo);

  if (!mods.some((mod) => mod !== "Shift")) {
    error.value = t("settings.shortcutNeedsModifier", shortcutModifiers());
    return;
  }

  stopRecording();
  try {
    // The core registers both shortcuts again, whether it took the keys or not.
    await api.towerShortcut(combo);
    error.value = "";
  } catch (reason) {
    error.value = String(reason);
  }
}

/** Escape while recording cancels the recording, not the dialog the section stands in. */
function onDialogCancel(event: Event): void {
  if (recording.value || performance.now() - escapedAt < 200) event.preventDefault();
}

let dialog: HTMLDialogElement | null = null;

onMounted(() => {
  dialog = root.value?.closest("dialog") ?? null;
  dialog?.addEventListener("cancel", onDialogCancel, true);
});

onBeforeUnmount(() => {
  dialog?.removeEventListener("cancel", onDialogCancel, true);
  if (recording.value) cancelRecording();
});
</script>

<template>
  <div ref="root" class="section part tower-part">
    <div id="tower-part" class="section-label">{{ t("tower.title") }}</div>
    <div class="rows" role="group" aria-labelledby="tower-part">
      <label class="row">
        <span class="name">{{ t("tower.show") }}</span>
        <input type="checkbox" role="switch" :checked="tower.on" @change="flip($event, api.towerShow, () => tower.on)" />
      </label>
      <div class="row boxed">
        <span id="tower-form" class="name">{{ t("tower.form") }}</span>
        <span class="seg" role="group" aria-labelledby="tower-form">
          <button v-for="form in FORMS" :key="form.id" type="button" :aria-pressed="tower.form === form.id" @click="setForm(form.id)">{{ t(form.name) }}</button>
        </span>
      </div>
      <label class="row">
        <span class="name">{{ t("tower.hideStopped") }}</span>
        <input type="checkbox" role="switch" :checked="tower.hideStopped" @change="flip($event, (on) => api.towerOption('hideStopped', on), () => tower.hideStopped)" />
      </label>
      <!-- Desktops of this kind are macOS's. -->
      <label v-if="isMac" class="row">
        <span class="name">{{ t("tower.allSpaces") }}</span>
        <input type="checkbox" role="switch" :checked="tower.allSpaces" @change="flip($event, (on) => api.towerOption('allSpaces', on), () => tower.allSpaces)" />
      </label>
      <div class="row boxed">
        <label for="tower-shortcut" class="name">{{ t("settings.shortcut") }}</label>
        <!-- One width, whatever it reads: the keys, those held, or the call to press some. -->
        <button
          type="button"
          :class="['recorder', 'keys', { live: recording }]"
          :aria-label="t(recording ? 'settings.shortcutRecording' : 'settings.shortcutChange')"
          @click="recording ? cancelRecording() : startRecording()"
        >
          <span class="keys-sizer" aria-hidden="true">{{ t("settings.shortcutPress") }}</span>
          <span class="keys-text">{{ recording ? held || t("settings.shortcutPress") : shortcutLabel(tower.shortcutKeys) }}</span>
        </button>
        <input id="tower-shortcut" type="checkbox" role="switch" :checked="tower.shortcut" @change="flip($event, (on) => api.towerOption('shortcut', on), () => tower.shortcut)" />
      </div>
    </div>
    <small v-if="recording" class="keys-hint">{{ t("settings.shortcutHint", { ...shortcutModifiers(), esc: keys("Escape") }) }}</small>
    <p v-if="error" class="error">{{ error }}</p>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.tower-part {
  font-size: 13px;
}

/* Under the part before it, as far as the tab's parts stand from each other. */
.tower-part:not(:first-child) {
  margin-top: 16px;
}

/* A line that ends in a boxed control is as tall as the lines that end in a switch. */
.row.boxed {
  padding-top: 4px;
  padding-bottom: 4px;
}

/* The quick switcher's field (`.recorder`, settings.css), at one width whatever it reads: its two
   texts share one cell, and the wider one (unseen: the call to press keys, in the letters that
   call is written in) sets the width. */
.keys {
  flex-shrink: 0;
  display: inline-grid;
  justify-items: center;
  align-items: center;
}

.keys-sizer,
.keys-text {
  grid-area: 1 / 1;
  white-space: nowrap;
}

.keys-sizer {
  visibility: hidden;
  font-family: var(--font);
  font-size: 12px;
  letter-spacing: 0;
}

.keys-hint {
  font-size: 12px;
  color: var(--text-subtle);
  line-height: 1.5;
}
</style>
