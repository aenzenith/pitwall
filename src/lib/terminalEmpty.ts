// The terminal panel's empty box (window/TerminalEmpty.vue): the four things it opens, which one is
// picked, and what is typed in it. The pick is the last one the user made, kept by the core in the
// settings (`terminalStart`, this Mac's own) so it comes back after a restart; before any, "Claude
// açılınca" decides (Settings › Terminal). What is typed, and the pictures pasted for Claude or
// Codex, stay in the page's memory only, per project (and per empty pane), until it opens something
// or the app quits: never written down here (the core writes the pictures for the agent as it
// starts, `prompt_images.rs`). A chat's box is the chat's own composer, its draft that one's
// (lib/chat/draft).

import { computed, ref, watch } from "vue";

import { agentOn } from "./agents";
import { opensChat } from "./chat/open";
import { api, snapshot } from "./store";
import type { ChatOutgoing } from "./chat/wire";
import type { AgentStartChoice, ChatStartChoice, PastSession, ShellChoice } from "./types";

export type EmptyKind = "shell" | "chat" | "claude" | "codex";

/** In the order the box offers them, the "+" menu's order (the user's word, 2026-11-14). */
export const EMPTY_KINDS: EmptyKind[] = ["shell", "chat", "claude", "codex"];

/** What the box asks the panel to open: the kind, the text typed (blank: it opens empty), the
 * start its chip picked for this one start (null: the project's own) and, on Windows, the shell
 * (null: a new tab's own). Claude's or Codex's: the pictures pasted (`data:` URLs). A chat's: its
 * first message as its composer made it, or a line it runs once ready (`later`), and its start
 * (`exact`: its mode was picked). */
export type EmptyStart = {
  kind: EmptyKind;
  text: string;
  start: AgentStartChoice | null;
  shell: ShellChoice | null;
  images?: string[];
  chat?: { message: ChatOutgoing | null; later: string | null; start: ChatStartChoice | null; exact: boolean };
};

function isKind(value: unknown): value is EmptyKind {
  return typeof value === "string" && (EMPTY_KINDS as string[]).includes(value);
}

/** A chat and Claude's tab need Claude on, Codex's tab Codex; a shell is always there. */
export function kindOn(kind: EmptyKind): boolean {
  return kind === "shell" || agentOn(kind === "codex" ? "codex" : "claude");
}

/** Picked here and not yet heard back from the core (or never, where it can't keep it). */
const pickedNow = ref<EmptyKind | null>(null);

watch(
  () => snapshot.value?.settings.terminalStart,
  (kept) => {
    if (kept === pickedNow.value) pickedNow.value = null;
  },
);

/** The kind the box shows picked: the last one picked while it is on, a shell when that one's
 * agent is off; before any pick, Claude's tab or a chat as "Claude açılınca" says, a shell while
 * Claude is off. */
export const emptyPick = computed<EmptyKind>(() => {
  const kept = pickedNow.value ?? snapshot.value?.settings.terminalStart;
  if (isKind(kept)) return kindOn(kept) ? kept : "shell";
  if (agentOn("claude")) return opensChat() ? "chat" : "claude";
  return "shell";
});

export function pickEmpty(kind: EmptyKind): void {
  if (kind === emptyPick.value) return;
  pickedNow.value = kind;
  void api.setTerminalStart(kind).catch(() => undefined);
}

/** What is typed in each box, by its key (the project's path; an empty pane's adds its key). */
const drafts = new Map<string, string>();

export function draftOf(key: string): string {
  return drafts.get(key) ?? "";
}

export function keepDraft(key: string, text: string): void {
  if (text) drafts.set(key, text);
  else drafts.delete(key);
}

/** A picture pasted for Claude or Codex: as the chat's composer reads one (scaled when too large). */
export type BoxImage = { key: string; mediaType: string; data: string; name: string; scaled: boolean };

/** The pictures pasted into each box, by the same key. */
const pictures = new Map<string, BoxImage[]>();

export function imagesOf(key: string): BoxImage[] {
  return pictures.get(key) ?? [];
}

export function keepImages(key: string, images: BoxImage[]): void {
  if (images.length) pictures.set(key, images);
  else pictures.delete(key);
}

/** The latest past session last found per agent and project ("claude\n/path"; null: none), shown
 * at once while the history is read again (Codex's takes a moment). In the page's memory only. */
export const lastSeen = new Map<string, PastSession | null>();
