/**
 * Settings › Terminal's own choices (`settings.terminal`): what a double click on the terminal
 * panel's tab row does and which click opens the folded panel. Read leniently: a value not known is
 * the default, as the core reads it. Where an ended Claude session opens at a click is the chat's
 * "Claude açılınca" (`settings.chat.open`), shown in the same tab (the user's word, 2026-11-14:
 * "bu iki ayar birleşsin").
 */
import { computed } from "vue";

import { agentOn } from "./agents";
import { chatSettings } from "./chat/open";
import { snapshot } from "./store";
import type { Agent, Settings } from "./types";

/** A double click on the tab row's empty part, in the open panel: nothing (the default), a Claude
 * tab as the Claude buttons open it (`claude`, as Settings' "Claude açılınca" says), a shell tab,
 * Claude Code's terminal (`newClaude`, "+"'s Yeni Claude), a Codex tab, a chat, the panel grown
 * and back (`fill`), or the panel between the bottom and the card (`stand`). */
export type DoubleClick = "none" | "claude" | "tab" | "newClaude" | "codex" | "chat" | "fill" | "stand";
export const DOUBLE_CLICKS: DoubleClick[] = ["none", "claude", "tab", "newClaude", "codex", "chat", "fill", "stand"];

export type TerminalSettings = {
  doubleClick: DoubleClick;
  /** The folded panel opens at a click on its row's empty part (`click`), or at a double click. */
  unfold: "click" | "double";
};

export const TERMINAL_DEFAULTS: TerminalSettings = { doubleClick: "none", unfold: "click" };

/** `saved` (the settings' `terminal`, or Settings' form) as the core reads it. */
export function readTerminal(saved: Settings["terminal"]): TerminalSettings {
  return {
    doubleClick: DOUBLE_CLICKS.find((each) => each === saved?.doubleClick) ?? TERMINAL_DEFAULTS.doubleClick,
    unfold: saved?.unfold === "double" ? "double" : "click",
  };
}

export const terminalSettings = computed<TerminalSettings>(() => readTerminal(snapshot.value?.settings.terminal));

/** The agent a double click's choice needs on; none for the others. */
export function doubleClickAgent(choice: DoubleClick): Agent | null {
  if (choice === "claude" || choice === "newClaude" || choice === "chat") return "claude";
  return choice === "codex" ? "codex" : null;
}

/** An ended Claude session clicked (the terminals' history, the Sessions page, the Today page, a
 * card) opens in a chat: "Claude açılınca" says Sohbet and Claude is on. */
export function sessionsOpenInChat(): boolean {
  return chatSettings.value.open === "chat" && agentOn("claude");
}
