// The Sessions page as numbers and text: which section a session sits in and in what order, what
// its status line says, where it runs, its spend and its hours. No component state; what reads
// the language (`t`, `clock`, `duration`) follows it.

import { clock, duration, HOUR, type TimeRange } from "./day";
import { ago, editorName } from "./format";
import { compactText, modelName, moneyText } from "./fuel";
import { fuzzy } from "./fuzzy";
import { t, type Key } from "./i18n";
import type { Project, SessionOrigin, SessionRow, SessionSpan, TerminalView, Turn } from "./types";

/** The list's sections, top to bottom. `open`: running, nothing going on, nothing unseen. */
export type Section = "waiting" | "working" | "open" | "ended";
export const SECTIONS: Section[] = ["waiting", "working", "open", "ended"];

/** The page's filter tabs, and the sections each shows. */
export type Filter = "all" | "waiting" | "working" | "ended";
export const FILTERS: Array<{ id: Filter; label: Key; sections: Section[] }> = [
  { id: "all", label: "sessions.filter.all", sections: SECTIONS },
  { id: "waiting", label: "sessions.filter.waiting", sections: ["waiting"] },
  { id: "working", label: "sessions.filter.working", sections: ["working"] },
  { id: "ended", label: "sessions.filter.ended", sections: ["ended"] },
];

export const SECTION_LABELS: Record<Section, Key> = {
  waiting: "sessions.section.waiting",
  working: "sessions.section.working",
  open: "sessions.section.open",
  ended: "sessions.section.ended",
};

export function sectionOf(row: SessionRow): Section {
  return row.phase === "idle" ? "open" : row.phase;
}

/** Its name, else the project's with the id's first part: `paddock · 9bb85e86`. */
export function sessionTitle(row: SessionRow): string {
  return row.title || `${row.project} · ${row.id.slice(0, 8)}`;
}

/* ---------- order ---------- */

/** Waits that block (a permission prompt, a question) come before a finished turn. */
export const WAIT_RANK: Record<Turn["kind"], number> = { permission: 0, asking: 1, finished: 2 };

function lastSpanEnd(row: SessionRow): number | null {
  const spans = row.today?.spans ?? [];
  return spans.length ? Math.max(...spans.map((span) => span.end)) : null;
}

/** When it last did something: its last span's end, else when its phase began. */
export function lastActive(row: SessionRow): number | null {
  return lastSpanEnd(row) ?? row.since ?? row.turn?.at ?? null;
}

/** When an ended session ended. */
export function endedAt(row: SessionRow): number | null {
  return row.since ?? lastSpanEnd(row) ?? row.turn?.at ?? null;
}

/** Earliest first, unknown last. */
function earlier(a: number | null, b: number | null): number {
  return (a ?? Infinity) - (b ?? Infinity);
}

/** Latest first, unknown last. */
function later(a: number | null, b: number | null): number {
  return (b ?? -Infinity) - (a ?? -Infinity);
}

const ORDER: Record<Section, (a: SessionRow, b: SessionRow) => number> = {
  // What blocks first, then whoever has waited longest.
  waiting: (a, b) => WAIT_RANK[a.turn?.kind ?? "finished"] - WAIT_RANK[b.turn?.kind ?? "finished"] || earlier(a.turn?.at ?? null, b.turn?.at ?? null),
  // Working longest first.
  working: (a, b) => earlier(a.since, b.since),
  // The latest active first.
  open: (a, b) => later(lastActive(a), lastActive(b)),
  // The latest ended first.
  ended: (a, b) => later(endedAt(a), endedAt(b)),
};

/** The sections shown, each with its sessions' ids in order; empty ones left out. */
export type Layout = Array<{ section: Section; ids: string[] }>;

export function layout(rows: SessionRow[], sections: Section[] = SECTIONS): Layout {
  return sections
    .map((section) => ({
      section,
      ids: rows
        .filter((row) => sectionOf(row) === section)
        .sort((a, b) => ORDER[section](a, b) || a.id.localeCompare(b.id))
        .map((row) => row.id),
    }))
    .filter((group) => group.ids.length);
}

/**
 * `next`, with the places of `frozen` kept (a row under the pointer or the keyboard never moves):
 * a session stays in its section and place whatever its state does meanwhile, even out of the
 * filter's sections, while it is in `present` (default: `next`); a new one joins the end of its
 * section, a gone one leaves.
 */
