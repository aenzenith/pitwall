<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t, type Key } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import type { ChatMessageRef } from "../../../lib/chat/ui";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { errorText, projectName, shownPath, useListNav, when, type DialogProps } from "./shared";

/**
 * Rewind (B10): the conversation's messages, the newest first; a picked one offers to put the
 * code back as it was there ("Kodu buraya geri al"), to go on from it in a new chat ("Konuşmayı
 * buradan çatalla", the `fork` event with its uuid), or both. Putting code back asks the CLI
 * first what it would change (`rewind_files` with `dry_run`) and lists it; only the user's yes
 * sends the real one, and what it did is said. Nothing changes a file before that yes.
 */
const props = defineProps<DialogProps & { messages: ChatMessageRef[] }>();
const emit = defineEmits<{ close: []; fork: [at: string | null] }>();

type Dry = { canRewind?: boolean; error?: string; filesChanged?: string[]; insertions?: number; deletions?: number };
type Done = { canRewind?: boolean; error?: string; skippedLinks?: number };
type Choice = { id: "code" | "fork" | "both"; name: Key; note: Key };

const CHOICES: Choice[] = [
  { id: "code", name: "chatDialog.rewind.code", note: "chatDialog.rewind.codeNote" },
  { id: "fork", name: "chatDialog.rewind.fork", note: "chatDialog.rewind.forkNote" },
  { id: "both", name: "chatDialog.rewind.both", note: "chatDialog.rewind.bothNote" },
];

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);
const choiceEl = ref<HTMLElement | null>(null);

/** The newest first, whichever way the conversation handed them over. */
const ordered = computed(() => {
  const list = [...props.messages];
  const first = list[0]?.time;
  const last = list[list.length - 1]?.time;
  return first === null || first === undefined || last === null || last === undefined || first <= last ? list.reverse() : list;
});

type Step = "list" | "choose" | "check" | "done";
const step = ref<Step>("list");
const picked = ref<ChatMessageRef | null>(null);
const plan = ref<Choice["id"] | null>(null);
const busy = ref(false);
const failed = ref<string | null>(null);
const dry = ref<Dry | null>(null);
const done = ref<Done | null>(null);

/** Code isn't put back under a turn that is still changing it. */
const turnRuns = computed(() => props.state === "running" || props.state === "requires_action");

function focusSoon(el: () => HTMLElement | null): void {
  void nextTick(() => el()?.focus({ preventScroll: true }));
}

function pick(message: ChatMessageRef): void {
  picked.value = message;
  failed.value = null;
  step.value = "choose";
  choices.move(0);
  focusSoon(() => choiceEl.value);
}

/** A message's own "rewind" action opens the dialog on that message. */
onMounted(() => {
  const asked = props.arg ? props.messages.find((message) => message.uuid === props.arg) : undefined;
  if (asked) pick(asked);
});

async function choose(choice: Choice): Promise<void> {
  const message = picked.value;
  if (!message || busy.value) return;
  if (choice.id === "fork") {
    emit("fork", message.uuid);
    frame.value?.close();
    return;
  }
  if (turnRuns.value) return;
  plan.value = choice.id;
  busy.value = true;
  failed.value = null;
  dry.value = null;
  try {
    dry.value = (await api.chatControl<Dry | null>(props.id, { kind: "rewindFiles", message: message.uuid, dryRun: true })) ?? {};
    step.value = "check";
  } catch (reason) {
    failed.value = errorText(reason);
  } finally {
    busy.value = false;
  }
}

const files = computed(() => (dry.value?.filesChanged ?? []).filter((file) => typeof file === "string"));
const nothing = computed(() => dry.value?.canRewind !== false && !files.value.length);

/** The user's yes: the real rewind, then the fork when both were asked for. Never while a turn
 * runs (one may have begun since the check). */
