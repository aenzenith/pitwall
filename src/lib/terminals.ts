import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon, type ISearchOptions } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { listen } from "@tauri-apps/api/event";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { computed, reactive, watch } from "vue";

import { accountLabel, accountName, codexAccounts } from "./accounts";
import { t } from "./i18n";
import { isLinux, isMac, isWindows, keys, primary, terminalChord } from "./platform";
import { api } from "./store";
import { resolvedTheme, themeAt, type ResolvedTheme } from "./theme";
import type { ClaudeAccount, TerminalView } from "./types";

/**
 * The window's terminal views. Each open project terminal gets one xterm, made the first time
 * it is shown and kept while the shell lives: switching tabs or projects, or hiding the panel,
 * only moves its element in and out of the panel.
 */

/**
 * What a terminal is drawn with in each theme. xterm paints by itself and takes its colours as
 * text, so these are written out here rather than read from the tokens: the ground, the text and
 * the scrollbar are the tokens' (--bg-log, --scroll-thumb…), the sixteen colours a shell's own.
 * On the light ground each of them is dark enough to read as text; its "white" is a grey.
 */
const THEMES: Record<ResolvedTheme, ITheme> = {
  dark: {
    background: "#0b0c0f",
    foreground: "#c9ced6",
    cursor: "#e6e8eb",
    cursorAccent: "#0b0c0f",
    selectionBackground: "rgba(115, 201, 145, 0.28)",
    scrollbarSliderBackground: "rgba(255, 255, 255, 0.13)",
    scrollbarSliderHoverBackground: "rgba(255, 255, 255, 0.24)",
    scrollbarSliderActiveBackground: "rgba(255, 255, 255, 0.32)",
    overviewRulerBorder: "rgba(0, 0, 0, 0)",
    black: "#1c1f25",
    red: "#f28b8b",
    green: "#73c991",
    yellow: "#e5c07b",
    blue: "#7aa2f7",
    magenta: "#c49ef0",
    cyan: "#6fc6d9",
    white: "#c7ccd3",
    brightBlack: "#5c626c",
    brightRed: "#ff9e9e",
    brightGreen: "#8fdcaa",
    brightYellow: "#f0d08e",
    brightBlue: "#94b6ff",
    brightMagenta: "#d6b4ff",
    brightCyan: "#8ad9e8",
    brightWhite: "#f2f3f5",
  },
  light: {
    background: "#ffffff",
    foreground: "#2a2f37",
    cursor: "#1b1e24",
    cursorAccent: "#ffffff",
    selectionBackground: "rgba(23, 117, 62, 0.2)",
    scrollbarSliderBackground: "rgba(15, 17, 20, 0.2)",
    scrollbarSliderHoverBackground: "rgba(15, 17, 20, 0.34)",
    scrollbarSliderActiveBackground: "rgba(15, 17, 20, 0.46)",
    overviewRulerBorder: "rgba(0, 0, 0, 0)",
    black: "#1b1e24",
    red: "#b42a2a",
    green: "#17753e",
    yellow: "#8a6a00",
    blue: "#2f5fc4",
    magenta: "#8447c4",
    cyan: "#0f7f96",
    white: "#646b76",
    brightBlack: "#6f7680",
    brightRed: "#d13b3b",
    brightGreen: "#1f8a4c",
    brightYellow: "#9c7000",
    brightBlue: "#3b6fd9",
    brightMagenta: "#8d5bd1",
    brightCyan: "#178299",
    brightWhite: "#8a909a",
  },
};

/**
 * The least contrast text keeps with the terminal's ground. A program that colours its output
 * for a dark terminal (true colour, near white) would vanish on the light one: there xterm
 * darkens whatever falls short. On the dark ground every colour is drawn as sent.
 */
const CONTRAST: Record<ResolvedTheme, number> = { dark: 1, light: 4.5 };

/**
 * What a search marks in a terminal, in each theme: every match and the one it stands on, in the
 * output's own find colours (tokens.css: --find-bg, --find-on-bg), written out as xterm takes
 * them (#RRGGBB); beside the scrollbar, the same marks in its ruler.
 */
const FIND: Record<ResolvedTheme, NonNullable<ISearchOptions["decorations"]>> = {
  dark: { matchBackground: "#3d3718", matchOverviewRuler: "#7a6a1c", activeMatchBackground: "#7a6a1c", activeMatchColorOverviewRuler: "#f0d08e" },
  light: { matchBackground: "#fbeeb0", matchOverviewRuler: "#e0b400", activeMatchBackground: "#f2c200", activeMatchColorOverviewRuler: "#8a6a00" },
};

