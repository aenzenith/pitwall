<script setup lang="ts">
import { computed, inject, ref } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import type { ChangeEntry, CommandEntry, TaskEntry, ToolEntry } from "../../lib/chat/store";
import { inputLines, toolLook } from "../../lib/chat/tools";
import { CHAT_PLAN, CHAT_ROW } from "../../lib/chat/ui";
import { t } from "../../lib/i18n";
import ChangeRow from "./ChangeRow.vue";
import ChatIcon from "./ChatIcon.vue";
import ChatMarkdown from "./ChatMarkdown.vue";
import CommandRow from "./CommandRow.vue";
import TaskRow from "./TaskRow.vue";

/**
 * A tool call on one line, boxed (TS3): its icon, a verb in the past tense, what it was called on,
 * and at the right end what came of it. A click opens it under itself, its height eased: a command
 * as Komutlar draws it (its output, its buttons), a change as Değişiklikler does (its diff, the ways
 * out), a subagent its state and then what it was asked and answered, any other what it was asked
 * and what came back (the user's word, 16 Nov: rows in the conversation open where they are, the
 * status line's card being the whole list). One that gave a picture back (a Read of an image)
 * shows it small in its icon's place with its size, and a click shows it large (15 Nov).
 */
const props = defineProps<{
  tool: ToolEntry;
  project: string;
  change?: ChangeEntry;
  command?: CommandEntry;
  task?: TaskEntry;
  open: boolean;
  /** A card waits for this call (its permission). */
  waiting: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
  long: [title: string, text: string];
  /** Its picture shown large: `<tool id>:<index>`, as the chat's lightbox lists it. */
  picture: [key: string];
}>();

/** The first picture it gave back, while the chat still holds it (`PICTURES_MOST`). */
const picture = computed(() => {
  const first = props.tool.result?.pictures[0];
  return first ? `data:${first.mediaType};base64,${first.data}` : null;
});
const pictureKey = computed(() => `${props.tool.id}:0`);
const pictureSize = ref<string | null>(null);
function sized(event: Event): void {
  const img = event.target as HTMLImageElement;
  if (img.naturalWidth) pictureSize.value = `${img.naturalWidth}×${img.naturalHeight}`;
}

const look = computed(() => toolLook(props.tool, props.project, props.change, props.command, props.task));
const running = computed(() => {
  const tool = props.tool;
  if (tool.history || tool.result || tool.ended !== null || props.waiting) return false;
  return props.command ? props.command.state === "running" : true;
});
/** The chat's id and clock, for a command's and a subagent's part (ChatView gives them). */
const row = inject(CHAT_ROW, null);
const opens = computed(() => !!picture.value || !!props.command || !!props.change || !!props.task || expandable.value);

const expandable = computed(() => {
  const name = props.tool.name;
  if (["TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet", "EnterPlanMode"].includes(name)) return false;
  return !!props.tool.result || Object.keys(props.tool.input).length > 0;
});

/** A plan's row brings the plan up on its screen (ChatView, PL5) instead of opening under itself. */
const planScreen = inject(CHAT_PLAN, null);
const rowEl = ref<HTMLElement | null>(null);

function click(): void {
  if (picture.value) emit("picture", pictureKey.value);
  else if (plan.value && planScreen && rowEl.value) planScreen.open(props.tool.id, plan.value, rowEl.value);
  else if (opens.value) emit("toggle");
}

/** What came back, cut for the row (the whole in the long text viewer). */
const RESULT_LINES = 40;
const resultText = computed(() => props.tool.result?.text ?? "");
const resultCut = computed(() => {
  const lines = resultText.value.split("\n");
  return lines.length > RESULT_LINES ? `${lines.slice(0, RESULT_LINES).join("\n")}\n…` : resultText.value;
});
const longResult = computed(() => resultText.value.split("\n").length > RESULT_LINES || resultText.value.length > 6000);

/** AskUserQuestion: each question and the answer given. */
const answers = computed(() => {
  if (props.tool.name !== "AskUserQuestion") return [];
  const questions = Array.isArray(props.tool.input.questions) ? (props.tool.input.questions as Record<string, unknown>[]) : [];
  const data = props.tool.result?.data as { answers?: Record<string, string> } | null;
  const given = data?.answers ?? (props.tool.input.answers as Record<string, string> | undefined) ?? {};
  return questions.map((question) => {
    const text = typeof question.question === "string" ? question.question : "";
    return { question: text, answer: given[text] ?? null };
  });
});

const plan = computed(() => (props.tool.name === "ExitPlanMode" && typeof props.tool.input.plan === "string" ? props.tool.input.plan : null));
const inputs = computed(() => inputLines(props.tool.input));
</script>

