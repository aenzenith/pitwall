import { getCurrentWindow } from "@tauri-apps/api/window";
import { watch } from "vue";

import { depJobs } from "./deps";
import { shellRequest } from "./flag";
import {
  cardRect,
  chosenTab,
  focusPanel,
  followProject,
  followTab,
  historyWidth,
  openInPanel,
  openPanelOn,
  outputErrorsOnly,
  outputFrom,
  panelPath,
  panelStand,
  revealTab,
  showOutput,
  terminalHistoryShown,
  terminalOrder,
  type PanelAsk,
  type PanelStand,
  type PanelTab,
} from "./panel";
import { fromCore, paneLayouts, panesHeight, toCore } from "./panes";
import { api, snapshot } from "./store";
import type { TerminalLayout } from "./types";

/**
 * Where the panel stands, and what goes with it from one window to the other: the main window's
 * bottom or a card over its page, or the terminals' own window (TerminalsApp). The terminals stay
 * the core's; what the panel shows of them (the project, the tab picked, the tabs' order, the
 * panes, the history column) is the page's, so it is handed over as the panel moves, through the
 * core's memory only (`set_terminal_place`, `keep_panel_state`, `panel-state`).
 */

/** What the panel shows, as one page hands it to the other. */
type PanelState = {
  path: string | null;
  chosen: Record<string, PanelTab>;
  from: Record<string, number>;
  order: Record<string, number[]>;
  historyWidth: number;
  historyShown: boolean;
  errorsOnly: Record<string, boolean>;
  layouts: Record<string, TerminalLayout>;
  panesHeight: number;
  jobs: Record<string, string[]>;
};

/** The panel as this page shows it, to hand over. */
export function panelState(): string {
  const layouts: Record<string, TerminalLayout> = {};
  for (const [path, layout] of Object.entries(paneLayouts.value)) layouts[path] = toCore(layout);
  const state: PanelState = {
    path: panelPath.value,
    chosen: chosenTab.value,
    from: { ...outputFrom },
    order: terminalOrder.value,
    historyWidth: historyWidth.value,
    historyShown: terminalHistoryShown.value,
    errorsOnly: outputErrorsOnly.value,
    layouts,
    panesHeight: panesHeight.value,
    jobs: depJobs.value,
  };
  return JSON.stringify(state);
}

/** The panel as the other page left it, taken here. One that doesn't read leaves this as it is. */
export function takePanelState(text: string | null | undefined): void {
  if (!text) return;
  let state: Partial<PanelState>;
  try {
    state = JSON.parse(text) as Partial<PanelState>;
  } catch {
    return;
  }
  if (typeof state.path === "string" || state.path === null) panelPath.value = state.path ?? null;
  if (state.chosen) chosenTab.value = state.chosen;
  if (state.from) {
    for (const key of Object.keys(outputFrom)) delete outputFrom[key];
    Object.assign(outputFrom, state.from);
  }
  if (state.order) terminalOrder.value = state.order;
  if (typeof state.historyWidth === "number") historyWidth.value = state.historyWidth;
  if (typeof state.historyShown === "boolean") terminalHistoryShown.value = state.historyShown;
  if (state.errorsOnly) outputErrorsOnly.value = state.errorsOnly;
  if (typeof state.panesHeight === "number") panesHeight.value = state.panesHeight;
  if (state.jobs) depJobs.value = { ...depJobs.value, ...state.jobs };
  // A pane's name is the page's own: each page names the panes it is given afresh.
  if (state.layouts) {
    const next: typeof paneLayouts.value = {};
    for (const [path, layout] of Object.entries(state.layouts)) {
      try {
        next[path] = fromCore(layout);
      } catch {
        // A layout that doesn't read: that project shows one terminal at a time.
      }
    }
    paneLayouts.value = next;
  }
}

/** A change asked for here that the core's snapshots don't show yet: they would take it back. */
let asked: { stand: PanelStand; at: number } | null = null;
/** How long one waits for the core to show it before the core's word wins again. */
const ASKED_MS = 3_000;

