<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { language, t } from "../../lib/i18n";
import { clockText } from "../../lib/fuel";
import { noonOf, refusalText, weekdayLong, weekdayOf } from "../../lib/laps";
import { dayLong } from "../../lib/period";
import { api } from "../../lib/store";

/**
 * A job's switch turned off for a day in the Takvim popover (TN2): that day alone, or its weekday
 * from now on (which changes the job's own schedule). Uygula takes the day off; Vazgeç, Esc or a
 * click outside leaves the job as it was, and the switch comes back on.
 */
const props = defineProps<{
  lap: string;
  name: string;
  date: string;
  /** The day's first moment of the job, for the line under the title. */
  at: number | null;
  /** The job's `when` can lose a weekday (not once, not at a refill). */
  weekly: boolean;
}>();
const emit = defineEmits<{ close: [applied: boolean] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const applyButton = ref<HTMLButtonElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();

const every = ref(false);
const busy = ref(false);
const error = ref("");
let applied = false;

onMounted(() => openUnfocused(dialog.value, () => applyButton.value));

const weekday = computed(() => weekdayLong(weekdayOf(props.date)));
const whole = computed(() => new Date(noonOf(props.date)).toLocaleDateString(language.value, { day: "numeric", month: "long", weekday: "long" }));
const line = computed(() => (props.at === null ? dayLong(props.date) : `${dayLong(props.date)} · ${clockText(props.at, language.value)}`));

async function apply(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await api.lapDayOff(props.lap, props.date, props.weekly && every.value);
    applied = true;
    dialog.value?.close();
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="dialog day-off"
    :aria-label="t('laps.dayOff.title', { name })"
    @close="emit('close', applied)"
    @pointerdown="backdrop.down"
    @click="backdrop.click"
  >
    <div class="dialog-head">
      <span class="dialog-title grow title">{{ t("laps.dayOff.title", { name }) }}</span>
      <button type="button" class="icon-button" :aria-label="t('common.close')" :title="t('common.close')" @click="dialog?.close()">
        <Icon name="close" :size="14" />
      </button>
    </div>
    <div class="body" role="radiogroup" :aria-label="t('laps.dayOff.title', { name })">
      <span class="when">{{ line }}</span>
      <label :class="['choice', { on: !every }]">
        <input v-model="every" type="radio" name="day-off" :value="false" />
        <span class="choice-text">
          <span class="choice-title">{{ t("laps.dayOff.only", { date: whole }) }}</span>
          <span class="hint">{{ t("laps.dayOff.only.hint", { weekday }) }}</span>
        </span>
      </label>
      <label v-if="weekly" :class="['choice', { on: every }]">
        <input v-model="every" type="radio" name="day-off" :value="true" />
        <span class="choice-text">
          <span class="choice-title">{{ t("laps.dayOff.every", { weekday }) }}</span>
          <span class="hint">{{ t("laps.dayOff.every.hint", { weekday }) }}</span>
        </span>
      </label>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </div>
    <div class="dialog-foot">
      <span class="grow"></span>
      <button type="button" class="control" @click="dialog?.close()">{{ t("common.cancel") }}</button>
      <button ref="applyButton" type="button" class="control primary" :disabled="busy" :aria-busy="busy" @click="apply">
        <span :class="{ hidden: busy }">{{ t("laps.dayOff.apply") }}</span>
        <Spinner v-if="busy" class="over" :size="12" />
      </button>
    </div>
  </dialog>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.day-off {
  width: 470px;
}

.title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 20px;
}

.when {
  font-size: 12.5px;
  color: var(--text-muted);
}

.choice {
  display: flex;
  align-items: flex-start;
  gap: 11px;
  padding: 12px 13px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--bg-well);
  cursor: pointer;
}

.choice.on {
  border-color: var(--line-strong);
  background: var(--bg-lit);
}

.choice input {
  width: 15px;
  height: 15px;
  margin: 1px 0 0;
  flex-shrink: 0;
  accent-color: var(--text-strong);
}

.choice-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.choice-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-strong);
}
</style>
