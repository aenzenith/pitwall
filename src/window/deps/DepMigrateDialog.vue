<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import type { DepLine } from "../../lib/deps";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import type { Project } from "../../lib/types";

/**
 * Asks before a migration tool's migrate runs: the command as it would run and, when known,
 * where it would go. Run starts it and closes the dialog (its line shows it running); Esc,
 * Cancel or a click outside leaves the database as it is. `installing`: an install runs in the
 * project, and migrate waits for it.
 */
const props = defineProps<{ project: Project; line: Extract<DepLine, { kind: "migration" }>; installing: boolean }>();
const emit = defineEmits<{ run: []; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);

const subtitle = computed(() => [props.project.name, t("deps.migrations", { count: props.line.pending.length })]);

onMounted(() => {
  dialog.value?.showModal();
  // Leaving the database as it is is the safe answer, and the one Return gives.
  cancelButton.value?.focus();
});

function run(): void {
  emit("run");
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="migrate-dialog" :aria-label="t('deps.migrateConfirm')" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="md-body">
      <div class="md-head">
        <div class="md-title">{{ t("deps.migrateConfirm") }}</div>
        <div class="md-sub" :title="subtitle.join(' · ')">
          <template v-for="(part, at) in subtitle" :key="at">
            <span v-if="at" aria-hidden="true"> · </span>
            <span>{{ part }}</span>
          </template>
        </div>
      </div>
      <!-- The command, and under it the database it would change. -->
      <div class="md-what">
        <span class="md-command" :title="line.command">{{ line.command }}</span>
        <span v-if="line.note" class="md-database" :title="line.note">{{ line.note }}</span>
      </div>
      <div class="md-actions">
        <button ref="cancelButton" type="button" class="md-control" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="md-control primary" :disabled="installing" :title="installing ? t('deps.migrateAfterInstall') : undefined" @click="run">{{ t("common.run") }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the app's other small dialogs. */
.migrate-dialog {
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

.migrate-dialog[open] {
  animation: md-pop 0.16s ease-out;
}

.migrate-dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

.md-body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.md-head {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.md-title {
  font-size: 15px;
  font-weight: 600;
}

.md-sub {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.md-what {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid #1f2228;
  border-radius: 8px;
  background: var(--bg-log);
  font-family: var(--font-mono);
}

.md-command,
.md-database {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.md-command {
  font-size: 12px;
  color: #c7ccd3;
}

.md-database {
  font-size: 11px;
  color: var(--text-subtle);
}

.md-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

.md-control {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.md-control:hover:not(:disabled) {
  background: #2c3039;
}

.md-control.primary {
  background: #2a3a30;
  border-color: #3d5446;
  color: #cdebd8;
}

.md-control.primary:hover:not(:disabled) {
  background: #33473a;
}

.md-control:disabled {
  opacity: 0.5;
}

@keyframes md-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .migrate-dialog[open] {
    animation: none;
  }
}
</style>
