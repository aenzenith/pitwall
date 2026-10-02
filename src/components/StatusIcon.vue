<script setup lang="ts">
import { computed } from "vue";

import { t } from "../lib/i18n";
import type { Project } from "../lib/types";

const props = defineProps<{ project: Project; ring?: string }>();

// Shape as well as colour: filled = running/crashed, ring = stopped, corner dot = Claude.
const kind = computed(() => {
  if (props.project.status === "busy") return "busy";
  if (props.project.status === "running") return props.project.issue ? "warn" : "run";
  if (props.project.status === "crashed") return "crash";
  return "idle";
});
</script>

<template>
  <span class="status" :aria-label="t(`status.${project.status}`)">
    <span :class="['dot', kind]"></span>
    <span v-if="project.claude" class="claude" :style="{ boxShadow: `0 0 0 2px ${ring ?? 'var(--bg-panel)'}` }"></span>
    <span v-else-if="project.claudeWorking" class="working" :style="{ boxShadow: `0 0 0 2px ${ring ?? 'var(--bg-panel)'}` }"></span>
  </span>
</template>

<style scoped>
.status {
  position: relative;
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
}

.run {
  background: var(--run);
}

.warn {
  background: #cca700;
}

.crash {
  background: var(--crash);
}

.idle {
  width: 7px;
  height: 7px;
  border: 1.5px solid var(--idle-ring);
}

.busy {
  width: 9px;
  height: 9px;
  border: 1.5px solid var(--text-muted);
  border-top-color: transparent;
  animation: spin 0.8s linear infinite;
}

.claude {
  position: absolute;
  top: -1px;
  right: -1px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--claude);
}

/* Claude is mid-turn: a hollow ring that breathes; it fills in once Claude waits on you. */
.working {
  position: absolute;
  top: -1px;
  right: -1px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--bg-panel);
  border: 1.5px solid var(--claude);
  animation: breathe 1.6s ease-in-out infinite;
}

@keyframes breathe {
  50% {
    opacity: 0.35;
  }
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
