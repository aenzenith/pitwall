<script setup lang="ts">
import { onMounted, ref } from "vue";

import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";

/** Names a group of sessions (the Sessions page, O10): a new one (`name` null) or one renamed.
 * Opens with the name selected; Enter saves, Esc or a click outside cancels; an empty name, or the
 * same one, saves nothing. `count`: the sessions it takes in at once, said under the title. */
const props = defineProps<{ name: string | null; count?: number }>();
const emit = defineEmits<{ save: [name: string]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const input = ref<HTMLInputElement | null>(null);
const draft = ref(props.name ?? "");

onMounted(() => {
  dialog.value?.showModal();
  input.value?.focus();
  input.value?.select();
});

function submit(): void {
  const name = draft.value.trim();
  if (name && name !== props.name) emit("save", name);
  dialog.value?.close();
}

const title = props.name === null ? t("chatSurface.groups.newTitle") : t("chatSurface.groups.renameTitle", { name: props.name });
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="title" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <form class="body" @submit.prevent="submit">
      <div class="head">
        <div class="title">{{ title }}</div>
        <div v-if="count" class="sub">{{ t("chatSurface.groups.takesIn", { count }) }}</div>
      </div>
      <label class="field">
        {{ t("command.name") }}
        <input ref="input" v-model="draft" maxlength="80" spellcheck="false" />
      </label>
      <div class="actions">
        <button type="button" class="control" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="submit" class="control primary" :disabled="!draft.trim()">{{ t("common.save") }}</button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
/* The same look as the tab's rename dialog (RenameDialog). */
.dialog {
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

.dialog[open] {
  animation: pop 0.16s ease-out;
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.head {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.sub {
  font-size: 12px;
  color: var(--text-subtle);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-muted);
}

.field input {
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

.control {
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

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .dialog[open] {
    animation: none;
  }
}
</style>
