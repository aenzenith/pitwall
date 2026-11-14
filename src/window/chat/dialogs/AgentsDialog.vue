<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t } from "../../../lib/i18n";
import type { ChatAgentInfo } from "../../../lib/chat/wire";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { modelName, projectName, send, useListNav, type DialogProps } from "./shared";

/**
 * Agents: the agents Claude Code lists for the session (`initialize.agents`), each with what it
 * is for and the model it runs; a pick makes it this chat's main agent
 * (`apply_flag_settings {agent}`) and closes. The CLI says which one is in use nowhere, so only
 * the one picked here is checked.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);

const agents = computed(() => (props.info?.agents ?? []).filter((agent) => typeof agent.name === "string"));
const chosen = ref<string | null>(null);
const busy = ref<string | null>(null);
const error = ref<string | null>(null);

function model(agent: ChatAgentInfo): string {
  if (!agent.model) return "";
  return agent.model === "inherit" ? t("chatDialog.agents.inherit") : modelName(props.info, agent.model);
}

async function pick(agent: ChatAgentInfo): Promise<void> {
  if (busy.value) return;
  busy.value = agent.name;
  error.value = await send(props.id, { kind: "setAgent", agent: agent.name });
  busy.value = null;
  if (error.value) return;
  chosen.value = agent.name;
  frame.value?.close();
}

const nav = useListNav(
  () => agents.value.length,
  (i) => void pick(agents.value[i]),
  listEl,
);
const wanted = props.arg?.trim().toLowerCase();
if (wanted)
  nav.active.value = Math.max(
    0,
    agents.value.findIndex((agent) => agent.name.toLowerCase() === wanted),
  );

const rowId = (i: number) => `chat-agent-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.agents.title')" :sub="projectName(path)" :width="500" @close="emit('close')" @key="nav.onKey">
    <p class="note">{{ t("chatDialog.agents.note") }}</p>

    <div v-if="!agents.length" class="state">
      <Spinner v-if="!info" :size="14" />
      <template v-else>{{ t("chatDialog.agents.empty") }}</template>
    </div>
    <div v-else ref="listEl" class="scroll list" role="listbox" tabindex="0" data-focus :aria-label="t('chatDialog.agents.title')" :aria-activedescendant="rowId(nav.active.value)">
      <div
        v-for="(agent, i) in agents"
        :id="rowId(i)"
        :key="agent.name"
        :data-row="i"
        role="option"
        :aria-selected="i === nav.active.value"
        :aria-checked="agent.name === chosen"
        :class="['row', 'tall', { active: i === nav.active.value }]"
        @click="
          nav.active.value = i;
          pick(agent);
        "
      >
        <span class="text">
          <span class="line">
            <span class="name">{{ agent.name }}</span>
            <span v-if="model(agent)" class="tag">{{ model(agent) }}</span>
          </span>
          <span v-if="agent.description" class="desc two" :title="agent.description">{{ agent.description }}</span>
        </span>
        <span class="mark end">
          <Spinner v-if="busy === agent.name" :size="12" />
          <Icon v-else-if="agent.name === chosen" name="check" :size="13" />
        </span>
      </div>
    </div>

    <NotAnswered v-if="error" :id="id" :state="state" :error="error" @done="frame?.close()" />
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list {
  max-height: 380px;
}

.row.tall {
  align-items: flex-start;
  padding-top: 6px;
  padding-bottom: 6px;
}

/* The check at the right stands level with the whole row. */
.row.tall .mark.end {
  align-self: center;
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
</style>
