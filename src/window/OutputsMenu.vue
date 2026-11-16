<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import Icon from "../components/Icon.vue";
import { t } from "../lib/i18n";

/** An output of the project: the dev server's (`id` null), a command's or a Garage job's, and what
 * its run is at: running, failed (a crashed server), or neither. */
export type OutputItem = { id: string | null; name: string; state: "on" | "bad" | null };

/**
 * The terminal panel's outputs, at its tab row's right end: one button, however wide the row,
 * since side by side their names took the row (the user's word, 2026-11-16). It says the output
 * shown, else how many there are ("2 işlem"); its menu is Pitwall's own, a row per output with
 * its state's dot and its name (the user's word, the same day), the one shown checked.
 */
const props = defineProps<{
  items: OutputItem[];
  /** The output shown in the panel, by its id; `undefined` while a terminal shows. */
  shown: string | null | undefined;
}>();

const emit = defineEmits<{ pick: [id: string | null] }>();

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const list = ref<HTMLElement | null>(null);
const open = ref(false);
/** The row under the pointer or the keyboard. */
const active = ref(0);
const place = ref<Record<string, string>>({});
/** Opened from the keyboard: closing it gives the keyboard back to the button. */
let keyedOpen = false;

const ROW = 30;

const label = computed(() => {
  const shown = props.shown === undefined ? undefined : props.items.find((item) => item.id === props.shown);
  return shown?.name ?? t("terminal.processes", { count: props.items.length });
});
/** The button's dot: red when one failed, else green while one runs. */
const state = computed(() => (props.items.some((item) => item.state === "bad") ? "bad" : props.items.some((item) => item.state === "on") ? "on" : null));

/** Opens the menu over the button (under it when there is room), its right edge on the button's. */
function show(keyed = false): void {
  const box = button.value?.getBoundingClientRect();
  if (!box) return;
  if (open.value) return hide();
  keyedOpen = keyed;
  const tall = props.items.length * ROW + 12;
  const below = window.innerHeight - box.bottom - 8 >= tall;
  const right = `${Math.max(8, window.innerWidth - box.right)}px`;
  place.value = below ? { top: `${box.bottom + 4}px`, right } : { bottom: `${window.innerHeight - box.top + 4}px`, right };
  open.value = true;
  active.value = Math.max(0, props.items.findIndex((item) => item.id === props.shown));
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => list.value?.focus());
}

function hide(): void {
  open.value = false;
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

function close(): void {
  hide();
  if (keyedOpen) button.value?.focus();
}

/** A press outside closes it; one on its own button is that button's click, which closes it. */
function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

function pick(at: number): void {
  const item = props.items[at];
  if (!item) return;
  close();
  emit("pick", item.id);
}

function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const count = props.items.length;
  if (!count) return close();
  switch (event.key) {
    case "ArrowDown":
      active.value = (active.value + 1) % count;
      break;
    case "ArrowUp":
      active.value = (active.value - 1 + count) % count;
      break;
    case "Home":
      active.value = 0;
      break;
    case "End":
      active.value = count - 1;
      break;
    case "Enter":
    case " ":
      pick(active.value);
      break;
    case "Escape":
    case "Tab":
      close();
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

onBeforeUnmount(hide);
</script>

<template>
  <div ref="root" class="omenu">
    <button
      ref="button"
      type="button"
      :class="['omenu-button', { on: shown !== undefined, open }]"
      aria-haspopup="menu"
      :aria-expanded="open"
      aria-keyshortcuts="ArrowDown"
      :title="t('detail.output')"
      @click="show(false)"
      @keydown.down.prevent="show(true)"
    >
      <span v-if="state" :class="['dot', state]" aria-hidden="true"></span>
      <span class="omenu-name">{{ label }}</span>
      <Icon name="chevron-down" :size="11" />
    </button>

    <div v-if="open" ref="list" class="omenu-list" role="menu" tabindex="-1" :aria-label="t('detail.output')" :style="place" @keydown="onKey" @mousedown.prevent>
      <div
        v-for="(item, i) in items"
        :key="item.id ?? 'server'"
        role="menuitemradio"
        :aria-checked="item.id === shown"
        :class="['row', { hv: i === active, checked: item.id === shown }]"
        @mousemove="active = i"
        @click="pick(i)"
      >
        <span :class="['dot', item.state ?? 'idle']" aria-hidden="true"></span>
        <span class="row-name" :title="item.name">{{ item.name }}</span>
        <Icon v-if="item.id === shown" name="check" :size="13" class="row-check" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.omenu {
  display: flex;
  flex: 0 1 auto;
  min-width: 0;
}

/* Drawn as the tabs beside it are: no ground, its colour alone says it is hovered or shown. */
.omenu-button {
  min-width: 0;
  max-width: 180px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 6px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.omenu-button:hover,
.omenu-button.open {
  color: var(--text-muted);
}

.omenu-button.on {
  color: var(--text-lit);
}

.omenu-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* What a run is at: green while it runs, red once it failed (a crashed server), a ring when it
   is neither (it ended well, or the server is starting or stopping). */
.dot {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  box-sizing: border-box;
  border-radius: 50%;
}

.dot.on {
  background: var(--run);
}

.dot.bad {
  background: var(--crash);
}

.dot.idle {
  border: 1px solid var(--text-faint);
}

/* The list, as "+"'s menu is drawn. */
.omenu-list {
  position: fixed;
  z-index: 70;
  min-width: 200px;
  max-width: 320px;
  box-sizing: border-box;
  padding: 5px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
  user-select: none;
}

.omenu-list:focus-visible {
  outline: none;
}

.row {
  height: 30px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: default;
}

.row.hv {
  background: var(--bg-lit);
}

.row.checked {
  color: var(--text-strong);
}

.row-name {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-check {
  flex-shrink: 0;
  color: var(--text-muted);
}
</style>
