<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import { mcpTool } from "../../lib/chat/cards";
import type { ChatCardEmits, ChatCardProps } from "../../lib/chat/ui";
import type { CanUseToolRequest, ChatAnswer, ElicitationRequest } from "../../lib/chat/wire";
import { t } from "../../lib/i18n";
import ElicitForm from "./ElicitForm.vue";
import PermissionCard from "./PermissionCard.vue";
import QuestionCard from "./QuestionCard.vue";

/**
 * One request of Claude Code's the user answers, drawn as its card (docs/SOHBET.md, Permissions,
 * questions, plans; B2–B4, B8): a tool's permission, a question (AskUserQuestion), an MCP server's
 * form, or a dialog the chat can't draw. Claude's plan (ExitPlanMode) has a screen of its own
 * (PlanScreen, PL5), never this card. The card is the frame:
 * its head folds it and counts the cards after it; what it asks is the inner card's. Nothing is
 * answered but by the user's click or key in this card.
 */
const props = defineProps<ChatCardProps>();
const emit = defineEmits<ChatCardEmits>();

type Kind = "permission" | "question" | "elicit" | "dialog";

const tool = computed<CanUseToolRequest | null>(() => (props.pending.request.subtype === "can_use_tool" ? props.pending.request : null));
const elicitation = computed<ElicitationRequest | null>(() => (props.pending.request.subtype === "elicitation" ? props.pending.request : null));

const kind = computed<Kind>(() => {
  if (tool.value) return tool.value.tool_name === "AskUserQuestion" ? "question" : "permission";
  return elicitation.value ? "elicit" : "dialog";
});

const questions = computed(() => {
  const list = tool.value?.input?.questions;
  return Array.isArray(list) ? list.length : 0;
});

const title = computed(() => {
  switch (kind.value) {
    case "permission": {
      const request = tool.value!;
      const mcp = mcpTool(request.tool_name);
      const name = request.display_name || (mcp ? `${mcp.server} · ${mcp.tool}` : request.tool_name);
      return t("chatInput.card.permission", { tool: name });
    }
    case "question":
      return t("chatInput.card.question", { count: Math.max(1, questions.value) });
    case "elicit":
      return t("chatInput.card.elicit", { server: elicitation.value?.mcp_server_name ?? "MCP" });
    default:
      return t("chatInput.card.dialog");
  }
});

const icon = computed(() => (kind.value === "permission" ? "shield" : kind.value === "question" ? "chat" : kind.value === "elicit" ? "server" : "info"));

const folded = ref(false);
const root = ref<HTMLElement | null>(null);
const inner = ref<{ key: (event: KeyboardEvent) => boolean } | null>(null);

/** The keyboard comes to a new card only from nowhere (the page itself) or from this chat's own
 * view, and not while a field of it holds words being written (the composer). A key meant for
 * another pane (a terminal's xterm, another chat's composer) never answers this card. */
function takeFocus(): void {
  const active = document.activeElement;
  if (active && active !== document.body && active !== document.documentElement) {
    const view = root.value?.closest(".chat-view");
    if (!view || !view.contains(active) || active.closest(".xterm")) return;
    const writing = (active instanceof HTMLTextAreaElement || active instanceof HTMLInputElement) && active.value.length > 0;
    if (writing) return;
  }
  root.value?.focus({ preventScroll: true });
}

onMounted(takeFocus);
watch(
  () => props.pending.requestId,
  () => {
    folded.value = false;
    takeFocus();
  },
);

function onKey(event: KeyboardEvent): void {
  if (folded.value || event.defaultPrevented) return;
  if (inner.value?.key(event)) {
    event.preventDefault();
    event.stopPropagation();
  }
}

function answer(value: ChatAnswer): void {
  emit("answer", value);
}

/** The card gives the keyboard back to its frame (a field closed with Esc). */
function refocus(): void {
  root.value?.focus({ preventScroll: true });
}
</script>

<template>
  <PermissionCard v-if="compact && tool && kind === 'permission'" :id="id" :key="pending.requestId" ref="inner" :path="path" :request="tool" compact @answer="answer" />
  <section v-else ref="root" :class="['cc', kind, { folded }]" tabindex="-1" :aria-label="title" @keydown="onKey">
    <button class="cc-head" type="button" :aria-expanded="!folded" @click="folded = !folded">
      <Icon :name="icon" :size="14" class="cc-icon" />
      <span class="cc-title">{{ title }}</span>
      <span v-if="more > 0" class="cc-more">{{ t("chatInput.card.more", { count: more }) }}</span>
      <Icon name="chevron-down" :size="13" :class="['cc-chev', { shut: folded }]" />
    </button>
    <Collapse :show="!folded" keep :fit="kind === 'question'">
      <div class="cc-body">
        <PermissionCard v-if="kind === 'permission' && tool" :id="id" :key="`p${pending.requestId}`" ref="inner" :path="path" :request="tool" :compact="false" @answer="answer" @refocus="refocus" />
        <QuestionCard v-else-if="kind === 'question' && tool" :key="`q${pending.requestId}`" ref="inner" :request="tool" @answer="answer" @refocus="refocus" />
        <ElicitForm v-else-if="kind === 'elicit' && elicitation" :key="`e${pending.requestId}`" ref="inner" :request="elicitation" @answer="answer" @refocus="refocus" />
        <template v-else>
          <p class="cc-text">{{ t("chatInput.card.dialogText") }}</p>
          <div class="cc-actions">
            <button type="button" class="cc-btn primary" @click="emit('terminal')"><Icon name="terminal" :size="12" />{{ t("chat.exit.terminal") }}</button>
          </div>
        </template>
      </div>
    </Collapse>
  </section>