type View = {
  id: number;
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  /** What its search last asked xterm for (lib: `asked`), while its bar is open. */
  asked: string | null;
  element: HTMLElement;
  /** The theme it is drawn in. */
  theme: ResolvedTheme;
  /** Chunks up to this number are already drawn (or in the history being drawn). */
  seq: number;
  /** Chunks that arrive while the history is still loading. */
  queue: Array<{ seq: number; data: string }> | null;
  cols: number;
  rows: number;
};

const views = new Map<number, View>();

void listen<{ id: number; seq: number; data: string }>("terminal", (event) => {
  const view = views.get(event.payload.id);
  if (!view) return;
  if (view.queue) view.queue.push(event.payload);
  else if (event.payload.seq > view.seq) {
    view.seq = event.payload.seq;
    view.term.write(event.payload.data);
  }
});

void listen<number>("terminal-exit", (event) => dispose(event.payload));

/**
 * Draws `view` in the theme of where it stands: the app's, or the dark one inside a night
 * (lib/theme: `themeAt`; no terminal stands in one today, the Track page's follow the theme).
 * Set only when it changes: xterm draws everything anew.
 */
function paint(view: View): void {
  const theme = themeAt(view.element);
  if (view.theme === theme) return;
  view.theme = theme;
  view.term.options.theme = colours(view);
  view.term.options.minimumContrastRatio = CONTRAST[theme];
  relook(view.id);
}

/**
 * A terminal's colours in its theme. While its search is open, the match it stands on (xterm's
 * selection, which xterm paints over a search's own marks) wears the find's colour as long as the
 * keyboard is out of the terminal, in the search's field.
 */
function colours(view: View): ITheme {
  const theme = THEMES[view.theme];
  return finds.has(view.id) ? { ...theme, selectionInactiveBackground: FIND[view.theme].activeMatchBackground } : theme;
}

// The theme changed (Settings, or the system's while it is followed): every open terminal with it.
watch(resolvedTheme, () => views.forEach(paint));

const options = {
  // Geist Mono is bundled; past it (box drawing, symbols), each system's own terminal font.
  fontFamily: isMac ? '"Geist Mono Variable", ui-monospace, Menlo, monospace' : '"Geist Mono Variable", "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace',
  fontSize: 12,
  lineHeight: 1.25,
  cursorBlink: true,
  scrollback: 5000,
  macOptionIsMeta: true,
  // The search's marks are decorations, still proposed API in xterm 6.
  allowProposedApi: true,
  // Where they show beside the scrollbar: as wide as xterm's scrollbar is anyway, so nothing moves.
  overviewRuler: { width: 14 },
};

/** How many columns and rows a terminal in `host` would have, before any shell starts. */
export function measure(host: HTMLElement): { cols: number; rows: number } | null {
  const probe = document.createElement("div");
  probe.style.cssText = "position: absolute; inset: 0; visibility: hidden";
  host.appendChild(probe);
  const term = new Terminal(options);
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(probe);
  const size = fit.proposeDimensions();
  term.dispose();
  probe.remove();
  return size && size.cols > 0 && size.rows > 0 ? { cols: size.cols, rows: size.rows } : null;
}

/** Windows and Linux: the terminal's selection to the clipboard (the core's, as the web view's
 * own copy needs a copy event these keys don't make), and the selection goes. */
function copySelection(term: Terminal, event: KeyboardEvent): void {
  event.preventDefault();
  if (!term.hasSelection()) return;
  void api.copyText(term.getSelection());
  term.clearSelection();
}

/**
 * A web address in a terminal: one it printed (`WebLinksAddon`) or a program's own link (OSC 8),
 * http and https only. ⌘-click (Ctrl-click on Windows and Linux) opens it in the browser through
 * the core (`open_url`), as Terminal.app, iTerm and VS Code do; a plain click stays the
 * terminal's, so selecting the address (a double-click too) never opens it. Over it, a tooltip
 * says so.
 */
