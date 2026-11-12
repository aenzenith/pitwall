<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { language, t, type Key } from "../../lib/i18n";
import { shiftDay } from "../../lib/period";
import { isMac, isWindows } from "../../lib/platform";
import { api, today } from "../../lib/store";
import type { ExportPreview } from "../../lib/types";

/**
 * The Today page's CSV export (core/export.rs): which days go into it, picked from the day the page
 * shows, its day, week or month, or two dates; what it would hold; then the core's save dialog
 * and its file. The page never names a path: the core writes the file and shows it in Finder.
 * While the save dialog is up nothing closes this one, as with the backup's.
 */
const props = defineProps<{ day: string }>();
const emit = defineEmits<{ close: [] }>();

type Mode = "day" | "week" | "month" | "custom";
const modes: Array<{ id: Mode; label: Key }> = [
  { id: "day", label: "day.export.day" },
  { id: "week", label: "day.export.week" },
  { id: "month", label: "day.export.month" },
  { id: "custom", label: "day.export.custom" },
];

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const saveButton = ref<HTMLButtonElement | null>(null);
const doneButton = ref<HTMLButtonElement | null>(null);

const mode = ref<Mode>("week");
const busy = ref(false);
const error = ref("");
/** The file written, once it is. */
const saved = ref<string | null>(null);
const preview = ref<ExportPreview | null>(null);

/** The day a week starts on in the app's language (0: Sunday), Monday where it can't be told. */
function weekStart(): number {
  try {
    const locale = new Intl.Locale(language.value) as Intl.Locale & { getWeekInfo?: () => { firstDay: number }; weekInfo?: { firstDay: number } };
    const info = locale.getWeekInfo?.() ?? locale.weekInfo;
    if (info) return info.firstDay % 7;
  } catch {
    // An engine without week info.
  }
  return 1;
}

function weekday(date: string): number {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day, 12).getDay();
}

/** The stretch the choice names around the day shown, never past today. */
function around(day: string, choice: Exclude<Mode, "custom">): [string, string] {
  if (choice === "day") return [day, day];
  let from: string;
  let to: string;
  if (choice === "week") {
    from = shiftDay(day, -((weekday(day) - weekStart() + 7) % 7));
    to = shiftDay(from, 6);
  } else {
    from = `${day.slice(0, 8)}01`;
    const [year, month] = day.split("-").map(Number);
    to = `${day.slice(0, 8)}${String(new Date(year, month, 0).getDate()).padStart(2, "0")}`;
  }
  return [from, to < today.value ? to : today.value];
}

/** The two dates of "Custom", from the week around the day shown. */
const [weekFrom, weekTo] = around(props.day, "week");
const customFrom = ref(weekFrom);
const customTo = ref(weekTo);

const range = computed<[string, string]>(() => {
  if (mode.value !== "custom") return around(props.day, mode.value);
  return customFrom.value <= customTo.value ? [customFrom.value, customTo.value] : [customTo.value, customFrom.value];
});

/** The earliest day there can be a record of: the core says, the day files being kept 90 days. */
const oldest = computed(() => preview.value?.oldest ?? shiftDay(today.value, -90));

let asked = 0;
watch(
  range,
  async ([from, to]) => {
    const ask = ++asked;
    try {
      const found = await api.dayExportPreview(from, to);
      if (ask === asked) {
        preview.value = found;
        error.value = "";
      }
    } catch (reason) {
      if (ask === asked) {
        preview.value = null;
        error.value = String(reason);
      }
    }
  },
  { immediate: true },
);

function noon(date: string): Date {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day, 12);
}

/** `9–11 Nov 2026`, as the language writes a stretch of days. */
const rangeLabel = computed(() => {
  const shown = preview.value;
  if (!shown) return "";
  const format = new Intl.DateTimeFormat(language.value, { day: "numeric", month: "short", year: "numeric" }) as Intl.DateTimeFormat & {
    formatRange?: (start: Date, end: Date) => string;
  };
  if (shown.from === shown.to) return format.format(noon(shown.from));
  return format.formatRange?.(noon(shown.from), noon(shown.to)) ?? `${format.format(noon(shown.from))} – ${format.format(noon(shown.to))}`;
});

const meta = computed(() => {
  const shown = preview.value;
  if (!shown) return "";
  if (shown.rows === 0) return `${rangeLabel.value} · ${t("day.export.empty")}`;
  return `${rangeLabel.value} · ${t("day.export.projects", { count: shown.projects })} · ${t("day.export.rows", { count: shown.rows })}`;
});

const nothing = computed(() => !preview.value || preview.value.rows === 0);
const revealLabel = computed(() => t(isMac ? "files.reveal.mac" : isWindows ? "files.reveal.windows" : "files.reveal.linux"));

