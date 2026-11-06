<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import { agentKey } from "../../lib/format";
import { language, t } from "../../lib/i18n";
import { hasAgent, noonOf, shareText, type Lap } from "../../lib/laps";
import { isLinux, isMac, isWindows } from "../../lib/platform";
import type { Agent } from "../../lib/types";

/**
 * Gelişmiş, the edit dialog's right half (TN4): every group optional, each closed one saying its
 * value in a line (empty while nothing is set where nothing is the default). The groups a kind
 * has no use for aren't shown: a script job has no fuel, no tab and nothing to carry. It writes
 * into the job being edited (`lap`), which the dialog saves whole.
 */
const props = defineProps<{ lap: Lap; agent: Agent; git: boolean | null }>();

type Group = "gate" | "time" | "fuel" | "where" | "missed" | "overlap" | "carry" | "error" | "notify" | "output" | "ends";

const open = ref<Set<Group>>(new Set());

function toggle(group: Group): void {
  const next = new Set(open.value);
  if (next.has(group)) next.delete(group);
  else next.add(group);
  open.value = next;
}

const withAgent = computed(() => hasAgent(props.lap.kind));

/** A whole figure typed into a sentence, kept within its bounds; anything else puts back the old. */
function figure(event: Event, min: number, max: number, old: number): number {
  const input = event.target as HTMLInputElement;
  const n = Math.round(Number(input.value.replace(/[^\d]/g, "")));
  const value = Number.isFinite(n) && input.value.trim() ? Math.min(max, Math.max(min, n)) : old;
  input.value = String(value);
  return value;
}

/* ---------- the summaries ---------- */

const summaries = computed<Record<Group, string>>(() => {
  const lap = props.lap;
  const gate: string[] = [];
  if (lap.kind === "scriptAgent" && lap.gate.script === "onFailure") gate.push(t("laps.adv.gate.sum.onFailure"));
  if (lap.kind === "scriptAgent" && lap.gate.script === "onOutput") gate.push(t("laps.adv.gate.sum.onOutput"));
  if (lap.gate.git) gate.push(t("laps.adv.git.sum"));
  if (lap.gate.sameSilent) gate.push(t("laps.adv.same.sum"));

  const time: string[] = [];
  if (lap.timeoutMin !== null) time.push(t("laps.time.sum.timeout", { minutes: lap.timeoutMin }));
  if (lap.awake) time.push(t(isMac ? "laps.time.sum.awake" : "laps.time.sum.awake.other"));

  const notify = lap.notify.done && lap.notify.failed ? "both" : lap.notify.done ? "done" : lap.notify.failed ? "failed" : "none";
  const error: string[] = [];
  if (lap.retry) error.push(t("laps.adv.retry.sum", { count: lap.retry.times }));
  if (lap.pauseAfter !== null) error.push(t("laps.adv.pause.sum", { count: lap.pauseAfter }));

  return {
    gate: gate.join(" · "),
    time: time.join(" · "),
    fuel: lap.fuel ? t(lap.fuel.then === "wait" ? "laps.adv.fuel.sum.wait" : "laps.adv.fuel.sum.skip", { share: shareText(lap.fuel.over) }) : "",
    where: t(lap.where === "background" ? "laps.adv.where.background" : "laps.adv.where.tab"),
    missed: lap.missed.catchUp ? t("laps.adv.missed.sum", { hours: hours(lap.missed.graceMin) }) : t("laps.adv.missed.skip"),
    overlap: t(`laps.adv.overlap.${lap.overlap}`),
    carry: lap.carry ? t("laps.adv.carry.on") : "",
    error: error.join(" · "),
    notify: `${t(`laps.adv.notify.sum.${notify}`)} · ${t(lap.notify.sound ? "laps.adv.notify.sum.sound" : "laps.adv.notify.sum.mute")}`,
    output: lap.file ? t("laps.adv.output.sumFile", { file: lap.file, count: lap.keep }) : t("laps.adv.output.sum", { count: lap.keep }),
    ends:
      lap.ends.at !== null
        ? t("laps.adv.ends.sum.at", { date: dateText(lap.ends.at) })
        : lap.ends.after !== null
          ? t("laps.adv.ends.sum.after", { count: lap.ends.after })
          : t("laps.adv.ends.none"),
  };
});

function hours(minutes: number): string {
  const h = minutes / 60;
  return Number.isInteger(h) ? String(h) : h.toLocaleString(language.value, { maximumFractionDigits: 1 });
}

function dateText(at: number): string {
  return new Date(at).toLocaleDateString(language.value, { day: "numeric", month: "short", year: "numeric" });
}

type IconName = InstanceType<typeof Icon>["$props"]["name"];
type Row = { id: Group; label: `laps.adv.${Group}`; icon: IconName };

