<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import ClaudeDot from "../../components/ClaudeDot.vue";
import Icon from "../../components/Icon.vue";
import { WAIT_LABELS } from "../../lib/board";
import { language, t } from "../../lib/i18n";
import type { Project, Turn } from "../../lib/types";

/**
 * The board picker: the board shown (a project's, or every project's) and, under it, the list to
 * pick another from. It follows the boards out of sight: a project Claude waits on you in
 * (`waits`: a finished turn not seen yet, a question, a permission prompt) carries Claude's dot
 * in the list, and the picker itself on its corner. Never the project whose board shows: that
 * one is in sight. Picking a project is seeing it (the Board page marks it).
 *
 * The list is drawn here rather than as a native menu: a menu's line holds a check mark or an
 * image, never both, and none in Claude's colour.
 */
const props = defineProps<{
  projects: Project[];
  /** The project whose board shows; null for every project's. */
  scope: string | null;
  /** What Claude waits on you with on each project's board, by the project's path. */
  waits: Map<string, Turn["kind"]>;
}>();
const emit = defineEmits<{ pick: [path: string | null] }>();

/** A typed name is forgotten this long after its last letter (ms). */
const TYPED_MS = 700;

type Option = { path: string | null; name: string; label: string; waits: boolean };

/** Every project's board, then each project's: with Claude's dot, what it waits on you with too.
 * The board shown has none. */
const options = computed<Option[]>(() => [
  { path: null, name: t("board.allProjects"), label: t("board.allProjects"), waits: false },
  ...props.projects.map((p) => {
    const wait = p.path === props.scope ? undefined : props.waits.get(p.path);
    return { path: p.path, name: p.name, label: wait ? t(WAIT_LABELS[wait], { name: p.name }) : p.name, waits: !!wait };
  }),
]);

const shown = computed(() => props.projects.find((p) => p.path === props.scope) ?? null);
const name = computed(() => shown.value?.name ?? t("board.allProjects"));

/** The projects Claude waits in other than the one shown: the boards out of sight. Every
 * project's board has them all in sight. */
const elsewhere = computed(() => (shown.value ? props.projects.filter((p) => p.path !== props.scope && props.waits.has(p.path)) : []));

const label = computed(() => {
  const text = t("board.pickerLabel", { name: name.value });
  const names = elsewhere.value.map((p) => p.name).join(", ");
  return names ? `${text} · ${t("board.picker.waiting", { projects: names })}` : text;
});

/* ---------- the list ---------- */

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const list = ref<HTMLElement | null>(null);

const open = ref(false);
/** The option under the pointer or the keyboard. */
const active = ref(0);
/** Under the picker, their right edges level; no taller than the window leaves. */
const place = ref<Record<string, string>>({});

function optionId(index: number): string {
  return `board-picker-${index}`;
}

function move(index: number): void {
  active.value = index;
  void nextTick(() => list.value?.querySelector(".on")?.scrollIntoView({ block: "nearest" }));
}

function show(): void {
  const box = button.value?.getBoundingClientRect();
  if (!box || open.value) return;
  place.value = {
    top: `${box.bottom + 4}px`,
    right: `${window.innerWidth - box.right}px`,
    minWidth: `${box.width}px`,
    maxHeight: `${Math.max(120, window.innerHeight - box.bottom - 16)}px`,
  };
  open.value = true;
  move(Math.max(0, options.value.findIndex((option) => option.path === props.scope)));
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => list.value?.focus());
}

function hide(): void {
  open.value = false;
  typed = "";
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

/** Closed with the keyboard: it goes back to the picker. */
function close(): void {
  hide();
  button.value?.focus();
}

/** A press anywhere else closes it; one on the picker is the picker's own to answer. */
function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

/** `keyed`: picked with the keyboard, which then goes back to the picker; a click leaves it
 * nowhere, as a click on the picker does. */
function pick(option: Option, keyed = false): void {
  if (keyed) close();
  else hide();
  if (option.path !== props.scope) emit("pick", option.path);
}

let typed = "";
let typedTimer: number | undefined;

/** Letters go to the first board whose name starts with them, as in a menu. */
function type(letter: string): void {
  const lower = (text: string): string => text.toLocaleLowerCase(language.value);
  window.clearTimeout(typedTimer);
  typed += lower(letter);
  typedTimer = window.setTimeout(() => (typed = ""), TYPED_MS);
  const found = options.value.findIndex((option) => lower(option.name).startsWith(typed));
  if (found >= 0) move(found);
}

/** In the list: arrows, Home and End move, ↵ or Space picks, Esc and Tab close; a shortcut closes
 * it and stays the page's. */
function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const last = options.value.length - 1;

  switch (event.key) {
    case "ArrowDown":
      move(Math.min(last, active.value + 1));
      break;
    case "ArrowUp":
      move(Math.max(0, active.value - 1));
      break;
    case "Home":
      move(0);
      break;
    case "End":
      move(last);
      break;
    case "Enter":
    case " ":
      pick(options.value[active.value], true);
      break;
    case "Escape":
    case "Tab":
      close();
      break;
    default:
      if (event.key.length !== 1) return;
      type(event.key);
  }
  event.preventDefault();
  event.stopPropagation();
}

