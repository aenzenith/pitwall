import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { isLinux, isMac, isWindows, terminalChord } from "./platform";
import { api } from "./store";

/**
 * The window's terminal views. Each open project terminal gets one xterm, made the first time
 * it is shown and kept while the shell lives: switching tabs or projects, or hiding the panel,
 * only moves its element in and out of the panel.
 */

const theme: ITheme = {
  background: "#0b0c0f",
  foreground: "#c9ced6",
  cursor: "#e6e8eb",
  cursorAccent: "#0b0c0f",
  selectionBackground: "rgba(115, 201, 145, 0.28)",
  scrollbarSliderBackground: "rgba(255, 255, 255, 0.13)",
  scrollbarSliderHoverBackground: "rgba(255, 255, 255, 0.24)",
  scrollbarSliderActiveBackground: "rgba(255, 255, 255, 0.32)",
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
};

type View = {
  term: Terminal;
  fit: FitAddon;
  element: HTMLElement;
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

const options = {
  theme,
  // Geist Mono is bundled; past it (box drawing, symbols), each system's own terminal font.
  fontFamily: isMac ? '"Geist Mono Variable", ui-monospace, Menlo, monospace' : '"Geist Mono Variable", "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace',
  fontSize: 12,
  lineHeight: 1.25,
  cursorBlink: true,
  scrollback: 5000,
  macOptionIsMeta: true,
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

function create(id: number): View {
  const element = document.createElement("div");
  element.className = "terminal-host";

  const term = new Terminal(options);
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(element);

  // The panel's own keys; everything else goes to the shell. Returning false keeps a key from
  // the shell: it bubbles to the window, its default (copy, paste) left to the web view.
  //  - macOS: ⌘T belongs to the window (new tab); ⌘K clears, like Terminal.app; ⌘C and ⌘V
  //    copy and paste through the web view.
  //  - Windows, Linux: Ctrl+letter is the shell's (Ctrl+C, Ctrl+D, Ctrl+K, Ctrl+R, Ctrl+T…), so
  //    the panel's keys take Ctrl+Shift (lib/platform: terminalChord), as Windows Terminal,
  //    GNOME Terminal and Konsole do: Ctrl+Shift+T new tab, Ctrl+Shift+K clears, Ctrl+Shift+C
  //    copies the selection, Ctrl+Shift+V pastes. On Windows also Windows Terminal's own: Ctrl+C
  //    copies while text is selected (^C otherwise), Ctrl+V pastes.
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    if (isMac) {
      if (!event.metaKey) return true;
      if (event.code === "KeyK") {
        term.clear();
        return false;
      }
      return event.code !== "KeyT";
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
        return false;
      default:
        return true;
    }
  });
  term.onData((data) => void api.writeTerminal(id, data));

  const view: View = { term, fit, element, seq: 0, queue: [], cols: 0, rows: 0 };
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
  if (fit) fitView(id);
  if (focus) view.term.focus();
}

export function focusView(id: number): void {
  views.get(id)?.term.focus();
}

/** Sizes the terminal to its host and tells the shell. */
export function fitView(id: number): void {
  const view = views.get(id);
  if (!view || !view.element.isConnected) return;
  view.fit.fit();
  if (view.term.cols !== view.cols || view.term.rows !== view.rows) {
    view.cols = view.term.cols;
    view.rows = view.term.rows;
    void api.resizeTerminal(id, view.cols, view.rows);
  }
}

export function dispose(id: number): void {
  const view = views.get(id);
  if (!view) return;
  view.term.dispose();
  view.element.remove();
  views.delete(id);
}
