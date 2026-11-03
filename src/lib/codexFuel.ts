// Codex's fuel as the window reads it (codex_usage.rs through the Fuel payload): its part of the
// payload, and when its limit refills. CX5's file.

import { computed, ref } from "vue";

import { agentsOn, multiAgent } from "./agents";
import { fuel } from "./store";
import type { Agent, AgentFuel, AgentsFuel, Fuel } from "./types";

/** The agent picked at the Fuel page's top, with more than one on: kept while the app runs, and
 * set by a click on an agent's fuel alert (`reveal-fuel`). */
export const fuelPicked = ref<Agent>("claude");

/** Codex's part of the Fuel payload: null while Codex is off (or an older core). */
export function codexFuel(fuel: Fuel | null): AgentFuel | null {
  return (fuel as AgentsFuel | null)?.codex ?? null;
}

/** The agents with a part on the Fuel page: Claude, and Codex while it is on and the core sends its. */
export const fuelAgents = computed<Agent[]>(() => agentsOn.value.filter((each) => each === "claude" || (each === "codex" && !!codexFuel(fuel.value))));

/** Whose fuel the Fuel page shows, and the sidebar's share beside it: the agent picked while more
 * than one has a part, else the one that has (Claude when none). */
export const fuelShown = computed<Agent>(() => (multiAgent.value && fuelAgents.value.length > 1 && fuelAgents.value.includes(fuelPicked.value) ? fuelPicked.value : (fuelAgents.value[0] ?? "claude")));

/** When Codex's limit a card can wait for refills (its 5-hour window, else the week), while the
 * reading knows a moment still ahead; null otherwise (no reading: no "at the refill" for Codex). */
export function codexRefillAt(fuel: Fuel | null, now: number): number | null {
  const windows = codexFuel(fuel)?.limits.usage?.windows ?? [];
  const lead = windows.find((window) => window.id === "five_hour") ?? windows.find((window) => window.id === "seven_day");
  return lead?.resetsAt != null && lead.resetsAt > now ? lead.resetsAt : null;
}
