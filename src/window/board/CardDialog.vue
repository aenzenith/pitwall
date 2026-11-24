<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { agentName, agentsOn } from "../../lib/agents";
import { cardAgent, columnLabel } from "../../lib/board";
import { useCardImages, useNoteImages } from "../../lib/cardImages";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { language, t } from "../../lib/i18n";
import { createNoteEditor, type NoteBlock, type NoteEditor, type SlashAt } from "../../lib/noteEditor";
import { stamp } from "../../lib/period";
import { keys, primary } from "../../lib/platform";
import { CODEX_MODE_LABELS, MODE_LABELS, choiceOf, codexChoiceOf, codexModelLabel, modelLabel, projectAgent } from "../../lib/start";
import { api, snapshot } from "../../lib/store";
import type { Card } from "../../lib/types";
import NoteBlockMenu, { type BlockItem } from "./NoteBlockMenu.vue";

/**
 * Writes a card, or edits one (`card`), as a page: its title large and unboxed, what the card is
 * (its agent, who added it, its images) under it, and its note, drawn as Markdown and written where
 * it is clicked (lib/noteEditor). A new card goes to `path`; without one it asks which of
 * `projects` (`suggested` first chosen), and it never goes to a folder that isn't one of them.
 * Return in the title or ⌘↵ anywhere saves. Esc, Cancel or a click outside closes; with changes
 * not saved, the first Esc or click outside asks in the footer, and a second Esc closes without
 * saving. `prefill`: what a new card starts from (an error of the output), to be changed or saved
 * as it is.
 *
 * An image pasted into the note becomes `[Image #n]` at the caret, as in Claude Code's prompt, and
 * is drawn there as its picture; it goes to the agent with the card's text. It stays the card's
 * while the note names it.
 */
const props = defineProps<{ card: Card | null; path: string | null; suggested?: string | null; projects: Array<{ path: string; name: string }>; prefill?: { title: string; note: string } }>();
const emit = defineEmits<{ added: [card: Card]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog, () => leave());
useReturnFocus();
const titleInput = ref<HTMLTextAreaElement | null>(null);
const noteHost = ref<HTMLDivElement | null>(null);
const editor = shallowRef<NoteEditor | null>(null);

const initial = { title: props.card?.title ?? props.prefill?.title ?? "", note: props.card?.note ?? props.prefill?.note ?? "" };
const draft = ref({ ...initial, path: props.card?.path ?? props.path ?? props.suggested ?? props.projects[0]?.path ?? "" });
const error = ref("");
const titleMissing = ref(false);
const saving = ref(false);
/** Esc or a click outside with changes not saved: the footer asks. */
const asking = ref(false);

const heading = computed(() => t(props.card ? "board.dialog.editTitle" : "board.dialog.newTitle"));
const projectName = computed(() => props.projects.find((p) => p.path === draft.value.path)?.name ?? draft.value.path);
const project = computed(() => snapshot.value?.projects.find((p) => p.path === draft.value.path));
/** A new card on every project's board picks its project; one project's board knows it. */
const picksProject = computed(() => !props.card && props.path === null && props.projects.length > 1);
/** Where the card is: its project and its column (a new one goes to Up next). */
const where = computed(() => ({ project: picksProject.value ? t("board.allProjects") : projectName.value, column: t(columnLabel(props.card?.column ?? "queued", agentsOn.value)) }));

/** The agent the card goes to: the one it went to last, else its project's; and the model and
 * mode its last session started with, the parts that were chosen. */
const agent = computed(() => (props.card?.start ? cardAgent(props.card) : projectAgent(project.value)));
const codex = computed(() => agent.value === "codex");
const notePlaceholder = computed(() => t(codex.value ? "board.dialog.notePlaceholder.codex" : "board.dialog.notePlaceholder"));
const agentStart = computed(() => {
  const start = props.card?.start;
  if (!start) return [];
  if (codex.value) {
    const choice = codexChoiceOf(start);
    return [choice.model ? codexModelLabel(choice.model) : null, choice.mode !== "normal" ? t(CODEX_MODE_LABELS[choice.mode]) : null].filter((part): part is string => !!part);
  }
  const choice = choiceOf(start);
  return [choice.model ? modelLabel(choice.model) : null, choice.mode !== "normal" ? t(MODE_LABELS[choice.mode]) : null].filter((part): part is string => !!part);
});

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

