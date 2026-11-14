<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Spinner from "../../../components/Spinner.vue";
import { t } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { projectName, share, useAsk, when, type DialogProps } from "./shared";

/**
 * Usage (B6): the plan's limits as Claude Code reads them for this session (`get_usage`: the five
 * hours, the week, a model's week…), each with how much is used and when it refills, then the
 * session's own totals as the CLI words them (`get_session_cost`). Nothing of it is kept.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Span = { utilization?: number | null; resets_at?: string | null } | null | undefined;
type Limit = {
  kind?: string;
  group?: string;
  percent?: number | null;
  severity?: string;
  resets_at?: string | null;
  scope?: { model?: { display_name?: string | null } | null } | null;
};
type Usage = {
  subscription_type?: string | null;
  rate_limits_available?: boolean;
  rate_limits?: { five_hour?: Span; seven_day?: Span; seven_day_opus?: Span; seven_day_sonnet?: Span; limits?: Limit[] } | null;
  session?: { total_cost_usd?: number; total_lines_added?: number; total_lines_removed?: number } | null;
};
type Cost = { text?: string };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const usage = useAsk<Usage>(() => props.id);
const cost = useAsk<Cost>(() => props.id);
onMounted(() => {
  void usage.load({ kind: "usage" });
  void cost.load({ kind: "sessionCost" });
});

type Meter = { key: string; label: string; used: number; refill: string; level: "" | "near" | "full" };

function level(severity: string | undefined, used: number): Meter["level"] {
  if (severity === "critical" || severity === "exceeded" || severity === "blocked" || used >= 100) return "full";
  if (severity === "warning" || severity === "high") return "near";
  return "";
}

function fromWindow(key: string, label: string, span: Span): Meter | null {
  if (!span || typeof span.utilization !== "number") return null;
  const used = span.utilization;
  return { key, label, used, refill: when(span.resets_at), level: level(undefined, used) };
}

function limitLabel(limit: Limit): string {
  const model = limit.scope?.model?.display_name;
  if (limit.kind === "session" || limit.kind === "five_hour" || limit.group === "session") return t("chatDialog.usage.fiveHour");
  if (model) return t("chatDialog.usage.weekModel", { model });
  if (limit.kind === "weekly_all" || limit.group === "weekly") return t("chatDialog.usage.week");
  return limit.kind ?? "";
}

/** The CLI's own list of limits when it gives one, else its windows one by one. */
const meters = computed<Meter[]>(() => {
  const limits = usage.value.value?.rate_limits;
  if (!limits) return [];
  const rows: Meter[] = [];
  const listed = Array.isArray(limits.limits) ? limits.limits : [];
  for (const [i, limit] of listed.entries()) {
    if (typeof limit.percent !== "number") continue;
    rows.push({ key: `${limit.kind ?? "limit"}-${i}`, label: limitLabel(limit), used: limit.percent, refill: when(limit.resets_at), level: level(limit.severity, limit.percent) });
  }
  const hasSession = listed.some((limit) => limit.kind === "session" || limit.kind === "five_hour" || limit.group === "session");
  const fiveHour = hasSession ? null : fromWindow("five_hour", t("chatDialog.usage.fiveHour"), limits.five_hour);
  if (fiveHour) rows.unshift(fiveHour);
  if (!listed.length) {
    for (const extra of [
      fromWindow("seven_day", t("chatDialog.usage.week"), limits.seven_day),
      fromWindow("seven_day_opus", t("chatDialog.usage.weekModel", { model: "Opus" }), limits.seven_day_opus),
      fromWindow("seven_day_sonnet", t("chatDialog.usage.weekModel", { model: "Sonnet" }), limits.seven_day_sonnet),
    ]) {
      if (extra) rows.push(extra);
    }
  }
  return rows;
});

const plan = computed(() => {
  const type = usage.value.value?.subscription_type;
  return type ? t("chatDialog.usage.plan", { plan: type.charAt(0).toUpperCase() + type.slice(1) }) : projectName(props.path);
});

/** The CLI's summary, else what the totals say. */
const totals = computed(() => {
  const text = cost.value.value?.text?.trim();
  if (text) return text;
  const session = usage.value.value?.session;
  if (!session || typeof session.total_cost_usd !== "number") return "";
  const dollars = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 4 }).format(session.total_cost_usd);
  return t("chatDialog.usage.totals", { cost: dollars, added: session.total_lines_added ?? 0, removed: session.total_lines_removed ?? 0 });
});

const loading = computed(() => (usage.busy.value || cost.busy.value) && !usage.value.value && !cost.value.value);
/** Neither answered: one error stands for both. */
const failed = computed(() => (!loading.value && !usage.value.value && !cost.value.value ? (usage.error.value ?? cost.error.value) : null));
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.usage.title')" :sub="plan" :width="460" @close="emit('close')">
    <div v-if="loading" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="failed" :id="id" :state="state" :error="failed" @done="frame?.close()" />

    <div v-else class="scroll">
      <div class="part">
        <span class="section-label">{{ t("chatDialog.usage.limits") }}</span>
        <div v-if="usage.busy.value" class="state small"><Spinner :size="13" /></div>
        <ul v-else-if="meters.length" class="meters">
          <li v-for="meter in meters" :key="meter.key" class="meter">
            <div class="line">
              <span class="label">{{ meter.label }}</span>
              <span class="value">{{ share(meter.used / 100) }}</span>
            </div>
            <div class="track" role="meter" :aria-label="meter.label" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="Math.round(meter.used)">
              <span :class="['fill', meter.level]" :style="{ width: `${Math.min(100, Math.max(0, meter.used))}%` }"></span>
            </div>
            <span v-if="meter.refill" class="refill">{{ t("chatDialog.usage.refills", { when: meter.refill }) }}</span>
          </li>
        </ul>
        <p v-else class="note">{{ usage.error.value ? t("chatDialog.usage.noLimitsError") : t("chatDialog.usage.noLimits") }}</p>
      </div>

      <div class="part">
        <span class="section-label">{{ t("chatDialog.usage.session") }}</span>
        <div v-if="cost.busy.value && !totals" class="state small"><Spinner :size="13" /></div>
        <pre v-else-if="totals" class="block selectable">{{ totals }}</pre>
        <p v-else class="note">{{ t("chatDialog.usage.noTotals") }}</p>
      </div>
    </div>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.scroll {
  gap: 16px;
}

.part {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.state.small {
  min-height: 40px;
}

.meters {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.meter {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.line {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.label {
  flex: 1 1 auto;
  min-width: 0;
  font-weight: 500;
  color: var(--text-strong);
}

.value {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

.track {
  height: 6px;
  border-radius: 3px;
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-soft);
  overflow: hidden;
}

/* Neutral while there is room; the app's state colours once a limit nears or is reached. */
.fill {
  display: block;
  height: 100%;
  border-radius: 3px;
  background: var(--text-muted);
}

.fill.near {
  background: var(--warn);
}

.fill.full {
  background: var(--crash);
}

.refill {
  font-size: 12px;
  color: var(--text-subtle);
}
</style>
