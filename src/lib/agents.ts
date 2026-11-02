// The coding agents as the window sees them (src-tauri/src/agent.rs, the snapshot's `agents`):
// which are on and whether more than one is. With Claude alone on, every place that could name
// an agent looks as it did before there were agents (`multiAgent` false).

import { computed } from "vue";

import { snapshot } from "./store";
import type { Agent, AgentInfo } from "./types";

/** An agent's name: a brand, the same in every language, as the core names it too. */
const NAMES: Record<Agent, string> = { claude: "Claude", codex: "Codex" };

export function agentName(agent: Agent): string {
  return NAMES[agent] ?? NAMES.claude;
}

/** What a file or the core says of an agent: one of the words, else Claude. */
export function agentOf(value: unknown): Agent {
  return value === "codex" ? "codex" : "claude";
}

/** Every agent as the core lists them, on or not; empty until the first snapshot. */
export const agents = computed<AgentInfo[]>(() => snapshot.value?.agents ?? []);

/** The agents that are on, in the core's order; Claude alone until the core says otherwise. */
export const agentsOn = computed<Agent[]>(() => {
  const listed = agents.value;
  return listed.length ? listed.filter((agent) => agent.on).map((agent) => agent.id) : ["claude"];
});

/** More than one agent is on: the places that name an agent show which. */
export const multiAgent = computed(() => agentsOn.value.length > 1);

export function agentInfo(agent: Agent): AgentInfo | null {
  return agents.value.find((info) => info.id === agent) ?? null;
}

export function agentOn(agent: Agent): boolean {
  return agentsOn.value.includes(agent);
}
