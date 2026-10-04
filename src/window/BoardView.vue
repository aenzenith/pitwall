<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, toRaw, watch } from "vue";

import Icon from "../components/Icon.vue";
import PeriodPicker from "../components/PeriodPicker.vue";
import Spinner from "../components/Spinner.vue";
import {
  boardWaits,
  cardMatches,
  cardStatus,
  cardText,
  COLUMN_LABELS,
  COLUMNS,
  columnCards,
  folderName,
  laneDrawn,
  laneOf,
  moveCard,
  onBoard,
  type BoardLayout,
} from "../lib/board";
import { SETTLE_MS, useCardDrag, type DropTarget } from "../lib/cardDrag";
import { useCardImages } from "../lib/cardImages";
import { t, type Key } from "../lib/i18n";
import { useNativeMenu, type MenuEntry, type MenuPoint } from "../lib/nativeMenu";
import { detailTerminalHeight } from "../lib/panel";
import { ALL as EVERY_DAY, type Period } from "../lib/period";
import { dragRegion, keys, primary } from "../lib/platform";
import { messageState, useLastMessage } from "../lib/lastMessage";
import { originView, othersWaiting, sessionTerminal } from "../lib/sessions";
import { api, board, now, sessions, snapshot } from "../lib/store";
import { focusView, measure } from "../lib/terminals";
import type { BoardColumn, Card, SessionRow, TerminalView } from "../lib/types";
import BoardCard from "./board/BoardCard.vue";
import BoardColumns from "./board/BoardColumns.vue";
import BoardLanes from "./board/BoardLanes.vue";
import BoardPicker from "./board/BoardPicker.vue";
import CardDetail from "./board/CardDetail.vue";
import CardDialog from "./board/CardDialog.vue";
import { BOARD_ACTIONS, type BoardActions, type CardBusy } from "./board/context";
import DeleteDialog from "./board/DeleteDialog.vue";
import SessionDetail from "./sessions/SessionDetail.vue";
import SessionNotice from "./sessions/SessionNotice.vue";

/**
 * The board: cards of work per project, given to Claude and followed through review to done.
 * One project's board (four columns, side by side or as a list), or every project's (a lane each), and beside it the selected
 * card's details: its session's, with the Pitwall terminal it runs in, or once its work is over
 * its note and what Claude last said. `projectPath`: the project
 * list's selection, the board it opens on. The board's cards come from `board`, their sessions'
 * state from `sessions` (WindowApp keeps both live while this page shows).
 */
const props = defineProps<{ projectPath: string | null }>();
const emit = defineEmits<{ openSettings: [] }>();

/** The least an action's spinner shows, so one the core answers at once still shows. */
const MIN_SPIN_MS = 400;
/** How long a failed action's message stays under its card. */
const ERROR_MS = 8_000;

const projects = computed(() => snapshot.value?.projects ?? []);
const listed = computed(() => new Set(projects.value.map((p) => p.path)));

/* ---------- which board ---------- */

/** Every project's board, as the settings keep it. */
const ALL = "all";

/** The board shown last; until one was picked, the project list's selection. */
function lastScope(): string | null {
  const kept = snapshot.value?.settings.boardScope;
  if (kept === ALL) return null;
  return kept ?? props.projectPath;
}

/** The project shown; null for every project's board. */
const scope = ref<string | null>(lastScope());
const project = computed(() => (scope.value ? (projects.value.find((p) => p.path === scope.value) ?? null) : null));

/**
 * A board picked by hand: the page opens on it next time too. What Claude finished counts as seen
 * on the board left (it was in sight) and on the one picked, as it does in a session's own tab;
 * the card that waited there is the one selected.
 */
function show(path: string | null): void {
  const left = scope.value;
  const waited = path ? all.value.find((card) => card.path === path && statuses.value.get(card.id)?.waits)?.id : undefined;
  scope.value = path;
  void api.setBoardScope(path ?? ALL);
  if (left) seeFinished(left);
  if (path) seeFinished(path);
  // Once the new board's first card has been selected.
  if (waited) {
    void nextTick(() => {
      if (shownIds.value.has(waited)) selectedId.value = waited;
    });
  }
}

// A project taken off the list leaves every project's board.
watch(
  projects,
  (list) => {
    if (snapshot.value && scope.value && !list.some((p) => p.path === scope.value)) scope.value = null;
  },
  { immediate: true },
);

const menu = useNativeMenu();

/* ---------- how it is drawn ---------- */

const LAYOUTS: Array<{ id: BoardLayout; icon: "board" | "list"; label: Key }> = [
  { id: "columns", icon: "board", label: "board.view.columns" },
  { id: "list", icon: "list", label: "board.view.list" },
];

/** A project's board as columns or as a list: the one picked here, else the one the settings keep
 * (columns until one is picked). */
const pickedLayout = ref<BoardLayout | null>(null);
const layout = computed<BoardLayout>(() => pickedLayout.value ?? (snapshot.value?.settings.boardView === "list" ? "list" : "columns"));

/** Shown at once, and kept for the page's next opening. */
function setLayout(next: BoardLayout): void {
  pickedLayout.value = next;
  void api.setBoardView(next).catch(() => undefined);
}

/* ---------- which days ---------- */

