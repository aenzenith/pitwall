// The day's timeline as numbers and text: spans, the hours shown, the blocks on each lane and
// their tooltips. No component state; what reads the language (`clock`, `duration`, `t`) follows it.

import { language, t } from "./i18n";
import type { DayProject, DaySession, DaySpan, DaySummary } from "./types";

export const HOUR = 3_600_000;
/** The timeline shows at least this many hours. */
const MIN_HOURS = 8;

export type Tip = { title: string; detail: string; tone: "claude" | "server" | "crash" | "plain" };
/** `at`: when it starts, the order the keyboard steps through a lane in. */
export type Block = { key: string; kind: "server" | "work" | "wait" | "commit" | "crash"; at: number; style: Record<string, string>; text: string; tip: Tip };
/** The hours the timeline shows, whole ones: from `from` to `to`, `hours` of them. */
export type TimeRange = { from: number; to: number; hours: number };
/** An hour on the axis; only the major ones that aren't under the now marker have a label. */
export type Tick = { at: number; left: string; label: string; major: boolean };
/** A project's lane: how long it was busy (overlaps once), that as text, what happened on it. */
export type Lane = { project: DayProject; time: number; total: string; blocks: Block[] };
/** Claude, live: waiting on you (a notification not read yet) or working; `text` says which. */
export type Live = { phase: "waiting" | "working"; text: string };
/** A project's part of the bar of where the time went. */
export type SplitPart = { path: string; name: string; grow: number; share: string; color: string; tip: Tip };

/* ---------- days ---------- */

/** The local day `at` falls on, `YYYY-MM-DD`. */
export function dayName(at: number): string {
  const d = new Date(at);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function yesterday(): string {
  const d = new Date();
  d.setHours(12, 0, 0, 0);
  d.setDate(d.getDate() - 1);
  return dayName(d.getTime());
}

/* ---------- formats ---------- */

/** One time formatter per language, made on first use: a day has hundreds of times to show. */
const clocks = new Map<string, Intl.DateTimeFormat>();

/** `14:05`, in the app's language. */
export function clock(at: number): string {
  const locale = language.value;
  let format = clocks.get(locale);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
    clocks.set(locale, format);
  }
  return format.format(at);
}

/** `09:12 – 17:40`. */
export function clockRange(start: number, end: number): string {
  return `${clock(start)} – ${clock(end)}`;
}

export function duration(ms: number): string {
  if (ms <= 0) return "—";
  const minutes = Math.round(ms / 60_000);
  if (minutes < 1) return t("time.lessThanMinute");
  const hours = Math.floor(minutes / 60);
  return hours ? t("time.duration", { hours, minutes: String(minutes % 60).padStart(2, "0") }) : t("time.durationMinutes", { minutes });
}

/* ---------- spans ---------- */

/** How long `spans` cover together, overlaps counted once. */
export function covered(spans: DaySpan[]): number {
  let total = 0;
  let from = 0;
  let to = -Infinity;
  for (const span of [...spans].sort((a, b) => a.start - b.start)) {
    if (span.start > to) {
      if (to > from) total += to - from;
      from = span.start;
      to = span.end;
    } else {
      to = Math.max(to, span.end);
    }
  }
  if (to > from) total += to - from;
  return total;
}

export function sum(spans: DaySpan[]): number {
  return spans.reduce((total, span) => total + span.end - span.start, 0);
}

export function spansOf(project: DayProject): DaySpan[] {
  return [...project.work, ...project.wait, ...project.server];
}

/** First and last moment anything happened in `projects`. */
export function bounds(projects: DayProject[]): [number, number] | null {
  let lo = Infinity;
  let hi = -Infinity;
  for (const project of projects) {
    for (const span of spansOf(project)) {
      lo = Math.min(lo, span.start);
      hi = Math.max(hi, span.end);
    }
    for (const at of [...project.commits.map((c) => c.at), ...project.crashes]) {
      lo = Math.min(lo, at);
      hi = Math.max(hi, at);
    }
  }
  return Number.isFinite(lo) ? [lo, hi] : null;
}

