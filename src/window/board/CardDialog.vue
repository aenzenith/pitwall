<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { primary } from "../../lib/platform";
import { api } from "../../lib/store";
import type { Card } from "../../lib/types";

/**
 * Writes a card, or edits one (`card`): a title (needed) and a note. A new card goes to `path`;
 * without one it asks which of `projects` (`suggested` first chosen), and it never goes to a folder
 * that isn't one of them. Enter in the title or ⌘↵ anywhere saves; Esc, Cancel or a click outside
 * closes without saving.
 */
const props = defineProps<{ card: Card | null; path: string | null; suggested?: string | null; projects: Array<{ path: string; name: string }> }>();
const emit = defineEmits<{ added: [card: Card]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const titleInput = ref<HTMLInputElement | null>(null);

const draft = ref({ title: props.card?.title ?? "", note: props.card?.note ?? "", path: props.card?.path ?? props.path ?? props.suggested ?? props.projects[0]?.path ?? "" });
const error = ref("");
const saving = ref(false);

const heading = computed(() => t(props.card ? "board.dialog.editTitle" : "board.dialog.newTitle"));
const projectName = computed(() => props.projects.find((p) => p.path === draft.value.path)?.name ?? draft.value.path);
/** A new card on every project's board picks its project; one project's board knows it. */
const picksProject = computed(() => !props.card && props.path === null && props.projects.length > 1);

/** A new card's project, one of `projects`: the chosen one, else the board's, the suggested one or
 * the first; empty when there is none. */
function listedPath(): string {
  const listed = (path: string | null | undefined): path is string => !!path && props.projects.some((p) => p.path === path);
  return [draft.value.path, props.path, props.suggested].find(listed) ?? props.projects[0]?.path ?? "";
}

// The chosen project left the list while the dialog was open: a listed one takes its place, in
// sight, so the card isn't written for a folder the board no longer shows.
watch(
  () => props.projects,
  () => {
    if (!props.card) draft.value.path = listedPath();
  },
);

onMounted(() => {
  dialog.value?.showModal();
  titleInput.value?.focus();
  if (props.card) titleInput.value?.select();
});

async function submit(): Promise<void> {
  if (saving.value) return;
  const title = draft.value.title.trim();
  const note = draft.value.note.trim();
  if (!title) {
    error.value = t("board.dialog.missing");
    titleInput.value?.focus();
    return;
  }
  if (props.card && title === props.card.title && note === props.card.note) {
    dialog.value?.close();
    return;
  }
  const path = listedPath();
  if (!props.card && !path) {
    error.value = t("board.noProjects");
    return;
  }

  saving.value = true;
  error.value = "";
  try {
    if (props.card) await api.boardEdit(props.card.id, title, note);
    else emit("added", await api.boardAdd(path, title, note));
    dialog.value?.close();
  } catch (failure) {
    error.value = String(failure);
  } finally {
    saving.value = false;
  }
}

/** ⌘↵ saves from the note too, where Enter is a new line. */
function onKey(event: KeyboardEvent): void {
  if (event.key === "Enter" && primary(event)) {
    event.preventDefault();
    void submit();
  }
}

/** Esc, Cancel and a click outside all end here, through the dialog's own close event. */
function cancel(): void {
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="card-dialog" :aria-label="heading" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click" @keydown="onKey">
    <form class="cd-body" @submit.prevent="submit">
      <div class="cd-head">
        <div class="cd-title">{{ heading }}</div>
        <div v-if="!picksProject" class="cd-project">{{ projectName }}</div>
      </div>
      <label v-if="picksProject" class="cd-field">
        {{ t("window.col.project") }}
        <select v-model="draft.path" class="cd-select">
          <option v-for="p in projects" :key="p.path" :value="p.path">{{ p.name }}</option>
        </select>
      </label>
      <label class="cd-field">
        {{ t("board.dialog.title") }}
        <input ref="titleInput" v-model="draft.title" maxlength="200" :placeholder="t('board.dialog.titlePlaceholder')" />
      </label>
      <label class="cd-field">
        {{ t("board.dialog.note") }}
        <textarea v-model="draft.note" rows="5" maxlength="8000" :placeholder="t('board.dialog.notePlaceholder')"></textarea>
      </label>
      <p v-if="error" class="cd-error" role="alert">{{ error }}</p>
      <div class="cd-actions">
        <button type="button" class="cd-control" @click="cancel">{{ t("common.cancel") }}</button>
        <button type="submit" class="cd-control primary" :disabled="saving">
          <Spinner v-if="saving" :size="11" />
          {{ t(card ? "common.save" : "board.dialog.add") }}
        </button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
/* The same look as the link dialog. */
.card-dialog {
  width: 480px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid #2f343d;
  border-radius: 12px;
  background: #1b1e24;
  color: var(--text);
  font-size: 13px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
}

.card-dialog[open] {
  animation: cd-pop 0.16s ease-out;
}

.card-dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

.cd-body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.cd-head {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.cd-title {
  font-size: 15px;
  font-weight: 600;
}

.cd-project {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cd-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-muted);
}

.cd-field input,
.cd-field textarea,
.cd-select {
  min-width: 0;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.cd-field input,
.cd-select {
  height: 30px;
}

.cd-field textarea {
  min-height: 96px;
  max-height: 320px;
  padding: 7px 10px;
  line-height: 1.45;
  resize: vertical;
}

.cd-field input::placeholder,
.cd-field textarea::placeholder {
  color: var(--text-faint);
}

.cd-error {
  margin: 0;
  font-size: 12px;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.cd-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

.cd-control {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.cd-control:hover:not(:disabled) {
  background: #2c3039;
}

.cd-control.primary {
  background: #2a3a30;
  border-color: #3d5446;
  color: #cdebd8;
}

.cd-control.primary:hover:not(:disabled) {
  background: #33473a;
}

.cd-control:disabled {
  opacity: 0.7;
}

@keyframes cd-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .card-dialog[open] {
    animation: none;
  }
}
</style>
