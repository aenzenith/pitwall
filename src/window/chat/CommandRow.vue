<script setup lang="ts">
import { computed, ref } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import type { CommandEntry } from "../../lib/chat/store";
import { exitMeta } from "../../lib/chat/tools";
import type { ChatAnswer, ChatPending } from "../../lib/chat/wire";
import { language, t } from "../../lib/i18n";
import { api } from "../../lib/store";
import ChatCard from "./ChatCard.vue";

/**
 * A row of Komutlar (TS3): `$ command` in mono, at its right the exit code and how long it ran, or
 * "izin bekliyor" while its card waits (the card's buttons then in the row too, thin). Opened: its
 * description, its output (stderr in red), and what can be done to it: sent to the background
 * after two seconds, stopped there, its latest output read. `bare`: its opened part alone, under
 * the command's own row in the conversation, which stands for its line.
 */
const props = defineProps<{
  id: number;
  path: string;
  command: CommandEntry;
  open: boolean;
  now: number;
  /** The card waiting for this call, if any. */
  pending: ChatPending | null;
  bare?: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
  long: [title: string, text: string];
  answer: [pending: ChatPending, answer: ChatAnswer];
}>();

const running = computed(() => props.command.state === "running");
const waiting = computed(() => !!props.pending);

function seconds(ms: number): string {
  const value = ms / 1000;
  const shown = new Intl.NumberFormat(language.value, { maximumFractionDigits: value < 10 ? 1 : 0 }).format(value);
  return t("chat.time.short", { value: shown });
}

const meta = computed(() => {
  if (waiting.value) return t("chat.command.waiting");
  const parts: string[] = [];
  const exit = exitMeta(props.command);
  if (exit) parts.push(exit);
  const { started, ended } = props.command;
  if (started && ended) parts.push(seconds(ended - started));
  else if (started && running.value) parts.push(seconds(Math.max(0, props.now - started)));
  return parts.join(" · ");
});

/** The output as kept, the last lines of a long one (the whole in the viewer). */
const OUTPUT_LINES = 200;
const fetched = ref<string | null>(null);
const output = computed(() => fetched.value ?? props.command.stdout);
const outputCut = computed(() => {
  const lines = output.value.replace(/\n$/, "").split("\n");
  return lines.length > OUTPUT_LINES ? lines.slice(-OUTPUT_LINES).join("\n") : output.value.replace(/\n$/, "");
});
const outputLong = computed(() => output.value.split("\n").length > OUTPUT_LINES);

/** Opened under its row, the command in full when the row shows what it is for instead, or cuts it. */
const whole = computed(() => !!props.command.description || props.command.command.length > 60 || props.command.command.includes("\n"));

const busy = ref<"background" | "stop" | "output" | null>(null);

/** A foreground call that has run two seconds can go to the background (one waiting for its
 * permission hasn't started). */
const canBackground = computed(
  () => running.value && !waiting.value && !!props.command.tool && !props.command.background && !!props.command.started && props.now - props.command.started >= 2000,
);
const canStop = computed(() => running.value && (!!props.command.background || !!props.command.run));

async function toBackground(): Promise<void> {
  if (!props.command.tool || busy.value) return;
  busy.value = "background";
  try {
    await api.chatControl(props.id, { kind: "backgroundTasks", toolUse: props.command.tool });
  } catch {
    // The CLI answered with nothing to do: the row stays as it is.
  } finally {
    busy.value = null;
  }
}

async function stop(): Promise<void> {
  if (busy.value) return;
  busy.value = "stop";
  try {
    if (props.command.run) await api.chatShellStop(props.id, props.command.run);
    else if (props.command.background) await api.chatControl(props.id, { kind: "stopTask", task: props.command.background });
  } catch {
    // Gone already.
  } finally {
    busy.value = null;
  }
}

async function latest(): Promise<void> {
  if (!props.command.background || busy.value) return;
  busy.value = "output";
  try {
    const reply = await api.chatControl<Record<string, unknown>>(props.id, { kind: "taskOutput", task: props.command.background });
    const text = [reply?.output, reply?.stdout, reply?.content].find((each) => typeof each === "string");
    if (typeof text === "string") fetched.value = text;
  } catch {
    // Nothing more to show.
  } finally {
    busy.value = null;
  }
}
</script>

