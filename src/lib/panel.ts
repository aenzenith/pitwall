import { ref, watch, type Ref } from "vue";

import { holding } from "./holding";
import { api } from "./store";
import type { Agent } from "./types";

/**
 * The panel along the window's bottom (window/TerminalPanel), under every page: one project's
 * outputs (the dev server, its commands) and terminals as tabs, with one height and one collapsed
 * state whatever the page. Every launch starts it collapsed, its tab row alone, and expanded at the
 * default; it can be dragged down to the minimum and up to the page's bar.
 */
/** Room for two terminal rows: tabs and a prompt stay usable. */
export const PANEL_MIN = 120;
/** The least an unfolded panel opens at, along the bottom or as a card, where there is room for it:
 * one dragged low opens this tall again (the user's word, 14 Nov: "dar açılıyor"). Open, it can
 * still be dragged down to `PANEL_MIN`. */
export const PANEL_OPEN = 520;
/** Room for a Claude session. */
const PANEL_DEFAULT = PANEL_OPEN;

export const terminalHeight = ref(PANEL_DEFAULT);
export const terminalCollapsed = ref(true);

/** A collapsed panel keeps only its handle and tab row; its height waits for its return. */
export const COLLAPSED_HEIGHT = 42;

/**
 * Where the panel stands (Settings › General, the row's buttons, a drag): along the window's
 * bottom (`dock`), as a card over the page (`float`), or in a window of its own (`window`). As the
 * core holds it (`settings.terminalPlace`), and a step ahead of it while a change is on its way
 * (lib/panelStand).
 */
export type PanelStand = "dock" | "float" | "window";
export const panelStand = ref<PanelStand>("dock");

/** The floating card's place, as it was last left: its right and bottom edges' distance from the
 * page's, its width and height (CSS px). None until it is first moved: the default place. */
export type CardRect = { right: number; bottom: number; width: number; height: number };
export const cardRect = ref<CardRect | null>(null);

/** The card a double click grew to the page (`to`, as it was given) and its place before (`from`),
 * for the next double click to bring it back; kept beside the card's place, which outlives the
 * panel's page. */
export const cardGrown: { from: CardRect | null; to: CardRect | null } = { from: null, to: null };

/** The panel stands in its own window, and this is the main window's page: what a page asks of the
 * panel goes to that window (`panelAsk`), where it is done as it would be here (TerminalsApp). */
export function panelAway(): boolean {
  return panelStand.value === "window" && document.documentElement.dataset.surface !== "terminals";
}

/** What a page of the main window asks of the panel in its own window. */
export type PanelAsk =
  | { kind: "follow"; path: string }
  | { kind: "followTab"; path: string; id: number }
  | { kind: "output"; path: string; job: string | null }
  | { kind: "open"; path: string }
  | { kind: "reveal"; path: string; id: number }
  | { kind: "new"; path: string; claude: boolean; account: string | null; agent: Agent }
  | { kind: "shell"; path: string }
  /** ⌘T in the main window: the keyboard into the panel there (`focusPanel`). */
  | { kind: "focus" };

/** Sent to the terminals' window; `show`: it comes up with the keyboard (a page's selection that it
 * follows does not). */
export function askAway(ask: PanelAsk, show: boolean): void {
  void api.panelAsk(JSON.stringify(ask), show).catch(() => undefined);
}

/** How far the panel, open, lies over the page (its height as drawn, less its tab row's, which the
 * page keeps free): the page's lists keep that much room under their end, so every row can be
 * scrolled above it (WindowApp: `--panel-over`). */
export const panelOver = ref(0);
/** Collapsing and expanding animate for this long (ms). */
export const COLLAPSE_MS = 220;

/** What the open panel takes around a terminal's rows: its handle (9), its tab row (24), the
 * terminal's margins (6 above, 20 under), padding (16) and border (2). */
export const TAB_CHROME = 77;

/**
 * A new tab's room where the panel can't be measured (the quick switcher, another window, gives a
 * card): the window's default width less the sidebar and the panel's margins, and the panel's
 * default height less what it takes around the rows.
 */