const dirty = computed(() => draft.value.title.trim() !== initial.title.trim() || draft.value.note.trim() !== initial.note.trim());

// Writing on puts the question away.
watch(
  () => [draft.value.title, draft.value.note],
  () => {
    asking.value = false;
    if (draft.value.title.trim()) titleMissing.value = false;
  },
);

/* ---------- the title ---------- */

/** As tall as its lines: a long title wraps, as a page's does. */
function fitTitle(): void {
  const field = titleInput.value;
  if (!field) return;
  field.style.height = "auto";
  field.style.height = `${field.scrollHeight}px`;
}

/** One line of text: a line break pasted in is a space. */
function onTitleInput(): void {
  if (/[\r\n]/.test(draft.value.title)) draft.value.title = draft.value.title.replace(/[\r\n]+/g, " ");
  fitTitle();
}

/** Return saves, as the title field always did; ↓ at its end goes on to the note. */
function onTitleKey(event: KeyboardEvent): void {
  if (event.isComposing) return;
  if (event.key === "Enter" && !event.shiftKey && !primary(event)) {
    event.preventDefault();
    void submit();
  } else if (event.key === "ArrowDown" && titleInput.value?.selectionStart === draft.value.title.length) {
    event.preventDefault();
    editor.value?.focus();
  }
}

/* ---------- the note ---------- */

/** The card's saved images, and those pasted here: the note draws the ones it names. */
const saved = useCardImages(() => props.card);
const note = computed({ get: () => draft.value.note, set: (text: string) => (draft.value.note = text) });
const { shots, onPaste, remove: removeImage, settled, taken } = useNoteImages(
  note,
  () => editor.value?.field ?? null,
  (message) => (error.value = message),
  () => saved.value,
);

// The note changed from outside the editor (an image taken out by its button): the editor follows.
watch(note, (text) => editor.value?.setText(text));
// Another project picked, whose agent is another: the empty note names it.
watch(notePlaceholder, (text) => editor.value?.setPlaceholder(text));
watch(
  [shots, editor],
  () => editor.value?.setImages(new Map(shots.value.map((shot) => [shot.n, shot.src]))),
  { deep: true },
);

/* ---------- the block menu ---------- */

type BlockDef = BlockItem & { words: string[] };

const BLOCKS = computed<BlockDef[]>(() => [
  { block: "heading", icon: "heading", label: t("board.dialog.block.heading"), markup: "##", words: ["heading", "title", "h"] },
  { block: "bullets", icon: "list", label: t("board.dialog.block.bullets"), markup: "-", words: ["list", "bullet", "-"] },
  { block: "numbers", icon: "list-ordered", label: t("board.dialog.block.numbers"), markup: "1.", words: ["numbered", "ordered", "1"] },
  { block: "todo", icon: "checklist", label: t("board.dialog.block.todo"), markup: "[ ]", words: ["todo", "task", "check", "["] },
  { block: "code", icon: "editor", label: t("board.dialog.block.code"), markup: "```", words: ["code", "`"] },
  { block: "quote", icon: "quote", label: t("board.dialog.block.quote"), markup: ">", words: ["quote", ">"] },
]);

const slash = ref<SlashAt | null>(null);
const slashIndex = ref(0);

/** For matching what was typed: lower case in the app's language, without accents (`bas` finds
 * "Başlık"). */
function folded(text: string): string {
  return text
    .toLocaleLowerCase(language.value)
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/ı/g, "i");
}

