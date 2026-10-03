import { nextTick, onBeforeUnmount, ref, type Ref } from "vue";

import { stillMotion } from "./slide";

/** Pixels the pointer must travel before a press becomes a drag; less is a click. */
const THRESHOLD = 5;
/** How long a released card takes to slide into its place (ms), as a dropped project row does. */
export const SETTLE_MS = 160;
/** Near a column's top or bottom edge (px) a drag scrolls it, faster the closer it gets; near the
 * left or right edge of a board wider than its room, the board. */
const SCROLL_ZONE = 40;
const SCROLL_SPEED = 12;

/** How far a drag scrolls in a frame, the pointer at `at` between the edges `start` and `end`:
 * back near the first, on near the second, not at all in between. */
function edgeStep(at: number, start: number, end: number): number {
  const before = start + SCROLL_ZONE - at;
  const after = at - (end - SCROLL_ZONE);
  const speed = (depth: number): number => Math.ceil((SCROLL_SPEED * Math.min(depth, SCROLL_ZONE)) / SCROLL_ZONE);
  return before > 0 ? -speed(before) : after > 0 ? speed(after) : 0;
}

/** Where a dragged card would land: a column, and its place among that column's other cards. */
export type DropTarget = { column: string; index: number };

/** The dragged card's box, following the pointer (window coordinates). */
export type Ghost = { x: number; y: number; width: number; height: number };

/**
 * Cards dragged within and across a board's columns, with pointer events rather than HTML drag and
 * drop (which Tauri's file-drop handling can swallow). Columns carry `data-column` (their name)
 * and `data-column-body` on the part that scrolls (inside each, or around them all where they sit
 * one under the other); cards carry `data-card` (their id), the place shown for the dragged one
 * `data-slot`. Buttons, inputs and `data-no-drag` keep their own clicks and never start a drag.
 *
 * While a drag is on, `dragging` is the card, `ghost` where it is drawn and `target` where it
 * would land; the column under the pointer takes it (the nearest one when between or past
 * columns). Releasing drops it (`drop`); a release outside the window, a cancelled pointer, the
 * window losing focus or Esc put it back. Either way it first slides into the place shown for it
 * (`settling`, `SETTLE_MS` long), and only then does the drag end.
 */
