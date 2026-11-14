<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Spinner from "../../../components/Spinner.vue";
import { tokenText } from "../../../lib/day";
import { t, type Key } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { modelName, projectName, share, useAsk, type DialogProps } from "./shared";

/**
 * What the context holds (B6, `get_context_usage`): the model and its window, a bar of what fills
 * it, each category with its tokens and its share of the window, the tools held back until needed
 * apart, and when Claude Code compacts by itself. `/compact` is offered as the text it is: put in
 * the composer, sent by the user.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: []; insert: [text: string] }>();

type Category = { id?: string; name?: string; tokens?: number; kind?: string; isDeferred?: boolean };
type Usage = {
  categories?: Category[];
  totalTokens?: number;
  maxTokens?: number;
  model?: string;
  autoCompactThreshold?: number;
  isAutoCompactEnabled?: boolean;
};

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const usage = useAsk<Usage>(() => props.id);
onMounted(() => void usage.load({ kind: "contextUsage" }));

/** The categories the CLI names by id, in the app's language; any other keeps the CLI's name. */
const NAMES: Record<string, Key> = {
  system_prompt: "chatDialog.context.cat.systemPrompt",
  system_tools: "chatDialog.context.cat.systemTools",
  system_tools_deferred: "chatDialog.context.cat.systemTools",
  mcp_instructions: "chatDialog.context.cat.mcpInstructions",
  mcp_tools: "chatDialog.context.cat.mcpTools",
  mcp_tools_deferred: "chatDialog.context.cat.mcpTools",
  custom_agents: "chatDialog.context.cat.agents",
  agents: "chatDialog.context.cat.agents",
  memory_files: "chatDialog.context.cat.memory",
  skills: "chatDialog.context.cat.skills",
  slash_commands: "chatDialog.context.cat.commands",
  messages: "chatDialog.context.cat.messages",
  autocompact_buffer: "chatDialog.context.cat.buffer",
  free_space: "chatDialog.context.cat.free",
};

/** A colour per category, the same each time: the app's data palette, then its lanes. */
const COLOURS: Record<string, string> = {
  system_prompt: "var(--data-1)",
  system_tools: "var(--data-2)",
  mcp_instructions: "var(--data-3)",
  mcp_tools: "var(--data-3)",
  skills: "var(--data-4)",
  messages: "var(--data-5)",
  memory_files: "var(--data-6)",
  custom_agents: "var(--lane-2)",
  agents: "var(--lane-2)",
  slash_commands: "var(--lane-3)",
};
const SPARE = ["var(--lane-0)", "var(--lane-1)", "var(--lane-4)"];

type Shown = { key: string; name: string; tokens: number; colour: string; share: number };

const span = computed(() => usage.value.value?.maxTokens ?? 0);

function named(category: Category): string {
  const key = category.id ? NAMES[category.id] : undefined;
  return key ? t(key) : (category.name ?? category.id ?? "");
}

const categories = computed(() => (usage.value.value?.categories ?? []).filter((c) => typeof c.tokens === "number" && c.tokens >= 0));

/** What is in the context now, in the CLI's order. */
const used = computed<Shown[]>(() => {
  let spare = 0;
  return categories.value
    .filter((c) => !c.isDeferred && (c.kind === "used" || c.kind === undefined) && c.id !== "free_space" && c.id !== "autocompact_buffer")
    .map((c, i) => ({
      key: c.id ?? `used-${i}`,
      name: named(c),
      tokens: c.tokens!,
      colour: (c.id && COLOURS[c.id]) || SPARE[spare++ % SPARE.length],
      share: span.value ? c.tokens! / span.value : 0,
    }));
});

/** The room Claude Code keeps for compacting, and the free room. */
const buffer = computed(() => categories.value.find((c) => c.kind === "buffer" || c.id === "autocompact_buffer") ?? null);
const free = computed(() => categories.value.find((c) => c.kind === "free" || c.id === "free_space") ?? null);
/** Tools held back: in the context only once Claude reaches for them. */
const deferred = computed(() => categories.value.filter((c) => c.isDeferred || c.kind === "deferred"));

const total = computed(() => usage.value.value?.totalTokens ?? used.value.reduce((sum, c) => sum + c.tokens, 0));

const sub = computed(() => {
  const value = usage.value.value;
  if (!value) return projectName(props.path);
  const model = modelName(props.info, value.model);
  return span.value ? t("chatDialog.context.window", { model, tokens: tokenText(span.value) }) : model;
});

