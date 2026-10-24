/**
 * The names tabs show: their own, and where two or more share one, the later opened (a higher id)
 * numbered from 2 ("zsh", "zsh 2"), so that two panes or tabs of the same name can be told apart.
 * Nothing is renamed: the numbers are the row's and the panes' only.
 */
export function distinctNames(terminals: ReadonlyArray<{ id: number; name: string }>): Map<number, string> {
  const seen = new Map<string, number>();
  const out = new Map<number, string>();
  for (const term of [...terminals].sort((a, b) => a.id - b.id)) {
    const count = (seen.get(term.name) ?? 0) + 1;
    seen.set(term.name, count);
    out.set(term.id, count === 1 ? term.name : `${term.name} ${count}`);
  }
  return out;
}

/**
 * Keys on a tab (roving tabindex: only the chosen tab is in the Tab order). The arrows move to
 * the next or previous tab of the same tablist, round the ends; Home and End to the first and
 * last. The tab gone to is focused, and its place is returned for the caller to choose it;
 * `null` for any other key.
 */
export function tabKey(event: KeyboardEvent, count: number, at: number): number | null {
  let next: number | null = null;
  if (event.key === "ArrowRight" || event.key === "ArrowDown") next = (at + 1) % count;
  else if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = (at - 1 + count) % count;
  else if (event.key === "Home") next = 0;
  else if (event.key === "End") next = count - 1;
  if (next === null || !count) return null;

  event.preventDefault();
  const list = (event.currentTarget as HTMLElement | null)?.closest('[role="tablist"]');
  list?.querySelectorAll<HTMLElement>('[role="tab"]')[next]?.focus();
  return next;
}
