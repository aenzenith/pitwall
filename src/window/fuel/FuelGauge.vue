<script setup lang="ts">
import { computed } from "vue";

import { LOW_LEFT, percentText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";

/**
 * One dial of the cluster: E on the left, F on the right, the needle at the share left. `big` is
 * the session's in the middle; `side` the two beside it.
 */
const props = defineProps<{ size: "big" | "side"; left: number; name: string; caption: string; stale: boolean }>();

/** Each size's drawing: width, centre height, radius, stroke, needle, hub, tick span (in from the arc), E/F type. */
const SIZES = {
  big: { w: 300, cy: 152, r: 125, stroke: 18, needle: 100, needleWidth: 3.6, hub: 7.5, tick: [16, 27], type: 12 },
  side: { w: 200, cy: 102, r: 80, stroke: 11, needle: 64, needleWidth: 2.2, hub: 4.6, tick: [10, 16.5], type: 10 },
} as const;

const shape = computed(() => {
  const s = SIZES[props.size];
  const cx = s.w / 2;
  const arc = `M ${cx - s.r} ${s.cy} A ${s.r} ${s.r} 0 0 1 ${cx + s.r} ${s.cy}`;
  // Quarter marks inside the arc.
  const ticks = [0.25, 0.5, 0.75].map((share) => {
    const angle = Math.PI * (1 - share);
    const at = (radius: number): string => `${cx + radius * Math.cos(angle)} ${s.cy - radius * Math.sin(angle)}`;
    return `M ${at(s.r - s.tick[0])} L ${at(s.r - s.tick[1])}`;
  });
  // E and F under the arc's ends, the box just tall enough for them.
  const textY = s.cy + s.stroke / 2 + s.type + 2;
  return { ...s, cx, arc, ticks, length: Math.PI * s.r, textY, h: textY + 4 };
});

const low = computed(() => props.left <= LOW_LEFT);
const value = computed(() => percentText(props.left, language.value));
/** The needle points at E unturned; turned clockwise by the share left, up to F. */
const turn = computed(() => `rotate(${(props.left / 100) * 180}deg)`);
</script>

<template>
  <div :class="['gauge', size, { low, stale }]">
    <svg class="dial" :viewBox="`0 0 ${shape.w} ${shape.h}`" fill="none" role="img" :aria-label="t('fuel.gauge.label', { name, value })">
      <path class="track" :d="shape.arc" :stroke-width="shape.stroke" />
      <path v-for="tick in shape.ticks" :key="tick" class="tick" :d="tick" />
      <path v-if="left > 0" class="fill" :d="shape.arc" :stroke-width="shape.stroke" :stroke-dasharray="`${(left / 100) * shape.length} ${shape.length + 40}`" />
      <g class="needle" :style="{ transform: turn, transformOrigin: `${shape.cx}px ${shape.cy}px` }">
        <line :x1="shape.cx" :y1="shape.cy" :x2="shape.cx - shape.needle" :y2="shape.cy" :stroke-width="shape.needleWidth" />
      </g>
      <circle class="hub" :cx="shape.cx" :cy="shape.cy" :r="shape.hub" />
      <text class="end" :x="shape.cx - shape.r" :y="shape.textY" :font-size="shape.type" text-anchor="middle">{{ t("fuel.gauge.empty") }}</text>
      <text class="end" :x="shape.cx + shape.r" :y="shape.textY" :font-size="shape.type" text-anchor="middle">{{ t("fuel.gauge.full") }}</text>
    </svg>
    <span class="value" aria-hidden="true">{{ value }}</span>
    <span class="caption" :title="caption">{{ caption }}</span>
  </div>
</template>

<style scoped>
.gauge {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.gauge.big {
  flex: 0 1 300px;
}

/* On the same baseline as the big one, a little up from the bottom. */
.gauge.side {
  flex: 0 1 200px;
  padding-bottom: 6px;
}

.dial {
  width: 100%;
  height: auto;
}

.track {
  stroke: var(--line);
  stroke-linecap: round;
}

.tick {
  stroke: var(--line-strong);
  stroke-width: 1.5;
}

.fill {
  stroke: var(--claude);
  stroke-linecap: round;
  transition: stroke-dasharray 0.6s ease;
}

.low .fill {
  stroke: var(--crash);
}

.stale .fill {
  stroke: var(--idle-ring);
}

.needle {
  transform-box: view-box;
  transition: transform 0.6s ease;
}

.needle line {
  stroke: var(--text);
  stroke-linecap: round;
}

.hub {
  fill: var(--text);
}

.end {
  fill: var(--text-faint);
  font-family: var(--font-mono);
}

.value {
  font-weight: 600;
  line-height: 1;
  color: var(--claude-text);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.big .value {
  font-size: 44px;
  letter-spacing: -0.02em;
}

.side .value {
  font-size: 24px;
}

.low .value {
  color: var(--crash-text);
}

.stale .value {
  color: var(--text-muted);
}

.caption {
  max-width: 100%;
  margin-top: 2px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.big .caption {
  font-size: 13px;
}

.side .caption {
  font-size: 12px;
}

@media (prefers-reduced-motion: reduce) {
  .fill,
  .needle {
    transition: none;
  }
}
</style>
