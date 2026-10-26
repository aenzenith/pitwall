<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { accountById, accountName, listenLogin, switchAccount } from "../../lib/accounts";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { isMac } from "../../lib/platform";
import { api } from "../../lib/store";
import type { AccountLogin } from "../../lib/types";

/**
 * Adding a Claude account, in three steps. Its name and folder (a new one named after it, shown
 * as the name is typed, or one picked), and whether it uses the main account's settings; then
 * Claude Code's own sign-in in the browser (`account_login`, its progress as `account-login`
 * events); then what Pitwall set up for it, with whether it syncs by itself when switched to and
 * whether to switch to it now (applied by OK). A sign-in that didn't complete adds nothing and can
 * be tried again. Esc or Cancel while it waits stops the sign-in (`account_login_cancel`).
 */
const emit = defineEmits<{ close: [] }>();

type Done = Extract<AccountLogin, { state: "done" }>;
type Failed = Extract<AccountLogin, { state: "failed" }>;

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const input = ref<HTMLInputElement | null>(null);

const step = ref<"form" | "waiting" | "done" | "failed">("form");

/* ---------- step 1: name, folder, settings ---------- */

const name = ref("");
const folderMode = ref<"new" | "pick">("new");
/** The folder a new account of the name typed gets (`account_folder`). */
const suggested = ref<{ shown: string; exists: boolean } | null>(null);
/** The one picked in the system's dialog (`account_pick_folder`). */
const picked = ref<{ path: string; shown: string } | null>(null);
const shared = ref(true);
const picking = ref(false);
/** The sign-in is being started: its button's spinner. */
const busy = ref(false);
const error = ref("");

const shownFolder = computed(() => (folderMode.value === "pick" ? (picked.value?.shown ?? "") : (suggested.value?.shown ?? "")));
const ready = computed(() => !!name.value.trim() && (folderMode.value === "new" || !!picked.value));

// The new folder follows the name as it is typed; an answer to an older name is dropped.
let folderAsk = 0;
let folderTimer: number | undefined;
watch(name, (value) => {
  window.clearTimeout(folderTimer);
  const mine = ++folderAsk;
  const typed = value.trim();
  if (!typed) {
    suggested.value = null;
    return;
  }
  folderTimer = window.setTimeout(() => {
    api.accountFolder(typed).then(
      (folder) => mine === folderAsk && (suggested.value = folder),
      () => mine === folderAsk && (suggested.value = null),
    );
  }, 150);
});

async function pickFolder(): Promise<void> {
  if (picking.value || busy.value) return;
  picking.value = true;
  error.value = "";
  try {
    const chosen = await api.accountPickFolder();
    // Cancelled: as it was.
    if (chosen) {
      picked.value = chosen;
      folderMode.value = "pick";
    }
  } catch (reason) {
    error.value = String(reason);
  } finally {
    picking.value = false;
  }
}

/* ---------- step 2: the sign-in ---------- */

/** The name the sign-in was started with: the later steps say it. */
const started = ref("");
/** The sign-in page, once Claude printed it: "Open in browser". */
const url = ref<string | null>(null);
/** A sign-in of ours is under way: its events are heard, and closing stops it. */
let asked = false;
/** Events heard so far: the answer to `account_login` knows whether one came before it. */
let heard = 0;

const added = ref<Done | null>(null);
const failure = ref<Failed | null>(null);
/** "Sync by itself when switching" as the account was added, and as it is set now. */
let syncWas = false;
const autoSync = ref(false);
const switchNow = ref(true);
const applying = ref(false);

function onLogin(event: AccountLogin): void {
  if (!asked) return;
  heard++;
  switch (event.state) {
    case "waiting":
      if (event.url) url.value = event.url;
      show("waiting");
      break;
    case "done":
      asked = false;
      added.value = event;
      // The core may send the new account's fuel a moment later: until then, what was asked for.
      syncWas = accountById(event.id)?.sync ?? true;
      autoSync.value = syncWas;
      switchNow.value = true;
      show("done");
      break;
    case "failed":
      asked = false;
      failure.value = event;
      show("failed");
      break;
  }
}

// Listening before the first sign-in can start: none of its events is missed.
const listening = listenLogin(onLogin);
let gone = false;

