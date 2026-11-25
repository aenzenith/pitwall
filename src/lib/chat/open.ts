/**
 * Every way the window opens a chat goes through `startChat`: the "+" menu's
 * Yeni sohbet row, "Sohbette aç" in the Sessions page and the terminals' history, a project's
 * Claude button while Settings › Claude says "Sohbet", a "chat-attach" for a project with no chat.
 *
 * Before a project's first chat Pitwall asks once (O6): a chat runs Claude Code there with the
 * project's own hooks and MCP servers, as the terminal does once its folder is trusted. The
 * question is asked when the project's settings don't hold `chatTrusted`, and again when the core
 * refuses `open_chat` with "untrusted" (it alone keeps the answer, `chat_trust`). A session that
 * runs somewhere else is never opened a second time: the core refuses with "elsewhere", and the
 * window says where it runs and offers to go there. Any other failure is told as it came.
 * What is asked or told waits in `chatAsks`, drawn one at a time by `StartChatDialog`
 * (window/chat), which WindowApp mounts beside its other dialogs.
 */
import { computed, shallowRef } from "vue";

import { revealTab } from "../panel";
import { projectSettings } from "../projectSettings";
import { api, snapshot } from "../store";
import type { AgentStartChoice, ChatStartChoice, TerminalOpened } from "../types";
import { CHAT_DEFAULTS, voiceDelayOf, type ChatOutgoing, type ChatSettings } from "./wire";

export type StartChatOptions = {
  start?: AgentStartChoice | ChatStartChoice | null;
  /** `start`'s mode was picked (the empty panel's chat box): Normal stays Normal. */
  exact?: boolean;
  account?: string | null;
  session?: string | null;
  fork?: { session: string; at?: string } | null;
  prompt?: ChatOutgoing | null;
  worktree?: boolean;
};

/* ---------- Settings › Claude › Sohbet, read leniently ---------- */

/** The chat's preferences: the snapshot's `settings.chat` over `CHAT_DEFAULTS`, a value of the
 * wrong kind (or a choice not known) being the default. */
export const chatSettings = computed<ChatSettings>(() => {
  const saved = (snapshot.value?.settings.chat ?? {}) as Partial<Record<keyof ChatSettings, unknown>>;
  const out: ChatSettings = { ...CHAT_DEFAULTS };
  for (const key of Object.keys(CHAT_DEFAULTS) as Array<keyof ChatSettings>) {
    const value = saved[key];
    if (typeof value === typeof CHAT_DEFAULTS[key]) (out as Record<keyof ChatSettings, unknown>)[key] = value;
  }
  if (out.open !== "chat") out.open = "terminal";
  if (out.sendKey !== "mod-enter") out.sendKey = "enter";
  if (out.mode !== "auto") out.mode = "default";
  if (out.voiceModel !== "large") out.voiceModel = "small";
  out.voiceDelay = voiceDelayOf(out.voiceDelay);
  return out;
});

/** "Claude açılınca": Sohbet. A project's Claude button then opens a chat, not Claude Code's
 * terminal ("+"'s Yeni Claude row stays the terminal: Yeni sohbet sits under it). */
export function opensChat(): boolean {
  return chatSettings.value.open === "chat";
}

/* ---------- what is asked or told ---------- */

/** The question or the notice a start waits on: the trust question (`done` takes the answer; the
 * dialog has already sent `chat_trust` on a yes), a session that runs elsewhere, or a failure. */
export type ChatAsk =
  | { id: number; kind: "trust"; path: string; done: (allowed: boolean) => void }
  | { id: number; kind: "elsewhere"; path: string; session: string }
  | { id: number; kind: "failed"; path: string; text: string };

/** In the order they came; the dialog shows the first. Page memory only. */
export const chatAsks = shallowRef<ChatAsk[]>([]);
let asks = 0;

type Unnumbered = ChatAsk extends infer A ? (A extends ChatAsk ? Omit<A, "id"> : never) : never;

function ask(entry: Unnumbered): void {
  chatAsks.value = [...chatAsks.value, { ...entry, id: ++asks } as ChatAsk];
}

/** The dialog is done with `entry`: the next one shows. */
export function chatAskDone(entry: ChatAsk): void {
  chatAsks.value = chatAsks.value.filter((each) => each.id !== entry.id);
}

/** One question per project at a time: a second start meanwhile waits for the same answer. */
const trustAsked = new Map<string, Promise<boolean>>();

function askTrust(path: string): Promise<boolean> {
  let answer = trustAsked.get(path);
  if (!answer) {
    answer = new Promise<boolean>((resolve) =>
      ask({
        kind: "trust",
        path,
        done: (allowed) => {
          trustAsked.delete(path);
          resolve(allowed);
        },
      }),
    );
    trustAsked.set(path, answer);
  }
  return answer;
}

/** The project's settings say its first chat was allowed. */
function trusted(path: string): boolean {
  const project = snapshot.value?.projects.find((p) => p.path === path);
  return !!project && projectSettings(project).chatTrusted === true;
}

function errorText(reason: unknown): string {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  return String(reason ?? "");
}

/** Its tab in the terminal panel, open, the keyboard in it (TerminalPanel takes the request). */
function shown(path: string, opened: TerminalOpened): TerminalOpened {
  if (opened.kind === "tab") revealTab(path, opened.view.id);
  return opened;
}

/** Opens a chat tab for the project at `path`; null when the user said no (trust) or it failed
 * (what went wrong is told in the dialog). The tab opened is shown in the terminal panel. */
export async function startChat(path: string, opts: StartChatOptions = {}): Promise<TerminalOpened | null> {
  let asked = false;
  if (!trusted(path)) {
    if (!(await askTrust(path))) return null;
    asked = true;
  }
  for (;;) {
    try {
      return shown(path, await api.openChat(path, opts));
    } catch (reason) {
      const text = errorText(reason);
      // The core holds no answer for it yet (a settings file without it): asked once, then told.
      if (text === "untrusted" && !asked) {
        if (!(await askTrust(path))) return null;
        asked = true;
        continue;
      }
      const session = opts.session ?? opts.fork?.session ?? null;
      if (text === "elsewhere" && session) ask({ kind: "elsewhere", path, session });
      else ask({ kind: "failed", path, text: text === "untrusted" || text === "elsewhere" ? "" : text });
      return null;
    }
  }
}
