<script setup lang="ts">
import { computed, ref, watch } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { agentName } from "../../lib/agents";
import { agentKey } from "../../lib/format";
import { language, t } from "../../lib/i18n";
import {
  countdown,
  hasAgent,
  KIND_LABELS,
  lapAgent,
  phaseText,
  reasonText,
  refusalText,
  resultLabel,
  runLength,
  runMoment,
  scheduled,
  shareText,
  whenText,
  type LapRun,
  type LapView,
} from "../../lib/laps";
import { useNativeMenu } from "../../lib/nativeMenu";
import { isMac } from "../../lib/platform";
import { codexModelLabel, modelLabel, timeParams } from "../../lib/start";
import { api, snapshot } from "../../lib/store";
import LapMark from "./LapMark.vue";

/**
 * The selected job (TN1 with DG3's head): its name, its agent and model and where it runs, the
 * countdown ring; Şimdi çalıştır (Durdur while it runs), Düzenle and ⋯ (Bu seferi atla, Duraklat
 * or Sürdür, İşi sil). Under it what the job is (only what is set), then its kept runs, newest
 * first: a row opens its result, the run that goes its terminal. The first line of what the agent said in each run that went well
 * is read when the job is shown, and held here only.
 */
const props = defineProps<{ lap: LapView; now: number; elsewhere: boolean }>();
const emit = defineEmits<{ edit: []; remove: []; openRun: [run: string] }>();

const agent = computed(() => lapAgent(props.lap));
const withAgent = computed(() => hasAgent(props.lap.kind));

/** `Codex · gpt-6.1-terra`, `Claude · Opus`; the agent alone on its own choice of model. */
const model = computed(() => {
  const start = props.lap.start;
  if (agent.value === "codex") return start.codex?.model ? codexModelLabel(start.codex.model) : null;
  return start.model ? modelLabel(start.model) : null;
});

const place = computed(() => props.lap.project ?? props.lap.folder);

/* ---------- the ring ---------- */

const R = 66;
const CIRCLE = 2 * Math.PI * R;

/** The moment before the next one: the last run's, else when the job was made. */
const from = computed(() => {
  const next = props.lap.nextAt;
  const before = props.lap.last.map((mark) => mark.due).filter((due) => next === null || due < next);
  return before.length ? Math.max(...before) : props.lap.createdAt;
});

const ring = computed<{ big: string; small: string; arc: number }>(() => {
  const lap = props.lap;
  if (lap.running) {
    return { big: countdown(props.now - lap.running.startedAt).replace(/^00:/, ""), small: phaseText(lap.running, lap), arc: 1 };
  }
  if (!scheduled(lap) || lap.nextAt === null) {
    const small =
      lap.paused === "failures"
        ? t("laps.paused.failures", { count: Math.max(lap.streak, lap.pauseAfter ?? 0) })
        : lap.paused === "user"
          ? t("laps.paused.user")
          : !lap.on
            ? t("laps.off")
            : lap.when.kind === "refill" && lap.noReading
              ? t("laps.reason.noReading")
              : "";
    return { big: "—", small, arc: 0 };
  }
  const left = lap.nextAt - props.now;
  const span = lap.nextAt - from.value;
  const arc = span > 0 ? Math.min(1, Math.max(0, left / span)) : 0;
  const params = timeParams(lap.nextAt, language.value);
  const today = new Date(lap.nextAt).toDateString() === new Date(props.now).toDateString();
  const small = today ? t("laps.ring.at", params) : t("laps.ring.atDay", { ...params, day: new Date(lap.nextAt).toLocaleDateString(language.value, { weekday: "short" }) });
  return { big: countdown(left), small, arc };
});

/* ---------- İş: what the job is, only what is set ---------- */

/** The script as the gate's line names it: a saved command's line, or the first line written. */
const scriptName = computed(() => {
  const script = props.lap.script;
  if (!script) return "";
  if (script.kind === "text") return script.text.split("\n")[0].trim();
  const project = snapshot.value?.projects.find((p) => p.path === props.lap.place.path);
  const command = project?.commands.find((c) => c.id === script.id);
  return command?.command ?? command?.name ?? script.id;
});

type Fact = { label: string; text: string; script?: boolean };

