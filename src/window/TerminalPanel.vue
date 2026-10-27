<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import ProjectPicker from "../components/ProjectPicker.vue";
import { accountLabel, accountName, accounts, severalAccounts } from "../lib/accounts";
import { depJobName, depJobs } from "../lib/deps";
import { commandFaded } from "../lib/format";
import { shellRequest } from "../lib/flag";
import { t } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import { plainText, shownLines } from "../lib/outputLines";
import { useOutputSearch } from "../lib/outputSearch";
import { useOutputStream, type OutputLine } from "../lib/outputStream";
import { openOrAtWork } from "../lib/pages";
import {
  chooseTab,
  chosenTab,
  foldPanel,
  followedTab,
  COLLAPSED_HEIGHT,
  COLLAPSE_MS,
  HISTORY_MIN,
  HISTORY_WIDTH,
  historyWidth,
  openRequest,
  outputErrorsOnly,
  panelAsks,
  outputFrom,
  panelFocus,
  panelMeasure,
  panelOpening,
  panelOver,
  panelPath,
  panelTyping,
  TAB_CHROME,
  TAB_ROOM,
  terminalCollapsed,
  terminalHeight,
  terminalHistoryShown,
  terminalOrder,
  terminalRequest,
  useResizer,
  PANEL_MIN,
} from "../lib/panel";
import {
  canAddPane,
  canSplit,
  claudeTerminals,
  evenAll,
  findLeaf,
  kindOf,
  leaf,
  leafOf,
  leaves,
  minSize,
  movePane,
  paneAction,
  paneLayouts,
  PANES_CHROME,
  PANES_HEIGHT,
  panesHeight,
  place,
  preset,
  presetOf,
  PRESETS,
  removePane,
  setPanes,
  setTerminal,
  splitPane,
  swapPanes,
  takeCoreLayouts,
  type PaneAction,
  type PaneKind,
  type PaneLeaf,
  type PaneNode,
  type Place,
  type Placed,
  type PlacedPane,
  type PresetId,
  type Side,
  type SplitDir,
} from "../lib/panes";
import { isMac, isWindows, primary } from "../lib/platform";
import { sessionWaits, statusLine } from "../lib/sessions";
import { SHELL_LABELS, shellIcon } from "../lib/shells";
import { MODEL_LABELS, MODELS, projectChoice } from "../lib/start";
import { api, now, sessions, snapshot, visible } from "../lib/store";
import { distinctNames, tabKey } from "../lib/tabs";
import { useTabReorder } from "../lib/tabReorder";
import { attach, fitView, focusView, measure, paneKeys, terminalTip } from "../lib/terminals";
import type { Project, Shell, ShellChoice, StartChoice } from "../lib/types";
import CardDialog from "./board/CardDialog.vue";
import OutputLog from "./detail/OutputLog.vue";
import OutputSearch from "./detail/OutputSearch.vue";
import LayoutMenu from "./LayoutMenu.vue";
import RenameDialog from "./RenameDialog.vue";
import TerminalHistory from "./TerminalHistory.vue";
import TerminalPanes from "./TerminalPanes.vue";

/** How long a `reveal-terminal` request waits for its project and tab to show up. */
const REQUEST_MS = 5_000;

/**
 * The panel along the window's bottom, under every page (WindowApp): one project's outputs (the
 * dev server, the commands that ran, the Garage page's jobs) and its terminals, as tabs in one
 * row. Its project is the one the page shown follows (lib/panel: `followProject`), or the one
 * picked at the row's start. One height and one collapsed state for every page (lib/panel).
 */
const props = defineProps<{ project: Project }>();

/** Room above the panel that always stays: the page's bar. */
const HEADER = 56;
/** One height for one terminal at a time, another for panes (lib/panes). */
const height = computed({
  get: () => (paneLayouts.value[props.project.path] ? panesHeight.value : terminalHeight.value),
  set: (value: number) => {
    if (paneLayouts.value[props.project.path]) panesHeight.value = value;
    else terminalHeight.value = value;
  },
});
const collapsed = terminalCollapsed;

const panel = ref<HTMLElement | null>(null);
const body = ref<HTMLElement | null>(null);
const viewport = ref<HTMLElement | null>(null);
const opening = panelOpening;
/** The tab being renamed: double-click opens the rename dialog for it. */
const renaming = ref<{ id: number; name: string } | null>(null);

/** The project's terminals, in the order their tabs were dragged into (lib/panel), new ones last. */
const terminals = computed(() => {
  const list = props.project.terminals;
  const order = terminalOrder.value[props.project.path];
  if (!order) return list;
  const rank = new Map(order.map((id, i) => [id, i]));
  const unranked = Number.MAX_SAFE_INTEGER;
  return [...list].sort((a, b) => (rank.get(a.id) ?? unranked) - (rank.get(b.id) ?? unranked));
});

/** The project's panes (lib/panes), or none: one terminal at a time. */
const layout = computed(() => paneLayouts.value[props.project.path] ?? null);
const panesOn = computed(() => layout.value !== null);
/** The panes' room, as they last measured it. */
const paneRoom = ref({ w: 0, h: 0 });

/** The tabs the row shows: every terminal, or with panes those in none of them, waiting there to
 * be clicked or dragged into one. */
const rowTabs = computed(() => {
  const now = layout.value;
  if (!now) return terminals.value;
  const held = new Set(leaves(now.tree).map((pane) => pane.terminal));
  return terminals.value.filter((term) => !held.has(term.id));
});

/** The names the row and the panes show: two of the same name told apart ("zsh", "zsh 2"). */
const names = computed(() => distinctNames(terminals.value));

// A Claude session's pane is larger at least (lib/panes): which of these terminals are Claude's.
watch(
  terminals,
  (list) => {
    claudeTerminals.value = new Set(list.filter((term) => term.kind === "claude").map((term) => term.id));
  },
  { immediate: true },
);

// The panel's height for panes as it was last left: taken once at the start, and kept for the next
// one a moment after it changes (dragged, or grown to hold its panes).
let heightTaken = false;
let heightKept: ReturnType<typeof setTimeout> | undefined;
watch(
  () => snapshot.value?.settings.panesHeight,
  (kept) => {
    if (heightTaken || !snapshot.value) return;
    heightTaken = true;
    if (kept) panesHeight.value = kept;
  },
  { immediate: true },
);
watch(panesHeight, (tall) => {
  if (!heightTaken) return;
  clearTimeout(heightKept);
  heightKept = setTimeout(() => void api.setPanesHeight(Math.round(tall)).catch(() => undefined), 600);
});

// Panes the core laid itself (put back after a quit or an update) are taken as they come.
watch(
  () => snapshot.value?.projects,
  (projects) => {
    if (projects) takeCoreLayouts(projects);
  },
  { immediate: true },
);

/** Claude in a tab's terminal, by the terminal's id: at work, or waiting on you (a turn finished
 * and not looked at yet, a question, a permission prompt). Nothing for a tab with neither. */
const live = computed(() => {
  const states = new Map<number, { phase: "waiting" | "working"; text: string }>();
  for (const row of sessions.value?.sessions ?? []) {
    if (row.origin.kind !== "pitwall" || (row.phase !== "waiting" && row.phase !== "working")) continue;
    // Two in one terminal (one left running in the background): the one that waits on you tells.
    if (row.phase === "working" && states.get(row.origin.terminal)?.phase === "waiting") continue;
    states.set(row.origin.terminal, { phase: row.phase, text: statusLine(row, now.value).text });
  }
  return states;
});
/** The tab chosen in this project (lib/panel), if any. */
const chosen = computed(() => chosenTab.value[props.project.path]);
/** The terminal shown: the one chosen, else (one chosen that closed, or nothing chosen) the last;
 * with panes, the focused pane's. None while an output is chosen. */
const active = computed(() => {
  const choice = chosen.value;
  if (choice?.kind === "output") return null;
  const now = layout.value;
  if (now) {
    const id = findLeaf(now.tree, now.focus)?.terminal;
    return terminals.value.find((t) => t.id === id) ?? null;
  }
  return terminals.value.find((t) => t.id === choice?.id) ?? terminals.value[terminals.value.length - 1] ?? null;
});

const resizer = useResizer(
  height,
  () => (panel.value?.parentElement?.clientHeight ?? 0) - HEADER,
  // With panes, never lower than they can be.
  () => (layout.value ? minSize(layout.value.tree).h + PANES_CHROME : PANEL_MIN),
);
const dragging = resizer.dragging;

/** Collapsed, only the tab row shows; the chosen height comes back on expanding. */
const shownHeight = computed(() => (collapsed.value ? COLLAPSED_HEIGHT : height.value));
/** While the panel animates, the shell keeps its size: no resize per frame, no squeeze. */
const animating = ref(false);
let settle: ReturnType<typeof setTimeout> | undefined;

function toggleCollapsed(): void {
  collapsed.value = !collapsed.value;
}

// Collapsed or expanded, from here or from a page (lib/panel: `foldPanel`, an output asked for):
// the panel slides with its shell at the size it had, and a terminal or an output going out of
// sight lets the keyboard go.
watch(
  collapsed,
  (folded) => {
    if (folded && body.value?.contains(document.activeElement)) (document.activeElement as HTMLElement).blur();
    animating.value = true;
    clearTimeout(settle);
    settle = setTimeout(() => {
      animating.value = false;
      if (!collapsed.value && active.value && !panesOn.value) fitView(active.value.id);
    }, COLLAPSE_MS + 40);
  },
  { flush: "sync" },
);

/* ---------- the outputs ---------- */

/** The outputs' tabs: the dev server, then every command that ran in this session, then the Garage
 * page's installs and migrations that did (lib/deps) and the core doesn't list as commands. Each
 * with its state: running, or failed (a crashed server). */
const outputs = computed(() => {
  const project = props.project;
  const server = { id: null as string | null, name: t("common.devServer"), state: project.status === "running" ? "on" : project.status === "crashed" ? "bad" : null };
  // A run that ended well a while ago leaves the row, unless its output is the one picked here.
  const picked = chosen.value?.kind === "output" ? chosen.value.job : undefined;
  const commands = (project.commands ?? [])
    .filter((c) => (c.status !== "idle" || c.result) && (!commandFaded(c, now.value) || c.id === picked))
    .map((c) => ({ id: c.id as string | null, name: c.name, state: c.status === "running" ? "on" : c.status === "failed" ? "bad" : null }));
  const listed = new Set((project.commands ?? []).map((c) => c.id));
  const jobs = (depJobs.value[project.path] ?? [])
    .filter((job) => !listed.has(job))
    .map((job) => ({ id: job as string | null, name: depJobName(project.path, job) ?? job, state: null }));
  return [server, ...commands, ...jobs];
});
/** The output picked: the one chosen while it is still a tab, else the dev server's. */
const job = computed<string | null>(() => {
  const choice = chosen.value;
  return choice?.kind === "output" && outputs.value.some((item) => item.id === choice.job) ? choice.job : null;
});
/**
 * An output shows in the place of the terminal only once it was picked (its tab, a command run, an
 * output asked for); never by itself, a project with no terminal included: that one shows the
 * empty terminal (the user's word, 2026-10-22).
 */
const showsOutput = computed(() => chosen.value?.kind === "output");

// Nothing of an output is read or followed while none shows: its lines are asked for when it does.
const { lines, runStart, load: loadOutput } = useOutputStream(
  () => (showsOutput.value ? props.project.path : ""),
  job,
);
watch(
  [() => props.project.path, job, showsOutput],
  ([, , shows]) => {
    if (shows) void loadOutput();
  },
  { immediate: true },
);

/**
 * An output's tab asked for: shown, and a collapsed panel comes up with it. The one shown, picked
 * again, gives its place back: to the terminal it took it from (lib/panel: `outputFrom`), else the
 * project's last terminal, the keyboard in it; with no terminal, to the empty terminal.
 */
function openOutput(id: string | null): void {
  if (showsOutput.value && job.value === id && !collapsed.value) return closeOutput();
  // From a terminal (one picked, or the last shown for want of one): that is where it goes back to.
  if (active.value) outputFrom[props.project.path] = active.value.id;
  chooseTab(props.project.path, { kind: "output", job: id });
  if (collapsed.value) toggleCollapsed();
}

