<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, reactive, ref, shallowRef, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import type { CanUseToolRequest, ChatAnswer } from "../../lib/chat/wire";
import { t } from "../../lib/i18n";

/**
 * Claude's questions (AskUserQuestion; B3, V151): a tab per question, its options (one, or
 * several when `multiSelect`), each with its description, and "Other" with the user's own words;
 * an option's preview beside the list when one came. The answer is the CLI's: `answers` by
 * question, a multiple choice's labels joined with ", ", "Other" as its words.
 */
const props = defineProps<{ request: CanUseToolRequest }>();
const emit = defineEmits<{ answer: [answer: ChatAnswer]; refocus: [] }>();

type Option = { label: string; description: string; preview: string };
type Question = { question: string; header: string; options: Option[]; multi: boolean };

const questions = computed<Question[]>(() => {
  const list = props.request.input?.questions;
  if (!Array.isArray(list)) return [];
  return list.flatMap((raw) => {
    if (!raw || typeof raw !== "object") return [];
    const each = raw as Record<string, unknown>;
    if (typeof each.question !== "string") return [];
    const options = Array.isArray(each.options)
      ? each.options.flatMap((option) => {
          if (!option || typeof option !== "object") return [];
          const one = option as Record<string, unknown>;
          return typeof one.label === "string" ? [{ label: one.label, description: typeof one.description === "string" ? one.description : "", preview: typeof one.preview === "string" ? one.preview : "" }] : [];
        })
      : [];
    return [{ question: each.question, header: typeof each.header === "string" ? each.header : "", options, multi: each.multiSelect === true }];
  });
});

const tab = ref(0);
const picks = reactive<Record<number, string[]>>({});
const other = reactive<Record<number, string>>({});
const otherOn = reactive<Record<number, boolean>>({});
/** The option the keys or the pointer rest on, in the shown question; its preview shows. */
const pointed = ref(0);
/** The "Other" field (one, outside any list: a plain element ref). */
const otherField = ref<HTMLInputElement | null>(null);
const sent = ref<"answer" | "decline" | null>(null);

const current = computed(() => questions.value[tab.value]);

function picked(at: number): string[] {
  return picks[at] ?? [];
}

function isAnswered(at: number): boolean {
  return picked(at).length > 0 || (!!otherOn[at] && !!other[at]?.trim());
}

const complete = computed(() => questions.value.length > 0 && questions.value.every((_, at) => isAnswered(at)));

function nextOpen(): void {
  const next = questions.value.findIndex((_, at) => at !== tab.value && !isAnswered(at));
  if (next >= 0) show(next);
}

function show(at: number): void {
  tab.value = at;
  pointed.value = 0;
}

function toggle(index: number): void {
  const question = current.value;
  if (!question || sent.value) return;
  const at = tab.value;
  pointed.value = index;
  if (index === question.options.length) {
    // "Other": its field takes the keyboard.
    otherOn[at] = question.multi ? !otherOn[at] : true;
    if (!question.multi) picks[at] = [];
    if (otherOn[at]) void nextTick(() => otherField.value?.focus());
    return;
  }
  const label = question.options[index].label;
  if (question.multi) {
    const now = picked(at);
    picks[at] = now.includes(label) ? now.filter((each) => each !== label) : question.options.map((each) => each.label).filter((each) => each === label || now.includes(each));
  } else {
    picks[at] = [label];
    otherOn[at] = false;
    nextOpen();
  }
}

function answerOf(at: number): string {
  const words = otherOn[at] ? (other[at] ?? "").trim() : "";
  return [...picked(at), ...(words ? [words] : [])].join(", ");
}

function submit(): void {
  if (!complete.value || sent.value) return;
  sent.value = "answer";
  const answers = Object.fromEntries(questions.value.map((question, at) => [question.question, answerOf(at)]));
  emit("answer", { kind: "allow", input: { answers } });
}

function decline(): void {
  if (sent.value) return;
  sent.value = "decline";
  emit("answer", { kind: "deny", message: "", interrupt: false });
}

/* ---------- the preview ---------- */

