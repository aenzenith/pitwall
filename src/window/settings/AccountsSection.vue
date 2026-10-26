<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { accountName, accounts, switchAccount } from "../../lib/accounts";
import { t } from "../../lib/i18n";
import { useNativeMenu } from "../../lib/nativeMenu";
import { isMac, isWindows } from "../../lib/platform";
import { api } from "../../lib/store";
import type { ClaudeAccount } from "../../lib/types";
import RenameDialog from "../RenameDialog.vue";
import AccountAddDialog from "./AccountAddDialog.vue";
import AccountRemoveDialog from "./AccountRemoveDialog.vue";

/**
 * Settings › Claude's accounts: every Claude account, the main one first (it is always there: it
 * can be renamed, never removed), the one in use marked, each added one with its "Sync when
 * switching" switch. A row's right-click: Use, Rename…, Show folder, Remove…. "Add account…"
 * under them is the only way to add one. Everything here goes to the core by its own command,
 * never with the form (`@change.stop`, `@submit.stop`: the dialogs' own forms).
 *
 * `startAdd`: the Fuel page's "Add account" opened Settings: the add dialog comes up at once.
 */
const props = defineProps<{ startAdd?: boolean }>();

const error = ref("");
const adding = ref(false);
const renaming = ref<ClaudeAccount | null>(null);
const removing = ref<ClaudeAccount | null>(null);

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
    { text: revealLabel.value, enabled: !account.missing, action: () => void act(() => api.accountReveal(account.id)) },
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
  return act(() => switchAccount(account.id));
}

function rename(account: ClaudeAccount, name: string): void {
  void act(() => api.accountRename(account.id, name));
}

async function setSync(account: ClaudeAccount, event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  error.value = "";
  try {
    await api.accountSetSync(account.id, box.checked);
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
  <div class="section accounts" role="group" aria-labelledby="accounts-section" @change.stop @submit.stop>
    <div id="accounts-section" class="section-label">{{ t("accounts.title") }}</div>
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
        </span>
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

    <AccountAddDialog v-if="adding" @close="adding = false" />
    <RenameDialog
      v-if="renaming"
      :name="accountName(renaming)"
      :project-name="renaming.dir"
      :clearable="renaming.main"
      @save="(name) => renaming && rename(renaming, name)"
      @close="renaming = null"
    />
    <AccountRemoveDialog v-if="removing" :account="removing" @close="removing = null" />
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