/** The days whose done cards show: today's, an earlier day's, or every day's. The cards still to
 * do show whatever the day. Today's again each time the page opens. */
const period = ref<Period>(null);

/* ---------- what it shows ---------- */

const query = ref("");
const q = computed(() => query.value.trim());

/** The listed projects' cards on the board (done ones: the shown days'). */
const all = computed(() => (board.value ?? []).filter((card) => listed.value.has(card.path) && onBoard(card, period.value, now.value)));
/** Those of folders no longer among the projects (none until the projects are known): every
 * project's board keeps them in sight under its lanes, to be read, closed or moved to a project. */
const unlisted = computed(() => (snapshot.value ? (board.value ?? []).filter((card) => !listed.value.has(card.path) && onBoard(card, period.value, now.value)) : []));
/** Those the search leaves. */
const shown = computed(() => all.value.filter((card) => cardMatches(card, q.value)));
const unlistedShown = computed(() => unlisted.value.filter((card) => cardMatches(card, q.value)));
const byId = computed(() => new Map((board.value ?? []).map((card) => [card.id, card])));

const rowsById = computed(() => new Map((sessions.value?.sessions ?? []).map((row) => [row.id, row])));

function rowOf(card: Card): SessionRow | null {
  return card.session ? (rowsById.value.get(card.session) ?? null) : null;
}

/** Its Pitwall terminal is still open; null without one. */
function terminalOpen(card: Card): boolean | null {
  if (card.terminal === null) return null;
  return projects.value.find((p) => p.path === card.path)?.terminals.some((term) => term.id === card.terminal) ?? false;
}

/** The Pitwall terminal its work runs in, while that tab is open: the one it was given in, else
 * the one its session's origin names (a card linked to a session already open). */
function terminalOf(card: Card): TerminalView | null {
  const given = card.terminal === null ? undefined : projects.value.find((p) => p.path === card.path)?.terminals.find((term) => term.id === card.terminal);
  if (given) return given;
  const row = rowOf(card);
  return row ? sessionTerminal(row, projects.value) : null;
}

const statuses = computed(
  () => new Map([...all.value, ...unlisted.value].map((card) => [card.id, cardStatus(card, rowOf(card) ?? undefined, sessions.value !== null, terminalOpen(card), now.value)])),
);

/** What Claude waits on you with on each project's board (a finished turn not seen yet, a
 * question, a permission prompt): the picker marks those projects with Claude's dot. */
const waits = computed(() => boardWaits(all.value, statuses.value));

/** What Claude finished on a project's board counts as seen: every session of the project stops
 * waiting on you. Not while one asks something there (a question, a permission prompt): seen, it
 * would no longer say so. */
function seeFinished(path: string): void {
  if (waits.value.get(path) !== "finished") return;
  const asks = (sessions.value?.sessions ?? []).some((row) => (row.path ?? row.folder) === path && row.phase === "waiting" && !!row.turn && row.turn.kind !== "finished");
  if (!asks) void api.markSeen(path).catch(() => undefined);
}

const projectCards = computed(() => (project.value ? all.value.filter((card) => card.path === project.value?.path) : []));

/** Every project's lanes: those with cards, the rest on request ("Show"); while searching, those
 * with a match. */
const showHidden = ref(false);
const laneAll = computed(() =>
  projects.value.map((p) => ({
    lane: laneOf(p.path, p, shown.value, all.value),
    has: shown.value.some((card) => card.path === p.path),
  })),
);
const lanes = computed(() => laneAll.value.filter((entry) => entry.has || (showHidden.value && !q.value)).map((entry) => entry.lane));
const hiddenLanes = computed(() => (q.value ? 0 : laneAll.value.filter((entry) => !entry.has).length));
/** Under them, a lane for each folder no longer listed that has a card the search leaves, by path
 * (the board's own order would swap two lanes as their cards move). */
const unlistedLanes = computed(() =>
  [...new Set(unlistedShown.value.map((card) => card.path))]
    .sort((a, b) => a.localeCompare(b))
    .map((path) => laneOf(path, null, unlistedShown.value, unlisted.value)),
);

const subtitle = computed(() => {
  if (project.value) return [project.value.name, project.value.git?.branch, t("board.cardCount", { count: projectCards.value.length })].filter(Boolean).join(" · ");
  const withCards = new Set(all.value.map((card) => card.path)).size;
  return `${t("board.allProjects")} · ${t("board.allSubtitle", { cards: t("board.cardCount", { count: all.value.length }), projects: t("board.inProjects", { count: withCards }) })}`;
});

/** No card at all here (searching or not). */
const empty = computed(() => (project.value ? !projectCards.value.length : !all.value.length && !unlisted.value.length));
/** Cards, but none the search leaves. */
const noMatch = computed(
  () => !!q.value && (project.value ? !shown.value.some((card) => card.path === project.value?.path) : !lanes.value.length && !unlistedLanes.value.length),
);

/** Claude Code's hook is missing or older than the current set: the board moves on from the logs. */
const hook = computed(() => {
  const state = snapshot.value;
  if (!state) return null;
  return !state.claudeHook ? "missing" : state.claudeHookOutdated ? "outdated" : null;
});

/* ---------- selection ---------- */

