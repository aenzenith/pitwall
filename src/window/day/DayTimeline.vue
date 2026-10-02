<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

import { clock, hourTicks, pct, type Lane, type Live, type TimeRange } from "../../lib/day";
import { t } from "../../lib/i18n";
import type { DaySummary } from "../../lib/types";
import DayLane from "./DayLane.vue";

/** `range`: the hours shown; `selected`: the selected project's path; `live`: Claude, right now, by project path. */
const props = defineProps<{ day: DaySummary; lanes: Lane[]; range: TimeRange | null; selected: string | null; live: Map<string, Live> }>();
const emit = defineEmits<{ select: [path: string] }>();

/** The hour row's width, so labels are spaced to fit and step aside for the now marker. */
const axis = ref<HTMLElement | null>(null);
const axisWidth = ref(0);
const axisObserver = new ResizeObserver(([entry]) => (axisWidth.value = entry.contentRect.width));

watch(axis, (element, old) => {
  if (old) axisObserver.unobserve(old);
  if (element) axisObserver.observe(element);
});

onBeforeUnmount(() => axisObserver.disconnect());

const ticks = computed(() => hourTicks(props.range, axisWidth.value, props.day.today ? props.day.now : null));
const nowLeft = computed(() => (props.day.today ? `${pct(props.day.now, props.range)}%` : null));
</script>

<template>
  <div class="timeline" role="group" :aria-label="t('day.timeline')">
    <div ref="axis" class="axis">
      <span v-for="tick in ticks" :key="tick.at" class="tick" :style="{ left: tick.left }">{{ tick.label }}</span>
      <span v-if="nowLeft" class="now-pill" :style="{ left: nowLeft }"><i></i>{{ clock(day.now) }}</span>
    </div>

    <div class="lanes">
      <DayLane
        v-for="lane in lanes"
        :key="lane.project.path"
        :lane="lane"
        :on="lane.project.path === selected"
        :ticks="ticks"
        :now-left="nowLeft"
        :live="live.get(lane.project.path)"
        @select="emit('select', lane.project.path)"
      />
    </div>
  </div>
</template>

<style scoped>
.timeline {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

/* Lines up with the tracks: lane padding 10 + label 160 + gap 18, and the lane's 10 on the right. */
.axis {
  position: relative;
  flex-shrink: 0;
  height: 20px;
  margin: 0 10px 0 188px;
}

.tick {
  position: absolute;
  top: 0;
  transform: translateX(-50%);
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
}

.now-pill {
  position: absolute;
  top: -1px;
  z-index: 1;
  transform: translateX(-50%);
  height: 18px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 7px;
  border-radius: 9px;
  background: #1d2a22;
  border: 1px solid #2d4537;
  font-family: var(--font-mono);
  font-size: 11px;
  color: #9ad7b0;
  white-space: nowrap;
}

.now-pill i {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--run);
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%,
  100% {
    box-shadow: 0 0 0 0 rgba(115, 201, 145, 0.55);
  }
  50% {
    box-shadow: 0 0 0 5px rgba(115, 201, 145, 0);
  }
}

.lanes {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
</style>