function closeOutput(): void {
  const list = terminals.value;
  if (!list.length) {
    chooseTab(props.project.path, { kind: "terminal", id: -1 });
    return;
  }
  const from = outputFrom[props.project.path];
  const back = list.find((term) => term.id === from) ?? list[list.length - 1];
  show(back.id);
}

/** ←/→, Home and End move between the outputs' tabs. */
function onOutputKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, outputs.value.length, at);
  if (next !== null) openOutput(outputs.value[next].id);
}

/** The output shown: a project's server, or one of its commands. Each has its own filter. */
const pane = computed(() => `${props.project.path}\u001f${job.value ?? ""}`);
const errorsOnly = computed({
  get: () => outputErrorsOnly.value[pane.value] ?? false,
  set: (on) => (outputErrorsOnly.value = { ...outputErrorsOnly.value, [pane.value]: on }),
});
/** The errors of the run that is on: those of a run before are no longer the server's state. */
const errorCount = computed(() => lines.value.reduce((count, line) => count + (line.level === "error" && line.id >= runStart.value ? 1 : 0), 0));
const shownOutput = computed(() => shownLines(lines.value, errorsOnly.value));
/** The output picked can be seen: the panel open, the window on screen. */
const outputSeen = computed(() => showsOutput.value && !collapsed.value && visible.value);
/** The lines its pane draws: while it can't be seen, they are followed and not drawn (no rows made,
 * no layout), and what came meanwhile is drawn at once when it can. */
const drawnOutput = shallowRef<OutputLine[]>([]);
watch(
  [shownOutput, outputSeen],
  ([shown, seen]) => {
    if (seen) drawnOutput.value = shown;
  },
  { immediate: true },
);
const emptyOutput = computed(() => {
  if (lines.value.length) return t("output.noErrors");
  if (job.value !== null) return t("detail.noOutput");
  return t(props.project.owner ? "detail.noServerOutput" : "detail.notRunning");
});

const log = ref<InstanceType<typeof OutputLog> | null>(null);
const { open: searching, query, ranges, count: matches, position, current, step, close: closeFind } = useOutputSearch(shownOutput);
const field = ref<InstanceType<typeof OutputSearch> | null>(null);
/** What had the keyboard when the search opened: closing gives it back. */
let before: HTMLElement | null = null;

async function openSearch(): Promise<void> {
  if (!searching.value) before = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  if (collapsed.value) toggleCollapsed();
  searching.value = true;
  await nextTick();
  field.value?.focus();
}

/** The marks go with the query; the keyboard goes back where it was, else to the lines. */
function closeSearch(): void {
  closeFind();
  const back = before;
  before = null;
  if (back?.isConnected) back.focus({ preventScroll: true });
  else log.value?.focus();
}

/** ⌘F while the keyboard is in the output (its lines take it with a click), and Esc there closes
 * the search: from its own buttons as from its field, else the keyboard stays. */
function onOutputKeys(event: KeyboardEvent): void {
  if (event.key === "Escape" && searching.value) {
    event.preventDefault();
    event.stopPropagation();
    if (event.target instanceof Node && field.value?.$el.contains(event.target)) return closeSearch();
    closeFind();
    before = null;
    return;
  }
  if (!primary(event) || event.shiftKey || event.altKey || event.key.toLowerCase() !== "f") return;
  event.preventDefault();
  void openSearch();
}

// Collapsed, or on another output or a terminal, there is no line to search in.
watch([collapsed, pane, showsOutput], () => closeFind());

const copied = ref(false);
let copiedTimer = 0;

/** What the output shows, as text. */
function copyAll(): void {
  void api.copyText(plainText(shownOutput.value));
  copied.value = true;
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = false), 1200);
}

/** An error of the output on its way to the board: the card dialog, filled in with it. */
const errorCard = ref<{ title: string; note: string; path: string } | null>(null);
const cardProjects = computed(() => [{ path: props.project.path, name: props.project.name }]);

/* ---------- the project ---------- */

/** What the picker lists, as Settings › Projects says: every project, or the favourites and the
 * projects open or at work (lib/pages), the default; the panel's own always. */
const projects = computed(() => {
  const all = snapshot.value?.projects ?? [];
  if (snapshot.value?.settings.terminalProjects === "all") return all;
  return all.filter((p) => p.path === props.project.path || p.favourite || openOrAtWork(p));
});
/** What Claude waits on you with in each project: the picker marks the others with Claude's dot. */
const waits = computed(() => sessionWaits(sessions.value?.sessions ?? []));

function pickProject(path: string | null): void {
  if (path) panelPath.value = path;
}

/** The handle doesn't drag a collapsed panel; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!collapsed.value) resizer.start(event);
}

/* ---------- the history ---------- */

/** Below this width the panel has no room for the history beside the terminal: it lies over the
 * terminal's right side instead, and goes once a session is picked. */
const HISTORY_ROOM = 760;
const narrow = ref(false);
/** The narrow panel's history, over the terminal. */
const overlayOpen = ref(false);
/** Shown, as "History" says it: never while the panel is collapsed. The column beside the terminal
 * is kept through a collapse and comes back with the panel; the narrow panel's goes with it. */
const historyOpen = computed(() => !collapsed.value && (narrow.value ? overlayOpen.value : terminalHistoryShown.value));
/** The history beside the terminal; `sliding`: its room is still opening, closing or being
 * resized. */
const beside = computed(() => terminalHistoryShown.value && !narrow.value && !collapsed.value);
const sliding = ref(false);
/** A few frames have passed since the panel came: until then the history takes its place (the
 * panel's width, measured) without moving, so coming to the page shows it as it was. */
const settled = ref(false);
/** What the terminal keeps beside the history, however wide that is dragged; and the margins
 * around the two (20 left, 12 between, 20 right). */
const TERMINAL_ROOM = 320;
const MARGINS = 52;
const panelWidth = ref(0);
/** The history's width as the panel allows it: what it was dragged to, the terminal's room kept. */
const widest = computed(() => Math.max(HISTORY_MIN, panelWidth.value - TERMINAL_ROOM - MARGINS));
const historyShown = computed(() => Math.min(historyWidth.value, widest.value));
/** The room opens in this long; closing, it waits as long again for the list to fade (CSS). */
const SLIDE_MS = COLLAPSE_MS + 120;
let slid: ReturnType<typeof setTimeout> | undefined;
const side = ref<InstanceType<typeof TerminalHistory> | null>(null);
const over = ref<InstanceType<typeof TerminalHistory> | null>(null);

// While the history's room opens or closes, the terminal follows its width frame by frame; the
// shell is told the size it ends at.
watch([beside, historyShown], () => {
  if (!settled.value) return;
  sliding.value = true;
  clearTimeout(slid);
  slid = setTimeout(() => {
    sliding.value = false;
    if (!collapsed.value && !animating.value && active.value && !panesOn.value) fitView(active.value.id);
  }, SLIDE_MS + 40);
});

/** "History": the column beside the terminal, or a narrow panel's over it; shown, the keyboard
 * goes to its search, and a collapsed panel comes up with it. Hidden, back to the terminal. */
async function toggleHistory(): Promise<void> {
  const open = !historyOpen.value;
  // The panel comes up first: coming up, it would put away a history over the terminal.
  if (open && collapsed.value) {
    toggleCollapsed();
    await nextTick();
  }
  if (narrow.value) overlayOpen.value = open;
  else terminalHistoryShown.value = open;
  if (!open) return backToTerminal();
  await nextTick();
  (narrow.value ? over : side).value?.focus();
}

function closeOverlay(): void {
  overlayOpen.value = false;
  backToTerminal();
}

function backToTerminal(): void {
  if (active.value) focusView(active.value.id);
}

/* The history's width: its handle, the gap between it and the terminal, drags it (the pointer
   moving left widens it); ←/→ step it; a double click puts the default back. */
let widthDrag: { x: number; width: number } | null = null;
const resizing = ref(false);

function setWidth(width: number): void {
  historyWidth.value = Math.round(Math.max(HISTORY_MIN, Math.min(widest.value, width)));
}

function startWidth(event: PointerEvent): void {
  widthDrag = { x: event.clientX, width: historyShown.value };
  resizing.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}

function moveWidth(event: PointerEvent): void {
  if (widthDrag) setWidth(widthDrag.width + widthDrag.x - event.clientX);
}

function endWidth(): void {
  widthDrag = null;
  resizing.value = false;
}

function nudgeWidth(event: KeyboardEvent): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  setWidth(historyShown.value + (event.key === "ArrowLeft" ? 24 : -24));
}

function resetWidth(): void {
  historyWidth.value = HISTORY_WIDTH;
}

/** A press on the terminal puts away the history lying over it. */
function pressScreen(): void {
  seeFinished(active.value?.id);
  overlayOpen.value = false;
}

/**
 * What the panel says under its terminals after an administrator's shell was asked for (Windows):
 * it opened in a window of its own, sudo not running inline (with a link to Windows' setting), or
 * Windows refused it. It stays until it is closed, another takes its place or the panel turns to
 * another project.
 */
const notice = ref<"window" | "denied" | null>(null);
watch(
  () => props.project.path,
  () => (notice.value = null),
);
/** Administrator's tabs the last quit or update had open, which nothing put back: said once, after
 * the one above. */
const notRestored = computed(() => snapshot.value?.adminNotRestored ?? 0);
const shownNotice = computed(() => notice.value ?? (notRestored.value ? "restored" : null));

function closeNotice(): void {
  if (notice.value) notice.value = null;
  else void api.adminNotRestoredSeen().catch(() => undefined);
}

function openSudoSettings(): void {
  void api.openSudoSettings().catch(() => undefined);
}

/** A new tab: the shell, or with `claude` a Claude Code session (`start`: with the model and in
 * the mode picked from its list; without one, as the project's settings say). With panes, or
 * `where` asked for, it goes into a pane (`placeTerminal`). `shell`: on Windows, the shell of the
 * list picked, an administrator's or not; an administrator's that opened in a window of its own,
 * or that Windows refused, opens no tab and the panel says so (`notice`). `account`: the Claude
 * account it runs with ("Start with account"); without one, the account in use. */
async function openTerminal(claude = false, start: StartChoice | null = null, where: Place | null = null, shell: ShellChoice | null = null, account: string | null = null): Promise<void> {
  if (opening.value) return;
  opening.value = true;
  try {
    // A new terminal is meant to be seen, and its shell starts at the open panel's size.
    if (collapsed.value) {
      toggleCollapsed();
      await new Promise((resolve) => setTimeout(resolve, COLLAPSE_MS + 60));
    }
    const size = viewport.value && !panesOn.value && !where ? measure(viewport.value) : null;
    const opened = await api.openTerminal(props.project.path, claude, size, start, shell, account);
    if (opened.kind !== "tab") {
      notice.value = opened.kind;
      return;
    }
    const view = opened.view;
    asked = view.id;
    // The core lists a new terminal before it answers: one listed already is waited for no
    // longer, and its pane goes as soon as it closes.
    if (!terminals.value.some((term) => term.id === view.id)) fresh.add(view.id);
    if (where || layout.value) placeTerminal(view.id, where ?? { kind: "auto" }, claude ? "claude" : "shell");
    chooseTab(props.project.path, { kind: "terminal", id: view.id });
  } finally {
    opening.value = false;
  }
}

function show(id: number): void {
  asked = id;
  chooseTab(props.project.path, { kind: "terminal", id });
}

/**
 * What Claude finished in a tab counts as seen once the tab is looked at: picked (a click, the
 * keyboard) or pressed in, the panel open. Every session of its project stops waiting on you, as
 * on a board picked by hand. Not while one asks something there (a question, a permission
 * prompt): seen, it would no longer say so.
 */
function seeFinished(id: number | undefined): void {
  if (id === undefined || collapsed.value) return;
  const rows = sessions.value?.sessions ?? [];
  const finished = rows.find((row) => row.origin.kind === "pitwall" && row.origin.terminal === id && row.phase === "waiting" && row.turn?.kind === "finished");
  if (!finished) return;
  const path = finished.path ?? finished.folder;
  const asks = rows.some((row) => (row.path ?? row.folder) === path && row.phase === "waiting" && !!row.turn && row.turn.kind !== "finished");
  if (!asks) void api.markSeen(path).catch(() => undefined);
}