/** Follows the core's word on where the panel stands, and the card's place, in every snapshot. */
export function followStand(): void {
  let cardTaken = false;
  watch(
    () => snapshot.value?.settings.terminalPlace,
    (place) => {
      if (!snapshot.value) return;
      const mode = place?.mode === "float" || place?.mode === "window" ? place.mode : "dock";
      if (asked && asked.stand !== mode && Date.now() - asked.at < ASKED_MS) return;
      asked = null;
      panelStand.value = mode;
      // The card's place is this page's once taken: the core only keeps it for the next start.
      if (!cardTaken) {
        cardTaken = true;
        const card = place?.card;
        if (card) cardRect.value = { right: card[0], bottom: card[1], width: card[2], height: card[3] };
      }
    },
    { immediate: true },
  );
}

/**
 * The panel to `stand`, here at once and in the core, which opens or closes the terminals' window.
 * Going to that window or coming back from it, the panel's state goes along. `at`: where on the
 * screen the card was let go, the window's place.
 */
export function standPanel(stand: PanelStand, at: [number, number] | null = null): void {
  const from = panelStand.value;
  if (stand === from && stand !== "window") return;
  asked = { stand, at: Date.now() };
  // The terminals' page stays what it is until the core closes its window.
  if (document.documentElement.dataset.surface !== "terminals") panelStand.value = stand;
  const handed = stand === "window" || from === "window" ? panelState() : null;
  void api.setTerminalPlace(stand, handed, at).catch(() => {
    asked = null;
    panelStand.value = from;
  });
}

/** The card's place, kept for the next start a moment after it was left. */
let cardKept: ReturnType<typeof setTimeout> | undefined;
export function keepCard(): void {
  clearTimeout(cardKept);
  cardKept = setTimeout(() => {
    const card = cardRect.value;
    void api.setTerminalCard(card ? [card.right, card.bottom, card.width, card.height].map(Math.round) as [number, number, number, number] : null).catch(() => undefined);
  }, 400);
}

/**
 * The main window's page: the panel's state as the terminals' window left it, whenever the panel
 * comes back from there (`panel-state`). It comes back open.
 */
export async function listenPanelState(open: () => void): Promise<() => void> {
  return getCurrentWindow().listen<string>("panel-state", (event) => {
    takePanelState(event.payload);
    open();
  });
}

/**
 * The terminals' window's page: what the main window's pages ask of the panel (`panel-ask`), done
 * as it would be done there.
 */
export async function listenPanelAsks(): Promise<() => void> {
  return getCurrentWindow().listen<PanelAsk | string>("panel-ask", (event) => {
    let ask: PanelAsk;
    try {
      ask = typeof event.payload === "string" ? (JSON.parse(event.payload) as PanelAsk) : event.payload;
    } catch {
      return;
    }
    if (ask && typeof ask === "object") doAsk(ask);
  });
}

function doAsk(ask: PanelAsk): void {
  switch (ask.kind) {
    case "follow":
      return followProject(ask.path);
    case "followTab":
      return followTab(ask.path, ask.id);
    case "output":
      return showOutput(ask.path, ask.job);
    case "open":
      return openPanelOn(ask.path);
    case "reveal":
      return revealTab(ask.path, ask.id);
    case "new":
      return openInPanel(ask.path, ask.claude, ask.account, ask.agent);
    case "shell":
      shellRequest.value = { path: ask.path, at: Date.now() };
      return;
    case "focus":
      return focusPanel();
  }
}

/** The terminals' window's page: its panel as it is now, kept by the core for the main window, a
 * moment after anything in it changed. */
export function keepPanelState(): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const stop = watch(
    [panelPath, chosenTab, terminalOrder, historyWidth, terminalHistoryShown, outputErrorsOnly, paneLayouts, panesHeight, depJobs],
    () => {
      clearTimeout(timer);
      timer = setTimeout(() => void api.keepPanelState(panelState()).catch(() => undefined), 250);
    },
    { deep: true },
  );
  return () => {
    clearTimeout(timer);
    stop();
  };
}
