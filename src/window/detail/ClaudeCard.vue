<script setup lang="ts">
import { computed } from "vue";

import ClaudeMark from "../../components/ClaudeMark.vue";
import { ago } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, now } from "../../lib/store";
import type { Project } from "../../lib/types";

const props = defineProps<{ project: Project }>();

const claudeTitle = computed(() => {
  const turn = props.project.claude;
  if (!turn) return t(props.project.claudeWorking ? "detail.claudeWorking" : "detail.noSession");
  return t(({ finished: "detail.claudeFinished", asking: "detail.claudeAsking", permission: "detail.claudePermission" } as const)[turn.kind]);
});

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
  <section class="block" :aria-label="t('window.col.claude')">
    <div class="section-label">Claude</div>
    <div :class="['card', { hot: project.claude }]">
      <span :class="['card-mark', { quiet: !project.claude && !project.claudeWorking }]">
        <ClaudeMark :size="18" :color="project.claude ? '#fff7f0' : undefined" />
        <svg v-if="!project.claude && project.claudeWorking" class="trace" viewBox="0 0 30 30" aria-hidden="true">
          <rect v-for="(line, i) in traceLines" :key="i" x="0.75" y="0.75" width="28.5" height="28.5" rx="7.25" :style="line" />
        </svg>
      </span>
      <div class="card-text">
        <span class="card-title">{{ claudeTitle }}</span>
        <span class="card-sub">
          {{
            project.claude
              ? t("detail.notLookedAt", { ago: ago(project.claude.at, now) })
              : t(project.claudeWorking ? "detail.workingHint" : "detail.sessionsHint")
          }}
        </span>
      </div>
      <button v-if="project.claude" type="button" class="seen" @click="api.markSeen(project.path)">{{ t("detail.markSeen") }}</button>
    </div>
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
/* Bare, with no box in any state. */
.card {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* Claude's spark at the card's left; dimmed while nothing is going on. */
.card-mark {
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

.card-mark.quiet {
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

/* Waiting: still no box; a solid spark that pings, the title in Claude's colour and a filled
   button carry it instead. */
.card.hot .card-mark {
  background: var(--claude);
  animation: ping 2.4s ease-out infinite;
}

.card.hot .card-title {
  font-weight: 600;
  color: var(--claude-text);
}

.card.hot .card-sub {
  color: #c9a88c;
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

.card-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.card-title {
  font-size: 13px;
  font-weight: 500;
}

.card-sub {
  font-size: 12px;
  color: var(--text-muted);
}

.seen {
  flex-shrink: 0;
  height: 28px;
  padding: 0 12px;
  border: 0;
  border-radius: var(--radius-control);
  background: var(--claude);
  color: #1a110a;
  font-size: 12px;
  font-weight: 600;
}

.seen:hover {
  background: var(--claude-text);
}

@media (prefers-reduced-motion: reduce) {
  .card.hot .card-mark {
    animation: none;
  }

  /* Paused rather than removed, so the strokes keep their shared head and the fade. */
  .trace rect {
    animation-play-state: paused;
  }
}
</style>