const facts = computed<Fact[]>(() => {
  const lap = props.lap;
  const list: Fact[] = [
    { label: t("laps.fact.kind"), text: t(KIND_LABELS[lap.kind]) },
    { label: t("laps.fact.where"), text: place.value },
    { label: t("laps.fact.when"), text: whenText(lap.when, agent.value) },
  ];
  const gates: string[] = [];
  if (lap.kind === "scriptAgent" && lap.gate.script === "onFailure") gates.push(t("laps.gate.sum.onFailure"));
  if (lap.kind === "scriptAgent" && lap.gate.script === "onOutput") gates.push(t("laps.gate.sum.onOutput"));
  if (lap.gate.git) gates.push(t("laps.gate.sum.git"));
  if (lap.gate.sameSilent) gates.push(t("laps.gate.sum.same"));
  if (gates.length) list.push({ label: t("laps.fact.gate"), text: gates.join(" · "), script: true });
  const time: string[] = [];
  if (lap.timeoutMin !== null) time.push(t("laps.time.sum.timeout", { minutes: lap.timeoutMin }));
  if (lap.awake) time.push(t(isMac ? "laps.time.sum.awake" : "laps.time.sum.awake.other"));
  if (time.length) list.push({ label: t("laps.fact.time"), text: time.join(" · ") });
  if (withAgent.value && lap.fuel) {
    const key = lap.fuel.then === "wait" ? "laps.fuel.sum.wait" : "laps.fuel.sum.skip";
    list.push({ label: t("laps.fact.fuel"), text: t(agentKey(key, agent.value), { share: shareText(lap.fuel.over) }) });
  }
  return list;
});

/* ---------- Son koşular ---------- */

const runs = ref<LapRun[]>([]);
/** The first line of what the agent last said, by run: read when the job is shown. */
const summaries = ref<Record<string, string>>({});
const asked = new Set<string>();

/** What moves the list: another job, a run that started, ended, was added or was read. */
const runsKey = computed(() => `${props.lap.id}\u0000${props.lap.last.map((mark) => `${mark.id}:${mark.result ?? ""}`).join()}\u0000${props.lap.running?.run ?? ""}\u0000${props.lap.unread}`);

let asking = 0;
watch(
  runsKey,
  async (_key, before) => {
    const id = props.lap.id;
    if (!before || before.split("\u0000")[0] !== id) {
      runs.value = [];
      summaries.value = {};
      asked.clear();
    }
    const ask = ++asking;
    const kept = await api.lapRuns(id).catch(() => null);
    if (ask !== asking || !kept) return;
    runs.value = kept;
    // What the agent said in each run that went well, the ones not read yet.
    const wanted = kept.filter((run) => run.result === "ok" && run.session && run.agent && !asked.has(run.id)).map((run) => run.id);
    if (!wanted.length) return;
    for (const run of wanted) asked.add(run);
    const read = await api.lapSummaries(wanted).catch(() => null);
    if (ask !== asking || !read) return;
    summaries.value = { ...summaries.value, ...read };
  },
  { immediate: true },
);

type RunRow = { run: LapRun; when: string; length: string; text: string };

const rows = computed<RunRow[]>(() =>
  runs.value.map((run) => {
    const end = run.endedAt ?? props.now;
    // A run skipped before it started took no time.
    const ran = run.result !== "skipped" || end - run.startedAt >= 1000;
    let text = "";
    if (run.result === null) text = props.lap.running?.run === run.id ? phaseText(props.lap.running, props.lap) : t("laps.phase.running");
    else text = summaries.value[run.id] ?? reasonText(run, props.lap.gate.script) ?? resultLabel(run.result);
    return { run, when: runMoment(run.due), length: ran ? runLength(end - run.startedAt) : "", text };
  }),
);

/* ---------- the buttons ---------- */

const busy = ref<"run" | "stop" | null>(null);
const error = ref("");

// Another job: what the last one's buttons said goes with it.
watch(
  () => props.lap.id,
  () => {
    error.value = "";
    busy.value = null;
  },
);

async function act(kind: "run" | "stop", call: () => Promise<unknown>): Promise<void> {
  if (busy.value) return;
  busy.value = kind;
  error.value = "";
  try {
    await call();
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    busy.value = null;
  }
}

