<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import Icon from "../../components/Icon.vue";
import Markdown from "../../components/Markdown.vue";
import Spinner from "../../components/Spinner.vue";
import { agentName } from "../../lib/agents";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { agentKey } from "../../lib/format";
import { t } from "../../lib/i18n";
import { hasAgent, lapAgent, phaseText, reasonText, refusalText, resultLabel, runDay, runLength, type LapResult, type LapTerminal, type LapView } from "../../lib/laps";
import { closeChord, terminalChord } from "../../lib/platform";
import { codexModelLabel, modelLabel } from "../../lib/start";
import { api, now } from "../../lib/store";
import { attach, fitView, measure } from "../../lib/terminals";
import TerminalFind from "../TerminalFind.vue";
import LapMark from "./LapMark.vue";

/**
 * One run's dialog (TN3, TND6), in two bodies. Sonuç: its result, read when it opens and held here
 * only: what the agent last said (read from its session's own log), the script's kept output, or
 * why the agent wasn't called; Kopyala copies the part shown, Yeniden çalıştır runs a script job
 * again now. Terminal: the run's own terminal (core: `lap_terminal`), a tab of the laps' that no
 * panel shows. A run that goes opens on it, its head saying what it is at; a run that has ended
 * comes to it through Oturumu aç, its session going on there, and from then on its head switches
 * between the two (Sonuç | Terminal). The dialog's height follows the body, smoothly. Inside the
 * terminal Esc is the terminal's; ⌘W, Kapat or a click outside closes the dialog, and the core
 * closes the tab once its agent has left (`lap_terminal_left`).
 */
const props = defineProps<{ run: string; lap: LapView | null; elsewhere: boolean }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const closeButton = ref<HTMLButtonElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();

/** The run goes: the dialog shows its terminal, its head what it is at. */
const going = computed(() => props.lap?.running?.run === props.run);
const running = computed(() => (going.value ? (props.lap?.running ?? null) : null));

const result = ref<LapResult | null>(null);
const error = ref("");

type Body = "result" | "terminal";
/** A run that goes opens on its terminal, at the terminal's height. */
const body = ref<Body>(going.value ? "terminal" : "result");

function loadResult(): void {
  api
    .lapResult(props.run)
    .then((read) => {
      result.value = read;
      part.value = read.run.agent && !gated.value ? "said" : "script";
      settle();
    })
    .catch((reason) => (error.value = refusalText(reason)));
}

onMounted(() => {
  openUnfocused(dialog.value, () => closeButton.value);
  if (going.value) void askTerminal();
  else loadResult();
});

// The run ended while its dialog was open: its result, the terminal staying in front.
watch(going, (on, was) => {
  if (was && !on) loadResult();
});

/** How long the run that goes has run, by the second. */
const clock = ref(Date.now());
let ticker: number | undefined;
watch(
  going,
  (on) => {
    window.clearInterval(ticker);
    ticker = on ? window.setInterval(() => (clock.value = Date.now()), 1000) : undefined;
    clock.value = Date.now();
  },
  { immediate: true },
);

const run = computed(() => result.value?.run ?? null);
const agent = computed(() => {
  if (going.value) return props.lap && hasAgent(props.lap.kind) ? lapAgent(props.lap) : null;
  return run.value?.agent ?? null;
});

/** The agent wasn't called: the script's gate held it back. */
const gated = computed(() => run.value?.reason === "gate");

/** Of a script job, or a gated run: the script is all there is. */
const scriptOnly = computed(() => !!run.value && (run.value.agent === null || gated.value));

/** The result's parts: what the agent said, the script's output. */
type Part = "said" | "script";
const part = ref<Part>("said");

const lines = computed(() => result.value?.script ?? null);
const output = computed(() => (lines.value ?? []).join("\n"));
/** The command line the script ran, as the output's first line says it (`$ …`). */
const command = computed(() => (result.value?.command ? result.value.command.split("\n").map((line) => `$ ${line}`).join("\n") : null));
const said = computed(() => result.value?.message ?? "");

/** The parts over the result, for an agent's run that had a script before it. */
const parts = computed<Part[]>(() => (run.value?.agent && !gated.value && lines.value !== null ? ["said", "script"] : []));