/**
 * A tab that came by itself while this panel was up: one the core opened at a board card's
 * moment (`unasked`). The tab or the output in sight stays in sight and the keyboard stays where
 * it is: whoever is typing here goes on typing where they were. With only the empty terminal in
 * sight, the one that came shows, without the keyboard. A tab the user asked for (put back, a card
 * given by hand) is shown as any other.
 */
watch(
  () => [props.project.path, terminals.value] as const,
  ([path, list], [wasPath, was]) => {
    // With panes, one that came by itself waits in no pane.
    if (path !== wasPath || layout.value) return;
    const came = list.filter((term) => !was.some((old) => old.id === term.id));
    // One this panel is opening is asked for, and so is any the core doesn't call unasked.
    if (!came.length || opening.value || !came.every((term) => term.unasked)) return;
    const choice = chosenTab.value[path];
    if (choice?.kind === "output") return;
    if (came.some((term) => term.id === choice?.id)) return;
    const shown = was.find((term) => term.id === choice?.id) ?? was[was.length - 1];
    if (!shown) {
      const first = came[came.length - 1];
      followedTab.value = first.id;
      chooseTab(path, { kind: "terminal", id: first.id });
    } else if (shown.id !== choice?.id && list.some((term) => term.id === shown.id)) chooseTab(path, { kind: "terminal", id: shown.id });
  },
  // Before the shown tab is worked out again.
  { flush: "sync" },
);

/**
 * The tab asked for here, until it shows: clicked, picked, opened from "+" or brought up
 * (`terminalRequest`). Only that one takes the keyboard, and the one a project picked in the list
 * shows (lib/panel: `panelFocus`); a tab a page shows for its own selection (`followProject`,
 * `followTab`) comes up without it, so the page keeps its arrow keys.
 */
let asked: number | null = null;

/* ---------- "+": a new tab, or a Claude session ---------- */

/** How "New Claude" starts a session here when nothing is picked: the project's own settings. */
const defaults = computed(() => projectChoice(props.project));

const menu = useNativeMenu();

/** The shells installed here, Windows' alone (none elsewhere): asked of the core once. */
const shells = ref<Shell[]>([]);
if (isWindows) {
  void api
    .terminalShells()
    .then((found) => (shells.value = found.shells))
    .catch(() => undefined);
}

/** "+"'s submenu on Windows: each shell installed, then each as an administrator's. */
function shellItems(): MenuEntry[] {
  const open = (shell: Shell, elevated: boolean) => () => void openTerminal(false, null, null, { shell, elevated });
  return [
    ...shells.value.map((shell) => ({ text: t(SHELL_LABELS[shell]), action: open(shell, false) })),
    "separator",
    ...shells.value.map((shell) => ({ text: t("terminal.admin", { shell: t(SHELL_LABELS[shell]) }), action: open(shell, true) })),
  ];
}

/** The "+" menu, under it: a Claude session as the project's settings say, a shell tab, on
 * Windows in a submenu the shells (and each as an administrator's), and in a submenu the models
 * (the project's own checked) and plan mode, each starting a session at once with that one
 * change, the rest as the project's settings say. With more than one Claude account, in a
 * submenu the accounts (the one in use checked): a session with that one. */
function addMenu(event: Event): void {
  if (opening.value) return;
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const { model, mode } = defaults.value;
  void menu.popup({ clientX: box.left, clientY: box.bottom + 4 }, [
    { text: t("terminal.newClaude"), action: () => void openTerminal(true) },
    { text: t("terminal.newTab"), accelerator: isMac ? "Cmd+T" : "Ctrl+Shift+T", action: () => void openTerminal() },
    "separator",
    ...(shells.value.length ? [{ text: t("terminal.openWith"), items: shellItems() }] : []),
    {
      text: t("terminal.claudeWith"),
      items: [
        ...MODELS.map((each) => ({ text: t(MODEL_LABELS[each]), checked: model === each, action: () => void openTerminal(true, { model: each, mode }) })),
        "separator",
        { text: t("start.newClaudePlan"), checked: mode === "plan", action: () => void openTerminal(true, { model, mode: "plan" }) },
      ],
    },
    ...(severalAccounts.value ? [{ text: t("terminal.accountWith"), items: accountItems() }] : []),
  ]);
}

/** "+"'s "Start with account": every account, the one in use checked; one whose folder is gone
 * can't be picked. */
function accountItems(): MenuEntry[] {
  return accounts.value.map((account) => ({
    text: accountName(account),
    checked: account.inUse,
    enabled: !account.missing,
    action: () => void openTerminal(true, null, null, null, account.id),
  }));
}

/* ---------- the panes ---------- */

/** Terminals just opened that the core's list doesn't show yet: their panes wait for them. */
const fresh = new Set<number>();

/** The stage's width: the panel's, less its margins and the history beside it. */
const stageWidth = computed(() => Math.max(0, panelWidth.value - 20 - (beside.value ? historyShown.value + 32 : 20)));

/** The room panes would have with the panel `tall` px high. */
function roomAt(tall: number): { w: number; h: number } {
  return { w: stageWidth.value, h: tall - PANES_CHROME };
}

/** How tall the panel can get: up to the page's bar. */
function tallest(): number {
  return (panel.value?.parentElement?.clientHeight ?? window.innerHeight) - HEADER;
}

/** Where the panes are now, as shown. */
function placedNow(): Placed | null {
  const now = layout.value;
  if (!now) return null;
  return panesView.value?.placed ?? place(now.tree, paneRoom.value.w, paneRoom.value.h);
}

/** The panes show as they are (not dealt out or squeezed for want of room, none zoomed): only then
 * is a pane split by the keys or by a terminal coming. */
function panesAsIs(): boolean {
  return !layout.value?.zoom && (panesView.value?.shown.mode ?? "as-is") === "as-is";
}

/** What a terminal is, for the room its pane needs: a Claude session's, or a shell's. */
function kindOfTerminal(id: number | undefined): PaneKind {
  return id !== undefined && claudeTerminals.value.has(id) ? "claude" : "shell";
}

/**
 * The panes changed: kept, the keyboard's pane `focus` and the maximized one `zoom` while it is
 * there; with fewer than two left, one terminal at a time again, the one left shown.
 */
function commitTree(tree: PaneNode | null, focus: string, zoom = layout.value?.zoom ?? null): void {
  const path = props.project.path;
  const panes = tree ? leaves(tree) : [];
  if (!tree || panes.length < 2) {
    setPanes(path, null);
    const left = panes[0]?.terminal ?? null;
    if (left !== null) chooseTab(path, { kind: "terminal", id: left });
    return;
  }
  const focused = panes.find((pane) => pane.key === focus) ?? panes[0];
  setPanes(path, { tree, focus: focused.key, zoom: panes.some((pane) => pane.key === zoom) ? zoom : null, rev: layout.value?.rev ?? 0 });
  if (focused.terminal !== null) chooseTab(path, { kind: "terminal", id: focused.terminal });
}

/** One of the last two panes left empty: the keyboard goes to the other's terminal, and it is
 * maximized no longer. The panes stay as they are for the next terminal. */
function emptied(tree: PaneNode, key: string, focus: string, zoom: string | null): { tree: PaneNode; focus: string; zoom: string | null } {
  const next = setTerminal(tree, key, null);
  return { tree: next, focus: focus === key ? (leaves(next).find((pane) => pane.terminal !== null)?.key ?? focus) : focus, zoom: zoom === key ? null : zoom };
}

/** A pane taken out of the panes: its terminal, if it has one, stays open and waits. Of the last
 * two, one with a terminal stays, empty, as a closed terminal's does; an empty one goes. */
function takeOut(key: string): void {
  const now = layout.value;
  if (!now) return;
  if (leaves(now.tree).length <= 2 && (findLeaf(now.tree, key)?.terminal ?? null) !== null) {
    const out = emptied(now.tree, key, now.focus, now.zoom);
    return commitTree(out.tree, out.focus, out.zoom);
  }
  const { tree, neighbour } = removePane(now.tree, key);
  commitTree(tree, now.focus === key ? (neighbour ?? "") : now.focus);
}

/** A pane picked (pressed, the keys); `keyboard`: its terminal takes the keyboard. */
function focusPane(key: string, keyboard: boolean): void {
  const now = layout.value;
  if (!now) return;
  if (now.focus !== key) setPanes(props.project.path, { ...now, focus: key });
  const id = findLeaf(now.tree, key)?.terminal ?? null;
  if (id === null) return;
  const same = active.value?.id === id;
  if (keyboard) asked = id;
  chooseTab(props.project.path, { kind: "terminal", id });
  seeFinished(id);
  if (keyboard && same) focusView(id);
}

/** The empty pane a terminal coming goes into: the focused one if it is empty, else the first. */
function emptyPane(tree: PaneNode, focus: string): PaneLeaf | undefined {
  const panes = leaves(tree).filter((pane) => pane.terminal === null);
  return panes.find((pane) => pane.key === focus) ?? panes[0];
}

/**
 * A new terminal into the panes: beside a pane, into an empty one, or where there is room (an
 * empty pane, else beside the focused one, to the right, then below, else in its place, the one
 * there waiting). With one terminal at a time, a split makes panes of the one shown and the new one.
 */
function placeTerminal(id: number, where: Place, kind: PaneKind = kindOfTerminal(id)): void {
  const path = props.project.path;
  const now = layout.value;
  const added = leaf(id);
  if (!now) {
    const shown = active.value;
    if (where.kind !== "split" || !shown) return;
    const first = leaf(shown.id);
    setPanes(path, { tree: splitPane(first, first.key, where.dir, added, where.before), focus: added.key, zoom: null, rev: 0 });
    return;
  }
  let tree: PaneNode;
  let focus = added.key;
  // With as many panes as there may be, a split asked for finds it a place as `auto` does.
  const more = canAddPane(now.tree);
  if (where.kind === "split" && more) tree = splitPane(now.tree, where.key, where.dir, added, where.before);
  else if (where.kind === "cell") {
    tree = setTerminal(now.tree, where.key, id);
    focus = where.key;
  } else {
    const empty = emptyPane(now.tree, now.focus);
    const rect = panesAsIs() ? placedNow()?.panes.find((pane) => pane.leaf.key === now.focus) : undefined;
    const kinds: [PaneKind, PaneKind] = [rect ? kindOf(rect.leaf) : "shell", kind];
    if (empty) {
      tree = setTerminal(now.tree, empty.key, id);
      focus = empty.key;
    } else if (rect && more && canSplit(rect, "row", kinds)) tree = splitPane(now.tree, now.focus, "row", added);
    else if (rect && more && canSplit(rect, "col", kinds)) tree = splitPane(now.tree, now.focus, "col", added);
    else {
      tree = setTerminal(now.tree, now.focus, id);
      focus = now.focus;
    }
  }
  setPanes(path, { ...now, tree, focus, zoom: null });
}

/** An open terminal in no pane, brought into the pane `into` (its terminal, if it has one, then
 * waits). It takes the keyboard. */
function bring(id: number, into: string): void {
  const now = layout.value;
  if (!now) return;
  asked = id;
  setPanes(props.project.path, { ...now, tree: setTerminal(now.tree, into, id), focus: into, zoom: null });
  chooseTab(props.project.path, { kind: "terminal", id });
}

// A terminal that closed leaves its pane, the room going to its neighbour; so does one put into a
// pane once it had closed. Of the last two panes, its pane stays, empty: the panes are left as
// they are for the next terminal, never back to one terminal at a time.
watch(
  () => [props.project.path, terminals.value.map((term) => term.id).join(), layout.value ? leaves(layout.value.tree).map((pane) => pane.terminal).join() : ""] as const,
  () => {
    const now = layout.value;
    const alive = new Set(terminals.value.map((term) => term.id));
    for (const id of fresh) if (alive.has(id)) fresh.delete(id);
    if (!now) return;
    const gone = leaves(now.tree).filter((pane) => pane.terminal !== null && !alive.has(pane.terminal) && !fresh.has(pane.terminal));
    if (!gone.length) return;
    let tree: PaneNode | null = now.tree;
    let focus = now.focus;
    let zoom = now.zoom;
    for (const pane of gone) {
      if (!tree) break;
      if (leaves(tree).length <= 2) {
        ({ tree, focus, zoom } = emptied(tree, pane.key, focus, zoom));
        continue;
      }
      const out = removePane(tree, pane.key);
      tree = out.tree;
      if (focus === pane.key) focus = out.neighbour ?? "";
    }
    commitTree(tree, focus, zoom);
  },
  { immediate: true },
);

