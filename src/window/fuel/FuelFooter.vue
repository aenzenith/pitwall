<script lang="ts">
import { ref } from "vue";

/** The list of the other limits, open or not: kept while the app runs, never stored. */
const shown = ref(false);
</script>

<script setup lang="ts">
import { computed } from "vue";

import { modelName, moneyText, percentText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import type { SpendToday, UsageExtra, UsageWindow } from "../../lib/types";
import AlertChip from "./AlertChip.vue";
import LimitList from "./LimitList.vue";

/**
 * Under the cluster, in one muted line: Extra Usage, the limits without a dial (with a button that
 * lists them), today's top model; and the 90 % notification chip. `rest`: the limits without a
 * dial; `extraOnGauge`: Extra Usage has one.
 */
const props = defineProps<{ rest: UsageWindow[]; extra: UsageExtra | null; extraOnGauge: boolean; today: SpendToday | null; limits: boolean; stale: boolean }>();

const LIST_ID = "fuel-limits";

/** Extra Usage in the list when it is on and has no dial of its own. */
const listedExtra = computed(() => (props.extra?.enabled && !props.extraOnGauge ? props.extra : null));

const extraText = computed(() => {
  const extra = props.extra;
  if (!extra || props.extraOnGauge) return "";
  if (!extra.enabled) return t("fuel.foot.extraOff");
  const money = (amount: number): string => moneyText(amount, extra.currency, language.value);
  return extra.monthlyLimit !== null && extra.monthlyLimit > 0
    ? t("fuel.foot.extraOf", { used: money(extra.usedCredits), cap: money(extra.monthlyLimit) })
    : t("fuel.foot.extraUsed", { used: money(extra.usedCredits) });
});

const unused = computed(() => props.rest.filter((window) => window.used <= 0).length);
const used = computed(() => props.rest.length - unused.value);

/** The model with most of today's tokens, and its share. */
const topModel = computed(() => {
  const today = props.today;
  if (!today?.tokens.total) return "";
  const byName = new Map<string, number>();
  for (const model of today.models) {
    const name = modelName(model.model);
    byName.set(name, (byName.get(name) ?? 0) + model.tokens.total);
  }
  const top = [...byName.entries()].sort((a, b) => b[1] - a[1])[0];
  return top ? t("fuel.foot.topModel", { model: top[0], value: percentText((top[1] / today.tokens.total) * 100, language.value) }) : "";
});

const unpriced = computed(() => {
  const models = [...new Set(props.today?.unpricedModels.map(modelName) ?? [])];
  return models.length ? t("fuel.today.unpriced", { models: models.join(", ") }) : "";
});

const canList = computed(() => props.rest.length > 0 || !!listedExtra.value);
</script>

<template>
  <div class="foot">
    <div class="line">
      <p class="facts">
        <span v-if="extraText" class="fact">{{ extraText }}</span>
        <span v-if="used" class="fact">{{ t("fuel.foot.more", { count: used }) }}</span>
        <span v-if="unused" class="fact">{{ t("fuel.foot.unused", { count: unused }) }}</span>
        <button v-if="canList" type="button" class="toggle" :aria-expanded="shown" :aria-controls="LIST_ID" @click="shown = !shown">
          {{ t(shown ? "fuel.foot.hide" : "fuel.foot.show") }}
        </button>
        <span v-if="topModel" class="fact">{{ topModel }}</span>
        <span v-if="unpriced" class="fact">{{ unpriced }}</span>
      </p>
      <AlertChip v-if="limits" />
    </div>
    <LimitList v-if="shown && canList" :id="LIST_ID" :windows="rest" :extra="listedExtra" :stale="stale" />
  </div>
</template>

<style scoped>
.foot {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.line {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}

/* A muted line; the parts are kept whole, the line wraps between them. */
.facts {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 0;
  margin: 0;
  min-height: 28px;
  font-size: 12px;
  color: var(--text-subtle);
}

.fact {
  white-space: nowrap;
}

/* " · " between the parts, none before the first. */
.fact + .fact::before,
.toggle + .fact::before {
  content: "·";
  margin: 0 7px;
}

.toggle {
  flex-shrink: 0;
  margin-left: 6px;
  padding: 2px 4px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
  text-decoration: underline;
  text-decoration-color: var(--line-strong);
  text-underline-offset: 3px;
}

.toggle:hover {
  color: var(--text-strong);
  text-decoration-color: currentColor;
}

.line :deep(.chip) {
  height: 28px;
}
</style>