/** The cards as they read, column by column (lane by lane): where the selection may go. */
const visibleIds = computed<string[]>(() => {
  const p = project.value;
  if (p) return COLUMNS.flatMap((column) => columnCards(shown.value, p.path, column).map((card) => card.id));
  return [...lanes.value, ...unlistedLanes.value].flatMap((lane) => [...laneDrawn(lane, "queued"), ...laneDrawn(lane, "claude"), ...laneDrawn(lane, "review")].map((card) => card.id));
});
/** Every card the page shows, drawn or not (a lane draws only its first few, and none of its done
 * ones): the selection stays while its card is one of them. */
const shownIds = computed(() => {
  const p = project.value;
  const cards = p ? shown.value.filter((card) => card.path === p.path) : [...shown.value, ...unlistedShown.value];
  return new Set(cards.map((card) => card.id));
});

const selectedId = ref<string | null>(null);
const selected = computed(() => (selectedId.value ? (byId.value.get(selectedId.value) ?? null) : null));
/** The keyboard is in the selected card's terminal: it stays selected, whatever takes it out of
 * the cards shown meanwhile. */
const typing = ref(false);

// Keep a selection while there is a card to show: the one selected for as long as the page shows
// it (wherever the board moves it), then the first; the first, on a new board.
watch(
  [visibleIds, shownIds, typing],
  ([ids, present, held]) => {
    if (held && selected.value) return;
    if (!selectedId.value || !present.has(selectedId.value)) selectedId.value = ids[0] ?? null;
  },
  { immediate: true },
);

/* ---------- the selected card's details ---------- */

/** Its session in the Sessions list: that session's details show; without one, the card's own. */
const selectedRow = computed(() => (selected.value ? rowOf(selected.value) : null));
/** Its work is over (in review or done, or its session closed): its details say what it was about,
 * its note with them, and leave its terminal out until the session is brought up. A card taken
 * back to Up next keeps its session: over once that session has ended or left the list. */
const selectedOver = computed(() => {
  const card = selected.value;
  if (!card) return false;
  if (card.column === "review" || card.column === "done") return true;
  // Closed, unless Claude never started in a tab that is still open: that tab stays in sight, it
  // says why.
  if (card.column === "claude") return statuses.value.get(card.id)?.closed === true && !(card.session === null && terminalOf(card) !== null);
  const row = selectedRow.value;
  return card.session !== null && (row ? row.phase === "ended" : terminalOf(card) === null);
});
/** The card whose terminal was brought up though its work is over: it shows while the card stays
 * selected. */
const opened = ref<string | null>(null);
const selectedTerminal = computed(() => {
  const card = selected.value;
  return card && (!selectedOver.value || opened.value === card.id) ? terminalOf(card) : null;
});
/** Its project's name; its folder's, for a card of a folder no longer listed. */
const selectedProject = computed(() => projects.value.find((p) => p.path === selected.value?.path)?.name ?? (selected.value ? folderName(selected.value.path) : ""));
const others = computed(() => (selectedRow.value ? othersWaiting(selectedRow.value, sessions.value?.sessions ?? []) : 0));

/** Seen is kept per project: every session of the project stops waiting on you. */
function markSeen(): void {
  const row = selectedRow.value;
  if (row?.phase === "waiting") void api.markSeen(row.path ?? row.folder).catch(() => undefined);
}

watch(selectedId, (id) => {
  if (id !== opened.value) opened.value = null;
});

/** The images the selected card's note names, for its details. */
const selectedImages = useCardImages(() => selected.value);

/** What Claude last said in the selected card's session, beside its terminal while it works and
 * once its work is over: the summary the card asked for. Asked for again when the card or its
 * session moves on. */
const lastMessage = useLastMessage(
  () => selected.value?.session ?? null,
  () => `${selected.value?.column} ${messageState(selectedRow.value, now.value)}`,
);

// Its work ends with the keyboard in its terminal: the terminal stays.
watch(selectedOver, (over) => {
  if (over && typing.value) opened.value = selectedId.value;
});

/** The selected card's details are changing hands with the keyboard in its terminal. */
let carrying = false;

// Its session's row comes (or goes) with the keyboard in its terminal: the details that take over
// (the session's for the card's own) attach the terminal anew, without the keyboard. It goes back
// in once they are drawn.
watch([selectedId, () => selectedRow.value !== null], ([id], [was]) => {
  if (id !== was || !typing.value) return;
  carrying = true;
  void nextTick(() => {
    carrying = false;
    const terminal = selectedTerminal.value;
    if (terminal) focusView(terminal.id);
    else typing.value = false;
  });
});

// The details taken out say the keyboard has left their terminal. Not while it is carried over:
// the card stays held, or the board could move the selection off it in between.
watch(
  typing,
  (on) => {
    if (!on && carrying) typing.value = true;
  },
  { flush: "sync" },
);

watch(scope, () => {
  selectedId.value = visibleIds.value[0] ?? null;
  showHidden.value = false;
});

const boardBody = ref<HTMLElement | null>(null);

/** ↓ in the search box goes on to the selected card; `still`: without scrolling to it. */
function focusSelected(still = false): void {
  boardBody.value?.querySelector<HTMLElement>('[data-card][tabindex="0"]')?.focus({ preventScroll: still });
}

/* ---------- actions ---------- */

