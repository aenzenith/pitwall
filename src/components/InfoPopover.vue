<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId } from "vue";

import Icon from "./Icon.vue";
import { t } from "../lib/i18n";

/**
 * An (i) beside a thing's name, and what there is to know of it in a popover over the page. It
 * opens on a click on the (i), or Return or Space on it, and nothing else; it closes on its ×, on
 * Esc (the dialog it stands in stays), on a click outside it or on the (i) again, and when the
 * keyboard goes elsewhere.
 *
 * Opened with the keyboard, the keyboard goes to the popover itself (a dialog never opens with a
 * button focused; Tab reaches its ×), and back to the (i) as it closes. Opened with a click, the
 * keyboard stays where it was: nothing in it wears a ring.
 *
 * What it holds scrolls in it when it is taller than the dialog leaves room for; its × stays.
 *
 * It stays inside the dialog it stands in (the window, outside one): under the (i), else over it,
 * else as near as it fits, over the (i) if it must. When the page scrolls it follows the (i), and
 * goes once the (i) is out of sight.
 */
defineProps<{
  /** What the (i) and the popover are read out as: what the popover holds. */
  label: string;
}>();

/** From the dialog's edges (px). */
const MARGIN = 8;
/** From the (i) (px). */
const GAP = 6;

const id = useId();
const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const pop = ref<HTMLElement | null>(null);

const open = ref(false);
const place = ref<Record<string, string>>({});
/** When Esc last closed it: the dialog's own answer to that same key is not wanted. */
let escapedAt = -Infinity;
let dialog: HTMLDialogElement | null = null;

/** What it may cover: the dialog's box, within the window. */
function room(): { left: number; top: number; right: number; bottom: number } {
  const box = dialog?.getBoundingClientRect();
  return {
    left: Math.max(0, box?.left ?? 0),
    top: Math.max(0, box?.top ?? 0),
    right: Math.min(window.innerWidth, box?.right ?? window.innerWidth),
    bottom: Math.min(window.innerHeight, box?.bottom ?? window.innerHeight),
  };
}

/** `keyed`: opened with the keyboard, which goes to the popover. */
function show(keyed: boolean): void {
  if (open.value) return;
  const within = room();
  // Drawn unseen first, no wider or taller than the dialog leaves, to be measured; then placed.
  place.value = {
    left: `${within.left + MARGIN}px`,
    top: `${within.top + MARGIN}px`,
    maxWidth: `${within.right - within.left - 2 * MARGIN}px`,
    maxHeight: `${within.bottom - within.top - 2 * MARGIN}px`,
    visibility: "hidden",
  };
  open.value = true;
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("resize", onResize);
  void nextTick(() => {
    settle();
    // Once it is drawn seen: nothing unseen takes the keyboard.
    if (keyed) void nextTick(() => pop.value?.focus({ preventScroll: true }));
  });
}

function settle(): void {
  const anchor = button.value?.getBoundingClientRect();
  const drawn = pop.value;
  if (!open.value || !anchor || !drawn) return;
  const within = room();
  const width = drawn.offsetWidth;
  const height = drawn.offsetHeight;
  const clamp = (value: number, low: number, high: number): number => Math.max(low, Math.min(value, high));

  const below = anchor.bottom + GAP;
  const above = anchor.top - GAP - height;
  let top: number;
  if (below + height <= within.bottom - MARGIN) top = below;
  else if (above >= within.top + MARGIN) top = above;
  // Room on neither side: from the roomier one, moved in until it fits.
  else top = within.bottom - anchor.bottom >= anchor.top - within.top ? below : above;

  place.value = {
    ...place.value,
    left: `${clamp(anchor.left - MARGIN, within.left + MARGIN, within.right - MARGIN - width)}px`,
    top: `${clamp(top, within.top + MARGIN, within.bottom - MARGIN - height)}px`,
    visibility: "visible",
  };
}

/** `back`: the keyboard, when it is in the popover, goes back to the (i). */
function hide(back = false): void {
  if (!open.value) return;
  const inside = !!pop.value?.contains(document.activeElement);
  open.value = false;
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("keydown", onKey, true);
  window.removeEventListener("scroll", onScroll, true);
  window.removeEventListener("resize", onResize);
  if (back && inside) button.value?.focus({ preventScroll: true });
}