/* ---------- the timeline ---------- */

/** Whole hours around what happened (and now, today), at least MIN_HOURS of them. */
export function timeRange(d: DaySummary | null): TimeRange | null {
  if (!d) return null;
  const span = bounds(d.projects);
  const hours = Math.round((d.end - d.start) / HOUR);
  let lo = span ? span[0] : d.today ? d.now : d.start + 9 * HOUR;
  let hi = span ? span[1] : lo;
  if (d.today) {
    lo = Math.min(lo, d.now);
    hi = Math.max(hi, d.now);
  }
  let from = Math.max(0, Math.floor((lo - d.start) / HOUR));
  let to = Math.min(hours, Math.ceil((hi - d.start) / HOUR) + (d.today ? 1 : 0));
  if (to - from < MIN_HOURS) {
    to = Math.min(hours, from + MIN_HOURS);
    from = Math.max(0, to - MIN_HOURS);
  }
  return { from: d.start + from * HOUR, to: d.start + to * HOUR, hours: to - from };
}

/** Where `at` falls across the range, 0–100. */
export function pct(at: number, r: TimeRange | null): number {
  if (!r) return 0;
  return Math.min(100, Math.max(0, ((at - r.from) / (r.to - r.from)) * 100));
}

function place(start: number, end: number, r: TimeRange | null): Record<string, string> {
  const left = pct(start, r);
  return { left: `${left}%`, width: `${Math.max(pct(end, r) - left, 0.25)}%` };
}

/**
 * The hour marks: labels spaced to fit the axis's `width` (px; 0 before it is measured) and
 * stepping aside for the now marker, at `now` while today is shown.
 */
export function hourTicks(r: TimeRange | null, width: number, now: number | null): Tick[] {
  if (!r) return [];
  const perHour = width ? width / r.hours : 60;
  const step = [1, 2, 3, 4, 6].find((hours) => hours * perHour >= 46) ?? 6;
  return Array.from({ length: r.hours + 1 }, (_, i) => {
    const at = r.from + i * HOUR;
    const major = i % step === 0;
    const underNow = now !== null && (Math.abs(at - now) / HOUR) * perHour < 50;
    return { at, left: `${pct(at, r)}%`, label: major && !underNow ? clock(at) : "", major };
  });
}

function sessionTitle(project: DayProject, id: string | undefined): string {
  return project.sessions.find((s) => s.id === id)?.title ?? "";
}

/** What happened on a project's lane, each with its place and tooltip. */
export function blocks(project: DayProject, r: TimeRange | null): Block[] {
  const list: Block[] = [];
  const range = (span: DaySpan): string => clockRange(span.start, span.end);

  project.server.forEach((span, i) =>
    list.push({ key: `s${i}`, kind: "server", at: span.start, style: place(span.start, span.end, r), text: "", tip: { title: t("day.tip.server", { duration: duration(span.end - span.start) }), detail: range(span), tone: "server" } }),
  );

  // A session's name on the first of its blocks in a row; the next blocks are the same work.
  let last: string | undefined;
  project.work.forEach((span, i) => {
    const title = sessionTitle(project, span.session);
    const text = span.session !== last ? title : "";
    last = span.session;
    list.push({
      key: `w${i}`,
      kind: "work",
      at: span.start,
      style: place(span.start, span.end, r),
      text,
      tip: { title: t("day.tip.work", { duration: duration(span.end - span.start) }), detail: title ? `${range(span)} · ${title}` : range(span), tone: "claude" },
    });
  });

  project.wait.forEach((span, i) => {
    const title = sessionTitle(project, span.session);
    const minutes = Math.round((span.end - span.start) / 60_000);
    list.push({
      key: `a${i}`,
      kind: "wait",
      at: span.start,
      style: place(span.start, span.end, r),
      text: minutes >= 1 ? t("time.durationMinutes", { minutes }) : "",
      tip: { title: t("day.tip.wait", { duration: duration(span.end - span.start) }), detail: title ? `${range(span)} · ${title}` : range(span), tone: "claude" },
    });
  });

  project.commits.forEach((commit, i) =>
    list.push({ key: `c${i}`, kind: "commit", at: commit.at, style: { left: `${pct(commit.at, r)}%` }, text: "", tip: { title: t("day.tip.commit", { time: clock(commit.at) }), detail: commit.subject, tone: "plain" } }),
  );

  project.crashes.forEach((at, i) =>
    list.push({ key: `x${i}`, kind: "crash", at, style: { left: `${pct(at, r)}%` }, text: "✕", tip: { title: t("day.tip.crash", { time: clock(at) }), detail: "", tone: "crash" } }),
  );

  return list;
}