const shown = computed<Part>(() => (scriptOnly.value ? "script" : part.value));

/** What the head says under the name: when (or, while it goes, what it is at and for how long),
 * how long, the agent and model, where. */
const model = computed(() => {
  const start = props.lap?.start;
  if (!start || !agent.value) return null;
  if (agent.value === "codex") return start.codex?.model ? codexModelLabel(start.codex.model) : null;
  return start.model ? modelLabel(start.model) : null;
});

const state = computed(() => (running.value && props.lap ? phaseText(running.value, props.lap) : null));
const when = computed(() => (run.value ? runDay(run.value.due, now.value) : ""));
const length = computed(() => {
  if (running.value) return runLength(clock.value - running.value.startedAt);
  const r = run.value;
  if (!r || r.endedAt === null || (r.result === "skipped" && r.endedAt - r.startedAt < 1000)) return null;
  return runLength(r.endedAt - r.startedAt);
});
const place = computed(() => (props.lap ? (props.lap.project ?? props.lap.folder) : null));
/** Under the output, as the board's output ends: how the script ended and how long it took. */
const tail = computed(() => (code.value !== null && length.value ? t("laps.result.tail", { code: code.value, length: length.value }) : null));

/** The script printed nothing (or only blank lines). */
const blank = computed(() => !lines.value || lines.value.every((line) => !line.trim()));

/** Why the agent wasn't called: the script did well (exit 0), or printed nothing. */
const gateLine = computed(() => {
  const r = run.value;
  if (!r || !gated.value) return null;
  const empty = blank.value && (r.code !== 0 || props.lap?.gate.script !== "onFailure");
  return empty ? t("laps.result.gate.empty") : t("laps.result.gate.passed", { code: r.code ?? 0 });
});

/** A skipped run, or one that didn't do well: why, in Pitwall's words (a script's exit code is
 * its pill's). */
const why = computed(() => {
  const r = run.value;
  if (!r || gated.value || r.result === "ok" || r.result === null) return null;
  if (r.reason === "exit" && scriptOnly.value) return null;
  return reasonText(r, props.lap?.gate.script ?? "none");
});

/** A script's exit code as the pill says it, for a run that had one. */
const code = computed(() => (run.value?.script && run.value.code !== null ? run.value.code : null));

const foot = computed(() => {
  const r = run.value;
  if (!r) return null;
  if (r.file) return { file: true, text: r.file };
  if (r.script && props.lap) return { file: false, text: t("laps.result.kept", { count: props.lap.keep }) };
  return null;
});

/* ---------- the terminal ---------- */

/** What the core last said of the run's terminal. */
const term = ref<LapTerminal | null>(null);
/** The tab shown, while its shell lives. */
const tabId = ref<number | null>(null);
/** The tab's shell ended while it was shown here. */
const shellGone = ref(false);
const asking = ref(false);

/** Sonuç | Terminal, over the bodies: a run that has ended, with its terminal open. */
const switchShown = computed(() => tabId.value !== null && !going.value && result.value !== null);

/** What the terminal's body says while it has no terminal to show; none: a spinner. */
const quiet = computed(() => {
  if (shellGone.value) return t("laps.terminal.closed");
  const kind = term.value?.kind;
  if (kind === "background") return t("laps.terminal.background");
  if (kind !== "notYet" || !running.value || !props.lap) return null;
  // A script-only job has no agent to wait for: its output shows here once it has run.
  if (props.lap.kind === "script") return running.value.phase === "script" ? t("laps.terminal.scriptOnly") : phaseText(running.value, props.lap);
  if (running.value.phase === "script") return t("laps.terminal.script");
  // Its tab is about to open.
  if (running.value.phase === "agent") return null;
  return t("laps.terminal.notYet", { phase: phaseText(running.value, props.lap) });
});

let asked = 0;

/** The run's terminal, as the core has it now (opened again for a run that has ended): whether
 * there is one to show. One that runs elsewhere was brought up there, and the dialog closes. */
