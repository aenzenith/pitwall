<script setup lang="ts">
import { onMounted, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { Card } from "../../lib/types";

/**
 * Asks before a card goes: Delete deletes it, Esc, Cancel or a click outside keeps it. `running`:
 * its Claude session is still open, and says so: deleting the card doesn't end it. `deleted`: it
 * has gone (the dialog closes next), so the page can see to the keyboard its card held.
 */
const props = defineProps<{ card: Card; running: boolean }>();
const emit = defineEmits<{ deleted: [id: string]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);
const deleting = ref(false);
const error = ref("");

onMounted(() => {
  dialog.value?.showModal();
  // Keeping it is the safe answer, and the one Return gives.
  cancelButton.value?.focus();
});

async function remove(): Promise<void> {
  if (deleting.value) return;
  deleting.value = true;
  error.value = "";
  try {
    await api.boardDelete(props.card.id);
    emit("deleted", props.card.id);
    dialog.value?.close();
  } catch (failure) {
    error.value = String(failure);
  } finally {
    deleting.value = false;
  }
}
</script>

<template>
  <dialog ref="dialog" class="delete-dialog" :aria-label="t('board.delete.title')" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="dd-body">
      <div class="dd-head">
        <div class="dd-title">{{ t("board.delete.title") }}</div>
        <div class="dd-card">{{ card.title }}</div>
      </div>
      <p class="dd-text">{{ t("board.delete.body") }}</p>
      <p v-if="running" class="dd-text">{{ t("board.delete.running") }}</p>
      <p v-if="error" class="dd-error" role="alert">{{ error }}</p>
      <div class="dd-actions">
        <button ref="cancelButton" type="button" class="dd-control" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="dd-control danger" :disabled="deleting" @click="remove">
          <Spinner v-if="deleting" :size="11" />
          {{ t("common.delete") }}
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
.delete-dialog {
  width: 360px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid #2f343d;
  border-radius: 12px;
  background: #1b1e24;
  color: var(--text);
  font-size: 13px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
}

.delete-dialog[open] {
  animation: dd-pop 0.16s ease-out;
}

.delete-dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

.dd-body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.dd-head {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.dd-title {
  font-size: 15px;
  font-weight: 600;
}

.dd-card {
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.dd-text {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.dd-error {
  margin: 0;
  font-size: 12px;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.dd-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}

.dd-control {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.dd-control:hover:not(:disabled) {
  background: #2c3039;
}

.dd-control.danger {
  color: var(--crash-text);
}

.dd-control:disabled {
  opacity: 0.7;
}

@keyframes dd-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .delete-dialog[open] {
    animation: none;
  }
}
</style>
