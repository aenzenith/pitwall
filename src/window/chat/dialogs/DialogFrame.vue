<script setup lang="ts">
import { onMounted, ref } from "vue";

import { useBackdropClose } from "../../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../../lib/dialogFocus";

/**
 * The frame of every chat dialog, as the app's other dialogs look (RenameDialog, ResumeDialog,
 * day/DayExportDialog): a modal `<dialog>` whose page behind is inert, so the keyboard stays in
 * it; Esc and a click outside close it through its own close event, and the focus goes back to
 * whatever had it. The keyboard starts on the element marked `data-focus` (a list, a search
 * field), else on the dialog itself. `hold`: nothing closes it (a save dialog of the system is
 * up over it). Many rows scroll inside (`.scroll` in dialog.css); the head and the foot stay.
 */
const props = withDefaults(defineProps<{ title: string; sub?: string; width?: number; hold?: boolean }>(), { sub: "", width: 440, hold: false });
/** `key`: every key pressed in the dialog, for its lists (`useListNav`). */
const emit = defineEmits<{ close: []; key: [event: KeyboardEvent] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();

onMounted(() => {
  const el = dialog.value;
  if (!el) return;
  const first = el.querySelector<HTMLElement>("[data-focus]");
  if (!first) return openUnfocused(el);
  el.showModal();
  first.focus({ preventScroll: true });
});

function onCancel(event: Event): void {
  if (props.hold) event.preventDefault();
}

function onBackdrop(event: MouseEvent): void {
  if (!props.hold) backdrop.click(event);
}

function close(): void {
  dialog.value?.close();
}

defineExpose({ close });
</script>

<template>
  <dialog
    ref="dialog"
    class="chat-dialog"
    :style="{ width: `${width}px` }"
    :aria-label="title"
    @cancel="onCancel"
    @close="emit('close')"
    @keydown="emit('key', $event)"
    @pointerdown="backdrop.down"
    @click="onBackdrop"
  >
    <div class="frame">
      <div class="head">
        <div class="titles">
          <div class="title">{{ title }}</div>
          <div v-if="sub" class="sub" :title="sub">{{ sub }}</div>
        </div>
        <slot name="tools" />
      </div>
      <slot />
      <div v-if="$slots.foot" class="foot">
        <slot name="foot" />
      </div>
    </div>
  </dialog>
</template>

<style scoped>
.chat-dialog {
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
  overflow: hidden;
}

.chat-dialog[open] {
  display: flex;
  flex-direction: column;
  animation: chat-dialog-pop 0.16s ease-out;
}

.frame {
  flex: 1 1 auto;
  min-height: 0;
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.head {
  flex-shrink: 0;
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.titles {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

.sub {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

@keyframes chat-dialog-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .chat-dialog[open] {
    animation: none;
  }
}
</style>
