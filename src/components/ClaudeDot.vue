<script setup lang="ts">
// Claude's live state before a name, as the project list shows it: a filled dot while it waits
// on you (a notification not read yet), a breathing ring while it works, grey when neither.
// `text` says which.
defineProps<{ live?: { phase: "waiting" | "working"; text: string } | null }>();
</script>

<template>
  <span :class="['claude-dot', live?.phase ?? 'idle']" :title="live?.text || undefined" :aria-label="live?.text || undefined"></span>
</template>

<style scoped>
.claude-dot {
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  box-sizing: border-box;
  border-radius: 50%;
}

.idle {
  background: #5c626c;
}

.waiting {
  background: var(--claude);
}

.working {
  border: 1.5px solid var(--claude);
  animation: breathe 1.6s ease-in-out infinite;
}

@keyframes breathe {
  50% {
    opacity: 0.35;
  }
}

@media (prefers-reduced-motion: reduce) {
  .working {
    animation: none;
  }
}
</style>