function openLink(event: MouseEvent, uri: string): void {
  if (primary(event) && /^https?:\/\//i.test(uri)) void api.openUrl(uri);
}

/** The tooltip over a link of the terminal in `element`. */
function linkHint(element: HTMLElement): { hover: () => void; leave: () => void } {
  return {
    hover() {
      element.title = t("terminal.openLink", { key: keys("mod") });
    },
    leave() {
      element.removeAttribute("title");
    },
  };
}

/** The panel's keys for its panes (lib/panes: `paneAction`), as the panel claims them now: kept
 * from the shell, they bubble to the panel. */
export const paneKeys: { claim: (event: KeyboardEvent) => boolean } = { claim: () => false };

function create(id: number): View {
  const element = document.createElement("div");
  element.className = "terminal-host";

  // The app's theme for now; `attach` looks again once it stands in its panel.
  const theme = resolvedTheme.value;
  const hint = linkHint(element);
  const term = new Terminal({ ...options, theme: THEMES[theme], minimumContrastRatio: CONTRAST[theme], linkHandler: { activate: openLink, ...hint } });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new WebLinksAddon(openLink, hint));
  const search = new SearchAddon({ highlightLimit: FIND_LIMIT });
  term.loadAddon(search);
  search.onDidChangeResults(({ resultIndex, resultCount }) => {
    const find = finds.get(id);
    if (!find) return;
    find.index = resultIndex;
    find.count = resultCount;
  });
  term.open(element);

  // The panel's own keys; everything else goes to the shell. Returning false keeps a key from
  // the shell: it bubbles to the window, its default (copy, paste) left to the web view.
  //  - macOS: ⌘T and ⌘W belong to the window (new tab, close tab); ⌘K clears, like Terminal.app;
  //    ⌘C and ⌘V copy and paste through the web view.
  //  - The search's keys (`findKey`) are the search's, here and nowhere else.
  //  - Windows, Linux: Ctrl+letter is the shell's (Ctrl+C, Ctrl+D, Ctrl+K, Ctrl+R, Ctrl+T…), so
  //    the panel's keys take Ctrl+Shift (lib/platform: terminalChord), as Windows Terminal,
  //    GNOME Terminal and Konsole do: Ctrl+Shift+T new tab, Ctrl+Shift+W closes it, Ctrl+Shift+K
  //    clears, Ctrl+Shift+C copies the selection, Ctrl+Shift+V pastes. On Windows also Windows
  //    Terminal's own: Ctrl+C copies while text is selected (^C otherwise), Ctrl+V pastes.
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    if (paneKeys.claim(event)) return false;
    const find = findKey(event);
    if (find === "open" || (find && finds.has(id))) {
      event.preventDefault();
      event.stopPropagation();
      if (find === "open") openFind(id);
      else findStep(id, find);
      return false;
    }
    if (isMac) {
      if (!event.metaKey) return true;
      if (event.code === "KeyK") {
        term.clear();
        relook(id);
        return false;
      }
      return event.code !== "KeyT" && event.code !== "KeyW";
    }
    const plainCtrl = event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey;
    if (isWindows && plainCtrl && event.code === "KeyC" && term.hasSelection()) {
      copySelection(term, event);
      return false;
    }
    // Ctrl+V: the web view's paste, which xterm takes as typed text.
    if (isWindows && plainCtrl && event.code === "KeyV") return false;
    if (!terminalChord(event)) return true;
    switch (event.code) {
      case "KeyK":
        event.preventDefault();
        term.clear();
        relook(id);
        return false;
      case "KeyC":
        copySelection(term, event);
        return false;
      case "KeyV":
        // WebView2 pastes on Ctrl+Shift+V by itself; WebKitGTK doesn't, so the clipboard is read.
        if (isLinux) {
          event.preventDefault();
          void navigator.clipboard?.readText().then((text) => text && term.paste(text), () => undefined);
        }
        return false;
      case "KeyT":
      case "KeyW":
        return false;
      default:
        return true;
    }
  });
  term.onData((data) => void api.writeTerminal(id, data));

  const view: View = { id, term, fit, search, asked: null, element, theme, seq: 0, queue: [], cols: 0, rows: 0 };
  views.set(id, view);

  // History first; chunks that arrived meanwhile are kept only if they are newer than it.
  void api.terminalBuffer(id).then((buffer) => {
    term.write(buffer.data);
    view.seq = buffer.seq;
    for (const chunk of view.queue ?? []) {
      if (chunk.seq > view.seq) {
        view.seq = chunk.seq;
        term.write(chunk.data);
      }
    }
    view.queue = null;
  });

  return view;
}

/**
 * Puts the terminal into `host` and, unless `focus` is false (the keyboard is picking tabs),
 * focuses it. With `fit` it is also sized to the host; a collapsed panel passes false, so the
 * shell isn't squeezed to its tab row.
 */
