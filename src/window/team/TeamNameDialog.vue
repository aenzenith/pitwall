<script setup lang="ts">
import { onMounted, ref, useId } from "vue";

import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";

/**
 * Names a team: a new one (`name` null), or one that has a name already, which opens selected.
 * Enter saves, Esc or a click outside cancels.
 *
 * A new team also takes its projects here: `choices` are the ones to pick from (the list hands
 * over the projects in no team), each a line with a check box, `picked` the ones ticked from the
 * start. `save` carries the ticked ones, in the order listed.
 */
const props = defineProps<{ name: string | null; choices?: Array<{ path: string; name: string }>; picked?: string[] }>();
const emit = defineEmits<{ save: [name: string, projects: string[]]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const input = ref<HTMLInputElement | null>(null);
const draft = ref(props.name ?? "");
const title = props.name === null ? "team.new" : "team.rename";
const chosen = ref<string[]>([...(props.picked ?? [])]);
const pickLabel = useId();

onMounted(() => {
  dialog.value?.showModal();
  input.value?.focus();
  input.value?.select();
});

/** An empty name makes no team, and keeps the old name of one that has it. */
function submit(): void {
  const name = draft.value.trim();
  const projects = (props.choices ?? []).map((choice) => choice.path).filter((path) => chosen.value.includes(path));
  if (name && name !== props.name) emit("save", name, projects);
  dialog.value?.close();
}

/** Esc, Cancel and a click outside all end here, through the dialog's own close event. */
function cancel(): void {
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="t(title)" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <form class="body" @submit.prevent="submit">
      <div class="title">{{ t(title) }}</div>
      <label class="field">
        {{ t("team.name") }}
        <!-- As long as the core keeps a name (core/teams.rs: NAME_LENGTH). -->
        <input ref="input" v-model="draft" maxlength="40" spellcheck="false" />
      </label>
      <!-- A new team's projects: the ones in no team yet, ticked to take them in. A plain group,
           not a fieldset: WebKit lays a flex fieldset out a legend too tall until its next
           layout, which left a gap over the buttons. -->
      <div v-if="choices" class="pick" role="group" :aria-labelledby="pickLabel">
        <span :id="pickLabel" class="pick-label">{{ t("common.projects") }}</span>
        <div v-if="choices.length" class="pick-list">
          <label v-for="choice in choices" :key="choice.path" class="pick-row" :title="choice.path">
            <input v-model="chosen" type="checkbox" :value="choice.path" />
            <span class="pick-name">{{ choice.name }}</span>
          </label>
        </div>
        <p v-else class="pick-none">{{ t("team.pickNone") }}</p>
      </div>
      <div class="actions">
        <button type="button" class="control" @click="cancel">{{ t("common.cancel") }}</button>
        <button type="submit" class="control primary" :disabled="!draft.trim()">{{ t(name === null ? "team.create" : "common.save") }}</button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
/* The same look as the terminal's rename dialog. */
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

.title {
  font-size: 15px;
  font-weight: 600;
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

/* The projects to take in: a box of its own that scrolls inside, the dialog never does. */
.pick {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

/* As the name field's label. */
.pick-label {
  font-size: 12px;
  color: var(--text-muted);
}

.pick-list {
  max-height: 192px;
  overflow-y: auto;
  padding: 4px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
}

/* The whole line ticks its box. */
.pick-row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 30px;
  padding: 0 8px;
  border-radius: 6px;
  cursor: pointer;
}

.pick-row:hover {
  background: var(--bg-row-hover);
}

.pick-row input {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  margin: 0;
  accent-color: var(--run);
}

.pick-name {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pick-none {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
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

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.control:disabled {
  opacity: 0.5;
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
