<script setup lang="ts">
import { computed, useId } from "vue";

import { LOW_LEFT, percentText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";

/**
 * One dial of the cluster in its round pod, everything on the pod's centre: a 240° arc from E at
 * the lower left to F at the lower right, the needle turning on the centre to the share left, the
 * figure and the limit's name between the arc's ends; `caption` under the pod, `until` after it
 * past a middle dot (on a line of its own when the pod is narrow). `big` is the session's in the middle;
 * `side` the two beside it, the same drawing smaller. The pod is marked `data-ring` for the
 * backdrop to light around.
 */
const props = defineProps<{ size: "big" | "side"; left: number; name: string; caption: string; until: string | null; stale: boolean }>();

/* The drawing, in a 200 × 200 box round its centre: the arc's radius and stroke, the needle, the hub, E/F type. */
const C = 100;
const R = 70;
const STROKE = 11;
const NEEDLE = 58;
const NEEDLE_WIDTH = 2.6;
const HUB = 6;
const TYPE = 9;
/** The arc runs clockwise from E at 210° to F at -30°, the way angles count from three o'clock. */
const START = 210;
const SWEEP = 240;

/** A point `radius` from the centre at `degrees`, in the box's coordinates (y down). */
function point(radius: number, degrees: number): [number, number] {
  const angle = (degrees * Math.PI) / 180;
  return [C + radius * Math.cos(angle), C - radius * Math.sin(angle)];
}

const at = (radius: number, degrees: number): string => point(radius, degrees).map((value) => value.toFixed(2)).join(" ");
const ARC = `M ${at(R, START)} A ${R} ${R} 0 1 1 ${at(R, START - SWEEP)}`;
const LENGTH = (SWEEP / 180) * Math.PI * R;
/** E and F under the arc's ends, out of the needle's way. */
const [E_X, END_Y] = point(R, START);
const [F_X] = point(R, START - SWEEP);
const TEXT_Y = END_Y + STROKE / 2 + TYPE / 2 + 4;
/** The unturned needle points at E. */
const [TIP_X, TIP_Y] = point(NEEDLE, START);

/** Quarter marks inside the arc. */
const QUARTERS = [0.25, 0.5, 0.75].map((share) => `M ${at(R - 10, START - share * SWEEP)} L ${at(R - 17, START - share * SWEEP)}`);

/** The bezel's sixty marks round the pod's centre, every fifth one longer. */
const MINUTES = Array.from({ length: 60 }, (_, index) => {
  const angle = (index / 60) * 2 * Math.PI;
  const at = (radius: number): string => `${(100 + radius * Math.sin(angle)).toFixed(2)} ${(100 - radius * Math.cos(angle)).toFixed(2)}`;
  const major = index % 5 === 0;
  return { d: `M ${at(major ? 86 : 89)} L ${at(93)}`, major };
});

/** The arc's glow filter; one per dial, so its id is too. */
const glowId = `fuel-glow-${useId()}`;

const low = computed(() => props.left <= LOW_LEFT);
const value = computed(() => percentText(props.left, language.value));
const dash = computed(() => `${(props.left / 100) * LENGTH} ${LENGTH + 40}`);
/** Turned clockwise from E by the share left, up to F. */
const turn = computed(() => `rotate(${(props.left / 100) * SWEEP}deg)`);
</script>

<template>
  <div :class="['gauge', size, { low, stale }]">
    <div class="pod" data-ring>
      <svg class="dial" viewBox="0 0 200 200" fill="none" role="img" :aria-label="t('fuel.gauge.label', { name, value })">
        <defs>
          <filter :id="glowId" x="-20%" y="-30%" width="140%" height="160%">
            <feGaussianBlur stdDeviation="4" />
          </filter>
        </defs>
        <path v-for="mark in MINUTES" :key="mark.d" :class="['minute', { major: mark.major }]" :d="mark.d" />
        <path class="track" :d="ARC" :stroke-width="STROKE" />
        <path v-for="tick in QUARTERS" :key="tick" class="tick" :d="tick" />
        <template v-if="left > 0">
          <path class="glow" :d="ARC" :stroke-width="STROKE" :stroke-dasharray="dash" :filter="`url(#${glowId})`" />
          <path class="fill" :d="ARC" :stroke-width="STROKE" :stroke-dasharray="dash" />
        </template>
        <g class="needle" :style="{ transform: turn, transformOrigin: `${C}px ${C}px` }">
          <line :x1="C" :y1="C" :x2="TIP_X" :y2="TIP_Y" :stroke-width="NEEDLE_WIDTH" />
        </g>
        <circle class="hub" :cx="C" :cy="C" :r="HUB" />
        <text class="end" :x="E_X" :y="TEXT_Y" :font-size="TYPE" text-anchor="middle" dominant-baseline="middle">{{ t("fuel.gauge.empty") }}</text>
        <text class="end" :x="F_X" :y="TEXT_Y" :font-size="TYPE" text-anchor="middle" dominant-baseline="middle">{{ t("fuel.gauge.full") }}</text>
      </svg>
      <span class="value" aria-hidden="true">{{ value }}</span>
      <span class="name" aria-hidden="true" :title="name">{{ name }}</span>
    </div>
    <span class="caption" :title="until ? `${caption} · ${until}` : caption">
      <span class="when">{{ caption }}</span>
      <span v-if="until" class="until">{{ until }}</span>
    </span>
  </div>
</template>

<style scoped>
.gauge {
  --tint: var(--claude);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  min-width: 0;
  container-type: inline-size;
}

.gauge.low {
  --tint: var(--crash);
}

.gauge.stale {
  --tint: var(--idle-ring);
}

/* `--dial`, set by the cluster, follows the window; the side ones are two thirds of it. */
.gauge.big {
  flex: 0 1 var(--dial, 300px);
}

.gauge.side {
  flex: 0 1 calc(var(--dial, 300px) * 2 / 3);
}

/* Smoked glass over the backdrop, lit faintly from below, in a shadow of its own. */
.pod {
  position: relative;
  width: 100%;
  aspect-ratio: 1;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 35%, rgba(30, 32, 38, 0.72), rgba(8, 9, 11, 0.9) 72%);
  -webkit-backdrop-filter: blur(10px);
  backdrop-filter: blur(10px);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    inset 0 -6cqi 12cqi -3cqi color-mix(in srgb, var(--tint) 18%, transparent),
    0 0 0 1px rgba(255, 255, 255, 0.05),
    0 6cqi 16cqi rgba(0, 0, 0, 0.55);
}

