// The Fuel page's arithmetic: how much of a limit is left, and when the session runs dry at the
// pace so far. Pure: the page, the sidebar badge and the popover's strip all read it.

import type { Key } from "./i18n";
import type { Fuel, UsageError, UsageExtra, UsageState, UsageWindow } from "./types";

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

/** `t` of lib/i18n, handed in so this stays pure. */
type Translate = (key: Key, params?: Record<string, string | number>) => string;

/** One dial of the cluster: `name` as the page's dial says it; `caption` under the pod: when it
 * refills, or what is left; `window`: the limit itself, null for Extra usage. */
export type ClusterDial = { key: "week" | "session" | "side" | "extra"; size: "big" | "side"; left: number; name: string; caption: string; window: UsageWindow | null };

/** The limits as they stand, and which of them get a dial. */
export type Cluster = {
  windows: UsageWindow[];
  session: UsageWindow | null;
  week: UsageWindow | null;
  /** Beside the week: the most used model's week. */
  side: UsageWindow | null;
  extra: UsageExtra | null;
  /** Extra usage takes the side dial: no model's week, and it is on with a cap. */
  extraOnGauge: boolean;
  /** Left to right: the week, the session (big), the side one. */
  dials: ClusterDial[];
};

/** `refills 16:40`, with the weekday for a week's (`refills Mon 09:00`), or `not started`. */
function refills(window: UsageWindow, weekday: boolean, translate: Translate, lang: string): string {
  if (window.resetsAt === null) return translate("fuel.notStarted");
  const when = weekday ? dayClockText(window.resetsAt, lang) : clockText(window.resetsAt, lang);
  return translate("fuel.refills", { when });
}

/**
 * The instrument cluster of a reading, as it stands at `now`: the Fuel page's dials, and the
 * popover's strip of the same three. A limit whose reset has passed since the reading has refilled.
 */
export function cluster(usage: UsageState["usage"], now: number, translate: Translate, lang: string): Cluster {
  const windows = (usage?.windows ?? []).map((window) => current(window, now));
  const session = windows.find((window) => window.id === "five_hour") ?? null;
  const week = windows.find((window) => window.id === "seven_day") ?? null;
  const side = sideWindow(windows);
  const extra = usage?.extra ?? null;
  const extraOnGauge = !side && !!extra?.enabled && (extra.monthlyLimit ?? 0) > 0;

  const dials: ClusterDial[] = [];
  if (week) dials.push({ key: "week", size: "side", left: left(week.used), name: translate("fuel.name.weekShort"), caption: refills(week, true, translate, lang), window: week });
  if (session) dials.push({ key: "session", size: "big", left: left(session.used), name: translate("fuel.session"), caption: refills(session, false, translate, lang), window: session });
  if (side) {
    dials.push({ key: "side", size: "side", left: left(side.used), name: windowName(side, translate), caption: refills(side, true, translate, lang), window: side });
  } else if (extraOnGauge && extra) {
    const cap = extra.monthlyLimit ?? 0;
    const used = extra.used ?? (extra.usedCredits / cap) * 100;
    const money = moneyText(Math.max(0, cap - extra.usedCredits), extra.currency, lang);
    dials.push({ key: "extra", size: "side", left: left(used), name: translate("fuel.extra"), caption: translate("fuel.left", { value: money }), window: null });
  }
  return { windows, session, week, side, extra, extraOnGauge, dials };
}

/** Where `at` falls in the window, 0–100. */
export function position(at: number, start: number, end: number): number {
  return end > start ? Math.min(100, Math.max(0, ((at - start) / (end - start)) * 100)) : 0;
}

/** The core couldn't read them this time: what it still has is an older reading, shown dimmed. */
export function isStale(limits: UsageState): boolean {
  return limits.status !== "ready" && limits.status !== "loading";
}

/** What went wrong reading the limits: a title, and a hint to go under it. */
const ERRORS: Record<UsageError | "unknown", [Key, Key]> = {
  network: ["fuel.error.network", "fuel.error.networkHint"],
  timeout: ["fuel.error.timeout", "fuel.error.timeoutHint"],
  forbidden: ["fuel.error.forbidden", "fuel.error.forbiddenHint"],
  server: ["fuel.error.server", "fuel.error.serverHint"],
  http: ["fuel.error.http", "fuel.error.httpHint"],
  badResponse: ["fuel.error.badResponse", "fuel.error.badResponseHint"],
  credentials: ["fuel.error.credentials", "fuel.error.credentialsHint"],
  keychain: ["fuel.error.keychain", "fuel.error.keychainHint"],
  unknown: ["fuel.error.unknown", "fuel.error.unknownHint"],
};

/** An error's title and hint; one a newer core names that isn't listed here reads as unknown. */
export function errorKeys(error: UsageError | null): [Key, Key] {
  return ERRORS[error ?? "unknown"] ?? ERRORS.unknown;
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
