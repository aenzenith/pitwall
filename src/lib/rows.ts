/**
 * The keyboard for a project list that is a single Tab stop (`role="grid"`, one `role="row"` per
 * project, carrying `data-path`): ↑/↓, Home and End move between rows, ←/→ between a row and its
 * buttons, ↵ and Space act on the row. Buttons inside rows keep `tabindex="-1"`: the arrows reach
 * them, Tab doesn't. Bind it as the list's `keydown`.
 */
export function rowKeys(
  event: KeyboardEvent,
  on: { move: (path: string) => void; enter: (path: string) => void; space: (path: string) => void },
): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const target = event.target as HTMLElement;
  const row = target.closest<HTMLElement>("[data-path]");
  const list = row?.parentElement;
  if (!row || !list) return;

  const path = row.dataset.path ?? "";
  const onRow = target === row;

  /** Focus (and, through `move`, select) another row. */
  const go = (to: HTMLElement | undefined): void => {
    event.preventDefault();
    if (!to) return;
    on.move(to.dataset.path ?? "");
    to.focus();
    to.scrollIntoView({ block: "nearest" });
  };

  const rows = Array.from(list.querySelectorAll<HTMLElement>(":scope > [data-path]"));
  const at = rows.indexOf(row);

  switch (event.key) {
    case "ArrowDown":
      return go(rows[at + 1] ?? row);
    case "ArrowUp":
      return go(rows[at - 1] ?? row);
    case "Home":
      return go(rows[0]);
    case "End":
      return go(rows[rows.length - 1]);
    case "ArrowRight":
    case "ArrowLeft": {
      // The row, then its enabled buttons left to right.
      event.preventDefault();
      const stops = [row, ...Array.from(row.querySelectorAll<HTMLElement>("button:not(:disabled)"))];
      const i = stops.indexOf(target);
      stops[Math.min(stops.length - 1, Math.max(0, i + (event.key === "ArrowRight" ? 1 : -1)))]?.focus();
      return;
    }
    case "Enter":
      if (!onRow) return;
      event.preventDefault();
      return on.enter(path);
    case " ":
      if (!onRow) return;
      event.preventDefault();
      return on.space(path);
  }
}
