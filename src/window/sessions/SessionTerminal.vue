<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";
import { COLLAPSE_MS, COLLAPSED_HEIGHT, detailTerminalCollapsed, detailTerminalHeight, useResizer } from "../../lib/panel";
import { attach, fitView } from "../../lib/terminals";
import type { TerminalView } from "../../lib/types";

/**
 * The Pitwall terminal a session runs in, under the session's details: the view the project's
 * terminal panel shows (lib/terminals), moved here while the session is selected and sized to this
 * column. It takes the keyboard only when clicked, so the list keeps its arrow keys. `hold`: the
 * keyboard is in it, so the page keeps the session where it is meanwhile.
 *
 * Pinned under the details' sections with its own height (lib/panel: detailTerminalHeight), as the
 * project's terminal panel is under the list, and with that panel's handle (useResizer): down to
 * its minimum, and up over the sections, which scroll in what it leaves, then over the head to
 * the details' top. It collapses as that panel does, to its handle and label row.
 */
const props = defineProps<{ terminal: TerminalView }>();
const emit = defineEmits<{ hold: [on: boolean] }>();

/** The details' head (panel.css): the sections' room starts under it. */
const HEAD = 100;

const panel = ref<HTMLElement | null>(null);
const screen = ref<HTMLElement | null>(null);
const viewport = ref<HTMLElement | null>(null);
const collapsed = detailTerminalCollapsed;
/** The details' whole height, the head included: all it can take. */
const full = ref(Number.POSITIVE_INFINITY);
/** The height it shows: the kept one, as far as the details go; collapsed, its handle and label
 * row, and the kept one comes back on expanding. Its top line lies outside it (the terminal
 * panel's is inside), so collapsed it is that line less: the two rows come out alike. */
const height = computed(() => (collapsed.value ? COLLAPSED_HEIGHT - 1 : Math.min(detailTerminalHeight.value, full.value)));
/** How far it stands over the head, once it is taller than the room under it. */
const over = computed(() => Math.max(0, height.value - (full.value - HEAD)));

// The head's line is a rest on the way up: near it the height takes it, and arrow keys stop at it.
const resizer = useResizer(detailTerminalHeight, () => full.value, () => full.value - HEAD);

/** While it slides, the shell keeps its size: no resize per frame, no squeeze. */
const animating = ref(false);
let settle: ReturnType<typeof setTimeout> | undefined;

/** The keyboard leaves a terminal that goes out of sight. */
function toggleCollapsed(): void {
  collapsed.value = !collapsed.value;
  if (collapsed.value && screen.value?.contains(document.activeElement)) (document.activeElement as HTMLElement).blur();
  animating.value = true;
  clearTimeout(settle);
  settle = setTimeout(() => {
    animating.value = false;
    if (!collapsed.value) fitView(props.terminal.id);
  }, COLLAPSE_MS + 40);
}

/** The handle doesn't move a collapsed terminal; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!collapsed.value) resizer.start(event);
}

function nudge(event: KeyboardEvent): void {
  if (!collapsed.value) resizer.nudge(event);
}

/** The keyboard is in it (clicked, or its session brought up): a collapsed one opens. */
function onFocusIn(): void {
  emit("hold", true);
  if (collapsed.value) toggleCollapsed();
}

function measureDetails(): void {
  const details = panel.value?.parentElement;
  if (details) full.value = details.clientHeight;
}

/** Collapsed or sliding, it isn't sized to what shows of it. */
function show(): void {
  if (viewport.value) attach(props.terminal.id, viewport.value, !collapsed.value && !animating.value, false);
}

// The details or the screen changed size: the details are measured again, the shell follows.
const observer = new ResizeObserver(() => {
  measureDetails();
  if (!collapsed.value && !animating.value) fitView(props.terminal.id);
});

onMounted(() => {
  measureDetails();
  show();
  if (viewport.value) observer.observe(viewport.value);
  if (panel.value?.parentElement) observer.observe(panel.value.parentElement);
});

// Another session's terminal takes its place: the keyboard was in the one that left.
watch(
  () => props.terminal.id,
  () => {
    emit("hold", false);
    show();
  },
);

