<script setup lang="ts">
import { computed } from "vue";

import { yesterday } from "../lib/day";
import { t } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import { ALL, dayMonth, nextDay, periodLabel, previousDay, type Period } from "../lib/period";
import { tabKey } from "../lib/tabs";
import Icon from "./Icon.vue";

/**
 * The stretch of time a page shows: every day, an earlier day or today, picked as the Today page
 * picks its day. The arrows step a day back and forth; the middle tab is yesterday until an
 * earlier day is stepped to, then that day. That tab keeps one width, the widest any of its
 * labels takes, so stepping through the days never moves the arrows from under the pointer.
 *
 * `all` false: without every day's, for a page that shows one day at a time (Today).
 *
 * A bar too narrow for all of it shows one button with the same choices in a menu: the page's own
 * container query hides `period-full` and shows `period-compact`, and in a bar narrower still
 * hides that button's text too (`compact-text`, `compact-more`): its icon alone, lit while the
 * days shown aren't today.
 */
const props = withDefaults(defineProps<{ all?: boolean }>(), { all: true });
const period = defineModel<Period>({ required: true });

/** The earlier day shown, if one is. */
const day = computed(() => (period.value !== null && period.value !== ALL ? period.value : null));

/** Every label the day tab takes: yesterday's name, and a day late in each month. */
const dayLabels = computed(() => {
  const year = new Date().getFullYear();
  const months = Array.from({ length: 12 }, (_, month) => dayMonth(`${year}-${String(month + 1).padStart(2, "0")}-28`));
  return [...new Set([t("day.yesterday"), ...months])];
});

/** `widths`: the labels a tab is as wide as the widest of (its own alone, unless it changes). */
const tabs = computed<Array<{ id: string; label: string; on: boolean; value: Period; widths: string[] }>>(() => [
  ...(props.all ? [{ id: "all", label: t("period.all"), on: period.value === ALL, value: ALL, widths: [] }] : []),
  {
    id: "day",
    label: day.value && day.value !== yesterday() ? dayMonth(day.value) : t("day.yesterday"),
    on: day.value !== null,
    value: day.value ?? yesterday(),
    widths: dayLabels.value,
  },
  { id: "today", label: t("day.today"), on: period.value === null, value: null, widths: [] },
]);

/** The narrow bar's button says the tab in force, and is as wide as the widest of them all. */
const current = computed(() => tabs.value.find((tab) => tab.on)?.label ?? "");
const compactLabels = computed(() => [...new Set([...tabs.value.map((tab) => tab.label), ...dayLabels.value])]);

const before = computed(() => previousDay(period.value));
const after = computed(() => nextDay(period.value));

function step(to: Period | undefined): void {
  if (to !== undefined) period.value = to;
}

/** The tabs are one Tab stop: ←/→ (Home, End) pick and focus the next. */
function onTabKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, tabs.value.length, at);
  if (next !== null) period.value = tabs.value[next].value;
}

const menu = useNativeMenu();

/** The narrow bar's button: the same choices, the one in force checked, under the button. */
function openMenu(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const entries: MenuEntry[] = [
    ...tabs.value.map((tab) => ({ text: tab.label, checked: tab.on, action: () => (period.value = tab.value) })),
    "separator",
    { text: t("day.previous"), enabled: before.value !== undefined, action: () => step(before.value) },
    { text: t("day.next"), enabled: after.value !== undefined, action: () => step(after.value) },
  ];
  void menu.popup({ clientX: box.left, clientY: box.bottom + 4 }, entries);
}
</script>

<template>
  <div class="period">
    <div class="period-full">
      <button type="button" class="arrow" :aria-label="t('day.previous')" :title="t('day.previous')" :disabled="before === undefined" @click="step(before)">
        <Icon name="back" :size="14" />
      </button>
      <div role="tablist" class="tabs" :aria-label="t('period.label')">
        <button
          v-for="(tab, i) in tabs"
          :key="tab.id"
          type="button"
          role="tab"
          :tabindex="tab.on ? 0 : -1"
          :aria-selected="tab.on"
          :class="{ on: tab.on }"
          @click="period = tab.value"
          @keydown="onTabKey($event, i)"
        >
          <span>{{ tab.label }}</span>
          <span v-for="text in tab.widths" :key="text" class="width" aria-hidden="true">{{ text }}</span>
        </button>
      </div>
      <button type="button" class="arrow" :aria-label="t('day.next')" :title="t('day.next')" :disabled="after === undefined" @click="step(after)">
        <Icon name="chevron" :size="14" />
      </button>
    </div>

    <button
      type="button"
      :class="['period-compact', { set: period !== null }]"
      aria-haspopup="menu"
      :aria-label="`${t('period.label')}: ${periodLabel(period)}`"
      :title="`${t('period.label')}: ${periodLabel(period)}`"
      @click="openMenu"
    >
      <Icon name="calendar" :size="13" />
      <span class="compact-text">
        <span>{{ current }}</span>
        <span v-for="text in compactLabels" :key="text" class="width" aria-hidden="true">{{ text }}</span>
      </span>
      <Icon name="chevron-down" :size="12" class="compact-more" />
    </button>
  </div>
</template>

<style scoped>
.period {
  flex-shrink: 0;
  display: flex;
}

.period-full {
  display: flex;
  align-items: center;
  gap: 2px;
}

.arrow {
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.arrow:hover:not(:disabled) {
  background: #262a33;
  color: var(--text-strong);
}

.arrow:disabled {
  opacity: 0.35;
}

/* As tall as the bar's other controls (30px). */
.tabs {
  display: flex;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-input);
}

/* A label and the labels it could be share one cell: the cell is as wide as the widest. */
.tabs button,
.compact-text {
  display: inline-grid;
  align-items: center;
  justify-items: center;
}

.tabs button > span,
.compact-text > span {
  grid-area: 1 / 1;
}

.width {
  visibility: hidden;
}

.tabs button {
  height: 24px;
  padding: 0 12px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text-subtle);
  white-space: nowrap;
}

.tabs button:hover:not(.on) {
  color: var(--text);
}

.tabs button.on {
  background: #23272e;
  color: var(--text-strong);
}

/* Out of sight until the page's bar runs out of room for the tabs. */
.period-compact {
  display: none;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
  color: #c7ccd3;
  white-space: nowrap;
}

.period-compact:hover {
  background: #2c3039;
}

.period-compact svg {
  flex-shrink: 0;
  color: var(--text-subtle);
}

/* Other days than today's show: said by the button itself, for when its text is out of sight. */
.period-compact.set,
.period-compact.set svg {
  color: var(--text-strong);
}

.period-compact.set {
  background: #2c3039;
}
</style>
