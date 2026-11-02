// How a Claude session is started from Pitwall: with which model, in which mode and, for a board
// card, when. The lists the window offers (the core keeps the same two and takes nothing else:
// src-tauri/src/start.rs), a project's own start, and what the Fuel reading says of a model's
// week and of the session's refill. Pure but for the language (`t`).

import { agentOf, agentOn, agentsOn } from "./agents";
import { clockText, current, left, LOW_LEFT, percentText } from "./fuel";
import { t, type Key } from "./i18n";
import { projectSettings } from "./projectSettings";
import type { Agent, AgentStartChoice, ClaudeMode, ClaudeModel, ClaudeStart, CodexChoice, CodexMode, CodexModel, Fuel, Project, StartChoice, UsageWindow } from "./types";

/** The models offered, by the alias Claude Code's `--model` takes. */
export const MODELS: ClaudeModel[] = ["fable", "opus", "sonnet", "haiku"];

export const MODES: ClaudeMode[] = ["normal", "plan", "acceptEdits"];

/** A model's name. The alias stands for the newest model of that name, whichever it is: no
 * version is shown. */
export const MODEL_LABELS: Record<ClaudeModel, Key> = {
  fable: "start.model.fable",
  opus: "start.model.opus",
  sonnet: "start.model.sonnet",
  haiku: "start.model.haiku",
};

export const MODE_LABELS: Record<ClaudeMode, Key> = {
  normal: "start.mode.normal",
  plan: "start.mode.plan",
  acceptEdits: "start.mode.acceptEdits",
};

/** A model's name; Claude Code's own choice for none. */
export function modelLabel(model: ClaudeModel | null): string {
  return t(model ? MODEL_LABELS[model] : "start.model.auto");
}

/** Codex's models, by the id its `-m` takes, in the order offered; shown as the ids themselves. */
export const CODEX_MODELS: CodexModel[] = ["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-astra", "gpt-6-luna"];

export const CODEX_MODES: CodexMode[] = ["normal", "readOnly"];

export const CODEX_MODE_LABELS: Record<CodexMode, Key> = {
  normal: "start.mode.normal",
  readOnly: "start.codexMode.readOnly",
};

/** A Codex model as shown: its id; Codex's own choice for none. */
export function codexModelLabel(model: CodexModel | null): string {
  return model ?? t("start.codexModel.auto");
}

/** Codex's part of a start as the core keeps it, with every part said: a model that isn't one of
 * the list is Codex's own choice, a mode that isn't is normal. */
export function codexChoiceOf(start: ClaudeStart | null | undefined): CodexChoice {
  const model = start?.codex?.model && CODEX_MODELS.includes(start.codex.model) ? start.codex.model : null;
  const mode = start?.codex?.mode && CODEX_MODES.includes(start.codex.mode) ? start.codex.mode : "normal";
  return { agent: "codex", model, mode };
}

/** A start as the core keeps it (a card's, a project's), with every part said: a model that
 * isn't one of the list is Claude Code's own choice, a mode that isn't is normal. */
export function choiceOf(start: ClaudeStart | null | undefined): StartChoice {
  const model = start?.model && MODELS.includes(start.model) ? start.model : null;
  const mode = start?.mode && MODES.includes(start.mode) ? start.mode : "normal";
  return { model, mode };
}

/** How a Claude session of `project` starts when nothing else is chosen: its own settings'. */
export function projectChoice(project: Project | null | undefined): StartChoice {
  const settings = project ? projectSettings(project) : null;
  return choiceOf({ model: settings?.claudeModel, mode: settings?.claudeMode });
}

/** How a Codex session of `project` starts when nothing else is chosen: its own settings'. */
export function projectCodexChoice(project: Project | null | undefined): CodexChoice {
  const settings = project ? projectSettings(project) : null;
  return codexChoiceOf({ codex: { model: settings?.codexModel, mode: settings?.codexMode } });
}

/** The agent a session of `project` goes to when none is picked: its own settings' while that
 * agent is on, else the first one on (Claude, while it is). The core picks the same
 * (`give_start` in src-tauri/src/core/board.rs). */
