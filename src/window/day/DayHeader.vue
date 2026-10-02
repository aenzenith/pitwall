<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import { bounds, clock, clockRange, dayName, yesterday } from "../../lib/day";
import { language, t } from "../../lib/i18n";
import { dragRegion } from "../../lib/platform";
import type { DaySummary } from "../../lib/types";

/** `day`: the summary shown, or the last one while the next loads. */
const props = defineProps<{ day: DaySummary | null }>();
/** The day shown, `YYYY-MM-DD`; null is today. */
const date = defineModel<string | null>("date", { required: true });

const isYesterday = computed(() => date.value === yesterday());

function previous(): void {
  if (props.day) date.value = dayName(props.day.start - 1);
}

function next(): void {
  if (!props.day || props.day.today) return;
  const name = dayName(props.day.end + 1);
  date.value = name === dayName(Date.now()) ? null : name;
}

/** The day shown, known before its summary arrives, so the heading never jumps. */
const shownName = computed(() => date.value ?? (props.day?.today ? props.day.date : dayName(Date.now())));

const dateLabel = computed(() => {
  const [year, month, dayOfMonth] = shownName.value.split("-").map(Number);
  return new Date(year, month - 1, dayOfMonth, 12).toLocaleDateString(language.value, { weekday: "long", day: "numeric", month: "long" });
});

const subtitle = computed(() => {
  const d = props.day;
  // The previous day's range stays out while the next day loads.
  const span = d && d.date === shownName.value ? bounds(d.projects) : null;
  if (!d || !span) return dateLabel.value;
  const range = d.today ? t("day.untilNow", { time: clock(span[0]) }) : clockRange(span[0], span[1]);
  return `${dateLabel.value} · ${range}`;
});

/** The two day tabs are one Tab stop: ←/→ (Home, End) pick and focus the other. */
const dayTabs = ["yesterday", "today"] as const;
const dayTab = computed(() => (isYesterday.value ? "yesterday" : "today"));

function pickDay(which: (typeof dayTabs)[number]): void {
  date.value = which === "today" ? null : yesterday();
}

function onDayTabKey(event: KeyboardEvent): void {
  const at = dayTabs.indexOf((event.target as HTMLElement).dataset.day as (typeof dayTabs)[number]);
  if (at < 0) return;
  const n = dayTabs.length;
  const to = ({ ArrowRight: (at + 1) % n, ArrowLeft: (at - 1 + n) % n, Home: 0, End: n - 1 } as Record<string, number>)[event.key];
  if (to === undefined) return;
  event.preventDefault();
  pickDay(dayTabs[to]);
  ((event.currentTarget as HTMLElement).querySelector(`[data-day="${dayTabs[to]}"]`) as HTMLElement | null)?.focus();
}
</script>

<template>
  <header class="bar" :data-tauri-drag-region="dragRegion">
    <!-- Its heading and free space drag the window; the day buttons stay clickable. -->
    <div class="heading">
      <span class="title">{{ t("day.title") }}</span>
      <span class="subtitle">{{ subtitle }}</span>
    </div>
    <div class="days">
      <button type="button" class="arrow" :aria-label="t('day.previous')" :title="t('day.previous')" @click="previous">
        <Icon name="back" :size="14" />
      </button>
      <div role="tablist" class="tabs" @keydown="onDayTabKey">
        <button
          type="button"
          role="tab"
          data-day="yesterday"
          :tabindex="dayTab === 'yesterday' ? 0 : -1"
          :aria-selected="isYesterday"
          :class="{ on: isYesterday }"
          @click="pickDay('yesterday')"
        >
          {{ t("day.yesterday") }}
        </button>
        <button
          type="button"
          role="tab"
          data-day="today"
          :tabindex="dayTab === 'today' ? 0 : -1"
          :aria-selected="date === null"
          :class="{ on: date === null }"
          @click="pickDay('today')"
        >
          {{ t("day.today") }}
        </button>
      </div>
      <button type="button" class="arrow" :aria-label="t('day.next')" :title="t('day.next')" :disabled="!day || day.today" @click="next">
        <Icon name="chevron" :size="14" />
      </button>
    </div>
  </header>
</template>

<style scoped>
/* The same height and line as the projects toolbar, so switching never moves them. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

/* A line of its own from the start, so the title above never moves. */
.subtitle {
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.days {
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

.tabs {
  display: flex;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-input);
}

.tabs button {
  height: 26px;
  padding: 0 12px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
}

.tabs button.on {
  background: #23272e;
  color: var(--text-strong);
}
</style>