/** Another step: the keyboard rests on the dialog (Return gives the step's answer, `onKey`). */
function show(next: typeof step.value): void {
  step.value = next;
  error.value = "";
  void nextTick(() => dialog.value?.focus({ preventScroll: true }));
}

async function login(): Promise<void> {
  const typed = name.value.trim();
  if (busy.value || !ready.value) return;
  busy.value = true;
  error.value = "";
  url.value = null;
  await listening.catch(() => undefined);
  if (gone) return;
  started.value = typed;
  asked = true;
  const before = heard;
  try {
    // Its servers are brought level when it is switched to unless the user turns that off at
    // the end, whether or not it shares the main account's settings (servers aren't settings).
    await api.accountLogin(typed, folderMode.value === "pick" ? (picked.value?.path ?? null) : null, shared.value, true);
    if (asked && heard === before) show("waiting");
  } catch (reason) {
    const ours = asked;
    asked = false;
    if (heard === before) {
      // Turned down before it began (a name or folder that can't be): back to the form, saying why.
      if (step.value !== "form") show("form");
      error.value = String(reason);
      void nextTick(() => input.value?.focus());
    } else if (ours) {
      failure.value = { state: "failed", reason: "error", error: String(reason) };
      show("failed");
    }
  } finally {
    busy.value = false;
  }
}

/** Stops the sign-in under way, if any, and closes: nothing is added. */
function cancelLogin(): void {
  if (asked) {
    asked = false;
    void api.accountLoginCancel().catch(() => undefined);
  }
  dialog.value?.close();
}

function openBrowser(): void {
  if (url.value) void api.openUrl(url.value).catch((reason) => (error.value = String(reason)));
}

/* ---------- step 3: added ---------- */

const addedAccount = computed(() => accountById(added.value?.id));
const addedName = computed(() => (addedAccount.value ? accountName(addedAccount.value) : started.value));
const plan = computed(() => addedAccount.value?.limits.plan ?? null);

/** What Pitwall set up for it: done, to be done in Claude (`todo`), or what failed. */
type Check = { kind: "ok" | "todo" | "failed"; text: string };

const checks = computed<Check[]>(() => {
  const done = added.value;
  if (!done) return [];
  const list: Check[] = [];
  if (done.hooks === "shared") list.push({ kind: "ok", text: t("accounts.add.hooksShared") });
  else if (done.hooks === "installed") list.push({ kind: "ok", text: t("accounts.add.hooksInstalled") });
  else if (done.hooks === "failed") list.push({ kind: "failed", text: t("accounts.add.hooksFailed") });
  // The MCP servers as they stand: nothing is copied at the adding, a switch to it syncs them.
  if (done.mcp) {
    const servers = done.mcp.servers;
    const named = (...states: string[]): string => servers.filter((server) => states.includes(server.state)).map((server) => server.name).join(", ");
    const level = named("synced", "same");
    const login = named("login");
    const missing = named("missing");
    if (level) list.push({ kind: "ok", text: t("accounts.add.mcpSynced", { names: level }) });
    if (login) list.push({ kind: "todo", text: t("accounts.add.mcpLogin", { names: login }) });
    if (missing && autoSync.value) list.push({ kind: "todo", text: t("accounts.add.mcpOnSwitch", { names: missing }) });
  }
  if (done.radio === "added") list.push({ kind: "ok", text: t("radio.title") });
  else if (done.radio === "failed") list.push({ kind: "failed", text: t("accounts.add.radioFailed") });
  return list;
});

const CHECK_ICONS = { ok: "check", todo: "info", failed: "close" } as const;

/** OK: the switches as set, then closed. */
async function finish(): Promise<void> {
  const done = added.value;
  if (applying.value || !done) return;
  applying.value = true;
  error.value = "";
  try {
    if (autoSync.value !== syncWas) {
      await api.accountSetSync(done.id, autoSync.value);
      syncWas = autoSync.value;
    }
    if (switchNow.value) await switchAccount(done.id);
    dialog.value?.close();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    applying.value = false;
  }
}

const failText = computed(() => {
  const name = started.value;
  switch (failure.value?.reason) {
    case "closed":
      return t("accounts.add.failedClosed", { name });
    case "cli":
      return t("accounts.add.failedCli", { name });
    default:
      return t("accounts.add.failedError", { name });
  }
});

