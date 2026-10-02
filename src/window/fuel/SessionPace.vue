<script setup lang="ts">
import { computed } from "vue";

import Rich from "../../components/Rich.vue";
import { clockText, pace, position } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import type { UsageWindow } from "../../lib/types";

/**
 * The session window as a strip from its start to its reset: the time gone by, now, what is still
 * ahead and, at the pace so far, when the fuel runs out. `readAt`: when `window` was read, the
 * moment the pace is measured to.
 */
const props = defineProps<{ window: UsageWindow; readAt: number; now: number; stale: boolean }>();

const HOUR = 3_600_000;

const state = computed(() => pace(props.window, props.readAt));

function clock(at: number): string {
  return clockText(at, language.value);
}

function duration(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  const hours = Math.floor(minutes / 60);
  return hours ? t("time.duration", { hours, minutes: String(minutes % 60).padStart(2, "0") }) : t("time.durationMinutes", { minutes });
}

/** A label at `pct` of the strip, kept inside it at the ends. */
function label(pct: number): Record<string, string> {
  const shift = pct < 8 ? "0" : pct > 92 ? "-100%" : "-50%";
  return { left: `${pct}%`, transform: `translateX(${shift})` };
}

const strip = computed(() => {
  const s = state.value;
  if (s.kind === "idle") return null;
  const now = position(props.now, s.start, s.end);
  const hours: number[] = [];
  for (let at = s.start; at <= s.end; at += HOUR) hours.push(at);
  return {
    range: `${clock(s.start)} – ${clock(s.end)}`,
    now,
    hours,
    empty: s.kind === "out" ? position(s.emptyAt, s.start, s.end) : null,
  };
});

const SENTENCES = { idle: "fuel.pace.idle", empty: "fuel.pace.empty", early: "fuel.pace.early", out: "fuel.pace.out", lasts: "fuel.pace.lasts" } as const;

/** What the strip means in words; all of it red once the fuel is out. */
const sentence = computed(() => ({ key: SENTENCES[state.value.kind], out: state.value.kind === "empty" }));
</script>

<template>
  <section :class="['card', 'pace', { stale }]" :aria-label="t('fuel.pace.title')">
    <div class="head">
      <span class="card-title caps">{{ t("fuel.pace.title") }}</span>
      <p :class="['sentence', { out: sentence.out }]">
        <Rich :text="t(sentence.key)">
          <template #empty><strong v-if="state.kind === 'out'" class="hot">{{ clock(state.emptyAt) }}</strong></template>
          <template #reset><span v-if="state.kind !== 'idle'">{{ clock(state.end) }}</span></template>
          <template #wait><span v-if="state.kind === 'out'">{{ duration(state.end - state.emptyAt) }}</span></template>
        </Rich>
      </p>
      <span v-if="strip" class="range">{{ strip.range }}</span>
    </div>

    <!-- Drawn for the eye; the sentence above says the same. -->
    <div v-if="strip" class="strip" aria-hidden="true">
      <div class="track">
        <span class="tag" :style="label(strip.now)">{{ t("fuel.strip.now", { time: clock(now) }) }}</span>
        <span class="rail"></span>
        <span class="gone" :style="{ width: `${strip.now}%` }"></span>
        <span v-if="strip.now < 100" class="ahead" :style="{ left: `${strip.now}%` }"></span>
        <span v-if="strip.empty !== null" class="mark out" :style="{ left: `${strip.empty}%` }"></span>
        <span class="mark now" :style="{ left: `${strip.now}%` }"></span>
      </div>
      <div class="hours">
        <span v-for="at in strip.hours" :key="at">{{ clock(at) }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.pace {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  padding: 16px 20px;
}

/* Title, the pace in words, the window's span; only the words may take a second line. */
.head {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}

.head .card-title,
.range {
  flex-shrink: 0;
  white-space: nowrap;
}

.sentence {
  flex: 1 1 auto;
  min-width: 0;
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  color: #c7ccd3;
}

.sentence .hot {
  font-weight: 500;
  color: var(--crash-text);
}

.sentence.out {
  color: var(--crash-text);
}

.stale .sentence.out,
.stale .sentence .hot {
  color: inherit;
}

.range {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.strip {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* "now" above the bar; the bar; the marks across it. */
.track {
  position: relative;
  height: 36px;
}

.tag {
  position: absolute;
  top: -2px;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text);
  white-space: nowrap;
}

.rail,
.gone,
.ahead {
  position: absolute;
  top: 24px;
  height: 8px;
}

.rail {
  left: 0;
  right: 0;
  border-radius: 4px;
  background: var(--line);
}

.gone {
  left: 0;
  border-radius: 4px 0 0 4px;
  background: var(--claude);
}

/* What is still ahead, dashed: the one gradient allowed. */
.ahead {
  right: 0;
  background: repeating-linear-gradient(90deg, rgba(240, 136, 62, 0.38) 0 6px, transparent 6px 10px);
}

.stale .gone {
  background: var(--idle-ring);
}

.stale .ahead {
  background: repeating-linear-gradient(90deg, rgba(125, 131, 141, 0.45) 0 6px, transparent 6px 10px);
}

.mark {
  position: absolute;
  top: 20px;
  width: 2px;
  height: 16px;
  margin-left: -1px;
  border-radius: 1px;
}

.mark.now {
  background: var(--text);
}

.mark.out {
  background: var(--crash);
}

.stale .mark.out {
  background: var(--idle-ring);
}

/* Every hour from the start to the refill, the first and last at the ends. */
.hours {
  display: flex;
  justify-content: space-between;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
}
</style>
