// The laps (Turlar): jobs that run on a schedule while Pitwall runs (src-tauri/src/core/laps/).
// These types mirror core/laps/types.rs field for field; the core is the one that checks them.

import { ref } from "vue";

import { agentOf } from "./agents";
import { dayName } from "./day";
import { clockText, dayClockText, percentText } from "./fuel";
import { language, t, type Key } from "./i18n";
import type { Agent, ClaudeStart } from "./types";

/** What a job does: an agent's session, a script then an agent, or a script alone. */
export type LapKind = "agent" | "scriptAgent" | "script";

/** A listed project's folder, or a folder the user picked (absolute). */
export type LapPlace = { kind: "project"; path: string } | { kind: "folder"; path: string };

/** A saved command of the project (never one marked "ask first"), or a command line. */
export type LapScript = { kind: "command"; id: string } | { kind: "text"; text: string };

/** When a job runs. Times `HH:MM` on this Mac's clock; days 1 (Monday) – 7 (Sunday), empty: every
 * day. "Hafta içi" is daily on days 1–5. */
export type LapWhen =
  | { kind: "daily"; at: string; days: number[] }
  | { kind: "every"; minutes: number; from: string | null; to: string | null; days: number[] }
  | { kind: "once"; at: number }
  | { kind: "refill" }
  | { kind: "cron"; expr: string };

export type ScriptGate = "none" | "onFailure" | "onOutput";
export type LapGate = { script: ScriptGate; git: boolean; sameSilent: boolean };
export type FuelGuard = { over: number; then: "skip" | "wait" };
export type LapMissed = { catchUp: boolean; graceMin: number };
export type LapOverlap = "skip" | "wait" | "stop";
export type LapWhere = "tab" | "background";
export type LapRetry = { times: number; everyMin: number };
export type LapNotify = { done: boolean; failed: boolean; sound: boolean };
export type LapEnds = { at: number | null; after: number | null };
export type PausedBy = "user" | "failures";

export type Lap = {
  /** Empty in a new job; the core gives it one. */
  id: string;
  name: string;
  kind: LapKind;
  place: LapPlace;
  start: ClaudeStart;
  account: string | null;
  script: LapScript | null;
  prompt: string;
  when: LapWhen;
  daysOff: string[];
  weekdaysOff: number[];
  on: boolean;
  paused: PausedBy | null;
  gate: LapGate;
  timeoutMin: number | null;
  awake: boolean;
  fuel: FuelGuard | null;
  where: LapWhere;
  missed: LapMissed;
  overlap: LapOverlap;
  carry: boolean;
  retry: LapRetry | null;
  pauseAfter: number | null;
  notify: LapNotify;
  keep: number;
  file: string | null;
  ends: LapEnds;
  /* The core's own: what the window sends of them is ignored. */
  nextAt: number | null;
  streak: number;
  runsDone: number;
  createdAt: number;
};

export type RunResult = "ok" | "failed" | "skipped" | "silent" | "cut";

/** Why a run ended as it did; its words are `laps.reason.<code>`. */
export type RunReason =
  | "gate"
  | "git"
  | "fuel"
  | "noReading"
  | "agentOff"
  | "missed"
  | "overlap"
  | "dayOff"
  | "userSkip"
  | "askFirst"
  | "timeout"
  | "exit"
  | "notFolder"
  | "notListed"
  | "stopped"
  | "quit"
  | "notStarted";

export type LapRun = {
  id: string;
  lap: string;
  due: number;
  byHand: boolean;
  startedAt: number;
  endedAt: number | null;
  /** Null while it goes. */
  result: RunResult | null;
  reason: RunReason | null;
  code: number | null;
  session: string | null;
  agent: Agent | null;
  account: string | null;
  tab: number | null;
  file: string | null;
  hash: string | null;
  head: string | null;
  tries: number;
  script: boolean;
  /** Done or failed, and its result not opened since (`lap_read`). */
  unread: boolean;
};

