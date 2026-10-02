<script setup lang="ts">
import { computed } from "vue";

import { dayClockText, isLow, left, moneyText, percentText, windowName } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import type { UsageExtra, UsageWindow } from "../../lib/types";

/** The limits the cluster has no dial for, then Extra Usage: a bar of what is left, and when it refills. */
const props = defineProps<{ id: string; windows: UsageWindow[]; extra: UsageExtra | null; stale: boolean }>();

type Row = { id: string; name: string; bar: number | null; low: boolean; amount: string; note: string; off: boolean };

const rows = computed<Row[]>(() => {
  const lang = language.value;
  const list: Row[] = props.windows.map((window) => ({
    id: window.id,
    name: windowName(window, t),
    bar: left(window.used),
    low: isLow(window.used),
    amount: t("fuel.left", { value: percentText(left(window.used), lang) }),
    note: window.resetsAt === null ? t("fuel.notStarted") : t("fuel.refills", { when: dayClockText(window.resetsAt, lang) }),
    off: false,
  }));

  const extra = props.extra;
  if (extra) {
    const money = (amount: number): string => moneyText(amount, extra.currency, lang);
    const capped = extra.monthlyLimit !== null && extra.monthlyLimit > 0;
    const used = extra.used ?? (capped ? (extra.usedCredits / (extra.monthlyLimit as number)) * 100 : 0);
    list.push({
      id: "extra",
      name: t("fuel.extra"),
      bar: extra.enabled && capped ? left(used) : null,
      low: extra.enabled && capped && isLow(used),
      amount: !extra.enabled
        ? t("fuel.extraOff")
        : capped
          ? t("fuel.left", { value: money(Math.max(0, (extra.monthlyLimit as number) - extra.usedCredits)) })
          : t("fuel.extraUsed", { value: money(extra.usedCredits) }),
      note: !extra.enabled ? "" : capped ? t("fuel.extraCap", { value: money(extra.monthlyLimit as number) }) : t("fuel.extraNoCap"),
      off: !extra.enabled,
    });
  }
  return list;
});
</script>

<template>
  <ul :id="id" :class="['list', { stale }]">
    <li v-for="row in rows" :key="row.id" class="row">
      <span class="name" :title="row.name">{{ row.name }}</span>
      <span class="bar" aria-hidden="true">
        <span v-if="row.bar !== null" :class="['level', { low: row.low }]" :style="{ width: `${row.bar}%` }"></span>
      </span>
      <span :class="['amount', { low: row.low, off: row.off }]">{{ row.amount }}</span>
      <span class="note">{{ row.note }}</span>
    </li>
  </ul>
</template>

<style scoped>
/* One grid for every row, so the columns line up whatever the language. */
.list {
  display: grid;
  grid-template-columns: fit-content(220px) minmax(60px, 1fr) max-content max-content;
  column-gap: 16px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.row {
  display: grid;
  grid-column: 1 / -1;
  grid-template-columns: subgrid;
  align-items: center;
  padding: 7px 2px;
  border-bottom: 1px solid #1e2127;
}

.name {
  min-width: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bar {
  height: 8px;
  border-radius: 4px;
  background: var(--line);
  overflow: hidden;
}

.level {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: var(--claude);
  transition: width 0.6s ease;
}

.level.low {
  background: var(--crash);
}

.stale .level {
  background: var(--idle-ring);
}

.amount {
  font-family: var(--font-mono);
  font-size: 13px;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.amount.low {
  color: var(--crash-text);
}

.amount.off {
  font-family: var(--font);
  color: var(--text-faint);
}

.stale .amount {
  color: var(--text-muted);
}

.note {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

@media (prefers-reduced-motion: reduce) {
  .level {
    transition: none;
  }
}
</style>