export function projectAgent(project: Project | null | undefined): Agent {
  const chosen = agentOf(project ? projectSettings(project).agent : undefined);
  return agentOn(chosen) ? chosen : (agentsOn.value[0] ?? "claude");
}

/** How a session of `project` starts when nothing else is chosen: its agent's, as its settings
 * say. */
export function projectAgentChoice(project: Project | null | undefined): AgentStartChoice {
  return projectAgent(project) === "codex" ? projectCodexChoice(project) : projectChoice(project);
}

/** A start as the core keeps it (a card's), for the agent it names, with every part said. */
export function agentChoiceOf(start: ClaudeStart | null | undefined): AgentStartChoice {
  return agentOf(start?.agent) === "codex" ? codexChoiceOf(start) : choiceOf(start);
}

/** The model's own week in the reading, as it stands at `now`: the limit named after it
 * (`seven_day_opus`), or a week's limit whose model the API names by that word. Null when the
 * reading has none. */
export function modelWeek(fuel: Fuel | null, model: ClaudeModel, now: number): UsageWindow | null {
  const weeks = (fuel?.limits.usage?.windows ?? []).filter((window) => window.id.startsWith("seven_day_"));
  const named = (window: UsageWindow): boolean => (window.name ?? "").toLowerCase().split(/[^a-z0-9]+/).includes(model);
  const found = weeks.find((window) => window.id === `seven_day_${model}`) ?? weeks.find(named);
  return found ? current(found, now) : null;
}

/** What is left of a model's week, beside its name: `haftasından %38 kaldı`. `tone`: red when
 * little is left (as on the Fuel page), Claude's when under half, plain otherwise. */
export type WeekLeft = { text: string; tone: "low" | "under" | "plain" };

export function weekLeft(fuel: Fuel | null, model: ClaudeModel, now: number, lang: string): WeekLeft | null {
  const week = modelWeek(fuel, model, now);
  if (!week) return null;
  const share = left(week.used);
  return { text: t("start.weekLeft", { share: percentText(share, lang) }), tone: share <= LOW_LEFT ? "low" : share < 50 ? "under" : "plain" };
}

/** When the 5-hour session refills, while the reading knows a moment still ahead; null
 * otherwise (no reading, no session limit, a session that hasn't started or has refilled). */
export function refillAt(fuel: Fuel | null, now: number): number | null {
  const at = fuel?.limits.usage?.windows.find((window) => window.id === "five_hour")?.resetsAt ?? null;
  return at !== null && at > now ? at : null;
}

/** Turkish joins "at" to the time as a suffix that follows the last number as it is read:
 * 16:40'ta, 17:00'de, 9:30'da, 9:05'te. */
function turkishAt(hour: number, minute: number): string {
  // A full hour is read as the hour alone.
  const n = minute === 0 ? hour : minute;
  // bir, iki, üç, dört, beş, altı, yedi, sekiz, dokuz
  const units = ["", "de", "de", "te", "te", "te", "da", "de", "de", "da"];
  // sıfır, on, yirmi, otuz, kırk, elli
  const tens = ["da", "da", "de", "da", "ta", "de"];
  return n % 10 === 0 ? (tens[n / 10] ?? "da") : units[n % 10];
}

/** What a text that names a moment takes: `time` (`16:40`), and the words some languages hang
 * on it: Turkish's suffix (`at`), Spanish's article (`las`, `la` for one o'clock). */
export function timeParams(at: number, lang: string): { time: string; at: string; las: string } {
  const date = new Date(at);
  return { time: clockText(at, lang), at: turkishAt(date.getHours(), date.getMinutes()), las: date.getHours() === 1 ? "la" : "las" };
}

/** The day a Codex moment falls on (`Thu`, `Per`), when it isn't today: its week's refill can be
 * days away. None for Claude's, a few hours away, read by the time alone. */
export function momentDay(at: number, agent: Agent | null | undefined, lang: string, now = Date.now()): string | null {
  if (agentOf(agent) !== "codex" || new Date(at).toDateString() === new Date(now).toDateString()) return null;
  return new Date(at).toLocaleDateString(lang, { weekday: "short" });
}