// Taken out with the keyboard in it, no focus leaves to say so.
onBeforeUnmount(() => {
  observer.disconnect();
  clearTimeout(settle);
  emit("hold", false);
});
</script>

<template>
  <section
    ref="panel"
    :class="['term', { animated: animating, collapsed }]"
    :style="{ height: `${height}px`, marginTop: over ? `-${over}px` : undefined }"
    :aria-label="t('sessions.origin.pitwall')"
  >
    <div
      class="grip"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('sessions.terminalResize')"
      tabindex="0"
      :aria-valuenow="Math.round(height)"
      @pointerdown="startDrag"
      @pointermove="resizer.move"
      @pointerup="resizer.end"
      @pointercancel="resizer.end"
      @keydown="nudge"
    ></div>
    <div class="term-head">
      <span class="section-label">{{ t("sessions.origin.terminal") }}</span>
      <button
        type="button"
        :class="['icon', 'collapse', { up: collapsed }]"
        :aria-expanded="!collapsed"
        :aria-label="t(collapsed ? 'sessions.terminalExpand' : 'sessions.terminalCollapse')"
        :title="t(collapsed ? 'common.expand' : 'common.collapse')"
        @click="toggleCollapsed"
      >
        <Icon name="chevron" :size="13" />
      </button>
    </div>
    <div ref="screen" class="screen" @focusin="onFocusIn" @focusout="emit('hold', false)">
      <div ref="viewport" class="viewport"></div>
    </div>
  </section>
</template>

<style scoped>
/* Pinned under the details' sections (panel.css: .body.over-terminal) with its own height, set on
   it, as the terminal panel is under the list (TerminalPanel), with that panel's shadow. Taller
   than the room under the head, it stands over the head by its margin, also set on it. Its top
   line lies on the last section's own where they meet. */
.term {
  position: relative;
  z-index: 1;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 0 20px 20px;
  overflow: clip;
  background: var(--bg-detail);
  box-shadow:
    0 -1px 0 var(--line),
    0 -12px 24px rgba(0, 0, 0, 0.22);
}

/* Collapsing and expanding slide (COLLAPSE_MS in lib/panel); a drag follows the pointer. */
.term.animated {
  transition: height 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.term.collapsed .grip {
  cursor: default;
}

.term.collapsed .grip::after {
  opacity: 0;
}

/* The terminal panel's handle (TerminalPanel). */
.grip {
  flex-shrink: 0;
  height: 9px;
  cursor: row-resize;
  position: relative;
  outline: none;
}

.grip::after {
  content: "";
  position: absolute;
  left: 50%;
  top: 3px;
  width: 36px;
  height: 3px;
  margin-left: -18px;
  border-radius: 2px;
  background: #2f343d;
}

.grip:hover::after {
  background: #4a515c;
}

/* The handle's own focus: the bar in the focus ring's colour. */
.grip:focus-visible::after {
  background: var(--focus-ring);
}

/* As tall as the terminal panel's tab row: with the handle, what a collapsed one keeps. */
.term-head {
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  margin-bottom: 5px;
}

.term-head .section-label {
  flex-shrink: 0;
  white-space: nowrap;
}

/* The terminal panel's collapse button, where that panel has it: 12 px from the edge. */
.icon {
  width: 24px;
  height: 24px;
  margin: 0 -8px 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.icon:hover {
  background: #262a33;
  color: var(--text-strong);
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

/* The terminal panel's screen (TerminalPanel). */
.screen {
  flex: 1 1 auto;
  min-height: 0;
  padding: 8px 4px 8px 10px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid #1f2228;
  display: flex;
  overflow: clip;
  transition: opacity 160ms ease;
}

.term.collapsed .screen {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .term.animated,
  .screen,
  .collapse :deep(svg) {
    transition: none;
  }
}

/* Nothing inside the terminal can scroll it or anything around it. */
.viewport {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow: clip;
}

.viewport :deep(.terminal-host),
.viewport :deep(.xterm) {
  height: 100%;
}

/* xterm paints its scroll area black; below the last row it would show as a strip. */
.viewport :deep(.xterm-viewport) {
  background-color: var(--bg-log) !important;
}
</style>