export type RunMark = { id: string; due: number; result: RunResult | null; unread: boolean };

export type LapRunning = {
  run: string;
  startedAt: number;
  phase: "script" | "agent" | "waiting" | "retry";
  tab: number | null;
  /** A permission it waits on: the tool's name only. */
  permission: string | null;
};

export type LapView = Lap & {
  /** The listed project's name, for a project's job. */
  project: string | null;
  /** The folder as shown (`~/…`). */
  folder: string;
  /** The last ten runs, oldest first. */
  last: RunMark[];
  running: LapRunning | null;
  /** The folder is a Git work tree. */
  git: boolean;
  /** No usage reading yet of the account its agent runs with: a refill job has no moment, and
   * the fuel guard skips its runs. */
  noReading: boolean;
  /** Its kept runs not read yet (any of them, not only the last ten). */
  unread: number;
};

/** `unread`: the kept runs not read yet, of every job (the sidebar's dot). */
export type LapsState = { laps: LapView[]; failing: number; unread: number; elsewhere: boolean };

/** A run's result. `command`: the command line its script ran, as it was then. */
export type LapResult = { run: LapRun; command: string | null; script: string[] | null; message: string | null; gone: boolean };

/**
 * What a run's dialog shows in its terminal body (core: `lap_terminal`): the laps' own tab its
 * agent runs in (in no panel: the dialog alone shows it); none while its agent runs in the
 * background, or before it has one (the script, a wait); none either for a session that runs
 * elsewhere, which was brought up there.
 */
export type LapTerminal = { kind: "tab"; id: number } | { kind: "background" } | { kind: "notYet" } | { kind: "elsewhere" };

export type CalendarDay = { date: string; results: RunResult[]; planned: number; off: boolean };

/** A job of one day: its moments still to come, the first ones (`count` is all of them, `last`
 * the last), and the runs kept of it. */
export type DayRow = { lap: string; name: string; at: number[]; count: number; last: number | null; off: "day" | "weekday" | null; runs: RunMark[] };


/* ---------- the kinds ---------- */

export const KINDS: LapKind[] = ["agent", "scriptAgent", "script"];

export const KIND_LABELS: Record<LapKind, Key> = { agent: "laps.kind.agent", scriptAgent: "laps.kind.scriptAgent", script: "laps.kind.script" };

/** What a kind is, said over its button in the edit dialog. */
export const KIND_HINTS: Record<LapKind, Key> = { agent: "laps.kind.agent.hint", scriptAgent: "laps.kind.scriptAgent.hint", script: "laps.kind.script.hint" };

export function hasAgent(kind: LapKind): boolean {
  return kind !== "script";
}

export function hasScript(kind: LapKind): boolean {
  return kind !== "agent";
}

/** The job's agent: the one its start names, Claude when none. Meaningless for a script job. */
export function lapAgent(lap: Pick<Lap, "start">): Agent {
  return agentOf(lap.start?.agent);
}

/** A job as a new one starts, in `place`: the core's own defaults (types.rs `Lap::default`). */
export function blankLap(place: LapPlace): Lap {
  return {
    id: "",
    name: "",
    kind: "agent",
    place,
    start: {},
    account: null,
    script: null,
    prompt: "",
    when: { kind: "daily", at: "09:00", days: [] },
    daysOff: [],
    weekdaysOff: [],
    on: true,
    paused: null,
    gate: { script: "none", git: false, sameSilent: false },
    timeoutMin: null,
    awake: false,
    fuel: null,
    where: "tab",
    missed: { catchUp: true, graceMin: 120 },
    overlap: "skip",
    carry: false,
    retry: null,
    pauseAfter: 3,
    notify: { done: false, failed: true, sound: false },
    keep: 30,
    file: null,
    ends: { at: null, after: null },
    nextAt: null,
    streak: 0,
    runsDone: 0,
    createdAt: 0,
  };
}

