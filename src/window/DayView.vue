<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import { lane, timeRange, type Live } from "../lib/day";
import { claudeLine } from "../lib/format";
import { t } from "../lib/i18n";
import { api, now, snapshot, visible } from "../lib/store";
import { provideTooltip } from "../lib/tooltip";
import type { DaySummary } from "../lib/types";
import DayHeader from "./day/DayHeader.vue";
import DayLegend from "./day/DayLegend.vue";
import DaySide from "./day/DaySide.vue";
import DayTimeline from "./day/DayTimeline.vue";
import DayTotals from "./day/DayTotals.vue";

/** Today is fetched again this often while it is shown. */
const REFRESH_MS = 30_000;

const day = ref<DaySummary | null>(null);
/** The day shown, `YYYY-MM-DD`; null is today. */
const date = ref<string | null>(null);
const selectedPath = ref<string | null>(null);
/** The tooltip as drawn, kept inside the window (lib/tooltip). */
const tipEl = ref<HTMLElement | null>(null);
const tooltip = provideTooltip(tipEl);
const tip = tooltip.tip;
const announced = tooltip.announced;

let asked = 0;

async function load(): Promise<void> {
  const ask = ++asked;
  try {
    const summary = await api.daySummary(date.value);
    if (ask !== asked) return;
    day.value = summary;
    // The selection stays put while the lanes reorder.
    if (!summary.projects.some((p) => p.path === selectedPath.value)) selectedPath.value = summary.projects[0]?.path ?? null;
  } catch {
    // Kept as it was; the next refresh tries again.
  }
}

/* ---------- the day ---------- */

const projects = computed(() => day.value?.projects ?? []);

/** Whole hours around what happened (and now, today), at least eight of them. */
const range = computed(() => timeRange(day.value));

const lanes = computed(() => projects.value.map((project) => lane(project, range.value)));

/** Live, whatever day is shown: where Claude waits on you (a notification not read yet) or
    works, by project; waiting wins, as in the project list. */
const liveProjects = computed(() => {
  const found = new Map<string, Live>();
  for (const p of snapshot.value?.projects ?? []) {
    if (p.claude) found.set(p.path, { phase: "waiting", text: claudeLine(p, now.value) });
    else if (p.claudeWorking) found.set(p.path, { phase: "working", text: t("claude.working") });
  }
  return found;
});

const selected = computed(() => projects.value.find((p) => p.path === selectedPath.value) ?? projects.value[0] ?? null);

/* ---------- life ---------- */

let timer: number | undefined;

watch(date, () => {
  tooltip.hide();
  void load();
});

onMounted(() => {
  void load();
  // Today keeps up while the window is on screen; off screen it waits, and catches up on return.
  timer = window.setInterval(() => {
    if (visible.value && (!day.value || day.value.today)) void load();
  }, REFRESH_MS);
});

watch(visible, (on) => {
  if (on && (!day.value || day.value.today)) void load();
});

onBeforeUnmount(() => window.clearInterval(timer));
</script>

<template>
  <div class="day">
    <div class="column">
      <DayHeader v-model:date="date" :day="day" />

      <div class="board">
        <template v-if="day && projects.length">
          <DayTotals :projects="projects" :lanes="lanes" :selected="selected?.path ?? null" @select="selectedPath = $event" />
          <DayTimeline :day="day" :lanes="lanes" :range="range" :selected="selected?.path ?? null" :live="liveProjects" @select="selectedPath = $event" />
          <DayLegend />
        </template>

        <section v-else-if="day" class="empty">
          <Icon name="calendar" :size="28" />
          <p>{{ t("day.empty") }}<br />{{ t("day.emptyHint") }}</p>
        </section>
      </div>
    </div>

    <DaySide :selected="selected" :live="selected ? liveProjects.get(selected.path) : undefined" />

    <span id="lane-keys" class="sr-only">{{ t("day.laneKeys") }}</span>
    <div class="sr-only" aria-live="polite">{{ announced }}</div>

    <div v-if="tip" ref="tipEl" class="tip" role="tooltip" :style="{ left: `${tip.x}px`, top: `${tip.y}px` }">
      <span :class="['tip-title', tip.tone]">{{ tip.title }}</span>
      <span v-if="tip.detail" class="tip-detail">{{ tip.detail }}</span>
    </div>
  </div>
</template>

<style scoped>
.day {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  overflow: clip;
}

.column {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.board {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 18px 22px 16px;
}

.empty {
  flex-grow: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  text-align: center;
  color: var(--text-muted);
}

.empty p {
  margin: 0;
  line-height: 1.6;
}

/* ---------- tooltip ---------- */

.tip {
  position: fixed;
  z-index: 20;
  transform: translate(-50%, calc(-100% - 8px));
  display: flex;
  flex-direction: column;
  gap: 3px;
  max-width: 340px;
  padding: 9px 11px;
  border-radius: 9px;
  background: #23272e;
  border: 1px solid #3a3f48;
  box-shadow: 0 10px 24px rgba(0, 0, 0, 0.45);
  font-size: 12px;
  pointer-events: none;
}

.tip-title {
  font-weight: 600;
  color: var(--text-strong);
}

.tip-title.claude {
  color: #f3c29b;
}

.tip-title.server {
  color: #9ad7b0;
}

.tip-title.crash {
  color: var(--crash-text);
}

.tip-detail {
  font-family: var(--font-mono);
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
