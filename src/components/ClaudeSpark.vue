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
// tail to 1 at the head. A negative delay puts each one's head at the same point of the lap.
const TRACE_STEPS = 12;
const TRACE_LENGTH = 30;
const TRACE_LAP = 101.55; // perimeter of the 28.5 × 28.5 rect with rx 7.25
const TRACE_SECONDS = 1.4;
const traceLines = Array.from({ length: TRACE_STEPS }, (_, i) => {
  const k = i + 1;
  const length = (TRACE_LENGTH * k) / TRACE_STEPS;
  return {
    strokeDasharray: `${length} ${TRACE_LAP - length}`,
    strokeOpacity: 1 / k,
    animationDelay: `${(length / TRACE_LAP - 1) * TRACE_SECONDS}s`,
  };
});
</script>

<template>
  <span :class="['spark', state]" aria-hidden="true">
    <Icon v-if="lock && state === 'waiting'" name="lock" :size="16" class="lock" />
    <ClaudeLogo v-else :size="18" :color="state === 'waiting' ? '#fff7f0' : undefined" />
    <svg v-if="state === 'working'" class="trace" viewBox="0 0 30 30">
      <rect v-for="(line, i) in traceLines" :key="i" x="0.75" y="0.75" width="28.5" height="28.5" rx="7.25" :style="line" />
    </svg>
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
  background: rgba(217, 119, 87, 0.12);
}

/* Nothing going on: dimmed. */
.spark.quiet {
  opacity: 0.55;
  filter: saturate(0.6);
}

/* Working: a line runs round the square's edge, its tail fading out (see traceLines). The rect's
   perimeter is 4 × (28.5 − 2 × 7.25) + 2π × 7.25 ≈ 101.55. */
.trace {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
}

.trace rect {
  fill: none;
  stroke: var(--claude);
  stroke-width: 1.5;
  stroke-linecap: round;
  animation: trace 1.4s linear infinite;
}

@keyframes trace {
  to {
    stroke-dashoffset: -101.55;
  }
}

/* Waiting: a solid square that pings. */
.spark.waiting {
  background: var(--claude);
  animation: ping 2.4s ease-out infinite;
}

.lock {
  color: #fff7f0;
}

@keyframes ping {
  0% {
    box-shadow: 0 0 0 0 rgba(240, 136, 62, 0.45);
  }
  70%,
  100% {
    box-shadow: 0 0 0 8px rgba(240, 136, 62, 0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .spark.waiting {
    animation: none;
  }

  /* Paused rather than removed, so the strokes keep their shared head and the fade. */
  .trace rect {
    animation-play-state: paused;
  }
}
</style>
