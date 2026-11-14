<script setup lang="ts">
import { computed, ref } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import type { TaskEntry } from "../../lib/chat/store";
import { compactNumber } from "../../lib/chat/tools";
import { language, t } from "../../lib/i18n";
import { api } from "../../lib/store";
import ChatIcon from "./ChatIcon.vue";

/**
 * A row of Görevler: a subagent Claude started, a shell or a monitor left running in the
 * background. Its state, how long it has run, the tokens it took; opened, what it last did and
 * said of itself, and "Durdur" while it runs (`stop_task`). `bare`: its opened part alone, under
 * the call's own row in the conversation.
 */
const props = defineProps<{ id: number; task: TaskEntry; open: boolean; now: number; bare?: boolean }>();
const emit = defineEmits<{ toggle: []; long: [title: string, text: string] }>();

const running = computed(() => props.task.state === "running");

function span(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return t("chat.time.seconds", { count: seconds });
  return t("chat.time.minutes", { minutes: Math.floor(seconds / 60), seconds: seconds % 60 });
}

const meta = computed(() => {
  const parts: string[] = [];
  if (props.task.state === "failed") parts.push(t("chat.task.failed"));
  else if (props.task.state === "stopped") parts.push(t("chat.task.stopped"));
  const { started, ended } = props.task;
  if (started) parts.push(span((ended ?? props.now) - started));
  if (props.task.tokens) parts.push(t("chat.task.tokens", { count: compactNumber(props.task.tokens) }));
  return parts.join(" · ");
});

const kindLabel = computed(() => {
  if (props.task.kind === "agent") return props.task.agentType ?? t("chat.task.agent");
  if (props.task.kind === "shell") return t("chat.task.shell");
  return t("chat.task.other");
});

const busy = ref<"stop" | "output" | null>(null);
const output = ref<string | null>(null);

async function stop(): Promise<void> {
  if (!props.task.task || busy.value) return;
  busy.value = "stop";
  try {
    await api.chatControl(props.id, { kind: "stopTask", task: props.task.task });
  } catch {
    // Over already.
  } finally {
    busy.value = null;
  }
}

async function latest(): Promise<void> {
  if (!props.task.task || busy.value) return;
  busy.value = "output";
  try {
    const reply = await api.chatControl<Record<string, unknown>>(props.id, { kind: "taskOutput", task: props.task.task });
    const text = [reply?.output, reply?.stdout, reply?.content].find((each) => typeof each === "string");
    output.value = typeof text === "string" ? text : "";
  } catch {
    output.value = "";
  } finally {
    busy.value = null;
  }
}

const tools = computed(() => (props.task.tools ? new Intl.NumberFormat(language.value).format(props.task.tools) : null));
</script>

<template>
  <div :class="['task-row', { open, bare }]">
    <button v-if="!bare" type="button" class="line" :aria-expanded="open" @click="emit('toggle')">
      <span class="state">
        <Spinner v-if="running" :size="10" />
        <ChatIcon v-else :name="task.kind === 'agent' ? 'agent' : 'terminal'" :size="13" />
      </span>
      <span class="desc" data-find>{{ task.description || kindLabel }}</span>
      <span :class="['meta', { bad: task.state === 'failed' }]">{{ meta }}</span>
    </button>
    <Collapse :show="bare || open">
      <div class="body">
        <div class="facts">
          <span>{{ kindLabel }}</span>
          <span v-if="task.background">· {{ t("chat.task.background") }}</span>
          <span v-if="tools">· {{ t("chat.task.tools", { count: tools }) }}</span>
          <span v-if="task.lastTool && running">· {{ t("chat.task.last", { tool: task.lastTool }) }}</span>
        </div>
        <div v-if="task.summary" class="summary selectable" data-find>{{ task.summary }}</div>
        <pre v-if="output" class="out selectable" data-find>{{ output.split("\n").slice(-120).join("\n") }}</pre>
        <div v-if="running && task.task" class="tools">
          <button v-if="task.kind !== 'agent'" type="button" class="control" :disabled="!!busy" @click="latest">
            <Spinner v-if="busy === 'output'" :size="10" /><Icon v-else name="restart" :size="11" />{{ t("chat.command.latest") }}
          </button>
          <button type="button" class="control" :disabled="!!busy" @click="stop">
            <Spinner v-if="busy === 'stop'" :size="10" /><Icon v-else name="stop" :size="11" />{{ t("chat.command.stop") }}
          </button>
        </div>
      </div>
    </Collapse>
  </div>
</template>

<style scoped>
.task-row {
  border-bottom: 1px solid var(--line-soft);
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
  width: 13px;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-muted);
}

.desc {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12.5px;
  color: var(--text-strong);
}

.meta {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.meta.bad {
  color: var(--crash-text);
}

.body {
  padding: 0 12px 10px 29px;
  font-size: 12px;
}

.task-row.bare {
  border-bottom: 0;
}

.bare .body {
  padding: 2px 12px 10px 32px;
  font-size: 12.5px;
}

/* In the conversation: its buttons at the end of its facts' line, what it said under them. */
.bare .body {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  column-gap: 8px;
}

.bare .facts {
  flex: 1;
  min-width: 0;
}

.bare .tools {
  margin-top: 0;
}

.bare .summary,
.bare .out {
  flex-basis: 100%;
  order: 1;
}

/* In the conversation, boxed as any call's output is (ToolRow). */
.bare .out {
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-log);
}

.facts {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  color: var(--text-subtle);
}

.summary {
  margin-top: 6px;
  white-space: pre-wrap;
  color: var(--text-soft);
}

.out {
  max-height: 220px;
  margin: 6px 0 0;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  color: var(--text-log);
}

.tools {
  display: flex;
  gap: 6px;
  margin-top: 8px;
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
