<script setup lang="ts">
import { computed } from "vue";

import ClaudeSpark from "../../components/ClaudeSpark.vue";
import { agentOn } from "../../lib/agents";
import { agentKey, ago, namesAgent, rowAgent } from "../../lib/format";
import { t } from "../../lib/i18n";
import { PROMPT_LEAD_MS } from "../../lib/sessions";
import { api, now } from "../../lib/store";
import type { Project } from "../../lib/types";

const props = defineProps<{ project: Project }>();

/** "Mark as seen" marks what Claude finished: offered while a finished turn waits, not for an open
 * question or permission prompt alone, which keeps waiting until it is answered. */
const finished = computed(() => {
  const sessions = props.project.claudeSessions;
  // A turn that ended after a question or permission opened stays unseen through a look too.
  const asks = sessions.flatMap((session) => (session.phase === "waiting" && session.turn && session.turn.kind !== "finished" ? [session.turn.at - PROMPT_LEAD_MS] : []));
  const seeable = (at: number) => asks.every((from) => at <= from);
  const turn = props.project.claude;
  return (turn?.kind === "finished" && seeable(turn.at)) || sessions.some((session) => session.phase === "waiting" && session.turn?.kind === "finished" && seeable(session.turn.at));
});

/** Whose state the card shows: the most urgent session's agent, as the project's row has it; else
 * the agent the project starts while that one is on, Claude's otherwise. */
const agent = computed(() => {
  const chosen = props.project.settings.agent;
  return rowAgent(props.project) ?? (chosen && agentOn(chosen) ? chosen : "claude");
});

const claudeTitle = computed(() => {
  const turn = props.project.claude;
  if (!turn) return props.project.claudeWorking ? t(agentKey("detail.claudeWorking", agent.value)) : t("detail.noSession");
  return t(agentKey(({ finished: "detail.claudeFinished", asking: "detail.claudeAsking", permission: "detail.claudePermission" } as const)[turn.kind], agent.value));
});

/** The section's head: "Claude", or "Agent" while more than one agent is on (the list's column). */
const heading = computed(() => t(namesAgent.value ? "window.col.agent" : "window.col.claude"));
</script>

<template>
  <section class="block" :aria-label="heading">
    <div class="section-label">{{ heading }}</div>
    <div :class="['card', { hot: project.claude }]">
      <ClaudeSpark :state="project.claude ? 'waiting' : project.claudeWorking ? 'working' : 'quiet'" :agent="agent" />
      <div class="card-text">
        <span class="card-title">{{ claudeTitle }}</span>
        <span class="card-sub">
          {{
            project.claude
              ? t("detail.notLookedAt", { ago: ago(project.claude.at, now) })
              : t(project.claudeWorking ? "detail.workingHint" : "detail.sessionsHint")
          }}
        </span>
      </div>
      <button v-if="finished" type="button" class="seen" @click="api.markSeen(project.path)">{{ t("detail.markSeen") }}</button>
    </div>
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
/* Bare, with no box in any state. */
.card {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* Waiting: still no box; a solid spark that pings (components/ClaudeSpark), the title in Claude's
   colour and a filled button carry it instead. */
.card.hot .card-title {
  font-weight: 600;
  color: var(--claude-text);
}

.card.hot .card-sub {
  color: var(--claude-muted);
}

.card-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.card-title {
  font-size: 13px;
  font-weight: 500;
}

.card-sub {
  font-size: 12px;
  color: var(--text-muted);
}

.seen {
  flex-shrink: 0;
  height: 28px;
  padding: 0 12px;
  border: 0;
  border-radius: var(--radius-control);
  background: var(--claude);
  color: var(--on-claude);
  font-size: 12px;
  font-weight: 600;
}

.seen:hover {
  background: var(--claude-text);
}
</style>
