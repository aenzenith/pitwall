<script setup lang="ts">
// Settings › Notifications: which events bring a notification, quiet mode's hours and what it
// lets through, and the muted projects. Each change goes to the core by its own command
// (core/signals.rs) and comes back in the next snapshot. Which sound an event plays is the
// Settings form's to save: those rows come in through the `sounds` slot. The phone's section
// stays last.
import { computed, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";
import { useNativeMenu } from "../../lib/nativeMenu";
import { EVENTS, eventOn, notifications, type NotifyEvent } from "../../lib/quiet";
import { api, snapshot } from "../../lib/store";
import PhoneSection from "./PhoneSection.vue";

/** A switch as just clicked goes to the core; one the core refuses goes back to what is stored. */
async function flip(event: Event, send: (on: boolean) => Promise<unknown>, stored: () => boolean): Promise<void> {
  const box = event.target as HTMLInputElement;
  try {
    await send(box.checked);
  } catch {
    box.checked = stored();
  }
}

function setEvent(event: Event, id: NotifyEvent): void {
  void flip(event, (on) => api.notifyEvent(id, on), () => eventOn(id));
}

/* ---------- quiet hours ---------- */

const hours = computed(() => notifications.value.quietHours);
/** The two times as typed: a snapshot never writes over the field the keyboard is in. */
const from = ref(hours.value.from);
const to = ref(hours.value.to);

watch(hours, (stored) => {
  const typing = (document.activeElement as HTMLElement | null)?.dataset?.quiet;
  if (typing !== "from") from.value = stored.from;
  if (typing !== "to") to.value = stored.to;
});

/** A time left half typed is no time yet: nothing is sent until both are whole. */
function saveHours(on = hours.value.on): Promise<unknown> {
  if (!from.value || !to.value) return Promise.resolve();
  return api.quietHours(on, from.value, to.value);
}

/** Leaving a field: what it holds is what is stored again, should it have been left half typed. */
function onTimeBlur(): void {
  if (!from.value) from.value = hours.value.from;
  if (!to.value) to.value = hours.value.to;
}

/* ---------- muted projects ---------- */

const projects = computed(() => snapshot.value?.projects ?? []);

/** Each muted path with the name its project is listed under; a folder no longer listed goes by
 * its own name. */
const muted = computed(() =>
  (notifications.value.muted ?? []).map((path) => ({
    path,
    name: projects.value.find((project) => project.path === path)?.name ?? path.split(/[\\/]/).filter(Boolean).pop() ?? path,
  })),
);
const unmuted = computed(() => projects.value.filter((project) => !project.muted));

const menu = useNativeMenu();

/** The projects that can still be muted, under the button. */
function pickProject(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  void menu.popup(
    { clientX: box.left, clientY: box.bottom + 4 },
    unmuted.value.map((project) => ({ text: project.name, action: () => void api.muteProject(project.path, true) })),
  );
}
</script>

<template>
  <div class="notif">
    <div class="rows">
      <label v-for="event in EVENTS" :key="event.id" class="row">
        <span class="name">{{ t(event.label) }}</span>
        <input type="checkbox" role="switch" :checked="eventOn(event.id)" @change="setEvent($event, event.id)" />
      </label>
    </div>

    <div v-if="$slots.sounds" class="section part">
      <div id="notif-sounds" class="section-label">{{ t("settings.sounds") }}</div>
      <slot name="sounds" />
    </div>

    <div class="section part">
      <div id="notif-quiet" class="section-label">{{ t("notifications.quiet") }}</div>
      <div class="rows" role="group" aria-labelledby="notif-quiet">
        <div class="row">
          <label for="notif-hours" class="name">{{ t("notifications.quietHours") }}</label>
          <span :class="['times', { off: !hours.on }]">
            <input v-model="from" type="time" data-quiet="from" :aria-label="t('notifications.from')" :disabled="!hours.on" @change="saveHours()" @blur="onTimeBlur" />
            <span aria-hidden="true">–</span>
            <input v-model="to" type="time" data-quiet="to" :aria-label="t('notifications.to')" :disabled="!hours.on" @change="saveHours()" @blur="onTimeBlur" />
          </span>
          <input id="notif-hours" type="checkbox" role="switch" :checked="hours.on" @change="flip($event, saveHours, () => hours.on)" />
        </div>
        <label class="row">
          <span class="name">{{ t("quiet.questions") }}</span>
          <input type="checkbox" role="switch" :checked="notifications.questionsThrough" @change="flip($event, (on) => api.quietOption('questions', on), () => notifications.questionsThrough)" />
        </label>
        <label class="row">
          <span class="name">{{ t("quiet.sounds") }}</span>
          <input type="checkbox" role="switch" :checked="notifications.soundsToo" @change="flip($event, (on) => api.quietOption('sounds', on), () => notifications.soundsToo)" />
        </label>
      </div>
    </div>

    <div class="section part">
      <div id="notif-muted" class="section-label">{{ t("notifications.muted") }}</div>
      <div class="chips" role="group" aria-labelledby="notif-muted">
        <span v-for="project in muted" :key="project.path" class="chip" :title="project.path">
          <span class="chip-name">{{ project.name }}</span>
          <button type="button" class="chip-off" :aria-label="t('notifications.unmuteName', { name: project.name })" :title="t('notifications.unmute')" @click="api.muteProject(project.path, false)">
            <Icon name="close" :size="11" />
          </button>
        </span>
        <button type="button" class="add" aria-haspopup="menu" :disabled="!unmuted.length" @click="pickProject">
          {{ t("notifications.muteAdd") }}
        </button>
      </div>
    </div>
  </div>

  <PhoneSection />
</template>

<style scoped src="./settings.css"></style>
<style scoped>
/* The tab's parts, as far apart as its rows are in the other tabs. */
.notif {
  display: flex;
  flex-direction: column;
  gap: 16px;
  font-size: 13px;
}

.times {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text-subtle);
}

.times input {
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-variant-numeric: tabular-nums;
}

/* The hours don't count: their times are there to read, out of reach. One dimming in every
   engine (WebKit greys a disabled field's text by itself, on top of any other). */
.times.off {
  color: var(--text-faint);
}

.times input:disabled {
  opacity: 1;
  color: var(--text-faint);
  -webkit-text-fill-color: var(--text-faint);
}

.chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.chip {
  max-width: 100%;
  height: 26px;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 3px 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: 13px;
  background: var(--bg-control);
  font-size: 12px;
}

.chip-name {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.chip-off {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: var(--text-muted);
}

.chip-off:hover {
  background: var(--bg-control-hover);
  color: var(--text-strong);
}

/* A way to pick, not a setting: no ground of its own until the pointer is on it. */
.add {
  height: 26px;
  padding: 0 10px;
  border: 1px dashed var(--line-bold);
  border-radius: 13px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.add:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text);
}

.add:disabled {
  opacity: 0.5;
}
</style>
