<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Rich from "../components/Rich.vue";
import { t } from "../lib/i18n";
import type { CustomCommand } from "../lib/types";

/** `running`: the command being edited runs right now; changes reach it on its next run. */
const props = defineProps<{ command: CustomCommand | null; running?: boolean; projectName: string }>();
const emit = defineEmits<{ save: [command: CustomCommand]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const nameInput = ref<HTMLInputElement | null>(null);
const draft = ref<CustomCommand>(props.command ? { ...props.command } : { id: "", name: "", command: "", keepRunning: false, confirm: false, withServer: false });
const error = ref("");
const title = computed(() => t(props.command ? "command.editTitle" : "command.newTitle"));

onMounted(() => {
  dialog.value?.showModal();
  nameInput.value?.focus();
});

function submit(): void {
  const name = draft.value.name.trim();
  const command = draft.value.command.trim();

  if (!name || !command) {
    error.value = t("command.missing");
    return;
  }

  emit("save", { ...draft.value, name, command });
}

/** Esc and Cancel both end here, through the dialog's own close event. */
function cancel(): void {
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="title" @close="emit('close')">
    <form class="body" @submit.prevent="submit">
      <div class="head">
        <div class="title">{{ title }}</div>
        <div class="project">{{ projectName }}</div>
      </div>
      <label class="field">{{ t("command.name") }} <input ref="nameInput" v-model="draft.name" :placeholder="t('command.namePlaceholder')" spellcheck="false" /></label>
      <label class="field">{{ t("command.command") }} <input v-model="draft.command" class="mono" placeholder="php artisan queue:work" spellcheck="false" /></label>
      <div class="checks">
        <label class="check"><input v-model="draft.keepRunning" type="checkbox" /> {{ t("command.keepRunning") }}</label>
        <label class="check"><input v-model="draft.confirm" type="checkbox" /> {{ t("command.confirm") }}</label>
        <label class="check"><input v-model="draft.withServer" type="checkbox" /> {{ t("command.withServer") }}</label>
      </div>
      <p v-if="error" class="error">{{ error }}</p>
      <p v-if="running" class="note">{{ t("command.runningNote") }}</p>
      <p class="hint">
        <Rich :text="t('command.hint')">
          <template #build><code>npm run build</code></template>
          <template #vite><code>vite build</code></template>
        </Rich>
      </p>
      <div class="actions">
        <button type="button" class="control" @click="cancel">{{ t("common.cancel") }}</button>
        <button type="submit" class="control primary">{{ t(command ? "common.save" : "command.add") }}</button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
.dialog {
  width: 440px;
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
  margin-bottom: 2px;
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

.field input.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.checks {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 2px;
}

.check {
  display: flex;
  align-items: center;
  gap: 8px;
}

.check input {
  width: 15px;
  height: 15px;
  margin: 0;
  accent-color: var(--run);
}

.error {
  margin: 0;
  font-size: 12px;
  color: var(--crash-text);
}

.note {
  margin: 0;
  font-size: 12px;
  color: #f3c29b;
}

.hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.hint code {
  font-family: var(--font-mono);
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
</style>