<template>
  <div ref="rowEl" :class="['tool-row', { open, failed: look.tone === 'error', waiting }]" :data-plan-row="plan ? tool.id : undefined">
    <button type="button" class="line" :aria-expanded="opens ? open : undefined" :disabled="!opens" @click="click">
      <!-- At work, its own icon breathes instead of a spinner (the user's word, 15 Nov). -->
      <span :class="['icon', { breath: running }]" :data-shot="picture ? pictureKey : undefined">
        <img v-if="picture" class="pthumb" :src="picture" alt="" draggable="false" @load="sized" />
        <ChatIcon v-else :name="look.icon" :size="14" />
      </span>
      <span class="verb">{{ look.verb }}</span>
      <span v-if="look.arg" :class="['arg', { mono: look.mono }]" :title="look.hint ?? undefined" data-find>{{ look.arg }}</span>
      <span class="grow"></span>
      <span v-if="waiting" class="meta warn">{{ t("chat.command.waiting") }}</span>
      <span v-else-if="look.meta" :class="['meta', look.tone]">{{ look.meta }}</span>
      <span v-else-if="picture && pictureSize" class="meta">{{ pictureSize }}</span>
      <span v-if="opens" :class="['chev', { down: open && !picture }]"><Icon name="chevron" :size="12" /></span>
    </button>
    <Collapse :show="open && !picture">
      <CommandRow
        v-if="command && row"
        :id="row.id.value"
        bare
        :path="project"
        :command="command"
        :open="true"
        :now="row.now.value"
        :pending="null"
        @long="(title, text) => emit('long', title, text)"
      />
      <ChangeRow v-else-if="change" bare :path="project" :file="change.file" :changes="[change]" :open="true" />
      <template v-else>
        <TaskRow v-if="task && row" :id="row.id.value" bare :task="task" :open="true" :now="row.now.value" @long="(title, text) => emit('long', title, text)" />
        <div v-if="expandable" :class="['body', 'selectable', { after: task && row }]">
          <template v-if="answers.length">
            <div v-for="(each, i) in answers" :key="i" class="qa" data-find>
              <div class="q">{{ each.question }}</div>
              <div class="a">{{ each.answer ?? t("chat.tool.noAnswer") }}</div>
            </div>
          </template>
          <ChatMarkdown v-else-if="plan" class="plan" :text="plan" :streaming="false" :project="project" />
          <template v-else>
            <dl v-if="inputs.length" class="inputs" data-find>
              <template v-for="each in inputs" :key="each.key">
                <dt>{{ each.key }}</dt>
                <dd>{{ each.value }}</dd>
              </template>
            </dl>
            <pre v-if="resultText" :class="['result', { error: tool.result?.error }]" data-find>{{ resultCut }}</pre>
            <button v-if="longResult" type="button" class="link" @click="emit('long', look.verb, resultText)">{{ t("chat.showAll") }}</button>
            <div v-if="tool.result?.images" class="note">{{ t("chat.tool.images", { count: tool.result.images }) }}</div>
          </template>
        </div>
      </template>
    </Collapse>
  </div>
</template>

<style scoped>
.tool-row {
  border: 1px solid var(--line-inset);
  border-radius: 8px;
  background: var(--bg-panel);
}

.tool-row.waiting {
  border-color: var(--claude-line);
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-width: 0;
  height: 32px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: default;
}

.line:not(:disabled):hover {
  background: var(--bg-row-hover);
}

.line:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.icon {
  display: inline-flex;
  width: 14px;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-muted);
}

/* A picture it read, in its icon's place. */
.pthumb {
  width: 14px;
  height: 14px;
  border-radius: 3px;
  object-fit: cover;
}

.icon.breath {
  color: var(--text-soft);
  animation: breath 1.6s ease-in-out infinite;
}

@keyframes breath {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.3;
  }
}

@media (prefers-reduced-motion: reduce) {
  .icon.breath {
    animation: none;
    opacity: 0.6;
  }
}

.verb {
  flex-shrink: 0;
  font-weight: 600;
  color: var(--text-strong);
}

.arg {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-soft);
}

.arg.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.grow {
  flex: 1;
}

.meta {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.meta.add {
  color: var(--run-text);
}

.meta.error {
  color: var(--crash-text);
}

.meta.warn {
  font-family: var(--font);
  color: var(--claude-text);
}

.meta.ok {
  font-family: var(--font);
  color: var(--run-text);
}

.chev {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-faint);
  transition: transform 0.15s;
}

.chev.down {
  transform: rotate(90deg);
}

.body {
  padding: 2px 12px 10px 32px;
  font-size: 12.5px;
}

/* Under a subagent's state: what it was asked and answered. */
.body.after {
  padding-top: 0;
}

.inputs {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 2px 10px;
  margin: 0 0 8px;
}

.inputs dt {
  color: var(--text-subtle);
  font-family: var(--font-mono);
  font-size: 11.5px;
}

.inputs dd {
  margin: 0;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-soft);
}

.result {
  max-height: 320px;
  margin: 0;
  overflow: auto;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-soft);
}

.result.error {
  color: var(--crash-text);
}

.qa + .qa {
  margin-top: 8px;
}

.q {
  color: var(--text-soft);
}

.a {
  margin-top: 2px;
  font-weight: 600;
  color: var(--text-strong);
}

.plan {
  padding-top: 4px;
}

.note {
  margin-top: 6px;
  color: var(--text-subtle);
}

.link {
  margin-top: 6px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-link);
  font: inherit;
  font-size: 12px;
}

.link:hover {
  text-decoration: underline;
}
</style>