export function attach(id: number, host: HTMLElement, fit = true, focus = true): void {
  const view = views.get(id) ?? create(id);
  if (view.element.parentElement !== host) {
    host.replaceChildren(view.element);
  }
  paint(view);
  if (fit) fitView(id);
  if (focus) view.term.focus();
}

export function focusView(id: number): void {
  views.get(id)?.term.focus();
}

/** Sizes the terminal to its host and tells the shell. */
export function fitView(id: number, tell = true): void {
  const view = views.get(id);
  if (!view || !view.element.isConnected) return;
  view.fit.fit();
  // A size still on its way (the history sliding in beside it): the view follows it frame by
  // frame, the shell hears only the size it ends at.
  if (!tell) return;
  if (view.term.cols !== view.cols || view.term.rows !== view.rows) {
    view.cols = view.term.cols;
    view.rows = view.term.rows;
    void api.resizeTerminal(id, view.cols, view.rows);
  }
}

export function dispose(id: number): void {
  const view = views.get(id);
  if (!view) return;
  finds.delete(id);
  view.term.dispose();
  view.element.remove();
  views.delete(id);
}

/* ---------- ⌘F: a terminal's search ---------- */

/** How many matches are marked and counted; past them the count reads "1000+". */
export const FIND_LIMIT = 1000;

/**
 * A terminal's search, while its bar is open: what is looked for, the match it stands on (from 0;
 * -1 with none, or one past the counted ones) of how many, whether the pattern doesn't parse, and
 * whether the bar's field is to take the keyboard.
 */
export type TerminalFind = { query: string; index: number; count: number; invalid: boolean; focus: boolean };

/** The searches open, by terminal. In memory only, as the terminal's lines are. */
export const finds = reactive(new Map<number, TerminalFind>());

/** Match case, whole word, regular expression: one choice for every terminal, until Pitwall quits. */
export const findOptions = reactive({ caseSensitive: false, wholeWord: false, regex: false });

export type FindStep = "next" | "previous";

/**
 * The search's keys, as VS Code's terminal has them: ⌘F opens it (Ctrl+Shift+F on Windows and
 * Linux, where Ctrl+F is the shell's), ⌘G and ⇧⌘G (F3 and Shift+F3) step down and up.
 */
export function findKey(event: KeyboardEvent): "open" | FindStep | null {
  if (isMac) {
    if (!event.metaKey || event.ctrlKey || event.altKey) return null;
    if (event.code === "KeyF") return event.shiftKey ? null : "open";
    return event.code === "KeyG" ? (event.shiftKey ? "previous" : "next") : null;
  }
  if (event.key === "F3" && !event.ctrlKey && !event.altKey && !event.metaKey) return event.shiftKey ? "previous" : "next";
  return terminalChord(event) && event.code === "KeyF" ? "open" : null;
}

/** The characters xterm's search takes for no part of a word, escaped for a character class. */
const WORD_EDGE = " ~!@#$%^&*()+`-=[]{}|\\;:\"',./<>?".replace(/[\\\]^-]/g, "\\$&");

/** Whether this WebKit has lookbehind (Safari 16.4 and later). */
const LOOKBEHIND = (() => {
  try {
    return new RegExp("(?<!a)b").test("b");
  } catch {
    return false;
  }
})();

/**
 * What xterm is asked to look for: the query, and as a regular expression the choices make it.
 * Whole word goes as the word between lookarounds of xterm's own word edges, since xterm's own
 * whole-word search tests only a line's first match (`laps … {lap}` finds no `lap`); without
 * lookbehind, xterm's own. Null for a regular expression that doesn't parse (xterm would throw).
 * `edged`: asked with those lookarounds.
 */
function asked(query: string): { term: string; options: ISearchOptions; edged: boolean } | null {
  const { caseSensitive, wholeWord, regex } = findOptions;
  let term = query;
  let options: ISearchOptions = { caseSensitive, wholeWord, regex };
  const edged = wholeWord && LOOKBEHIND;
  if (edged) {
    const body = regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    term = `(?<![^${WORD_EDGE}])(?:${body})(?![^${WORD_EDGE}])`;
    options = { caseSensitive, wholeWord: false, regex: true };
  }
  if (options.regex) {
    try {
      new RegExp(term, "g");
    } catch {
      return null;
    }
  }
  return { term, options, edged };
}

/** Looks for the query from the match the search stands on (the selection), up (`previous`, to
 * older lines) or down, round at the ends; with none, from the newest line up. */
