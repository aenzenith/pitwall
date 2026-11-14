<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { chatAskDone, type ChatAsk } from "../../lib/chat/open";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { originView } from "../../lib/sessions";
import { api, loadSessions, sessions, snapshot } from "../../lib/store";

/**
 * What a chat's start waits on (lib/chat/open: `chatAsks`), one at a time over the window:
 *
 * - `trust`: the project's first chat (O6). A chat runs Claude Code there with the project's own
 *   hooks and MCP servers; Allow tells the core (`chat_trust`, a spinner meanwhile) and the start
 *   goes on; Cancel, Esc or a click outside opens nothing.
 * - `elsewhere`: the session runs somewhere else (as the sessions' list knows it), so it isn't
 *   opened a second time; "Oraya git" brings it up where it runs.
 * - `failed`: the chat didn't start, and why, as the core said it.
 */
const props = defineProps<{ ask: ChatAsk }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const mainButton = ref<HTMLButtonElement | null>(null);
const busy = ref(false);
const error = ref("");
/** The trust question's answer, once given: a close after it is no "no". */
let answered = false;

const projectName = computed(() => {
  const path = props.ask.path;
  return snapshot.value?.projects.find((p) => p.path === path)?.name ?? path.split(/[\\/]/).filter(Boolean).pop() ?? path;
});

/** Where the session runs, as the Sessions page names it; null while that isn't known. */
const where = computed(() => {
  if (props.ask.kind !== "elsewhere") return null;
  const id = props.ask.session;
  const row = sessions.value?.sessions.find((each) => each.id === id);
  if (!row || row.origin.kind === "unknown") return null;
  return originView(row.origin, row.project, snapshot.value?.settings.editor, row.agent).long;
});

onMounted(() => {
  // The sessions' list comes only once a page that shows it has asked for it.
  if (props.ask.kind === "elsewhere" && !sessions.value) void loadSessions();
  // Return gives the dialog's own answer: Allow (the chat was asked for), go there, or OK.
  openUnfocused(dialog.value, () => mainButton.value);
});

async function allow(): Promise<void> {
  if (props.ask.kind !== "trust" || busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await api.chatTrust(props.ask.path);
    answered = true;
    props.ask.done(true);
    dialog.value?.close();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

function goThere(): void {
  if (props.ask.kind === "elsewhere") void api.revealClaude(props.ask.path, props.ask.session).catch(() => undefined);
  dialog.value?.close();
}

/** Esc, Cancel, a click outside and every answer end here, through the dialog's own close. */
function onClose(): void {
  if (props.ask.kind === "trust" && !answered) props.ask.done(false);
  chatAskDone(props.ask);
}

/** While the core takes the answer, nothing closes it. */
function onCancel(event: Event): void {
  if (busy.value) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!busy.value) backdrop.click(event);
}

const title = computed(() => {
  switch (props.ask.kind) {
    case "trust":
      return t("chatSurface.trust.title", { project: projectName.value });
    case "elsewhere":
      return t("chatSurface.elsewhere.title");
    default:
      return t("chatSurface.failed.title");
  }
});
</script>

<template>
  <dialog ref="dialog" class="start-chat" :aria-label="title" @cancel="onCancel" @close="onClose" @pointerdown="backdrop.down" @click="onBackdrop">
    <div class="body">
      <div class="title">{{ title }}</div>

      <template v-if="ask.kind === 'trust'">
        <p class="text">{{ t("chatSurface.trust.body") }}</p>
        <p class="note">{{ t("chatSurface.trust.once") }}</p>
      </template>

      <template v-else-if="ask.kind === 'elsewhere'">
        <div v-if="where" class="where"><Icon name="arrow-up-right" :size="12" />{{ where }}</div>
        <p class="text">{{ t("chatSurface.elsewhere.body") }}</p>
      </template>

      <template v-else>
        <p class="text">{{ t("chatSurface.failed.body") }}</p>
        <p v-if="ask.text" class="reason">{{ ask.text }}</p>
      </template>

      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <div class="actions">
        <template v-if="ask.kind === 'trust'">
          <button type="button" class="control" :disabled="busy" @click="dialog?.close()">{{ t("common.cancel") }}</button>
          <button ref="mainButton" type="button" class="control primary" :disabled="busy" :aria-busy="busy" @click="allow">
            <span :class="{ hidden: busy }">{{ t("chatSurface.trust.allow") }}</span>
            <Spinner v-if="busy" class="over" :size="12" />
          </button>
        </template>
        <template v-else-if="ask.kind === 'elsewhere'">
          <button type="button" class="control" @click="dialog?.close()">{{ t("common.close") }}</button>
          <button ref="mainButton" type="button" class="control primary" @click="goThere">{{ t("chatSurface.elsewhere.go") }}</button>
        </template>
        <button v-else ref="mainButton" type="button" class="control" @click="dialog?.close()">{{ t("common.close") }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The app's small question dialog (settings/AccountRemoveDialog, team/TeamDeleteDialog). */
.start-chat {
  width: 380px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
  -webkit-user-select: none;
  user-select: none;
}

.start-chat[open] {
  animation: pop 0.16s ease-out;
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.text {
  margin: 0;
  line-height: 1.5;
  color: var(--text-soft);
}

.note {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

/* Where the session runs: a line of its own, as the Sessions page names the place. */
.where {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.where svg {
  flex-shrink: 0;
}

/* What the core said, as it said it: the user may need to copy it. */
.reason {
  margin: 0;
  padding: 8px 10px;
  border-radius: var(--radius-control);
  background: var(--bg-well);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-soft);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 160px;
  overflow-y: auto;
  -webkit-user-select: text;
  user-select: text;
}

.error {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}

.control {
  position: relative;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.control:disabled {
  opacity: 0.5;
}

/* A busy button's label stays, unseen, under its spinner: the button never changes width. */
.hidden {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .start-chat[open] {
    animation: none;
  }
}
</style>