export function keepOrder(frozen: Layout, next: Layout, present: Set<string> = new Set(next.flatMap((group) => group.ids))): Layout {
  const placed = new Set<string>();
  const ids = new Map<Section, string[]>();
  for (const group of frozen) {
    const kept = group.ids.filter((id) => present.has(id));
    kept.forEach((id) => placed.add(id));
    ids.set(group.section, kept);
  }
  for (const group of next) {
    const fresh = group.ids.filter((id) => !placed.has(id));
    if (fresh.length) ids.set(group.section, [...(ids.get(group.section) ?? []), ...fresh]);
  }
  return SECTIONS.filter((section) => ids.get(section)?.length).map((section) => ({ section, ids: ids.get(section) ?? [] }));
}

/** What bringing a session up does, as its button and menu say it: an ended one opens again in
 * the editor, one running where Pitwall can't reach only has the project's window come up, the
 * rest come up where they run. */
export function bringKeys(row: SessionRow): { label: Key; title: Key } {
  if (row.phase === "ended") return { label: "sessions.action.reopen", title: "sessions.action.reopenTitle" };
  if (runsElsewhere(row)) return { label: "common.openInEditor", title: unlinkedEditor(row.origin) ? "sessions.unlinked.body" : "sessions.elsewhere.body" };
  return { label: "sessions.action.bringUp", title: "sessions.action.bringUpTitle" };
}

/** Matches `q` (trimmed, lower case) by its name or its project's (a whole path would match
 * nearly anything). */
export function matches(row: SessionRow, q: string): boolean {
  return !q || !!(fuzzy(sessionTitle(row), q) ?? fuzzy(row.project, q));
}

/* ---------- status ---------- */

/** `hot`: waiting on you (Claude's colour); `live`: working; `quiet`: open or ended. */
export type Status = { text: string; tone: "hot" | "live" | "quiet" };

/** How long, at least "< 1 min". */
function lasting(ms: number): string {
  return duration(Math.max(ms, 1));
}

/** The row's status: `Bash wants permission · 3 min`, `Working for 6 min`, `Ended · 19:12`. */
export function statusLine(row: SessionRow, now: number): Status {
  switch (row.phase) {
    case "waiting": {
      const turn = row.turn;
      if (!turn) return { text: t("sessions.status.waiting"), tone: "hot" };
      const at = turn.at ?? row.since;
      // Without a time, the wait's kind alone.
      if (!at) return { text: t(({ finished: "claude.finished", asking: "claude.asking", permission: "claude.permission" } as const)[turn.kind]), tone: "hot" };
      const wait = lasting(now - at);
      if (turn.kind === "permission") {
        return { text: row.tool ? t("sessions.status.permissionTool", { tool: row.tool, duration: wait }) : t("sessions.status.permission", { duration: wait }), tone: "hot" };
      }
      if (turn.kind === "asking") return { text: t("sessions.status.asking", { duration: wait }), tone: "hot" };
      return { text: t("sessions.status.finished", { ago: ago(at, now) }), tone: "hot" };
    }
    case "working":
      return { text: row.since ? t("sessions.status.working", { duration: lasting(now - row.since) }) : t("claude.working"), tone: "live" };
    case "idle": {
      const at = lastActive(row);
      return { text: at ? t("sessions.status.idle", { ago: ago(at, now) }) : t("sessions.status.idleNow"), tone: "quiet" };
    }
    case "ended": {
      const at = endedAt(row);
      return { text: at ? t("sessions.status.ended", { time: clock(at) }) : t("sessions.status.endedNow"), tone: "quiet" };
    }
  }
}

/* ---------- where it runs ---------- */

export type OriginIcon = "pitwall" | "editor" | "terminal" | "external" | "info";

/** `short` for the row (a VS Code window by its name when that isn't the project's), `long` for
 * the details and the row's tooltip. */
export type OriginView = { icon: OriginIcon; short: string; long: string };

