// How a Claude session is started from Pitwall: with which model, in which mode and, for a board
// card, when. The lists the window offers (the core keeps the same two and takes nothing else:
// src-tauri/src/start.rs), a project's own start, and what the Fuel reading says of a model's
// week and of the session's refill. Pure but for the language (`t`).

import { clockText, current, left, LOW_LEFT, percentText } from "./fuel";
import { t, type Key } from "./i18n";
import { projectSettings } from "./projectSettings";
import type { ClaudeMode, ClaudeModel, ClaudeStart, Fuel, Project, StartChoice, UsageWindow } from "./types";

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
