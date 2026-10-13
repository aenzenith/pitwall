// Notifications as the popover and Settings › Notifications read them (core/signals.rs): whether
// it is quiet and until when, what was missed meanwhile, and Settings' switches. The core keeps
// all of it; every change goes there by its own command and comes back in the next snapshot.

import { computed } from "vue";

import { clock, dayName } from "./day";
import { language, t, type Key } from "./i18n";
import { snapshot } from "./store";
import type { Missed, NotificationSettings, Quiet, SignalKind } from "./types";

/** Settings' own, as the core has them when it has none stored. */
const DEFAULTS: NotificationSettings = {
  serverCrashed: true,
  command: false,
  longTurn: true,
  deps: false,
  muted: [],
  quietHours: { on: false, from: "22:00", to: "08:00" },
  questionsThrough: true,
  soundsToo: true,
};

export const notifications = computed<NotificationSettings>(() => snapshot.value?.settings.notifications ?? DEFAULTS);

/** Quiet now, or null. */
export const quiet = computed<Quiet | null>(() => snapshot.value?.quiet ?? null);

/** What the user wasn't told meanwhile, newest first. */
export const missed = computed<Missed[]>(() => snapshot.value?.missed ?? []);

/** The events a notification can be switched on and off for, in the order Settings lists them. */
export type NotifyEvent = "claude" | "serverCrashed" | "command" | "longTurn" | "deps" | "fuel";

export const EVENTS: Array<{ id: NotifyEvent; label: Key }> = [
  { id: "claude", label: "notifications.claude" },
  { id: "serverCrashed", label: "notifications.serverCrashed" },
  { id: "command", label: "notifications.command" },
  { id: "longTurn", label: "notifications.longTurn" },
  { id: "deps", label: "notifications.deps" },
  { id: "fuel", label: "notifications.fuel" },
];

/** Whether an event brings a notification: Claude's switch is Settings' older `notify`, the fuel
 * alert's its `fuelAlert` (the Fuel page's chip turns the same one). */
export function eventOn(event: NotifyEvent): boolean {
  if (event === "claude") return snapshot.value?.settings.notify ?? true;
  if (event === "fuel") return snapshot.value?.settings.fuelAlert ?? true;
  return notifications.value[event];
}

/** Claude waits on you with these: of a project that has one listed as missed, the popover's
 * "Claude is waiting" card says nothing more. */
const WAITS: SignalKind[] = ["claude-asking", "claude-permission", "claude-finished"];

export function waits(kind: SignalKind): boolean {
  return WAITS.includes(kind);
}

/**
 * The Turkish dative after a time read aloud, as in `15:30'a kadar`: by the last word said (the
 * minutes, or the hour on the hour), `otuz` takes `a`, `sekiz` `e`, `altı` `ya`, `yirmi` `ye`.
 */
function trDative(time: string): string {
  const [hours, minutes] = time.split(":").map(Number);
  const said = minutes || hours || 0;
  const last = said % 10 || said;
  return ({ 0: "a", 1: "e", 2: "ye", 3: "e", 4: "e", 5: "e", 6: "ya", 7: "ye", 8: "e", 9: "a", 10: "a", 20: "ye", 30: "a", 40: "a", 50: "ye" } as Record<number, string>)[last] ?? "e";
}

/** `Quiet · until 15:30`; `Quiet` alone when it lasts until it is switched off. */
export function quietLine(state: Quiet, now: number): string {
  if (state.until === null) return t("quiet.on");
  const time = clock(state.until);
  const params = { time, suffix: language.value === "tr" ? trDative(time) : "" };
  return t(dayName(state.until) === dayName(now) ? "quiet.until" : "quiet.untilTomorrow", params);
}

const LINES: Record<SignalKind, Key> = {
  "claude-asking": "missed.claudeAsking",
  "claude-permission": "missed.claudePermission",
  "claude-finished": "missed.claudeFinished",
  "claude-long-turn": "missed.claudeLongTurn",
  "server-crashed": "missed.serverCrashed",
  "command-done": "missed.commandDone",
  "command-failed": "missed.commandFailed",
  "deps-waiting": "missed.depsWaiting",
  "fuel-high": "missed.fuelHigh",
  "card-review": "missed.cardReview",
};

/** What happened, in a line: `Command failed: migrate`. */
export function missedLine(entry: Missed): string {
  if (entry.kind === "claude-permission" && entry.detail) return t("missed.claudePermissionTool", { tool: entry.detail });
  // The fuel alert's own line says how much.
  if (entry.kind === "fuel-high" && entry.detail) return entry.detail;
  return t(LINES[entry.kind], { name: entry.detail ?? "" });
}

/** Whose it is: its project, or the page it is about when it is no project's. */
export function missedName(entry: Missed): string {
  return entry.name || t("fuel.nav");
}

/** What a missed line's dot says: Claude's, something that broke, something that waits, or a thing done. */
export function missedTone(kind: SignalKind): "claude" | "crash" | "warn" | "run" {
  if (kind.startsWith("claude-") || kind === "card-review") return "claude";
  if (kind === "server-crashed" || kind === "command-failed") return "crash";
  return kind === "command-done" ? "run" : "warn";
}
