<script setup lang="ts">
import { computed } from "vue";

import { LOW_LEFT, percentText } from "../lib/fuel";
import { language, t } from "../lib/i18n";

/**
 * One dial of the popover's strip: the Fuel page's dial (window/fuel/FuelGauge) drawn to be read
 * small. The same 240° arc from E at the lower left to F at the lower right and the needle turning
 * on the centre to the share left, with a thick stroke and no pod, marks, glow or figure: the
 * strip writes the share beside it. `big` is the session's; `mini` starts a line, as the menu
 * bar's does its share.
 */
const props = defineProps<{ variant: "big" | "mini"; left: number; name: string; stale: boolean }>();

/** The drawing in a 200 × 200 box: its radius and stroke, the needle and the hub; the smaller it
 * is drawn, the thicker. */
const SHAPES = {
  big: { radius: 74, stroke: 17, needle: 52, needleWidth: 8, hub: 12 },
  mini: { radius: 70, stroke: 25, needle: 45, needleWidth: 15, hub: 13 },
};
/** The arc's centre: under the box's own, so the arc and the gap between its ends stand in the middle. */
const CY = 113;
const CX = 100;
/** The arc runs clockwise from E at 210° to F at -30°, the way angles count from three o'clock. */
const START = 210;
const SWEEP = 240;

const shape = computed(() => SHAPES[props.variant]);

/** A point `radius` from the centre at `degrees`, in the box's coordinates (y down). */
function at(radius: number, degrees: number): string {
  const angle = (degrees * Math.PI) / 180;
  return `${(CX + radius * Math.cos(angle)).toFixed(2)} ${(CY - radius * Math.sin(angle)).toFixed(2)}`;
}

const arc = computed(() => `M ${at(shape.value.radius, START)} A ${shape.value.radius} ${shape.value.radius} 0 1 1 ${at(shape.value.radius, START - SWEEP)}`);
const length = computed(() => (SWEEP / 180) * Math.PI * shape.value.radius);
/** The unturned needle points at E. */
const tip = computed(() => at(shape.value.needle, START).split(" "));

const low = computed(() => props.left <= LOW_LEFT);
const value = computed(() => percentText(props.left, language.value));
const dash = computed(() => `${(props.left / 100) * length.value} ${length.value + 40}`);
/** Turned clockwise from E by the share left, up to F. */
const turn = computed(() => `rotate(${(props.left / 100) * SWEEP}deg)`);
</script>

<template>
  <span :class="['fueldial', variant, { low, stale }]">
    <svg class="fueldial-drawing" viewBox="0 0 200 200" fill="none" role="img" :aria-label="t('fuel.gauge.label', { name, value })">
      <path class="fueldial-track" :d="arc" :stroke-width="shape.stroke" />
      <path v-if="left > 0" class="fueldial-fill" :d="arc" :stroke-width="shape.stroke" :stroke-dasharray="dash" />
      <g class="fueldial-needle" :style="{ transform: turn, transformOrigin: `${CX}px ${CY}px` }">
        <line :x1="CX" :y1="CY" :x2="tip[0]" :y2="tip[1]" :stroke-width="shape.needleWidth" />
      </g>
      <circle class="fueldial-hub" :cx="CX" :cy="CY" :r="shape.hub" />
    </svg>
  </span>
</template>

<style scoped>
.fueldial {
  --tint: var(--claude);
  position: relative;
  flex-shrink: 0;
  display: block;
}

.fueldial.low {
  --tint: var(--crash);
}

.fueldial.stale {
  --tint: var(--idle-ring);
}

.fueldial.big {
  width: 38px;
  height: 38px;
}

.fueldial.mini {
  width: 16px;
  height: 16px;
}

.fueldial-drawing {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

/* The arc's unlit part: the dial's own colour, faint. */
.fueldial-track {
  stroke: color-mix(in srgb, var(--tint) 16%, transparent);
  stroke-linecap: round;
}

.fueldial-fill {
  stroke: var(--tint);
  stroke-linecap: round;
  transition: stroke-dasharray 0.6s ease;
}

.fueldial-needle {
  transform-box: view-box;
  transition: transform 0.6s ease;
}

.fueldial-needle line {
  stroke: var(--text);
  stroke-linecap: round;
}

.fueldial-hub {
  fill: var(--text);
}

.stale .fueldial-needle line {
  stroke: var(--text-muted);
}

.stale .fueldial-hub {
  fill: var(--text-muted);
}

@media (prefers-reduced-motion: reduce) {
  .fueldial-fill,
  .fueldial-needle {
    transition: none;
  }
}
</style>