/** The action under way and the last failure, by card id; a folder's move to a project (`rehome`)
 * is kept under the folder's path. */
const busy = ref<Record<string, CardBusy>>({});
const errors = ref<Record<string, string>>({});
const errorTimers = new Map<string, number>();

function setError(id: string, text: string): void {
  window.clearTimeout(errorTimers.get(id));
  errorTimers.delete(id);
  const next = { ...errors.value };
  if (text) {
    next[id] = text;
    errorTimers.set(
      id,
      window.setTimeout(() => setError(id, ""), ERROR_MS),
    );
  } else delete next[id];
  errors.value = next;
}

/** One action on a card at a time; its button spins until the core answers (at least a moment).
 * A failure undoes what was shown ahead of it and says why under the card. */
async function run(id: string, kind: Exclude<CardBusy, null>, work: () => Promise<unknown>, undo?: () => void): Promise<void> {
  if (busy.value[id]) return;
  busy.value = { ...busy.value, [id]: kind };
  const started = Date.now();
  try {
    await work();
    setError(id, "");
  } catch (failure) {
    undo?.();
    setError(id, String(failure));
  }
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  const next = { ...busy.value };
  delete next[id];
  busy.value = next;
}

/** The size the card's details will give the new Claude tab, so its shell starts at that size: the
 * details' width (380) less the terminal's room around it (57), and the height the details'
 * terminal is kept at (lib/panel), as far as the window goes, less its handle, label and room
 * (76). */
function terminalSize(): { cols: number; rows: number } | null {
  const host = document.createElement("div");
  const width = 323;
  const height = Math.min(detailTerminalHeight.value, window.innerHeight) - 76;
  host.style.cssText = `position: fixed; left: -10000px; top: 0; width: ${width}px; height: ${height}px; visibility: hidden`;
  document.body.appendChild(host);
  try {
    return measure(host);
  } catch {
    return null;
  } finally {
    host.remove();
  }
}

/** A new Claude session in one of the project's Pitwall terminals, with the card's text; the card
 * goes to Claude's column, and the board stays on screen. */
function give(id: string, mode: "new" | "plan"): void {
  void run(id, "give", () => api.boardGive(id, mode, terminalSize()));
}

function link(id: string, session: string): void {
  void run(id, "give", () => api.boardLink(id, session));
}

/**
 * `id` to `column`, at `index` among its project's other cards there. Shown at once, put back if
 * the core refuses. A move changes only where the card is: it keeps its session. Into Claude's
 * column from elsewhere it joins the end of that column and its session goes on (the core follows
 * one that still runs and opens one that has ended again); a card that has none starts one.
 */
function moveTo(id: string, column: BoardColumn, index: number, kind: "move" | "done" = "move"): void {
  const card = byId.value.get(id);
  if (!board.value || !card || busy.value[id]) return;
  const giving = column === "claude" && card.column !== "claude";
  // Done now is done today: an earlier day's board goes back to today's, where the card shows.
  if (column === "done" && card.column !== "done" && period.value !== EVERY_DAY) period.value = null;
  const before = toRaw(board.value);
  const at = Date.now();
  const next = giving
    ? moveCard(before, id, column, peers(card, column).length, at).map((c) => (c.id === id && c.session === null ? { ...c, givenAt: at } : c))
    : moveCard(before, id, column, index, at);
  board.value = next;
  void run(
    id,
    giving ? "give" : kind,
    () => (giving ? api.boardGive(id, "continue", terminalSize()) : api.boardMove(id, column, index)),
    () => {
      // Put back unless the core has sent the board since.
      if (toRaw(board.value) === next) board.value = before;
    },
  );
}

/** Its project's other cards in `column`, in the board's order (every one, not just those shown). */
function peers(card: Card, column: BoardColumn): Card[] {
  return (board.value ?? []).filter((c) => c.id !== card.id && c.path === card.path && c.column === column);
}

function moveToEnd(id: string, column: BoardColumn): void {
  const card = byId.value.get(id);
  if (card) moveTo(id, column, column === "done" ? 0 : peers(card, column).length);
}

/** A drop, once the card has slid into the place shown for it: that place among the cards shown,
 * as a place among all the column's cards. The card dropped is selected, the keyboard on it. */
function onDrop(id: string, target: DropTarget): void {
  const card = byId.value.get(id);
  const p = project.value;
  if (!card || !p) return;
  selectedId.value = id;
  // Without scrolling to it: its column may still be growing to hold it.
  void nextTick(() => focusSelected(true));
  const column = target.column as BoardColumn;

  const every = peers(card, column);
  const visible = columnCards(shown.value, p.path, column).filter((c) => c.id !== id);
  const beside = visible[target.index];
  const last = visible[visible.length - 1];
  const index = beside ? every.findIndex((c) => c.id === beside.id) : last ? every.findIndex((c) => c.id === last.id) + 1 : every.length;
  if (index < 0) return;

  if (column === card.column) {
    const from = (board.value ?? []).filter((c) => c.path === card.path && c.column === column).findIndex((c) => c.id === id);
    if (from === index) return;
  }
  moveTo(id, column, index);
}

const drag = useCardDrag(boardBody, onDrop);
const { dragging, ghost, target, settling } = drag;
const draggedCard = computed(() => (dragging.value ? (byId.value.get(dragging.value) ?? null) : null));

