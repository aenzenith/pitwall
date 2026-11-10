<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { accountName, askLoginEnds, listenRenew } from "../../lib/accounts";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { AccountRenew, ClaudeAccount } from "../../lib/types";

/**
 * "Renew sign-in" of a Claude account, the main one too: Claude Code's own sign-in again
 * (`account_renew`, `claude auth login` with the account's folder), in the browser, its progress
 * as `account-renew` events. Done, its sign-in's end is asked again and the dialog closes; one that
 * didn't complete changes nothing and can be tried again. Esc or Cancel while it waits stops it
 * (`account_login_cancel`, the sign-in under way whichever it is).
 */
const props = defineProps<{ account: ClaudeAccount }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();

const step = ref<"waiting" | "failed">("waiting");
const url = ref<string | null>(null);
const failure = ref<Extract<AccountRenew, { state: "failed" }> | null>(null);
/** Why the core turned it down before it began (another sign-in going on, the folder gone). */
const error = ref("");
/** A sign-in of ours is under way: its events are heard, and closing stops it. */
let asked = false;
let gone = false;

const name = accountName(props.account);

function onRenew(event: AccountRenew): void {
  if (!asked) return;
  switch (event.state) {
    case "waiting":
      if (event.url) url.value = event.url;
      break;
    case "done":
      asked = false;
      void askLoginEnds();
      dialog.value?.close();
      break;
    case "failed":
      asked = false;
      failure.value = event;
      show("failed");
      break;
  }
}

// Listening before the sign-in starts: none of its events is missed.
const listening = listenRenew(onRenew);

function show(next: typeof step.value): void {
  step.value = next;
  void nextTick(() => dialog.value?.focus({ preventScroll: true }));
}

async function renew(): Promise<void> {
  await listening.catch(() => undefined);
  if (gone) return;
  url.value = null;
  failure.value = null;
  error.value = "";
  show("waiting");
  asked = true;
  try {
    await api.accountRenew(props.account.id);
  } catch (reason) {
    asked = false;
    error.value = String(reason);
    show("failed");
  }
}

function cancel(): void {
  if (asked) {
    asked = false;
    void api.accountLoginCancel().catch(() => undefined);
  }
  dialog.value?.close();
}

function openBrowser(): void {
  if (url.value) void api.openUrl(url.value).catch((reason) => (error.value = String(reason)));
}

function failText(): string {
  if (error.value) return error.value;
  return t(failure.value?.reason === "cli" ? "accounts.renew.failedCli" : "accounts.renew.failedClosed", { name });
}

onMounted(() => {
  dialog.value?.showModal();
  if (dialog.value && !dialog.value.hasAttribute("tabindex")) dialog.value.tabIndex = -1;
  void renew();
});

onBeforeUnmount(() => {
  gone = true;
  void listening.then((unlisten) => unlisten(), () => undefined);
  // Gone while the sign-in waits: it stops, and nothing changes.
  if (asked) {
    asked = false;
    void api.accountLoginCancel().catch(() => undefined);
  }
});

/** Esc while it waits stops the sign-in. */
function onCancel(event: Event): void {
  if (step.value === "waiting") {
    event.preventDefault();
    cancel();
  }
}

/** A click outside closes it once it no longer waits on anything. */
function onBackdrop(event: MouseEvent): void {
  if (step.value !== "waiting") backdrop.click(event);
}

/** Return on the dialog itself tries again. */
function onKey(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.repeat || event.isComposing || event.target !== dialog.value) return;
  if (step.value === "failed") {
    event.preventDefault();
    void renew();
  }
}
</script>

<template>
  <dialog ref="dialog" class="renew-dialog" :aria-label="t('accounts.renew.title', { name })" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="onBackdrop" @keydown="onKey">
    <div v-if="step === 'waiting'" class="body">
      <div class="title">{{ t("accounts.renew.title", { name }) }}</div>
      <div class="waiting" role="status"><Spinner :size="16" />{{ t("accounts.add.waiting") }}</div>
      <p class="note">{{ t("accounts.renew.hint") }}</p>
      <div class="actions">
        <button v-if="url" type="button" class="link" @click="openBrowser">{{ t("accounts.add.openBrowser") }}</button>
        <span class="grow"></span>
        <button type="button" class="control" @click="cancel">{{ t("common.cancel") }}</button>
      </div>
    </div>

    <div v-else class="body">
      <div class="title">{{ t("accounts.add.failedTitle") }}</div>
      <p class="text" role="alert">{{ failText() }}</p>
      <div class="actions">
        <button type="button" class="control" @click="dialog?.close()">{{ t("common.close") }}</button>
        <button type="button" class="control primary" @click="renew">{{ t("accounts.retry") }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the add dialog's sign-in step. */
.renew-dialog {
  width: 400px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
}

.renew-dialog[open] {
  animation: pop 0.16s ease-out;
}

/* Resting on the dialog itself shows no ring. */
.renew-dialog:focus-visible {
  outline: none;
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.waiting {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 0 2px;
  font-size: 14px;
}

.waiting > .spinner {
  color: var(--text-muted);
}

.note {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.text {
  margin: 0;
  line-height: 1.5;
  color: var(--text-soft);
  overflow-wrap: anywhere;
}

.actions {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}

.grow {
  flex-grow: 1;
}

.control {
  position: relative;
  flex-shrink: 0;
  height: 30px;
  padding: 0 14px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
  white-space: nowrap;
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

/* A web address in the browser: a link's look. */
.link {
  height: 30px;
  padding: 0 4px;
  border: 0;
  background: transparent;
  color: var(--text-link);
  white-space: nowrap;
}

.link:hover {
  text-decoration: underline;
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .renew-dialog[open] {
    animation: none;
  }
}
</style>
