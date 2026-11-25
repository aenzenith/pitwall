import { peekChat } from "./chat/store";
import { t } from "./i18n";
import { statusLine } from "./sessions";
import { now, sessions } from "./store";
import type { TerminalView } from "./types";

/** An agent's live state in a tab: at work, or waiting on you; and its words for it. */
export type TabState = { phase: "waiting" | "working"; text: string };

/**
 * The agent in each tab, by the terminal's id: at work, or waiting on you (a turn finished and not
 * looked at yet, a question, a permission prompt). Nothing for a tab with neither. Any tab of
 * Pitwall's the sessions name; of `terms`, a chat tells its own state (its frames), where no
 * session row of the hook's tells it. Reactive: read inside a `computed`.
 */
export function tabStates(terms: readonly TerminalView[]): Map<number, TabState> {
  const states = new Map<number, TabState>();
  for (const row of sessions.value?.sessions ?? []) {
    if (row.origin.kind !== "pitwall" || (row.phase !== "waiting" && row.phase !== "working")) continue;
    // Two in one terminal (one left running in the background): the one that waits on you tells.
    if (row.phase === "working" && states.get(row.origin.terminal)?.phase === "waiting") continue;
    states.set(row.origin.terminal, { phase: row.phase, text: statusLine(row, now.value).text });
  }
  for (const term of terms) {
    if (term.kind !== "chat" || states.has(term.id)) continue;
    const chat = peekChat(term.id);
    const state = chat?.state ?? term.chat?.state;
    if (state === "running") states.set(term.id, { phase: "working", text: t("chat.tab.working") });
    else if (state === "requires_action" || (chat?.pending.length ?? 0) > 0) states.set(term.id, { phase: "waiting", text: t("chat.tab.waiting") });
  }
  return states;
}