// A terminal chosen elsewhere (a page's selection, a request, the output given back): its pane
// takes the focus; one asked for that is in no pane comes in where there is room.
watch(chosen, (choice) => {
  const now = layout.value;
  if (!now || choice?.kind !== "terminal") return;
  const pane = leafOf(now.tree, choice.id);
  if (pane) {
    if (pane.key !== now.focus) setPanes(props.project.path, { ...now, focus: pane.key });
  } else if (asked === choice.id && terminals.value.some((term) => term.id === choice.id)) placeTerminal(choice.id, { kind: "auto" });
});

// Back to one terminal at a time: the one shown goes into the viewport again.
watch(panesOn, async (on) => {
  if (on) return;
  await nextTick();
  const id = active.value?.id;
  if (id !== undefined && viewport.value) attach(id, viewport.value, !collapsed.value && !animating.value, panelTyping.value && !collapsed.value);
});

// Panes that need more room than the panel has: it grows to hold them, as far as it can.
watch(
  () => (layout.value ? minSize(layout.value.tree).h : 0),
  (least) => {
    if (!least) return;
    const need = least + PANES_CHROME;
    if (panesHeight.value < need) panesHeight.value = Math.min(need, Math.max(PANEL_MIN, tallest()));
  },
  { immediate: true },
);

/* The Düzen menu. */

const presetNow = computed(() => presetOf(layout.value?.tree ?? null));
/** "Tek", the panes' count, or which of them is zoomed. */
const layoutCount = computed(() => {
  const now = layout.value;
  if (!now) return t("panes.single");
  const panes = leaves(now.tree);
  // Squeezed for want of room, the pane with the keyboard fills it as a zoomed one does.
  const filling = now.zoom ?? (panesView.value?.shown.mode === "squeezed" ? now.focus : null);
  const zoomed = filling ? panes.findIndex((pane) => pane.key === filling) : -1;
  return zoomed >= 0 ? `${zoomed + 1}/${panes.length}` : String(panes.length);
});
const menuRoom = computed(() => roomAt(Math.max(panesHeight.value, PANES_HEIGHT)));

/** A layout fits the panel at its tallest, with the terminals it would hold. */
function fitsPreset(id: PresetId): boolean {
  return id === "1" || preset(id, presetOrder(PRESETS.find((each) => each.id === id)?.count ?? 2), stageWidth.value, tallest() - PANES_CHROME) !== null;
}

/** The terminals a layout of `count` panes holds, in their tabs' order: the one shown among them. */
function presetOrder(count: number): number[] {
  const shown = active.value?.id;
  const order = terminals.value.map((term) => term.id);
  const at = shown === undefined ? -1 : order.indexOf(shown);
  if (shown !== undefined && at >= count) {
    order.splice(at, 1);
    order.splice(count - 1, 0, shown);
  }
  return order;
}

/**
 * A layout picked: Tek, one terminal at a time, the focused one shown; else that many panes, the
 * project's terminals in them in their tabs' order (the one shown among them), empty past them;
 * the panel grows if it must. Terminals left over wait.
 */
function pickLayout(id: PresetId): void {
  const path = props.project.path;
  const shown = active.value?.id;
  if (id === "1") {
    setPanes(path, null);
    if (shown !== undefined) chooseTab(path, { kind: "terminal", id: shown });
    return;
  }
  const order = presetOrder(PRESETS.find((each) => each.id === id)?.count ?? 2);
  const top = tallest();
  const tall = Math.min(Math.max(panesHeight.value, PANES_HEIGHT), top);
  const tree = preset(id, order, stageWidth.value, tall - PANES_CHROME) ?? preset(id, order, stageWidth.value, top - PANES_CHROME);
  if (!tree) return;
  panesHeight.value = Math.min(top, Math.max(tall, minSize(tree).h + PANES_CHROME));
  const panes = leaves(tree);
  const focus = panes.find((pane) => pane.terminal === shown) ?? panes[0];
  setPanes(path, { tree, focus: focus.key, zoom: null, rev: layout.value?.rev ?? 0 });
  if (focus.terminal !== null) chooseTab(path, { kind: "terminal", id: focus.terminal });
  if (collapsed.value) toggleCollapsed();
}

function evenPanes(): void {
  const now = layout.value;
  if (now) setPanes(props.project.path, { ...now, tree: evenAll(now.tree) });
}

function renameTerminal(id: number): void {
  const term = terminals.value.find((each) => each.id === id);
  if (term) renaming.value = { id, name: term.name };
}

function zoomFocused(): void {
  const now = layout.value;
  if (now) setPanes(props.project.path, { ...now, zoom: now.zoom ? null : now.focus });
}

/** ⌥⌘ and an arrow: the nearest pane that way, the keyboard in it. */
function moveFocus(act: PaneAction): void {
  const now = layout.value;
  const placed = placedNow();
  if (!now || !placed || now.zoom) return;
  const from = placed.panes.find((pane) => pane.leaf.key === now.focus);
  if (!from) return;
  const across = act === "left-pane" || act === "right-pane";
  const ahead = placed.panes.filter((pane) => {
    if (act === "left-pane") return pane.x + pane.w <= from.x;
    if (act === "right-pane") return pane.x >= from.x + from.w;
    if (act === "up-pane") return pane.y + pane.h <= from.y;
    return pane.y >= from.y + from.h;
  });
  const middle = (pane: PlacedPane) => ({ x: pane.x + pane.w / 2, y: pane.y + pane.h / 2 });
  const here = middle(from);
  const score = (pane: PlacedPane): number => {
    const there = middle(pane);
    const overlap = across ? Math.min(pane.y + pane.h, from.y + from.h) - Math.max(pane.y, from.y) : Math.min(pane.x + pane.w, from.x + from.w) - Math.max(pane.x, from.x);
    const along = across ? Math.abs(there.x - here.x) : Math.abs(there.y - here.y);
    const aside = across ? Math.abs(there.y - here.y) : Math.abs(there.x - here.x);
    return (overlap > 0 ? 0 : 100_000) + along + aside / 4;
  };
  const next = ahead.sort((a, b) => score(a) - score(b))[0];
  if (!next) return;
  focusPane(next.leaf.key, true);
  if (next.leaf.terminal === null) void nextTick(() => panel.value?.querySelector<HTMLElement>(".pane.on .cell button")?.focus());
}

/** The panes' keys, pressed anywhere in the panel (lib/panes: `paneAction`). */
function onPanelKey(event: KeyboardEvent): void {
  const act = paneAction(event);
  if (!act || renaming.value || errorCard.value) return;
  const now = layout.value;
  if (act === "right" || act === "down") {
    const dir: SplitDir = act === "right" ? "row" : "col";
    event.preventDefault();
    if (opening.value) return;
    if (now) {
      const rect = panesAsIs() ? placedNow()?.panes.find((pane) => pane.leaf.key === now.focus) : undefined;
      if (rect && canAddPane(now.tree) && canSplit(rect, dir, [kindOf(rect.leaf), "shell"])) void openTerminal(false, null, { kind: "split", key: now.focus, dir });
    } else if (active.value) {
      const room = roomAt(Math.min(Math.max(panesHeight.value, PANES_HEIGHT), tallest()));
      if (canSplit({ x: 0, y: 0, ...room }, dir, [kindOfTerminal(active.value.id), "shell"])) void openTerminal(false, null, { kind: "split", key: "", dir });
    }
    return;
  }
  if (!now) return;
  event.preventDefault();
  if (act === "zoom") zoomFocused();
  else if (act === "even") evenPanes();
  else moveFocus(act);
}

/* ---------- dropped onto the terminals ---------- */

/** Something dragged onto the terminals from elsewhere in the panel: a session from the history
 * (TerminalHistory) or, one terminal at a time, a tab from the row. */
type Incoming = { kind: "session" | "tab"; id: string | number; name: string; claude: boolean; x: number; y: number };
const incoming = ref<Incoming | null>(null);
/** Where it would land: a pane (`pane`; none with one terminal at a time) and its side, or the
 * middle (`side` none: into that pane, or for a session with one terminal at a time, a tab). */
type Landing = { pane: string | null; side: Side | null };

const stageEl = ref<HTMLElement | null>(null);
const panesView = ref<InstanceType<typeof TerminalPanes> | null>(null);

function overStage(x: number, y: number): boolean {
  const box = stageEl.value?.getBoundingClientRect();
  return !!box && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom;
}

function landingAt(drag: Incoming): Landing | null {
  const box = stageEl.value?.getBoundingClientRect();
  if (!box || !overStage(drag.x, drag.y)) return null;
  if (layout.value) {
    const at = panesView.value?.dropAt(drag.x, drag.y, "");
    // With as many panes as there may be, it goes into a pane, never beside one.
    return at ? { pane: at.key, side: canAddPane(layout.value.tree) ? at.side : null } : null;
  }
  const shown = active.value;
  // A tab beside itself is no split; with nothing shown, a session opens as a tab.
  if (drag.kind === "tab" && (!shown || shown.id === drag.id)) return null;
  if (!shown) return { pane: null, side: null };
  const rx = (drag.x - box.left) / box.width;
  const ry = (drag.y - box.top) / box.height;
  const near: [Side, number][] = [
    ["left", rx],
    ["right", 1 - rx],
    ["top", ry],
    ["bottom", 1 - ry],
  ];
  const side = near.sort((a, b) => a[1] - b[1])[0][0];
  const middle = rx > 0.3 && rx < 0.7 && ry > 0.3 && ry < 0.7;
  const room = roomAt(Math.min(Math.max(panesHeight.value, PANES_HEIGHT), tallest()));
  const coming: PaneKind = drag.kind === "session" || drag.claude ? "claude" : "shell";
  if (!middle && canSplit({ x: 0, y: 0, ...room }, side === "left" || side === "right" ? "row" : "col", [kindOfTerminal(shown.id), coming])) return { pane: null, side };
  return drag.kind === "tab" ? null : { pane: null, side: null };
}

const landing = computed(() => (incoming.value ? landingAt(incoming.value) : null));
/** The panes show their own landing; one terminal at a time shows it here. */
const paneLanding = computed(() => (landing.value?.pane ? { key: landing.value.pane, side: landing.value.side } : null));
const OPEN_AT: Record<Side, "panes.openLeft" | "panes.openRight" | "panes.openAbove" | "panes.openBelow"> = { left: "panes.openLeft", right: "panes.openRight", top: "panes.openAbove", bottom: "panes.openBelow" };
const MOVE_AT: Record<Side, "panes.moveLeft" | "panes.moveRight" | "panes.moveUp" | "panes.moveDown"> = { left: "panes.moveLeft", right: "panes.moveRight", top: "panes.moveUp", bottom: "panes.moveDown" };
const landingLabel = computed(() => {
  const at = landing.value;
  if (!at || at.pane) return "";
  if (at.side) return t(incoming.value?.kind === "tab" ? MOVE_AT[at.side] : OPEN_AT[at.side]);
  return t("panes.openTab");
});

function placeOf(at: Landing): Place | null {
  if (!at.side) return at.pane === null ? null : { kind: "cell", key: at.pane };
  return { kind: "split", key: at.pane ?? "", dir: at.side === "left" || at.side === "right" ? "row" : "col", before: at.side === "left" || at.side === "top" };
}

/**
 * A terminal put where something was let go: beside a pane, into one (whose terminal then waits),
 * or with one terminal at a time beside the one shown. One that is in a pane already moves there.
 */
function placeDropped(id: number, place: Place): void {
  const now = layout.value;
  const own = now ? leafOf(now.tree, id) : undefined;
  if (now && own) {
    if (place.kind === "split" && own.key !== place.key) {
      const edge: Side = place.dir === "row" ? (place.before ? "left" : "right") : place.before ? "top" : "bottom";
      setPanes(props.project.path, { ...now, tree: movePane(now.tree, own.key, place.key, edge), focus: own.key, zoom: null });
    } else if (place.kind === "cell" && own.key !== place.key) setPanes(props.project.path, { ...now, tree: swapPanes(now.tree, own.key, place.key), focus: own.key, zoom: null });
    return;
  }
  if (!now && active.value?.id === id) return;
  placeTerminal(id, place);
}

