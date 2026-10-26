<script setup lang="ts">
import { onMounted, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { accountName } from "../../lib/accounts";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { ClaudeAccount } from "../../lib/types";

/** Asks before an added account goes: Remove takes back what Pitwall added to its folder (the
 * folder and its sign-in stay); Esc, Cancel or a click outside keeps it. */
const props = defineProps<{ account: ClaudeAccount }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);
const busy = ref(false);
const error = ref("");

onMounted(() => {
  // Keeping it is the safe answer, and the one Return gives.
  openUnfocused(dialog.value, () => cancelButton.value);
});

async function remove(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await api.accountRemove(props.account.id);
    dialog.value?.close();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

/** While the core takes back what it added nothing closes it. */
function onCancel(event: Event): void {
  if (busy.value) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!busy.value) backdrop.click(event);
}
</script>

<template>
  <dialog
    ref="dialog"
    class="remove"
    :aria-label="t('accounts.remove.title', { name: accountName(account) })"
    @cancel="onCancel"
    @close="emit('close')"
    @pointerdown="backdrop.down"
    @click="onBackdrop"
  >
    <div class="body">
      <div class="title">{{ t("accounts.remove.title", { name: accountName(account) }) }}</div>
      <p class="text">{{ t("accounts.remove.body") }}</p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <div class="actions">
        <button ref="cancelButton" type="button" class="control" :disabled="busy" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="control danger" :disabled="busy" :aria-busy="busy" @click="remove">
          <span :class="{ hidden: busy }">{{ t("accounts.remove.confirm") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the team's delete dialog. */
.remove {
  width: 360px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
}

.remove[open] {
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

.control:disabled {
  opacity: 0.5;
}

.control.danger {
  color: var(--crash-text);
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
  .remove[open] {
    animation: none;
  }
}
</style>