/* The bezel: a thin ring catching the light at two corners, like brushed metal. */
.pod::before {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: conic-gradient(
    from 200deg,
    rgba(255, 255, 255, 0.3),
    rgba(255, 255, 255, 0.03) 22%,
    rgba(255, 255, 255, 0.16) 50%,
    rgba(255, 255, 255, 0.03) 78%,
    rgba(255, 255, 255, 0.3)
  );
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px));
  mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px));
  pointer-events: none;
}

.dial {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

/* The marks in a see-through warm white: they take the glass's colour, never a cold grey. */
.gauge {
  --mark: 255, 238, 222;
}

.minute {
  stroke: rgba(var(--mark), 0.08);
  stroke-width: 0.8;
}

.minute.major {
  stroke: rgba(var(--mark), 0.22);
  stroke-width: 1.4;
}

/* The arc's unlit part: the dial's own colour, faint, as a lamp's dark side. */
.track {
  stroke: color-mix(in srgb, var(--tint) 14%, transparent);
  stroke-linecap: round;
}

.tick {
  stroke: rgba(var(--mark), 0.18);
  stroke-width: 1.5;
}

.fill,
.glow {
  stroke: var(--tint);
  stroke-linecap: round;
  transition: stroke-dasharray 0.6s ease;
}

/* A blurred copy under the arc: it reads as lit. */
.glow {
  opacity: 0.7;
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

/* The figure in the gap between the arc's ends, below the hub. */
.value {
  position: absolute;
  left: 0;
  right: 0;
  top: 64%;
  font-size: max(18px, 13cqi);
  font-weight: 600;
  line-height: 1;
  text-align: center;
  letter-spacing: -0.02em;
  color: var(--claude-text);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

/* Which limit this is, under its figure, as a car's dial says FUEL. */
.name {
  position: absolute;
  left: 22%;
  right: 22%;
  top: 79%;
  /* Measured from the big dial, so a side dial's name stays readable beside it. */
  font-size: max(10px, calc(var(--dial, 300px) * 0.036));
  font-weight: 600;
  line-height: 1.2;
  text-align: center;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

:lang(zh) .name,
:lang(ja) .name {
  letter-spacing: 0;
}

/* A step below the big one's name, not shrunk with the smaller pod. */
.side .name {
  font-size: max(10px, calc(var(--dial, 300px) * 0.031));
}

.low .value {
  color: var(--crash-text);
}

.stale .value {
  color: var(--text-muted);
}

/* The refill and the time to it on one line, a middle dot between. */
.caption {
  max-width: 100%;
  display: flex;
  justify-content: center;
  color: var(--text-strong);
  white-space: nowrap;
}

.when,
.until {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.until::before {
  content: "·";
  margin: 0 6px;
}

/* A narrow pod: the time to it on a line of its own, no dot. */
@container (max-width: 165px) {
  .caption {
    flex-direction: column;
    align-items: center;
  }

  .until::before {
    content: none;
  }
}

.stale .caption,
.stale .name {
  color: var(--text-muted);
}

/* The caption is a clause ("refills Tue 21:00"): it starts a line here. */
.when::first-letter {
  text-transform: uppercase;
}

.caption {
  font-size: clamp(13px, calc(var(--dial, 300px) * 0.03), 16px);
}

@media (prefers-reduced-motion: reduce) {
  .fill,
  .glow,
  .needle {
    transition: none;
  }
}
</style>
