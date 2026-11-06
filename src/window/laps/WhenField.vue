<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

import { agentKey } from "../../lib/format";
import { clockText } from "../../lib/fuel";
import { language, t, type Key } from "../../lib/i18n";
import { isWeekdays, momentsText, refusalText, weekdayShort, type LapKind, type LapWhen } from "../../lib/laps";
import { api } from "../../lib/store";
import type { Agent } from "../../lib/types";

/**
 * Ne zaman, in the edit dialog: Her gün, Hafta içi, Aralık, Tek sefer, Dolunca, Cron, each with
 * its fields, and under them the next four moments as the core works them out, or why it refuses
 * them: asked a moment after the last change to a field, at once when another of these, a kind,
 * an agent or an account is picked; a line of the form left is hidden until the new one comes, and
 * an empty Cron is asked nothing (the user's word, 2026-11-06). The days' chips say the job's weekdays off as
 * well: one the Takvim popover turned off ("Bundan sonra her Çarşamba") shows off here, and turned
 * back on it is on again.
 */
const props = defineProps<{ when: LapWhen; weekdaysOff: number[]; kind: LapKind; agent: Agent; account: string | null }>();
const emit = defineEmits<{ "update:when": [when: LapWhen]; "update:weekdaysOff": [days: number[]] }>();

type Preset = "daily" | "weekdays" | "every" | "once" | "refill" | "cron";

const PRESETS: Array<{ id: Preset; label: Key }> = [
  { id: "daily", label: "laps.when.daily" },
  { id: "weekdays", label: "laps.when.weekdays" },
  { id: "every", label: "laps.when.every" },
  { id: "once", label: "laps.when.once" },
  { id: "refill", label: "laps.when.refill" },
  { id: "cron", label: "laps.when.cron" },
];

const preset = computed<Preset>(() => {
  const when = props.when;
  if (when.kind === "daily") return isWeekdays(when.days) ? "weekdays" : "daily";
  return when.kind;
});

/** The time a form of `when` has, carried to the next one picked. */
function timeOf(when: LapWhen): string {
  if (when.kind === "daily") return when.at;
  if (when.kind === "once") {
    const d = new Date(when.at);
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }
  if (when.kind === "every" && when.from) return when.from;
  return "09:00";
}

const pad = (n: number): string => String(n).padStart(2, "0");

function clock(at: number): string {
  return clockText(at, language.value);
}

function pick(id: Preset): void {
  if (id === preset.value) return;
  const at = timeOf(props.when);
  const days = props.when.kind === "daily" || props.when.kind === "every" ? props.when.days : [];
  let next: LapWhen;
  switch (id) {
    case "daily":
      next = { kind: "daily", at, days: isWeekdays(days) ? [] : days };
      break;
    case "weekdays":
      next = { kind: "daily", at, days: [1, 2, 3, 4, 5] };
      break;
    case "every":
      next = { kind: "every", minutes: 240, from: null, to: null, days: isWeekdays(days) ? days : [] };
      break;
    case "once": {
      const [h, m] = at.split(":").map(Number);
      const day = new Date();
      day.setHours(h, m, 0, 0);
      if (day.getTime() <= Date.now()) day.setDate(day.getDate() + 1);
      next = { kind: "once", at: day.getTime() };
      break;
    }
    case "refill":
      next = { kind: "refill" };
      break;
    case "cron":
      next = { kind: "cron", expr: "" };
      break;
  }
  emit("update:when", next);
}

/* ---------- the days ---------- */

const DAYS = [1, 2, 3, 4, 5, 6, 7];

const days = computed<number[] | null>(() => (props.when.kind === "daily" || props.when.kind === "every" ? props.when.days : null));

function dayOn(day: number): boolean {
  const list = days.value ?? [];
  return (list.length === 0 || list.includes(day)) && !props.weekdaysOff.includes(day);
}

/** A chip pressed: the day leaves the job's days, or comes back (off the weekdays off too). */
function toggleDay(day: number): void {
  const when = props.when;
  if (when.kind !== "daily" && when.kind !== "every") return;
  const on = dayOn(day);
  let list = when.days.length ? [...when.days] : [...DAYS];
  if (on) list = list.filter((d) => d !== day);
  else {
    if (!list.includes(day)) list.push(day);
    if (props.weekdaysOff.includes(day)) emit("update:weekdaysOff", props.weekdaysOff.filter((d) => d !== day));
  }
  list.sort((a, b) => a - b);
  // Every day is no day named; none is no run at all, which the core refuses with its reason.
  emit("update:when", { ...when, days: list.length === 7 ? [] : list });
}