// A card picked up is selected then, not as it lands: the buttons of the one selected before go
// while the cards make room anyway, and nothing but the card's own arrival moves at the drop.
// Lifted, it isn't drawn, so the keyboard leaves it: a drag called off (Esc) gives it back once
// the card shows again, as a drop does (`onDrop`).
watch(dragging, (id) => {
  if (id) selectedId.value = id;
  else void nextTick(() => focusSelected(true));
});

/**
 * Brought up. In one of Pitwall's terminals: here, in the card's details, the keyboard in it and
 * its project seen (the board stays on screen). Anywhere else: where its session runs.
 */
function reveal(id: string): void {
  const card = byId.value.get(id);
  if (!card) return;
  const terminal = terminalOf(card);
  const session = card.session;
  if (terminal) {
    selectedId.value = id;
    opened.value = id;
    void api.markSeen(card.path).catch(() => undefined);
    void nextTick(() => focusView(terminal.id));
  } else if (session) void run(id, "reveal", () => api.revealClaude(card.path, session));
  // Its terminal isn't among the project's yet: its details show it once it is.
  else selectedId.value = id;
}

/* ---------- dialogs ---------- */

/** The card dialog: a new card (`card` null) or one being edited. */
const dialog = ref<{ card: Card | null } | null>(null);
const deleting = ref<Card | null>(null);

function openAdd(): void {
  if (projects.value.length) dialog.value = { card: null };
}

function edit(id: string): void {
  const card = byId.value.get(id);
  if (card) dialog.value = { card };
}

function askDelete(id: string): void {
  const card = byId.value.get(id);
  if (card) deleting.value = card;
}

/** A new card is selected, the keyboard on it once the dialog has gone (`handover`); shown at once
 * if the core's board hasn't come with it yet. */
function added(card: Card): void {
  if (board.value && !board.value.some((c) => c.id === card.id)) board.value = [...toRaw(board.value), card];
  selectedId.value = card.id;
  handover.value = { gone: null };
}

/**
 * The keyboard, owed to the selected card when a dialog ends. The dialog gives focus back to
 * whatever had it (lib/dialogFocus): after a card is added that is the card selected before, and
 * after one is deleted (`gone`) its own, which takes the focus along as it leaves the board
 * (before the dialog closes or after, as the core's board comes). So once the dialog has left the
 * page (behind an open one everything is inert) and the deleted card the board, the selected card
 * takes the focus: a new one whatever holds it, the one selected next only if nothing does.
 */
const handover = ref<{ gone: string | null } | null>(null);

watch(
  () => handover.value !== null && !dialog.value && !deleting.value && !(handover.value.gone !== null && byId.value.has(handover.value.gone)),
  (due) => {
    const owed = handover.value;
    if (!due || !owed) return;
    handover.value = null;
    void nextTick(() => {
      if (owed.gone === null || document.activeElement === document.body) focusSelected(true);
    });
  },
);

/** Its Claude session is still open: in the Sessions list and not ended, or only starting, in a
 * Pitwall terminal still open. Deleting the card leaves it running, and the dialog says so. */
function stillRuns(card: Card): boolean {
  const row = rowOf(card);
  return row ? row.phase !== "ended" : card.session === null && terminalOpen(card) === true;
}

/* ---------- menus ---------- */

/** A card's menu. While an action on it is under way it can't be moved (a move then would be
 * dropped); a card of a folder no longer listed is never given, so Claude's column is no place
 * for it to go. */
function cardMenu(at: MenuPoint, id: string): void {
  const card = byId.value.get(id);
  if (!card) return;
  const columns = COLUMNS.filter((column) => column !== card.column && (column !== "claude" || listed.value.has(card.path)));
  void menu.popup(at, [
    { text: t("common.edit"), action: () => edit(id) },
    { text: t("board.move"), enabled: !busy.value[id], items: columns.map((column) => ({ text: t(COLUMN_LABELS[column]), action: () => moveToEnd(id, column) })) },
    "separator",
    { text: t("common.delete"), action: () => askDelete(id) },
  ]);
}

/** The cards of a folder no longer listed go to a project picked from this menu, each in its
 * column; the folder's lane spins meanwhile and says why if the core refuses. */
function rehomeMenu(at: MenuPoint, from: string): void {
  void menu.popup(
    at,
    projects.value.map((p) => ({ text: p.name, action: () => void run(from, "move", () => api.boardRehome(from, p.path)) })),
  );
}

/** "Give to Claude"'s other ways: in plan mode, to a session already open in the project, or
 * just its text to paste (the card stays where it is). */
function giveMenu(at: MenuPoint, id: string): void {
  const card = byId.value.get(id);
  if (!card) return;
  const editor = snapshot.value?.settings.editor;
  const open = (sessions.value?.sessions ?? []).filter((row) => (row.path ?? row.folder) === card.path && row.phase !== "ended" && row.running !== false);
  const linkEntry: MenuEntry = open.length
    ? {
        text: t("board.giveLink"),
        items: open.map((row) => ({ text: `${row.title || t("board.untitledSession")} · ${originView(row.origin, row.project, editor).short}`, action: () => link(id, row.id) })),
      }
    : { text: t("board.giveLink"), enabled: false };
  void menu.popup(at, [
    { text: t("board.giveNew"), accelerator: "CmdOrCtrl+Enter", action: () => give(id, "new") },
    { text: t("board.givePlan"), action: () => give(id, "plan") },
    linkEntry,
    "separator",
    { text: t("board.copyText"), action: () => void api.copyText(cardText(card)) },
  ]);
}