async function askTerminal(): Promise<boolean> {
  const ask = ++asked;
  asking.value = true;
  error.value = "";
  try {
    const got = await api.lapTerminal(props.run, terminalSize());
    if (ask !== asked) return false;
    term.value = got;
    if (got.kind === "elsewhere") dialog.value?.close();
    if (got.kind !== "tab") return false;
    shellGone.value = false;
    tabId.value = got.id;
    return true;
  } catch (reason) {
    if (ask === asked) error.value = refusalText(reason);
    return false;
  } finally {
    if (ask === asked) asking.value = false;
  }
}

// A run that goes, with no terminal yet (its script, a wait, an agent in the background): asked
// again as it moves on, so that it comes up once its agent has its tab.
watch(
  () => `${running.value?.phase ?? ""}\u0000${running.value?.tab ?? ""}`,
  () => {
    if (going.value && tabId.value === null && body.value === "terminal") void askTerminal();
  },
);

/** Sonuç once the run has ended and there is no terminal to show (its agent ran in the
 * background, or its shell ended). */
function settle(): void {
  if (body.value === "terminal" && tabId.value === null && !going.value && result.value) void switchTo("result");
}

let unlistenExit: UnlistenFn | null = null;
let unmounted = false;
void listen<number>("terminal-exit", (event) => {
  if (event.payload !== tabId.value) return;
  tabId.value = null;
  shellGone.value = true;
  settle();
}).then((unlisten) => (unmounted ? unlisten() : (unlistenExit = unlisten)));

/** The size a new tab gets: the terminal body's, as the dialog will hold it, measured off-screen. */
function terminalSize(): { cols: number; rows: number } | null {
  const el = dialog.value;
  if (!el) return null;
  const head = el.querySelector<HTMLElement>(".dialog-head")?.offsetHeight ?? 54;
  const footing = el.querySelector<HTMLElement>(".dialog-foot")?.offsetHeight ?? 58;
  const tall = Math.min(TALL, window.innerHeight - 64);
  // The body's padding, the screen's border and padding (the styles below).
  const width = el.clientWidth - 2 * 16 - 2 - 14;
  const height = tall - 2 - head - footing - 2 * 12 - 2 - 16;
  const probe = document.createElement("div");
  probe.style.cssText = `position: fixed; left: -10000px; top: 0; width: ${Math.max(width, 80)}px; height: ${Math.max(height, 40)}px; visibility: hidden`;
  document.body.appendChild(probe);
  try {
    return measure(probe);
  } catch {
    return null;
  } finally {
    probe.remove();
  }
}

/** The terminal body's height (the style's `.tall`), at most. */
const TALL = 680;

const screen = ref<HTMLElement | null>(null);
const host = ref<HTMLElement | null>(null);

/** The dialog's height moves between the bodies' (`switchTo`): the terminal follows it frame by
 * frame, the shell hears its size once it settles. */
const moving = ref(false);

// The terminal into its body, the keyboard with it.
watch(
  [host, tabId],
  ([el, id]) => {
    if (el && id !== null) attach(id, el, !moving.value, true);
  },
  { flush: "post" },
);

const observer = new ResizeObserver(() => {
  if (tabId.value !== null) fitView(tabId.value, !moving.value);
});
watch(
  host,
  (el, old) => {
    if (old) observer.unobserve(old);
    if (el) observer.observe(el);
  },
  { flush: "post" },
);
watch(moving, (on) => {
  if (!on && tabId.value !== null) fitView(tabId.value);
});

const MOVE_MS = 200;
const still = (): boolean => matchMedia("(prefers-reduced-motion: reduce)").matches;
/** Ends the move under way, the dialog at the height of its body. */
let moved: (() => void) | null = null;

/** Shows `next`, the dialog's height going from the old body's to the new one's (measured, the
 * old one set, the new one the next frame, cleared once there): no jump, and no frame of the new
 * body at the old size it isn't drawn for. None under reduced motion. */