const compact = computed(() => {
  const value = usage.value.value;
  if (!value) return "";
  if (value.isAutoCompactEnabled === false) return t("chatDialog.context.autoOff");
  if (value.autoCompactThreshold && span.value) return t("chatDialog.context.autoAt", { share: share(value.autoCompactThreshold / span.value) });
  return "";
});

/** Each piece of the bar's width, at least a hair when it holds anything. */
const width = (tokens: number) => (span.value ? `max(2px, ${(tokens / span.value) * 100}%)` : "0");

function insertCompact(): void {
  emit("insert", "/compact ");
  frame.value?.close();
}
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.context.title')" :sub="sub" :width="460" @close="emit('close')">
    <div v-if="usage.busy.value && !usage.value.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="usage.error.value" :id="id" :state="state" :error="usage.error.value" @done="frame?.close()" />

    <template v-else-if="usage.value.value">
      <div class="total">
        <span class="big">{{ share(span ? total / span : 0) }}</span>
        <span class="of">{{ t("chatDialog.context.used", { tokens: tokenText(total), window: tokenText(span) }) }}</span>
      </div>

      <div class="bar" role="img" :aria-label="t('chatDialog.context.used', { tokens: tokenText(total), window: tokenText(span) })">
        <span v-for="c in used" :key="c.key" class="piece" :style="{ width: width(c.tokens), background: c.colour }"></span>
        <span class="room"></span>
        <span v-if="buffer?.tokens" class="buffer" :style="{ width: width(buffer.tokens) }"></span>
      </div>

      <div class="scroll">
        <ul class="legend">
          <li v-for="c in used" :key="c.key" class="item">
            <span class="swatch" :style="{ background: c.colour }"></span>
            <span class="label">{{ c.name }}</span>
            <span class="tokens">{{ tokenText(c.tokens) }}</span>
            <span class="share">{{ share(c.share) }}</span>
          </li>
          <li v-if="buffer?.tokens" class="item">
            <span class="swatch buffer-swatch"></span>
            <span class="label">{{ named(buffer) }}</span>
            <span class="tokens">{{ tokenText(buffer.tokens) }}</span>
            <span class="share">{{ share(span ? buffer.tokens / span : 0) }}</span>
          </li>
          <li v-if="free?.tokens !== undefined" class="item">
            <span class="swatch free-swatch"></span>
            <span class="label">{{ named(free) }}</span>
            <span class="tokens">{{ tokenText(free.tokens) }}</span>
            <span class="share">{{ share(span ? free.tokens / span : 0) }}</span>
          </li>
        </ul>

        <template v-if="deferred.length">
          <div class="group">
            <span class="section-label">{{ t("chatDialog.context.deferred") }}</span>
          </div>
          <ul class="legend">
            <li v-for="(c, i) in deferred" :key="c.id ?? i" class="item dim">
              <span class="swatch free-swatch"></span>
              <span class="label">{{ named(c) }}</span>
              <span class="tokens">{{ tokenText(c.tokens ?? 0) }}</span>
              <span class="share"></span>
            </li>
          </ul>
          <p class="note">{{ t("chatDialog.context.deferredNote") }}</p>
        </template>
      </div>

      <p v-if="compact" class="note">{{ compact }}</p>
    </template>

    <template #foot>
      <span class="grow note">{{ t("chatDialog.context.compactNote") }}</span>
      <button type="button" class="control" @click="insertCompact">{{ t("chatDialog.context.compact") }}</button>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.total {
  flex-shrink: 0;
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.big {
  font-size: 22px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--text-strong);
}

.of {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.bar {
  flex-shrink: 0;
  height: 10px;
  display: flex;
  gap: 1px;
  border-radius: 5px;
  overflow: hidden;
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-soft);
}

.piece {
  flex-shrink: 0;
  height: 100%;
}

.room {
  flex: 1 1 auto;
}

/* The compacting room: kept, not used, drawn as a hatch at the window's end. */
.buffer,
.buffer-swatch {
  flex-shrink: 0;
  background: repeating-linear-gradient(135deg, var(--line-strong) 0 2px, transparent 2px 5px);
}

.legend {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
}

.item {
  min-height: 26px;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12.5px;
}

.item.dim {
  color: var(--text-subtle);
}

.swatch {
  flex-shrink: 0;
  width: 9px;
  height: 9px;
  border-radius: 3px;
}

.free-swatch {
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-strong);
}

.label {
  flex: 1 1 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tokens,
.share {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  text-align: right;
  color: var(--text-subtle);
}

.tokens {
  min-width: 56px;
}

.share {
  min-width: 44px;
  color: var(--text-faint);
}

.grow.note {
  font-size: 11.5px;
}
</style>
