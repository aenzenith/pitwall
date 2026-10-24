<script setup lang="ts">
// Claude's spark in its square, showing Claude's state: waiting on you, a solid square that pings
// (with `lock`, a permission prompt: a lock in place of the spark); working, a line running round
// the square's edge; quiet, dimmed. The project details' Claude card and the session details
// show it.
import ClaudeLogo from "./ClaudeLogo.vue";
import Icon from "./Icon.vue";

withDefaults(defineProps<{ state: "waiting" | "working" | "quiet"; lock?: boolean }>(), { lock: false });

// The working line round the spark's square, faded from head to tail: TRACE_STEPS strokes share
// one head, the k-th k steps long at opacity 1/k, so their overlap rises evenly from 1/n at the
// tail to 1 at the head.
const TRACE_STEPS = 12;
const TRACE_LENGTH = 30;
const TRACE_LAP = 101.55; // perimeter of the 28.5 × 28.5 rect with rx 7.25
const TRACE_SECONDS = 1.4;
const TRACE_RADIUS = 7.25;
const TRACE_SIDE = 28.5 - 2 * TRACE_RADIUS; // a side's straight part, 14
const TRACE_CAP = 0.75; // how far a round cap reaches past a stroke's end
const CIRCLE = 2 * Math.PI * TRACE_RADIUS;

// The line is drawn once and only ever moved, so the compositor runs it: animating the strokes'
// dash offset had the spark redrawn every frame. The square's edge is four sides and four quarter
// circles, and each quarter of the lap (a side and the corner after it) shows the line through a
// clip of its own: on the side a straight copy slides along, at the corner a copy laid on the
// corner's whole circle turns about its centre. Each copy keeps its strokes' round caps and
// overlap, so the line looks as it did. The other three quarters are the first one turned by 90°,
// a quarter lap later.
const traceLines = Array.from({ length: TRACE_STEPS }, (_, i) => {
  const k = i + 1;
  const length = (TRACE_LENGTH * k) / TRACE_STEPS;
  return {
    opacity: 1 / k,
    // Twice, a lap apart, the heads where the side begins and a lap behind it.
    side: `M${-length} 0.75H0M${-TRACE_LAP - length} 0.75H${-TRACE_LAP}`,
    // The head at the circle's top, where the corner begins.
    corner: { strokeDasharray: `${length} ${CIRCLE - length}`, strokeDashoffset: length + CIRCLE / 4 },
  };
});
// Each quarter a quarter lap behind the one before; the delays are negative, so every copy is in
// its place from the first frame.
const traceQuarters = Array.from({ length: 4 }, (_, q) => ({
  turn: `rotate(${q * 90}deg)`,
  side: `${q * (TRACE_SECONDS / 4) - TRACE_SECONDS}s`,
  // The corner's turn starts as the head's cap reaches the corner (see `trace-corner`).
  corner: `${((TRACE_SIDE - TRACE_CAP) / TRACE_LAP + q / 4 - 1) * TRACE_SECONDS}s`,
}));
</script>

<template>
  <span :class="['spark', state]" aria-hidden="true">
    <Icon v-if="lock && state === 'waiting'" name="lock" :size="16" class="lock" />
    <ClaudeLogo v-else :size="18" :color="state === 'waiting' ? 'var(--on-claude-mark)' : undefined" />
    <span v-if="state === 'working'" class="trace">
      <span v-for="(quarter, q) in traceQuarters" :key="q" class="quarter" :style="{ transform: quarter.turn }">
        <span class="side">
          <svg viewBox="-132.55 0 133.55 2" :style="{ animationDelay: quarter.side }">
            <path v-for="(line, i) in traceLines" :key="i" :d="line.side" :stroke-opacity="line.opacity" />
          </svg>
        </span>
        <span class="corner">
          <svg viewBox="-8 -8 16 16" :style="{ animationDelay: quarter.corner }">
            <circle v-for="(line, i) in traceLines" :key="i" r="7.25" :stroke-opacity="line.opacity" :style="line.corner" />
          </svg>
        </span>
      </span>
    </span>
  </span>