const actions: BoardActions = {
  select: (id) => (selectedId.value = id),
  isClick: () => drag.isClick(),
  edit,
  menu: cardMenu,
  give,
  giveMenu,
  reveal,
  done: (id) => moveTo(id, "done", 0, "done"),
  // Not while an action on it is under way: the move at the drop would be dropped.
  press: (event, id) => {
    if (!busy.value[id]) drag.down(event, id);
  },
};
provide(BOARD_ACTIONS, actions);

/* ---------- keyboard ---------- */

/** It can be given with ⌘↵: queued, or its session closed before finishing; never a card of a
 * folder no longer listed. */
function givable(card: Card): boolean {
  return listed.value.has(card.path) && (card.column === "queued" || statuses.value.get(card.id)?.closed === true);
}

/** ⌘N writes a card, ⌘↵ gives the selected one; not while a dialog is open. */
function onWindowKey(event: KeyboardEvent): void {
  if (event.defaultPrevented || dragging.value || document.querySelector("dialog[open]")) return;
  if (!primary(event) || event.shiftKey || event.altKey) return;
  if (event.code === "KeyN") {
    event.preventDefault();
    openAdd();
  } else if (event.key === "Enter" && !(event.target instanceof HTMLTextAreaElement)) {
    const card = selected.value;
    if (card && givable(card)) {
      event.preventDefault();
      give(card.id, "new");
    }
  }
}

/** The column (0–3) a card sits in. */
function columnOf(el: HTMLElement): number {
  return Number(el.closest<HTMLElement>("[data-colx]")?.dataset.colx ?? 0);
}

/** The arrows go to the next card up or down (its column's; in a list, where the columns sit one
 * under the other, the next column's after its own), or to the nearest one in the next column
 * that has any. */
function go(from: HTMLElement, key: string): void {
  const cards = Array.from(boardBody.value?.querySelectorAll<HTMLElement>("[data-card]") ?? []).filter((el) => el !== from && el.offsetParent !== null);
  const box = from.getBoundingClientRect();
  const middle = box.top + box.height / 2;
  const x = columnOf(from);
  let next: HTMLElement | undefined;

  if (key === "ArrowUp" || key === "ArrowDown") {
    const down = key === "ArrowDown";
    // Above or below it: what shares its place sideways.
    const inLine = (el: HTMLElement): boolean => {
      const r = el.getBoundingClientRect();
      return r.left < box.right && r.right > box.left;
    };
    next = cards
      .filter(inLine)
      .map((el) => ({ el, top: el.getBoundingClientRect().top }))
      .filter((c) => (down ? c.top > box.top : c.top < box.top))
      .sort((a, b) => (down ? a.top - b.top : b.top - a.top))[0]?.el;
  } else {
    const step = key === "ArrowRight" ? 1 : -1;
    const distance = (el: HTMLElement): number => {
      const r = el.getBoundingClientRect();
      return Math.abs(r.top + r.height / 2 - middle);
    };
    for (let col = x + step; !next && col >= 0 && col < COLUMNS.length; col += step) {
      next = cards.filter((el) => columnOf(el) === col).sort((a, b) => distance(a) - distance(b))[0];
    }
  }

  if (!next) return;
  selectedId.value = next.dataset.card ?? null;
  next.focus();
  next.scrollIntoView({ block: "nearest" });
}

/** On a card: arrows move, ↵ edits, Delete asks to delete it, the menu key opens its menu. */
function onBoardKey(event: KeyboardEvent): void {
  const target = event.target as HTMLElement;
  const card = target.closest<HTMLElement>("[data-card]");
  if (!card || target !== card || event.metaKey || event.ctrlKey || event.altKey) return;
  const id = card.dataset.card ?? "";

  switch (event.key) {
    case "ArrowUp":
    case "ArrowDown":
    case "ArrowLeft":
    case "ArrowRight":
      event.preventDefault();
      go(card, event.key);
      return;
    case "Enter":
      event.preventDefault();
      edit(id);
      return;
    case "Delete":
    case "Backspace":
      event.preventDefault();
      askDelete(id);
      return;
    case "ContextMenu":
    case "F10":
      if (event.key === "F10" && !event.shiftKey) return;
      event.preventDefault();
      {
        const box = card.getBoundingClientRect();
        cardMenu({ clientX: box.left + 12, clientY: box.top + 12 }, id);
      }
      return;
  }
}

/* ---------- life ---------- */

onMounted(() => window.addEventListener("keydown", onWindowKey));

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onWindowKey);
  for (const timer of errorTimers.values()) window.clearTimeout(timer);
});

/** Under the pointer; released, it slides to its place. */
const ghostStyle = computed(() =>
  ghost.value
    ? {
        width: `${ghost.value.width}px`,
        transform: `translate(${ghost.value.x}px, ${ghost.value.y}px)`,
        transition: settling.value ? `transform ${SETTLE_MS}ms ease-out` : undefined,
      }
    : undefined,
);
</script>