/** Where a session let go on the terminals goes, while its tab opens (or comes up): the request
 * that brings its terminal takes it (`terminalRequest`). */
let dropPlace: { place: Place; at: number } | null = null;

function sessionDragged(drag: { id: string; name: string; x: number; y: number } | null): void {
  incoming.value = drag ? { kind: "session", ...drag, claude: true } : null;
}

/** A session let go on the terminals: it opens there (`resume_claude` in the history), a tab of
 * Pitwall's it runs in coming there. Let go elsewhere, nothing happens. */
function sessionDropped(id: string): void {
  const at = landing.value;
  incoming.value = null;
  if (!at) return;
  const place = placeOf(at);
  dropPlace = place ? { place, at: Date.now() } : null;
  const history = side.value ?? over.value;
  void history?.open(id).then((result) => {
    if (result !== "shown" && result !== "opened") dropPlace = null;
  });
}

/* A pane dragged by its head back onto the tab row: out of the panes, a tab again where it is let
   go among the others (the line in the row says where). */

const tabsRow = ref<HTMLElement | null>(null);
/** Where along the row a pane dragged over it would go in, while one is. */
const rowMark = ref<number | null>(null);

function rowAt(x: number, y: number): boolean {
  const box = tabsRow.value?.getBoundingClientRect();
  return !!box && x >= box.left && x <= box.right && y >= box.top - 6 && y <= box.bottom + 4;
}

/** The row's tab a pane let go at `x` goes before (none: after the last), and where the line is. */
function rowSlot(x: number): { before: number | null; left: number } {
  const row = tabsRow.value?.getBoundingClientRect();
  const list = tablist.value;
  if (!row || !list) return { before: null, left: 0 };
  const tabs = [...list.querySelectorAll<HTMLElement>("[data-tab-id]")];
  for (const tab of tabs) {
    const box = tab.getBoundingClientRect();
    if (x < box.left + box.width / 2) return { before: Number(tab.dataset.tabId), left: box.left - row.left - 1 };
  }
  const last = tabs[tabs.length - 1]?.getBoundingClientRect();
  return { before: null, left: (last ? last.right : list.getBoundingClientRect().left) - row.left + 1 };
}

const rowMarkLeft = computed(() => (rowMark.value === null ? null : rowSlot(rowMark.value).left));

/** A pane let go on the row: its terminal is a tab there again (out of the panes, as "Take out of
 * the panes" does), in its place among the others. */
function toRow(key: string, x: number): void {
  rowMark.value = null;
  const now = layout.value;
  const id = now ? findLeaf(now.tree, key)?.terminal : null;
  if (!now || id === null || id === undefined) return;
  const { before } = rowSlot(x);
  const ids = terminals.value.map((term) => term.id).filter((each) => each !== id);
  const row = rowTabs.value.map((term) => term.id).filter((each) => each !== id);
  let at = before !== null ? ids.indexOf(before) : row.length ? ids.indexOf(row[row.length - 1]) + 1 : ids.length;
  if (at < 0) at = ids.length;
  ids.splice(at, 0, id);
  terminalOrder.value = { ...terminalOrder.value, [props.project.path]: ids };
  takeOut(key);
}

/** A tab dragged off the row onto the terminals: one at a time, it splits the one shown there;
 * with panes (the row holds the waiting tabs), it goes beside a pane or into it. */
const tabElsewhere = {
  over(id: number, x: number, y: number): boolean {
    if (!overStage(x, y)) {
      incoming.value = null;
      return false;
    }
    const term = terminals.value.find((each) => each.id === id);
    incoming.value = { kind: "tab", id, name: names.value.get(id) ?? term?.name ?? "", claude: term?.kind === "claude", x, y };
    return true;
  },
  drop(id: number): boolean {
    const at = landing.value;
    incoming.value = null;
    const place = at ? placeOf(at) : null;
    if (!place) return false;
    asked = id;
    placeDropped(id, place);
    chooseTab(props.project.path, { kind: "terminal", id });
    return true;
  },
  leave(): void {
    incoming.value = null;
  },
};

/** The tab row's empty part (between and after the tabs), not a tab, a name or a button. */
function emptyPart(event: MouseEvent): boolean {
  const target = event.target as HTMLElement;
  return target === event.currentTarget || target === tablist.value || target.classList.contains("grow") || target.classList.contains("outlist");
}

/**
 * The tab row's empty part, and the handle: a click on a collapsed panel brings it up at once (the
 * user's word: no need to find the button). A double click on the row's empty part opens a Claude
 * tab, as "New Claude" does, only on a panel open all the way: folded or still coming up, a double
 * click is no more than the click that opens it (the user's word). A tab's own double click
 * renames it, and the project's name and the buttons keep theirs.
 */
function onRowClick(event: MouseEvent): void {
  const empty = emptyPart(event);
  if (event.detail >= 2) {
    if (empty && !collapsed.value && !animating.value) void openTerminal(true);
    return;
  }
  if (!collapsed.value || !(empty || (event.target as HTMLElement).classList.contains("grip"))) return;
  toggleCollapsed();
}


/** A tab asked for by a click, Return or Space: shown, and a collapsed panel comes up with it. The
 * tab already shown takes the keyboard here, the watch on the shown tab not running for it: left
 * on the tab's label, a Backspace would close the tab. */
function open(id: number): void {
  // With panes, a tab waiting in the row comes into an empty pane, else into the pane with the
  // keyboard.
  const now = layout.value;
  if (now) {
    const pane = leafOf(now.tree, id);
    if (pane) focusPane(pane.key, true);
    else bring(id, emptyPane(now.tree, now.focus)?.key ?? now.focus);
    if (collapsed.value) toggleCollapsed();
    seeFinished(id);
    return;
  }
  const shown = active.value?.id === id;
  show(id);
  if (collapsed.value) toggleCollapsed();
  seeFinished(id);
  if (shown) focusView(id);
}

function close(id: number): void {
  void api.closeTerminal(id);
}

/** A tab closed with ⌘W: once it is gone, the one shown in its place takes the keyboard (the
 * watch on the shown tab gives it); with none left, the empty terminal's "New tab" does, so the
 * next ⌘W still finds the panel. */
let closing: number | null = null;

/**
 * ⌘W with the panel in front (WindowApp: `closeFront`): the output shown gives its place back, as
 * its tab picked again does; else the terminal shown closes; with no terminal left, the panel folds.
 */
function closeShown(): void {
  if (showsOutput.value) return closeOutput();
  // An empty pane goes.
  const now = layout.value;
  const pane = now ? findLeaf(now.tree, now.focus) : undefined;
  if (pane && pane.terminal === null) return takeOut(pane.key);
  const id = active.value?.id;
  if (id === undefined) return foldPanel();
  closing = id;
  close(id);
}

function rename(name: string): void {
  if (renaming.value) void api.renameTerminal(renaming.value.id, name);
}

/** Saved or not, the keyboard goes back to the terminal. */
function endRename(): void {
  const id = renaming.value?.id;
  renaming.value = null;
  if (id !== undefined) focusView(id);
}

const tablist = ref<HTMLElement | null>(null);

/** Tabs dragged into another order keep it, per project, for as long as Pitwall runs. */
const tabDrag = useTabReorder(
  tablist,
  (ids) => {
    // With panes the row holds only the waiting tabs: they take their new order in the places
    // they had among all of them.
    const moved = new Set(ids);
    let next = 0;
    const order = terminals.value.map((term) => (moved.has(term.id) ? ids[next++] : term.id));
    terminalOrder.value = { ...terminalOrder.value, [props.project.path]: order };
  },
  tabElsewhere,
  tabDragEnded,
);

/** A tab's drag is over, taken off the row or not: the keyboard goes to the terminal shown, as
 * after a click on a tab, the panel open. Never to the tab itself (a press on one leaves the
 * keyboard where it is): left on its label, a Backspace would close that tab. */
function tabDragEnded(): void {
  if (!collapsed.value) backToTerminal();
}

/** A click on a tab opens it; not the one that ends its drag: a drag only moves a tab. */
function clickTab(id: number): void {
  if (!tabDrag.justDragged()) open(id);
}

/* A tab opens by growing to its width and closes by shrinking away, the gap beside it too; the
   tabs and the buttons after it follow it frame by frame. Not as the panel comes, nor for a
   project's tabs when another project is picked: those are there at once. */
const TAB_IN_MS = 260;
const TAB_OUT_MS = 200;
/** Even at both ends: a tab is seen growing from nothing, not appearing nearly whole. */
const EASE = "cubic-bezier(0.4, 0, 0.2, 1)";