export const TAB_ROOM = { width: 1280 - 232 - 56, height: PANEL_DEFAULT - TAB_CHROME };

/**
 * The panel's project: the one the page shown last followed (the list's selection, the Track
 * page's tower, the Sessions page's session, the Board page's card) or the one picked in the
 * panel itself. Kept while Pitwall runs; a page with no project of its own (Today, Fuel, Garage)
 * leaves it as it is.
 */
export const panelPath = ref<string | null>(null);

/** What the panel shows of a project: one of its outputs (`job`: null for the dev server, otherwise
 * a command's id) or one of its terminals. */
export type PanelTab = { kind: "output"; job: string | null } | { kind: "terminal"; id: number };

/**
 * The tab shown, per project, so coming back to a project shows the same one. Without one, the
 * project's last terminal, else its dev server's output.
 */
export const chosenTab = ref<Record<string, PanelTab>>({});

/** The terminal an output took the place of, per project: picked again, the output goes back to it. */
export const outputFrom: Record<string, number> = {};

export function chooseTab(path: string, tab: PanelTab): void {
  const was = chosenTab.value[path];
  if (tab.kind === "output" && was?.kind === "terminal") outputFrom[path] = was.id;
  chosenTab.value = { ...chosenTab.value, [path]: tab };
}

/** The keyboard is in the panel: a page that moves it from project to project holds still meanwhile. */
export const panelTyping = ref(false);

/** A tab is being opened: its shell starts at the open panel's size, so the panel stays open. */
export const panelOpening = ref(false);

/** The project whose terminal takes the keyboard once it shows (`followProject` with `focus`). */
export const panelFocus = ref<string | null>(null);

/** The tab a page followed to (`followTab`): it shows without the keyboard. */
export const followedTab = ref<number | null>(null);

/**
 * The page's project: the panel shows it. `focus`: its terminal takes the keyboard, the panel
 * open (a project picked in the list). Not while the keyboard is in the panel.
 */
export function followProject(path: string, focus = false): void {
  if (panelTyping.value) return;
  if (panelAway() && panelPath.value !== path) askAway({ kind: "follow", path }, false);
  panelFocus.value = focus ? path : null;
  panelPath.value = path;
}

/** The page's session runs in one of the project's terminals: the panel shows that tab, without
 * the keyboard. Not while the keyboard is in the panel. */
export function followTab(path: string, id: number): void {
  if (panelTyping.value) return;
  if (panelAway()) askAway({ kind: "followTab", path, id }, false);
  const chosen = chosenTab.value[path];
  if (chosen?.kind !== "terminal" || chosen.id !== id) {
    followedTab.value = id;
    chooseTab(path, { kind: "terminal", id });
  }
  panelFocus.value = null;
  panelPath.value = path;
}

/**
 * A page's part in what the panel shows: the project and, while its selection runs in one of
 * Pitwall's terminals, that tab (`target`). Followed when it changes; while the keyboard is in the
 * panel what changed waits, and comes once the keyboard leaves. Nothing else moves the panel: a
 * project picked in the panel stays until the page's selection moves on.
 */
export function usePanelFollow(target: () => { path: string; tab: number | null } | null): void {
  let behind = false;
  function apply(): void {
    const to = target();
    if (!to) return;
    if (to.tab === null) followProject(to.path);
    else followTab(to.path, to.tab);
  }
  watch(
    () => {
      const to = target();
      return to ? `${to.path}\u0000${to.tab ?? ""}` : "";
    },
    () => {
      behind = panelTyping.value;
      if (!behind) apply();
    },
    { immediate: true },
  );
  watch(panelTyping, (on) => {
    if (on || !behind) return;
    behind = false;
    apply();
  });
}

/** Something asked the panel to show a tab of its, open (an output, a session, a new tab): counted,
 * so that the press that asked doesn't fold it (TerminalPanel: a press outside). */
export const panelAsks = ref(0);

/** ⌘T, on any page: the keyboard into the panel, or with it there already, the panel folded again
 * (TerminalPanel), counted. No tab is opened for it (the user's word, 2026-11-14: "yeni sekme değil
 * terminali odaklasın"). */