onBeforeUnmount(() => {
  hide();
  window.clearTimeout(typedTimer);
});
</script>

<template>
  <div ref="root" class="bpick">
    <button
      ref="button"
      type="button"
      class="bpick-button"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-label="label"
      :title="open ? undefined : label"
      @click="open ? hide() : show()"
      @keydown.down.prevent="show"
      @keydown.up.prevent="show"
    >
      <Icon v-if="shown" name="folder" :size="13" />
      <span class="bpick-text">{{ name }}</span>
      <Icon name="chevron-down" :size="11" />
      <span v-if="elsewhere.length" class="bpick-mark" aria-hidden="true"></span>
    </button>

    <!-- A press on an option leaves the keyboard in the list. -->
    <ul
      v-if="open"
      ref="list"
      class="bpick-list"
      role="listbox"
      tabindex="-1"
      :aria-label="t('common.projects')"
      :aria-activedescendant="optionId(active)"
      :style="place"
      @keydown="onKey"
      @mousedown.prevent
    >
      <template v-for="(option, i) in options" :key="option.path ?? ''">
        <!-- Every project's board, then a line, then the projects. -->
        <li v-if="i === 1" class="bpick-line" role="presentation"></li>
        <li
          :id="optionId(i)"
          :class="['bpick-option', { on: i === active }]"
          role="option"
          :aria-selected="option.path === scope"
          :aria-label="option.label"
          :title="option.waits ? option.label : undefined"
          @mousemove="active = i"
          @click="pick(option)"
        >
          <span class="bpick-check"><Icon v-if="option.path === scope" name="check" :size="12" /></span>
          <span class="bpick-name">{{ option.name }}</span>
          <ClaudeDot v-if="option.waits" :live="{ phase: 'waiting', text: '' }" />
        </li>
      </template>
    </ul>
  </div>
</template>

<style scoped>
/* A long project name gives way; the arrow never does. */
.bpick {
  flex-shrink: 1;
  display: flex;
  min-width: 90px;
  max-width: 220px;
}

.bpick-button {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
  color: #c7ccd3;
  white-space: nowrap;
}

.bpick-button:hover,
.bpick-button[aria-expanded="true"] {
  background: #2c3039;
}

.bpick-button svg {
  flex-shrink: 0;
}

.bpick-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Claude waits on you on another board: its dot on the picker's corner, as on a project's
   status. */
.bpick-mark {
  position: absolute;
  top: -3px;
  right: -3px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--claude);
  box-shadow: 0 0 0 2px var(--bg-app);
}

/* Over the page, wherever the bar's edges are: only the list scrolls. */
.bpick-list {
  position: fixed;
  z-index: 60;
  max-width: min(320px, calc(100vw - 16px));
  margin: 0;
  padding: 4px;
  list-style: none;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-card);
  background: var(--bg-control);
  box-shadow: 0 16px 32px -12px rgba(0, 0, 0, 0.75);
}

/* The keyboard's place is the lit option, not a ring round the list. */
.bpick-list:focus-visible {
  outline: none;
}

.bpick-option {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 28px;
  padding: 0 10px 0 8px;
  border-radius: 6px;
  font-size: 13px;
  color: #c7ccd3;
  white-space: nowrap;
}

.bpick-option.on {
  background: #2c3039;
  color: var(--text-strong);
}

/* The board shown: a check, in a place every option keeps so the names line up. */
.bpick-check {
  width: 12px;
  flex-shrink: 0;
  display: inline-flex;
  color: var(--text-muted);
}

.bpick-name {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bpick-option .claude-dot {
  margin-left: 8px;
}

.bpick-line {
  height: 1px;
  margin: 4px 6px;
  background: var(--line-strong);
}
</style>