const groups = computed<Row[]>(() => {
  const list: Row[] = [
    { id: "gate", label: "laps.adv.gate", icon: "source-control" },
    { id: "time", label: "laps.adv.time", icon: "stop" },
  ];
  // On Windows a lap's agent always runs in the background (the core's `lap_agent`).
  if (withAgent.value) {
    list.push({ id: "fuel", label: "laps.adv.fuel", icon: "fuel" });
    if (!isWindows) list.push({ id: "where", label: "laps.adv.where", icon: "terminal" });
  }
  list.push({ id: "missed", label: "laps.adv.missed", icon: "moon" }, { id: "overlap", label: "laps.adv.overlap", icon: "clock" });
  if (withAgent.value) list.push({ id: "carry", label: "laps.adv.carry", icon: "restart" });
  list.push(
    { id: "error", label: "laps.adv.error", icon: "close" },
    { id: "notify", label: "laps.adv.notify", icon: "bell" },
    { id: "output", label: "laps.adv.output", icon: "file" },
    { id: "ends", label: "laps.adv.ends", icon: "calendar" },
  );
  return list;
});

/* ---------- the fields ---------- */

function setTimeout_(on: boolean): void {
  props.lap.timeoutMin = on ? (props.lap.timeoutMin ?? 20) : null;
}

function setFuel(on: boolean): void {
  props.lap.fuel = on ? (props.lap.fuel ?? { over: 85, then: "skip" }) : null;
}

function setRetry(on: boolean): void {
  props.lap.retry = on ? (props.lap.retry ?? { times: 1, everyMin: 10 }) : null;
}

function setPause(on: boolean): void {
  props.lap.pauseAfter = on ? (props.lap.pauseAfter ?? 3) : null;
}

function setFile(on: boolean): void {
  props.lap.file = on ? (props.lap.file ?? "") : null;
}

function setFileText(event: Event): void {
  props.lap.file = (event.target as HTMLInputElement).value;
}

type Ends = "none" | "at" | "after";
const ends = computed<Ends>(() => (props.lap.ends.at !== null ? "at" : props.lap.ends.after !== null ? "after" : "none"));

function setEnds(to: Ends): void {
  if (to === "none") props.lap.ends = { at: null, after: null };
  else if (to === "at") props.lap.ends = { at: props.lap.ends.at ?? Date.now() + 30 * 86_400_000, after: null };
  else props.lap.ends = { at: null, after: props.lap.ends.after ?? 10 };
}

const endsDate = computed(() => {
  const at = props.lap.ends.at;
  if (at === null) return "";
  const d = new Date(at);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
});

function setEndsDate(event: Event): void {
  const value = (event.target as HTMLInputElement).value;
  // The end of that day: the job runs on it still.
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) props.lap.ends = { at: noonOf(value) + 12 * 3_600_000 - 60_000, after: null };
}
</script>