/* ---------- the dialog ---------- */

onMounted(() => {
  dialog.value?.showModal();
  if (dialog.value && !dialog.value.hasAttribute("tabindex")) dialog.value.tabIndex = -1;
  input.value?.focus();
});

onBeforeUnmount(() => {
  gone = true;
  window.clearTimeout(folderTimer);
  void listening.then((unlisten) => unlisten(), () => undefined);
  // Gone while the sign-in waits: it stops, and nothing is added.
  if (asked) {
    asked = false;
    void api.accountLoginCancel().catch(() => undefined);
  }
});

/** Esc: while the sign-in waits it stops it; while the folder dialog is up or OK applies, nothing. */
function onCancel(event: Event): void {
  if (picking.value || applying.value) {
    event.preventDefault();
    return;
  }
  if (step.value === "waiting" || busy.value) {
    event.preventDefault();
    cancelLogin();
  }
}

/** A click outside closes the first step and the last ones, never one that waits on something. */
function onBackdrop(event: MouseEvent): void {
  if (step.value === "waiting" || busy.value || picking.value || applying.value) return;
  backdrop.click(event);
}

/** Return on the dialog itself gives the step's answer: OK, or Try again. */
function onKey(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.repeat || event.isComposing || event.target !== dialog.value) return;
  if (step.value === "done") {
    event.preventDefault();
    void finish();
  } else if (step.value === "failed") {
    event.preventDefault();
    void login();
  }
}
</script>

