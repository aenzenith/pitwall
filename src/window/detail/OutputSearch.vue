<script setup lang="ts">
import { ref } from "vue";

import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";

/**
 * The output's search field: what is looked for, which match of how many it stands on, and the
 * steps to the one before and the next. Return steps on, ⇧Return back, Esc closes.
 *
 * `position`: the match it stands on, from 0; -1 with none.
 */
defineProps<{ query: string; position: number; count: number }>();
const emit = defineEmits<{ "update:query": [text: string]; step: [by: 1 | -1]; close: [] }>();

const field = ref<HTMLInputElement | null>(null);

function onKey(event: KeyboardEvent): void {
  if (event.key === "Enter" && !event.isComposing) {
    event.preventDefault();
    emit("step", event.shiftKey ? -1 : 1);
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    emit("close");
  }
}

defineExpose({
  /** The keyboard to the field, what it holds selected. */
  focus: () => {
    field.value?.focus();
    field.value?.select();
  },
});
</script>

<template>
  <div class="find" role="search">
    <Icon name="search" :size="13" />
    <input
      ref="field"
      type="text"
      :value="query"
      :placeholder="t('output.search')"
      :aria-label="t('output.search')"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      autocorrect="off"
      @input="emit('update:query', ($event.target as HTMLInputElement).value)"
      @keydown="onKey"
    />
    <span v-if="query" class="count" aria-live="polite">{{ position + 1 }} / {{ count }}</span>
    <button type="button" class="step" :disabled="!count" :aria-label="t('output.searchPrevious')" :title="t('output.searchPrevious')" @click="emit('step', -1)">
      <Icon name="chevron-down" :size="13" class="up" />
    </button>
    <button type="button" class="step" :disabled="!count" :aria-label="t('output.searchNext')" :title="t('output.searchNext')" @click="emit('step', 1)">
      <Icon name="chevron-down" :size="13" />
    </button>
    <button type="button" class="step" :aria-label="t('output.searchClose')" :title="t('output.searchClose')" @click="emit('close')">
      <Icon name="close" :size="13" />
    </button>
  </div>
</template>

<style scoped>
.find {
  flex-shrink: 0;
  height: 30px;
  margin: 2px 20px 2px;
  padding: 0 3px 0 10px;
  display: flex;
  align-items: center;
  gap: 2px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text-subtle);
}

.find input {
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
  margin-left: 4px;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--text);
}

/* The field's own ring is the box's. */
.find:focus-within {
  border-color: var(--line-picked);
}

.count {
  flex-shrink: 0;
  padding: 0 4px;
  font-family: var(--font-mono);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.step {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.step:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.step:disabled {
  opacity: 0.45;
}

.step .up {
  transform: rotate(180deg);
}
</style>
