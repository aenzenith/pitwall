import { nextTick, ref, type Ref } from "vue";

import { api, snapshot } from "./store";

/** Pixels the pointer must travel before a press becomes a drag; less is a click. */
const THRESHOLD = 5;
/** How long the dropped row takes to slide into its place (ms); the CSS uses the same. */
const SETTLE_MS = 160;
/** Near the list's top or bottom edge (px) a drag scrolls the list, faster the closer it gets. */
const SCROLL_ZONE = 40;
const SCROLL_SPEED = 14;

/**
 * Where the rows were when the drag began, in the list's own coordinates, and the list's top and
 * bottom padding: the dragged row keeps that much room from the list's edges.
 */
type Layout = { paths: string[]; tops: number[]; heights: number[]; from: number; shift: number; padTop: number; padBottom: number };

/**
 * Drag-to-reorder for a project list, animated: the dragged row follows the pointer anywhere in
 * the list (the empty space under the last row too), the others slide aside to show where it
 * will land, and on release it slides into that place before the new order is saved. A long list
 * scrolls while the row is held near its top or bottom edge. Rows carry `data-path`; controls marked `data-no-drag` (and inputs) keep
 * their own clicks and never start a drag. Pointer events rather than HTML drag and drop, which
 * Tauri's file-drop handling can swallow.
 *
 * Bind `listClass()` on the list, `rowClass(path)` and `rowStyle(path)` on each row.
 */