/** A job of the page, as the window sends it to `lap_save`: what the page adds left out. */
export function lapOf(view: LapView): Lap {
  const lap: Partial<LapView> = JSON.parse(JSON.stringify(view));
  delete lap.project;
  delete lap.folder;
  delete lap.last;
  delete lap.running;
  delete lap.git;
  delete lap.noReading;
  delete lap.unread;
  return lap as Lap;
}

/* ---------- the order of the tower ---------- */

/** On the schedule: on, not paused. */
export function scheduled(lap: LapView): boolean {
  return lap.on && lap.paused === null;
}

/** The tower's parts, top to bottom: the jobs on the schedule, the paused ones, those that are off. */
export type TowerPart = "on" | "paused" | "off";

const PARTS: TowerPart[] = ["on", "paused", "off"];

export function towerPart(lap: LapView): TowerPart {
  return scheduled(lap) ? "on" : lap.paused !== null ? "paused" : "off";
}

/** The tower's order: its parts, each in the jobs' own order (the user's, dragged in the tower;
 * the core's `lap_reorder`). */
export function towerOrder(laps: LapView[]): LapView[] {
  return PARTS.flatMap((part) => laps.filter((lap) => towerPart(lap) === part));
}

/* ---------- times ---------- */

const pad = (n: number): string => String(n).padStart(2, "0");

/** What is left until a moment: `02:39:40`; past a day `3g 02:39`. One width while it counts. */
export function countdown(ms: number): string {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return t("laps.left.days", { days, time: `${pad(hours)}:${pad(minutes)}` });
  return `${pad(hours)}:${pad(minutes)}:${pad(seconds % 60)}`;
}

