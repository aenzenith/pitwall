<script setup lang="ts">
import { computed, nextTick, ref, useId } from "vue";

import { t } from "../lib/i18n";
import type { Theme } from "../lib/types";
import Icon from "./Icon.vue";

/**
 * How the app is drawn, picked in Settings › General: as the system is, dark, or light. Three
 * radios drawn as one segmented control, so the keyboard is the browser's own: the group is one
 * Tab stop and the arrow keys move and choose. A thumb slides under the one chosen. It only says
 * what was picked (`v-model`); whoever mounts it keeps the choice.
 */
const props = defineProps<{ modelValue: Theme }>();
const emit = defineEmits<{ "update:modelValue": [value: Theme] }>();

const OPTIONS = [
  { value: "system", icon: "monitor", label: "theme.system" },
  { value: "dark", icon: "moon", label: "theme.dark" },
  { value: "light", icon: "sun", label: "theme.light" },
] as const;

/** The radios' group: its own, so two switches on a page stay apart. */
const name = useId();

/** Where the thumb stands. A value that is none of the three (a core that sends no theme yet)
 * shows as the system's. */
const at = computed(() => Math.max(0, OPTIONS.findIndex((option) => option.value === props.modelValue)));

const group = ref<HTMLElement | null>(null);

function pick(value: Theme): void {
  emit("update:modelValue", value);
  // A pick that wasn't kept (the save failed): the radios go back to the one that is shown.
  void nextTick(() => {
    for (const input of group.value?.querySelectorAll("input") ?? []) input.checked = input.value === OPTIONS[at.value].value;
  });
}
</script>

<template>
  <div ref="group" class="theme-switch" role="radiogroup" :aria-label="t('theme.label')" :style="{ '--at': at }">
    <span class="thumb" aria-hidden="true"></span>
    <label v-for="(option, i) in OPTIONS" :key="option.value" :class="{ on: i === at }">
      <input type="radio" :name="name" :value="option.value" :checked="i === at" @change="pick(option.value)" />
      <Icon :name="option.icon" :size="14" />
      <span>{{ t(option.label) }}</span>
    </label>
  </div>
</template>

<style scoped>
/* As tall as the fields it sits with (32px). Three columns of one width, the widest label's and
   never under 104px, which fits the longest of the seven languages' (Russian, 102px): the switch
   is as wide in every language (318px, a field's width), and no label wraps. */
.theme-switch {
  position: relative;
  flex-shrink: 0;
  display: inline-grid;
  grid-template-columns: repeat(3, minmax(104px, 1fr));
  height: 32px;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 9px;
  background: var(--bg-seg);
}

/* Under the one chosen; it slides there. */
.thumb {
  position: absolute;
  top: 2px;
  bottom: 2px;
  left: 2px;
  width: calc((100% - 4px) / 3);
  border-radius: 6px;
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
  transform: translateX(calc(var(--at) * 100%));
  transition: transform 180ms cubic-bezier(0.3, 0.7, 0.2, 1);
}

label {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-width: 0;
  padding: 0 8px;
  border-radius: 6px;
  font-size: 12.5px;
  color: var(--text-muted);
  white-space: nowrap;
  transition: color 120ms ease;
}

label:hover {
  color: var(--text);
}

label.on {
  color: var(--text-strong);
}

/* The radio itself: over its whole segment, unseen. */
input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  opacity: 0;
  appearance: none;
  -webkit-appearance: none;
}

/* The keyboard's place, on the segment: the app's own ring. */
label:has(input:focus-visible) {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}

@media (prefers-reduced-motion: reduce) {
  .thumb,
  label {
    transition: none;
  }
}
</style>