<template>
  <div class="board-page">
    <main class="board-main">
      <!-- Its heading and free space drag the window; the picker, search and button stay clickable. -->
      <header class="board-bar" :data-tauri-drag-region="dragRegion">
        <div class="board-heading">
          <span class="board-title">{{ t("board.title") }}</span>
          <span class="board-subtitle">{{ subtitle }}</span>
        </div>
        <PeriodPicker v-model="period" />
        <!-- A project's board only: every project's is drawn one way. -->
        <div v-if="project" class="board-views" role="group" :aria-label="t('board.view.label')">
          <button
            v-for="option in LAYOUTS"
            :key="option.id"
            type="button"
            :class="{ on: layout === option.id }"
            :aria-pressed="layout === option.id"
            :aria-label="t(option.label)"
            :title="t(option.label)"
            @click="setLayout(option.id)"
          >
            <Icon :name="option.icon" :size="14" />
          </button>
        </div>
        <label class="board-search">
          <Icon name="search" :size="13" />
          <input v-model="query" type="search" :placeholder="t('board.search')" :aria-label="t('board.search')" spellcheck="false" @keydown.down.prevent="focusSelected()" />
        </label>
        <BoardPicker :projects="projects" :scope="scope" :waits="waits" @pick="show" />
        <button type="button" class="board-add" :title="t('board.addCard')" :disabled="!projects.length" @click="openAdd">
          <Icon name="plus" :size="13" />
          <span class="board-add-text">{{ t("board.addCard") }}</span>
          <span class="board-keys" aria-hidden="true">{{ keys("mod+KeyN") }}</span>
        </button>
      </header>

      <div v-if="hook" class="board-notice">
        <SessionNotice compact icon="link" :tag="t('sessions.noHook.tag')" :title="t('board.noHook.title')">
          {{ t("board.noHook.body") }}
          <template #action>
            <button type="button" class="board-control" @click="emit('openSettings')">{{ hook === "outdated" ? t("board.noHook.update") : t("settings.addHook") }}</button>
          </template>
        </SessionNotice>
      </div>

      <div v-if="snapshot?.boardUnsaved" class="board-notice">
        <SessionNotice compact icon="info" :tag="t('board.unsaved.tag')" :title="t('board.unsaved.title')">{{ t("board.unsaved.body") }}</SessionNotice>
      </div>

      <div v-if="board === null" class="board-loading" role="status">
        <Spinner />
        <span>{{ t("board.loading") }}</span>
      </div>

      <div v-else-if="!projects.length && !unlisted.length" class="board-empty">
        <SessionNotice icon="board" :tag="t('board.empty.tag')" :title="t('board.empty.titleAll')">
          {{ t("board.noProjects") }}
          <template #action>
            <button type="button" class="board-control" @click="api.pickFolder()">{{ t("common.addProject") }}</button>
          </template>
        </SessionNotice>
      </div>

      <div v-else-if="empty" class="board-empty">
        <SessionNotice icon="board" :tag="t('board.empty.tag')" :title="t(project ? 'board.empty.title' : 'board.empty.titleAll')">
          {{ t("board.empty.body") }}
          <template #action>
            <button type="button" class="board-control" @click="openAdd">
              {{ t("board.addCard") }}<span class="board-keys" aria-hidden="true">{{ keys("mod+KeyN") }}</span>
            </button>
          </template>
        </SessionNotice>
      </div>

      <p v-else-if="noMatch" class="board-none">{{ t("board.none.search") }}</p>

      <div v-else ref="boardBody" class="board-body" role="region" :aria-label="t('board.title')" aria-describedby="board-keys" @keydown="onBoardKey">
        <span id="board-keys" class="sr-only">{{ t("board.cardKeys", { give: keys("mod+Enter") }) }}</span>
        <BoardColumns
          v-if="project"
          :class="layout === 'list' ? 'board-list' : 'board-wide'"
          :path="project.path"
          :layout="layout"
          :cards="shown"
          :period="period"
          :selected="selectedId"
          :statuses="statuses"
          :row-of="rowOf"
          :busy="busy"
          :errors="errors"
          :dragging="dragging"
          :target="target"
          :ghost="ghost"
          @add="openAdd"
        />
        <BoardLanes
          v-else
          class="board-wide"
          :lanes="lanes"
          :unlisted="unlistedLanes"
          :can-rehome="projects.length > 0"
          :hidden="hiddenLanes"
          :show-hidden="showHidden"
          :selected="selectedId"
          :statuses="statuses"
          :row-of="rowOf"
          :busy="busy"
          :errors="errors"
          @open="show"
          @toggle-hidden="showHidden = !showHidden"
          @give-next="give($event, 'new')"
          @rehome="rehomeMenu"
        />
      </div>
    </main>

    <!-- The selected card's details: its session's once it has one, with the Pitwall terminal it
         runs in (its note and what Claude last said instead, once its work is over); before that
         the card's own. -->
    <SessionDetail
      v-if="selected && selectedRow"
      :row="selectedRow"
      :now="now"
      :others="others"
      error=""
      :terminal="selectedTerminal"
      :note="selectedOver ? selected.note : ''"
      :images="selectedOver ? selectedImages : []"
      :last-message="lastMessage"
      @mark-seen="markSeen"
      @hold="typing = $event"
    />
    <CardDetail
      v-else-if="selected"
      :card="selected"
      :project="selectedProject"
      :status="statuses.get(selected.id) ?? null"
      :terminal="selectedTerminal"
      :images="selectedImages"
      :last-message="lastMessage"
      @hold="typing = $event"
    />
    <!-- Nothing to show: the panel stays blank but keeps its place, so the layout never jumps. -->
    <section v-else class="board-detail-empty" :aria-label="t('sessions.details')"></section>

    <!-- The dragged card, under the pointer. -->
    <BoardCard
      v-if="ghost && draggedCard"
      class="board-ghost"
      :style="ghostStyle"
      :card="draggedCard"
      :variant="draggedCard.column === 'done' ? 'done' : project && layout === 'list' ? 'row' : 'full'"
      :status="statuses.get(draggedCard.id) ?? null"
      :row="rowOf(draggedCard)"
      ghost
    />

    <CardDialog v-if="dialog" :card="dialog.card" :path="project?.path ?? null" :suggested="props.projectPath" :projects="projects" @added="added" @close="dialog = null" />
    <DeleteDialog v-if="deleting" :card="deleting" :running="stillRuns(deleting)" @deleted="handover = { gone: $event }" @close="deleting = null" />
  </div>
