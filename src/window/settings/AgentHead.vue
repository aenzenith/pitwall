<script setup lang="ts">
import { computed } from "vue";

import { agentName } from "../../lib/agents";
import type { Agent, AgentDetail } from "../../lib/types";

/**
 * The head of an agent's part of Settings › Agents: its name, and under it its CLI with the
 * version it said and its home folder (`agent_details`). No logo: the list beside it has them.
 * `faded`: the agent isn't on this computer, and the name stands alone.
 */
const props = defineProps<{ agent: Agent; detail: AgentDetail | null; faded?: boolean }>();

/** Each agent's CLI, by its own name: brands, the same in every language. */
const CLI: Record<Agent, string> = { claude: "Claude Code", codex: "Codex CLI" };

const line = computed(() => {
  const cli = props.detail?.version ? `${CLI[props.agent]} ${props.detail.version}` : CLI[props.agent];
  return props.detail?.home ? `${cli} · ${props.detail.home}` : cli;
});
</script>

<template>
  <div :class="['agent-head', { faded }]">
    <span class="agent-name">{{ agentName(agent) }}</span>
    <span v-if="!faded" class="agent-line">{{ line }}</span>
  </div>
</template>

<style scoped>
.agent-head {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.agent-name {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-strong);
}

.faded .agent-name {
  opacity: 0.6;
}

.agent-line {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  overflow-wrap: anywhere;
}
</style>