export const keyboardAsks = ref(0);

/** The keyboard into the panel; in its own window, that window comes up with it. */
export function focusPanel(): void {
  if (panelAway()) return askAway({ kind: "focus" }, true);
  keyboardAsks.value++;
}

/** An output asked for (a command's row, the switcher's ⌘↵, the Garage page): its
 * project and tab in the panel, open. */
export function showOutput(path: string, job: string | null): void {
  if (panelAway()) return askAway({ kind: "output", path, job }, true);
  panelAsks.value++;
  chooseTab(path, { kind: "output", job });
  panelPath.value = path;
  terminalCollapsed.value = false;
}

/** The panel on `path`'s project, open, showing what it shows there (a project opened where its
 * terminal is the choice, with no terminal: no tab is opened for it). */
export function openPanelOn(path: string): void {
  if (panelAway()) return askAway({ kind: "open", path }, true);
  panelAsks.value++;
  panelPath.value = path;
  terminalCollapsed.value = false;
}

/** The panel folds to its tab row (a press outside it, TerminalPanel). Not while a tab is opening,
 * nor under one of the panel's own dialogs (a tab's name, a card from an output): a card folded
 * into its sidebar entry would hide it while it holds the window. */
export function foldPanel(): void {
  if (!panelOpening.value && !document.querySelector(".term-panel dialog[open]")) terminalCollapsed.value = true;
}

/**
 * A Claude session in one of Pitwall's terminals was brought up (`reveal-terminal`): the panel
 * shows that project and tab, open, the keyboard in it, on whatever page is shown. Whoever shows it
 * clears it; `at` lets one that never finds its tab lapse.
 */
export const terminalRequest = ref<{ path: string; id: number; at: number } | null>(null);

export function revealTab(path: string, id: number): void {
  if (panelAway()) return askAway({ kind: "reveal", path, id }, true);
  panelAsks.value++;
  panelPath.value = path;
  terminalRequest.value = { path, id, at: Date.now() };
}

/** A new tab asked for from a page (the Sessions page's "Open a Claude terminal", the Fuel page's
 * "Open Claude with <account>"): the panel's project becomes `path` and opens it as its "+" would;
 * `account`, with that account of the agent's; `agent`, the agent's tab it opens (none: Claude's).
 * Whoever opens it clears it. */
export const openRequest = ref<{ path: string; claude: boolean; account: string | null; agent?: Agent; at: number } | null>(null);

export function openInPanel(path: string, claude: boolean, account: string | null = null, agent: Agent = "claude"): void {
  if (panelAway()) return askAway({ kind: "new", path, claude, account, agent }, true);
  panelAsks.value++;
  panelPath.value = path;
  openRequest.value = { path, claude, account, agent, at: Date.now() };
}

/** The size a new tab gets in the panel, measured by the panel (`register`): the Board page gives
 * a card to Claude at it. */
export const panelMeasure: { size: () => { cols: number; rows: number } | null } = { size: () => null };

/**
 * A card's Claude session was brought up while the Board page shows: the page selects that card
 * (an empty id: the project's board alone). Whoever shows it clears it.
 */
export const cardRequest = ref<{ id: string; path: string } | null>(null);

/**
 * The terminals' history column (window/TerminalHistory), beside the terminal: shown or not, and
 * how wide (its handle drags it, a double click puts the default back), the same for every
 * project. Every launch starts it shown (the user's word, 2026-10-22), at the default width.
 */
export const terminalHistoryShown = ref(true);
/** The default puts the terminal's right edge on the line before a page's 380 px details: the
 * history (348), the gap before it (12) and the panel's right margin (20) take the 380. */
export const HISTORY_WIDTH = 348;
export const HISTORY_MIN = 240;
export const historyWidth = ref(HISTORY_WIDTH);

/**
 * The order the terminal tabs were dragged into, per project, by terminal id; tabs it doesn't name
 * (opened since) come after them. Kept while Pitwall runs.
 */
export const terminalOrder = ref<Record<string, number[]>>({});