async function switchTo(next: Body): Promise<void> {
  if (body.value === next) return;
  const el = dialog.value;
  const from = el?.getBoundingClientRect().height ?? 0;
  moved?.();
  if (!el || !el.open || still()) {
    body.value = next;
    return;
  }
  moving.value = true;
  body.value = next;
  await nextTick();
  const to = el.getBoundingClientRect().height;
  if (!el.isConnected || Math.abs(to - from) < 1) {
    moving.value = false;
    return;
  }
  el.style.height = `${from}px`;
  void el.offsetHeight;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const onEnd = (event: TransitionEvent): void => {
    if (event.target === el && event.propertyName === "height") done();
  };
  const done = (): void => {
    if (moved !== done) return;
    moved = null;
    clearTimeout(timer);
    el.removeEventListener("transitionend", onEnd);
    el.style.transition = "";
    el.style.height = "";
    moving.value = false;
  };
  moved = done;
  el.addEventListener("transitionend", onEnd);
  timer = setTimeout(done, MOVE_MS + 150);
  requestAnimationFrame(() => {
    if (moved !== done) return;
    el.style.transition = `height ${MOVE_MS}ms ease`;
    el.style.height = `${to}px`;
  });
}

/* ---------- the keys ---------- */

/** ⌘W closes the dialog and nothing behind it (the window's Close Window, WindowApp: `closeFront`);
 * ⌘T opens no tab in the panel while it is up. */
function onKey(event: KeyboardEvent): void {
  if (closeChord(event)) {
    event.preventDefault();
    if (!event.repeat) dialog.value?.close();
  } else if (terminalChord(event) && event.code === "KeyT") {
    event.preventDefault();
    event.stopPropagation();
  }
}

/** Esc is the terminal's (Claude's interrupt) while the terminal, or its search, has the keyboard. */
function onCancel(event: Event): void {
  if (screen.value?.contains(document.activeElement)) event.preventDefault();
}

/* ---------- the buttons ---------- */

const copied = ref(false);
let copiedTimer: ReturnType<typeof setTimeout> | undefined;

const copyText = computed(() => (shown.value === "said" ? said.value : output.value));

function copy(): void {
  if (!copyText.value) return;
  void api.copyText(copyText.value).then(() => {
    copied.value = true;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied.value = false), 1200);
  });
}

onBeforeUnmount(() => {
  unmounted = true;
  clearTimeout(copiedTimer);
  window.clearInterval(ticker);
  observer.disconnect();
  moved?.();
  unlistenExit?.();
  // The dialog let go of its terminal: the core closes it once its agent has left.
  if (tabId.value !== null) void api.lapTerminalLeft(tabId.value);
});

const busy = ref<"open" | "again" | "stop" | null>(null);

/** Oturumu aç: the run's session in the dialog's terminal, its tab if it runs in one of the
 * laps', else opened again. */
async function openSession(): Promise<void> {
  if (busy.value || !run.value) return;
  busy.value = "open";
  try {
    if (await askTerminal()) await switchTo("terminal");
  } finally {
    busy.value = null;
  }
}

/** The script job once more, now. */
async function again(): Promise<void> {
  if (busy.value || !run.value) return;
  busy.value = "again";
  error.value = "";
  try {
    await api.lapRunNow(run.value.lap);
    dialog.value?.close();
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    busy.value = null;
  }
}

/** Durdur, as the job's own button stops it. */
async function stop(): Promise<void> {
  if (busy.value || !props.lap) return;
  busy.value = "stop";
  error.value = "";
  try {
    await api.lapStop(props.lap.id);
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    busy.value = null;
  }
}

const canOpen = computed(() => !!run.value?.session && !!run.value.agent && !gated.value && tabId.value === null);
const canAgain = computed(() => !!props.lap && props.lap.kind === "script" && !props.lap.running);
</script>

