import { nextTick, ref, type Ref } from "vue";

/** Pixels the pointer must travel before a press becomes a drag; less is a click. */
const THRESHOLD = 5;
/** How long the dropped tab takes to slide into its place (ms); the CSS uses the same. */
const SETTLE_MS = 160;

/** Where the tabs were when the drag began, in the row's own coordinates. */
type Layout = { ids: number[]; lefts: number[]; widths: number[]; from: number; shift: number };

/**
 * Drag-to-reorder for a row of tabs, as lib/reorder does it for the list's rows but sideways: the
 * dragged tab follows the pointer along the row, the others slide aside to show where it will
 * land, and on release it slides into that place before `onDrop` is given the new order. Tabs
 * carry `data-tab-id`; controls marked `data-no-drag` keep their own clicks and never start a
 * drag. Pointer events rather than HTML drag and drop, which Tauri's file-drop handling can
 * swallow.
 *
 * Bind `listClass()` on the row, `tabClass(id)` and `tabStyle(id)` on each tab.
 */
export function useTabReorder(list: Ref<HTMLElement | null>, onDrop: (ids: number[]) => void) {
  const dragging = ref<number | null>(null);
  /** Horizontal offset per tab while a drag is on. */
  const offsets = ref<Record<number, number>>({});
  /** The dropped tab is sliding into place. */
  const settling = ref(false);
  /** Transitions off for the instant the real order replaces the offsets. */
  const frozen = ref(false);

  let press: { id: number; x: number } | null = null;
  let layout: Layout | null = null;
  let target = -1;

  function down(event: PointerEvent, id: number): void {
    if (event.button !== 0 || settling.value || (event.target as HTMLElement).closest("[data-no-drag]")) return;

    press = { id, x: event.clientX };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    window.addEventListener("pointercancel", abort);
    window.addEventListener("blur", abort);
  }

  function measure(id: number): Layout | null {
    const root = list.value;
    if (!root) return null;

    const tabs = Array.from(root.querySelectorAll<HTMLElement>("[data-tab-id]"));
    const origin = root.getBoundingClientRect().left;
    const ids = tabs.map((tab) => Number(tab.dataset.tabId));
    const lefts = tabs.map((tab) => tab.getBoundingClientRect().left - origin);
    const widths = tabs.map((tab) => tab.getBoundingClientRect().width);
    const from = ids.indexOf(id);
    if (from < 0 || tabs.length < 2) return null;

    // Tabs make room by the dragged tab's width plus the gap between tabs.
    const gap = Math.max(0, lefts[1] - lefts[0] - widths[0]);
    return { ids, lefts, widths, from, shift: widths[from] + gap };
  }

  function move(event: PointerEvent): void {
    if (!press) return;
    // Released where the page couldn't hear it (a native menu took the mouse-up): no drag.
    if (!(event.buttons & 1)) return abort();

    if (dragging.value === null) {
      if (Math.abs(event.clientX - press.x) < THRESHOLD) return;
      layout = measure(press.id);
      if (!layout) return;
      dragging.value = press.id;
      target = layout.from;
      document.body.classList.add("grabbing");
    }

    place(event.clientX);
  }

  /** Puts the dragged tab under the pointer, inside the row, and slides the others to make room. */
  function place(pointerX: number): void {
    if (!press || !layout) return;

    const { ids, lefts, widths, from, shift } = layout;
    const last = ids.length - 1;
    const lowest = lefts[0] - lefts[from];
    const highest = lefts[last] + widths[last] - widths[from] - lefts[from];
    const dx = Math.min(highest, Math.max(lowest, pointerX - press.x));
    const left = lefts[from] + dx;
    const right = left + widths[from];

    // Its new place: a tab trades places once the dragged tab's edge passes its middle (the left
    // edge for tabs before it, the right edge for tabs after it).
    target = ids.reduce((count, _, i) => {
      if (i === from) return count;
      const middle = lefts[i] + widths[i] / 2;
      const before = i < from ? left >= middle : right > middle;
      return before ? count + 1 : count;
    }, 0);

    const next: Record<number, number> = { [ids[from]]: dx };
    ids.forEach((id, i) => {
      if (i === from) return;
      if (from < target && i > from && i <= target) next[id] = -shift;
      else if (from > target && i >= target && i < from) next[id] = shift;
    });
    offsets.value = next;
  }

  /** The pointer is let go of: nothing follows it any more. */
  function release(): void {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", abort);
    window.removeEventListener("blur", abort);
    document.body.classList.remove("grabbing");
  }

  /** The press is called off: the tabs are back where they were and the order stays. */
  function abort(): void {
    release();
    press = null;
    layout = null;
    offsets.value = {};
    dragging.value = null;
  }

  function up(): void {
    release();

    const moved = dragging.value;
    const drop = layout;
    press = null;

    if (moved === null || !drop) {
      dragging.value = null;
      return;
    }

    // Slide the tab into the slot the others opened, then keep the order.
    const { lefts, widths, from } = drop;
    const left = target > from ? lefts[target] + widths[target] - widths[from] : lefts[target];
    settling.value = true;
    offsets.value = { ...offsets.value, [moved]: left - lefts[from] };

    window.setTimeout(() => void finish(moved, drop, target), SETTLE_MS);
  }

  async function finish(moved: number, drop: Layout, to: number): Promise<void> {
    // The real order takes over without a frame of movement.
    frozen.value = true;
    if (to !== drop.from) {
      const ids = drop.ids.filter((id) => id !== moved);
      ids.splice(to, 0, moved);
      onDrop(ids);
    }
    offsets.value = {};
    dragging.value = null;
    settling.value = false;
    layout = null;

    await nextTick();
    requestAnimationFrame(() => (frozen.value = false));
  }

  function listClass(): Record<string, boolean> {
    return { reordering: dragging.value !== null, frozen: frozen.value };
  }

  function tabClass(id: number): Record<string, boolean> {
    return { dragging: dragging.value === id, settling: settling.value && dragging.value === id };
  }

  /** `translate` rather than `transform`: a TransitionGroup around the tabs moves them with
   * `transform` itself whenever it sees a transition on it, and would wipe the offsets. */
  function tabStyle(id: number): Record<string, string> | undefined {
    const offset = offsets.value[id];
    return offset ? { translate: `${offset}px 0` } : undefined;
  }

  return { dragging, down, listClass, tabClass, tabStyle };
}
