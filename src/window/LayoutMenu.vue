<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import Icon from "../components/Icon.vue";
import { t } from "../lib/i18n";
import { isMac } from "../lib/platform";
import { preset, PRESETS, type PaneNode, type PresetId } from "../lib/panes";

/**
 * The terminal panel's "Düzen": how many panes its terminals take, Tek (one at a time) to eight,
 * and "Equalize sizes". Each layout is drawn as it would come out in the panel's room now; one that
 * can't fit there, even with the panel as tall as it gets, is off.
 */
const props = defineProps<{
  /** The layout the panes read as now (Tek: none of them). */
  current: PresetId | null;
  /** What the button says after its name: "Tek", the panes' count, "1/4" while one is zoomed. */
  count: string;
  /** The room the panes would have, for the drawings. */
  room: { w: number; h: number };
  /** Whether a layout fits, the panel at its tallest. */
  fits: (id: PresetId) => boolean;
  /** Equalize means something: panes are on. */
  panes: boolean;
}>();

const emit = defineEmits<{ pick: [id: PresetId]; even: [] }>();

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const list = ref<HTMLElement | null>(null);
const open = ref(false);
/** The item under the pointer or the keyboard: the layouts, then "Equalize sizes". */
const active = ref(0);
const place = ref<Record<string, string>>({});

const LABELS: Record<PresetId, () => string> = {
  "1": () => t("panes.single"),
  "2": () => "2",
  "2s": () => t("panes.twoStacked"),
  "3": () => "3",
  "4": () => "4",
  "6": () => "6",
  "8": () => "8",
};

/** A layout drawn small: its panes' places in a 44 × 28 box, 2 px apart. */
type Box = { x: number; y: number; w: number; h: number };
const THUMB = { w: 44, h: 28, gap: 2 };

function boxes(node: PaneNode | null): Box[] {
  if (!node) return [{ x: 0, y: 0, w: THUMB.w, h: THUMB.h }];
  const out: Box[] = [];
  const walk = (at: PaneNode, x: number, y: number, w: number, h: number): void => {
    if (at.kind === "pane") {
      out.push({ x, y, w, h });
      return;
    }
    const across = at.dir === "row";
    const total = (across ? w : h) - THUMB.gap * (at.kids.length - 1);
    let from = across ? x : y;
    at.kids.forEach((kid, i) => {
      const length = total * at.sizes[i];
      if (across) walk(kid, from, y, length, h);
      else walk(kid, x, from, w, length);
      from += length + THUMB.gap;
    });
  };
  walk(node, 0, 0, THUMB.w, THUMB.h);
  return out;
}

// Worked out again each time the menu opens: the window may have changed size since.
const items = computed(() => {
  void open.value;
  return PRESETS.map((each) => ({
    id: each.id,
    label: LABELS[each.id](),
    boxes: boxes(each.id === "1" ? null : (preset(each.id, [], Math.max(props.room.w, 1), Math.max(props.room.h, 1)) ?? fallback(each.id))),
    off: !props.fits(each.id),
  }));
});

/** One that doesn't fit the room now is drawn as it would in a room it fits. */
function fallback(id: PresetId): PaneNode | null {
  return preset(id, [], 1456, 1080);
}

const EVEN = PRESETS.length;
const evenKeys = isMac ? "⌥⌘0" : "Ctrl+Shift+0";

function move(index: number): void {
  active.value = Math.max(0, Math.min(EVEN, index));
}

function show(): void {
  const box = button.value?.getBoundingClientRect();
  if (!box || open.value) return;
  const tall = 214;
  const below = window.innerHeight - box.bottom - 12 >= tall;
  place.value = below ? { top: `${box.bottom + 4}px`, right: `${window.innerWidth - box.right}px` } : { bottom: `${window.innerHeight - box.top + 4}px`, right: `${window.innerWidth - box.right}px` };
  open.value = true;
  move(Math.max(0, PRESETS.findIndex((each) => each.id === props.current)));
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
  button.value?.focus();
}

function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

function choose(index: number, keyed = false): void {
  if (index === EVEN) {
    if (!props.panes) return;
    if (keyed) close();
    else hide();
    emit("even");
    return;
  }
  const item = items.value[index];
  if (!item || item.off) return;
  if (keyed) close();
  else hide();
  emit("pick", item.id);
}

