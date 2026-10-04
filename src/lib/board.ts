// The Board page as numbers and text: which cards a column shows and in what order, where a moved
// card lands, and what a card given to Claude says about its session. No component state; what
// reads the language (`t`, `clock`, `duration`) follows it.

import { clock, duration } from "./day";
import { ago } from "./format";
import { language, t, type Key } from "./i18n";
import { inPeriod, type Period } from "./period";
import { WAIT_RANK } from "./sessions";
import type { BoardColumn, Card, Project, SessionRow, Turn } from "./types";

export const COLUMNS: BoardColumn[] = ["queued", "claude", "review", "done"];

export const COLUMN_LABELS: Record<BoardColumn, Key> = {
  queued: "board.column.queued",
  claude: "board.column.claude",
  review: "board.column.review",
  done: "board.column.done",
};

/** The mark at a column's head, before its name; Claude's column has Claude's own (ClaudeLogo). */
export const COLUMN_ICONS = { queued: "clock", review: "eye", done: "check" } as const;

/** A project in the board picker's list while Claude waits on you on its board: its name, and on
 * what. */
export const WAIT_LABELS: Record<Turn["kind"], Key> = {
  finished: "board.picker.finished",
  asking: "board.picker.asking",
  permission: "board.picker.permission",
};

/** How a project's board is drawn: its columns side by side, or one under the other with a card
 * a row. The settings keep the one picked (`boardView`). */
export type BoardLayout = "columns" | "list";

/** Every project's board shows this many cards per column and lane; "+N more" opens the rest. */
export const LANE_LIMIT = 2;

/**
 * A lane on every project's board: a project's cards per column (those the search leaves) and how
 * many it did in the days shown. `idle` and `next` are true whatever the search says: it has no card in
 * Claude's column at all, and its first card up next. `project` null: a folder no longer among the
 * projects (`name`: its last part), whose cards are kept but never given.
 */
export type Lane = {
  path: string;
  name: string;
  project: Project | null;
  queued: Card[];
  claude: Card[];
  review: Card[];
  done: number;
  idle: boolean;
  next: Card | null;
};

/** The cards a lane draws in a column: its first few (the project's board has the rest), or every
 * one for a folder no longer listed, which has no board to open. */
export function laneDrawn(lane: Lane, column: "queued" | "claude" | "review"): Card[] {
  return lane.project ? lane[column].slice(0, LANE_LIMIT) : lane[column];
}

/** A folder's name: the last part of its path. */
export function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/**
 * The lane of `path` (`project`: the listed project there, null for a folder no longer listed):
 * `shown`, the cards the search leaves, per column; `every`, all the cards on the board, for what
 * the search must not change.
 */
export function laneOf(path: string, project: Project | null, shown: Card[], every: Card[]): Lane {
  const mine = every.filter((card) => card.path === path);
  return {
    path,
    name: project?.name ?? folderName(path),
    project,
    queued: columnCards(shown, path, "queued"),
    claude: columnCards(shown, path, "claude"),
    review: columnCards(shown, path, "review"),
    done: columnCards(shown, path, "done").length,
    idle: !mine.some((card) => card.column === "claude"),
    next: project ? (mine.find((card) => card.column === "queued") ?? null) : null,
  };
}

/** A session not in the list this long after the card was given has gone, and a card still
 * without one by then is no longer starting (ms). */
const LINK_GRACE_MS = 60_000;
/** A terminal not among the project's this long after the card was given has closed (ms): the
 * board can come before the project's new terminal does. */
const TERMINAL_GRACE_MS = 5_000;

/** On the board for the days shown (`period`): every card still to do, and those done then. Today
 * (null), that is every card but those done on an earlier day. */
export function onBoard(card: Card, period: Period, now: number): boolean {
  return card.column !== "done" || inPeriod(card.movedAt, period, now);
}

/** Matches `q` (trimmed) in its title or note, case aside. */
export function cardMatches(card: Card, q: string): boolean {
  if (!q) return true;
  const lower = (text: string): string => text.toLocaleLowerCase(language.value);
  const needle = lower(q);
  return lower(card.title).includes(needle) || lower(card.note).includes(needle);
}

/** A project's cards in a column, in the board's order (a card just done goes on top, and like
 * any other it can be moved from there). `cards`: those on the board (`onBoard`), so the done ones
 * are the shown days' only. */
export function columnCards(cards: Card[], path: string, column: BoardColumn): Card[] {
  return cards.filter((card) => card.path === path && card.column === column);
}

/**
 * `cards` with `id` moved to `column`, at `index` among its project's other cards there: what
 * `board_move` does, shown before the core confirms it. Every card stays; only that one moves.
 */
export function moveCard(cards: Card[], id: string, column: BoardColumn, index: number, now: number): Card[] {
  const card = cards.find((c) => c.id === id);
  if (!card) return cards;
  const rest = cards.filter((c) => c.id !== id);
  const peers = rest.filter((c) => c.path === card.path && c.column === column);
  const before = peers[Math.max(0, index)];
  const at = before ? rest.indexOf(before) : peers.length ? rest.indexOf(peers[peers.length - 1]) + 1 : rest.length;
  rest.splice(at, 0, { ...card, column, movedAt: card.column === column ? card.movedAt : now });
  return rest;
}

/** The title and note as one text: what "Copy text" copies and what Claude gets (the core adds a
 * line asking for a summary at the end). */
export function cardText(card: Card): string {
  return card.note.trim() ? `${card.title}\n\n${card.note.trim()}` : card.title;
}

