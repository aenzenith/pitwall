import { computed, ref, watch } from "vue";

import { t } from "../i18n";
import { api, snapshot } from "../store";

/**
 * The empty chat's ready prompts (V16; the user's word, 14 Nov): the user's own list, added to
 * (a right-click or the add button under them), edited and removed by a right-click, kept in the
 * core's settings (`settings.chat.examples`). Until that was ever written, the three of the
 * language Pitwall shows: the first chat that shows them writes them as the list, so they stay in
 * that language after.
 */
const FIRST = ["chat.empty.example1", "chat.empty.example2", "chat.empty.example3"] as const;

/** As many as the core keeps (`CHAT_EXAMPLES_MOST` in src-tauri/src/core/chat.rs). */
export const EXAMPLES_MOST = 30;

/** What was just asked for, shown at once until the core's snapshot says it too. */
const asked = ref<string[] | null>(null);

const kept = computed<string[] | null>(() => {
  // Beside the chat's preferences (`settings.chat`), not one of its switches (lib/chat/wire).
  const list = (snapshot.value?.settings.chat as { examples?: unknown } | undefined)?.examples;
  return Array.isArray(list) ? list.filter((text): text is string => typeof text === "string") : null;
});

watch(kept, () => (asked.value = null));

export const chatExamples = computed<string[]>(() => asked.value ?? kept.value ?? FIRST.map((key) => t(key)));

/** The first three written as the list, once a chat shows them and nothing was written yet. */
export function keepFirstExamples(): void {
  if (!snapshot.value || kept.value || asked.value) return;
  setExamples(FIRST.map((key) => t(key)));
}

/** The list as it is to be; one that can't be written goes back to what the core holds. */
export function setExamples(list: string[]): void {
  const next = list.map((text) => text.trim()).filter(Boolean);
  asked.value = next;
  void api.setChatExamples(next).catch(() => (asked.value = null));
}