/** Four to a row: the arrows move as on the grid, ↓ from the last row to "Equalize sizes". */
function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const at = active.value;
  switch (event.key) {
    case "ArrowRight":
      move(at + 1);
      break;
    case "ArrowLeft":
      move(at - 1);
      break;
    case "ArrowDown":
      move(at === EVEN ? EVEN : at + 4 >= EVEN ? EVEN : at + 4);
      break;
    case "ArrowUp":
      move(at === EVEN ? EVEN - 1 : at - 4 < 0 ? at : at - 4);
      break;
    case "Home":
      move(0);
      break;
    case "End":
      move(EVEN);
      break;
    case "Enter":
    case " ":
      choose(at, true);
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
  <div ref="root" class="lmenu">
    <button
      ref="button"
      type="button"
      :class="['lmenu-button', { on: open }]"
      aria-haspopup="menu"
      :aria-expanded="open"
      :title="t('panes.layoutTitle')"
      @click="open ? hide() : show()"
      @keydown.down.prevent="show"
    >
      <Icon name="layout" :size="13" />
      <span class="lmenu-text">{{ t("panes.layout") }}</span>
      <!-- One width whatever it says: the widest it can say sets it. -->
      <span class="lmenu-count">
        <span class="lmenu-sizer" aria-hidden="true">{{ t("panes.single") }}</span>
        <span class="lmenu-sizer" aria-hidden="true">8/8</span>
        <span>{{ count }}</span>
      </span>
    </button>

    <div
      v-if="open"
      ref="list"
      class="lmenu-list"
      role="menu"
      tabindex="-1"
      :aria-label="t('panes.layout')"
      :style="place"
      @keydown="onKey"
      @mousedown.prevent
    >
      <span class="lmenu-label" aria-hidden="true">{{ t("panes.panes") }}</span>
      <div class="lmenu-grid">
        <button
          v-for="(item, i) in items"
          :key="item.id"
          type="button"
          role="menuitemradio"
          tabindex="-1"
          :aria-checked="item.id === current"
          :disabled="item.off"
          :class="['lmenu-item', { on: i === active, checked: item.id === current }]"
          @mousemove="move(i)"
          @click="choose(i)"
        >
          <span class="lmenu-thumb" aria-hidden="true">
            <span v-for="(b, j) in item.boxes" :key="j" :style="{ left: `${b.x}px`, top: `${b.y}px`, width: `${b.w}px`, height: `${b.h}px` }"></span>
          </span>
          <span class="lmenu-name">{{ item.label }}</span>
        </button>
      </div>
      <span class="lmenu-line" aria-hidden="true"></span>
      <button
        type="button"
        role="menuitem"
        tabindex="-1"
        :disabled="!panes"
        :class="['lmenu-even', { on: active === EVEN }]"
        @mousemove="move(EVEN)"
        @click="choose(EVEN)"
      >
        <span class="lmenu-even-text">{{ t("panes.equalize") }}</span>
        <span class="lmenu-keys">{{ evenKeys }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.lmenu {
  display: flex;
  flex-shrink: 0;
}

/* Drawn as "History" is: no ground, its colour alone says it is hovered or open. */
.lmenu-button {
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

.lmenu-button:hover,
.lmenu-button.on {
  color: var(--text-muted);
}

.lmenu-button.on {
  color: var(--text-lit);
}

.lmenu-count {
  display: inline-grid;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
}

.lmenu-count > span {
  grid-area: 1 / 1;
  text-align: left;
}

.lmenu-sizer {
  visibility: hidden;
}

.lmenu-button.on .lmenu-count {
  color: var(--text-muted);
}

/* A narrow row: the mark and the count alone. */
@container (max-width: 455px) {
  .lmenu-text {
    display: none;
  }
}

.lmenu-list {
  position: fixed;
  z-index: 60;
  width: 306px;
  box-sizing: border-box;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-card);
  background: var(--bg-control);
  box-shadow: var(--shadow-menu);
}

.lmenu-list:focus-visible {
  outline: none;
}

.lmenu-label {
  padding: 2px 4px 0;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-faint);
}

:lang(zh) .lmenu-label,
:lang(ja) .lmenu-label {
  letter-spacing: 0;
}

.lmenu-grid {
  display: grid;
  grid-template-columns: repeat(4, 64px);
  justify-content: space-between;
  gap: 6px;
}

.lmenu-item {
  width: 64px;
  height: 60px;
  padding: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 7px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  font-size: 11.5px;
  color: var(--text-muted);
  white-space: nowrap;
}

.lmenu-item.on:not(:disabled) {
  background: var(--bg-control-on);
}

.lmenu-item.checked {
  color: var(--text-strong);
}

.lmenu-item:disabled {
  opacity: 0.4;
}

.lmenu-thumb {
  position: relative;
  width: 44px;
  height: 28px;
  flex-shrink: 0;
}

.lmenu-thumb span {
  position: absolute;
  border-radius: 2px;
  background: var(--line-heavy);
}

.lmenu-item.checked .lmenu-thumb span {
  background: var(--text-muted);
}

.lmenu-name {
  max-width: 60px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lmenu-line {
  height: 1px;
  margin: 2px 0;
  background: var(--line-strong);
}

.lmenu-even {
  height: 26px;
  padding: 0 6px;
  display: flex;
  align-items: center;
  gap: 12px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12.5px;
  color: var(--text-soft);
  white-space: nowrap;
}

.lmenu-even.on:not(:disabled) {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

.lmenu-even:disabled {
  opacity: 0.4;
}

.lmenu-even-text {
  flex-grow: 1;
  min-width: 0;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lmenu-keys {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-faint);
}
</style>