</template>

<style scoped>
.spark {
  position: relative;
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  background: color-mix(in srgb, var(--claude-logo) 12%, transparent);
}

/* Nothing going on: dimmed. */
.spark.quiet {
  opacity: 0.55;
  filter: saturate(0.6);
}

/* Working: a line runs round the square's edge, its tail fading out (see traceLines). The edge is
   the rect at 0.75 with rx 7.25: the top side's straight part runs from x 8 to 22 at y 0.75, the
   top-right corner is centred at (22, 8). Each clip reaches a pixel past the stroke outwards. */
.trace,
.quarter {
  position: absolute;
  inset: 0;
}

.side,
.corner {
  position: absolute;
  top: -1px;
  overflow: hidden;
}

.side {
  left: 8px;
  width: 14px;
  height: 4px;
}

.corner {
  left: 22px;
  width: 9px;
  height: 9px;
}

.trace svg {
  position: absolute;
  top: 1px;
  overflow: visible;
}

.trace path,
.trace circle {
  fill: none;
  stroke: var(--claude);
  stroke-width: 1.5;
  stroke-linecap: round;
}

/* The side's strip: x 0 is where the side begins, and it reaches back a lap and the line's length
   and cap (101.55 + 30 + 1) for the copy behind. It slides a lap in a lap's time, so one copy's
   head runs on from where the other's began. */
.side svg {
  left: -132.55px;
  width: 133.55px;
  height: 2px;
  animation: trace-side 1.4s linear infinite;
}

@keyframes trace-side {
  to {
    transform: translateX(101.55px);
  }
}

/* The corner's circle, centred on the corner's centre. It shows in the clip only from the moment
   the head's cap reaches the corner until the tail's has left it, 0.75 + 11.39 (the quarter
   circle) + 30 + 0.75 = 42.89 of the lap's 101.55, 42.234 %: it turns at the line's speed then
   (42.89 / 7.25 rad = 338.94°, starting 0.75 / 7.25 rad = 5.93° before the top). The rest of the
   turn, 21.06°, is made out of sight: half of it on at that speed, then a wait with the line well
   clear of the clip's edges (where the moved picture's softened edge would show), then the other
   half. */
.corner svg {
  left: -8px;
  width: 16px;
  height: 16px;
  transform-origin: 50% 50%;
  animation: trace-corner 1.4s linear infinite;
}

@keyframes trace-corner {
  from {
    transform: rotate(-5.927deg);
  }
  42.234% {
    transform: rotate(333.013deg);
  }
  43.546%,
  98.688% {
    transform: rotate(343.543deg);
  }
  to {
    transform: rotate(354.073deg);
  }
}

/* Waiting: a solid square that pings. The ping is a square of the same colour under the spark (the
   square isolates, so it stays over whatever lies behind the square), grown and faded: only scaled
   and faded, so the compositor runs it, where a spread shadow had the spark redrawn every frame.
   Drawn at its largest (8 px out) and scaled down, so it stays sharp; its corners are rounded so
   that, scaled, they keep close to the shadow's (8 px plus the spread). */
.spark.waiting {
  isolation: isolate;
  background: var(--claude);
}

.spark.waiting::before {
  content: "";
  position: absolute;
  inset: -8px;
  z-index: -1;
  border-radius: 13.2px;
  background: var(--claude);
  animation: ping 2.4s ease-out infinite;
}

.lock {
  color: var(--on-claude-mark);
}

@keyframes ping {
  0% {
    transform: scale(0.6522);
    opacity: 0.45;
  }
  70%,
  100% {
    transform: scale(1);
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .spark.waiting::before {
    display: none;
  }

  /* Paused rather than removed, so the line stays whole where a lap begins. */
  .trace svg {
    animation-play-state: paused;
  }
}
</style>