/** The blocks what was typed after the `/` matches: by a word of their name, or the English one. */
const blockItems = computed<BlockItem[]>(() => {
  const query = slash.value ? folded(slash.value.query) : "";
  return BLOCKS.value.filter((item) => !query || folded(item.label).split(/\s+/).some((word) => word.startsWith(query)) || folded(item.label).startsWith(query) || item.words.some((word) => word.startsWith(query)));
});

watch(
  () => slash.value?.query,
  () => (slashIndex.value = 0),
);

/** Where the menu stands within the dialog: under the `/`, or over it when the window's bottom is
 * too near. */
const menuPlace = computed(() => {
  const at = slash.value;
  const box = dialog.value?.getBoundingClientRect();
  if (!at || !box) return { left: 0, top: 0 };
  const height = 24 + 10 + blockItems.value.length * 32;
  const below = at.bottom + 6;
  const top = below + height > window.innerHeight - 8 ? at.top - 6 - height : below;
  return { left: Math.max(8, Math.min(at.left - box.left - 10, box.width - 256)), top: top - box.top };
});

function pickBlock(block: NoteBlock): void {
  editor.value?.applyBlock(block);
}

/* ---------- open, save, close ---------- */

onMounted(() => {
  if (noteHost.value) {
    editor.value = createNoteEditor(noteHost.value, {
      doc: initial.note,
      placeholder: notePlaceholder.value,
      label: t("board.dialog.note"),
      imageLabel: (n) => t("board.image", { n }),
      removeLabel: (n) => t("board.dialog.removeImage", { n }),
      onChange: (text) => (draft.value.note = text),
      onSubmit: () => void submit(),
      onPaste,
      onRemoveImage: removeImage,
      onOpenLink: (url) => void api.openUrl(url).catch(() => {}),
      onSlash: (at) => (slash.value = at),
      onSlashMove: (by) => {
        const count = blockItems.value.length;
        if (!count) return false;
        slashIndex.value = (slashIndex.value + by + count) % count;
        return true;
      },
      onSlashPick: () => {
        const item = blockItems.value[slashIndex.value];
        if (!item) return false;
        pickBlock(item.block);
        return true;
      },
    });
  }
  dialog.value?.showModal();
  titleInput.value?.focus();
  if (props.card || props.prefill) titleInput.value?.select();
  void nextTick(fitTitle);
  window.addEventListener("resize", fitTitle);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", fitTitle);
  editor.value?.destroy();
  editor.value = null;
});

const pasteKeys = keys("mod+V");
const saveKeys = keys("mod+Enter");

async function submit(): Promise<void> {
  if (saving.value) return;
  const title = draft.value.title.trim();
  const text = draft.value.note.trim();
  if (!title) {
    titleMissing.value = true;
    asking.value = false;
    titleInput.value?.focus();
    return;
  }
  if (props.card && title === props.card.title && text === props.card.note) {
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
    await settled();
    // An image that couldn't be read took its token out of the note, and says so.
    if (error.value) return;
    const images = taken(text);
    if (props.card) await api.boardEdit(props.card.id, title, text, images);
    else emit("added", await api.boardAdd(path, title, text, images));
    dialog.value?.close();
  } catch (failure) {
    error.value = String(failure);
  } finally {
    saving.value = false;
  }
}

/** Esc, Cancel's neighbour and a click outside: closes, but with changes not saved asks first, in
 * the footer; asked already, it closes. */
function leave(): void {
  if (saving.value) return;
  if (dirty.value && !asking.value) {
    asking.value = true;
    return;
  }
  dialog.value?.close();
}

/** Cancel closes as it says, whatever was written. */
function cancel(): void {
  dialog.value?.close();
}

/** ⌘↵ saves from anywhere; Esc leaves. What the note's editor took (its menu's Esc, its own ⌘↵)
 * comes here already handled. */
function onKey(event: KeyboardEvent): void {
  if (event.defaultPrevented || event.isComposing) return;
  if (event.key === "Enter" && primary(event)) {
    event.preventDefault();
    void submit();
  } else if (event.key === "Escape") {
    event.preventDefault();
    leave();
  }
}