/** A project's tabs as its row shows them: in the order they were dragged into (`terminalOrder`),
 * new ones last; the pinned ones before the others, each in that order. The list itself when that
 * changes nothing, so that what follows it isn't stirred for nothing. */
export function tabsInOrder<T extends { id: number; pinned: boolean }>(list: T[], order: readonly number[] | undefined): T[] {
  const rank = new Map((order ?? []).map((id, i) => [id, i]));
  const unranked = Number.MAX_SAFE_INTEGER;
  const sorted = order ? [...list].sort((a, b) => (rank.get(a.id) ?? unranked) - (rank.get(b.id) ?? unranked)) : list;
  return sorted.some((term) => term.pinned) ? [...sorted.filter((term) => term.pinned), ...sorted.filter((term) => !term.pinned)] : sorted;
}

/** A terminal looked at: its id, and when it was last on screen (ms). */
export type Looked = { id: number; at: number };

/** How many terminals `recentTabs` keeps. */
export const RECENT_MAX = 50;

/**
 * The terminals last looked at, every project's, the latest first: one goes to the front once it
 * has been on screen for a second (the tab shown, the focused pane, the panel open), and its time
 * is when it was last there. ⌃⇥ lists them (MruSwitcher). Kept in the page's memory only and handed
 * to the panel's other window as the tabs' order is; never written down.
 */
export const recentTabs = ref<Looked[]>([]);

/** `id` is looked at now: to the front. */
export function lookedAt(id: number, at = Date.now()): void {
  recentTabs.value = [{ id, at }, ...recentTabs.value.filter((each) => each.id !== id)].slice(0, RECENT_MAX);
}

/** `id` was on screen until now: its time, in its place. */
export function lookedUntil(id: number, at = Date.now()): void {
  if (recentTabs.value.some((each) => each.id === id)) recentTabs.value = recentTabs.value.map((each) => (each.id === id ? { id, at } : each));
}

/**
 * Handle behaviour for the panel: drag, arrow keys or a double click change its `height`, up to what `max()`
 * allows (the room under the page's bar) and down to what `least()` does (its panes at their least).
 */
export function useResizer(height: Ref<number>, max: () => number, least: () => number = () => PANEL_MIN) {
  let drag: { startY: number; startHeight: number; max: number } | null = null;
  /** While a drag is on, the panel follows the pointer without animating. */
  const dragging = ref(false);

  function limit(): number {
    return Math.max(least(), max());
  }

  function start(event: PointerEvent): void {
    const top = limit();
    drag = { startY: event.clientY, startHeight: Math.min(height.value, top), max: top };
    dragging.value = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    // No text is selected on the way (a chat's would be).
    event.preventDefault();
    holding(true);
  }

  function move(event: PointerEvent): void {
    if (!drag) return;
    height.value = Math.min(drag.max, Math.max(least(), drag.startHeight + (drag.startY - event.clientY)));
  }

  function end(): void {
    drag = null;
    dragging.value = false;
    holding(false);
  }

  /** Arrow keys on the handle, for keyboard users. */
  function nudge(event: KeyboardEvent): void {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const top = limit();
    const step = event.key === "ArrowUp" ? 24 : -24;
    height.value = Math.min(top, Math.max(least(), Math.min(height.value, top) + step));
  }

  /** The height before a double click took the panel to the top: the next one brings it back. */
  let before: number | null = null;

  /** A double click on the handle: the panel up to the page's bar; up there already after one, back
   * to the height it had. */
  function fill(): void {
    const top = limit();
    if (height.value >= top - 1) {
      // Up there with nothing to go back to (the page came again, Pitwall started again): the
      // height a panel opens at.
      height.value = Math.min(top, Math.max(least(), before ?? PANEL_DEFAULT));
      before = null;
      return;
    }
    before = Math.min(height.value, top);
    height.value = top;
  }

  return { start, move, end, nudge, fill, dragging };
}

/**
 * The output panes that show their errors only, by pane (a project's path, U+001F, and the
 * command's id; none for the dev server). Off for every pane at each launch.
 */
export const outputErrorsOnly = ref<Record<string, boolean>>({});