</template>

<!-- The cards' common look, for this frame and the cards drawn in it (PermissionCard, QuestionCard,
     ElicitForm) and for the plan's screen (PlanScreen): not scoped, every class under `cc-`. -->
<style>
/* On the panel's own ground, Claude's colour in its edge and head alone (the user's word, 16 Nov:
   grey rows on an orange ground read badly). */
.cc {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--claude-line);
  border-radius: 10px;
  background: var(--bg-panel);
  color: var(--text);
  outline: none;
}

.cc.elicit,
.cc.dialog {
  border-color: var(--line-strong);
}

/* A question's card keeps its head, its tabs and its buttons in sight; what it asks scrolls between
   them (QuestionCard's room; the user's word, 25 Nov). It takes no more than the room over the
   composer leaves it (the to-do list giving its own up first), so that room never scrolls along
   with it (the user's word, 26 Nov: a second scroll bar moved it by a pixel or two). Its least
   height is its head, tabs and buttons alone: only a room smaller than those scrolls, the card's
   own then shut. That least height eases with its fold, so it never stands taller than what is
   shown. */
.cc.question {
  min-height: 0;
  transition: min-height 0.2s cubic-bezier(0.2, 0, 0, 1);
}

/* The head (36), the buttons (28), the gap and the padding under them (10, 12) and the edge. */
.cc.question:not(.folded) {
  min-height: 88px;
}

/* And the tabs' row (26) with its gap. */
.cc.question:not(.folded):has(.tabs) {
  min-height: 124px;
}

.cc.question > .cc-head {
  flex-shrink: 0;
}

.cc.question .cc-body {
  min-height: 0;
}

@media (prefers-reduced-motion: reduce) {
  .cc.question {
    transition: none;
  }
}

.cc-head {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 12px;
  border: none;
  border-radius: 10px;
  background: transparent;
  text-align: left;
  color: var(--claude-text);
}

.cc.elicit .cc-head,
.cc.dialog .cc-head {
  color: var(--text-strong);
}

.cc-icon {
  flex-shrink: 0;
}

.cc-title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-weight: 600;
}

.cc-more {
  flex-shrink: 0;
  height: 18px;
  padding: 0 7px;
  border-radius: 9px;
  background: var(--bg-tag);
  font-size: 11px;
  line-height: 18px;
  color: var(--text-muted);
}

.cc-chev {
  flex-shrink: 0;
  margin-left: auto;
  color: var(--text-muted);
  transition: transform 0.12s;
}

.cc-chev.shut {
  transform: rotate(-90deg);
}

.cc-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 0 12px 12px;
  min-width: 0;
}

.cc-text,
.cc-note,
.cc-lead {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.cc-note {
  font-size: 12.5px;
  color: var(--text-muted);
}

.cc-lead {
  color: var(--text-strong);
}

.cc-code {
  max-height: 220px;
  margin: 0;
  padding: 8px 10px;
  overflow: auto;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-strong);
}

.cc-code.small {
  font-size: 11.5px;
  color: var(--text-log);
}

.cc-file {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-muted);
}

.cc-path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-soft);
}

.cc-file .cc-link {
  margin-left: auto;
}

.cc-link {
  flex-shrink: 0;
  height: 22px;
  padding: 0 7px;
  border: none;
  border-radius: 5px;
  background: transparent;
  font-size: 12px;
  color: var(--text-nav);
}

.cc-link:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.cc-diff {
  max-height: 300px;
  overflow: auto;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 19px;
}

.cc-diff .row {
  display: flex;
  min-width: max-content;
  padding-right: 10px;
  white-space: pre;
  color: var(--text-log);
}

.cc-diff .row.add {
  background: var(--bg-diff-add);
  color: var(--text-strong);
}

.cc-diff .row.del {
  background: var(--bg-diff-del);
  color: var(--text-strong);
}

.cc-diff .mk {
  flex-shrink: 0;
  width: 22px;
  text-align: center;
  color: var(--text-faint);
}