<template>
  <div :class="['command-row', { open, waiting, bare, failed: command.state === 'failed' }]">
    <button v-if="!bare" type="button" class="line" :aria-expanded="open" @click="emit('toggle')">
      <span class="state" aria-hidden="true">
        <span v-if="waiting" class="ring"></span>
        <Spinner v-else-if="running" :size="10" />
        <span v-else-if="command.state === 'failed'" class="dot bad"></span>
      </span>
      <span class="cmd" data-find><span class="sigil">{{ command.run ? "!" : "$" }}</span> {{ command.command }}</span>
      <span :class="['meta', { warn: waiting, bad: command.state === 'failed' }]">{{ meta }}</span>
    </button>
    <Collapse :show="bare || open">
      <div class="body">
        <div v-if="bare && whole" class="full selectable" data-find><span class="sigil">{{ command.run ? "!" : "$" }}</span> {{ command.command }}</div>
        <div v-if="pending" class="card" @click.stop>
          <ChatCard :id="id" :path="path" :pending="pending" :more="0" :compact="true" @answer="(answer) => emit('answer', pending!, answer)" />
        </div>
        <div v-if="command.description && !bare" class="desc" data-find>{{ command.description }}</div>
        <pre v-if="outputCut" class="out selectable" data-find>{{ outputCut }}</pre>
        <pre v-if="command.stderr" class="out err selectable" data-find>{{ command.stderr.replace(/\n$/, "") }}</pre>
        <div v-if="!outputCut && !command.stderr && !running && !waiting" class="none">{{ t("chat.command.noOutput") }}</div>
        <div v-if="outputLong || canBackground || canStop || (command.background && running)" class="tools">
          <button v-if="outputLong" type="button" class="control" @click="emit('long', command.command, output)">{{ t("chat.showAll") }}</button>
          <button v-if="canBackground" type="button" class="control" :disabled="!!busy" @click="toBackground">
            <Spinner v-if="busy === 'background'" :size="10" />{{ t("chat.command.toBackground") }}
          </button>
          <button v-if="command.background && running" type="button" class="control" :disabled="!!busy" @click="latest">
            <Spinner v-if="busy === 'output'" :size="10" /><Icon v-else name="restart" :size="11" />{{ t("chat.command.latest") }}
          </button>
          <button v-if="canStop" type="button" class="control" :disabled="!!busy" @click="stop">
            <Spinner v-if="busy === 'stop'" :size="10" /><Icon v-else name="stop" :size="11" />{{ t("chat.command.stop") }}
          </button>
        </div>
      </div>
    </Collapse>
  </div>
</template>

<style scoped>
.command-row {
  border-bottom: 1px solid var(--line-soft);
}

.command-row.open {
  background: var(--bg-panel);
}

/* Under its row in the conversation, in that row's box: no rule, no ground of its own. */
.command-row.bare {
  border-bottom: 0;
  background: none;
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-width: 0;
  height: 30px;
  padding: 0 12px 0 8px;
  border: 0;
  background: none;
  color: var(--text);
  font: inherit;
  text-align: left;
}

.line:hover {
  background: var(--bg-row-hover);
}

.line:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.state {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 12px;
  flex-shrink: 0;
  color: var(--claude);
}

.ring {
  width: 8px;
  height: 8px;
  box-sizing: border-box;
  border: 1.5px solid var(--claude);
  border-radius: 50%;
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.dot.bad {
  background: var(--crash);
}

.cmd {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-strong);
}

.sigil {
  color: var(--run-text);
}

.meta {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.meta.warn {
  color: var(--claude-text);
}

.meta.bad {
  color: var(--crash-text);
}

.body {
  padding: 0 12px 10px 28px;
}

.bare .body {
  padding: 2px 12px 10px 32px;
  font-size: 12.5px;
}

/* The whole command, wrapped, since its row shows what it is for. */
.full {
  margin-bottom: 8px;
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-strong);
}

.card {
  margin-bottom: 8px;
}

.desc {
  margin-bottom: 6px;
  font-size: 12px;
  color: var(--text-subtle);
}

.out {
  max-height: 260px;
  margin: 0 0 6px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-log);
}

.out.err {
  color: var(--crash-text);
}

/* In the conversation, boxed as any call's output is (ToolRow). */
.bare .out {
  max-height: 320px;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-log);
}

.none {
  font-size: 12px;
  color: var(--text-faint);
}

.tools {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 4px;
}

.control {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 9px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text-soft);
  font: inherit;
  font-size: 11.5px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control:disabled {
  opacity: 0.5;
}
</style>
