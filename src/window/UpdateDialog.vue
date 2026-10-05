<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { useBackdropClose } from "../lib/dialog";
import { useReturnFocus } from "../lib/dialogFocus";
import { updateError } from "../lib/format";
import { t } from "../lib/i18n";
import { api } from "../lib/store";
import type { RestartPlan, UpdateView } from "../lib/types";

/**
 * Names the new version and asks before the restart that installs it. What the restart would do to
 * what is open is asked of the core as the dialog opens: with nothing cut off it is two lines, else
 * it lists what is cut off for good and what comes back. Restart installs; Later, Esc or a click
 * outside leaves it. An install that can't replace itself gets the way to the download page instead.
 */
const props = defineProps<{ update: UpdateView }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const laterButton = ref<HTMLButtonElement | null>(null);

// What the dialog is about, as it opened: the state moves on under it once the install starts.
const version = props.update.version ?? "";
const current = props.update.current;
const onQuit = props.update.onQuit;
/** A new version this install can't put in place itself: no restart to ask about. */
const manual = props.update.status === "available";
const title = computed(() => t(manual ? "update.dialog.available" : "update.dialog.ready", { version }));

/** Null until the core answers: the text's room is kept and Restart waits for it. */
const plan = ref<RestartPlan | null>(null);
const installing = ref(false);
const error = ref("");
/** Beside the buttons: Later installs it too, at the quit. Not once an install has failed: nothing
 * waits for the quit then. */
const note = computed(() => (onQuit && !manual && !error.value ? t("update.dialog.onQuit") : ""));

onMounted(() => {
  dialog.value?.showModal();
  // Later is the safe answer, and the one Return gives.
  laterButton.value?.focus();
  // A core that doesn't answer leaves Restart waiting: nothing restarts on a promise not checked.
  if (!manual) void api.updatePlan().then((answer) => (plan.value = answer), () => undefined);
});

type Row = { mark: "claude" | "terminal" | "server"; working?: boolean; name: string; sub: string; outcome: string };

/** Cut off for good: a Claude turn at work, a command running in a shell tab or on its own. */
const cut = computed<Row[]>(() =>
  (plan.value?.cut ?? []).map((entry) => {
    const name = `${entry.project} · ${entry.name}`;
    return entry.kind === "claude"
      ? { mark: "claude", working: true, name, sub: t("update.cut.claude"), outcome: t("update.cut.claudeOutcome") }
      : { mark: "terminal", name, sub: t("update.cut.command"), outcome: t("update.cut.commandOutcome") };
  }),
);

/** Comes back: a line each for the servers, Claude's sessions and the tabs, where there are any. */
const back = computed<Row[]>(() => {
  const rows: Row[] = [];
  if (!plan.value) return rows;
  const { servers, claude, terminals, terminalProjects } = plan.value;
  if (servers.length) rows.push({ mark: "server", name: t("update.back.servers", { count: servers.length }), sub: servers.join(", "), outcome: t("update.back.serversOutcome", { count: servers.length }) });
  // One entry a session: a project with two of them is named once.
  if (claude.length) rows.push({ mark: "claude", name: t("update.back.claude", { count: claude.length }), sub: [...new Set(claude)].join(", "), outcome: t("update.back.claudeOutcome", { count: claude.length }) });
  if (terminals) rows.push({ mark: "terminal", name: t("update.back.terminals", { count: terminals }), sub: t("update.back.terminalsIn", { count: terminalProjects }), outcome: t("update.back.terminalsOutcome", { count: terminals }) });
  return rows;
});

/** With nothing cut off there is nothing to weigh: no lists at all. */
const groups = computed(() =>
  cut.value.length ? [{ label: t("update.dialog.cut"), rows: cut.value }, { label: t("update.dialog.back"), rows: back.value }].filter((group) => group.rows.length) : [],
);

/** When it works the app goes away under the dialog, so only a failure ever comes back. */
async function install(): Promise<void> {
  if (installing.value) return;
  installing.value = true;
  error.value = "";
  try {
    await api.updateInstall();
  } catch (failure) {
    error.value = updateError(typeof failure === "string" ? failure : null);
    installing.value = false;
  }
}

function openPage(): void {
  void api.openLink("releases");
  dialog.value?.close();
}