.cc-diff .row.add .mk {
  color: var(--btn-start-text);
}

.cc-diff .row.del .mk {
  color: var(--btn-stop-text);
}

.cc-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.cc-actions .cc-gap {
  flex: 1;
}

.cc-btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 28px;
  padding: 0 11px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
  color: var(--text);
}

.cc-btn:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.cc-btn:disabled {
  opacity: 0.55;
}

.cc-btn.primary {
  border-color: var(--btn-primary-line);
  background: var(--btn-primary-bg);
  color: var(--btn-primary-text);
}

.cc-btn.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.cc-btn.quiet {
  border-color: transparent;
  background: transparent;
  color: var(--text-muted);
}

.cc-btn.quiet:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* The choice the keys rest on (1/2/3, ↑/↓), ringed as the keyboard's place is: only while the
   card has the keyboard, where Enter would answer with it. */
.cc:focus-within .cc-btn.on {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}

.cc-btn.sm {
  height: 24px;
  padding: 0 9px;
  font-size: 12px;
}

.cc-k {
  min-width: 14px;
  height: 16px;
  padding: 0 3px;
  border-radius: 4px;
  background: var(--bg-key);
  font-family: var(--font-mono);
  font-size: 10.5px;
  line-height: 16px;
  text-align: center;
  color: var(--text-muted);
}

.cc-input,
.cc-textarea,
.cc-select {
  box-sizing: border-box;
  width: 100%;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  font: inherit;
  font-size: 13px;
  color: var(--text-strong);
  outline: none;
}

.cc-input,
.cc-select {
  height: 30px;
  padding: 0 9px;
}

.cc-textarea {
  min-height: 64px;
  max-height: 300px;
  padding: 7px 9px;
  resize: vertical;
  line-height: 1.5;
}

.cc-textarea.mono {
  min-height: 140px;
  font-family: var(--font-mono);
  font-size: 12px;
}

.cc-input:focus,
.cc-textarea:focus,
.cc-select:focus {
  border-color: var(--line-lit);
}

.cc-tell {
  display: flex;
  gap: 6px;
}

.cc-label {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-faint);
}

:lang(zh) .cc-label,
:lang(ja) .cc-label {
  letter-spacing: 0;
}

.cc-seg {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-control);
  background: var(--bg-seg);
}

.cc-seg button {
  height: 24px;
  padding: 0 9px;
  border: none;
  border-radius: 5px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.cc-seg button.on {
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
  color: var(--text-strong);
}

.cc-tags {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 5px;
}

.cc-tag {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--bg-tag);
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-soft);
}

/* A plan's or a preview's Markdown (lib/markdownPreview), drawn small inside a card or on the plan's screen. */
.cc-md {
  font-size: 13px;
  line-height: 1.6;
  color: var(--text);
  overflow-wrap: break-word;
}

.cc-md > :first-child {
  margin-top: 0;
}

.cc-md > :last-child {
  margin-bottom: 0;
}

.cc-md h1,
.cc-md h2,
.cc-md h3,
.cc-md h4,
.cc-md h5,
.cc-md h6 {
  margin: 1.1em 0 0.45em;
  font-size: 1.05em;
  font-weight: 600;
  line-height: 1.35;
  color: var(--text-strong);
}

.cc-md h1 {
  font-size: 1.25em;
}

.cc-md h2 {
  font-size: 1.15em;
}

.cc-md p,
.cc-md ul,
.cc-md ol,
.cc-md blockquote,
.cc-md pre,
.cc-md table {
  margin: 0 0 0.75em;
}

.cc-md ul,
.cc-md ol {
  padding-left: 1.6em;
}

.cc-md li + li {
  margin-top: 0.2em;
}

.cc-md code {
  padding: 0.1em 0.35em;
  border-radius: 4px;
  background: var(--bg-tag);
  font-family: var(--font-mono);
  font-size: 0.9em;
}

.cc-md pre {
  padding: 8px 10px;
  overflow: auto;
  border-radius: 6px;
  background: var(--bg-log);
}

.cc-md pre code {
  padding: 0;
  background: none;
}

.cc-md blockquote {
  padding-left: 10px;
  border-left: 2px solid var(--line-strong);
  color: var(--text-muted);
}

.cc-md table {
  border-collapse: collapse;
}

.cc-md th,
.cc-md td {
  padding: 4px 8px;
  border: 1px solid var(--line-strong);
}

.cc-md [data-href] {
  color: var(--text-link);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
}

.cc-md .md-check {
  display: inline-block;
  width: 11px;
  height: 11px;
  margin-right: 6px;
  border: 1.5px solid var(--line-lit);
  border-radius: 3px;
  vertical-align: -1px;
}

.cc-md .md-check.done {
  background: var(--text-soft);
}
</style>
