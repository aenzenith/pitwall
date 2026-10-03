<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import { IMAGE_MB_MAX, imageData, imageNumbers, IMAGES_MAX, imageToken, nextImageNumber, pastedImages, spaced, useCardImages, withoutImage, type Shot } from "../../lib/cardImages";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { t } from "../../lib/i18n";
import { keys, primary } from "../../lib/platform";
import { api } from "../../lib/store";
import type { Card } from "../../lib/types";
import CardImages from "./CardImages.vue";

/**
 * Writes a card, or edits one (`card`): a title (needed) and a note. A new card goes to `path`;
 * without one it asks which of `projects` (`suggested` first chosen), and it never goes to a folder
 * that isn't one of them. Enter in the title or ⌘↵ anywhere saves; Esc, Cancel or a click outside
 * closes without saving.
 *
 * An image pasted into the note becomes `[Image #n]` at the caret, as in Claude Code's prompt,
 * and shows under the note; it goes to Claude with the card's text. It stays the card's while the
 * note names it.
 */
const props = defineProps<{ card: Card | null; path: string | null; suggested?: string | null; projects: Array<{ path: string; name: string }> }>();
const emit = defineEmits<{ added: [card: Card]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const titleInput = ref<HTMLInputElement | null>(null);
const noteInput = ref<HTMLTextAreaElement | null>(null);

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

/* ---------- the note's images ---------- */

/** The card's saved images, and those pasted here (not saved yet; `src` null while one is read). */
const saved = useCardImages(() => props.card);
const pasted = ref<Shot[]>([]);
/** Each pasted image being read: saving waits for them. */
const reading: Promise<void>[] = [];
/** The images the note names, in its order: what the card holds once saved. */
const shots = computed(() =>
  imageNumbers(draft.value.note)
    .map((n) => pasted.value.find((shot) => shot.n === n) ?? saved.value.find((shot) => shot.n === n))
    .filter((shot): shot is Shot => !!shot),
);
const pasteKeys = keys("mod+V");

/** A paste that carries an image puts the image in; any other is the text it always was. */
function onPaste(event: ClipboardEvent): void {
  const files = pastedImages(event.clipboardData);
  if (!files.length) return;
  event.preventDefault();
  for (const file of files) attach(file);
}

function refused(): void {
  error.value = t("board.dialog.imageRefused", { count: IMAGES_MAX, size: IMAGE_MB_MAX });
}

/** Puts `file` into the note: its token at the caret at once, the picture once it is read. */
function attach(file: File): void {
  error.value = "";
  if (shots.value.length >= IMAGES_MAX) {
    refused();
    return;
  }

  const n = nextImageNumber(draft.value.note, [...(props.card?.images ?? []), ...pasted.value].map((image) => image.n));
  pasted.value.push({ n, src: null });
  const field = noteInput.value;
  if (field) {
    field.setRangeText(spaced(field.value, field.selectionStart, imageToken(n)), field.selectionStart, field.selectionEnd, "end");
    draft.value.note = field.value;
  } else {
    draft.value.note += spaced(draft.value.note, draft.value.note.length, imageToken(n));
  }

  reading.push(
    imageData(file)
      .then((src) => {
        const shot = pasted.value.find((shot) => shot.n === n);
        if (shot) shot.src = src;
      })
      .catch(() => {
        removeImage(n);
        refused();
      }),
  );
}

/** Takes image `n` out of the note; a saved one leaves the card when the card is saved. */
function removeImage(n: number): void {
  draft.value.note = withoutImage(draft.value.note, n);
  pasted.value = pasted.value.filter((shot) => shot.n !== n);
}

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
    await Promise.all(reading);
    // An image that couldn't be read took its token out of the note, and says so.
    if (error.value) return;
    const named = imageNumbers(note);
    const images = pasted.value.flatMap((shot) => (shot.src && named.includes(shot.n) ? [{ n: shot.n, data: shot.src }] : []));
    if (props.card) await api.boardEdit(props.card.id, title, note, images);
    else emit("added", await api.boardAdd(path, title, note, images));
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
      <div class="cd-note">
        <label class="cd-field">
          {{ t("board.dialog.note") }}
          <textarea ref="noteInput" v-model="draft.note" rows="5" maxlength="8000" :placeholder="t('board.dialog.notePlaceholder')" @paste="onPaste"></textarea>
        </label>
        <CardImages v-if="shots.length" :images="shots" removable @remove="removeImage" />
        <p v-else class="cd-hint">{{ t("board.dialog.imageHint", { keys: pasteKeys }) }}</p>
      </div>
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

/* The note, and under it its images or how one gets there. */
.cd-note {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.cd-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
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
