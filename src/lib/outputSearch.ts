import { computed, ref, shallowRef, watch, type Ref } from "vue";

import { finder, type OutputLine, type Range } from "./outputLines";

/** A match of the search: the line it is in, and which of that line's matches it is. */
export type Hit = { id: number; k: number };

const NO_RANGES: Range[] = [];

/**
 * The output pane's search over the lines it shows (`view`): plain text, whatever the case.
 *
 * - A line is searched once per query: `ranges` hands out what was found in it from then on, so
 *   a pane that redraws with every batch of output searches the new lines only.
 * - The match it stands on starts as the newest one and stays the same match as lines arrive;
 *   when its line scrolls out of the output, the next one after it takes over.
 * - `close()` forgets the query, and with it every mark.
 */
export function useOutputSearch(view: Ref<OutputLine[]>) {
  const open = ref(false);
  const query = ref("");

  /** What the query finds in a line, or null while nothing is searched for. */
  const ranges = computed(() => {
    const find = open.value ? finder(query.value) : null;
    if (!find) return null;

    const found = new WeakMap<OutputLine, Range[]>();
    return (line: OutputLine): Range[] => {
      let places = found.get(line);
      if (!places) {
        // A marker's words are the pane's, not the output's.
        places = line.marker ? NO_RANGES : find(line.text);
        found.set(line, places);
      }
      return places;
    };
  });

  /** Every match in the pane, from the top. */
  const hits = computed<Hit[]>(() => {
    const of = ranges.value;
    if (!of) return [];
    const all: Hit[] = [];
    for (const line of view.value) {
      const count = of(line).length;
      for (let k = 0; k < count; k++) all.push({ id: line.id, k });
    }
    return all;
  });

  const current = shallowRef<Hit | null>(null);
  const position = computed(() => {
    const at = current.value;
    return at ? hits.value.findIndex((hit) => hit.id === at.id && hit.k === at.k) : -1;
  });

  watch([query, hits], ([text, all], [textBefore]) => {
    const at = text === textBefore ? current.value : null;
    if (at && all.some((hit) => hit.id === at.id && hit.k === at.k)) return;
    current.value = (at && all.find((hit) => hit.id > at.id)) ?? all[all.length - 1] ?? null;
  });

  /** To the next match (`1`) or the one before (`-1`), round at the ends. */
  function step(by: 1 | -1): void {
    const all = hits.value;
    if (!all.length) return;
    const at = position.value;
    current.value = all[at < 0 ? (by > 0 ? 0 : all.length - 1) : (at + by + all.length) % all.length];
  }

  function close(): void {
    open.value = false;
    query.value = "";
    current.value = null;
  }

  return { open, query, ranges, count: computed(() => hits.value.length), position, current, step, close };
}