function look(id: number, step: FindStep): void {
  const view = views.get(id);
  const find = finds.get(id);
  if (!view || !find) return;
  const ask = find.query ? asked(find.query) : null;
  find.invalid = !!find.query && !ask;
  if (!ask) {
    view.asked = null;
    view.search.clearDecorations();
    view.term.clearSelection();
    find.index = -1;
    find.count = 0;
    return;
  }
  // xterm hands a regular expression the part of a line from the selection on, or up to it, so
  // whole word's lookarounds take the cut for a word's edge: asked anew, it starts from the newest
  // line rather than from the selection another query made. Stepping from a match it found cuts at
  // that match's own edges, which are a word's.
  const key = `${ask.term}\u0000${ask.options.caseSensitive}`;
  if (ask.edged && key !== view.asked) view.term.clearSelection();
  view.asked = key;
  const options: ISearchOptions = { ...ask.options, decorations: FIND[view.theme] };
  if (step === "next") view.search.findNext(ask.term, options);
  else view.search.findPrevious(ask.term, options);
}

/** Marks drawn anew where the search stands: the theme or a choice changed, or the terminal was
 * cleared. */
function relook(id: number): void {
  const view = views.get(id);
  if (!view || !finds.get(id)?.query) return;
  view.search.clearDecorations();
  look(id, "previous");
}

/** ⌘F: the terminal's search bar, its field taking the keyboard. Text selected in the terminal,
 * on one line, is what it looks for. */
export function openFind(id: number): void {
  const view = views.get(id);
  if (!view) return;
  const picked = view.term.hasSelection() ? view.term.getSelection() : "";
  const seed = picked.trim() && !picked.includes("\n") ? picked : null;
  const find = finds.get(id);
  if (!find) {
    finds.set(id, { query: seed ?? "", index: -1, count: 0, invalid: false, focus: true });
    view.term.options.theme = colours(view);
  } else {
    find.focus = true;
    if (seed === null || seed === find.query) return;
    find.query = seed;
  }
  if (seed !== null) look(id, "previous");
}

/** The search closes: its marks go, the match it stood on stays selected (⌘C copies it), and the
 * keyboard goes back to the terminal. */
export function closeFind(id: number): void {
  finds.delete(id);
  const view = views.get(id);
  if (!view) return;
  view.asked = null;
  view.search.clearDecorations();
  view.term.options.theme = colours(view);
  view.term.focus();
}

/** Typed: looked for from where the search stands, so each letter narrows the same match (with
 * whole word, from the newest line: `look`). */
export function findQuery(id: number, query: string): void {
  const find = finds.get(id);
  if (!find) return;
  find.query = query;
  look(id, "previous");
}

/** To the match below (`next`) or above. */
export function findStep(id: number, step: FindStep): void {
  if (finds.get(id)?.query) look(id, step);
}

export function toggleFindOption(option: keyof typeof findOptions): void {
  findOptions[option] = !findOptions[option];
  for (const id of finds.keys()) relook(id);
}

/* ---------- Codex's accounts, for its tabs ---------- */

/** Codex's accounts (lib/accounts `codexAccounts`: the Fuel payload's, as the core sends them while
 * Codex is on), the main one first, then each added in the order added, the one new Codex
 * sessions use marked; none sent yet is the main account alone. */
export { codexAccounts };

/** More than Codex's main account: "+"'s Codex account section and a Codex tab's badge show only
 * then. */
export const severalCodexAccounts = computed(() => codexAccounts.value.length > 1);

export function codexAccountName(account: ClaudeAccount): string {
  return accountName(account);
}

/** The name a tab shows beside its own: the account its agent runs with (a Codex tab's a Codex
 * account, any other's a Claude account), when it is another than the main one and still listed. */
export function tabAccountLabel(view: TerminalView | undefined): string | null {
  if (view?.kind !== "codex") return accountLabel(view?.account);
  const account = view.account ? codexAccounts.value.find((each) => each.id === view.account) : null;
  return account && !account.main ? codexAccountName(account) : null;
}

/** A terminal's tooltip, on its tab and its pane's head: what they show short said whole (the
 * account it runs with, an administrator's shield), then how to rename it. */
export function terminalTip(view: TerminalView | undefined): string {
  const account = tabAccountLabel(view);
  return [account && t("terminal.accountTip", { name: account }), view?.elevated && t("terminal.adminTip"), t("terminal.renameHint")]
    .filter(Boolean)
    .join("\n");
}
