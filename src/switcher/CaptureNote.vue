<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import type { Shot } from "../lib/cardImages";
import { t } from "../lib/i18n";
import type { Project } from "../lib/types";
import CardImages from "../window/board/CardImages.vue";

/**
 * The note of a card written in the quick switcher, under its field: the project the card goes to
 * (a button that lists `projects`; the keyboard picks it in the list before the note opens), the
 * note itself, and under it the images pasted into it (`shots`). The note grows with its text up
 * to a height the window still holds, then scrolls. The keyboard stays in the note whatever is
 * clicked here. `room`: the tallest the window gets; the open list asks for what it needs of it
 * (`reach`).
 */
const props = defineProps<{ projects: Project[]; path: string | null; shots: Shot[]; room: number }>();
const emit = defineEmits<{ pick: [path: string]; paste: [event: ClipboardEvent]; remove: [n: number]; resized: [] }>();
const note = defineModel<string>("note", { required: true });

const field = ref<HTMLTextAreaElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const list = ref<HTMLElement | null>(null);
/** The project list is open. */
const listing = ref(false);
/** Where the open list hangs, in the window: under its button, as tall as its projects or as the
 * window's most leaves it. */
const hang = ref<{ top: number; left: number; height: number } | null>(null);
/** The room kept between the list and the window's edge. */
const EDGE = 8;

/** Opens the list (or closes it). It lies over the note and the footer, and the window grows to
 * hold it. */
async function toggle(): Promise<void> {
  listing.value = !listing.value;
  hang.value = null;
  if (listing.value) {
    await nextTick();
    const from = button.value?.getBoundingClientRect();
    if (from && list.value) {
      const top = Math.round(from.bottom) + 4;
      hang.value = { top, left: Math.round(from.left), height: Math.min(list.value.scrollHeight + 2, props.room - top - EDGE) };
    }
  }
  emit("resized");
}

function close(): boolean {
  const was = listing.value;
  listing.value = false;
  hang.value = null;
  if (was) emit("resized");
  return was;
}

const name = computed(() => props.projects.find((project) => project.path === props.path)?.name ?? "");

/** The note as tall as its text (the stylesheet keeps it between its least and most). */
function grow(): void {
  const area = field.value;
  if (!area) return;
  const before = area.offsetHeight;
  area.style.height = "auto";
  // Its border is no part of what it scrolls.
  area.style.height = `${area.scrollHeight + 2}px`;
  if (area.offsetHeight !== before) emit("resized");
}

// Typed, pasted, or an image's token put in or taken out.
watch(note, () => void nextTick(grow));

function pick(path: string): void {
  close();
  emit("pick", path);
}

/** A press anywhere but on the list or its button closes the list. */
function onPress(event: MouseEvent): void {
  if (listing.value && !(event.target instanceof Element && event.target.closest(".note-picker"))) close();
}

onMounted(() => {
  grow();
  document.addEventListener("mousedown", onPress, true);
});

onBeforeUnmount(() => document.removeEventListener("mousedown", onPress, true));

defineExpose({
  /** The note's text area: where a pasted image's token goes. */
  field,
  /** The keyboard to the note, the caret after its last character. */
  focus: () => {
    const area = field.value;
    if (!area) return;
    area.focus({ preventScroll: true });
    area.setSelectionRange(area.value.length, area.value.length);
  },
  /** Closes the project list; false when it wasn't open. */
  closeList: close,
  /** How tall the window must be to hold the open list; 0 when it is closed. */
  reach: (): number => (hang.value ? hang.value.top + hang.value.height + EDGE : 0),
});
</script>

<template>
  <div class="note" data-scrolls>
    <div class="note-head">
      <span id="capture-project-label" class="section-label">{{ t("window.col.project") }}</span>
      <div class="note-picker">
        <!-- Never a Tab stop: Tab and Shift+Tab move between the title and the note. -->
        <button
          id="capture-project"
          ref="button"
          type="button"
          tabindex="-1"
          :class="['note-project', { on: listing }]"
          aria-haspopup="listbox"
          :aria-expanded="listing"
          aria-labelledby="capture-project-label capture-project"
          @mousedown.prevent
          @click="toggle"
        >
          <span class="note-project-name">{{ name }}</span>
          <Icon name="chevron-down" :size="12" />
        </button>
        <ul
          v-if="listing"
          ref="list"
          class="note-projects"
          role="listbox"
          aria-labelledby="capture-project-label"
          :style="hang ? { top: `${hang.top}px`, left: `${hang.left}px`, maxHeight: `${hang.height}px` } : { visibility: 'hidden' }"
        >
          <li
            v-for="project in projects"
            :key="project.path"
            :class="['note-option', { on: project.path === path }]"
            role="option"
            :aria-selected="project.path === path"
            @mousedown.prevent
            @click="pick(project.path)"
          >
            <span class="note-option-name">{{ project.name }}</span>
            <Icon v-if="project.path === path" name="check" :size="12" />
          </li>
        </ul>
      </div>
    </div>
    <textarea
      ref="field"
      v-model="note"
      class="note-text"
      rows="1"
      maxlength="8000"
      spellcheck="false"
      :aria-label="t('board.dialog.note')"
      :placeholder="t('board.dialog.notePlaceholder')"
      @paste="emit('paste', $event)"
    ></textarea>
    <!-- A press on a tile's button leaves the caret where it is. -->
    <div v-if="shots.length" class="note-shots" @mousedown.prevent>
      <CardImages :images="shots" removable flat @remove="emit('remove', $event)" />
    </div>
  </div>
</template>

<style scoped>
.note {
  flex: 0 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 16px 14px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.note-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.note-head .section-label {
  flex-shrink: 0;
  white-space: nowrap;
}

.note-picker {
  min-width: 0;
}

.note-project {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  max-width: 100%;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: 7px;
  background: var(--bg-control);
  font-size: 12px;
  color: var(--text);
}

.note-project:hover,
.note-project.on {
  background: var(--bg-control-hover);
}

.note-project-name,
.note-option-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Over the note and the footer, placed in the window itself (`hang`): nothing here cuts it. */
.note-projects {
  position: fixed;
  z-index: 1;
  box-sizing: border-box;
  min-width: 160px;
  max-width: 320px;
  margin: 0;
  padding: 4px;
  list-style: none;
  overflow-y: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--line-dialog);
  border-radius: 9px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
}

.note-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  height: 28px;
  padding: 0 8px;
  border-radius: 6px;
  font-size: 12px;
  white-space: nowrap;
  color: var(--text);
}

.note-option:hover {
  background: var(--bg-control-hover);
}

.note-option.on {
  color: var(--text-strong);
}

.note-text {
  flex-shrink: 0;
  box-sizing: border-box;
  width: 100%;
  min-height: 96px;
  max-height: 200px;
  padding: 10px 12px;
  border: 1px solid var(--btn-primary-line);
  border-radius: 8px;
  outline: none;
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  line-height: 1.5;
  caret-color: var(--run);
  resize: none;
}

.note-shots {
  flex-shrink: 0;
}
</style>
