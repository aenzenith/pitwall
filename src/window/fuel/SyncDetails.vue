<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t, type Key } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { SyncReport, SyncServer } from "../../lib/types";

/**
 * An account's MCP servers beside the main account's, by their names only: what a sync would
 * bring over (`account_sync_plan`, asked as it opens; until then the report the strip had).
 */
const props = defineProps<{ id: string; name: string; report: SyncReport | null }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const closeButton = ref<HTMLButtonElement | null>(null);

const plan = ref<SyncReport | null>(props.report);
const asking = ref(true);

const LABELS: Record<SyncServer["state"], Key> = {
  synced: "accounts.server.synced",
  same: "accounts.server.same",
  missing: "accounts.server.missing",
  login: "accounts.server.login",
  onlyHere: "accounts.server.onlyHere",
  failed: "accounts.server.failed",
};

const servers = computed(() => plan.value?.servers ?? []);

onMounted(() => {
  openUnfocused(dialog.value, () => closeButton.value);
  api
    .accountSyncPlan(props.id)
    .then((report) => (plan.value = report))
    .catch(() => undefined)
    .finally(() => (asking.value = false));
});
</script>

<template>
  <dialog ref="dialog" class="details" :aria-label="t('accounts.details.title', { name })" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="body">
      <div class="title">
        <span>{{ t("accounts.details.title", { name }) }}</span>
        <Spinner v-if="asking" :size="12" />
      </div>
      <div v-if="servers.length" class="list">
        <div v-for="server in servers" :key="server.name" class="server">
          <span class="server-name">{{ server.name }}</span>
          <span v-if="server.state === 'synced'" class="state synced"><Icon name="check" :size="13" />{{ t(LABELS.synced) }}</span>
          <span v-else :class="['state', 'tag', server.state]">{{ t(LABELS[server.state] ?? LABELS.same) }}</span>
        </div>
      </div>
      <p v-else-if="!asking" class="none">{{ t("accounts.details.none") }}</p>
      <div class="actions">
        <button ref="closeButton" type="button" class="control" @click="dialog?.close()">{{ t("common.close") }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
.details {
  width: 460px;
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

.details[open] {
  animation: pop 0.16s ease-out;
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.title {
  display: flex;
  align-items: center;
  gap: 10px;
  padding-bottom: 8px;
  font-size: 15px;
  font-weight: 600;
}

.title > .spinner {
  color: var(--text-muted);
}

.list {
  display: flex;
  flex-direction: column;
}

.server {
  min-height: 36px;
  display: flex;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid var(--line-soft);
}

.server-name {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 12.5px;
}

.state {
  flex-shrink: 0;
  white-space: nowrap;
  font-size: 12px;
}

.state.synced {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--run-text);
}

.tag {
  padding: 2px 7px;
  border-radius: 5px;
  background: var(--bg-tag);
  font-size: 11px;
  color: var(--text-soft);
}

/* What waits on the user: Claude's colour. */
.tag.missing,
.tag.login {
  background: var(--claude-bg);
  color: var(--claude-text);
}

.tag.failed {
  background: var(--crash-bg);
  color: var(--crash-text);
}

.none {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.actions {
  display: flex;
  justify-content: flex-end;
  padding-top: 10px;
}

.control {
  height: 30px;
  padding: 0 14px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover {
  background: var(--bg-control-hover);
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .details[open] {
    animation: none;
  }
}
</style>