/* ---------- times and figures, as typed ---------- */

/** `9` → `09:00`, `930` → `09:30`, `9:5` → `09:05`; null when it isn't a time. */
function readTime(text: string): string | null {
  const clean = text.trim();
  if (!clean) return null;
  let h: number;
  let m: number;
  const colon = /^(\d{1,2})[:.](\d{1,2})$/.exec(clean);
  if (colon) {
    h = Number(colon[1]);
    m = Number(colon[2]);
  } else if (/^\d{1,4}$/.test(clean)) {
    const digits = clean.length <= 2 ? clean.padEnd(clean.length + 2, "0") : clean.padStart(4, "0");
    h = Number(digits.slice(0, -2));
    m = Number(digits.slice(-2));
  } else return null;
  if (h > 23 || m > 59) return null;
  return `${pad(h)}:${pad(m)}`;
}

const timeError = ref(false);

function setTime(field: "at" | "from" | "to", event: Event): void {
  const input = event.target as HTMLInputElement;
  const when = props.when;
  const value = readTime(input.value);
  // An emptied window's end is no end: the whole day.
  if (value === null && !(field !== "at" && !input.value.trim())) {
    timeError.value = true;
    return;
  }
  timeError.value = false;
  if (when.kind === "daily" && field === "at" && value) emit("update:when", { ...when, at: value });
  else if (when.kind === "every" && field !== "at") emit("update:when", { ...when, [field]: value });
  else if (when.kind === "once" && field === "at" && value) {
    const d = new Date(when.at);
    const [h, m] = value.split(":").map(Number);
    d.setHours(h, m, 0, 0);
    emit("update:when", { ...when, at: d.getTime() });
  }
  input.value = value ?? "";
}

/** Aralık's figure and its unit (minutes, or hours when it is whole hours). */
const unit = computed<"minutes" | "hours">(() => (props.when.kind === "every" && props.when.minutes % 60 === 0 ? "hours" : "minutes"));
const figure = computed(() => (props.when.kind === "every" ? (unit.value === "hours" ? props.when.minutes / 60 : props.when.minutes) : 0));

function setFigure(event: Event): void {
  const input = event.target as HTMLInputElement;
  const when = props.when;
  const n = Math.round(Number(input.value));
  if (when.kind !== "every" || !Number.isFinite(n) || n < 1) {
    input.value = String(figure.value);
    return;
  }
  emit("update:when", { ...when, minutes: unit.value === "hours" ? n * 60 : n });
}

function setUnit(event: Event): void {
  const when = props.when;
  if (when.kind !== "every") return;
  const to = (event.target as HTMLSelectElement).value as "minutes" | "hours";
  emit("update:when", { ...when, minutes: to === "hours" ? Math.max(1, figure.value) * 60 : figure.value });
}

/** Tek sefer's day, as the date field takes it. */
const onceDate = computed(() => {
  if (props.when.kind !== "once") return "";
  const d = new Date(props.when.at);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
});

