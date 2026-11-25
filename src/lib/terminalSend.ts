// The terminal panel's send window (window/TerminalSend.vue): one message typed once, sent to the
// tabs picked in its list, the project's or every project's. Which tabs are picked, which list is
// shown, what is typed and what came of the last send stay in the page's memory only, per project
// the panel shows, until the app quits: never written down. The core judges each tab right before
// it writes to it (`send_to_tabs`); what the window shows locked beforehand is only a preview.

import { reactive } from "vue";

import { peekChat } from "./chat/store";
import { agentKey } from "./format";
import { t } from "./i18n";
import { tabsInOrder, terminalOrder } from "./panel";
import { isMac } from "./platform";
import { sessions, snapshot } from "./store";
import type { TabState } from "./tabStates";
import { distinctNames } from "./tabs";
import type { SessionRow, TerminalView } from "./types";

/** Why the core gave a tab nothing (`SendSkip` in `core/send.rs`). */
export type SendSkip = "gone" | "laps" | "admin" | "dialog" | "unknown" | "ended" | "busy" | "failed";
export type SendResult = { id: number; skipped: SendSkip | null };

/** A tab left out, as the panel's line names it: the tab's name (with its project's, when it is
 * another project's) and why, never the text; `path`, its project, for Show. */
export type Skipped = { id: number; path: string; name: string; why: string };

/** What the last send came to: how many tabs took it, and those left out. */
export type SendOutcome = { sent: number; skipped: Skipped[] };

/** Which tabs the window lists: the panel's project's, or every project's. */
export type SendScope = "project" | "all";

/** One project's window: open or not, its list, the tabs picked (any project's), what is typed and
 * what came of the last send, which the panel's line shows once the window is gone. */
export type SendBox = { on: boolean; scope: SendScope; picked: Set<number>; draft: string; outcome: SendOutcome | null };

const boxes = reactive(new Map<string, SendBox>());

/** The window of the project at `path`, closed until it is opened. */
export function sendBox(path: string): SendBox {
  let box = boxes.get(path);
  if (!box) {
    boxes.set(path, { on: false, scope: "project", picked: new Set(), draft: "", outcome: null });
    box = boxes.get(path)!;
  }
  return box;
}

/** A tab as the window lists it: its name as its row shows it. */
export type SendRow = { term: TerminalView; name: string };

/** A project's tabs in the window: the agents' first (Claude's, Codex's, the chats), then the
 * shells, each in the row's order. */
export type SendGroup = { path: string; name: string; agents: SendRow[]; shells: SendRow[] };

/** The tabs the window lists for the panel showing `path`: that project's, or with `all` those of
 * every project that has some, that project first. Pitwall's own tabs only (the laps' are in no
 * project's list). Reactive: read inside a `computed`. */
export function sendGroups(path: string, scope: SendScope): SendGroup[] {
  const projects = snapshot.value?.projects ?? [];
  const own = projects.filter((project) => project.path === path);
  const shown = scope === "project" ? own : [...own, ...projects.filter((project) => project.path !== path)];
  return shown
    .filter((project) => project.terminals.length)
    .map((project) => {
      const terms = tabsInOrder(project.terminals, terminalOrder.value[project.path]);
      const names = distinctNames(terms);
      const rows = terms.map((term) => ({ term, name: names.get(term.id) ?? term.name }));
      return { path: project.path, name: project.name, agents: rows.filter((row) => row.term.kind !== "shell"), shells: rows.filter((row) => row.term.kind === "shell") };
    });
}

/** How many tabs each list holds, for its button. Reactive. */
export function sendCounts(path: string): Record<SendScope, number> {
  const projects = snapshot.value?.projects ?? [];
  return {
    project: projects.find((project) => project.path === path)?.terminals.length ?? 0,
    all: projects.reduce((sum, project) => sum + project.terminals.length, 0),
  };
}

/** What a row says under a tab's name that isn't locked: its agent's state as the tab shows it,
 * else what the tab is (a shell, a quiet chat or agent). */