</template>

<style scoped>
.board-page {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  overflow: clip;
}

.board-main {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

/* As wide as the details that take its place. */
.board-detail-empty {
  width: 380px;
  flex-shrink: 0;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
}

/* As tall as the projects toolbar, so switching never moves the title. */
.board-bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
  container-type: inline-size;
}

/* Without room for the days' tabs beside the rest, one button holds them (PeriodPicker), and the
   subtitle gives way well before a control's name is cut; the title keeps its room. */
@container (max-width: 1100px) {
  .board-bar :deep(.period-full) {
    display: none;
  }

  .board-bar :deep(.period-compact) {
    display: inline-flex;
  }

  .board-bar > .board-heading {
    flex-shrink: 1000;
    min-width: 64px;
  }
}

/* Narrower still (the window at its first size): that button's icon alone, and "Add card"
   without its keys. */
@container (max-width: 900px) {
  .board-bar :deep(.compact-text),
  .board-bar :deep(.compact-more),
  .board-add .board-keys {
    display: none;
  }
}

/* Narrowest: "Add card" is its plus alone (still its tooltip, and read out). */
@container (max-width: 520px) {
  .board-add-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

/* The window at its narrowest: the bar has never had room for all of it, and the days' button
   takes none from the rest (the title and the search box give it up). */
@container (max-width: 340px) {
  .board-bar > .board-heading {
    min-width: 0;
  }

  .board-bar > .board-search {
    min-width: 64px;
  }
}

.board-heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.board-title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.board-subtitle {
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.board-add,
.board-control {
  display: inline-flex;
  align-items: center;
  height: 30px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  white-space: nowrap;
}

.board-add:hover:not(:disabled),
.board-control:hover {
  background: #2c3039;
}

.board-add svg {
  flex-shrink: 0;
}

.board-add-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The first to give way when the bar runs out of room: so much sooner than the rest that they
   lose no part of a pixel (which would already cut a label short) until it is at its narrowest. */
.board-search {
  flex-shrink: 10000;
  display: flex;
  align-items: center;
  gap: 7px;
  width: 200px;
  min-width: 110px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text-subtle);
  cursor: text;
}

.board-search svg {
  flex-shrink: 0;
}

.board-search input {
  -webkit-appearance: none;
  appearance: none;
  flex-grow: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.board-search:focus-within {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}

.board-search input::placeholder {
  color: var(--text-faint);
}

.board-search input::-webkit-search-cancel-button {
  cursor: pointer;
}

/* Columns or list: the one shown is lit. */
.board-views {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  height: 30px;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
}

.board-views button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text-subtle);
}

.board-views button:hover {
  color: var(--text);
}

.board-views button.on {
  background: #2c3039;
  color: var(--text-strong);
}

.board-add {
  flex-shrink: 1;
  min-width: 0;
  gap: 8px;
  padding: 0 12px;
  font-size: 13px;
}

.board-add:disabled {
  opacity: 0.4;
}

.board-keys {
  flex-shrink: 0;
  margin-left: 8px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.board-add .board-keys {
  margin-left: 0;
}

.board-control {
  padding: 0 12px;
  font-size: 13px;
}

.board-notice {
  flex-shrink: 0;
  padding: 12px 20px 0;
}

.board-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 48px 0;
  color: var(--text-muted);
}

.board-empty {
  padding: 16px 20px;
}

.board-none {
  margin: 0;
  padding: 40px 12px;
  text-align: center;
  color: var(--text-muted);
}

/* With less room than its columns need to be read (`board-wide`; as a list, `board-list`), the
   board scrolls sideways under a bar that stays put. */
.board-body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow-x: auto;
  overflow-y: hidden;
}

.board-wide {
  min-width: 920px;
}

.board-list {
  min-width: 520px;
}

.board-ghost {
  position: fixed;
  left: 0;
  top: 0;
  z-index: 50;
}
</style>