const preview = computed(() => current.value?.options[pointed.value]?.preview ?? "");
const previewHtml = shallowRef("");
watch(
  preview,
  async (text) => {
    if (!text) return (previewHtml.value = "");
    const lib = await import("../../lib/markdownPreview");
    previewHtml.value = lib.renderMarkdown(text, { note: t("files.md.note"), tip: t("files.md.tip"), important: t("files.md.important"), warning: t("files.md.warning"), caution: t("files.md.caution") });
  },
  { immediate: true },
);
const hasPreview = computed(() => current.value?.options.some((each) => !!each.preview) ?? false);

/* ---------- keys ---------- */

function key(event: KeyboardEvent): boolean {
  if (sent.value) return false;
  const target = event.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    if (event.key === "Escape") {
      emit("refocus");
      return true;
    }
    if (event.key === "Enter" && !event.isComposing) {
      if (complete.value) emit("refocus");
      else nextOpen();
      return true;
    }
    return false;
  }
  if (event.metaKey || event.ctrlKey || event.altKey) return false;
  const question = current.value;
  if (!question) return false;
  const count = question.options.length + 1;
  const digit = Number(event.key);
  if (Number.isInteger(digit) && digit >= 1 && digit <= count) {
    toggle(digit - 1);
    return true;
  }
  switch (event.key) {
    case "ArrowDown":
    case "ArrowUp":
      pointed.value = (pointed.value + (event.key === "ArrowDown" ? 1 : -1) + count) % count;
      return true;
    case "ArrowLeft":
    case "ArrowRight":
      if (questions.value.length < 2) return false;
      show((tab.value + (event.key === "ArrowRight" ? 1 : -1) + questions.value.length) % questions.value.length);
      return true;
    case " ":
      if (target instanceof HTMLButtonElement) return false;
      toggle(pointed.value);
      return true;
    case "Enter":
      if (target instanceof HTMLButtonElement) return false;
      // Picking is not answering: the answer goes by its button (`requires_user_interaction`).
      if (!isAnswered(tab.value)) toggle(pointed.value);
      else nextOpen();
      return true;
    case "Escape":
      decline();
      return true;
  }
  return false;
}

/* The question's room follows what its tab holds, its height easing from one tab to the next and
   as "Other"'s field comes or goes (the user's word, 16 Nov: "tak diye geçiş istemiyorum"); the
   first height is the one it stands at. What doesn't fit yet is hidden while it moves. */
const roomIn = ref<HTMLElement | null>(null);
const roomHeight = ref<number | null>(null);
const roomMoving = ref(false);
let roomTimer: ReturnType<typeof setTimeout> | undefined;
let roomWatch: ResizeObserver | null = null;
watch(roomIn, (el) => {
  roomWatch?.disconnect();
  if (!el) return;
  roomWatch ??= new ResizeObserver(() => {
    const height = roomIn.value?.offsetHeight;
    if (height === undefined || height === roomHeight.value) return;
    if (roomHeight.value !== null) {
      roomMoving.value = true;
      clearTimeout(roomTimer);
      roomTimer = setTimeout(() => (roomMoving.value = false), 260);
    }
    roomHeight.value = height;
  });
  roomWatch.observe(el);
});
onBeforeUnmount(() => {
  roomWatch?.disconnect();
  clearTimeout(roomTimer);
});

defineExpose({ key });
</script>