export function useCardDrag(root: Ref<HTMLElement | null>, drop: (id: string, target: DropTarget) => void) {
  const dragging = ref<string | null>(null);
  const ghost = ref<Ghost | null>(null);
  const target = ref<DropTarget | null>(null);
  /** Released: the ghost is sliding into its place. */
  const settling = ref(false);

  let press: { id: string; x: number; y: number; dx: number; dy: number; width: number; height: number; origin: DropTarget } | null = null;
  let pointer = { x: 0, y: 0 };
  let frame = 0;
  let timer = 0;
  let swallowClick = false;

  function down(event: PointerEvent, id: string): void {
    if (event.button !== 0 || press || (event.target as HTMLElement).closest("[data-no-drag], button, input, textarea, a")) return;
    const card = event.currentTarget as HTMLElement;
    const box = card.getBoundingClientRect();
    // Where it is now: where a drag called off puts it back.
    const column = card.closest<HTMLElement>("[data-column]");
    const origin = { column: column?.dataset.column ?? "", index: Math.max(0, Array.from(column?.querySelectorAll("[data-card]") ?? []).indexOf(card)) };
    press = { id, x: event.clientX, y: event.clientY, dx: event.clientX - box.left, dy: event.clientY - box.top, width: box.width, height: box.height, origin };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", abort);
    window.addEventListener("blur", abort);
    window.addEventListener("keydown", onKey, true);
  }

  function move(event: PointerEvent): void {
    if (!press) return;
    // Released where the window couldn't hear it (outside it): no drop.
    if (!(event.buttons & 1)) return abort();
    pointer = { x: event.clientX, y: event.clientY };

    if (!dragging.value) {
      if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < THRESHOLD) return;
      dragging.value = press.id;
      document.body.classList.add("grabbing");
      frame = requestAnimationFrame(autoScroll);
    }
    place();
  }

  function place(): void {
    if (!press) return;
    ghost.value = { x: pointer.x - press.dx, y: pointer.y - press.dy, width: press.width, height: press.height };
    target.value = locate();
  }

  /** The column under the pointer (else the nearest: sideways where they sit side by side, up or
   * down where one is under the other), and the place among its cards. */
  function columnAt(): HTMLElement | null {
    const columns = Array.from(root.value?.querySelectorAll<HTMLElement>("[data-column]") ?? []);
    let best: HTMLElement | null = null;
    let distance = Infinity;
    for (const column of columns) {
      const box = column.getBoundingClientRect();
      const dx = pointer.x < box.left ? box.left - pointer.x : pointer.x > box.right ? pointer.x - box.right : 0;
      const dy = pointer.y < box.top ? box.top - pointer.y : pointer.y > box.bottom ? pointer.y - box.bottom : 0;
      const away = Math.hypot(dx, dy);
      if (away < distance) {
        best = column;
        distance = away;
      }
    }
    return best;
  }

  function locate(): DropTarget | null {
    const column = columnAt();
    if (!column || !press) return null;
    const id = press.id;
    const cards = Array.from(column.querySelectorAll<HTMLElement>("[data-card]")).filter((card) => card.dataset.card !== id);
    const index = cards.filter((card) => {
      const box = card.getBoundingClientRect();
      return box.top + box.height / 2 < pointer.y;
    }).length;
    return { column: column.dataset.column ?? "", index };
  }

  /** Held near the top or bottom of a column that scrolls (or of what scrolls them all), it
   * scrolls; near the left or right of a board that scrolls sideways (`root`), the board. */
  function autoScroll(): void {
    if (!dragging.value) return;
    let scrolled = false;
    const board = root.value;
    if (board && board.scrollWidth > board.clientWidth) {
      const box = board.getBoundingClientRect();
      const before = board.scrollLeft;
      board.scrollLeft = before + edgeStep(pointer.x, box.left, box.right);
      if (board.scrollLeft !== before) scrolled = true;
    }
    const column = columnAt();
    const body = column?.querySelector<HTMLElement>("[data-column-body]") ?? column?.closest<HTMLElement>("[data-column-body]");
    if (body && body.scrollHeight > body.clientHeight) {
      const box = body.getBoundingClientRect();
      const before = body.scrollTop;
      body.scrollTop = before + edgeStep(pointer.y, box.top, box.bottom);
      if (body.scrollTop !== before) scrolled = true;
    }
    if (scrolled) place();
    frame = requestAnimationFrame(autoScroll);
  }

  /** The pointer is let go of: nothing follows it any more. */
  function release(): void {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", abort);
    window.removeEventListener("blur", abort);
    window.removeEventListener("keydown", onKey, true);
    cancelAnimationFrame(frame);
    document.body.classList.remove("grabbing");
  }

  function stop(): void {
    release();
    window.clearTimeout(timer);
    press = null;
    dragging.value = null;
    ghost.value = null;
    target.value = null;
    settling.value = false;
  }

  /**
   * The drag is over: the ghost slides into the place shown for it, then the card takes it (`to`
   * given to `drop`; null when the drag was called off and the place is its own again).
   */
  function settle(to: DropTarget | null): void {
    const id = dragging.value;
    if (!id) return stop();
    release();
    // The click that follows the release must not select or open anything.
    swallowClick = true;
    window.setTimeout(() => (swallowClick = false), 0);

    const end = (): void => {
      stop();
      if (to) drop(id, to);
    };
    if (stillMotion()) return end();

    // Once the place shown has moved back, if it had to.
    void nextTick(() => {
      // Stopped meanwhile (the board went away).
      if (!press) return;
      const slot = root.value?.querySelector<HTMLElement>("[data-slot]");
      if (!root.value || !slot || !ghost.value) return end();
      // Its place may be out of sight in a long column: the ghost never leaves the board.
      const box = slot.getBoundingClientRect();
      const board = root.value.getBoundingClientRect();
      const y = Math.max(board.top, Math.min(box.top, board.bottom - ghost.value.height));
      settling.value = true;
      ghost.value = { ...ghost.value, x: box.left, y };
      timer = window.setTimeout(end, SETTLE_MS);
    });
  }

  function up(): void {
    settle(target.value);
  }

  function abort(): void {
    if (dragging.value && press) target.value = press.origin;
    settle(null);
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key !== "Escape" || !dragging.value) return;
    event.preventDefault();
    event.stopPropagation();
    abort();
  }

  /** Use in a card's click handler: false right after a drag. */
  function isClick(): boolean {
    return !swallowClick;
  }

  onBeforeUnmount(stop);

  return { dragging, ghost, target, settling, down, isClick };
}