/**
 * A given card's session as its card shows it. `mark`: a filled dot waits on you, a ring works
 * (breathing) or is quiet (grey); `tone`: Claude's colour while it waits on you. `closed`: the
 * session ended before its turn did (it can be given again); `unseen`: a finished turn not looked
 * at yet; `waits`: what it waits on you with (a finished turn not looked at yet among them), null
 * while it doesn't.
 */
export type CardStatus = {
  text: string;
  tone: "hot" | "live" | "quiet";
  mark: "waiting" | "working" | "idle" | null;
  closed: boolean;
  unseen: boolean;
  waits: Turn["kind"] | null;
};

/**
 * What Claude waits on you with on each project's board, by the project's path: the most pressing
 * of its cards' waits (a permission prompt, a question, then a finished turn not seen yet). Seen,
 * a finished turn waits no longer, though its card stays to review. A board where nothing waits
 * has no entry.
 */
export function boardWaits(cards: Card[], statuses: Map<string, CardStatus | null>): Map<string, Turn["kind"]> {
  const waits = new Map<string, Turn["kind"]>();
  for (const card of cards) {
    const kind = statuses.get(card.id)?.waits;
    if (!kind) continue;
    const known = waits.get(card.path);
    if (!known || WAIT_RANK[kind] < WAIT_RANK[known]) waits.set(card.path, kind);
  }
  return waits;
}

/** How long, at least "< 1 min". */
function lasting(ms: number): string {
  return duration(Math.max(ms, 1));
}

/**
 * What a card in Claude's column or in review says, and one taken back to Up next while its
 * session still runs. A card in review whose session went on (at work again, or waiting on a
 * permission or a question) says that, not when it finished. `row`: its session in the Sessions
 * list, if there; `listed`: that list has been read (else a missing row says nothing yet);
 * `terminalOpen`: its Pitwall terminal is still open (null when it has none).
 */
export function cardStatus(card: Card, row: SessionRow | undefined, listed: boolean, terminalOpen: boolean | null, now: number): CardStatus | null {
  const status = (text: string, tone: CardStatus["tone"], mark: CardStatus["mark"], extra: Partial<CardStatus> = {}): CardStatus => ({
    text,
    tone,
    mark,
    closed: false,
    unseen: false,
    waits: null,
    ...extra,
  });

  /** A finished turn not looked at yet. */
  const unread: Partial<CardStatus> = { unseen: true, waits: "finished" };

  /** A session at work, or waiting on a permission or a question: what its card says, in Claude's
   * column or anywhere else; null in any other state. */
  const busy = (session: SessionRow): CardStatus | null => {
    if (session.phase === "working") {
      return status(session.since ? t("board.working", { duration: lasting(now - session.since) }) : t("claude.working"), "live", "working");
    }
    const turn = session.phase === "waiting" ? session.turn : null;
    if (!turn || turn.kind === "finished") return null;
    const wait = lasting(now - (turn.at ?? session.since ?? now));
    if (turn.kind === "asking") return status(t("sessions.status.asking", { duration: wait }), "hot", "waiting", { waits: "asking" });
    const text = session.tool ? t("board.permissionTool", { tool: session.tool, duration: wait }) : t("sessions.status.permission", { duration: wait });
    return status(text, "hot", "waiting", { waits: "permission" });
  };

  if (card.column === "review") {
    // Its session went on after the turn that brought it here: it says so, as in Claude's column.
    const again = row ? busy(row) : null;
    if (again) return again;
    const unseen = row?.phase === "waiting" && row.turn?.kind === "finished";
    const at = unseen && row?.turn ? row.turn.at : card.movedAt;
    return status(t("sessions.status.finished", { ago: ago(at, now) }), unseen ? "hot" : "quiet", unseen ? "waiting" : null, unseen ? unread : {});
  }
  // Up next: only a session that still runs says so; the card kept it when it was moved.
  if (card.column === "queued" && (!row || row.phase === "ended")) return null;
  if (card.column !== "claude" && card.column !== "queued") return null;

  const closed = (at: number | null): CardStatus =>
    status(at ? t("board.closed", { time: clock(at) }) : t("board.closedNow"), "quiet", "idle", { closed: true });
  const starting = (): CardStatus => status(t("board.starting"), "live", "working");

  const given = card.givenAt ?? card.movedAt;
  if (!card.session) {
    // Given, its session not linked yet: starting, unless its terminal has closed meanwhile. Past
    // the grace no session of its own will come, though its tab may still be open (Claude never
    // started in it): it can be given again.
    const gone = now - given > LINK_GRACE_MS || (terminalOpen === false && now - given > TERMINAL_GRACE_MS) || (card.terminal === null && card.givenAt === null);
    return gone ? closed(null) : starting();
  }
  if (!row) return listed && now - given > LINK_GRACE_MS ? closed(null) : starting();

  switch (row.phase) {
    case "ended":
      // Opened again in a new terminal a moment ago: it ended before, and is starting now.
      return card.column === "claude" && card.terminal !== null && now - given <= LINK_GRACE_MS ? starting() : closed(row.since);
    case "idle":
      return status(t("sessions.status.idleNow"), "quiet", "idle");
    case "working":
    case "waiting": {
      const at = row.turn?.at ?? row.since ?? now;
      return busy(row) ?? status(t("sessions.status.finished", { ago: ago(at, now) }), "hot", "waiting", unread);
    }
  }
}