function setDate(which: "from" | "to", event: Event): void {
  const value = (event.target as HTMLInputElement).value;
  // A field emptied, or a date the field let through outside its bounds, changes nothing.
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return;
  const kept = value < oldest.value ? oldest.value : value > today.value ? today.value : value;
  if (which === "from") customFrom.value = kept;
  else customTo.value = kept;
}

onMounted(() => {
  // Saving is the answer Return gives; once saved, closing is.
  openUnfocused(dialog.value, () => (saved.value === null ? saveButton.value : doneButton.value));
});

async function save(): Promise<void> {
  if (busy.value || nothing.value || !preview.value) return;
  busy.value = true;
  error.value = "";
  try {
    // Null: the save dialog was cancelled, and this one stays for another try.
    saved.value = await api.dayExport(preview.value.from, preview.value.to);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

function reveal(): void {
  void api.dayExportReveal();
}

function onCancel(event: Event): void {
  if (busy.value) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!busy.value) backdrop.click(event);
}
</script>

<template>
  <dialog ref="dialog" class="dialog day-export" :aria-label="t('day.export.title')" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="onBackdrop">
    <div class="body">
      <div class="title">{{ t("day.export.title") }}</div>

      <template v-if="saved === null">
        <div class="field">
          <span id="day-export-range" class="label">{{ t("day.export.range") }}</span>
          <div class="seg" role="group" aria-labelledby="day-export-range">
            <button v-for="each in modes" :key="each.id" type="button" :aria-pressed="mode === each.id" :disabled="busy" @click="mode = each.id">{{ t(each.label) }}</button>
          </div>
          <div v-if="mode === 'custom'" class="dates">
            <input class="date" type="date" :min="oldest" :max="today" :value="customFrom" :aria-label="t('day.export.from')" :disabled="busy" @change="setDate('from', $event)" />
            <span class="dash" aria-hidden="true">–</span>
            <input class="date" type="date" :min="oldest" :max="today" :value="customTo" :aria-label="t('day.export.to')" :disabled="busy" @change="setDate('to', $event)" />
          </div>
          <span class="meta" aria-live="polite">{{ meta }}</span>
        </div>

        <div v-if="preview && preview.columns.length" class="field">
          <span class="label">{{ t("day.export.columns") }}</span>
          <div class="columns">
            <span v-for="column in preview.columns" :key="column" class="column">{{ column }}</span>
          </div>
        </div>

        <p v-if="error" class="error" role="alert">{{ error }}</p>

        <div class="actions">
          <button type="button" class="control" :disabled="busy" @click="dialog?.close()">{{ t("common.cancel") }}</button>
          <button ref="saveButton" type="button" class="control primary" :disabled="busy || nothing" :aria-busy="busy" @click="save">
            <span :class="{ hidden: busy }">{{ t("day.export.save") }}</span>
            <Spinner v-if="busy" class="over" :size="12" />
          </button>
        </div>
      </template>

      <template v-else>
        <p class="saved" role="status">
          <span class="saved-mark"><Icon name="check" :size="12" /></span>
          <span class="saved-text">{{ t("day.export.saved", { file: saved }) }}</span>
        </p>
        <div class="actions">
          <button type="button" class="control" @click="reveal">{{ revealLabel }}</button>
          <button ref="doneButton" type="button" class="control primary" @click="dialog?.close()">{{ t("day.export.done") }}</button>
        </div>
      </template>
    </div>
  </dialog>
</template>

<style scoped src="../backup/dialog.css"></style>

<style scoped>
.day-export {
  width: 452px;
}

.body {
  gap: 14px;
}

.field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  min-width: 0;
}

.label {
  font-size: 12px;
  color: var(--text-muted);
}

/* One of a few, the picked one lit: Settings' segmented control. */
.seg {
  display: inline-flex;
  max-width: 100%;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-seg);
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

.seg button:hover:not([aria-pressed="true"]):not(:disabled) {
  color: var(--text);
}

.seg button[aria-pressed="true"] {
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
  color: var(--text-strong);
}

.dates {
  display: flex;
  align-items: center;
  gap: 8px;
}

.date {
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.dash {
  color: var(--text-subtle);
}

.meta {
  min-height: 16px;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.columns {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.column {
  flex-shrink: 0;
  white-space: nowrap;
  height: 20px;
  padding: 0 7px;
  display: inline-flex;
  align-items: center;
  border-radius: 5px;
  background: var(--bg-icon-hover);
  color: var(--text-muted);
  font-size: 11px;
}

.saved {
  margin: 0;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  font-size: 12.5px;
  line-height: 1.5;
}

.saved-mark {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--bg-icon-hover);
  color: var(--run-text);
}

.saved-text {
  min-width: 0;
  overflow-wrap: anywhere;
}
</style>