function runNow(): void {
  void act("run", () => api.lapRunNow(props.lap.id));
}

function stop(): void {
  void act("stop", () => api.lapStop(props.lap.id));
}

const menu = useNativeMenu();

/** ⋯: Bu seferi atla, Duraklat or Sürdür (Aç for one that is off), then İşi sil, which asks. */
function more(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const lap = props.lap;
  const id = lap.id;
  const fail = (reason: unknown): void => void (error.value = refusalText(reason));
  void menu.popup({ clientX: box.left, clientY: box.bottom + 4 }, [
    { text: t("laps.skipNext"), enabled: scheduled(lap) && lap.nextAt !== null && !props.elsewhere, action: () => void api.lapSkipNext(id).catch(fail) },
    lap.paused !== null
      ? { text: t("laps.resume"), action: () => void api.lapPause(id, false).catch(fail) }
      : !lap.on
        ? { text: t("laps.switchOn"), action: () => void api.lapSwitch(id, true).catch(fail) }
        : { text: t("laps.pause"), action: () => void api.lapPause(id, true).catch(fail) },
    "separator",
    { text: t("laps.delete"), action: () => emit("remove") },
  ]);
}
</script>

<template>
  <section class="detail" :aria-label="lap.name">
    <div class="head">
      <div class="head-text">
        <h2 class="name">{{ lap.name }}</h2>
        <span class="meta">
          <template v-if="withAgent">
            <span class="meta-agent"><AgentMark :agent="agent" :size="11" />{{ agentName(agent) }}<template v-if="model"> · {{ model }}</template></span>
            <span class="faint">·</span>
          </template>
          <span class="meta-place" :title="lap.folder">{{ place }}</span>
        </span>
        <div class="buttons">
          <button v-if="lap.running" type="button" class="control small stop" :disabled="elsewhere || busy !== null" :aria-busy="busy === 'stop'" @click="stop">
            <span :class="['label-row', { hidden: busy === 'stop' }]"><Icon name="stop" :size="11" />{{ t("common.stop") }}</span>
            <Spinner v-if="busy === 'stop'" class="over" :size="12" />
          </button>
          <button v-else type="button" :class="['control', 'small', withAgent ? 'agent' : 'primary']" :disabled="elsewhere || busy !== null" :aria-busy="busy === 'run'" @click="runNow">
            <span :class="['label-row', { hidden: busy === 'run' }]"><Icon name="play" :size="11" />{{ t("laps.runNow") }}</span>
            <Spinner v-if="busy === 'run'" class="over" :size="12" />
          </button>
          <button type="button" class="control small" @click="emit('edit')"><Icon name="pencil" :size="12" />{{ t("common.edit") }}</button>
          <button type="button" class="icon-button boxed" aria-haspopup="menu" :aria-label="t('laps.more')" :title="t('laps.more')" @click="more">
            <Icon name="more" :size="14" />
          </button>
        </div>
        <p v-if="error" class="error" role="alert">{{ error }}</p>
      </div>
      <div class="ring" role="img" :aria-label="`${ring.big} · ${ring.small}`">
        <svg width="160" height="160" viewBox="0 0 160 160" aria-hidden="true">
          <circle cx="80" cy="80" :r="R" fill="none" class="ring-track" stroke-width="7" />
          <circle
            v-if="ring.arc > 0"
            cx="80"
            cy="80"
            :r="R"
            fill="none"
            :class="['ring-arc', { live: lap.running }]"
            stroke-width="7"
            stroke-linecap="round"
            :stroke-dasharray="`${(CIRCLE * ring.arc).toFixed(1)} ${CIRCLE.toFixed(1)}`"
            transform="rotate(-90 80 80)"
          />
        </svg>
        <div class="ring-text">
          <span :class="['ring-big', { long: ring.big.length > 5 }]">{{ ring.big }}</span>
          <span class="ring-small">{{ ring.small }}</span>
        </div>
      </div>
    </div>

    <div class="rule"></div>

    <div class="facts">
      <span class="section-label part">{{ t("laps.fact.title") }}</span>
      <div v-for="fact in facts" :key="fact.label" class="fact">
        <span class="fact-label">{{ fact.label }}</span>
        <span class="fact-text">
          <Rich v-if="fact.script" :text="fact.text">
            <template #script><code class="mono">{{ scriptName }}</code></template>
          </Rich>
          <template v-else>{{ fact.text }}</template>
        </span>
      </div>
    </div>

    <div class="rule"></div>

    <div class="runs">
      <span class="section-label part">{{ t("laps.runs.title") }}</span>
      <p v-if="!rows.length" class="hint">{{ t("laps.runs.none") }}</p>
      <button
        v-for="row in rows"
        :key="row.run.id"
        type="button"
        class="run"
        :disabled="row.run.result === null && lap.running?.run !== row.run.id"
        :aria-label="row.run.unread ? `${row.when} · ${row.text} · ${t('laps.runs.unread')}` : `${row.when} · ${row.text}`"
        @click="emit('openRun', row.run.id)"
      >
        <LapMark :result="row.run.result" />
        <span class="run-when">{{ row.when }}</span>
        <span class="run-length">{{ row.length }}</span>
        <span class="run-text">{{ row.text }}</span>
        <span v-if="row.run.unread" class="run-unread" :title="t('laps.runs.unread')" aria-hidden="true"></span>
        <!-- The run that goes opens on its terminal. -->
        <span class="run-go"><span class="run-see">{{ t(row.run.result === null ? "laps.runs.terminal" : "laps.runs.see") }}</span><Icon name="chevron" :size="12" /></span>
      </button>
    </div>
  </section>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.detail {
  flex: 1 1 420px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px 20px 16px 18px;
  border-left: 1px solid var(--line-soft);
  overflow-y: auto;
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
}