export function lane(project: DayProject, r: TimeRange | null): Lane {
  const time = covered(spansOf(project));
  return {
    project,
    time,
    total: time ? duration(time) : t("day.commitCount", { count: project.commits.length }),
    blocks: blocks(project, r),
  };
}

/* ---------- totals ---------- */

/** The day's figures, over every project. */
export function dayTotals(projects: DayProject[]): Array<{ label: string; value: string }> {
  return [
    { label: t("day.active"), value: duration(covered(projects.flatMap(spansOf))) },
    { label: t("day.claudeWorked"), value: duration(sum(projects.flatMap((p) => p.work))) },
    { label: t("day.waitedOnYou"), value: duration(sum(projects.flatMap((p) => p.wait))) },
    { label: t("day.commits"), value: String(projects.reduce((n, p) => n + p.commits.length, 0)) },
  ];
}

/** One project's figures; an empty `value` is shown as a dash. */
export function projectTiles(p: DayProject): Array<{ label: string; value: string; note: string }> {
  const server = covered(p.server);
  return [
    { label: t("day.claudeWorked"), value: p.work.length ? duration(sum(p.work)) : "", note: "" },
    { label: t("day.waitedOnYou"), value: p.wait.length ? duration(sum(p.wait)) : "", note: "" },
    { label: t("day.serverUp"), value: server ? duration(server) : "", note: p.crashes.length ? t("day.crashCount", { count: p.crashes.length }) : "" },
    { label: t("day.commits"), value: p.commits.length ? String(p.commits.length) : "", note: "" },
  ];
}

/** `part`'s width in a session's bar of work and waiting, as a CSS percentage. */
export function sessionShare(part: number, session: DaySession): string {
  const whole = session.work + session.wait;
  return whole ? `${(part / whole) * 100}%` : "0%";
}

/* ---------- where the time went ---------- */

/** Project colours, picked at random for now: from the path, so a project keeps its colour.
    Hues far apart, and none of Claude's orange, the server's green or a crash's red. */
const PALETTE = ["#7aa2f7", "#c49ef0", "#6fc6d9", "#e5c07b", "#f08fb6", "#a3acc2"];

function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** Each project its colour; two projects of the day never share one while the palette lasts. */
export function projectColors(paths: string[]): Map<string, string> {
  const taken = new Set<number>();
  const map = new Map<string, string>();
  for (const path of [...paths].sort()) {
    let at = hash(path) % PALETTE.length;
    for (let i = 0; i < PALETTE.length && taken.has(at); i++) at = (at + 1) % PALETTE.length;
    taken.add(at);
    map.set(path, PALETTE[at]);
  }
  return map;
}

/** Each project's share of the day's time, as its lane counts it. */
export function timeSplit(lanes: Lane[], colors: Map<string, string>, percent: Intl.NumberFormat): SplitPart[] {
  const parts = lanes.filter((lane) => lane.time > 0);
  const whole = parts.reduce((total, lane) => total + lane.time, 0);
  return parts.map((lane) => {
    const share = percent.format(lane.time / whole);
    return {
      path: lane.project.path,
      name: lane.project.name,
      grow: lane.time / whole,
      share,
      color: colors.get(lane.project.path) ?? PALETTE[0],
      tip: { title: lane.project.name, detail: `${lane.total} · ${share}`, tone: "plain" },
    };
  });
}
