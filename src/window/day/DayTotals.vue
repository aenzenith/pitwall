<script setup lang="ts">
import { computed, ref } from "vue";

import { dayTotals, projectColors, timeSplit, type Lane, type SplitPart } from "../../lib/day";
import { language, t } from "../../lib/i18n";
import { useTooltip } from "../../lib/tooltip";
import type { DayProject } from "../../lib/types";

/** `selected`: the path of the project the side panel shows. */
const props = defineProps<{ projects: DayProject[]; lanes: Lane[]; selected: string | null }>();
const emit = defineEmits<{ select: [path: string] }>();

const tooltip = useTooltip();
/** The bar of where the time went; its parts anchor the legend's tooltips. */
const splitEl = ref<HTMLElement | null>(null);

const totals = computed(() => dayTotals(props.projects));
const colors = computed(() => projectColors(props.projects.map((p) => p.path)));
const percent = computed(() => new Intl.NumberFormat(language.value, { style: "percent", maximumFractionDigits: 0 }));
const split = computed(() => timeSplit(props.lanes, colors.value, percent.value));

/** A legend entry focused from the keyboard: the tooltip of its part of the bar. */
function focusPart(event: FocusEvent, part: SplitPart): void {
  if (!(event.currentTarget as HTMLElement).matches(":focus-visible")) return;
  const element = Array.from(splitEl.value?.children ?? []).find((child) => (child as HTMLElement).dataset.part === part.path);
  if (element) tooltip.showAt(element, part.tip);
}
</script>

<template>
  <div class="summary">
    <div class="totals">
      <div v-for="total in totals" :key="total.label" class="total">
        <span class="section-label">{{ total.label }}</span>
        <span class="total-value">{{ total.value }}</span>
      </div>
    </div>

    <!-- Where the time went: each project's share, in its colour. -->
    <template v-if="split.length">
      <!-- The bar is the pointer's; the keyboard and VoiceOver use the legend below, which
           picks the same projects and shows the same tooltips. -->
      <div ref="splitEl" class="split" aria-hidden="true">
        <span
          v-for="part in split"
          :key="part.path"
          class="part"
          :data-part="part.path"
          :style="{ flexGrow: part.grow, background: part.color }"
          @pointerenter="tooltip.show($event, part.tip)"
          @pointerleave="tooltip.hide()"
          @click="emit('select', part.path)"
        ></span>
      </div>
      <div class="split-legend" role="group" :aria-label="t('day.focus')">
        <button
          v-for="part in split"
          :key="part.path"
          type="button"
          :class="{ on: part.path === selected }"
          :aria-label="`${part.tip.title} ${part.tip.detail}`"
          :aria-pressed="part.path === selected"
          @click="emit('select', part.path)"
          @focus="focusPart($event, part)"
          @blur="tooltip.hide()"
        >
          <i :style="{ background: part.color }"></i>{{ part.name }}<span class="share">{{ part.share }}</span>
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.summary {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.totals {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 26px;
}

/* Square like the timeline's blocks; slivers stay visible. */
.split {
  display: flex;
  gap: 2px;
  height: 8px;
  margin-top: 4px;
  background: #15171c;
}

.part {
  flex: 0 1 0;
  min-width: 3px;
  cursor: pointer;
}

.part:hover {
  filter: brightness(1.2);
}

.split-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 16px;
}

.split-legend button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.split-legend button:hover,
.split-legend button.on {
  color: var(--text-strong);
}

.split-legend i {
  width: 8px;
  height: 8px;
  flex-shrink: 0;
}

.share {
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}

.total {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.total-value {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-strong);
  font-variant-numeric: tabular-nums;
}
</style>
