// The Files window's way to the chat (ui.ts: `chat-attach`, `chat-context`). Only where a file
// is and which lines are picked go to the main window, a path relative to the project as the
// window names it (what a mention says: `@path`): never a line of the file's text.
//
// - "Sohbete ekle" (a file's menu in the tree and on its tab, the code's own menu, ⌥K) sends
//   `chat-attach`: the file, with the lines picked in the code when there are.
// - `chat-context` follows the file a Files window shows, with the lines picked in its code, a
//   moment after either changes. There is one in force for the window: the last sent, by the
//   window last changed or brought to the front. Null once the window that sent it shows no file
//   any more (all its tabs closed, a commit shown) or closes; a window folded into the sidebar
//   keeps its own, as it keeps its tabs.

import { emitTo } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from "vue";

import { agentOn } from "../agents";
import { COMMIT_TAB, peekFiles, type FilesWin } from "../explorer";
import { t } from "../i18n";
import { useNativeMenu } from "../nativeMenu";
import { api } from "../store";
import { CHAT_ATTACH, CHAT_CONTEXT, type ChatAttach, type ChatContext } from "./ui";

type Lines = { start: number; end: number };

/** "Sohbete ekle": the file, and the lines picked of it, to the project's chat. */
export function attachToChat(project: string, file: string, lines: Lines | null = null): void {
  const attach: ChatAttach = { project, path: file, ...(lines ? { startLine: lines.start, endLine: lines.end } : {}) };
  void emitTo("main", CHAT_ATTACH, attach).catch(() => undefined);
}

/** A file's line of the code view a node is in. */
function lineOf(node: Node): HTMLElement | null {
  return (node instanceof Element ? node : node.parentElement)?.closest<HTMLElement>("[data-line]") ?? null;
}

/** The lines a selection in the code covers; null when it doesn't start and end on a line (the
 * whole text picked with ⌘A: the file, no lines). A selection that ends at the very start of a line
 * (a line picked whole with a triple click) ends on the line before. */
function linesOf(range: Range): Lines | null {
  const first = lineOf(range.startContainer);
  const last = lineOf(range.endContainer);
  if (!first || !last) return null;
  const start = Number(first.dataset.line);
  let end = Number(last.dataset.line);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
  const code = last.querySelector(".code");
  if (end > start && code) {
    const before = document.createRange();
    before.setStart(code, 0);
    // An end before the line's code (its number) leaves the range empty, as does one at its start.
    before.setEnd(range.endContainer, range.endOffset);
    if (!before.toString()) end -= 1;
  }
  return { start, end: Math.max(start, end) };
}

/* ---------- the context in force ---------- */

/** The window (its project) that sent the context in force; null when none is. */
let sentBy: string | null = null;
let lastSent = "null";

function send(from: string, context: ChatContext): void {
  if (context === null) {
    // Another window's context stays in force.
    if (sentBy !== from) return;
    sentBy = null;
  } else sentBy = from;
  const text = JSON.stringify(context);
  if (text === lastSent) return;
  lastSent = text;
  void emitTo("main", CHAT_CONTEXT, context).catch(() => undefined);
}

/** How long the file and lines rest before they are sent. */
const SETTLE_MS = 150;

/**
 * A Files window's link to the chat: its context sent as it changes, and what it offers.
 * `root`: the window's own element. `key` is for the window's keydown (⌥K: true when it took the
 * key); `menu` for a right-click in it (only the code's own text is answered).
 */
export function useChatLink(win: FilesWin, root: Ref<HTMLElement | null>): { key: (event: KeyboardEvent) => boolean; menu: (event: MouseEvent) => void } {
  const lines = ref<Lines | null>(null);
  const file = computed(() => {
    const active = peekFiles(win.path)?.active ?? null;
    return active && !active.startsWith(COMMIT_TAB) ? active : null;
  });
  // Another file shown: what was picked was another's.
  watch(file, () => (lines.value = null));

  /** The selection, when it is in this window's code: a click there (nothing picked) lets the
   * lines go. One made anywhere else (the chat's own box, the find field) leaves them as they were. */
  function read(): void {
    const code = root.value?.querySelector(".code-view .lines");
    const selection = window.getSelection();
    if (!code || !selection?.rangeCount) return;
    const range = selection.getRangeAt(0);
    if (!code.contains(range.startContainer)) return;
    lines.value = selection.isCollapsed ? null : linesOf(range);
  }

  const context = computed<ChatContext>(() => {
    if (!win.open || !file.value || !agentOn("claude")) return null;
    return { project: win.path, path: file.value, ...(lines.value ? { startLine: lines.value.start, endLine: lines.value.end } : {}) };
  });

  let timer = 0;
  watch(
    context,
    (next) => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => send(win.path, next), SETTLE_MS);
    },
    { immediate: true },
  );

  // Brought to the front, its context is the one in force again.
  watch(
    () => win.z,
    () => {
      if (context.value) send(win.path, context.value);
    },
  );

  onMounted(() => document.addEventListener("selectionchange", read));
  onBeforeUnmount(() => {
    document.removeEventListener("selectionchange", read);
    window.clearTimeout(timer);
    send(win.path, null);
  });

  /** ⌥K, as Claude Code's own editor extension has it: the file shown, with its lines picked. */
  function key(event: KeyboardEvent): boolean {
    if (!event.altKey || event.metaKey || event.ctrlKey || event.shiftKey || event.code !== "KeyK") return false;
    if ((event.target as HTMLElement).closest("input, textarea") || !file.value || !agentOn("claude")) return false;
    event.preventDefault();
    read();
    attachToChat(win.path, file.value, lines.value);
    return true;
  }

  const native = useNativeMenu();

  /** A right-click on the code's text, while Claude is on: "Sohbete ekle" (the lines picked, else
   * the line clicked's file) and, with text picked, Copy. Elsewhere the window's own menus answer. */
  function menu(event: MouseEvent): void {
    if (event.defaultPrevented || !agentOn("claude") || !file.value) return;
    if (!(event.target as HTMLElement).closest(".code-view .lines")) return;
    event.preventDefault();
    read();
    const shown = file.value;
    const picked = lines.value;
    const selection = window.getSelection();
    const text = selection && !selection.isCollapsed ? selection.toString() : "";
    // ⌘A picks the whole text, drawn or not: Copy copies the file's (CodeView: `onCopy`).
    const whole = !!selection?.rangeCount && selection.getRangeAt(0).startContainer === root.value?.querySelector(".code-view .lines");
    const view = peekFiles(win.path)?.opened.get(shown)?.view;
    const copied = whole && view?.kind === "text" ? view.text : text;
    const label = !picked
      ? t("chatSurface.files.attach")
      : picked.start === picked.end
        ? t("chatSurface.files.attachLine", { line: picked.start })
        : t("chatSurface.files.attachLines", { start: picked.start, end: picked.end });
    void native.popup(event, [
      { text: label, action: () => attachToChat(win.path, shown, picked) },
      ...(copied ? ["separator" as const, { text: t("chatSurface.files.copy"), action: () => void api.copyText(copied) }] : []),
    ]);
  }

  return { key, menu };
}
