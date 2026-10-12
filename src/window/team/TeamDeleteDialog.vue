<script setup lang="ts">
import { onMounted, ref } from "vue";

import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import type { Team } from "../../lib/types";

/** Asks before a team goes: Delete deletes it (its projects stay listed), Esc, Cancel or a click
 * outside keeps it. */
defineProps<{ team: Team }>();
const emit = defineEmits<{ remove: []; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);

onMounted(() => {
  // Keeping it is the safe answer, and the one Return gives.
  openUnfocused(dialog.value, () => cancelButton.value);
});

function remove(): void {
  emit("remove");
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="delete-dialog" :aria-label="t('team.delete.title')" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="dd-body">
      <div class="dd-head">
        <div class="dd-title">{{ t("team.delete.title") }}</div>
        <div class="dd-team">{{ team.name }}</div>
      </div>
      <p class="dd-text">{{ t("team.delete.body") }}</p>
      <div class="dd-actions">
        <button ref="cancelButton" type="button" class="dd-control" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="dd-control danger" @click="remove">{{ t("common.delete") }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the board's delete dialog. */
.delete-dialog {
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

.delete-dialog[open] {
  animation: dd-pop 0.16s ease-out;
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

.dd-team {
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.dd-text {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.dd-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}

.dd-control {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.dd-control:hover {
  background: var(--bg-control-hover);
}

.dd-control.danger {
  color: var(--crash-text);
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
