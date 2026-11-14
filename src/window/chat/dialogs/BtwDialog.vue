<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Markdown from "../../../components/Markdown.vue";
import Spinner from "../../../components/Spinner.vue";
import { chatModel, type SideQuestion, type SideQuestions } from "../../../lib/chat/store";
import { t } from "../../../lib/i18n";
import { useReturnFocus } from "../../../lib/dialogFocus";
import { api } from "../../../lib/store";
import NotAnswered from "./NotAnswered.vue";
import { errorText, type DialogProps } from "./shared";

/**
 * A side question (B11, Claude Code's `/btw`): asked of the session beside its turn
 * (`side_question`), with what the conversation already holds and no tools; the answer is shown
 * here and never goes into the conversation. "Add to the chat" puts it into the composer, where
 * the user sends it or not. A question that came with the command (`/btw why…`) is asked at once.
 *
 * A panel from the chat's right edge, on the conversation's own ground, over it and never
 * narrowing it, the conversation dimmed under it (the user's word, 16 Nov: "dialog gibi değil,
 * sağdan açılan drawer", "oturum ekranını daraltmayacak", "zemin gene bu renk", "overlay basılınca
 * kapansın", then "dışarı tıklayınca kapatmaz, sadece küçültür"). Its questions and answers follow
 * one another and live in the chat's page memory (`ChatUi.btw`): × closes it and drops them; folded
 * away (–, Esc, a press on the dim) it keeps them, an answer still coming in too, and the status
 * line offers it again ("1 ara soru"). Its box is the composer's.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: []; insert: [text: string] }>();

useReturnFocus();

const ui = computed(() => chatModel(props.id).ui);
if (!ui.value.btw) ui.value.btw = { items: [], draft: "", count: 0 };
/** This chat's side questions; the panel opens on them as they were left. */
const panel = computed<SideQuestions>(() => ui.value.btw ?? { items: [], draft: "", count: 0 });
const items = computed(() => panel.value.items);
const question = computed({
  get: () => panel.value.draft,
  set: (text: string) => {
    if (ui.value.btw) ui.value.btw.draft = text;
  },
});
const arg = (props.arg ?? "").trim();
if (arg) question.value = arg;

const shown = ref(true);
const busy = computed(() => items.value.some((item) => item.busy));
const log = ref<HTMLElement | null>(null);
const field = ref<HTMLTextAreaElement | null>(null);

function toEnd(): void {
  void nextTick(() => {
    const box = log.value;
    if (box) box.scrollTop = box.scrollHeight;
  });
}

async function ask(): Promise<void> {
  const text = question.value.trim();
  const btw = ui.value.btw;
  if (!text || busy.value || !btw) return;
  btw.items.push({ n: ++btw.count, question: text, answer: null, none: false, failed: null, busy: true });
  // The list's own (reactive) row: written when the answer comes, whether the panel shows or not.
  const item: SideQuestion = btw.items[btw.items.length - 1];
  question.value = "";
  void nextTick(fit);
  toEnd();
  try {
    const reply = await api.chatControl<{ response?: string | null } | null>(props.id, { kind: "sideQuestion", text });
    const response = typeof reply?.response === "string" ? reply.response.trim() : "";
    if (response) item.answer = response;
    else item.none = true;
  } catch (reason) {
    item.failed = errorText(reason);
  } finally {
    item.busy = false;
    toEnd();
  }
}

