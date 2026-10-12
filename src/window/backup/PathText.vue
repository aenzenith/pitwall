<script setup lang="ts">
import { computed } from "vue";

/**
 * A path on one line that gives way before its end: its last parts (`keep` of them) stay whole,
 * what leads up to them is cut with an ellipsis. Its tooltip has all of it.
 */
const props = withDefaults(defineProps<{ path: string; keep?: number }>(), { keep: 2 });

/** Where the kept end starts: at the separator before the first of its parts. */
const cut = computed(() => {
  let at = props.path.length;
  for (let i = 0; i < props.keep && at > 0; i++) at = Math.max(props.path.lastIndexOf("/", at - 1), props.path.lastIndexOf("\\", at - 1));
  return Math.max(at, 0);
});
</script>

<template>
  <span :class="['path', { led: cut > 0 }]" :title="path">
    <span v-if="cut > 0" class="lead">{{ path.slice(0, cut) }}</span>
    <span class="end">{{ path.slice(cut) }}</span>
  </span>
</template>

<style scoped>
/* A grid, not a flex row: the end takes the room it needs first and the lead what is left. (Flex
   would shave a fraction of a pixel off the end as well, which is enough to cut a letter from it.) */
.path {
  display: inline-grid;
  grid-template-columns: minmax(0, max-content);
  min-width: 0;
  max-width: 100%;
  font-family: var(--font-mono);
  white-space: nowrap;
}

/* The lead keeps room for its ellipsis, which says something was cut. */
.path.led {
  grid-template-columns: minmax(2ch, 1fr) minmax(0, max-content);
}

.lead,
.end {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

</style>
