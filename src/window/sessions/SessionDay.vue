<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

import { clock, clockRange, duration, hourTicks, pct } from "../../lib/day";
import { t } from "../../lib/i18n";
import { sessionRange } from "../../lib/sessions";
import type { SessionRow } from "../../lib/types";

/** A session's day: its work (solid) and its waits on you (hatched) on the hours around them,
 * and what they add up to. */
const props = defineProps<{ row: SessionRow; now: number }>();

const today = computed(() => props.row.today);
const spans = computed(() => today.value?.spans ?? []);
/** Still running: the hours reach now, and now is marked. */
const live = computed(() => props.row.phase !== "ended");
const range = computed(() => sessionRange(spans.value, props.now, live.value));

/** The hour row's width, so labels are spaced to fit and step aside for now. */
const axis = ref<HTMLElement | null>(null);
const axisWidth = ref(0);
const observer = new ResizeObserver(([entry]) => (axisWidth.value = entry.contentRect.width));

watch(axis, (element, old) => {
  if (old) observer.unobserve(old);
  if (element) observer.observe(element);
});

onBeforeUnmount(() => observer.disconnect());

const ticks = computed(() => hourTicks(range.value, axisWidth.value, live.value ? props.now : null));
const nowLeft = computed(() => (live.value && range.value ? `${pct(props.now, range.value)}%` : null));

const blocks = computed(() =>
  spans.value.map((span, i) => {
    const left = pct(span.start, range.value);
    const lasted = duration(span.end - span.start);
    return {
      key: i,
      kind: span.kind,
      style: { left: `${left}%`, width: `${Math.max(pct(span.end, range.value) - left, 0.6)}%` },
      title: `${t(span.kind === "work" ? "day.tip.work" : "day.tip.wait", { duration: lasted })} · ${clockRange(span.start, span.end)}`,
    };
  }),
);

/** The bar as one sentence, for screen readers. */
const summary = computed(() => {
  const day = today.value;
  if (!day || !spans.value.length) return "";
  const first = Math.min(...spans.value.map((span) => span.start));
  const last = Math.max(...spans.value.map((span) => span.end));
  return t("sessions.day.summary", { work: duration(day.work), wait: duration(day.wait), range: clockRange(first, last) });
});

const stats = computed(() => {
  const day = today.value;
  if (!day) return [];
  const list = [
    { key: "work", label: t("sessions.stat.worked"), value: duration(day.work) },
    { key: "wait", label: t("sessions.stat.waited"), value: duration(day.wait) },
    { key: "turns", label: t("sessions.stat.turns"), value: String(day.turns) },
  ];
  if (props.row.subagents > 0) list.push({ key: "agents", label: t("sessions.stat.subagents"), value: String(props.row.subagents) });
  return list;
});
</script>

<template>
  <section class="block" :aria-label="t('day.today')">
    <div class="section-label">{{ t("day.today") }}</div>

    <template v-if="range">
      <div class="track" role="img" :aria-label="summary">
        <i v-for="tick in ticks.filter((tick) => tick.major)" :key="tick.at" class="hour" :style="{ left: tick.left }"></i>
        <i v-for="block in blocks" :key="block.key" :class="['span', block.kind]" :style="block.style" :title="block.title"></i>
        <i v-if="nowLeft" class="now" :style="{ left: nowLeft }"></i>
      </div>
      <div ref="axis" class="axis" aria-hidden="true">
        <span v-for="tick in ticks" :key="tick.at" class="tick" :style="{ left: tick.left, transform: `translateX(-${tick.left})` }">{{ tick.label }}</span>
        <span v-if="nowLeft" class="tick now-label" :style="{ left: nowLeft, transform: `translateX(-${nowLeft})` }" :title="clock(now)">{{
          t("sessions.day.now")
        }}</span>
      </div>
    </template>
    <p v-else class="none">{{ t("sessions.day.none") }}</p>

    <dl v-if="stats.length" class="stats" :style="{ '--columns': stats.length === 4 ? 2 : 3 }">
      <div v-for="stat in stats" :key="stat.key" class="stat">
        <dt class="stat-label">{{ stat.label }}</dt>
        <dd :class="['stat-value', { empty: stat.value === '—' }]">{{ stat.value }}</dd>
      </div>
    </dl>
  </section>
</template>

<style scoped src="../detail/detail.css"></style>
<style scoped>
.track {
  position: relative;
  height: 14px;
  border-radius: 4px;
  background: #1c1f25;
  overflow: hidden;
}

.track i {
  position: absolute;
  top: 0;
  bottom: 0;
}

/* The hours, faintly, under the spans. */
.hour {
  width: 1px;
  background: #262a31;
}

.span.work {
  background: linear-gradient(180deg, #f6a560, #e3792e);
}

.span.wait {
  background: repeating-linear-gradient(135deg, rgba(240, 136, 62, 0.55) 0 2px, transparent 2px 5px);
}

.now {
  width: 2px;
  margin-left: -1px;
  background: var(--text-muted);
}

/* Each label on its hour, moved back by its own width times how far along it is: centred in the
   middle, flush with the bar at its ends, so none reaches past the bar. */
.axis {
  position: relative;
  height: 14px;
  margin-top: -4px;
}

.tick {
  position: absolute;
  top: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 14px;
  color: var(--text-faint);
  white-space: nowrap;
}

.now-label {
  color: var(--text-muted);
  font-family: var(--font);
}

.none {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.stats {
  display: grid;
  /* Three side by side; four as two pairs, never three and one. */
  grid-template-columns: repeat(var(--columns), minmax(0, 1fr));
  gap: 10px;
  margin: 4px 0 0;
}

.stat {
  display: flex;
  flex-direction: column-reverse;
  gap: 2px;
  min-width: 0;
}

.stat-value {
  margin: 0;
  font-size: 17px;
  font-weight: 600;
  color: var(--text-strong);
  white-space: nowrap;
}

.stat-value.empty {
  color: var(--text-faint);
}

.stat-label {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