function setDate(event: Event): void {
  const when = props.when;
  const value = (event.target as HTMLInputElement).value;
  if (when.kind !== "once" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return;
  const [y, mo, d] = value.split("-").map(Number);
  const at = new Date(when.at);
  at.setFullYear(y, mo - 1, d);
  emit("update:when", { ...when, at: at.getTime() });
}

function setCron(event: Event): void {
  const when = props.when;
  if (when.kind === "cron") emit("update:when", { ...when, expr: (event.target as HTMLInputElement).value.trim() });
}

/* ---------- the next moments ---------- */

const moments = ref<number[]>([]);
const refused = ref("");
let timer: ReturnType<typeof setTimeout> | undefined;
let asked = 0;
/** The form and the switches the line under the fields was last asked for. */
let shownForm: Preset | null = null;
let shownSwitches = "";

watch(
  () => JSON.stringify([props.when, props.kind, props.agent, props.account]),
  () => {
    clearTimeout(timer);
    const ask = ++asked;
    // Another form picked: what was said of the one left goes at once. That or another kind, agent
    // or account is nothing typed, so it is asked at once too.
    const switches = JSON.stringify([props.kind, props.agent, props.account]);
    const formChanged = preset.value !== shownForm;
    const picked = formChanged || switches !== shownSwitches;
    shownForm = preset.value;
    shownSwitches = switches;
    if (formChanged) {
      moments.value = [];
      refused.value = "";
    }
    if (props.when.kind === "cron" && !props.when.expr) {
      moments.value = [];
      refused.value = "";
      return;
    }
    timer = setTimeout(
      () => {
        api
          .lapPreview(props.when, props.kind, props.agent, props.account)
          .then((list) => {
            if (ask !== asked) return;
            moments.value = list;
            refused.value = "";
          })
          .catch((reason) => {
            if (ask !== asked) return;
            moments.value = [];
            refused.value = refusalText(reason);
          });
      },
      picked ? 0 : 250,
    );
  },
  { immediate: true },
);

onBeforeUnmount(() => clearTimeout(timer));

const next = computed(() => (moments.value.length ? t("laps.when.next", { moments: momentsText(moments.value) }) : ""));
</script>

<template>
  <div class="when-field">
    <div class="seg small" role="radiogroup" :aria-label="t('laps.fact.when')">
      <button v-for="one in PRESETS" :key="one.id" type="button" role="radio" :aria-checked="preset === one.id" :class="{ on: preset === one.id }" @click="pick(one.id)">
        {{ t(one.label) }}
      </button>
    </div>

    <div class="line">
      <template v-if="when.kind === 'daily'">
        <input class="input num time" maxlength="5" :value="when.at" :aria-label="t('laps.when.time')" @change="setTime('at', $event)" />
      </template>

      <template v-else-if="when.kind === 'every'">
        <span>{{ t("laps.when.everyLead") }}</span>
        <input class="input num" inputmode="numeric" maxlength="4" :value="figure" :aria-label="t('laps.when.every')" @change="setFigure" />
        <select class="select small-select" :value="unit" :aria-label="t('laps.when.unit')" @change="setUnit">
          <option value="minutes">{{ t("laps.when.unit.minutes") }}</option>
          <option value="hours">{{ t("laps.when.unit.hours") }}</option>
        </select>
        <input class="input num time" maxlength="5" :value="when.from ?? ''" placeholder="00:00" :aria-label="t('laps.when.from')" @change="setTime('from', $event)" />
        <span>–</span>
        <input class="input num time" maxlength="5" :value="when.to ?? ''" placeholder="23:59" :aria-label="t('laps.when.to')" @change="setTime('to', $event)" />
      </template>

      <template v-else-if="when.kind === 'once'">
        <input class="input date" type="date" :value="onceDate" :aria-label="t('laps.when.date')" @change="setDate" />
        <input class="input num time" maxlength="5" :value="clock(when.at)" :aria-label="t('laps.when.time')" @change="setTime('at', $event)" />
      </template>

      <span v-else-if="when.kind === 'refill'" class="hint">{{ t(agentKey("laps.when.refill.hint", agent)) }}</span>

      <template v-else-if="when.kind === 'cron'">
        <input class="input mono cron" spellcheck="false" :value="when.expr" placeholder="0 9 * * 1-5" :aria-label="t('laps.when.cron')" @change="setCron" />
      </template>

      <div v-if="days !== null" class="seg small days" role="group" :aria-label="t('laps.when.days')">
        <button v-for="day in DAYS" :key="day" type="button" :aria-pressed="dayOn(day)" :class="{ on: dayOn(day) }" @click="toggleDay(day)">{{ weekdayShort(day) }}</button>
      </div>
    </div>

    <span v-if="when.kind === 'cron'" class="hint">{{ t("laps.when.cron.hint") }}</span>
    <span v-if="timeError" class="error">{{ t("laps.when.badTime") }}</span>
    <span v-else-if="refused" class="error">{{ refused }}</span>
    <span v-else-if="next" class="hint next">{{ next }}</span>
  </div>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.when-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.line {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12.5px;
  color: var(--text);
}

.input.num.time {
  width: 62px;
}

.small-select {
  height: 26px;
  font-size: 12.5px;
}

.input.date {
  height: 26px;
  font-size: 12.5px;
}

.cron {
  width: 220px;
  height: 26px;
}

.days {
  margin-left: 4px;
}

.next {
  font-variant-numeric: tabular-nums;
}
</style>
