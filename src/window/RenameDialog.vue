<script setup lang="ts">
import { onMounted, ref } from "vue";

import { t } from "../lib/i18n";

/** Renames a terminal tab: opens with the name selected, Enter saves, Esc cancels. */
const props = defineProps<{ name: string; projectName: string }>();
const emit = defineEmits<{ save: [name: string]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const input = ref<HTMLInputElement | null>(null);
const draft = ref(props.name);

onMounted(() => {
  dialog.value?.showModal();
  input.value?.focus();
  input.value?.select();
});

/** An empty name keeps the old one. */
function submit(): void {
  const name = draft.value.trim();
  if (name && name !== props.name) emit("save", name);
  dialog.value?.close();
}

/** Esc and Cancel both end here, through the dialog's own close event. */
function cancel(): void {
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="t('terminal.renameName', { name })" @close="emit('close')">
    <form class="body" @submit.prevent="submit">
      <div class="head">
        <div class="title">{{ t("terminal.renameName", { name }) }}</div>
        <div class="project">{{ projectName }}</div>
      </div>
      <label class="field">
        {{ t("command.name") }}
        <input ref="input" v-model="draft" maxlength="40" spellcheck="false" />
      </label>
      <div class="actions">
        <button type="button" class="control" @click="cancel">{{ t("common.cancel") }}</button>
        <button type="submit" class="control primary">{{ t("common.save") }}</button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
/* The same look as the command dialog, smaller. */
.dialog {
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

.dialog[open] {
  animation: pop 0.16s ease-out;
}

.dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
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
}

.project {
  font-family: var(--font-mono);
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

.control:hover {
  background: #2c3039;
}

.control.primary {
  background: #2a3a30;
  border-color: #3d5446;
  color: #cdebd8;
}

.control.primary:hover {
  background: #33473a;
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
