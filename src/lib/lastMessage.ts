// What Claude last said in the session whose details show (the Sessions page's, a board card's):
// asked of the core when the session or its state changes, held only while it shows.

import { computed, ref, watch, type ComputedRef } from "vue";

import { lastActive } from "./sessions";
import { api } from "./store";
import type { SessionRow } from "./types";

/** A working session is asked again this often (ms): it says more without changing state. */
const WORKING_EVERY_MS = 10_000;

/** What tells a session's states apart: a new one may have a new last message. */
export function messageState(row: SessionRow | null, now: number): string {
  if (!row) return "";
  return [row.phase, row.turn?.at, row.since, lastActive(row), row.phase === "working" ? Math.floor(now / WORKING_EVERY_MS) : ""].join(" ");
}

/**
 * The last message of `session()` (null: none wanted), empty until the core answers or when there
 * is none. Asked again whenever `state()` changes; another session's text never shows meanwhile.
 */
export function useLastMessage(session: () => string | null, state: () => string): ComputedRef<string> {
  const said = ref<{ session: string; text: string } | null>(null);
  const wanted = computed(() => {
    const id = session();
    return id ? `${id} ${state()}` : null;
  });

  watch(
    wanted,
    (key) => {
      const id = session();
      if (!key || !id) {
        said.value = null;
        return;
      }
      if (said.value?.session !== id) said.value = null;
      void api
        .lastMessage(id)
        .then((text) => {
          if (wanted.value === key) said.value = text ? { session: id, text } : null;
        })
        .catch(() => undefined);
    },
    { immediate: true },
  );

  return computed(() => said.value?.text ?? "");
}