/** How long a run took: `4 sn`, `6 dk 10 sn`, `20 dk`, `1 sa 5 dk`. */
export function runLength(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return t("laps.length.seconds", { seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return seconds % 60 ? t("laps.length.minutesSeconds", { minutes, seconds: seconds % 60 }) : t("laps.length.minutes", { minutes });
  return t("laps.length.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}

/** A run's moment as a row says it: `Sal 13:00`; today's with its time alone where `short`. */
export function runMoment(at: number): string {
  return dayClockText(at, language.value);
}

/** `Bugün 08:30` today, `Sal 13:00` within the week, `2 Kas 13:00` before. */
export function runDay(at: number, now: number): string {
  if (dayName(at) === dayName(now)) return t("laps.today", { time: clockText(at, language.value) });
  if (now - at < 6 * 86_400_000) return dayClockText(at, language.value);
  return `${new Date(at).toLocaleDateString(language.value, { day: "numeric", month: "short" })} ${clockText(at, language.value)}`;
}

/** A Monday, for the names of the week's days. */
const MONDAY = new Date(2024, 0, 1, 12).getTime();

/** A day of the week (1 Monday – 7 Sunday) by its short name, as the language writes it. */
export function weekdayShort(day: number): string {
  return new Date(MONDAY + (day - 1) * 86_400_000).toLocaleDateString(language.value, { weekday: "short" });
}

/** …and by its whole name. */
export function weekdayLong(day: number): string {
  return new Date(MONDAY + (day - 1) * 86_400_000).toLocaleDateString(language.value, { weekday: "long" });
}

/** The weekday (1–7) of a `YYYY-MM-DD` date. */
export function weekdayOf(date: string): number {
  const [year, month, day] = date.split("-").map(Number);
  const weekday = new Date(year, month - 1, day, 12).getDay();
  return weekday === 0 ? 7 : weekday;
}

/** Noon of a `YYYY-MM-DD` date (ms). */
export function noonOf(date: string): number {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day, 12).getTime();
}

const WEEKDAYS = [1, 2, 3, 4, 5];

function sameDays(days: number[], list: number[]): boolean {
  const set = new Set(days);
  return set.size === list.length && list.every((day) => set.has(day));
}

/** "Hafta içi": daily on Monday to Friday. */
export function isWeekdays(days: number[]): boolean {
  return sameDays(days, WEEKDAYS);
}

/** Days as a row says them: none for every day, `Hafta içi`, else their short names. */
function daysText(days: number[]): string | null {
  if (!days.length || days.length === 7) return null;
  if (isWeekdays(days)) return t("laps.when.weekdays");
  return [...days].sort((a, b) => a - b).map(weekdayShort).join(", ");
}

/** `09–21`, or `09:30–21:00` when a minute isn't 0. */
function windowText(from: string | null, to: string | null): string {
  if (!from && !to) return "";
  const a = from ?? "00:00";
  const b = to ?? "23:59";
  const whole = a.endsWith(":00") && b.endsWith(":00");
  return whole ? `${a.slice(0, 2)}–${b.slice(0, 2)}` : `${a}–${b}`;
}

/** Every N minutes as words: `15 dakikada bir`, `4 saatte bir`. */
export function intervalText(minutes: number): string {
  return minutes % 60 === 0 ? t("laps.when.everyHours", { count: minutes / 60 }) : t("laps.when.everyMinutes", { count: minutes });
}

/** When a job runs, in a line: `Her gün 08:30`, `Hafta içi 4 saatte bir, 09–21`, `Claude dolunca`. */
export function whenText(when: LapWhen, agent: Agent): string {
  switch (when.kind) {
    case "daily": {
      const days = daysText(when.days);
      return days ? t("laps.when.sum.days", { days, time: when.at }) : t("laps.when.sum.daily", { time: when.at });
    }
    case "every": {
      const days = daysText(when.days);
      const span = windowText(when.from, when.to);
      const every = intervalText(when.minutes) + (span ? `, ${span}` : "");
      return days ? t("laps.when.sum.everyDays", { days, every }) : t("laps.when.sum.every", { every });
    }
    case "once":
      return t("laps.when.sum.once", { when: `${new Date(when.at).toLocaleDateString(language.value, { day: "numeric", month: "short" })} ${clockText(when.at, language.value)}` });
    case "refill":
      return t(agent === "codex" ? "laps.when.sum.refill.codex" : "laps.when.sum.refill");
    case "cron":
      return t("laps.when.sum.cron", { expr: when.expr });
  }
}

/** The next moments as the dialog lists them: `Sal 17:00 · Sal 21:00 · Çar 09:00`. */
export function momentsText(moments: number[]): string {
  return moments.map((at) => dayClockText(at, language.value)).join(" · ");
}

/** The day off a job has, for its row in the tower: `Her Çar kapalı`, `4 Kas kapalı`; none when
 * it has none to come. */
export function dayOffTag(lap: Lap, today: string): string | null {
  if (lap.weekdaysOff.length) return t("laps.off.every", { days: [...lap.weekdaysOff].sort((a, b) => a - b).map(weekdayShort).join(", ") });
  const coming = lap.daysOff.filter((date) => date >= today).sort();
  if (!coming.length) return null;
  const first = new Date(noonOf(coming[0])).toLocaleDateString(language.value, { day: "numeric", month: "short" });
  return coming.length > 1 ? t("laps.off.dates", { date: first, count: coming.length - 1 }) : t("laps.off.date", { date: first });
}

/** A share as the language writes it: `%85`, `85%`. */
export function shareText(over: number): string {
  return percentText(over, language.value);
}

/* ---------- runs ---------- */

/** A run's mark: done, failed, or grey (skipped, silent, cut); none while it goes. */
export type MarkTone = "ok" | "bad" | "grey" | "going";

export function markTone(result: RunResult | null): MarkTone {
  if (result === null) return "going";
  return result === "ok" ? "ok" : result === "failed" ? "bad" : "grey";
}

const RESULT_LABELS: Record<RunResult, Key> = {
  ok: "laps.result.ok",
  failed: "laps.result.failed",
  skipped: "laps.result.skipped",
  silent: "laps.result.silent",
  cut: "laps.result.cut",
};

/** A result by its name: `Bitti`, `Hata`, `Atlandı`, `Sessiz`, `Kesildi`; `Koşuyor` while it goes. */
export function resultLabel(result: RunResult | null): string {
  return t(result === null ? "laps.phase.running" : RESULT_LABELS[result]);
}

const REASONS: Record<Exclude<RunReason, "gate">, Key> = {
  git: "laps.reason.git",
  fuel: "laps.reason.fuel",
  noReading: "laps.reason.noReading",
  agentOff: "laps.reason.agentOff",
  missed: "laps.reason.missed",
  overlap: "laps.reason.overlap",
  dayOff: "laps.reason.dayOff",
  userSkip: "laps.reason.userSkip",
  askFirst: "laps.reason.askFirst",
  timeout: "laps.reason.timeout",
  exit: "laps.reason.exit",
  notFolder: "laps.reason.notFolder",
  notListed: "laps.reason.notListed",
  stopped: "laps.reason.stopped",
  quit: "laps.reason.quit",
  notStarted: "laps.reason.notStarted",
};

/** Why a run ended as it did, in Pitwall's words; the script gate by what the script did. `gate`:
 * the job's own gate, for a gated run whose output isn't at hand. */
export function reasonText(run: Pick<LapRun, "reason" | "code" | "result">, gate: ScriptGate, empty?: boolean): string | null {
  if (run.result === "silent") return t("laps.reason.silent");
  if (!run.reason) return null;
  if (run.reason === "gate") return t(empty === true || (empty === undefined && gate === "onOutput") ? "laps.reason.gate.empty" : "laps.reason.gate.passed");
  if (run.reason === "exit" && run.code !== null) return t("laps.reason.exitCode", { code: run.code });
  return t(REASONS[run.reason]);
}

/** What a run in progress is at, for the tower's Kalan and the detail: `Koşuyor`,
 * `İzin bekliyor: Bash`, `Dolumu bekliyor`. */
export function phaseText(running: LapRunning, lap: Lap): string {
  if (running.permission) return t("laps.phase.permission", { tool: running.permission });
  if (running.phase === "waiting") return t(lap.fuel?.then === "wait" ? "laps.phase.refill" : "laps.phase.waiting");
  if (running.phase === "retry") return t("laps.phase.retry");
  return t("laps.phase.running");
}

/** What a run in progress is at, in a word, for the tower's narrow Kalan column; `phaseText`
 * says it whole (a permission names its tool there). */
export function phaseShort(running: LapRunning, lap: Lap): string {
  if (running.permission) return t("laps.phase.short.permission");
  if (running.phase === "waiting") return t(lap.fuel?.then === "wait" ? "laps.phase.short.refill" : "laps.phase.short.waiting");
  if (running.phase === "retry") return t("laps.phase.short.retry");
  return t("laps.phase.running");
}

/** Whether a run in progress waits on the user or on something else (the waiting colour), or
 * works (the working one). */
export function phaseTone(running: LapRunning): "waiting" | "working" {
  return running.permission || running.phase === "waiting" ? "waiting" : "working";
}

/* ---------- the core's errors ---------- */

/** A refusal from the core as the window shows it: a catalog key the core sent is said in the
 * language shown; any other text as it came. */
export function refusalText(reason: unknown): string {
  const text = String(reason);
  return isKey(text) ? t(text) : text;
}

function isKey(text: string): text is Key {
  return /^(laps|core)\.[\w.]+$/.test(text) && t(text as Key) !== text;
}

/* ---------- a run's dialog, asked for from outside the page ---------- */

/**
 * A run whose dialog is to open on the Turlar page: what a lap's session waits on was clicked in
 * its banner (core: `RevealLap`, event `reveal-page` with the run). The page opens it and lets it go.
 */
export const lapAsked = ref<string | null>(null);