export function useReorder(list: Ref<HTMLElement | null>) {
  const dragging = ref<string | null>(null);
  /** Vertical offset per row while a drag is on. */
  const offsets = ref<Record<string, number>>({});
  /** The dropped row is sliding into place. */
  const settling = ref(false);
  /** Transitions off for the instant the real order replaces the offsets. */
  const frozen = ref(false);

  let press: { path: string; y: number; scroll: number } | null = null;
  let layout: Layout | null = null;
  let target = -1;
  let pointerY = 0;
  let scrolling = 0;
  let swallowClick = false;

  function down(event: PointerEvent, path: string): void {
    if (event.button !== 0 || settling.value || (event.target as HTMLElement).closest("[data-no-drag], input, textarea")) return;

    press = { path, y: event.clientY, scroll: list.value?.scrollTop ?? 0 };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
  }

  function measure(path: string): Layout | null {
    const root = list.value;
    if (!root) return null;

    const rows = Array.from(root.querySelectorAll<HTMLElement>("[data-path]"));
    const origin = root.getBoundingClientRect().top - root.scrollTop;
    const paths = rows.map((row) => row.dataset.path ?? "");
    const tops = rows.map((row) => row.getBoundingClientRect().top - origin);
    const heights = rows.map((row) => row.getBoundingClientRect().height);
    const from = paths.indexOf(path);
    if (from < 0) return null;

    // Rows make room by the dragged row's height plus the gap between rows.
    const gap = rows.length > 1 ? Math.max(0, tops[1] - tops[0] - heights[0]) : 0;
    const style = getComputedStyle(root);
    return {
      paths,
      tops,
      heights,
      from,
      shift: heights[from] + gap,
      padTop: parseFloat(style.paddingTop) || 0,
      padBottom: parseFloat(style.paddingBottom) || 0,
    };
  }

  function move(event: PointerEvent): void {
    if (!press) return;
    pointerY = event.clientY;

    if (!dragging.value) {
      if (Math.abs(event.clientY - press.y) < THRESHOLD) return;
      layout = measure(press.path);
      if (!layout) return;
      dragging.value = press.path;
      target = layout.from;
      document.body.classList.add("grabbing");
      scrolling = requestAnimationFrame(autoScroll);
    }

    place();
  }

  /** Held near an edge of a list taller than its box, the list scrolls and the row stays put. */
  function autoScroll(): void {
    const root = list.value;
    if (!root || !dragging.value || settling.value) return;

    const box = root.getBoundingClientRect();
    const above = box.top + SCROLL_ZONE - pointerY;
    const below = pointerY - (box.bottom - SCROLL_ZONE);
    const step = above > 0 ? -Math.ceil((SCROLL_SPEED * above) / SCROLL_ZONE) : below > 0 ? Math.ceil((SCROLL_SPEED * below) / SCROLL_ZONE) : 0;

    if (step) {
      const before = root.scrollTop;
      root.scrollTop = before + step;
      if (root.scrollTop !== before) place();
    }

    scrolling = requestAnimationFrame(autoScroll);
  }

  /** Puts the dragged row under the pointer and slides the others to make room. */
  function place(): void {
    const root = list.value;
    if (!press || !layout || !root) return;

    const { paths, tops, heights, from, shift, padTop, padBottom } = layout;

    // Under the pointer, the list's scrolling included. It may go anywhere in the visible list,
    // the empty space under the last row too, keeping the list's padding from its edges.
    const wanted = tops[from] + pointerY - press.y + root.scrollTop - press.scroll;
    const visibleTop = root.scrollTop + padTop;
    const visibleBottom = root.scrollTop + root.clientHeight - padBottom - heights[from];
    const highest = Math.max(tops[0], visibleTop);
    const lowest = Math.max(highest, visibleBottom);
    const dy = Math.min(lowest, Math.max(highest, wanted)) - tops[from];
    const top = tops[from] + dy;
    const bottom = top + heights[from];

    // Its new place: a row trades places once the dragged row's edge passes its middle (the top
    // edge for rows above, the bottom edge for rows below). Edges rather than centres, so rows of
    // slightly different heights never keep it from the first or the last place.
    target = paths.reduce((count, _, i) => {
      if (i === from) return count;
      const middle = tops[i] + heights[i] / 2;
      const before = i < from ? top >= middle : bottom > middle;
      return before ? count + 1 : count;
    }, 0);

    const next: Record<string, number> = { [paths[from]]: dy };
    paths.forEach((path, i) => {
      if (i === from) return;
      if (from < target && i > from && i <= target) next[path] = -shift;
      else if (from > target && i >= target && i < from) next[path] = shift;
    });
    offsets.value = next;
  }

  function up(): void {
    window.removeEventListener("pointermove", move);
    cancelAnimationFrame(scrolling);
    document.body.classList.remove("grabbing");

    const moved = dragging.value;
    const drop = layout;
    press = null;

    if (!moved || !drop) {
      dragging.value = null;
      return;
    }

    // The click that follows the drop must not open the project.
    swallowClick = true;
    window.setTimeout(() => (swallowClick = false), 0);

    // Slide the row into the slot the others opened, then save the order.
    const { tops, heights, from } = drop;
    const top = target > from ? tops[target] + heights[target] - heights[from] : tops[target];
    settling.value = true;
    offsets.value = { ...offsets.value, [moved]: top - tops[from] };

    window.setTimeout(() => void finish(moved, drop, target), SETTLE_MS);
  }

  async function finish(moved: string, drop: Layout, to: number): Promise<void> {
    // The real order takes over without a frame of movement.
    frozen.value = true;
    if (to !== drop.from) commit(moved, drop.paths[to], to > drop.from);
    offsets.value = {};
    dragging.value = null;
    settling.value = false;
    layout = null;

    await nextTick();
    requestAnimationFrame(() => (frozen.value = false));
  }

  function commit(moved: string, beside: string, after: boolean): void {
    const current = snapshot.value?.projects ?? [];
    if (!current.length) return;

    const order = current.map((p) => p.path).filter((p) => p !== moved);
    const at = order.indexOf(beside);
    if (at < 0) return;
    order.splice(after ? at + 1 : at, 0, moved);

    // Show the new order at once; the core confirms with the next state.
    if (snapshot.value) {
      const byPath = new Map(current.map((p) => [p.path, p]));
      snapshot.value.projects = order.map((p) => byPath.get(p)).filter((p) => p !== undefined);
    }

    void api.reorder(order);
  }

  /** Use in a row's click handler: false right after a drag. */
  function isClick(): boolean {
    return !swallowClick;
  }

  function listClass(): Record<string, boolean> {
    return { reordering: dragging.value !== null, frozen: frozen.value };
  }

  function rowClass(path: string): Record<string, boolean> {
    return { dragging: dragging.value === path, settling: settling.value && dragging.value === path };
  }

  function rowStyle(path: string): Record<string, string> | undefined {
    const offset = offsets.value[path];
    return offset ? { transform: `translateY(${offset}px)` } : undefined;
  }

  return { dragging, down, isClick, listClass, rowClass, rowStyle };
}
