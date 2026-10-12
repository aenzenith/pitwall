<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t, type Key } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { BackupParts } from "../../lib/types";

/**
 * What goes into a backup, picked before the core asks where to save it. `saved`: the file was
 * written (the dialog closes next). A save dialog that was cancelled leaves this one open for
 * another try; Esc, Cancel or a click outside closes it.
 */
const emit = defineEmits<{ saved: [file: string]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const exportButton = ref<HTMLButtonElement | null>(null);

/** Everything but the days' records, which is the bulk of it. */
const parts = reactive<BackupParts>({ settings: true, projects: true, board: true, activity: false });
const rows: Array<{ id: keyof BackupParts; label: Key }> = [
  { id: "settings", label: "common.settings" },
  { id: "projects", label: "backup.part.projects" },
  { id: "board", label: "backup.part.board" },
  { id: "activity", label: "backup.part.activity" },
];
const nothing = computed(() => !rows.some((row) => parts[row.id]));

const busy = ref(false);
const error = ref("");

onMounted(() => {
  // Saving is the answer Return gives.
  openUnfocused(dialog.value, () => exportButton.value);
});

async function save(): Promise<void> {
  if (busy.value || nothing.value) return;
  busy.value = true;
  error.value = "";
  try {
    const file = await api.backupExport({ ...parts });
    // Null: the save dialog was cancelled.
    if (file) {
      emit("saved", file);
      dialog.value?.close();
    }
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

/** While the core has its save dialog up nothing closes this one: not Esc, not a click outside. */
function onCancel(event: Event): void {
  if (busy.value) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!busy.value) backdrop.click(event);
}
</script>

<template>
  <dialog ref="dialog" class="dialog export" :aria-label="t('backup.exportTitle')" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="onBackdrop">
    <div class="body">
      <div class="head">
        <div class="title">{{ t("backup.exportTitle") }}</div>
        <div class="sub">{{ t("backup.text") }}</div>
      </div>

      <div class="checks">
        <label v-for="row in rows" :key="row.id" class="check">
          <input v-model="parts[row.id]" type="checkbox" :disabled="busy" />
          <span class="check-text">{{ t(row.label) }}</span>
        </label>
      </div>

      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <div class="actions">
        <button type="button" class="control" :disabled="busy" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button ref="exportButton" type="button" class="control primary" :disabled="busy || nothing" :aria-busy="busy" @click="save">
          <span :class="{ hidden: busy }">{{ t("backup.exportDo") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.export {
  width: 400px;
}
</style>