<template>
  <div class="advanced">
    <div class="advanced-head">
      <span class="label">{{ t("laps.adv.title") }}</span>
      <span class="hint">{{ t("laps.adv.optional") }}</span>
    </div>
    <div class="groups">
      <template v-for="group in groups" :key="group.id">
        <button type="button" class="group-head" :aria-expanded="open.has(group.id)" @click="toggle(group.id)">
          <Icon :name="open.has(group.id) ? 'chevron-down' : 'chevron'" :size="13" class="chev" />
          <span class="group-name"><Icon :name="group.icon" :size="14" />{{ t(group.label) }}</span>
          <span v-if="!open.has(group.id)" class="summary">{{ summaries[group.id] }}</span>
        </button>

        <div v-if="open.has(group.id)" class="group-body">
          <!-- Koşul -->
          <template v-if="group.id === 'gate'">
            <template v-if="lap.kind === 'scriptAgent'">
              <label v-for="one in (['none', 'onFailure', 'onOutput'] as const)" :key="one" class="check">
                <input v-model="lap.gate.script" type="radio" name="lap-gate" :value="one" />
                {{ t(`laps.adv.gate.${one}`) }}
              </label>
            </template>
            <label :class="['check', { off: git === false }]">
              <input v-model="lap.gate.git" type="checkbox" :disabled="git === false && !lap.gate.git" />
              {{ t("laps.adv.git") }}
            </label>
            <span v-if="git === false" class="hint indent">{{ t("laps.adv.git.not") }}</span>
            <label class="check">
              <input v-model="lap.gate.sameSilent" type="checkbox" />
              {{ t("laps.adv.same") }}
            </label>
          </template>

          <!-- Süre -->
          <template v-else-if="group.id === 'time'">
            <label class="check">
              <input type="checkbox" :checked="lap.timeoutMin !== null" @change="setTimeout_(($event.target as HTMLInputElement).checked)" />
              <Rich :text="t('laps.adv.timeout')">
                <template #minutes>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="lap.timeoutMin ?? 20"
                    :disabled="lap.timeoutMin === null"
                    :aria-label="t('laps.adv.time')"
                    @change="lap.timeoutMin = figure($event, 1, 1440, lap.timeoutMin ?? 20)"
                  />
                </template>
              </Rich>
            </label>
            <label :class="['check', { off: isLinux }]" :title="isLinux ? t('laps.adv.awake.linux') : undefined">
              <input v-model="lap.awake" type="checkbox" :disabled="isLinux" />
              {{ t(isMac ? "laps.adv.awake" : "laps.adv.awake.other") }}
            </label>
          </template>

          <!-- Yakıt -->
          <template v-else-if="group.id === 'fuel'">
            <div class="check">
              <input type="checkbox" :checked="lap.fuel !== null" :aria-label="t('laps.adv.fuel')" @change="setFuel(($event.target as HTMLInputElement).checked)" />
              <Rich :text="t(agentKey('laps.adv.fuel.when', agent))">
                <template #share>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="shareText(lap.fuel?.over ?? 85)"
                    :disabled="lap.fuel === null"
                    :aria-label="t('laps.adv.fuel')"
                    @change="lap.fuel && (lap.fuel.over = figure($event, 1, 100, lap.fuel.over))"
                    @blur="($event.target as HTMLInputElement).value = shareText(lap.fuel?.over ?? 85)"
                  />
                </template>
              </Rich>
              <span class="seg small">
                <button type="button" :class="{ on: lap.fuel?.then !== 'wait' }" :disabled="lap.fuel === null" @click="lap.fuel && (lap.fuel.then = 'skip')">{{ t("laps.adv.fuel.skip") }}</button>
                <button type="button" :class="{ on: lap.fuel?.then === 'wait' }" :disabled="lap.fuel === null" @click="lap.fuel && (lap.fuel.then = 'wait')">{{ t("laps.adv.fuel.wait") }}</button>
              </span>
            </div>
          </template>

          <!-- Nerede koşar -->
          <template v-else-if="group.id === 'where'">
            <label v-for="one in (['tab', 'background'] as const)" :key="one" class="check">
              <input v-model="lap.where" type="radio" name="lap-where" :value="one" />
              {{ t(`laps.adv.where.${one}`) }}
            </label>
            <span class="hint">{{ t("laps.adv.where.hint") }}</span>
          </template>

          <!-- Kaçan koşu -->
          <template v-else-if="group.id === 'missed'">
            <label class="check">
              <input v-model="lap.missed.catchUp" type="radio" name="lap-missed" :value="true" />
              <Rich :text="t('laps.adv.missed.catchUp')">
                <template #hours>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="hours(lap.missed.graceMin)"
                    :disabled="!lap.missed.catchUp"
                    :aria-label="t('laps.adv.missed')"
                    @change="lap.missed.graceMin = figure($event, 1, 72, lap.missed.graceMin / 60) * 60"
                  />
                </template>
              </Rich>
            </label>
            <label class="check">
              <input v-model="lap.missed.catchUp" type="radio" name="lap-missed" :value="false" />
              {{ t("laps.adv.missed.skip") }}
            </label>
          </template>

          <!-- Çakışma -->
          <template v-else-if="group.id === 'overlap'">
            <label v-for="one in (['skip', 'wait', 'stop'] as const)" :key="one" class="check">
              <input v-model="lap.overlap" type="radio" name="lap-overlap" :value="one" />
              {{ t(`laps.adv.overlap.${one}`) }}
            </label>
          </template>

          <!-- Süreklilik -->
          <template v-else-if="group.id === 'carry'">
            <label class="check">
              <input v-model="lap.carry" type="checkbox" />
              {{ t("laps.adv.carry.on") }}
            </label>
            <span class="hint">{{ t("laps.adv.carry.hint") }}</span>
          </template>

          <!-- Hata -->
          <template v-else-if="group.id === 'error'">
            <label class="check">
              <input type="checkbox" :checked="lap.retry !== null" @change="setRetry(($event.target as HTMLInputElement).checked)" />
              <Rich :text="t('laps.adv.retry')">
                <template #times>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="lap.retry?.times ?? 1"
                    :disabled="lap.retry === null"
                    :aria-label="t('laps.adv.error')"
                    @change="lap.retry && (lap.retry.times = figure($event, 1, 5, lap.retry.times))"
                  />
                </template>
                <template #minutes>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="lap.retry?.everyMin ?? 10"
                    :disabled="lap.retry === null"
                    :aria-label="t('laps.adv.error')"
                    @change="lap.retry && (lap.retry.everyMin = figure($event, 1, 120, lap.retry.everyMin))"
                  />
                </template>
              </Rich>
            </label>
            <label class="check">
              <input type="checkbox" :checked="lap.pauseAfter !== null" @change="setPause(($event.target as HTMLInputElement).checked)" />
              <Rich :text="t('laps.adv.pause', { count: lap.pauseAfter ?? 3 })">
                <template #n>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="lap.pauseAfter ?? 3"
                    :disabled="lap.pauseAfter === null"
                    :aria-label="t('laps.adv.error')"
                    @change="lap.pauseAfter = figure($event, 1, 20, lap.pauseAfter ?? 3)"
                  />
                </template>
              </Rich>
            </label>
          </template>

          <!-- Bildirim -->
          <template v-else-if="group.id === 'notify'">
            <div class="checks-row">
              <label class="check"><input v-model="lap.notify.done" type="checkbox" />{{ t("laps.adv.notify.done") }}</label>
              <label class="check"><input v-model="lap.notify.failed" type="checkbox" />{{ t("laps.adv.notify.failed") }}</label>
              <label class="check"><input v-model="lap.notify.sound" type="checkbox" />{{ t("laps.adv.notify.sound") }}</label>
            </div>
          </template>

          <!-- Çıktı -->
          <template v-else-if="group.id === 'output'">
            <div class="check">
              <Rich :text="t('laps.adv.keep', { count: lap.keep })">
                <template #n>
                  <input class="input num" inputmode="numeric" :value="lap.keep" :aria-label="t('laps.adv.output')" @change="lap.keep = figure($event, 1, 100, lap.keep)" />
                </template>
              </Rich>
            </div>
            <label class="check">
              <input type="checkbox" :checked="lap.file !== null" @change="setFile(($event.target as HTMLInputElement).checked)" />
              {{ t("laps.adv.file") }}
            </label>
            <template v-if="lap.file !== null">
              <input class="input mono indent-input" spellcheck="false" :value="lap.file" :placeholder="t('laps.adv.file.placeholder')" :aria-label="t('laps.adv.file')" @input="setFileText" />
              <span class="hint indent">{{ t(withAgent ? "laps.adv.file.hint" : "laps.adv.file.hint.script") }}</span>
            </template>
          </template>

          <!-- Tekrar ve bitiş -->
          <template v-else-if="group.id === 'ends'">
            <label class="check">
              <input type="radio" name="lap-ends" :checked="ends === 'none'" @change="setEnds('none')" />
              {{ t("laps.adv.ends.none") }}
            </label>
            <label class="check">
              <input type="radio" name="lap-ends" :checked="ends === 'at'" @change="setEnds('at')" />
              <Rich :text="t('laps.adv.ends.at')">
                <template #date>
                  <input class="input date" type="date" :value="endsDate" :disabled="ends !== 'at'" :aria-label="t('laps.adv.ends')" @change="setEndsDate" />
                </template>
              </Rich>
            </label>
            <label class="check">
              <input type="radio" name="lap-ends" :checked="ends === 'after'" @change="setEnds('after')" />
              <Rich :text="t('laps.adv.ends.after', { count: lap.ends.after ?? 10 })">
                <template #n>
                  <input
                    class="input num"
                    inputmode="numeric"
                    :value="lap.ends.after ?? 10"
                    :disabled="ends !== 'after'"
                    :aria-label="t('laps.adv.ends')"
                    @change="lap.ends = { at: null, after: figure($event, 1, 10000, lap.ends.after ?? 10) }"
                  />
                </template>
              </Rich>
            </label>
          </template>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.advanced {
  flex-grow: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.advanced-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 2px 8px;
}

.groups {
  min-height: 0;
  overflow-y: auto;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--bg-well);
}

.group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-height: 34px;
  padding: 0 12px 0 10px;
  border: 0;
  border-top: 1px solid var(--line-soft);
  background: transparent;
  color: var(--text);
  font-size: 13px;
  text-align: left;
}

.group-head:first-child {
  border-top: 0;
}

.group-head:hover {
  background: var(--bg-row-hover);
}

.chev {
  flex-shrink: 0;
  color: var(--text-faint);
}

.group-name {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.group-name :deep(svg) {
  color: var(--text-muted);
}

.summary {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: right;
  font-size: 12px;
  color: var(--text-muted);
}

.group-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 2px 14px 12px 33px;
}

.check {
  flex-wrap: wrap;
  row-gap: 4px;
}

.checks-row {
  display: flex;
  gap: 18px;
}

.indent {
  padding-left: 24px;
}

.indent-input {
  margin-left: 24px;
  height: 28px;
}

.input.date {
  height: 26px;
  font-size: 12px;
}
</style>