export function rowState(term: TerminalView, live: Map<number, TabState>): string {
  const state = live.get(term.id);
  if (state) return state.text;
  if (term.kind === "shell") return t("terminal.empty.shell");
  if (term.kind === "chat") return `${t("terminal.empty.chat")} · ${t("sessions.status.idleNow")}`;
  return t("sessions.status.idleNow");
}

/** The window's key: ⇧⌘I, on Windows and Linux Ctrl+Shift+I (the Web Inspector's there, which no
 * window of Pitwall's opens). */
export function sendKey(event: KeyboardEvent): boolean {
  if (event.code !== "KeyI" || !event.shiftKey || event.altKey) return false;
  return isMac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
}

/** The key as tooltips name it. */
export const SEND_KEY = isMac ? "⇧⌘I" : "Ctrl+Shift+I";

/** A tab of an agent's (Claude's, Codex's, a chat) or a shell's: the window first picks the tabs
 * of the kind the tab in front is. */
function group(term: TerminalView): "agent" | "shell" {
  return term.kind === "shell" ? "shell" : "agent";
}

/** The tabs picked as the window opens: the project's of the same kind as the tab in front (agents
 * with an agent's, shells with a shell's; agents when none is), less those locked. */
export function firstPick(terms: TerminalView[], front: TerminalView | null, locks: Map<number, string>): Set<number> {
  const kind = front ? group(front) : "agent";
  return new Set(terms.filter((term) => group(term) === kind && !locks.has(term.id)).map((term) => term.id));
}

/** Why a tab with a session waiting on the user takes nothing, as the panel can tell beforehand:
 * a permission prompt (its tool by its name) or a question; none for a finished turn. */
export function waitLock(row: SessionRow): string | null {
  if (row.phase !== "waiting" || !row.turn || row.turn.kind === "finished") return null;
  if (row.turn.kind === "asking") return t("terminal.send.why.asking");
  return row.tool ? t("terminal.send.why.permissionTool", { tool: row.tool }) : t("terminal.send.why.permission");
}

/** The tabs of `terms` that take nothing now, as far as the panel can tell, and why: an agent with
 * a permission prompt or a question open (the sessions' rows, a chat's own cards), an
 * administrator's shell. Whatever else the core finds (an agent ended, a command running, a state
 * it can't read) it says after the send. Reactive: read inside a `computed`. */
export function sendLocks(terms: readonly TerminalView[]): Map<number, string> {
  const locks = new Map<number, string>();
  const ids = new Set(terms.map((term) => term.id));
  for (const row of sessions.value?.sessions ?? []) {
    if (row.origin.kind !== "pitwall" || !ids.has(row.origin.terminal)) continue;
    const why = waitLock(row);
    if (why) locks.set(row.origin.terminal, why);
  }
  for (const term of terms) {
    if (term.elevated) locks.set(term.id, t("terminal.send.why.admin"));
    else if (term.kind === "chat" && !locks.has(term.id)) {
      const chat = peekChat(term.id);
      const waits = (chat?.state ?? term.chat?.state) === "requires_action" || (chat?.pending.length ?? 0) > 0;
      if (waits) locks.set(term.id, t("terminal.send.why.dialog"));
    }
  }
  return locks;
}

/** Why the core left a tab out, in the window's words; `kind`: the tab's, for whose agent ended. */
export function skipWhy(skip: SendSkip, kind: TerminalView["kind"] | undefined): string {
  switch (skip) {
    case "ended":
      return t(agentKey("terminal.send.why.ended", kind === "codex" ? "codex" : "claude"));
    case "admin":
      return t("terminal.send.why.admin");
    case "dialog":
      return t("terminal.send.why.dialog");
    case "unknown":
      return t("terminal.send.why.unknown");
    case "busy":
      return t("terminal.send.why.busy");
    case "gone":
      return t("terminal.send.why.gone");
    case "laps":
      return t("terminal.send.why.laps");
    case "failed":
      return t("terminal.send.why.failed");
  }
}
