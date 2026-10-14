<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import PeriodPicker from "../../components/PeriodPicker.vue";
import Spinner from "../../components/Spinner.vue";
import { bounds, clock, clockRange, dayName } from "../../lib/day";
import { flagOpening, openFlag } from "../../lib/flag";
import { language, t } from "../../lib/i18n";
import { dragRegion } from "../../lib/platform";
import type { DaySummary } from "../../lib/types";

/** `day`: the summary shown, or the last one while the next loads. */
const props = defineProps<{ day: DaySummary | null }>();
/** The day shown, `YYYY-MM-DD`; null is today. Picked as the Board and Sessions pages pick their
 * days (components/PeriodPicker), one day at a time. */
const date = defineModel<string | null>("date", { required: true });

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
</script>

<template>
  <header class="bar" :data-tauri-drag-region="dragRegion">
    <!-- Its heading and free space drag the window; the day buttons stay clickable. -->
    <div class="heading">
      <span class="title">{{ t("day.title") }}</span>
      <span class="subtitle">{{ subtitle }}</span>
    </div>
    <PeriodPicker v-model="date" :all="false" />
    <!-- The chequered flag: what is still open (window/FlagDialog). It reads the projects' Git
         state again first, so the button spins until the dialog is up. In a narrow bar it is its
         glyph alone: its words stay its name and its tooltip. -->
    <button type="button" class="flag" :aria-label="t('flag.day')" :title="t('flag.day')" :disabled="flagOpening" :aria-busy="flagOpening" @click="openFlag()">
      <span :class="['flag-label', { hidden: flagOpening }]"><Icon name="flag" :size="13" /> <span class="flag-text">{{ t("flag.day") }}</span></span>
      <Spinner v-if="flagOpening" class="over" :size="12" />
    </button>
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
  container-type: inline-size;
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
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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

.flag {
  position: relative;
  flex-shrink: 0;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.flag:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.flag-label {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

/* Without room for the title, the days and the button's words (the longest of the seven
   languages needs 547 px), the button is its glyph alone. */
@container (max-width: 550px) {
  .flag {
    width: 30px;
    padding: 0;
  }

  .flag-text {
    display: none;
  }
}

/* Narrower still, where the days leave the title no room for a word: its sliver goes, and its
   gap is the days' to use. */
@container (max-width: 320px) {
  .heading {
    visibility: hidden;
    margin-right: -12px;
  }
}

/* The label keeps its room under the spinner: the button never changes width. */
.flag-label.hidden {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}
</style>
