/**
 * The keyboard for a project list that is a single Tab stop (`role="grid"`, one `role="row"` per
 * project, carrying `data-path`): ↑/↓, Home and End move between rows, ←/→ between a row and its
 * buttons, ↵ and Space act on the row. Buttons inside rows keep `tabindex="-1"`: the arrows reach
 * them, Tab doesn't. Bind it as the list's `keydown`.
 *
 * A list drawn in groups has their heads among its rows (`data-group`): ↑/↓ go on to a head too,
 * which takes the focus as a row does, and the head's own keys are the list's to give.
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

  /** The line above or below: a row, or a team's head (where the projects in no team begin,
   * `data-group` empty, takes no keyboard). */
  const step = (by: 1 | -1): void => {
    const lines = Array.from(list.querySelectorAll<HTMLElement>(':scope > [data-path], :scope > [data-group]:not([data-group=""])'));
    const to = lines[lines.indexOf(row) + by];
    if (to?.dataset.group === undefined) return go(to ?? row);
    event.preventDefault();
    to.focus();
    to.scrollIntoView({ block: "nearest" });
  };

  switch (event.key) {
    case "ArrowDown":
      return step(1);
    case "ArrowUp":
      return step(-1);
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
