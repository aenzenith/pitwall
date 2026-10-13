<script setup lang="ts">
import { ref } from "vue";

import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { BackupInfo, ImportReport } from "../../lib/types";
import { counted } from "./counts";
import ExportDialog from "./ExportDialog.vue";
import ImportDialog from "./ImportDialog.vue";
import PathText from "./PathText.vue";

/**
 * Settings › General's backup: one file out, one file in. Export asks what to take
 * (backup/ExportDialog) before the core asks where to save; Import has the core ask for the file
 * first, then shows what it holds (backup/ImportDialog). The last thing done stays as a line under
 * the buttons: the file written, what came back, or what went wrong. `imported`: what is listed
 * here may have changed.
 */
const emit = defineEmits<{ imported: [] }>();

const exporting = ref(false);
/** The core has its file dialog up. */
const picking = ref(false);
/** The file just picked, while its dialog is open. */
const picked = ref<BackupInfo | null>(null);

type Result = { kind: "saved"; file: string } | { kind: "imported"; text: string } | { kind: "error"; text: string };
const result = ref<Result | null>(null);

async function pick(): Promise<void> {
  if (picking.value) return;
  picking.value = true;
  result.value = null;
  try {
    // Null: the file dialog was cancelled.
    picked.value = await api.backupPick();
  } catch (reason) {
    result.value = { kind: "error", text: String(reason) };
  } finally {
    picking.value = false;
  }
}

/** `12 projects, 8 cards, 30 days restored`, the settings named last. */
function reportLine(report: ImportReport): string {
  const what = counted([
    ["backup.count.projects", report.projects],
    ["backup.count.cards", report.cards],
    ["backup.count.days", report.days],
  ]);
  if (what) return t(report.settings ? "backup.restoredWithSettings" : "backup.restored", { what });
  return t(report.settings ? "backup.restoredSettings" : "backup.restoredNothing");
}

function onImported(report: ImportReport): void {
  result.value = { kind: "imported", text: reportLine(report) };
  emit("imported");
}
</script>

<template>
  <div class="backup">
    <span class="text">{{ t("backup.text") }}</span>
    <div class="buttons">
      <button type="button" class="control" @click="exporting = true">{{ t("backup.export") }}</button>
      <button type="button" class="control" :disabled="picking" :aria-busy="picking" @click="pick">
        <!-- The label keeps its room under the spinner: the button never changes width. -->
        <span :class="{ hidden: picking }">{{ t("backup.import") }}</span>
        <Spinner v-if="picking" class="over" :size="12" />
      </button>
    </div>

    <p v-if="result?.kind === 'saved'" class="result saved" role="status">
      <Rich :text="t('backup.saved')">
        <template #file><PathText class="file" :path="result.file" /></template>
      </Rich>
    </p>
    <p v-else-if="result?.kind === 'imported'" class="result" role="status">{{ result.text }}</p>
    <p v-else-if="result" class="result bad" role="alert">{{ result.text }}</p>

    <ExportDialog v-if="exporting" @saved="result = { kind: 'saved', file: $event }" @close="exporting = false" />
    <ImportDialog v-if="picked" :info="picked" @imported="onImported" @close="picked = null" />
  </div>
</template>

<style scoped src="../settings/settings.css"></style>
<style scoped>
.backup {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.text {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.buttons {
  display: flex;
  gap: 8px;
}

.result {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-muted);
}

/* One line: the words stay, the path gives way. */
.result.saved {
  display: flex;
  gap: 5px;
  white-space: nowrap;
}

.file {
  flex: 0 1000 auto;
}

.result.bad {
  color: var(--crash-text);
  overflow-wrap: anywhere;
}
</style>
