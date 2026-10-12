<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { stamp } from "../../lib/period";
import { api, now } from "../../lib/store";
import type { BackupInfo, BackupParts, ImportReport } from "../../lib/types";
import { counted } from "./counts";
import PathText from "./PathText.vue";

/**
 * What a picked backup file holds, and which of its parts to put back: a part the file lacks
 * can't be ticked. `imported`: it went in (the dialog closes next). Esc, Cancel or a click outside
 * leaves everything as it is.
 */
const props = defineProps<{ info: BackupInfo }>();
const emit = defineEmits<{ imported: [report: ImportReport]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);

// The file as it was picked: the core holds on to that one.
const info = props.info;

/** The parts the file has. */
const has: BackupParts = {
  settings: info.settings,
  projects: info.projects + info.commands + info.links + info.teams > 0,
  board: info.cards + info.images > 0,
  activity: info.days > 0,
};
const parts = reactive<BackupParts>({ ...has });

/** Each part by name, with what the file holds of it. */
const rows = computed<Array<{ id: keyof BackupParts; name: string; holds: string }>>(() => [
  { id: "settings", name: t("common.settings"), holds: "" },
  {
    id: "projects",
    name: t("common.projects"),
    holds: counted([
      ["backup.count.projects", info.projects],
      ["backup.count.commands", info.commands],
      ["backup.count.links", info.links],
      ["backup.count.teams", info.teams],
    ]),
  },
  {
    id: "board",
    name: t("board.nav"),
    holds: counted([
      ["backup.count.cards", info.cards],
      ["backup.count.images", info.images],
    ]),
  },
  { id: "activity", name: t("backup.name.activity"), holds: counted([["backup.count.days", info.days]]) },
]);
const nothing = computed(() => !rows.value.some((row) => parts[row.id]));

const busy = ref(false);
const error = ref("");

onMounted(() => {
  // It writes over what is here: leaving it is the safe answer, and the one Return gives.
  openUnfocused(dialog.value, () => cancelButton.value);
});

async function run(): Promise<void> {
  if (busy.value || nothing.value) return;
  busy.value = true;
  error.value = "";
  try {
    const report = await api.backupImport({ ...parts });
    emit("imported", report);
    dialog.value?.close();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

/** While it is being put back nothing closes the dialog: not Esc, not a click outside. */
function onCancel(event: Event): void {
  if (busy.value) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!busy.value) backdrop.click(event);
}
</script>

<template>
  <dialog ref="dialog" class="dialog import" :aria-label="t('backup.importTitle')" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="onBackdrop">
    <div class="body">
      <div class="head">
        <div class="title">{{ t("backup.importTitle") }}</div>
        <div class="sub file"><PathText :path="info.file" /></div>
        <div class="sub">{{ t("backup.madeAt", { when: stamp(info.createdAt, now), version: info.app }) }}</div>
      </div>

      <div class="checks">
        <label v-for="row in rows" :key="row.id" :class="['check', { lacks: !has[row.id] }]">
          <input v-model="parts[row.id]" type="checkbox" :disabled="!has[row.id] || busy" />
          <span class="check-text">
            <span class="check-name">{{ row.name }}</span>
            <small v-if="!has[row.id]">{{ t("backup.missingPart") }}</small>
            <small v-else-if="row.holds">{{ row.holds }}</small>
          </span>
        </label>
      </div>

      <p class="note">{{ t("backup.importNote") }}</p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <div class="actions">
        <button ref="cancelButton" type="button" class="control" :disabled="busy" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="control primary" :disabled="busy || nothing" :aria-busy="busy" @click="run">
          <span :class="{ hidden: busy }">{{ t("backup.importDo") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.import {
  width: 420px;
}

/* The file's own line: its path gives way inside it. */
.file {
  display: flex;
  min-width: 0;
}

.check.lacks .check-name {
  color: var(--text-faint);
}

.note {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}
</style>
