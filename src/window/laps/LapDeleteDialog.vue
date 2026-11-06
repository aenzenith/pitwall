<script setup lang="ts">
import { onMounted, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { refusalText } from "../../lib/laps";
import { api } from "../../lib/store";

/**
 * İşi sil asks once: Sil deletes the job with its kept runs and outputs; Vazgeç, Esc or a click
 * outside keeps it (the safe answer, Return's). `running`: a run of it goes on, and says so.
 */
const props = defineProps<{ id: string; name: string; running: boolean }>();
const emit = defineEmits<{ deleted: [id: string]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);
const deleting = ref(false);
const error = ref("");

onMounted(() => openUnfocused(dialog.value, () => cancelButton.value));

async function remove(): Promise<void> {
  if (deleting.value) return;
  deleting.value = true;
  error.value = "";
  try {
    await api.lapDelete(props.id);
    emit("deleted", props.id);
    dialog.value?.close();
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    deleting.value = false;
  }
}
</script>

<template>
  <dialog ref="dialog" class="dialog delete" :aria-label="t('laps.deleteAsk.title')" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="body">
      <div class="head">
        <span class="dialog-title">{{ t("laps.deleteAsk.title") }}</span>
        <span class="name">{{ name }}</span>
      </div>
      <p class="hint">{{ t("laps.deleteAsk.body") }}</p>
      <p v-if="running" class="hint">{{ t("laps.deleteAsk.running") }}</p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <div class="actions">
        <button ref="cancelButton" type="button" class="control" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="control delete-button" :disabled="deleting" :aria-busy="deleting" @click="remove">
          <span :class="{ hidden: deleting }">{{ t("common.delete") }}</span>
          <Spinner v-if="deleting" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.delete {
  width: 360px;
}

.body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 18px 20px 16px;
}

.head {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.name {
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}

.delete-button {
  color: var(--crash-text);
}
</style>
