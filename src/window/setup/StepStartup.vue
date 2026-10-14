<script setup lang="ts">
import { computed, ref } from "vue";

import { t, type Key } from "../../lib/i18n";
import { shortcutCaps, useSwitch } from "../../lib/setup";
import { api, snapshot } from "../../lib/store";
import { towerSettings } from "../../lib/tower";
import type { TowerSettings } from "../../lib/types";

/**
 * The fifth step: the quick switcher's shortcut, starting at login and the mini tower (shown or
 * not, its layout, its own shortcut), each a switch, and the button on to the next step. The keys
 * are only shown here (Settings records new ones). `next`: the button was pressed.
 */
const emit = defineEmits<{ next: [] }>();

const caps = computed(() => shortcutCaps(snapshot.value?.settings.shortcutKeys ?? ""));

const { on: shortcutOn, error: shortcutError, set: setShortcut } = useSwitch("shortcut");
// A development run can't start at login: the core says why, and the switch goes back.
const { on: loginOn, error: loginError, set: setLogin } = useSwitch("launchAtLogin");

/* ---------- the mini tower: by its own commands, as Settings › General's part has it ---------- */

const FORMS: Array<{ id: TowerSettings["form"]; name: Key }> = [
  { id: "list", name: "tower.form.list" },
  { id: "strip", name: "tower.form.strip" },
];

const tower = computed(() => towerSettings(snapshot.value?.settings.tower));
const towerCaps = computed(() => shortcutCaps(tower.value.shortcutKeys));
/** Why the core didn't take a change: the shortcut's keys belong to another app, or to the quick switcher. */
const towerError = ref("");

/** A switch as just clicked goes to the core; one the core refuses goes back to what is stored. */
async function flip(event: Event, send: (on: boolean) => Promise<unknown>, stored: () => boolean): Promise<void> {
  const box = event.target as HTMLInputElement;
  towerError.value = "";
  try {
    await send(box.checked);
  } catch (reason) {
    box.checked = stored();
    towerError.value = String(reason);
  }
}

function setForm(form: TowerSettings["form"]): void {
  if (tower.value.form !== form) void api.towerForm(form);
}
</script>

<template>
  <section class="pane">
    <div class="head">
      <span class="head-text">
        <span class="title">{{ t("setup.step.startup") }}</span>
      </span>
    </div>

    <div class="block options">
      <div class="option">
        <label class="opt">
          <input type="checkbox" role="switch" :checked="shortcutOn" @change="setShortcut(($event.target as HTMLInputElement).checked)" />
          <span class="opt-text">
            <span>{{ t("settings.shortcutToggle") }}</span>
            <span class="hint">{{ t("setup.startup.shortcutHint") }}</span>
          </span>
        </label>
        <span v-if="caps.length" :class="['caps', { off: !shortcutOn }]">
          <kbd v-for="(cap, index) in caps" :key="index">{{ cap }}</kbd>
        </span>
      </div>
      <p v-if="shortcutError" class="error" role="alert">{{ shortcutError }}</p>

      <div class="option">
        <label class="opt">
          <input type="checkbox" role="switch" :checked="loginOn" @change="setLogin(($event.target as HTMLInputElement).checked)" />
          <span class="opt-text">
            <span>{{ t("settings.launchAtLogin") }}</span>
          </span>
        </label>
      </div>
      <p v-if="loginError" class="error" role="alert">{{ loginError }}</p>

      <!-- The mini tower: whether it shows and in which layout, and under it its own shortcut. -->
      <div class="option tower" role="group" :aria-label="t('tower.title')">
        <div class="line">
          <label class="opt">
            <input type="checkbox" role="switch" :checked="tower.on" @change="flip($event, api.towerShow, () => tower.on)" />
            <span class="opt-text">
              <span>{{ t("tower.show") }}</span>
            </span>
          </label>
          <span class="seg" role="group" :aria-label="t('tower.form')">
            <button v-for="form in FORMS" :key="form.id" type="button" :aria-pressed="tower.form === form.id" @click="setForm(form.id)">{{ t(form.name) }}</button>
          </span>
        </div>
        <div class="line">
          <label class="opt">
            <input type="checkbox" role="switch" :checked="tower.shortcut" @change="flip($event, (on) => api.towerOption('shortcut', on), () => tower.shortcut)" />
            <span class="opt-text">
              <span>{{ t("setup.startup.towerShortcut") }}</span>
            </span>
          </label>
          <span v-if="towerCaps.length" :class="['caps', { off: !tower.shortcut }]">
            <kbd v-for="(cap, index) in towerCaps" :key="index">{{ cap }}</kbd>
          </span>
        </div>
      </div>
      <p v-if="towerError" class="error" role="alert">{{ towerError }}</p>
    </div>

    <div class="foot">
      <button type="button" class="control primary" @click="emit('next')">{{ t("setup.next") }}</button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>

<style scoped>
/* Rows under hairlines, as Settings' Track tab has its own. */
.options {
  gap: 0;
  padding: 0;
}

.option {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px 0;
}

.option ~ .option {
  border-top: 1px solid var(--line);
}

.option .opt {
  flex: 1 1 auto;
}

.options .error {
  margin: -6px 0 12px 38px;
}

/* The mini tower's row: two lines, the second (its shortcut) under the first with no hairline
   between them, as one thing's. */
.option.tower {
  flex-direction: column;
  align-items: stretch;
  gap: 12px;
}

.line {
  display: flex;
  align-items: center;
  gap: 16px;
  min-height: 24px;
}

.line .opt {
  flex: 1 1 auto;
}

/* The shortcut as key caps; dimmed while it is off. */
.caps {
  flex-shrink: 0;
  display: inline-flex;
  gap: 4px;
}

.caps.off {
  opacity: 0.45;
}

kbd {
  min-width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0 6px;
  border: 1px solid var(--line-strong);
  border-radius: 6px;
  background: var(--bg-key);
  font-size: 12px;
  color: var(--text);
  white-space: nowrap;
}

/* macOS writes its keys as glyphs, which read small at the letters' size and aren't in the
   mono face anyway. */
:root[data-platform="mac"] kbd {
  font-family: var(--font);
  font-size: 14px;
}
</style>