/** The dialog's own Esc, when it comes without a key the page saw. */
function onCancel(event: Event): void {
  event.preventDefault();
  leave();
}

/** A click under the note's last line writes at its end. */
function onNoteArea(event: MouseEvent): void {
  if (event.target === noteHost.value) {
    event.preventDefault();
    editor.value?.focus();
  }
}
</script>

<template>
  <dialog ref="dialog" class="card-dialog" :aria-label="heading" @close="emit('close')" @cancel="onCancel" @pointerdown="backdrop.down" @click="backdrop.click" @keydown="onKey">
    <form class="cd-body" @submit.prevent="submit">
      <div class="cd-top">
        <Icon name="folder" :size="13" />
        <span class="cd-where">{{ where.project }}</span>
        <Icon name="chevron" :size="11" class="cd-step" />
        <span class="cd-column">{{ where.column }}</span>
      </div>

      <div class="cd-scroll" @scroll="editor?.placeSlash()">
        <textarea
          ref="titleInput"
          v-model="draft.title"
          class="cd-title"
          rows="1"
          maxlength="200"
          :placeholder="t('board.dialog.titlePlaceholder')"
          :aria-label="t('board.dialog.title')"
          :aria-invalid="titleMissing || undefined"
          @input="onTitleInput"
          @keydown="onTitleKey"
        ></textarea>
        <p v-if="titleMissing" class="cd-error cd-title-error" role="alert">{{ t("board.dialog.missing") }}</p>

        <div class="cd-props">
          <label v-if="picksProject" class="cd-prop">
            <span class="cd-label"><Icon name="folder" :size="14" />{{ t("window.col.project") }}</span>
            <span class="cd-value">
              <select v-model="draft.path" class="cd-select">
                <option v-for="p in projects" :key="p.path" :value="p.path">{{ p.name }}</option>
              </select>
            </span>
          </label>
          <div class="cd-prop">
            <span class="cd-label"><Icon name="sparkles" :size="14" />{{ t("board.dialog.agent") }}</span>
            <span class="cd-value">
              <AgentMark :agent="agent" :size="13" />
              <span>{{ agentName(agent) }}</span>
              <span v-for="part in agentStart" :key="part" class="cd-meta">· {{ part }}</span>
            </span>
          </div>
          <div v-if="card?.by" class="cd-prop" :title="t('board.dialog.byRadioHint')">
            <span class="cd-label"><Icon name="user" :size="14" />{{ t("board.dialog.by") }}</span>
            <span class="cd-value">
              <AgentMark :agent="card.by" :size="13" />
              <span>{{ agentName(card.by) }}</span>
              <span class="cd-meta">· {{ t("board.dialog.byRadio", { when: stamp(card.createdAt, Date.now()) }) }}</span>
            </span>
          </div>
          <div v-if="shots.length" class="cd-prop">
            <span class="cd-label"><Icon name="image" :size="14" />{{ t("board.images") }}</span>
            <span class="cd-value">{{ shots.length }}</span>
          </div>
        </div>

        <div class="cd-rule"></div>
        <div ref="noteHost" class="cd-note" @mousedown="onNoteArea"></div>
        <p v-if="error" class="cd-error" role="alert">{{ error }}</p>
      </div>

      <div class="cd-foot">
        <template v-if="asking">
          <span class="cd-ask" role="alert"><span class="cd-ask-dot" aria-hidden="true"></span>{{ t("board.dialog.unsaved") }}</span>
          <button type="button" class="cd-control" @click="cancel">{{ t("board.dialog.discard") }}</button>
        </template>
        <template v-else>
          <span class="cd-hints">
            <span class="cd-key">/</span>{{ t("board.dialog.hintBlocks") }}<span class="cd-dot" aria-hidden="true">·</span><span class="cd-key">{{ pasteKeys }}</span>{{ t("board.dialog.hintPaste") }}
          </span>
          <button type="button" class="cd-control" @click="cancel">{{ t("common.cancel") }}</button>
        </template>
        <button type="submit" class="cd-control primary" :disabled="saving">
          <Spinner v-if="saving" :size="11" />
          {{ t(card ? "common.save" : "board.dialog.add") }}
          <span class="cd-keys" aria-hidden="true">{{ saveKeys }}</span>
        </button>
      </div>
    </form>

    <NoteBlockMenu v-if="slash && blockItems.length" :items="blockItems" :active="slashIndex" :left="menuPlace.left" :top="menuPlace.top" @pick="pickBlock" @hover="slashIndex = $event" />
  </dialog>