async function rewind(): Promise<void> {
  const message = picked.value;
  if (!message || busy.value || turnRuns.value) return;
  busy.value = true;
  failed.value = null;
  try {
    done.value = (await api.chatControl<Done | null>(props.id, { kind: "rewindFiles", message: message.uuid, dryRun: false })) ?? {};
    if (done.value.canRewind === false) {
      failed.value = done.value.error || t("chatDialog.rewind.cant");
      return;
    }
    if (plan.value === "both") {
      emit("fork", message.uuid);
      frame.value?.close();
      return;
    }
    step.value = "done";
  } catch (reason) {
    failed.value = errorText(reason);
  } finally {
    busy.value = false;
  }
}

/** Nothing to put back, and both were asked for: the fork alone is left. */
function forkOnly(): void {
  const message = picked.value;
  if (!message) return;
  emit("fork", message.uuid);
  frame.value?.close();
}

function back(): void {
  failed.value = null;
  if (step.value === "check") {
    step.value = "choose";
    focusSoon(() => choiceEl.value);
    return;
  }
  step.value = "list";
  picked.value = null;
  focusSoon(() => listEl.value);
}

const rows = useListNav(
  () => (step.value === "list" ? ordered.value.length : 0),
  (i) => pick(ordered.value[i]),
  listEl,
);
const choices = useListNav(
  () => (step.value === "choose" ? CHOICES.length : 0),
  (i) => void choose(CHOICES[i]),
  choiceEl,
);

function onKey(event: KeyboardEvent): void {
  if (step.value === "list") rows.onKey(event);
  else if (step.value === "choose") choices.onKey(event);
}

