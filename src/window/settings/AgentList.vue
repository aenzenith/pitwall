<script setup lang="ts">
import { ref } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import { agents } from "../../lib/agents";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { Agent, AgentDetail, AgentInfo } from "../../lib/types";

/**
 * Settings › Agents' left column: every agent with its mark, a line of how it stands, and its
 * switch. A click on a row shows that agent's part (`pick`). The switch is the user's word on
 * the agent (`agent_set_on`): once turned off, Pitwall never turns it on again. Claude's switch
 * is on and can't be moved: it is always on. An agent that isn't on this computer is faded, with
 * no switch.
 * Everything here goes to the core by its own command, never with Settings' form.
 */
defineProps<{ picked: Agent; details: AgentDetail[] }>();
const emit = defineEmits<{ pick: [agent: Agent] }>();

const error = ref("");

/** The row's second line: the agent's version once it is on and at work, else what stands in
 * the way. */
function state(info: AgentInfo, detail: AgentDetail | undefined): { text: string; waits: boolean } {
  const version = detail?.version ?? "";
  if (info.id === "claude") return { text: info.on ? version : t("settings.agents.state.off"), waits: false };
  if (!info.found) return { text: t("settings.agents.state.notFound"), waits: false };
  if (!info.on) return { text: t("settings.agents.state.off"), waits: false };
  if (info.hook === "waiting") return { text: t("settings.agents.state.approval"), waits: true };
  if (info.hook === "missing") return { text: t("settings.agents.state.noHook"), waits: false };
  return { text: version, waits: false };
}

async function turn(info: AgentInfo, event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  error.value = "";
  try {
    await api.agentSetOn(info.id, box.checked);
  } catch (reason) {
    // Not taken: the switch shows what is stored again.
    box.checked = info.on;
    error.value = String(reason);
  }
}
</script>

<template>
  <div class="agent-list" role="tablist" aria-orientation="vertical" :aria-label="t('settings.agents')" @change.stop>
    <div v-for="info in agents" :key="info.id" :class="['agent-row', { on: picked === info.id, faded: !info.found }]" @click="emit('pick', info.id)">
      <button type="button" role="tab" class="agent-pick" :aria-selected="picked === info.id" :tabindex="picked === info.id ? 0 : -1">
        <AgentMark :agent="info.id" :size="18" />
        <span class="agent-text">
          <span class="agent-name">{{ info.name }}</span>
          <span :class="['agent-state', { waits: state(info, details.find((detail) => detail.id === info.id)).waits }]">
            {{ state(info, details.find((detail) => detail.id === info.id)).text }}
          </span>
        </span>
      </button>
      <input
        v-if="info.on || info.found"
        type="checkbox"
        role="switch"
        :checked="info.on"
        :aria-label="info.name"
        @click.stop
        @change="turn(info, $event)"
      />
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.agent-list {
  width: 178px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 12px 8px;
  border-right: 1px solid var(--line);
  overflow-y: auto;
}

/* The whole row picks; its switch only turns the agent on or off. */
.agent-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 9px 0 0;
  border-radius: 8px;
}

.agent-row:hover {
  background: var(--bg-rail-hover);
}

.agent-row.on {
  background: var(--bg-rail-on);
}

.agent-row.faded {
  opacity: 0.55;
}

.agent-pick {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 0 9px 9px;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: left;
}

.agent-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.agent-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-strong);
}

.agent-state {
  min-height: 1lh;
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Waiting for the user's approval in the agent: the colour of a wait. */
.agent-state.waits {
  color: var(--claude-text);
}

.error {
  padding: 0 9px;
  font-size: 12px;
}

/* Claude's: always on. */
input[role="switch"]:disabled {
  cursor: default;
}
</style>