.head-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.name {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-strong);
  overflow-wrap: anywhere;
}

.meta {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-muted);
}

.meta-agent {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  white-space: nowrap;
}

.meta-place {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.faint {
  color: var(--text-faint);
}

.buttons {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-top: 4px;
}

.label-row {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.ring {
  position: relative;
  flex-shrink: 0;
  width: 160px;
  height: 160px;
}

.ring-track {
  stroke: var(--line);
}

.ring-arc {
  stroke: var(--claude);
}

.ring-arc.live {
  animation: breathe 1.6s ease-in-out infinite;
}

.ring-text {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  padding: 0 18px;
  text-align: center;
}

.ring-big {
  font-family: var(--font-mono);
  font-size: 25px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  color: var(--text-strong);
}

/* Hours and seconds (`03:27:59`), or days: smaller, so that it stays clear of the ring. */
.ring-big.long {
  font-size: 21px;
}

.ring-small {
  font-size: 11.5px;
  color: var(--text-faint);
}

.rule {
  flex-shrink: 0;
  height: 1px;
  background: var(--line-soft);
}

.part {
  display: block;
  padding: 2px 0 6px;
}

.facts {
  display: flex;
  flex-direction: column;
}

.fact {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 27px;
  font-size: 12.5px;
}

.fact-label {
  width: 92px;
  flex-shrink: 0;
  color: var(--text-faint);
}

.fact-text {
  min-width: 0;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fact-text code {
  font-size: 11.5px;
}

.runs {
  display: flex;
  flex-direction: column;
}

.run {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 30px;
  margin: 0 -8px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  text-align: left;
}

.run:hover:not(:disabled) {
  background: var(--bg-row-hover);
}

.run:disabled {
  cursor: default;
}

.run-when {
  width: 84px;
  flex-shrink: 0;
  font-size: 12.5px;
  color: var(--text);
}

.run-length {
  width: 80px;
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-faint);
}

.run-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-faint);
}

/* A result not opened yet, right after its words: the laps' orange. */
.run-unread {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--claude);
}

.run-go {
  flex-shrink: 0;
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11.5px;
  color: var(--claude-idle);
}

.run-see {
  display: none;
}

.run:hover:not(:disabled) .run-go,
.run:focus-visible .run-go {
  color: var(--text-soft);
}

.run:hover:not(:disabled) .run-see,
.run:focus-visible .run-see {
  display: inline;
}

.run:disabled .run-go {
  visibility: hidden;
}

@keyframes breathe {
  50% {
    opacity: 0.45;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ring-arc.live {
    animation: none;
  }
}
</style>