const firstLine = (text: string) => text.trim().split("\n")[0] || t("chatDialog.rewind.noText");
const rowId = (i: number) => `chat-rewind-${props.id}-${i}`;
const choiceId = (i: number) => `chat-rewind-choice-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.rewind.title')" :sub="projectName(path)" :width="520" :hold="busy && step === 'check'" @close="emit('close')" @key="onKey">
    <!-- 1. Which message. -->
    <template v-if="step === 'list'">
      <p v-if="!ordered.length" class="state">{{ t("chatDialog.rewind.empty") }}</p>
      <div v-else ref="listEl" class="scroll list" role="listbox" tabindex="0" data-focus :aria-label="t('chatDialog.rewind.title')" :aria-activedescendant="rowId(rows.active.value)">
        <div
          v-for="(message, i) in ordered"
          :id="rowId(i)"
          :key="message.uuid"
          :data-row="i"
          role="option"
          :aria-selected="i === rows.active.value"
          :class="['row', { active: i === rows.active.value }]"
          @click="
            rows.active.value = i;
            pick(message);
          "
        >
          <span class="text">
            <span class="name said" :title="message.text">{{ firstLine(message.text) }}</span>
          </span>
          <span v-if="message.changed" class="tag">{{ t("chatDialog.rewind.changed", { count: message.changed }) }}</span>
          <span v-if="message.time !== null" class="meta">{{ when(message.time) }}</span>
          <Icon name="chevron" :size="13" class="chevron" />
        </div>
      </div>
    </template>

    <!-- 2. What to do there. -->
    <template v-else-if="step === 'choose' && picked">
      <blockquote class="quote selectable">{{ picked.text.trim() || t("chatDialog.rewind.noText") }}</blockquote>
      <div ref="choiceEl" class="list" role="listbox" tabindex="0" :aria-label="t('chatDialog.rewind.title')" :aria-activedescendant="choiceId(choices.active.value)">
        <div
          v-for="(choice, i) in CHOICES"
          :id="choiceId(i)"
          :key="choice.id"
          :data-row="i"
          role="option"
          :aria-selected="i === choices.active.value"
          :aria-disabled="(choice.id !== 'fork' && turnRuns) || undefined"
          :class="['row', 'tall', { active: i === choices.active.value, off: choice.id !== 'fork' && turnRuns }]"
          @click="
            choices.active.value = i;
            choose(choice);
          "
        >
          <span class="mark">
            <Spinner v-if="busy && plan === choice.id" :size="12" />
            <Icon v-else :name="choice.id === 'fork' ? 'source-control' : choice.id === 'code' ? 'restart' : 'commit'" :size="13" />
          </span>
          <span class="text">
            <span class="name">{{ t(choice.name) }}</span>
            <span class="desc">{{ t(choice.note) }}</span>
          </span>
        </div>
      </div>
      <p v-if="turnRuns" class="note">{{ t("chatDialog.rewind.running") }}</p>
    </template>

    <!-- 3. What putting the code back would change, before the yes. -->
    <template v-else-if="step === 'check' && dry">
      <p v-if="dry.canRewind === false" class="error">{{ dry.error || t("chatDialog.rewind.cant") }}</p>
      <p v-else-if="nothing" class="note">{{ t("chatDialog.rewind.nothing") }}</p>
      <template v-else>
        <p class="say">
          {{ t("chatDialog.rewind.willChange", { count: files.length }) }}
          <span class="lines mono">
            <span class="plus">+{{ dry.insertions ?? 0 }}</span>
            <span class="minus">−{{ dry.deletions ?? 0 }}</span>
          </span>
        </p>
        <ul class="scroll files">
          <li v-for="file in files" :key="file" class="file mono" :title="file">{{ shownPath(path, file) }}</li>
        </ul>
        <p class="note">{{ t("chatDialog.rewind.confirmNote") }}</p>
        <p v-if="turnRuns" class="note">{{ t("chatDialog.rewind.running") }}</p>
      </template>
    </template>

    <!-- 4. What it did. -->
    <p v-else-if="step === 'done'" class="saved" role="status">
      <span class="saved-mark"><Icon name="check" :size="12" /></span>
      <span>
        {{ t("chatDialog.rewind.done", { count: files.length }) }}
        <template v-if="done?.skippedLinks">{{ t("chatDialog.rewind.skipped", { count: done.skippedLinks }) }}</template>
      </span>
    </p>

    <NotAnswered v-if="failed && step !== 'check'" :id="id" :state="state" :error="failed" @done="frame?.close()" />
    <p v-else-if="failed" class="error" role="alert">{{ failed }}</p>

    <template #foot>
      <template v-if="step === 'choose'">
        <button type="button" class="control" :disabled="busy" @click="back">{{ t("common.back") }}</button>
      </template>
      <template v-else-if="step === 'check' && dry">
        <button type="button" class="control" :disabled="busy" @click="back">{{ t("common.back") }}</button>
        <button v-if="nothing && plan === 'both'" type="button" class="control primary" @click="forkOnly">{{ t("chatDialog.rewind.forkOnly") }}</button>
        <button v-else-if="!nothing && dry.canRewind !== false" type="button" class="control primary" :disabled="busy || turnRuns" :aria-busy="busy" @click="rewind">
          <span :class="{ hidden: busy }">{{ t(plan === "both" ? "chatDialog.rewind.yesBoth" : "chatDialog.rewind.yes") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </template>
      <button v-else-if="step === 'done'" type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
      <button v-else type="button" class="control" @click="frame?.close()">{{ t("common.cancel") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list.scroll {
  max-height: 360px;
}

.said {
  font-weight: 400;
}

.chevron {
  flex-shrink: 0;
  color: var(--text-faint);
}

.row.tall {
  align-items: flex-start;
  padding-top: 7px;
  padding-bottom: 7px;
}

.row.tall .mark {
  margin-top: 2px;
  color: var(--text-muted);
}

.quote {
  flex-shrink: 0;
  margin: 0;
  padding: 8px 12px;
  border-left: 3px solid var(--line-strong);
  border-radius: 0 6px 6px 0;
  background: var(--bg-well);
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text-soft);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  display: -webkit-box;
  -webkit-line-clamp: 4;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.say {
  margin: 0;
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 10px;
  font-weight: 500;
  color: var(--text-strong);
}

.lines {
  display: inline-flex;
  gap: 6px;
  font-size: 12px;
  font-weight: 400;
}

.plus {
  color: var(--run-text);
}

.minus {
  color: var(--crash-text);
}

.files {
  margin: 0 -20px;
  list-style: none;
  max-height: 220px;
}

.file {
  min-height: 24px;
  display: flex;
  align-items: center;
  font-size: 12px;
  color: var(--text-soft);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.saved {
  margin: 0;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  font-size: 12.5px;
  line-height: 1.5;
}

.saved-mark {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--bg-icon-hover);
  color: var(--run-text);
}
</style>
