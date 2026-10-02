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
