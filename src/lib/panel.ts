import { ref, watch, type Ref } from "vue";

/**
 * The panel along the window's bottom (window/TerminalPanel), under every page: one project's
 * outputs (the dev server, its commands) and terminals as tabs, with one height and one collapsed
 * state whatever the page. Every launch starts it collapsed, its tab row alone, and expanded at the
 * default; it can be dragged down to the minimum and up to the page's bar.
 */
/** Room for two terminal rows: tabs and a prompt stay usable. */
export const PANEL_MIN = 120;
/** Room for a Claude session. */
const PANEL_DEFAULT = 300;

export const terminalHeight = ref(PANEL_DEFAULT);
export const terminalCollapsed = ref(true);

/** A collapsed panel keeps only its handle and tab row; its height waits for its return. */
export const COLLAPSED_HEIGHT = 42;

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
  panelFocus.value = focus ? path : null;
  panelPath.value = path;
}

/** The page's session runs in one of the project's terminals: the panel shows that tab, without
 * the keyboard. Not while the keyboard is in the panel. */
export function followTab(path: string, id: number): void {
  if (panelTyping.value) return;
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

/** An output asked for (a command run or its row's button, the switcher's ⌘↵, the Garage page): its
 * project and tab in the panel, open. */
export function showOutput(path: string, job: string | null): void {
  panelAsks.value++;
  chooseTab(path, { kind: "output", job });
  panelPath.value = path;
  terminalCollapsed.value = false;
}

/** The panel on `path`'s project, open, showing what it shows there (a project opened where its
 * terminal is the choice, with no terminal: no tab is opened for it). */
export function openPanelOn(path: string): void {
  panelAsks.value++;
  panelPath.value = path;
  terminalCollapsed.value = false;
}

/** The panel folds to its tab row (a press outside it, TerminalPanel). Not while a tab is opening. */
export function foldPanel(): void {
  if (!panelOpening.value) terminalCollapsed.value = true;
}

/**
 * A Claude session in one of Pitwall's terminals was brought up (`reveal-terminal`): the panel
 * shows that project and tab, open, the keyboard in it, on whatever page is shown. Whoever shows it
 * clears it; `at` lets one that never finds its tab lapse.
 */
export const terminalRequest = ref<{ path: string; id: number; at: number } | null>(null);

export function revealTab(path: string, id: number): void {
  panelAsks.value++;
  panelPath.value = path;
  terminalRequest.value = { path, id, at: Date.now() };
}

/** A new tab asked for from a page (the Sessions page's "Open a Claude terminal"): the panel's
 * project becomes `path` and opens it as its "+" would. Whoever opens it clears it. */
export const openRequest = ref<{ path: string; claude: boolean; at: number } | null>(null);

export function openInPanel(path: string, claude: boolean): void {
  panelAsks.value++;
  panelPath.value = path;
  openRequest.value = { path, claude, at: Date.now() };
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

/**
 * Handle behaviour for the panel: drag or arrow keys change its `height`, up to what `max()`
 * allows (the room under the page's bar).
 */
export function useResizer(height: Ref<number>, max: () => number) {
  let drag: { startY: number; startHeight: number; max: number } | null = null;
  /** While a drag is on, the panel follows the pointer without animating. */
  const dragging = ref(false);

  function limit(): number {
    return Math.max(PANEL_MIN, max());
  }

  function start(event: PointerEvent): void {
    const top = limit();
    drag = { startY: event.clientY, startHeight: Math.min(height.value, top), max: top };
    dragging.value = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function move(event: PointerEvent): void {
    if (!drag) return;
    height.value = Math.min(drag.max, Math.max(PANEL_MIN, drag.startHeight + (drag.startY - event.clientY)));
  }

  function end(): void {
    drag = null;
    dragging.value = false;
  }

  /** Arrow keys on the handle, for keyboard users. */
  function nudge(event: KeyboardEvent): void {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const top = limit();
    const step = event.key === "ArrowUp" ? 24 : -24;
    height.value = Math.min(top, Math.max(PANEL_MIN, Math.min(height.value, top) + step));
  }

  return { start, move, end, nudge, dragging };
}

/**
 * The output panes that show their errors only, by pane (a project's path, U+001F, and the
 * command's id; none for the dev server). Off for every pane at each launch.
 */
export const outputErrorsOnly = ref<Record<string, boolean>>({});
