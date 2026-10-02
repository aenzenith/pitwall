import { ref, type Ref } from "vue";

/**
 * The bottom panels: the output (details) and the terminals (list), each with its own height
 * covering handle, tabs and content. Every launch starts both at the default; either can be
 * dragged down to the minimum.
 */
/** Room for two terminal rows: tabs and a prompt stay usable. */
export const PANEL_MIN = 120;
/** Room for about seven terminal rows. */
const PANEL_DEFAULT = 240;

export const outputHeight = ref(PANEL_DEFAULT);
export const terminalHeight = ref(PANEL_DEFAULT);
/** The output is on screen (not while the project's settings fill the details). */
export const outputShown = ref(false);

/** A collapsed panel keeps only its handle and tab row; its height waits for its return. */
export const COLLAPSED_HEIGHT = 42;
export const outputCollapsed = ref(false);
export const terminalCollapsed = ref(false);
/** Collapsing and expanding animate for this long (ms). */
export const COLLAPSE_MS = 220;

/** While dragging, a height this close to the other panel's takes it: the top lines line up. */
const SNAP = 12;

/**
 * Handle behaviour for a bottom panel: drag or arrow keys change its `height`, up to what
 * `max()` allows (the area under the header). `snapTo()` is the other panel's height, or null
 * when it isn't on screen; near it, the height snaps to it, and arrow keys stop at it.
 */
export function useResizer(height: Ref<number>, max: () => number, snapTo: () => number | null = () => null) {
  let drag: { startY: number; startHeight: number; max: number } | null = null;
  /** While a drag is on, the panel follows the pointer without animating. */
  const dragging = ref(false);

  function limit(): number {
    return Math.max(PANEL_MIN, max());
  }

  function start(event: PointerEvent): void {
    const top = limit();
    drag = { startY: event.clientY, startHeight: Math.min(height.value, top), max: top };
    dragging.value = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function move(event: PointerEvent): void {
    if (!drag) return;
    let wanted = drag.startHeight + (drag.startY - event.clientY);
    const target = snapTo();
    if (target !== null && Math.abs(wanted - target) <= SNAP) wanted = target;
    height.value = Math.min(drag.max, Math.max(PANEL_MIN, wanted));
  }

  function end(): void {
    drag = null;
    dragging.value = false;
  }

  /** Arrow keys on the handle, for keyboard users. */
  function nudge(event: KeyboardEvent): void {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const top = limit();
    const step = event.key === "ArrowUp" ? 24 : -24;
    const current = Math.min(height.value, top);
    let next = Math.min(top, Math.max(PANEL_MIN, current + step));
    const target = snapTo();
    if (target !== null && (current - target) * (next - target) < 0) next = target;
    height.value = next;
  }

  return { start, move, end, nudge, dragging };
}