<template>
  <dialog ref="dialog" class="add-dialog" :aria-label="t('accounts.add')" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="onBackdrop" @keydown="onKey">
    <!-- 1 · Name and folder -->
    <form v-if="step === 'form'" class="body" @submit.prevent="login">
      <div class="title">{{ t("accounts.add") }}</div>
      <label class="field">
        {{ t("accounts.add.name") }}
        <input ref="input" v-model="name" maxlength="60" spellcheck="false" autocomplete="off" :disabled="busy" />
      </label>
      <div class="field">
        <span id="account-folder">{{ t("accounts.add.folder") }}</span>
        <div class="folder">
          <span class="seg" role="radiogroup" aria-labelledby="account-folder">
            <button type="button" role="radio" :aria-checked="folderMode === 'new'" :disabled="busy" @click="folderMode = 'new'">{{ t("accounts.add.folderNew") }}</button>
            <button type="button" role="radio" :aria-checked="folderMode === 'pick'" :aria-busy="picking" :disabled="busy || picking" @click="pickFolder">
              {{ t("accounts.add.folderPick") }}
            </button>
          </span>
          <span class="folder-path" :title="shownFolder">{{ shownFolder }}</span>
        </div>
        <small v-if="folderMode === 'new' && suggested?.exists">{{ t("accounts.add.folderExists") }}</small>
      </div>
      <!-- The whole line is the switch's. -->
      <label class="switch-row ruled">
        <span class="switch-text">
          <span>{{ t("accounts.add.shared") }}</span>
          <small>{{ t("accounts.add.sharedHint") }}</small>
        </span>
        <input v-model="shared" type="checkbox" role="switch" :disabled="busy" />
      </label>
      <!-- Whatever the switch says: the core links the main account's session logs (and the
           projects' memory) into every account's folder, Pitwall follows sessions through them. -->
      <p class="note always">{{ t("accounts.add.sharedAlways") }}</p>
      <p class="note">{{ t(isMac ? "accounts.add.keepsMac" : "accounts.add.keepsFolder") }}</p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <div class="actions">
        <button type="button" class="control" @click="cancelLogin">{{ t("common.cancel") }}</button>
        <button type="submit" class="control primary" :disabled="!ready || busy || picking" :aria-busy="busy">
          <span :class="{ hidden: busy }">{{ t("accounts.add.signIn") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
    </form>

    <!-- 2 · The sign-in in the browser -->
    <div v-else-if="step === 'waiting'" class="body">
      <div class="title">{{ t("accounts.add.signingIn", { name: started }) }}</div>
      <div class="waiting" role="status"><Spinner :size="16" />{{ t("accounts.add.waiting") }}</div>
      <p class="note">{{ t("accounts.add.waitingHint") }}</p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <div class="actions">
        <button v-if="url" type="button" class="link" @click="openBrowser">{{ t("accounts.add.openBrowser") }}</button>
        <span class="grow"></span>
        <button type="button" class="control" @click="cancelLogin">{{ t("common.cancel") }}</button>
      </div>
    </div>

    <!-- 3 · Added: what Pitwall set up for it -->
    <div v-else-if="step === 'done'" class="body">
      <div class="title">
        <span class="title-name">{{ t("accounts.add.added", { name: addedName }) }}</span>
        <span v-if="plan" class="plan">{{ plan }}</span>
      </div>
      <ul v-if="checks.length" class="checks">
        <li v-for="check in checks" :key="check.text" :class="['check', check.kind]">
          <Icon :name="CHECK_ICONS[check.kind]" :size="14" />
          <span>{{ check.text }}</span>
        </li>
      </ul>
      <div class="switches">
        <label class="switch-row ruled">
          <span class="switch-text">{{ t("accounts.add.autoSync") }}</span>
          <input v-model="autoSync" type="checkbox" role="switch" :disabled="applying" />
        </label>
        <label class="switch-row">
          <span class="switch-text">{{ t("accounts.add.switchNow", { name: addedName }) }}</span>
          <input v-model="switchNow" type="checkbox" role="switch" :disabled="applying" />
        </label>
      </div>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <div class="actions">
        <button type="button" class="control primary" :disabled="applying" :aria-busy="applying" @click="finish">
          <span :class="{ hidden: applying }">{{ t("accounts.add.ok") }}</span>
          <Spinner v-if="applying" class="over" :size="12" />
        </button>
      </div>
    </div>

    <!-- The sign-in didn't complete: nothing was added. -->
    <div v-else class="body">
      <div class="title">{{ t("accounts.add.failedTitle") }}</div>
      <p class="text">{{ failText }}</p>
      <p v-if="failure?.reason === 'error' && failure.error" class="note detail">{{ failure.error }}</p>
      <div class="actions">
        <button type="button" class="control" :disabled="busy" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="control primary" :disabled="busy" :aria-busy="busy" @click="login">
          <span :class="{ hidden: busy }">{{ t("accounts.retry") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the command dialog. */
.add-dialog {
  width: 420px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
}

.add-dialog[open] {
  animation: pop 0.16s ease-out;
}

/* Resting on the dialog itself shows no ring. */
.add-dialog:focus-visible {
  outline: none;
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.title {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  font-size: 15px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.plan {
  flex-shrink: 0;
  font-size: 13px;
  font-weight: 400;
  color: var(--text-subtle);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--text-muted);
}

.field small {
  font-size: 12px;
  color: var(--text-subtle);
}

.field input {
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.folder {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

/* One of two, the picked one lit (settings.css `.seg`). */
.seg {
  flex-shrink: 0;
  display: inline-flex;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-seg);
}

.seg button {
  height: 24px;
  padding: 0 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
  white-space: nowrap;
}

.seg button:hover:not([aria-checked="true"]):not(:disabled) {
  color: var(--text);
}

.seg button[aria-checked="true"] {
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
  color: var(--text-strong);
}

.folder-path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: rtl;
  text-align: left;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-soft);
}

.switch-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 40px;
}

.switch-row.ruled {
  border-top: 1px solid var(--line);
  border-bottom: 1px solid var(--line-soft);
}

.switches .switch-row.ruled {
  border-bottom: 0;
}

.switches .switch-row + .switch-row {
  border-top: 1px solid var(--line-soft);
}

.switch-text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 6px 0;
}

.switch-text small {
  font-size: 12px;
  color: var(--text-subtle);
}

.note {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

/* Right under the switch it qualifies. */
.note.always {
  margin-top: -6px;
}

.note.detail {
  font-family: var(--font-mono);
  overflow-wrap: anywhere;
}

.text {
  margin: 0;
  line-height: 1.5;
  color: var(--text-soft);
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

.checks {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
}

.check {
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 10px;
  line-height: 1.4;
}

.check > svg {
  flex-shrink: 0;
}

.check.ok > svg {
  color: var(--run);
}

.check.todo > svg {
  color: var(--claude-text);
}

.check.failed > svg {
  color: var(--crash-text);
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

.control:disabled {
  opacity: 0.5;
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
  .add-dialog[open] {
    animation: none;
  }
}
</style>
