<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";

/**
 * A tooltip drawn by the page (lib/hoverTip says when): over its anchor, centred on it, and under
 * it when the window has no room above (a panel in a window of its own, a card at the top). Kept
 * inside the window; the pointer passes through it.
 */
const props = defineProps<{
  anchor: DOMRect;
  title: string;
  /** Quieter lines under the title. */
  lines?: string[];
}>();

/** Between the anchor and the tooltip, and the least room left at the window's edges. */
const GAP = 6;
const EDGE = 6;

const el = ref<HTMLElement | null>(null);
const place = ref<{ left: number; top: number } | null>(null);

function layout(): void {
  const drawn = el.value;
  if (!drawn) return;
  const { anchor } = props;
  const width = drawn.offsetWidth;
  const height = drawn.offsetHeight;
  const above = anchor.top - GAP - height;
  const top = above >= EDGE ? above : Math.min(anchor.bottom + GAP, window.innerHeight - height - EDGE);
  const left = Math.min(Math.max(anchor.left + anchor.width / 2 - width / 2, EDGE), window.innerWidth - width - EDGE);
  place.value = { left: Math.round(left), top: Math.round(Math.max(EDGE, top)) };
}

onMounted(layout);
watch(
  () => [props.anchor, props.title, props.lines?.join("\n")],
  () => void nextTick(layout),
);
</script>

<template>
  <Teleport to="body">
    <div
      ref="el"
      class="hover-tip"
      role="tooltip"
      :style="place ? { left: `${place.left}px`, top: `${place.top}px` } : { visibility: 'hidden' }"
    >
      <span class="title">{{ title }}</span>
      <span v-for="(line, i) in lines" :key="i" class="line">{{ line }}</span>
    </div>
  </Teleport>
</template>

<style scoped>
.hover-tip {
  position: fixed;
  left: 0;
  top: 0;
  z-index: 90;
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-width: min(360px, calc(100vw - 12px));
  box-sizing: border-box;
  padding: 6px 9px;
  border: 1px solid var(--line-bold);
  border-radius: 7px;
  background: var(--bg-tip);
  box-shadow: var(--shadow-tip);
  font-size: 12px;
  line-height: 1.35;
  pointer-events: none;
  user-select: none;
  -webkit-user-select: none;
}

.title {
  font-weight: 600;
  color: var(--text-strong);
  overflow-wrap: anywhere;
}

.line {
  font-size: 11.5px;
  color: var(--text-muted);
}
</style>
