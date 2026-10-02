<script setup lang="ts">
import { computed } from "vue";

import { burnRate, clockText, compactText, LOW_LEFT, leftAtRefill, moneyText, pace, percentText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import type { SpendToday, UsageWindow } from "../../lib/types";

/**
 * The cluster's trip computer: the session's burn rate, what is left when it refills (or when it
 * runs out first), and today's tokens with what the API would have charged. `readAt`: when the
 * session was read.
 */
const props = defineProps<{ session: UsageWindow | null; readAt: number; today: SpendToday | null; stale: boolean }>();

const NONE = "—";

type Cell = { key: string; value: string; label: string; hot: boolean };

const cells = computed<Cell[]>(() => {
  const lang = language.value;
  const session = props.session;
  const rate = session ? burnRate(session, props.readAt) : null;
  const state = session ? pace(session, props.readAt) : null;
  const atRefill = session ? leftAtRefill(session, props.readAt) : null;
  const today = props.today;

  const refill: Cell =
    state?.kind === "out"
      ? { key: "refill", value: t("fuel.strip.empty", { time: clockText(state.emptyAt, lang) }), label: t("fuel.trip.atPace"), hot: true }
      : {
          key: "refill",
          value: atRefill === null ? NONE : `~${percentText(atRefill, lang)}`,
          label: t("fuel.trip.atRefill"),
          hot: atRefill !== null && atRefill <= LOW_LEFT,
        };

  return [
    { key: "rate", value: rate === null ? NONE : t("fuel.trip.rateValue", { value: percentText(rate, lang, 1) }), label: t("fuel.trip.rate"), hot: false },
    refill,
    { key: "tokens", value: today ? compactText(today.tokens.total, lang) : NONE, label: t("fuel.trip.tokens"), hot: false },
    { key: "cost", value: today ? `≈ ${moneyText(today.costUsd, "USD", lang)}` : NONE, label: t("fuel.trip.cost"), hot: false },
  ];
});
</script>

<template>
  <dl :class="['trip', { stale }]">
    <div v-for="cell in cells" :key="cell.key" class="cell">
      <dt class="label" :title="cell.label">{{ cell.label }}</dt>
      <dd :class="['value', { hot: cell.hot, none: cell.value === NONE }]">{{ cell.value }}</dd>
    </div>
  </dl>
</template>

<style scoped>
/* Four equal cells divided by hairlines. */
.trip {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  margin: 0;
  border-top: 1px solid var(--line);
  background: var(--bg-footer);
}

/* The value over its label, as on a car's display; the label comes first for VoiceOver. */
.cell {
  display: flex;
  flex-direction: column-reverse;
  justify-content: flex-end;
  gap: 3px;
  min-width: 0;
  padding: 12px 16px;
}

.cell + .cell {
  border-left: 1px solid var(--line);
}

.value {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 18px;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-variant-numeric: tabular-nums;
}

.value.hot {
  color: var(--crash-text);
}

.value.none {
  color: var(--text-faint);
}

.label {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.stale .value.hot {
  color: var(--text-muted);
}

/* A narrow window: smaller figures, so a long one ("~20:14 に枯渇") still fits its cell. */
@container (max-width: 720px) {
  .cell {
    padding: 10px 12px;
  }

  .value {
    font-size: 15px;
  }
}
</style>