/** While it installs nothing closes the dialog: not Esc, not a click outside. */
function onCancel(event: Event): void {
  if (installing.value) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!installing.value) backdrop.click(event);
}
</script>

<template>
  <dialog ref="dialog" :class="['dialog', { wide: groups.length }]" :aria-label="title" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="onBackdrop">
    <div class="body">
      <div class="head">
        <div class="title">{{ title }}</div>
        <p class="text">
          <template v-if="manual">{{ t("update.dialog.textAvailable") }}</template>
          <template v-else-if="plan">
            {{ cut.length ? t("update.dialog.textCut", { current, count: cut.length }) : t("update.dialog.text", { current }) }}
            <button type="button" class="notes" @click="api.openLink('releases')">{{ t("update.notes") }}</button>
          </template>
        </p>
      </div>

      <div v-for="group in groups" :key="group.label" class="group" role="group" :aria-label="group.label">
        <div class="section-label group-label">{{ group.label }}</div>
        <div v-for="(row, index) in group.rows" :key="index" class="row">
          <span class="mark">
            <ClaudeLogo v-if="row.mark === 'claude'" :size="14" :color="row.working ? 'var(--claude)' : 'currentColor'" />
            <Icon v-else :name="row.mark" />
          </span>
          <span class="what">
            <span class="name">{{ row.name }}</span>
            <span class="sub">{{ row.sub }}</span>
          </span>
          <span class="outcome">{{ row.outcome }}</span>
        </div>
      </div>

      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <div class="actions">
        <!-- Always there, so the buttons keep to the right. -->
        <span class="note" :title="note || undefined">{{ note }}</span>
        <button ref="laterButton" type="button" class="control" :disabled="installing" @click="dialog?.close()">{{ t("update.later") }}</button>
        <button v-if="manual" type="button" class="control primary" @click="openPage">{{ t("update.openPage") }}</button>
        <button v-else type="button" class="control primary" :disabled="installing || !plan || update.status !== 'ready'" :aria-busy="installing" @click="install">
          <!-- The label keeps its room under the spinner: the button never changes width. -->
          <span :class="{ hidden: installing }">{{ t("common.restart") }}</span>
          <Spinner v-if="installing" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the command dialog; wider when it lists what a restart does. */
.dialog {
  width: 400px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid #2f343d;
  border-radius: 12px;
  background: #1b1e24;
  color: var(--text);
  font-size: 13px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
}

.dialog.wide {
  width: 500px;
}

.dialog[open] {
  animation: pop 0.16s ease-out;
}

.dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.head {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

/* Two lines' room from the start, so the text coming in moves nothing. */
.text {
  margin: 0;
  min-height: 36px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-muted);
}

/* A link in the text's flow: the release notes. */
.notes {
  padding: 0;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--text);
  text-decoration: underline;
  text-decoration-color: var(--text-subtle);
  text-underline-offset: 3px;
}

.notes:hover {
  color: var(--text-strong);
  text-decoration-color: currentColor;
}

.group {
  display: flex;
  flex-direction: column;
}

.group-label {
  padding-bottom: 4px;
}

.row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 9px 0;
}

.row + .row {
  border-top: 1px solid var(--line-strong);
}

/* As tall as the name's line, so the mark sits on it. */
.mark {
  width: 16px;
  height: 18px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
}

.what {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.name {
  font-weight: 500;
  color: var(--text-strong);
}

.name,
.sub {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sub {
  font-size: 12px;
  color: var(--text-subtle);
}

.outcome {
  width: 176px;
  flex-shrink: 0;
  font-size: 12px;
  line-height: 1.45;
  color: #c7ccd3;
  text-align: right;
}

.error {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--crash-text);
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* Gives way to the buttons beside it. */
.note {
  flex-grow: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.control {
  position: relative;
  flex-shrink: 0;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover:not(:disabled) {
  background: #2c3039;
}

.control.primary {
  background: #2a3a30;
  border-color: #3d5446;
  color: #cdebd8;
}

.control.primary:hover:not(:disabled) {
  background: #33473a;
}

.control:disabled {
  opacity: 0.5;
}

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
  .dialog[open] {
    animation: none;
  }
}
</style>
