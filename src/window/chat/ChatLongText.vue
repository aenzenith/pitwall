<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";

/**
 * A long message or output whole, over the conversation: its text selectable, a copy button,
 * closed with Esc, the close button or a press beside it.
 */
defineProps<{ title: string; text: string }>();
const emit = defineEmits<{ close: [] }>();

const box = ref<HTMLElement | null>(null);
const copied = ref(false);

function copy(text: string): void {
  void api
    .copyText(text)
    .then(() => {
      copied.value = true;
      setTimeout(() => (copied.value = false), 1200);
    })
    .catch(() => undefined);
}

function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}

onMounted(() => void nextTick(() => box.value?.focus()));
</script>

<template>
  <div class="long-scrim" @pointerdown.self="emit('close')">
    <div ref="box" class="long" role="dialog" :aria-label="title" tabindex="-1" @keydown="onKey">
      <div class="head">
        <span class="title">{{ title }}</span>
        <span class="lines">{{ t("chat.long.lines", { count: text.split("\n").length }) }}</span>
        <span class="grow"></span>
        <button type="button" class="icon" :title="t(copied ? 'chat.copied' : 'chat.action.copy')" :aria-label="t('chat.action.copy')" @click="copy(text)">
          <Icon :name="copied ? 'check' : 'copy'" :size="13" />
        </button>
        <button type="button" class="icon" :title="t('common.close')" :aria-label="t('common.close')" @click="emit('close')">
          <Icon name="close" :size="13" />
        </button>
      </div>
      <pre class="text selectable">{{ text }}</pre>
    </div>
  </div>
</template>

<style scoped>
.long-scrim {
  position: absolute;
  inset: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  background: var(--scrim);
}

.long {
  display: flex;
  flex-direction: column;
  width: min(100%, 980px);
  max-height: 100%;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-dialog);
  outline: none;
}

.head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
  height: 38px;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid var(--line-soft);
}

.title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  font-size: 13px;
  color: var(--text-strong);
}

.lines {
  flex-shrink: 0;
  font-size: 11.5px;
  color: var(--text-faint);
}

.grow {
  flex: 1;
}

.icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text-muted);
}

.icon:hover {
  background: var(--bg-control-hover);
  color: var(--text);
}

.text {
  flex: 1;
  min-height: 0;
  margin: 0;
  padding: 12px 14px 16px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text);
}
</style>
