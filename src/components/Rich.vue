<script setup lang="ts">
import { computed } from "vue";

/** A translated text with markup in it: each `{name}` placeholder renders the slot of that name. */
const props = defineProps<{ text: string }>();

const parts = computed(() => props.text.split(/\{(\w+)\}/).map((value, i) => ({ value, slot: i % 2 === 1 })));
</script>

<template>
  <template v-for="(part, i) in parts" :key="i">
    <slot v-if="part.slot" :name="part.value" />
    <template v-else>{{ part.value }}</template>
  </template>
</template>
