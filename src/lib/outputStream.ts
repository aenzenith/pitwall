import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onBeforeUnmount, shallowRef, toValue, type MaybeRefOrGetter, type ShallowRef } from "vue";

import { api } from "./store";

/** An output line; `id` only grows, so a line keeps its key as older ones scroll out. */
export type OutputLine = { id: number; text: string; kind: "cmd" | "sys" | null };

/** The pane keeps the newest lines only, as the core does. */
const MAX_LINES = 500;

/** The core's `output` event: a batch of lines (or, from older cores, one line) of a project's
 * dev server (`job` unset) or one of its commands. */
type OutputEvent = { path: string; job?: string | null } & ({ lines: string[] } | { line: string });

/** How many of `late`'s first lines are already `fetched`'s last ones (sent while it was read). */
function overlap(fetched: string[], late: string[]): number {
  for (let k = Math.min(fetched.length, late.length); k > 0; k--) {
    let match = true;
    for (let i = 0; i < k && match; i++) match = fetched[fetched.length - k + i] === late[i];
    if (match) return k;
  }
  return 0;
}

/**
 * The live output of a project's dev server (`job` null) or one of its commands: `load()` reads
 * what the core has kept, then live lines keep coming. Call `load()` whenever `path` or `job`
 * changes (or to read it all again); lines of any other path or job are ignored.
 *
 * - Listening starts before the first `get_output`, so no line falls between the two; lines that
 *   arrive while it is on its way are added after its answer, less those it already had.
 * - Only the newest `load()` counts: an older one's answer is dropped.
 * - A batch of lines is one update, and the newest MAX_LINES are kept.
 */
export function useOutputStream(
  path: MaybeRefOrGetter<string>,
  job: MaybeRefOrGetter<string | null>,
): { lines: ShallowRef<OutputLine[]>; load: () => Promise<void> } {
  const lines = shallowRef<OutputLine[]>([]);

  let lineSeq = 0;
  /** Lines that arrive while `get_output` is on its way, added after its answer. */
  let arriving: string[] | null = null;
  /** The newest `load`: an older one's answer is dropped. */
  let outputLoad = 0;

  function toLines(texts: string[]): OutputLine[] {
    return texts.map((text) => ({
      id: ++lineSeq,
      text,
      kind: text.startsWith("$ ") ? "cmd" : text.startsWith("[pitwall]") ? "sys" : null,
    }));
  }

  /** A whole batch in one update. */
  function append(texts: string[]): void {
    if (!texts.length) return;
    const next = lines.value.concat(toLines(texts));
    lines.value = next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
  }

  const listening = listen<OutputEvent>("output", (event) => {
    const payload = event.payload;
    if (payload.path !== toValue(path) || (payload.job ?? null) !== toValue(job)) return;
    const texts = "lines" in payload ? payload.lines : [payload.line];
    if (arriving) arriving.push(...texts);
    else append(texts);
  });

  async function load(): Promise<void> {
    const ask = ++outputLoad;
    const from = toValue(path);
    const of = toValue(job);

    await listening;
    if (ask !== outputLoad) return;
    arriving = [];

    let fetched: string[] = [];
    try {
      fetched = await api.output(from, of);
    } catch {
      // Nothing to show; live lines still come.
    }
    if (ask !== outputLoad) return;

    const late = arriving ?? [];
    arriving = null;
    lines.value = toLines(fetched.slice(-MAX_LINES));
    append(late.slice(overlap(fetched, late)));
  }

  onBeforeUnmount(() => {
    void listening.then((unlisten: UnlistenFn) => unlisten());
  });

  return { lines, load };
}