<template>
  <dialog
    ref="dialog"
    :class="['dialog', 'result', { tall: body === 'terminal' }]"
    :aria-label="lap?.name ?? t('laps.title')"
    @close="emit('close')"
    @cancel="onCancel"
    @keydown="onKey"
    @pointerdown="backdrop.down"
    @click="backdrop.click"
  >
    <div class="dialog-head">
      <LapMark v-if="going" :result="null" :size="20" />
      <LapMark v-else-if="run" :result="run.result" :size="20" />
      <div class="head-text">
        <span class="dialog-title name">{{ lap?.name ?? resultLabel(run?.result ?? null) }}</span>
        <span v-if="going || run" class="meta">
          <span v-if="state" class="state">{{ state }}</span>
          <span v-else>{{ when }}</span>
          <template v-if="length"><span class="faint">·</span><span>{{ length }}</span></template>
          <template v-if="agent && !gated">
            <span class="faint">·</span>
            <span class="meta-part"><AgentMark :agent="agent" :size="11" />{{ agentName(agent) }}<template v-if="model"> · {{ model }}</template></span>
          </template>
          <template v-else-if="run?.script">
            <span class="faint">·</span>
            <span class="meta-part"><Icon name="terminal" :size="12" />{{ t("laps.edit.script") }}</span>
          </template>
          <template v-if="place">
            <span class="faint">·</span>
            <span class="meta-part ell"><Icon v-if="lap && !lap.project" name="folder" :size="12" />{{ place }}</span>
          </template>
        </span>
      </div>
      <span class="grow"></span>
      <div v-if="switchShown" class="seg small" role="tablist">
        <button type="button" role="tab" :aria-selected="body === 'result'" :class="{ on: body === 'result' }" @click="switchTo('result')">{{ t("laps.result.tabResult") }}</button>
        <button type="button" role="tab" :aria-selected="body === 'terminal'" :class="{ on: body === 'terminal' }" @click="switchTo('terminal')">
          <Icon name="terminal" :size="12" />{{ t("laps.result.tabTerminal") }}
        </button>
      </div>
      <button type="button" class="icon-button" :aria-label="t('common.close')" :title="t('common.close')" @click="dialog?.close()">
        <Icon name="close" :size="14" />
      </button>
    </div>

    <div v-if="body === 'result' && parts.length" class="tabs">
      <div class="seg small" role="tablist">
        <button
          v-for="one in parts"
          :key="one"
          type="button"
          role="tab"
          :aria-selected="part === one"
          :class="{ on: part === one }"
          @click="part = one"
        >
          <AgentMark v-if="one === 'said' && agent" :agent="agent" :size="11" />
          <Icon v-else name="terminal" :size="12" />
          {{ one === "said" && agent ? t(agentKey("laps.result.said", agent)) : t("laps.result.script") }}
        </button>
      </div>
    </div>

    <div v-if="body === 'result'" class="body">
      <p v-if="!result && !error" class="loading"><Spinner :size="13" /></p>

      <template v-else-if="result && run">
        <!-- Why the agent wasn't called, or why the run didn't go on. -->
        <p v-if="gateLine" class="why">{{ gateLine }}</p>
        <p v-else-if="why" class="why">{{ why }}</p>

        <!-- What the agent last said. -->
        <template v-if="shown === 'said'">
          <p v-if="result.gone" class="hint">{{ t("laps.result.gone") }}</p>
          <div v-else-if="said" class="said">
            <Markdown :text="said" />
          </div>
          <p v-else-if="!why" class="hint">{{ t(agentKey("laps.result.saidNone", agent ?? "claude")) }}</p>
        </template>

        <!-- The script's kept output. -->
        <template v-else>
          <div v-if="code !== null && !gated" class="code-line">
            <span :class="['pill', { bad: code !== 0 }]">{{ t("laps.result.exitCode", { code }) }}</span>
            <span class="hint">{{ t("laps.result.lines") }}</span>
          </div>
          <pre v-if="lines !== null && (!blank || command)" class="output selectable"><span v-if="command" class="command">{{ command + (blank ? "" : "\n") }}</span><template v-if="!blank">{{ output }}</template><span v-if="tail" class="tail">{{ "\n\n" + tail }}</span></pre>
          <p v-if="lines !== null && blank && run.script" class="hint">{{ t("laps.result.noOutput") }}</p>
          <p v-else-if="lines === null && run.script" class="hint">{{ t("laps.result.outputGone") }}</p>
        </template>
      </template>

      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </div>

    <!-- The run's terminal, the laps' own tab: in no panel, here alone. -->
    <div v-else class="body term-body">
      <div v-if="tabId !== null" ref="screen" class="screen">
        <div ref="host" class="viewport"></div>
        <TerminalFind :key="tabId" :id="tabId" />
      </div>
      <p v-else-if="quiet" class="quiet">{{ quiet }}</p>
      <p v-else-if="!error" class="loading"><Spinner :size="13" /></p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </div>

    <div class="dialog-foot">
      <span v-if="body === 'result' && foot" class="foot-text grow">
        <Icon v-if="foot.file" name="file" :size="12" />
        <span :class="['ell', { selectable: foot.file }]" :title="foot.file ? foot.text : undefined">{{ foot.text }}</span>
      </span>
      <span v-else class="grow"></span>
      <template v-if="body === 'result'">
        <button type="button" class="control small" :disabled="!copyText" @click="copy">
          <Icon :name="copied ? 'check' : 'copy'" :size="12" />{{ t("laps.result.copy") }}
        </button>
        <button v-if="canOpen" type="button" class="control small agent" :disabled="busy !== null" :aria-busy="busy === 'open'" @click="openSession">
          <span :class="['label-row', { hidden: busy === 'open' }]"><Icon name="terminal" :size="12" />{{ t("laps.result.open") }}</span>
          <Spinner v-if="busy === 'open'" class="over" :size="12" />
        </button>
        <button v-if="canAgain" type="button" class="control small" :disabled="busy !== null || elsewhere" :aria-busy="busy === 'again'" @click="again">
          <span :class="['label-row', { hidden: busy === 'again' }]"><Icon name="restart" :size="12" />{{ t("laps.result.again") }}</span>
          <Spinner v-if="busy === 'again'" class="over" :size="12" />
        </button>
      </template>
      <button v-else-if="going" type="button" class="control small stop" :disabled="elsewhere || busy !== null" :aria-busy="busy === 'stop'" @click="stop">
        <span :class="['label-row', { hidden: busy === 'stop' }]"><Icon name="stop" :size="11" />{{ t("common.stop") }}</span>
        <Spinner v-if="busy === 'stop'" class="over" :size="12" />
      </button>
      <button ref="closeButton" type="button" class="control small primary" @click="dialog?.close()">{{ t("common.close") }}</button>
    </div>
  </dialog>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.result {
  width: 840px;
  height: min(520px, calc(100vh - 32px));
}