</template>

<style scoped>
/* A page over the board: wide, its note read as written, one scroll for all of it. */
.card-dialog {
  width: 680px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 64px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
  /* The block menu may reach past the dialog's edge. */
  overflow: visible;
}

.card-dialog[open] {
  display: flex;
  flex-direction: column;
  animation: cd-pop 0.16s ease-out;
}

.cd-body {
  flex: 1 1 auto;
  min-height: 0;
  max-height: calc(100vh - 66px);
  display: flex;
  flex-direction: column;
}

.cd-top {
  height: 42px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 20px;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  min-width: 0;
}

.cd-where {
  font-weight: 500;
  color: var(--text-soft);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cd-step {
  flex-shrink: 0;
  color: var(--text-faint);
}

.cd-column {
  flex-shrink: 0;
}

.cd-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 52px 22px;
}

.cd-title {
  display: block;
  width: 100%;
  min-height: 34px;
  margin: 2px 0 12px;
  padding: 0;
  border: 0;
  outline: none;
  resize: none;
  overflow: hidden;
  background: transparent;
  color: var(--text-strong);
  font: inherit;
  font-size: 24px;
  font-weight: 600;
  line-height: 1.3;
  letter-spacing: -0.012em;
}

.cd-title::placeholder {
  color: var(--text-faint);
  opacity: 0.6;
}

.cd-title-error {
  margin: -8px 0 12px;
}

.cd-props {
  display: flex;
  flex-direction: column;
  gap: 1px;
  margin-bottom: 12px;
}

.cd-prop {
  height: 30px;
  display: flex;
  align-items: center;
  min-width: 0;
  font-size: 13px;
  white-space: nowrap;
}

.cd-label {
  width: 118px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-subtle);
  overflow: hidden;
  text-overflow: ellipsis;
}

.cd-label :deep(svg) {
  flex-shrink: 0;
  color: var(--text-faint);
}

.cd-value {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  color: var(--text);
}

.cd-meta {
  color: var(--text-subtle);
  overflow: hidden;
  text-overflow: ellipsis;
}

.cd-select {
  height: 26px;
  min-width: 0;
  max-width: 320px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.cd-rule {
  height: 1px;
  margin: 2px 0 16px;
  background: var(--line-card);
}

.cd-note {
  min-height: 132px;
  cursor: text;
}

.cd-error {
  margin: 10px 0 0;
  font-size: 12px;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.cd-foot {
  height: 52px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 14px 0 20px;
  border-top: 1px solid var(--line);
  white-space: nowrap;
}

.cd-hints,
.cd-ask {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
  font-size: 12px;
  color: var(--text-faint);
}

.cd-ask {
  font-size: 12.5px;
  color: var(--text-soft);
}

.cd-ask-dot {
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  border-radius: 50%;
  background: var(--text-soft);
}

.cd-key {
  display: inline-flex;
  align-items: center;
  height: 17px;
  padding: 0 5px;
  flex-shrink: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 4px;
  background: var(--bg-key);
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-soft);
}

.cd-dot {
  padding: 0 2px;
}

.cd-control {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  flex-shrink: 0;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.cd-control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.cd-control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.cd-control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.cd-control:disabled {
  opacity: 0.7;
}

.cd-keys {
  font-family: var(--font-mono);
  font-size: 10.5px;
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