/** The field grows with what is typed, up to its most, as the composer's does. */
const FIELD_MOST = 160;
function fit(): void {
  const el = field.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, FIELD_MOST)}px`;
}

/** Return asks, Shift+Return breaks the line, as in the composer; nothing while a word is composed. */
function onFieldKey(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.isComposing || event.shiftKey || event.altKey) return;
  event.preventDefault();
  void ask();
}

/** Esc anywhere in the panel folds it away, as a press outside it does, before the chat or the
 * panel hear it. */
function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape" || event.isComposing) return;
  event.preventDefault();
  event.stopPropagation();
  fold();
}

/** Its answer into the composer, and the panel folded away so the composer is in sight (the user's
 * word, 16 Nov: "sohbete ekle diyince minify etsin"). */
function add(item: SideQuestion): void {
  if (!item.answer) return;
  emit("insert", item.answer);
  fold();
}

/** Closed: its questions and answers go. */
function close(): void {
  ui.value.btw = null;
  shown.value = false;
}

/** Folded away into the status line, all of it kept, even with nothing asked yet (the user's word,
 * 16 Nov: a press outside folds it, never closes it). */
function fold(): void {
  shown.value = false;
}

onMounted(() => {
  field.value?.focus({ preventScroll: true });
  fit();
  toEnd();
  if (arg) void ask();
});


const placeholder = computed(() => (items.value.length ? t("chatDialog.btw.again") : t("chatDialog.btw.placeholder")));
</script>

<template>
  <Transition name="btw-dim" appear>
    <div v-if="shown" class="dim" aria-hidden="true" @pointerdown="fold"></div>
  </Transition>
  <Transition name="btw" appear @after-leave="emit('close')">
    <aside v-if="shown" class="btw" :aria-label="t('chatDialog.btw.title')" @keydown="onKey">
      <header class="head">
        <!-- Its name alone: the chat it stands in is the project's (the user's word, 16 Nov). -->
        <div class="title">{{ t("chatDialog.btw.title") }}</div>
        <button type="button" class="close" :aria-label="t('chatDialog.btw.minimize')" :title="t('chatDialog.btw.minimize')" @click="fold">
          <Icon name="minus" :size="13" />
        </button>
        <button type="button" class="close" :aria-label="t('common.close')" :title="t('common.close')" @click="close">
          <Icon name="close" :size="12" />
        </button>
      </header>

      <div ref="log" class="log">
        <p class="note">{{ t("chatDialog.btw.note") }}</p>
        <section v-for="item in items" :key="item.n" class="item">
          <p class="asked selectable">{{ item.question }}</p>
          <div v-if="item.busy" class="state" role="status"><Spinner :size="14" /></div>
          <NotAnswered v-else-if="item.failed" :id="id" :state="props.state" :error="item.failed" @done="close" />
          <p v-else-if="item.none" class="state">{{ t("chatDialog.btw.noAnswer") }}</p>
          <template v-else-if="item.answer">
            <div class="answer selectable"><Markdown :text="item.answer" /></div>
            <div class="tools">
              <button type="button" class="add" @click="add(item)"><Icon name="enter" :size="12" />{{ t("chatDialog.btw.add") }}</button>
            </div>
          </template>
        </section>
      </div>

      <div class="foot">
        <div class="box">
          <textarea
            ref="field"
            v-model="question"
            class="input"
            rows="1"
            spellcheck="true"
            :placeholder="placeholder"
            :aria-label="t('chatDialog.btw.title')"
            @input="fit"
            @keydown="onFieldKey"
          ></textarea>
          <div class="row">
            <button type="button" class="send" :disabled="!question.trim() || busy" :aria-busy="busy" :title="t('chatDialog.btw.ask')" :aria-label="t('chatDialog.btw.ask')" @click="ask">
              <Spinner v-if="busy" :size="12" />
              <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M5.5 11.5 12 5l6.5 6.5" /></svg>
            </button>
          </div>
        </div>
      </div>
    </aside>
  </Transition>
</template>

<style scoped>
/* The conversation dimmed under the panel; a press on it folds the panel away. */
.dim {
  position: absolute;
  inset: 0;
  z-index: 29;
  background: var(--scrim-soft);
}

.btw-dim-enter-active,
.btw-dim-leave-active {
  transition: opacity 0.22s ease;
}

.btw-dim-enter-from,
.btw-dim-leave-to {
  opacity: 0;
}

/* Over the conversation from its right edge, its whole height, on the conversation's own ground;
   the conversation keeps its width under it. Over the status line (20) and its card. */
.btw {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  z-index: 30;
  display: flex;
  flex-direction: column;
  width: min(400px, calc(100% - 40px));
  border-left: 1px solid var(--line-dialog);
  background: var(--bg-log);
  box-shadow: var(--shadow-dialog);
  user-select: none;
  -webkit-user-select: none;
}

.btw-enter-active,
.btw-leave-active {
  transition:
    transform 0.22s cubic-bezier(0.2, 0, 0, 1),
    opacity 0.22s cubic-bezier(0.2, 0, 0, 1);
}

.btw-enter-from,
.btw-leave-to {
  transform: translateX(24px);
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .btw-dim-enter-active,
  .btw-dim-leave-active,
  .btw-enter-active,
  .btw-leave-active {
    transition: none;
  }
}

.head {
  display: flex;
  align-items: center;
  flex: none;
  gap: 8px;
  padding: 10px 10px 10px 16px;
  border-bottom: 1px solid var(--line-soft);
}

.title {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-strong);
}


.close {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-muted);
}

.close:hover {
  color: var(--text);
}

.close:focus-visible,
.add:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
  border-radius: var(--radius-control);
}

.log {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 18px;
  min-height: 0;
  padding: 14px 16px 16px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.note {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.item {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* The question as the conversation draws the user's message. */
.asked {
  margin: 0;
  padding: 7px 11px;
  border: 1px solid var(--line-user);
  border-radius: 8px;
  background: var(--bg-user);
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-strong);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.state {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 48px;
  margin: 0;
  font-size: 12.5px;
  color: var(--text-subtle);
  text-align: center;
}

.answer {
  font-size: 13px;
  line-height: 1.55;
  color: var(--text);
}

.tools {
  display: flex;
  margin-top: -2px;
}

/* Words with no ground, as the card's buttons are. */
.add {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 6px;
  margin-left: -6px;
  border: 0;
  background: none;
  font: inherit;
  font-size: 12px;
  color: var(--text-subtle);
}

.add:hover {
  color: var(--text-strong);
}

/* The composer's box: the field, and the round button at its foot. */
.foot {
  flex: none;
  padding: 10px 12px 12px;
}

.box {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 10px 8px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: var(--bg-input);
}

.box:focus-within {
  border-color: var(--line-lit);
}

.input {
  box-sizing: border-box;
  display: block;
  width: 100%;
  min-height: 40px;
  max-height: 160px;
  margin: 0;
  padding: 2px 2px 2px 3px;
  border: none;
  outline: none;
  background: none;
  font-family: var(--font);
  font-size: 13.5px;
  line-height: 20px;
  color: var(--text-strong);
  resize: none;
  user-select: text;
  -webkit-user-select: text;
}

.input::placeholder {
  color: var(--text-faint);
}

.row {
  display: flex;
  justify-content: flex-end;
}

.send {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: var(--text-strong);
  color: var(--bg-app);
}

.send:hover:not(:disabled) {
  background: var(--text-lit);
}

.send:disabled {
  background: var(--bg-control-hover);
  color: var(--text-faint);
}
</style>
