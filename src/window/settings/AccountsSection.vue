<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { accountName, accountsOf, loginEndView, switchAccount, type LoginEndView } from "../../lib/accounts";
import { t } from "../../lib/i18n";
import { useNativeMenu } from "../../lib/nativeMenu";
import { isMac, isWindows } from "../../lib/platform";
import { api, now } from "../../lib/store";
import type { Agent, ClaudeAccount } from "../../lib/types";
import RenameDialog from "../RenameDialog.vue";
import AccountAddDialog from "./AccountAddDialog.vue";
import AccountRemoveDialog from "./AccountRemoveDialog.vue";
import AccountRenewDialog from "./AccountRenewDialog.vue";

/**
 * Settings › Claude's accounts: every Claude account, the main one first (it is always there: it
 * can be renamed, never removed), the one in use marked, each added one with its "Sync when
 * switching" switch. A row's right-click: Use, Rename…, Show folder, Remove…. "Add account…"
 * under them is the only way to add one. Everything here goes to the core by its own command,
 * never with the form (`@change.stop`, `@submit.stop`: the dialogs' own forms).
 *
 * A Claude account's third line says when its claude.ai sign-in ends (`loginEnds`, asked as the
 * tab shows: SettingsView), and within three days of it, or past it, "Renew sign-in" runs Claude
 * Code's own sign-in again for it (`AccountRenewDialog`). Codex's accounts have none of it.
 *
 * `startAdd`: the Fuel page's "Add account" opened Settings: the add dialog comes up at once.
 * `agent`: whose accounts (none: Claude's, as before); Codex's in Settings › Codex, each a home
 * of its own (`CODEX_HOME`).
 */
const props = defineProps<{ startAdd?: boolean; agent?: Agent }>();

const agent: Agent = props.agent ?? "claude";
const other = agent !== "claude";
const accounts = computed(() => accountsOf(agent));

const error = ref("");
const adding = ref(false);
const renaming = ref<ClaudeAccount | null>(null);
const removing = ref<ClaudeAccount | null>(null);
const renewing = ref<ClaudeAccount | null>(null);

/** When each one's sign-in ends, by its id: Claude's accounts' alone. */
const ends = computed<Record<string, LoginEndView | null>>(() =>
  Object.fromEntries(accounts.value.map((account) => [account.id, other || account.missing ? null : loginEndView(account.id, now.value)])),
);

/** Its folder, plan and settings, as one line under its name. */
function details(account: ClaudeAccount): string[] {
  const parts: string[] = [];
  if (account.limits.plan) parts.push(account.limits.plan);
  if (!account.main) parts.push(t(account.shared ? "accounts.sharedSettings" : "accounts.ownSettings"));
  return parts;
}

const revealLabel = computed(() => t(isMac ? "accounts.menu.revealMac" : isWindows ? "accounts.menu.revealWindows" : "accounts.menu.revealLinux"));

const menu = useNativeMenu();

function rowMenu(event: MouseEvent, account: ClaudeAccount): void {
  error.value = "";
  void menu.popup(event, [
    { text: t("accounts.menu.use"), enabled: !account.inUse && !account.missing, action: () => void use(account) },
    { text: t("accounts.menu.rename"), action: () => (renaming.value = account) },
    { text: revealLabel.value, enabled: !account.missing, action: () => void act(() => (other ? api.agentAccountReveal(agent, account.id) : api.accountReveal(account.id))) },
    // The main account is never removed: no line for it.
    ...(account.main ? [] : ["separator" as const, { text: t("accounts.menu.remove"), action: () => (removing.value = account) }]),
  ]);
}

/** A command whose failure is told under the list. */
async function act(run: () => Promise<unknown>): Promise<void> {
  error.value = "";
  try {
    await run();
  } catch (reason) {
    error.value = String(reason);
  }
}

function use(account: ClaudeAccount): Promise<void> {
  return act(() => switchAccount(account.id, agent));
}

function rename(account: ClaudeAccount, name: string): void {
  void act(() => (other ? api.agentAccountRename(agent, account.id, name) : api.accountRename(account.id, name)));
}

