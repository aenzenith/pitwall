<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

import { t } from "../../lib/i18n";
import { attach, fitView } from "../../lib/terminals";
import type { TerminalView } from "../../lib/types";

/**
 * The Pitwall terminal a session runs in, under the session's details: the view the project's
 * terminal panel shows (lib/terminals), moved here while the session is selected and sized to this
 * column. It takes the keyboard only when clicked, so the list keeps its arrow keys. `hold`: the
 * keyboard is in it, so the page keeps the session where it is meanwhile.
 */
const props = defineProps<{ terminal: TerminalView }>();
const emit = defineEmits<{ hold: [on: boolean] }>();

const viewport = ref<HTMLElement | null>(null);

function show(): void {
  if (viewport.value) attach(props.terminal.id, viewport.value, true, false);
}

const observer = new ResizeObserver(() => fitView(props.terminal.id));

onMounted(() => {
  show();
  if (viewport.value) observer.observe(viewport.value);
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
  emit("hold", false);
});
</script>

<template>
  <section class="term" :aria-label="t('sessions.origin.pitwall')">
    <div class="term-head">
      <span class="section-label">{{ t("sessions.origin.terminal") }}</span>
      <span class="tab-name" :title="terminal.name">{{ terminal.name }}</span>
    </div>
    <div class="screen" @focusin="emit('hold', true)" @focusout="emit('hold', false)">
      <div ref="viewport" class="viewport"></div>
    </div>
  </section>
</template>

<style scoped>
/* Takes what the sections above leave, down to a few rows; past that the details scroll. */
.term {
  flex: 1 1 auto;
  min-height: 240px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px 20px 0;
}

.term-head {
  flex-shrink: 0;
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.term-head .section-label {
  flex-shrink: 0;
  white-space: nowrap;
}

/* Its tab in the project's terminal panel. */
.tab-name {
  margin-left: auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
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
