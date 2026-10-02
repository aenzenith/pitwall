<script setup lang="ts">
import { computed } from "vue";

import { segments } from "../lib/fuzzy";

/** A name with the characters a search matched (`hits`, their places) stressed. */
const props = defineProps<{ text: string; hits: Set<number> }>();

const parts = computed(() => segments(props.text, props.hits));
</script>

<template>
  <span v-for="(seg, j) in parts" :key="j" :class="{ hit: seg.hit }">{{ seg.text }}</span>
</template>

<style scoped>
.hit {
  color: var(--run);
  font-weight: 600;
}
</style>