async function setSync(account: ClaudeAccount, event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  error.value = "";
  try {
    await (other ? api.agentAccountSetSync(agent, account.id, box.checked) : api.accountSetSync(account.id, box.checked));
  } catch (reason) {
    // Not taken: the switch shows what is stored again.
    box.checked = account.sync;
    error.value = String(reason);
  }
}

function openAdd(): void {
  error.value = "";
  adding.value = true;
}

onMounted(() => {
  if (props.startAdd) openAdd();
});
</script>

<template>
  <div class="section accounts" role="group" :aria-labelledby="other ? `accounts-section-${agent}` : 'accounts-section'" @change.stop @submit.stop>
    <div :id="other ? `accounts-section-${agent}` : 'accounts-section'" class="section-label">{{ t("accounts.title") }}</div>
    <!-- The main account is there all the same with none added; nothing before the core says. -->
    <div v-if="accounts.length" class="list">
      <div v-for="account in accounts" :key="account.id" :class="['account', { missing: account.missing }]" @contextmenu.prevent="rowMenu($event, account)">
        <span class="account-text">
          <span class="account-name">{{ accountName(account) }}</span>
          <span class="account-sub">
            <span class="dir">{{ account.dir }}</span>
            <template v-if="account.missing"> · <span class="gone">{{ t("accounts.missing") }}</span></template>
            <template v-for="part in details(account)" :key="part"> · {{ part }}</template>
          </span>
          <span v-if="ends[account.id]" :class="['account-end', ends[account.id]?.level]" :title="ends[account.id]?.title ?? undefined">{{ ends[account.id]?.text }}</span>
        </span>
        <button
          v-if="ends[account.id]?.renew"
          type="button"
          class="control renew"
          :aria-label="t('accounts.loginEnd.renewFor', { name: accountName(account) })"
          @click="
            error = '';
            renewing = account;
          "
        >
          {{ t("accounts.loginEnd.renew") }}
        </button>
        <span v-if="account.inUse" class="badge">{{ t("accounts.inUse") }}</span>
        <!-- The whole label is the switch's. -->
        <label v-if="!account.main" class="sync">
          {{ t("accounts.syncOnSwitch") }}
          <input type="checkbox" role="switch" :checked="account.sync" :aria-label="t('accounts.syncOnSwitchFor', { name: accountName(account) })" @change="setSync(account, $event)" />
        </label>
      </div>
    </div>
    <button type="button" class="control add" @click="openAdd"><Icon name="plus" :size="13" />{{ t("accounts.addMenu") }}</button>
    <p v-if="error" class="error" role="alert">{{ error }}</p>

    <AccountAddDialog v-if="adding" :agent="other ? agent : undefined" @close="adding = false" />
    <RenameDialog
      v-if="renaming"
      :name="accountName(renaming)"
      :project-name="renaming.dir"
      :clearable="renaming.main"
      @save="(name) => renaming && rename(renaming, name)"
      @close="renaming = null"
    />
    <AccountRenewDialog v-if="renewing" :account="renewing" @close="renewing = null" />
    <AccountRemoveDialog v-if="removing" :account="removing" :agent="other ? agent : undefined" @close="removing = null" />
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.accounts {
  display: flex;
  flex-direction: column;
  gap: 10px;
  font-size: 13px;
}

.list {
  display: flex;
  flex-direction: column;
}

.account {
  min-height: 52px;
  padding: 7px 0;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 12px;
  border-top: 1px solid var(--line-soft);
}

.list > .account:first-child {
  border-top: 0;
}

.account-text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.account-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}

.account-sub {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-subtle);
}

.dir {
  font-family: var(--font-mono);
}

/* When its sign-in ends: plain while far, in the warning's colour within three days, the crash's
   once past. */
.account-end {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-subtle);
}

.account-end.near {
  color: var(--warn-text);
}

.account-end.ended,
.account-end.out {
  color: var(--crash-text);
}

.renew {
  flex-shrink: 0;
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
}

.gone {
  color: var(--warn-text);
}

.account.missing .account-name {
  color: var(--text-muted);
}

.sync {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

.add {
  align-self: flex-start;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 12px;
}
</style>