/* ---------- what opens and closes it ---------- */

/** A click's `detail` is 0 when Return or Space pressed the (i). */
function onClick(event: MouseEvent): void {
  if (open.value) hide(true);
  else show(event.detail === 0);
}

/** A press outside it and its (i). */
function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape" || event.isComposing) return;
  escapedAt = performance.now();
  event.preventDefault();
  event.stopPropagation();
  hide(true);
}

/** The keyboard went to a control outside it. */
function onFocusOut(event: FocusEvent): void {
  const next = event.relatedTarget;
  if (next instanceof Node && !root.value?.contains(next)) hide();
}

/** The page moved under it (the keyboard bringing the (i) into view, too): it follows the (i), and
 * goes once the (i) is out of sight. */
function onScroll(event: Event): void {
  const scrolled = event.target;
  if (scrolled instanceof Node && pop.value?.contains(scrolled)) return;
  const anchor = button.value?.getBoundingClientRect();
  const port = scrolled instanceof Element ? scrolled.getBoundingClientRect() : null;
  if (!anchor || (port && (anchor.bottom <= port.top || anchor.top >= port.bottom))) hide(true);
  else settle();
}

function onResize(): void {
  hide(true);
}

function onDialogCancel(event: Event): void {
  if (open.value || performance.now() - escapedAt < 200) event.preventDefault();
}

onMounted(() => {
  dialog = root.value?.closest("dialog") ?? null;
  dialog?.addEventListener("cancel", onDialogCancel, true);
});

onBeforeUnmount(() => {
  dialog?.removeEventListener("cancel", onDialogCancel, true);
  hide();
});
</script>

<template>
  <div ref="root" class="info" @focusout="onFocusOut">
    <button
      ref="button"
      type="button"
      class="info-button"
      aria-haspopup="dialog"
      :aria-label="label"
      :aria-expanded="open"
      :aria-controls="open ? id : undefined"
      @click="onClick"
    >
      <Icon name="info" :size="14" />
    </button>
    <!-- Next to the (i) in the page's order: VoiceOver reads it right after it. -->
    <Transition name="info-pop">
      <div v-if="open" :id="id" ref="pop" class="info-pop" role="dialog" :aria-label="label" tabindex="-1" :style="place">
        <button type="button" class="info-close" :aria-label="t('common.close')" @click="hide(true)">
          <Icon name="close" :size="12" />
        </button>
        <div class="info-body">
          <slot />
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.info {
  display: inline-flex;
}

/* The (i): no taller, in its line, than the label it stands by. */
.info-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  margin: -3px 0;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: var(--text-subtle);
}

.info-button:hover,
.info-button[aria-expanded="true"] {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Over the dialog, as wide as what it holds; nothing of the label it stands by carries into it.
   Taller than it may be, what it holds scrolls under its ×, which stays. */
.info-pop {
  position: fixed;
  z-index: 60;
  display: flex;
  flex-direction: column;
  width: max-content;
  padding: 10px 14px 12px;
  border: 1px solid var(--line-bold);
  border-radius: var(--radius-card);
  background: var(--bg-tip);
  box-shadow: var(--shadow-tip);
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
  letter-spacing: normal;
  text-transform: none;
  text-align: start;
  white-space: normal;
  color: var(--text-soft);
}

/* The keyboard rests on it when it opens: no place to show, as a dialog isn't (tokens.css). */
.info-pop:focus-visible {
  outline: none;
}

.info-body {
  min-height: 0;
  overflow: auto;
  overscroll-behavior: contain;
}

/* It comes as the app's dialogs do (their `pop`), rising a little into place, and goes the same
   way back, a little quicker; with less motion asked for, it only fades. */
.info-pop-enter-active {
  animation: info-pop 0.16s ease-out;
}

.info-pop-leave-active {
  animation: info-pop 0.12s ease-in reverse;
  pointer-events: none;
}

@keyframes info-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@keyframes info-fade {
  from {
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .info-pop-enter-active,
  .info-pop-leave-active {
    animation-name: info-fade;
  }
}

/* The ×, in its top corner, over what it holds. */
.info-close {
  position: absolute;
  top: 6px;
  right: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
}

.info-close:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}
</style>