<template>
  <div v-if="questions.length > 1" class="tabs" role="tablist">
    <button v-for="(question, at) in questions" :key="at" type="button" role="tab" :aria-selected="tab === at" :class="['tab', { on: tab === at, done: isAnswered(at) }]" @click="show(at)">
      <span class="dot" aria-hidden="true"></span>{{ question.header || t("chatInput.question.tab", { n: at + 1 }) }}
    </button>
  </div>

  <div v-if="current" :class="['room', { moving: roomMoving }]" :style="roomHeight === null ? undefined : { height: `${roomHeight}px` }">
    <div ref="roomIn" class="room-in">
      <p class="cc-lead selectable">{{ current.question }}</p>
      <div :class="['qbody', { split: hasPreview }]">
        <div class="options" :role="current.multi ? 'group' : 'radiogroup'" :aria-label="current.question">
          <button
            v-for="(option, index) in current.options"
            :key="option.label"
            type="button"
            :role="current.multi ? 'checkbox' : 'radio'"
            :aria-checked="picked(tab).includes(option.label)"
            :class="['opt', { on: picked(tab).includes(option.label), pointed: pointed === index }]"
            :disabled="!!sent"
            @mouseenter="pointed = index"
            @click="toggle(index)"
          >
            <span :class="['mark', current.multi ? 'box' : 'round']" aria-hidden="true"></span>
            <span class="cc-k">{{ index + 1 }}</span>
            <span class="txt">
              <span class="lbl">{{ option.label }}</span>
              <span v-if="option.description" class="desc">{{ option.description }}</span>
            </span>
          </button>
          <button
            type="button"
            :role="current.multi ? 'checkbox' : 'radio'"
            :aria-checked="!!otherOn[tab]"
            :class="['opt', { on: otherOn[tab], pointed: pointed === current.options.length }]"
            :disabled="!!sent"
            @mouseenter="pointed = current.options.length"
            @click="toggle(current.options.length)"
          >
            <span :class="['mark', current.multi ? 'box' : 'round']" aria-hidden="true"></span>
            <span class="cc-k">{{ current.options.length + 1 }}</span>
            <span class="txt"><span class="lbl">{{ t("chatInput.question.other") }}</span></span>
          </button>
          <input v-if="otherOn[tab]" ref="otherField" v-model="other[tab]" class="cc-input" type="text" :placeholder="t('chatInput.question.otherHint')" :aria-label="t('chatInput.question.other')" :disabled="!!sent" />
        </div>
        <div v-if="hasPreview" class="preview selectable">
          <!-- eslint-disable-next-line vue/no-v-html -->
          <div v-if="previewHtml" class="cc-md" v-html="previewHtml"></div>
          <p v-else class="cc-note">{{ t("chatInput.question.noPreview") }}</p>
        </div>
      </div>
    </div>
  </div>

  <div class="cc-actions">
    <button class="cc-btn primary" type="button" :disabled="!complete || !!sent" @click="submit">
      <Spinner v-if="sent === 'answer'" :size="11" />{{ t("chatInput.question.send") }}
    </button>
    <button class="cc-btn" type="button" :disabled="!!sent" @click="decline">
      <Spinner v-if="sent === 'decline'" :size="11" /><span v-else class="cc-k">esc</span>{{ t("chatInput.question.decline") }}
    </button>
    <span class="cc-gap"></span>
    <span v-if="current?.multi" class="cc-note">{{ t("chatInput.question.multi") }}</span>
  </div>
</template>

<style scoped>
.room {
  transition: height 0.22s cubic-bezier(0.2, 0, 0, 1);
}

.room.moving {
  overflow: hidden;
}

.room-in {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

@media (prefers-reduced-motion: reduce) {
  .room {
    transition: none;
  }
}

.tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 12.5px;
  color: var(--text-muted);
}

.tab:hover {
  background: var(--bg-icon-hover);
}

.tab.on {
  border-color: var(--line-strong);
  background: var(--bg-control);
  color: var(--text-strong);
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  border: 1.5px solid var(--text-faint);
}

.tab.done .dot {
  border-color: var(--btn-start-text);
  background: var(--btn-start-text);
}

.qbody {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 10px;
}

.qbody.split {
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
}

.options {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.opt {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  width: 100%;
  padding: 7px 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  text-align: left;
  white-space: normal;
  color: var(--text);
}

.opt.pointed {
  background: var(--bg-control-hover);
}

.opt.on {
  border-color: var(--line-lit);
}

.opt:disabled {
  opacity: 0.6;
}

.mark {
  flex-shrink: 0;
  width: 13px;
  height: 13px;
  margin-top: 2px;
  border: 1.5px solid var(--line-lit);
  box-sizing: border-box;
}

.mark.round {
  border-radius: 50%;
}

.mark.box {
  border-radius: 3px;
}

.opt.on .mark {
  border-color: var(--text-strong);
  box-shadow: inset 0 0 0 2.5px var(--bg-control), inset 0 0 0 7px var(--text-strong);
}

.opt.on.pointed .mark {
  box-shadow: inset 0 0 0 2.5px var(--bg-control-hover), inset 0 0 0 7px var(--text-strong);
}

.opt .cc-k {
  flex-shrink: 0;
  margin-top: 1px;
}

.txt {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.lbl {
  font-size: 13px;
  color: var(--text-strong);
}

.desc {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-muted);
}

.preview {
  max-height: 320px;
  overflow: auto;
  padding: 8px 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
}
</style>
