<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import { markTone, resultLabel, type RunResult } from "../../lib/laps";

/**
 * A run's mark, in a round well: a tick when it was done, a cross when it failed, a dash when it
 * was skipped, silent or cut, a breathing ring while it goes. Its shape tells it as well as its
 * colour; its name is read out.
 */
const props = withDefaults(defineProps<{ result: RunResult | null; size?: number }>(), { size: 16 });

const tone = computed(() => markTone(props.result));
</script>

<template>
  <span :class="['mark', tone]" :style="{ width: `${size}px`, height: `${size}px` }" role="img" :aria-label="resultLabel(result)">
    <Icon v-if="tone === 'ok'" name="check" :size="size - 6" />
    <Icon v-else-if="tone === 'bad'" name="close" :size="size - 6" />
    <Icon v-else-if="tone === 'grey'" name="minus" :size="size - 6" />
    <span v-else class="ring"></span>
  </span>
</template>

<style scoped>
.mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: 50%;
}

.mark :deep(svg) {
  stroke-width: 2.6;
}

.ok {
  background: var(--run-bg);
  color: var(--run);
}

.bad {
  background: var(--crash-bg);
  color: var(--crash);
}

.grey {
  background: var(--bg-tag);
  color: var(--text-muted);
}

.going {
  background: var(--claude-bg);
}

.ring {
  width: 7px;
  height: 7px;
  border: 1.5px solid var(--claude);
  border-radius: 50%;
  animation: breathe 1.6s ease-in-out infinite;
}

@keyframes breathe {
  50% {
    opacity: 0.35;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ring {
    animation: none;
  }
}
</style>