/* The terminal's body: as wide, taller (`TALL` above). */
.result.tall {
  height: min(680px, calc(100vh - 64px));
}

.head-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.name {
  font-size: 14.5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

.meta-part {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}

.faint {
  color: var(--text-faint);
}

.ell {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tabs {
  flex-shrink: 0;
  padding: 12px 20px 0;
}

.body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px 20px;
  overflow: hidden;
}

.loading {
  margin: 0;
  display: flex;
  justify-content: center;
  padding-top: 40px;
  color: var(--text-faint);
}

.why {
  margin: 0;
  font-size: 12.5px;
  color: var(--text);
}

.said {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 14px 18px 8px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--bg-well);
  font-size: 13px;
  line-height: 1.6;
}

.code-line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.output .tail,
.output .command {
  color: var(--text-faint);
}
.output {
  flex: 1 1 auto;
  min-height: 0;
  margin: 0;
  overflow: auto;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--bg-log);
  color: var(--text-log);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.65;
  white-space: pre;
}

.foot-text {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11.5px;
  color: var(--text-faint);
}

.label-row {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

/* What a run that goes is at, in the agent's colour as the tower says it. */
.state {
  color: var(--claude-text);
}

.term-body {
  padding: 12px 16px;
}

/* The terminal, as the panel draws one (TerminalPanel: `.screen`, `.viewport`). */
.screen {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  display: flex;
  padding: 8px 4px 8px 10px;
  border: 1px solid var(--line-inset);
  border-radius: 8px;
  background: var(--bg-log);
  overflow: clip;
}

.viewport {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow: clip;
}

.viewport :deep(.terminal-host),
.viewport :deep(.xterm) {
  height: 100%;
}

/* xterm paints its scroll area black; below the last row it would show as a strip. */
.viewport :deep(.xterm-viewport) {
  background-color: var(--bg-log) !important;
}

/* No terminal to show: why, quietly, where it would be. */
.quiet {
  flex: 1 1 auto;
  margin: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 60px 40px;
  text-align: center;
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-muted);
}
</style>