export function originView(origin: SessionOrigin, project: string, editor: string | undefined): OriginView {
  const app = editorName(editor);
  switch (origin.kind) {
    case "pitwall": {
      const text = t("sessions.origin.pitwall");
      return { icon: "pitwall", short: text, long: text };
    }
    case "vscodeTab":
      return { icon: "editor", short: origin.window && origin.window !== project ? origin.window : app, long: t("sessions.origin.window", { editor: app, window: origin.window }) };
    case "vscodeTerminal": {
      const terminal = t("sessions.origin.editorTerminal", { editor: app });
      return { icon: "terminal", short: origin.window && origin.window !== project ? origin.window : terminal, long: t("sessions.origin.window", { editor: terminal, window: origin.window }) };
    }
    case "terminal": {
      const text = origin.app || t("sessions.origin.terminal");
      return { icon: "terminal", short: text, long: text };
    }
    case "other": {
      if (unlinkedEditor(origin)) return { icon: "editor", short: app, long: t("sessions.origin.vscodeUnlinked", { editor: app }) };
      const text = t("sessions.origin.other");
      return { icon: "external", short: text, long: origin.entrypoint ? `${text} · ${origin.entrypoint}` : text };
    }
    case "unknown": {
      return { icon: "info", short: t("sessions.origin.unknown"), long: t("sessions.origin.unknownLong") };
    }
  }
}

/** A Claude Code tab in an editor window without the extension: no window record names it, so it
 * can't be brought up there. */
export function unlinkedEditor(origin: SessionOrigin): boolean {
  return origin.kind === "other" && origin.entrypoint === "claude-vscode";
}

/**
 * Running somewhere Pitwall can't reach into (a terminal app, the SDK…): bringing it up only
 * brings up the project's window, as a running session is never opened a second time.
 */
export function runsElsewhere(row: SessionRow): boolean {
  return row.phase !== "ended" && row.running !== false && (row.origin.kind === "terminal" || row.origin.kind === "other");
}

/** The Pitwall terminal it runs (or ran) in, while that tab is still open. */
export function sessionTerminal(row: SessionRow, projects: Project[]): TerminalView | null {
  const origin = row.origin;
  if (origin.kind !== "pitwall") return null;
  for (const project of projects) {
    const found = project.terminals.find((terminal) => terminal.id === origin.terminal);
    if (found) return found;
  }
  return null;
}

/** Its project's other sessions that wait on you: marking it seen clears them too (seen is kept
 * per project). */
export function othersWaiting(row: SessionRow, rows: SessionRow[]): number {
  const where = row.path ?? row.folder;
  return rows.filter((other) => other.id !== row.id && other.phase === "waiting" && (other.path ?? other.folder) === where).length;
}

/** The command that continues it in a terminal. */
export function resumeCommand(row: SessionRow): string {
  return `claude --resume ${row.id}`;
}

/* ---------- spend ---------- */

/** A row's `2.1M · $3.40` (the details say the cost is the API's); just the tokens when none of
 * it has a price; `—` without any. */
export function spendText(spend: SessionRow["spend"], lang: string): string {
  if (!spend || !spend.tokens.total) return "—";
  const tokens = compactText(spend.tokens.total, lang);
  return spend.costUsd > 0 || spend.priced ? `${tokens} · ${moneyText(spend.costUsd, "USD", lang)}` : tokens;
}

/** Its models as people say them, each once: `Opus 5.5, Haiku 4.5`. */
export function modelsText(spend: SessionRow["spend"]): string {
  return [...new Set((spend?.models ?? []).map(modelName))].join(", ");
}

/* ---------- its day ---------- */

/** Whole hours around its spans (and now, while it runs), at least two of them. */
export function sessionRange(spans: SessionSpan[], now: number, live: boolean): TimeRange | null {
  if (!spans.length) return null;
  let lo = Math.min(...spans.map((span) => span.start));
  let hi = Math.max(...spans.map((span) => span.end));
  if (live) {
    lo = Math.min(lo, now);
    hi = Math.max(hi, now);
  }
  const from = new Date(lo);
  from.setMinutes(0, 0, 0);
  const to = new Date(hi);
  if (to.getMinutes() || to.getSeconds() || to.getMilliseconds()) {
    to.setMinutes(0, 0, 0);
    to.setHours(to.getHours() + 1);
  }
  const hours = Math.max(2, Math.round((to.getTime() - from.getTime()) / HOUR));
  return { from: from.getTime(), to: from.getTime() + hours * HOUR, hours };
}