function animateTab(tab: HTMLElement, opening: boolean, done: () => void): void {
  if (!settled.value || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return done();
  const style = getComputedStyle(tab);
  const row = tab.parentElement;
  const gap = row && row.children.length > 1 ? parseFloat(getComputedStyle(row).columnGap) || 0 : 0;
  const full = { width: `${tab.getBoundingClientRect().width}px`, minWidth: "0px", paddingLeft: style.paddingLeft, paddingRight: style.paddingRight, marginRight: "0px", opacity: 1 };
  const none = { width: "0px", minWidth: "0px", paddingLeft: "0px", paddingRight: "0px", marginRight: `${-gap}px`, opacity: 0 };
  // The row's room for one tab at least would show at once: it waits for its first tab.
  if (opening && row?.children.length === 1) row.style.minWidth = "0px";
  tab.style.overflow = "hidden";
  if (!opening) tab.style.pointerEvents = "none";
  const motion = tab.animate(opening ? [none, full] : [full, none], { duration: opening ? TAB_IN_MS : TAB_OUT_MS, easing: EASE, fill: opening ? "none" : "forwards" });
  const end = (): void => {
    tab.style.overflow = "";
    if (row) row.style.minWidth = "";
    done();
  };
  motion.onfinish = end;
  motion.oncancel = end;
}

function tabEnter(el: Element, done: () => void): void {
  animateTab(el as HTMLElement, true, done);
}

function tabLeave(el: Element, done: () => void): void {
  animateTab(el as HTMLElement, false, done);
}
/** An arrow key picked the tab: the keyboard stays on the tabs instead of going to the shell. */
let stayOnTabs = false;
/** A tab closed from the keyboard: once it is gone, the focus goes to the tab shown next. */
let refocus: number | null = null;

function focusTab(id: number | undefined): void {
  if (id !== undefined) tablist.value?.querySelector<HTMLElement>(`[data-tab="${id}"]`)?.focus();
}

/** More tabs than the row holds even narrowed: the one shown comes into sight. */
function showTab(id: number | undefined): void {
  if (id !== undefined) tablist.value?.querySelector(`[data-tab="${id}"]`)?.closest(".tab")?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

/**
 * The tabs are one Tab stop: ←/→ (Home, End) pick the next tab and keep the focus on the tabs;
 * ↵ and Space go into the shown terminal; Delete closes the tab, F2 renames it.
 */
function onTabKey(event: KeyboardEvent, id: number): void {
  const list = rowTabs.value;
  const at = list.findIndex((term) => term.id === id);
  let to: number | undefined;
  switch (event.key) {
    case "ArrowRight":
      to = list[(at + 1) % list.length]?.id;
      break;
    case "ArrowLeft":
      to = list[(at - 1 + list.length) % list.length]?.id;
      break;
    case "Home":
      to = list[0]?.id;
      break;
    case "End":
      to = list[list.length - 1]?.id;
      break;
    case "Enter":
    case " ":
      event.preventDefault();
      open(id);
      focusView(id);
      return;
    case "Delete":
    case "Backspace":
      event.preventDefault();
      refocus = id;
      stayOnTabs = true;
      close(id);
      return;
    case "F2": {
      event.preventDefault();
      const term = list[at];
      if (term) renaming.value = { id: term.id, name: term.name };
      return;
    }
    default:
      return;
  }
  event.preventDefault();
  if (to === undefined) return;
  // With panes, the arrows only move between the waiting tabs: ↵ brings one in.
  if (panesOn.value) return focusTab(to);
  if (to !== active.value?.id) stayOnTabs = true;
  show(to);
  seeFinished(to);
  focusTab(to);
}

/** The project the shown terminal was last worked out for. */
let shownPath = "";

// The shown terminal goes into the viewport; the others keep running out of sight. It takes the
// keyboard, unless the arrows on the tabs are picking it, or the panel is collapsed: a terminal
// out of sight leaves the keyboard where it is.
watch(
  () => active.value?.id,
  async (id) => {
    const onTabs = stayOnTabs;
    stayOnTabs = false;
    const wanted = id !== undefined && asked === id;
    asked = null;
    const path = props.project.path;
    const moved = path !== shownPath;
    shownPath = path;
    const arrived = moved && panelFocus.value === path;
    if (moved) panelFocus.value = null;
    const followed = id !== undefined && followedTab.value === id;
    followedTab.value = null;
    await nextTick();
    // Another project's, or the tab a page follows to: without the keyboard, unless asked for (a
    // project picked in the list). One shown in the place of a tab that closed takes it.
    const quiet = !wanted && (moved ? !arrived : followed);
    showTab(id);
    // With panes, every terminal stands in its own pane (TerminalPanes): only the keyboard moves.
    if (id !== undefined && panesOn.value) {
      if (!onTabs && !collapsed.value && !quiet) focusView(id);
    } else if (id !== undefined && viewport.value) attach(id, viewport.value, !collapsed.value && !animating.value, !onTabs && !collapsed.value && !quiet);
    if (onTabs) focusTab(id);
  },
  { immediate: true },
);

// A tab closed from the keyboard: the focus goes to the tab shown in its place (or, with none
// left, to "New tab").
watch(terminals, async (list) => {
  if (refocus === null || list.some((term) => term.id === refocus)) return;
  refocus = null;
  stayOnTabs = false;
  await nextTick();
  if (active.value) focusTab(active.value.id);
  else panel.value?.querySelector<HTMLElement>(".action")?.focus();
});

// A tab closed with ⌘W (`closing`): with none left, the keyboard goes to "New tab".
watch(terminals, async (list) => {
  if (closing === null || list.some((term) => term.id === closing)) return;
  closing = null;
  await nextTick();
  if (!active.value && !collapsed.value) body.value?.querySelector<HTMLElement>(".empty .control")?.focus();
});

// A Claude session in one of these terminals was brought up (`reveal-terminal`, lib/panel:
// terminalRequest): its tab, the panel open, the keyboard in the shell. A request waits a moment
// for its project and tab to arrive, then lapses.
watch(
  () => [terminalRequest.value, props.project.path, terminals.value] as const,
  async ([request, path, list]) => {
    if (!request) return;
    if (Date.now() - request.at > REQUEST_MS) {
      terminalRequest.value = null;
      return;
    }
    if (request.path !== path || !list.some((term) => term.id === request.id)) return;
    terminalRequest.value = null;
    // A session let go on the terminals a moment ago: its terminal goes where it was let go.
    const dropped = dropPlace && Date.now() - dropPlace.at < REQUEST_MS * 2 ? dropPlace.place : null;
    dropPlace = null;
    if (dropped) placeDropped(request.id, dropped);
    if (collapsed.value) toggleCollapsed();
    const shown = active.value?.id === request.id;
    show(request.id);
    // Already the shown tab: the watch on it won't run, so the keyboard goes there from here.
    if (shown) {
      await nextTick();
      focusView(request.id);
    }
  },
  { immediate: true },
);

// The chequered flag's "Open in terminal" (lib/flag: shellRequest): a new shell tab of this
// project, opened as "New tab" opens one. The list's panel only, once it is on the page (its
// size is the tab's); a request waits a moment for its project to be the one shown, then lapses.
watch(
  () => [shellRequest.value, props.project.path] as const,
  async ([request, path]) => {
    if (!request) return;
    if (Date.now() - request.at > REQUEST_MS) {
      shellRequest.value = null;
      return;
    }
    if (request.path !== path) {
      panelPath.value = request.path;
      return;
    }
    shellRequest.value = null;
    await nextTick();
    void openTerminal();
  },
  { immediate: true },
);

// A page's "Open a Claude terminal" (lib/panel: openRequest): a new tab of the project it names,
// once that is this panel's project.
watch(
  () => [openRequest.value, props.project.path] as const,
  async ([request, path]) => {
    if (!request) return;
    if (Date.now() - request.at > REQUEST_MS) {
      openRequest.value = null;
      return;
    }
    if (request.path !== path) return;
    openRequest.value = null;
    await nextTick();
    void openTerminal(request.claude, null, null, null, request.account);
  },
  { immediate: true },
);

// No terminal opens on its own: a project starts with its dev server's output until you open one.

/* ---------- the keyboard in the panel ---------- */

/** The keyboard came into the panel (lib/panel: `panelTyping`). */
function onFocusIn(): void {
  panelTyping.value = true;
}

/** The keyboard left the panel, and not for another place in it. */
function onFocusOut(event: FocusEvent): void {
  const to = event.relatedTarget as Node | null;
  if (!to || !panel.value?.contains(to)) panelTyping.value = false;
}

/** The keyboard is somewhere else, though the panel never said it left (a terminal that closed
 * under it takes its focus along). */
function onFocusAnywhere(event: FocusEvent): void {
  if (panelTyping.value && !panel.value?.contains(event.target as Node)) panelTyping.value = false;
}

/* ---------- a press outside ---------- */

/**
 * A press anywhere on the page (its bar, its lists, the Track page's stage), the panel open, folds
 * it to its tab row as soon as the press is over: on something a double click opens (`data-dblopen`:
 * a project on the Track and Today pages, a session, a card's state) only once a double click is out
 * of the question (the Track page's rule, every page's since the user's
 * word, 2026-10-22), unless Settings › General turns it off. The sidebar, dialogs, floating
 * windows and what picks a project (`data-picks-project`: the list's rows, the Track page's
 * standings and tags; the panel turns to it) don't. Nor does a press that asks
 * the panel for something of its own ("Show in terminal", a command's output: lib/panel
 * `panelAsks`): it stays open for that.
 */
function onPressAnywhere(event: PointerEvent): void {
  if (collapsed.value || event.button !== 0 || snapshot.value?.settings.panelFoldOutside === false) return;
  const target = event.target instanceof Element ? event.target : null;
  if (!target?.closest(".page") || panel.value?.contains(target)) return;
  // A project picked (the list's row, the Track page's standings and tags): the panel turns to it
  // and stays as it is (the user's word).
  if (target.closest("[data-picks-project]")) return;
  const asks = panelAsks.value;
  const wait = target.closest("[data-dblopen]") ? FOLD_AFTER_MS : 0;
  window.addEventListener(
    "pointerup",
    () =>
      window.setTimeout(() => {
        if (panelAsks.value === asks) foldPanel();
      }, wait),
    { once: true },
  );
}

/** On something a double click opens, a press folds the panel this long after it is over: a
 * double click that opens a project's terminal or a session (lib/open, `revealTab`) asks the panel
 * for it in the meantime, and the panel never bounces. */
const FOLD_AFTER_MS = 300;

/** The size a new tab gets here, measured as the open panel would hold it: the Board page gives a
 * card to Claude at it (lib/panel: `panelMeasure`). */
function newTabSize(): { cols: number; rows: number } | null {
  const room = viewport.value?.clientWidth || TAB_ROOM.width;
  const tall = Math.min(height.value, (panel.value?.parentElement?.clientHeight ?? window.innerHeight) - HEADER) - TAB_CHROME;
  const host = document.createElement("div");
  host.style.cssText = `position: fixed; left: -10000px; top: 0; width: ${room}px; height: ${Math.max(tall, 40)}px; visibility: hidden`;
  document.body.appendChild(host);
  try {
    return measure(host);
  } catch {
    return null;
  } finally {
    host.remove();
  }
}

let observer: ResizeObserver | null = null;
/** How wide the panel is: room for the history beside the terminal or not. */
let panelObserver: ResizeObserver | null = null;
/** A row that narrows (the window, the panel) keeps the shown tab in sight too. */
let tabsObserver: ResizeObserver | null = null;
watch(tablist, (list, old) => {
  if (old) tabsObserver?.unobserve(old);
  if (list) tabsObserver?.observe(list);
});

onMounted(() => {
  observer = new ResizeObserver(() => {
    if (active.value && !collapsed.value && !animating.value && !panesOn.value) fitView(active.value.id, !sliding.value);
  });
  if (viewport.value) observer.observe(viewport.value);
  tabsObserver = new ResizeObserver(() => showTab(active.value?.id));
  if (tablist.value) tabsObserver.observe(tablist.value);
  const measure = (): void => {
    panelWidth.value = panel.value?.clientWidth ?? 0;
    narrow.value = panelWidth.value < HISTORY_ROOM;
    // As it slides too, frame by frame: the page's room under its lists follows.
    panelOver.value = Math.max(0, (panel.value?.offsetHeight ?? 0) - COLLAPSED_HEIGHT);
  };
  panelObserver = new ResizeObserver(measure);
  if (panel.value) panelObserver.observe(panel.value);
  measure();
  requestAnimationFrame(() => requestAnimationFrame(() => (settled.value = true)));
  panelMeasure.size = newTabSize;
  // The panes' keys are the panel's, not the shell's: splitting from one terminal at a time too.
  paneKeys.claim = (event) => {
    const act = paneAction(event);
    return !!act && (panesOn.value || act === "right" || act === "down");
  };
  document.addEventListener("focusin", onFocusAnywhere);
  document.addEventListener("pointerdown", onPressAnywhere, true);
});

// Wide again, the column is as it was; narrow again or collapsed, the history over the terminal
// waits for its button.
watch([narrow, collapsed], () => {
  overlayOpen.value = false;
});

onBeforeUnmount(() => {
  observer?.disconnect();
  tabsObserver?.disconnect();
  panelObserver?.disconnect();
  clearTimeout(settle);
  clearTimeout(slid);
  window.clearTimeout(copiedTimer);
  panelMeasure.size = () => null;
  paneKeys.claim = () => false;
  panelTyping.value = false;
  panelOver.value = 0;
  document.removeEventListener("focusin", onFocusAnywhere);
  document.removeEventListener("pointerdown", onPressAnywhere, true);
});

defineExpose({ openTerminal, closeShown });
</script>

<template>
  <section
    ref="panel"
    :class="['term-panel', { animated: !dragging, collapsed }]"
    :style="{ height: `${shownHeight}px` }"
    :aria-label="t('terminal.panel', { name: project.name })"
    @focusin="onFocusIn"
    @focusout="onFocusOut"
    @keydown="onPanelKey"
  >
    <div
      class="grip"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('terminal.resize')"
      tabindex="0"
      :aria-valuenow="Math.round(shownHeight)"
      @pointerdown="startDrag"
      @pointermove="resizer.move"
      @pointerup="resizer.end"
      @pointercancel="resizer.end"
      @keydown="resizer.nudge"
      @click="onRowClick"
    ></div>
    <div ref="tabsRow" class="tabs" @click="onRowClick">
      <!-- The panel's project: the page's, or another picked here. -->
      <div class="where">
        <ProjectPicker :projects="projects" :scope="project.path" :waits="waits" label="terminal.projectLabel" :every="false" plain working @pick="pickProject" />
      </div>
      <span id="terminal-tab-keys" class="sr-only">{{ t("terminal.tabKeys") }}</span>
      <!-- Each tab and its close button side by side, never one inside the other; the list holds
           tabs only, so the close button is the mouse's (the keyboard closes with Delete). One Tab
           stop: the shown tab; ←/→ move. Double-click (or F2) renames; a middle click closes; a drag
           moves it. A press leaves the keyboard where it is: Tab alone brings it to the tabs. -->
      <!-- With panes the names of the terminals in them are on the panes: the row lists the others,
           waiting to be clicked or dragged into one. -->
      <div ref="tablist" :class="['tablist', { some: rowTabs.length }, tabDrag.listClass()]" role="tablist" :aria-label="t('terminal.tabs')">
        <TransitionGroup :key="project.path" :css="false" @enter="tabEnter" @leave="tabLeave">
        <div
          v-for="term in rowTabs"
          :key="term.id"
          :class="['tab', { on: active?.id === term.id }, tabDrag.tabClass(term.id)]"
          :style="tabDrag.tabStyle(term.id)"
          :data-tab-id="term.id"
          role="none"
          :title="`${term.name}\n${terminalTip(term)}`"
          @pointerdown="tabDrag.down($event, term.id)"
          @click="clickTab(term.id)"
          @dblclick="renaming = { id: term.id, name: term.name }"
          @mousedown.prevent
          @mouseup.middle="close(term.id)"
        >
          <div
            role="tab"
            class="tab-label"
            :data-tab="term.id"
            :tabindex="(panesOn ? rowTabs[0]?.id === term.id : active?.id === term.id) ? 0 : -1"
            :aria-selected="active?.id === term.id"
            aria-keyshortcuts="Delete F2"
            aria-describedby="terminal-tab-keys"
            @keydown="onTabKey($event, term.id)"
          >
            <ClaudeLogo v-if="term.kind === 'claude'" :size="11" />
            <Icon v-else :name="shellIcon(term.shell)" :size="11" />
            <span class="tab-name">{{ names.get(term.id) ?? term.name }}</span>
            <span v-if="accountLabel(term.account)" class="tab-account">{{ accountLabel(term.account) }}</span>
            <span v-if="term.elevated" class="admin" role="img" :aria-label="t('terminal.adminBadge')"><Icon name="shield" :size="10" /></span>
            <ClaudeDot v-if="live.has(term.id)" :live="live.get(term.id)" />
          </div>
          <button
            type="button"
            class="x"
            tabindex="-1"
            aria-hidden="true"
            data-no-drag
            :title="t('common.close')"
            @click.stop="close(term.id)"
            @dblclick.stop
          >×</button>
        </div>
        </TransitionGroup>
      </div>
      <!-- The terminals' tabs and right after them the button that opens one; at the right end, the
           outputs' tabs, the shown output's tools, the history and collapse. -->
      <span :class="['divider', { shown: rowTabs.length }]" aria-hidden="true"></span>
      <!-- A shell tab, a Claude session, or one with a model or mode of its own: a menu under it. -->
      <button
        type="button"
        class="icon add"
        aria-haspopup="menu"
        aria-keyshortcuts="ArrowDown"
        :aria-label="t('terminal.add')"
        :title="t('terminal.add')"
        :disabled="opening"
        @click="addMenu"
        @contextmenu.prevent="addMenu"
        @keydown.down.prevent="addMenu"
      >
        <Icon name="plus" :size="13" />
      </button>
      <span class="grow"></span>
      <!-- The outputs, always at the right end: the dev server's, then the commands' that ran. -->
      <div class="outlist" role="tablist" :aria-label="t('detail.output')">
        <!-- A command's tab comes as a terminal's does, growing to its width. -->
        <TransitionGroup :key="project.path" :css="false" @enter="tabEnter" @leave="tabLeave">
        <button
          v-for="(item, i) in outputs"
          :key="item.id ?? 'server'"
          type="button"
          role="tab"
          :aria-selected="showsOutput && job === item.id"
          :tabindex="item.id === job ? 0 : -1"
          :class="['out-tab', { on: showsOutput && job === item.id }]"
          :title="item.name"
          @click="openOutput(item.id)"
          @keydown="onOutputKey($event, i)"
        >
          <span v-if="item.state" :class="['out-state', item.state]" aria-hidden="true"></span>
          <span class="tab-name">{{ item.name }}</span>
        </button>
        </TransitionGroup>
      </div>
      <!-- The shown output's tools: they come and go as a tab does, growing to their width and
           shrinking away, the outputs' tabs sliding along. -->
      <Transition :css="false" @enter="tabEnter" @leave="tabLeave">
      <div v-if="showsOutput && !collapsed" class="tools">
        <span class="divider shown" aria-hidden="true"></span>
        <button type="button" :class="['icon', 'tool', { on: searching }]" :aria-pressed="searching" :aria-label="t('output.search')" :title="t('output.search')" @click="searching ? closeSearch() : openSearch()">
          <Icon name="search" :size="13" />
        </button>
        <button
          type="button"
          :class="['icon', 'tool', { on: errorsOnly }]"
          :aria-pressed="errorsOnly"
          :aria-label="errorCount ? `${t('output.errorsOnly')} (${errorCount})` : t('output.errorsOnly')"
          :title="t('output.errorsOnly')"
          @click="errorsOnly = !errorsOnly"
        >
          <Icon name="filter" :size="13" />
          <span v-if="errorCount" class="errors">{{ errorCount }}</span>
        </button>
        <button type="button" class="icon tool" :disabled="!shownOutput.length" :aria-label="t('output.copyAll')" :title="t(copied ? 'output.copied' : 'output.copyAll')" @click="copyAll">
          <Icon :name="copied ? 'check' : 'copy'" :size="13" />
        </button>
      </div>
      </Transition>
      <LayoutMenu :current="presetNow" :count="layoutCount" :room="menuRoom" :fits="fitsPreset" :panes="panesOn" @pick="pickLayout" @even="evenPanes" />
      <button
        type="button"
        :class="['action', 'history-toggle', { on: historyOpen }]"
        :aria-pressed="historyOpen"
        :title="t('terminal.historyTitle')"
        @click="toggleHistory"
      >
        <Icon name="clock" :size="13" /> <span class="action-text">{{ t("terminal.history") }}</span>
      </button>
      <button
        type="button"
        :class="['icon', 'collapse', { up: collapsed }]"
        :aria-expanded="!collapsed"
        :aria-label="t(collapsed ? 'terminal.expand' : 'terminal.collapse')"
        :title="t(collapsed ? 'common.expand' : 'common.collapse')"
        @click="toggleCollapsed"
      >
        <Icon name="chevron" :size="13" />
      </button>
      <!-- A pane dragged over the row: where it goes in among the tabs. -->
      <span v-if="rowMarkLeft !== null" class="row-mark" :style="{ left: `${rowMarkLeft}px` }" aria-hidden="true"></span>
    </div>
    <div ref="body" class="body" :style="{ '--history-width': `${historyShown}px` }">
      <!-- The terminal, and over it the output shown in its place (one per output, each starting
           at its end, following); collapsed, out of the keyboard's way. -->
      <div ref="stageEl" class="stage">
        <div v-show="!panesOn" :class="['screen', { away: showsOutput }]">
          <div ref="viewport" class="viewport" @pointerdown="pressScreen"></div>
          <div v-if="!active" class="empty" @pointerdown="overlayOpen = false">
            <span>{{ t("terminal.none", { name: project.name }) }}</span>
            <!-- The "+" menu's first two, at hand where there is nothing yet. -->
            <div class="empty-actions">
              <button type="button" class="control" :disabled="opening" @click="openTerminal()">
                <Icon name="terminal" :size="12" /> {{ t("terminal.newTab") }}
              </button>
              <button type="button" class="control" :disabled="opening" @click="openTerminal(true)">
                <ClaudeLogo :size="12" /> {{ t("terminal.newClaude") }}
              </button>
            </div>
          </div>
        </div>
        <TerminalPanes
          v-if="layout"
          ref="panesView"
          :class="{ away: showsOutput }"
          :incoming="paneLanding"
          :incoming-moves="incoming?.kind === 'tab'"
          :row-at="rowAt"
          :names="names"
          @over-row="(x) => (rowMark = x)"
          @to-row="toRow"
          :project="project"
          :layout="layout"
          :terminals="terminals"
          :live="live"
          :hold="collapsed || animating"
          :quiet="sliding"
          :opening="opening"
          :shells="shells"
          @open="(where, claude, start, shell) => openTerminal(claude, start, where, shell)"
          @close="close"
          @rename="renameTerminal"
          @focus="focusPane"
          @remove="takeOut"
          @room="(w, h) => (paneRoom = { w, h })"
        />
        <!-- One terminal at a time: where something dragged onto it would land. -->
        <div v-if="!panesOn && landing && !landing.pane" :class="['landing', landing.side ?? 'whole']" aria-hidden="true">
          <span>{{ landingLabel }}</span>
        </div>
        <div v-if="showsOutput" class="output" @keydown="onOutputKeys" @pointerdown="overlayOpen = false">
          <OutputSearch v-if="searching" ref="field" v-model:query="query" :position="position" :count="matches" @step="step" @close="closeSearch" />
          <OutputLog
            ref="log"
            :key="pane"
            class="log"
            :inert="collapsed"
            :path="project.path"
            :lines="drawnOutput"
            :run-start="runStart"
            :ranges="ranges"
            :current="current"
            :empty="emptyOutput"
            @card="errorCard = { ...$event, path: project.path }"
          />
        </div>
      </div>
      <!-- The history's room beside the terminal: the panel's right margin alone while it is
           hidden; shown, it grows to the left and the terminal gives up that width as it goes. -->
      <div :class="['side', { open: beside, resizing, still: !settled }]" :style="beside ? { width: `${historyShown + 32}px` } : undefined">
        <Transition name="fade">
          <div
            v-if="beside"
            class="width-grip"
            role="separator"
            aria-orientation="vertical"
            tabindex="0"
            :aria-label="t('terminal.historyResize')"
            :aria-valuenow="historyShown"
            @pointerdown="startWidth"
            @pointermove="moveWidth"
            @pointerup="endWidth"
            @pointercancel="endWidth"
            @keydown="nudgeWidth"
            @dblclick="resetWidth"
          ></div>
        </Transition>
        <Transition name="fade">
          <TerminalHistory v-if="beside" ref="side" :project="project" @leave="backToTerminal" @drag="sessionDragged" @drop="sessionDropped" />
        </Transition>
      </div>
      <Transition name="pop">
        <TerminalHistory v-if="narrow && overlayOpen" ref="over" :project="project" overlay @close="closeOverlay" @leave="backToTerminal" @drag="sessionDragged" @drop="sessionDropped" />
      </Transition>
    </div>
    <!-- After an administrator's shell was asked for, or a start that put none back: a line of its
         own under the terminals (they give up its height), until it is closed. -->
    <Transition name="fade">
      <div v-if="shownNotice && !collapsed" class="notice" role="status">
        <Icon name="shield" :size="13" />
        <span class="notice-text">
          <template v-if="shownNotice === 'window'">
            {{ t("terminal.adminOutside") }}
            <button type="button" class="notice-link" @click="openSudoSettings">{{ t("terminal.adminSettings") }}</button>
          </template>
          <template v-else-if="shownNotice === 'denied'">{{ t("terminal.adminDenied") }}</template>
          <template v-else>{{ t("terminal.adminNotRestored", { count: notRestored }) }}</template>
        </span>
        <button type="button" class="notice-x" :aria-label="t('common.close')" :title="t('common.close')" @click="closeNotice">×</button>
      </div>
    </Transition>

    <div v-if="incoming" class="ghost" :style="{ left: `${incoming.x + 12}px`, top: `${incoming.y + 10}px` }" aria-hidden="true">
      <ClaudeLogo v-if="incoming.claude" :size="11" />
      <Icon v-else name="terminal" :size="11" />
      {{ incoming.name }}
    </div>

    <RenameDialog v-if="renaming" :name="renaming.name" :project-name="project.name" @save="rename" @close="endRename" />
    <CardDialog v-if="errorCard" :card="null" :path="errorCard.path" :projects="cardProjects" :prefill="errorCard" @close="errorCard = null" />
  </section>
</template>

<style scoped>
/* Along the window's bottom, on its own ground, with its own height (lib/panel): up to the page's
   bar. The page keeps only its tab row's height for it (WindowApp); open, it lies over the page,
   which is never laid out again or scrolls in less room. Over what the page draws, under its
   tooltips, pickers and the Files windows. */
.term-panel {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 10;
  min-height: 0;
  max-height: calc(100% - 56px);
  background: var(--bg-app);
  overflow: clip;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--line);
  box-shadow: var(--shadow-rise);
}

/* Collapsing and expanding slide (COLLAPSE_MS in lib/panel); a drag follows the pointer. */
.term-panel.animated {
  transition: height 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

/* Collapsed, the handle and the row's empty part bring it up with a click. */
.term-panel.collapsed .grip,
.term-panel.collapsed .tabs {
  cursor: pointer;
}

.term-panel.collapsed .grip::after {
  opacity: 0;
}

.grip {
  flex-shrink: 0;
  height: 9px;
  margin: 0 20px;
  cursor: row-resize;
  position: relative;
  outline: none;
}

.grip::after {
  content: "";
  position: absolute;
  left: 50%;
  top: 3px;
  width: 36px;
  height: 3px;
  margin-left: -18px;
  border-radius: 2px;
  background: var(--line-dialog);
}

.grip:hover::after {
  background: var(--line-heavy);
}

/* The handle's own focus: the bar in the focus ring's colour. */
.grip:focus-visible::after {
  background: var(--focus-ring);
}

/* A fixed height: renaming a tab (its input) never pushes the terminal down. */
.tabs {
  position: relative;
  container-type: inline-size;
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 12px 0 14px;
  min-width: 0;
}

/* More tabs than the row holds: they narrow, their names cut (well before the project's is), and
   the buttons on the right stay where they are; the one shown keeps in sight (one tab's room at
   least). */
.tablist {
  flex: 0 100000 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow: hidden;
}

.tablist.some {
  min-width: 56px;
}

/* Drag to reorder (lib/tabReorder): the tab lifts and follows the pointer, the others slide
   aside, and on release it slides into place. */
.tablist.reordering .tab {
  transition: translate 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.tablist.reordering .tab:hover:not(.on, .dragging) {
  color: var(--text-faint);
}

.tab.dragging {
  position: relative;
  z-index: 2;
  background: var(--bg-lit);
  color: var(--text-strong);
  transition: none;
}

/* The dragged tab is under the pointer at every move: the others' slide (above) would make it trail
   the pointer and start over at each move, a stutter. Only its settling slides. */
.tablist.reordering .tab.dragging:not(.settling) {
  transition: none;
}

.tablist.reordering .tab.dragging.settling {
  transition: translate 160ms ease-out;
}

.tablist.frozen .tab {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .tablist.reordering .tab,
  .tablist.reordering .tab.dragging.settling {
    transition: none;
  }
}

/* The tab and its close button read as one: the hover, the colour and the hand are the pair's. */
.tab {
  min-width: 56px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 6px;
  border-radius: 6px;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
  cursor: pointer;
}

.tab-label {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

/* A tab holds its whole name; what passes this is cut, and the tooltip shows it all. */
.tab-name {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab-label > :not(.tab-name),
.x {
  flex-shrink: 0;
}

/* Another Claude account's tab (its `claude` runs with it): the account's name, quietly. */
.tab-account {
  max-width: 64px;
  font-size: 11px;
  color: var(--text-faint);
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab-account::before {
  content: "· ";
}

/* An administrator's shell: a shield in the warning's colour, the word in the tooltip. */
.admin {
  display: inline-flex;
  color: var(--warn);
}

/* The keyboard's tab: the ring goes round the pair, as the tab looks like one piece. */
.tab-label:focus-visible {
  outline: none;
}

.tab:has(> .tab-label:focus-visible) {
  outline: 2px solid var(--focus-ring);
  outline-offset: -1px;
}

.tab:hover {
  color: var(--text-muted);
}

.tab.on {
  color: var(--text-lit);
}

.x {
  width: 14px;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: center;
  font-size: 13px;
  line-height: 1;
  color: transparent;
  border-radius: 4px;
}

.tab:hover .x,
.tab.on .x {
  color: var(--text-faint);
}

.x:hover {
  color: var(--text-strong) !important;
  background: var(--bg-icon-hover);
}

.icon {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.icon:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.grow {
  flex-grow: 1;
}

/* "History", drawn as the outputs' tabs are: no ground, its colour alone says it is hovered or
   shown. */
.action {
  height: 22px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 6px;
  border: 0;
  /* The keyboard's ring only: no ground is drawn. */
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.action:hover:not(:disabled) {
  color: var(--text-muted);
}

/* "History" keeps its mark whatever the tabs take ("New tab" and "New Claude" are marks alone). */
.action.history-toggle {
  flex-shrink: 0;
}

/* "History" while its list shows: lit, as the tab shown is. */
.action.on,
.action.on:hover:not(:disabled) {
  color: var(--text-lit);
}

/* A narrow row: "History" is its clock alone (still its tooltip, and read out). */
@container (max-width: 455px) {
  .action.history-toggle {
    flex-shrink: 0;
    width: 24px;
    padding: 0;
    justify-content: center;
  }

  .history-toggle .action-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

/* The project, first in the row, set off from its tabs; its name is cut only once they have no
   more room to give. */
.where {
  flex: 0 1 auto;
  min-width: 64px;
  max-width: 200px;
  display: flex;
  margin-right: 6px;
  padding-right: 6px;
  border-right: 1px solid var(--line);
}

/* The outputs' tabs, at the row's right end: they give way before the terminals' do. */
.outlist {
  flex: 0 100000 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow: hidden;
}

.out-tab {
  min-width: 0;
  height: 22px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0 6px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.out-tab:hover {
  color: var(--text-muted);
}

.out-tab.on {
  color: var(--text-lit);
}

/* What the output's run is at: green while it runs, red once it failed (a crashed server). */
.out-state {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.out-state.on {
  background: var(--run);
}

.out-state.bad {
  background: var(--crash);
}

/* The shown output's tools, before "History": one piece that grows in and shrinks away. */
.tools {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.tool {
  position: relative;
}

.tool:disabled {
  opacity: 0.45;
}

.tool.on {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* How many errors the output holds, on the corner of the button that shows them alone: the
   button keeps its width. Inside the row's height, which cuts what leaves it. */
.errors {
  position: absolute;
  top: 0;
  right: -4px;
  min-width: 13px;
  height: 13px;
  padding: 0 3px;
  box-sizing: border-box;
  border-radius: 7px;
  background: var(--crash-bg);
  box-shadow: 0 0 0 1px var(--crash-line);
  font-size: 9px;
  font-weight: 600;
  line-height: 13px;
  font-variant-numeric: tabular-nums;
  text-align: center;
  color: var(--crash-text);
  pointer-events: none;
}

/* The tabs set off from the buttons after them, as the project is from its tabs (its right
   border); with no tabs there is nothing to set off, and it comes and goes with the first. The
   shown output's tools are set off from the outputs' tabs the same way. */
.divider {
  flex-shrink: 0;
  width: 0;
  height: 14px;
  margin: 0 -2px 0 0;
  background: var(--line);
  opacity: 0;
  transition:
    width 260ms cubic-bezier(0.4, 0, 0.2, 1),
    margin 260ms cubic-bezier(0.4, 0, 0.2, 1),
    opacity 200ms ease;
}

.divider.shown {
  width: 1px;
  margin: 0 6px 0 8px;
  opacity: 1;
}

@media (prefers-reduced-motion: reduce) {
  .divider {
    transition: none;
  }
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

/* The terminal, and the history beside it or over it. */
.body {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
}

/* The terminal's place, which the output shown takes over. */
.stage {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  margin: 6px 0 20px 20px;
  display: flex;
  transition: opacity 160ms ease;
}

.screen {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  padding: 8px 4px 8px 10px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid var(--line-inset);
  display: flex;
  overflow: clip;
  transition: opacity 160ms ease;
}

/* Opening, the room comes first and the list fades in once it is there; closing, the list fades
   first and then the room goes. */
.side {
  position: relative;
  flex-shrink: 0;
  width: 20px;
  overflow: hidden;
  transition:
    width 220ms cubic-bezier(0.2, 0.8, 0.2, 1) 120ms,
    opacity 160ms ease;
}

/* Open, the gap, the history (`historyShown`) and the panel's right margin: set on the element. */
.side.open {
  transition-delay: 0ms;
}

/* A drag follows the pointer; coming to the page, an open history is there without moving. */
.side.resizing,
.side.still,
.side.still .fade-enter-active,
.side.still .fade-leave-active {
  transition: none;
}

/* The history's handle: the gap beside the terminal, with a bar like the panel's handle. */
.width-grip {
  position: absolute;
  top: 6px;
  bottom: 20px;
  left: 0;
  width: 12px;
  cursor: col-resize;
  outline: none;
}

.width-grip::after {
  content: "";
  position: absolute;
  top: 50%;
  left: 4px;
  width: 3px;
  height: 36px;
  margin-top: -18px;
  border-radius: 2px;
  background: var(--line-dialog);
}

.width-grip:hover::after,
.side.resizing .width-grip::after {
  background: var(--line-heavy);
}

.width-grip:focus-visible::after {
  background: var(--focus-ring);
}

.fade-enter-active {
  transition: opacity 160ms ease 220ms;
}

.fade-leave-active {
  transition: opacity 120ms ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

/* A narrow panel's history comes in over the terminal from its right edge. */
.pop-enter-active,
.pop-leave-active {
  transition:
    opacity 160ms ease,
    transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateX(12px);
}

/* An output in its place: the terminal keeps its size (it is measured) out of sight. */
.screen.away,
.panes.away {
  visibility: hidden;
}

/* Where something dragged onto the one terminal shown would land: beside it, or into it. */
.landing {
  position: absolute;
  inset: 4px;
  z-index: 2;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 7px;
  border: 1.5px dashed var(--line-heavy);
  background: color-mix(in srgb, var(--text-muted) 10%, transparent);
  pointer-events: none;
}

.landing.left {
  right: 50%;
}

.landing.right {
  left: 50%;
}

.landing.top {
  bottom: 50%;
}

.landing.bottom {
  top: 50%;
}

.landing span {
  padding: 3px 9px;
  border-radius: 6px;
  background: var(--bg-lit);
  font-size: 12px;
  color: var(--text-strong);
  white-space: nowrap;
}

/* Where a pane dragged over the tab row goes in among the tabs. */
.row-mark {
  position: absolute;
  top: 3px;
  bottom: 3px;
  width: 2px;
  border-radius: 1px;
  background: var(--text-muted);
  pointer-events: none;
}

/* What is being dragged, under the pointer. */
.ghost {
  position: fixed;
  z-index: 70;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 280px;
  height: 22px;
  padding: 0 8px;
  border-radius: 6px;
  background: var(--bg-lit);
  box-shadow: var(--shadow-menu);
  font-size: 12px;
  color: var(--text-strong);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
}

/* The output over the terminal's place: its search, then its lines (detail/OutputLog). */
.output {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.output > .find {
  margin: 0;
}

.output > .log {
  flex: 1 1 auto;
  min-height: 0;
}

/* Collapsed, the tab row alone: nothing of the terminal or the history shows under it. */
.term-panel.collapsed .stage,
.term-panel.collapsed .side,
.term-panel.collapsed .history {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .term-panel.animated,
  .stage,
  .screen,
  .side,
  .fade-enter-active,
  .fade-leave-active,
  .pop-enter-active,
  .pop-leave-active,
  .collapse :deep(svg) {
    transition: none;
  }
}

/* Nothing inside the terminal can scroll it or anything around it. */
.viewport {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow: clip;
}

.viewport :deep(.terminal-host),
.viewport :deep(.xterm) {
  height: 100%;
}

/* xterm paints its scroll area black; below the last row it would show as a strip. */
.viewport :deep(.xterm-viewport) {
  background-color: var(--bg-log) !important;
}

/* No terminal open: over the viewport, so the viewport always keeps its size and can be measured. */
.empty {
  position: absolute;
  inset: 0;
  border-radius: 8px;
  background: var(--bg-log);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-subtle);
}

.empty-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.control {
  height: 30px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
  color: var(--text-soft);
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control:disabled {
  opacity: 0.4;
  cursor: default;
}

/* What the panel says about an administrator's shell, on a row of its own under the terminals (the
   stage's foot margin above it, the panel's margins beside and under it): its own ground, in the
   warning's colours, a link to Windows' setting where there is one, and its close button. */
.notice {
  flex: none;
  display: flex;
  align-items: flex-start;
  gap: 9px;
  margin: -8px 20px 12px;
  padding: 9px 8px 9px 12px;
  border-radius: 7px;
  border: 1px solid var(--warn-line);
  background: var(--warn-bg);
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-soft);
}

.notice > :deep(svg) {
  flex-shrink: 0;
  margin-top: 2px;
  color: var(--warn);
}

.notice-text {
  flex: 1 1 auto;
  min-width: 0;
}

.notice-link {
  padding: 0;
  border: 0;
  background: transparent;
  font: inherit;
  color: var(--warn-text);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
}

.notice-link:hover {
  color: var(--text-strong);
}

.notice-x {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font-size: 14px;
  line-height: 1;
  color: var(--text-faint);
}

.notice-x:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}
</style>
