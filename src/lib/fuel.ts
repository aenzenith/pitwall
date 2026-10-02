// The Fuel page's arithmetic: how much of a limit is left, and when the session runs dry at the
// pace so far. Pure: the page and the sidebar badge both read it.

import type { Key } from "./i18n";
import type { Fuel, UsageState, UsageWindow } from "./types";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
/** The session window's length. */
export const SESSION_MS = 5 * 60 * MINUTE;
/** Before this much of the window has gone by, a pace says nothing yet. */
const MIN_ELAPSED_MS = 5 * MINUTE;
/** At or under this share left, a limit shows red. */
export const LOW_LEFT = 10;

/** Percent left of a limit, 0–100 (used can pass 100 when over the limit). */
export function left(used: number): number {
  return Math.min(100, Math.max(0, 100 - used));
}

/** Little enough left to show red. */
export function isLow(used: number): boolean {
  return left(used) <= LOW_LEFT;
}

/**
 * The window as it stands at `now`: one whose reset has passed since it was read has refilled,
 * and starts again (empty, not started) with the next message.
 */
export function current(window: UsageWindow, now: number): UsageWindow {
  return window.resetsAt !== null && window.resetsAt <= now ? { ...window, used: 0, resetsAt: null } : window;
}

export type Pace =
  /** Not started: no message since the last reset. */
  | { kind: "idle" }
  /** Used up already. */
  | { kind: "empty"; start: number; end: number }
  /** Too early, or nothing used yet, to tell a pace. */
  | { kind: "early"; start: number; end: number }
  /** At this pace it runs out at `emptyAt`, before the window refills at `end`. */
  | { kind: "out"; start: number; end: number; emptyAt: number }
  /** At this pace it lasts until the window refills (`emptyAt` past `end`). */
  | { kind: "lasts"; start: number; end: number; emptyAt: number };

/**
 * The session's pace, from a reading of it taken at `at`: the share used so far over the time
 * gone by since the window started (its reset less 5 h), carried on to empty.
 */
export function pace(window: UsageWindow, at: number): Pace {
  if (window.resetsAt === null) return { kind: "idle" };
  const end = window.resetsAt;
  const start = end - SESSION_MS;
  if (window.used >= 100) return { kind: "empty", start, end };
  const elapsed = at - start;
  if (window.used <= 0 || elapsed < MIN_ELAPSED_MS) return { kind: "early", start, end };
  const rate = window.used / elapsed;
  const emptyAt = at + (100 - window.used) / rate;
  return emptyAt < end ? { kind: "out", start, end, emptyAt } : { kind: "lasts", start, end, emptyAt };
}

/** The session's burn rate: percent used per hour of the window gone by, as read at `at`; null
 * before 5 minutes of it have gone by, or while it hasn't started. */
export function burnRate(window: UsageWindow, at: number): number | null {
  if (window.resetsAt === null) return null;
  const elapsed = at - (window.resetsAt - SESSION_MS);
  if (elapsed < MIN_ELAPSED_MS) return null;
  return (Math.max(0, window.used) / elapsed) * HOUR;
}

/** Percent left when the window refills, if the pace so far holds; null without a pace yet. */
export function leftAtRefill(window: UsageWindow, at: number): number | null {
  const rate = burnRate(window, at);
  if (rate === null || window.resetsAt === null) return null;
  return left(window.used + (rate * Math.max(0, window.resetsAt - at)) / HOUR);
}

/** The gauge beside the week's: the most used model's own week that has started (ties: the API's
 * order), if there is one. */
export function sideWindow(windows: UsageWindow[]): UsageWindow | null {
  let best: UsageWindow | null = null;
  for (const window of windows) {
    if (window.id.startsWith("seven_day_") && window.resetsAt !== null && (!best || window.used > best.used)) best = window;
  }
  return best;
}

/** The limits with a name of ours; model and product names stay as they are. */
const NAMES: Record<string, Key> = {
  five_hour: "fuel.session",
  seven_day: "fuel.name.week",
  seven_day_oauth_apps: "fuel.name.oauthApps",
  seven_day_omelette: "fuel.name.design",
};

/** A limit's name: ours, else the API's, else its id without `seven_day_` (`Opus`). */
export function windowName(window: UsageWindow, translate: (key: Key) => string): string {
  const key = NAMES[window.id];
  if (key) return translate(key);
  if (window.name) return window.name;
  const bare = window.id.replace(/^seven_day_/, "").replace(/_/g, " ");
  return bare.charAt(0).toUpperCase() + bare.slice(1);
}

/** Where `at` falls in the window, 0–100. */
export function position(at: number, start: number, end: number): number {
  return end > start ? Math.min(100, Math.max(0, ((at - start) / (end - start)) * 100)) : 0;
}

/** The core couldn't read them this time: what it still has is an older reading, shown dimmed. */
export function isStale(limits: UsageState): boolean {
  return limits.status !== "ready" && limits.status !== "loading";
}

/** The session window as it stands now, if the plan has one and it has been read. */
export function sessionWindow(fuel: Fuel | null, now: number): UsageWindow | null {
  const found = fuel?.limits.usage?.windows.find((window) => window.id === "five_hour");
  return found ? current(found, now) : null;
}

/**
 * A model id as people say it: `claude-opus-5-5` → `Opus 5.5`, `claude-3-5-sonnet-20241022` →
 * `3.5 Sonnet`, `us.anthropic.claude-sonnet-4-5-20250929-v1:0[1m]` → `Sonnet 4.5`.
 */
export function modelName(id: string): string {
  let rest = id.trim();
  const at = rest.indexOf("claude-");
  if (at < 0) return rest;
  rest = rest
    .slice(at + "claude-".length)
    .replace(/\[[^\]]*\]$/, "")
    .replace(/@.*$/, "")
    .replace(/-v\d+(:\d+)?$/, "")
    .replace(/-\d{8}$/, "")
    .replace(/-latest$/, "");
  const groups: string[] = [];
  let digits: string[] = [];
  for (const part of rest.split("-").filter(Boolean)) {
    if (/^\d+$/.test(part)) {
      digits.push(part);
      continue;
    }
    if (digits.length) groups.push(digits.join("."));
    digits = [];
    groups.push(part.charAt(0).toUpperCase() + part.slice(1));
  }
  if (digits.length) groups.push(digits.join("."));
  return groups.join(" ") || id;
}

/* ---------- formats; `lang` is the language spoken (lib/i18n) ---------- */

/** `26%`, `%26`, `26 %`: a share 0–100 as the language writes it, with up to `digits` decimals. */
export function percentText(value: number, lang: string, digits = 0): string {
  return new Intl.NumberFormat(lang, { style: "percent", maximumFractionDigits: digits }).format(value / 100);
}

/** `16:40`. */
export function clockText(at: number, lang: string): string {
  return new Date(at).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

/** `Mon 09:00`, `Pzt 09:00`. */
export function dayClockText(at: number, lang: string): string {
  return new Date(at).toLocaleString(lang, { weekday: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

/** An amount in its currency (`$37.60`, `37,60 €`); a plain number when the currency is unknown. */
export function moneyText(amount: number, currency: string | null, lang: string): string {
  if (currency) {
    try {
      return new Intl.NumberFormat(lang, { style: "currency", currency }).format(amount);
    } catch {
      // Not an ISO code: the number alone.
    }
  }
  return new Intl.NumberFormat(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(amount);
}

/** `1.8M`, `1,8 Mn`, `183.5万`. */
export function compactText(count: number, lang: string): string {
  return new Intl.NumberFormat(lang, { notation: "compact", maximumFractionDigits: 1 }).format(count);
}
