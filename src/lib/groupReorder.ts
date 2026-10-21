import { nextTick, ref, type Ref } from "vue";

/** Pixels the pointer must travel before a press becomes a drag; less is a click. */
const THRESHOLD = 5;
/** How long the dropped lines take to slide into place (ms); the CSS uses the same. */
const SETTLE_MS = 160;
/** Near the list's top or bottom edge (px) a drag scrolls the list, faster the closer it gets. */
const SCROLL_ZONE = 40;
const SCROLL_SPEED = 14;

/** A line's key: a project's row, a group's head (`""` names the group of projects in no team),
 * or the line a group with no rows shows under its head. */
export const rowKey = (path: string): string => `row:${path}`;
export const headKey = (group: string): string => `head:${group}`;
export const hintKey = (group: string): string => `hint:${group}`;

/** What a key names: `row`, `head` or `hint`, and the path or group after it. */
export function parseKey(key: string): { kind: "row" | "head" | "hint"; id: string } {
  const at = key.indexOf(":");
  return { kind: key.slice(0, at) as "row" | "head" | "hint", id: key.slice(at + 1) };
}

/**
 * A drop: `moved` is the line that was dragged (a head brings its group's lines along), `keys`
 * every line of the list as it now stands, top to bottom. `beside` is the line it last traded
 * places with (for a head, that group's head) and `after` tells on which side of it it landed.
 */
export type Drop = { moved: string; keys: string[]; beside: string; after: boolean };

/** One line of the list as it stood when the drag began, in the list's own coordinates. */
type Line = { key: string; top: number; bottom: number };

/** Lines that give way as one: a single line (a row is dragged), or a group's block (a head is). */
type Unit = { keys: string[]; top: number; bottom: number };

/**
 * Where everything was when the drag began. `run`: the dragged lines, `units`: what trades places
 * with them (`from` of them above the run), `before` and `after`: lines that keep their place
 * above and below all of that. `lo`..`hi`: how many units may end up above the run. `free`: the
 * run may be held in the empty space under the last line. `padTop`, `padBottom`: the list's own
 * padding, which the run keeps from the list's edges.
 */
type Layout = {
  run: string[];
  top: number;
  height: number;
  units: Unit[];
  from: number;
  lo: number;
  hi: number;
  free: boolean;
  before: string[];
  after: string[];
  padTop: number;
  padBottom: number;
};

function keyOf(line: HTMLElement): string {
  if (line.dataset.path !== undefined) return rowKey(line.dataset.path);
  if (line.dataset.group !== undefined) return headKey(line.dataset.group);
  return hintKey(line.dataset.hint ?? "");
}

/**
 * Drag-to-reorder for a list in groups, with the feel of lib/reorder: the dragged line follows
 * the pointer, the others slide aside to show where it will land, and on release it slides into
 * that place before `drop` is told. A long list scrolls while the line is held near its top or
 * bottom edge.
 *
 * The list's lines are its direct children: rows carry `data-path`, heads `data-group` (empty
 * for the group that is no team: its head never moves), and the line of a group with no rows
 * `data-hint`. A row goes anywhere under a head, into another group too; rows that carry
 * `data-zone` (a list drawn without its heads) stay among the neighbours with the same zone. A
 * head moves among the other heads and takes its group's lines along. Lines have no margins:
 * each begins where the one before ends. Controls marked `data-no-drag` (and inputs) keep their
 * own clicks and never start a drag.
 *
 * Bind `listClass()` on the list, `lineClass(key)` and `lineStyle(key)` on each line.
 */
export function useGroupReorder(list: Ref<HTMLElement | null>, drop: (change: Drop) => void) {
  const dragging = ref<string | null>(null);
  /** The lines that travel with the pointer: the dragged one, and for a head its group's. */
  const carried = ref<string[]>([]);
  /** Vertical offset per line while a drag is on. */
  const offsets = ref<Record<string, number>>({});
  /** The dropped lines are sliding into place. */
  const settling = ref(false);
  /** Transitions off for the instant the real order replaces the offsets. */
  const frozen = ref(false);

  let press: { key: string; y: number; scroll: number } | null = null;
  let layout: Layout | null = null;
  let target = -1;
  let pointerY = 0;
  let scrolling = 0;
  let swallowClick = false;

  function down(event: PointerEvent, key: string): void {
    if (event.button !== 0 || settling.value || (event.target as HTMLElement).closest("[data-no-drag], input, textarea")) return;

    press = { key, y: event.clientY, scroll: list.value?.scrollTop ?? 0 };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    window.addEventListener("pointercancel", abort);
    window.addEventListener("blur", abort);
  }

  function measure(key: string): Layout | null {
    const root = list.value;
    if (!root) return null;

    const nodes = Array.from(root.querySelectorAll<HTMLElement>(":scope > [data-path], :scope > [data-group], :scope > [data-hint]"));
    const origin = root.getBoundingClientRect().top - root.scrollTop;
    const lines: Line[] = nodes.map((node) => {
      const box = node.getBoundingClientRect();
      return { key: keyOf(node), top: box.top - origin, bottom: box.bottom - origin };
    });
    const at = lines.findIndex((line) => line.key === key);
    if (at < 0) return null;

    const style = getComputedStyle(root);
    const pads = { padTop: parseFloat(style.paddingTop) || 0, padBottom: parseFloat(style.paddingBottom) || 0 };
    const one = (line: Line): Unit => ({ keys: [line.key], top: line.top, bottom: line.bottom });

    if (parseKey(key).kind === "row") {
      const units = lines.filter((_, i) => i !== at).map(one);
      // Under a head, never above the first one; among its zone's rows where the heads aren't drawn.
      let lo = nodes[0].dataset.group !== undefined ? 1 : 0;
      let hi = units.length;
      const zone = nodes[at].dataset.zone;
      if (zone !== undefined) {
        const same = (i: number): boolean => nodes[i]?.dataset.path !== undefined && nodes[i].dataset.zone === zone;
        lo = at;
        hi = at;
        while (same(lo - 1)) lo--;
        while (same(hi + 1)) hi++;
      }
      return { run: [key], top: lines[at].top, height: lines[at].bottom - lines[at].top, units, from: at, lo, hi, free: hi === units.length, before: [], after: [], ...pads };
    }

    // A head: the list in blocks, each a head with the lines under it. The teams' blocks trade
    // places; what comes before the first of them, or after the last, stays.
    const starts = nodes.map((node, i) => (node.dataset.group !== undefined ? i : -1)).filter((i) => i >= 0);
    const blocks = starts.map((start, i) => ({ start, end: starts[i + 1] ?? lines.length, team: nodes[start].dataset.group !== "" }));
    const own = blocks.find((block) => block.start === at);
    const teams = blocks.filter((block) => block.team);
    if (!own?.team || teams.length < 2) return null;

    const first = teams[0].start;
    const last = teams[teams.length - 1].end;
    const others = teams.filter((block) => block !== own);
    const units = others.map((block) => ({ keys: lines.slice(block.start, block.end).map((line) => line.key), top: lines[block.start].top, bottom: lines[block.end - 1].bottom }));
    return {
      run: lines.slice(own.start, own.end).map((line) => line.key),
      top: lines[own.start].top,
      height: lines[own.end - 1].bottom - lines[own.start].top,
      units,
      from: others.filter((block) => block.start < own.start).length,
      lo: 0,
      hi: units.length,
      free: last === lines.length,
      before: lines.slice(0, first).map((line) => line.key),
      after: lines.slice(last).map((line) => line.key),
      ...pads,
    };
  }

  /** Where the run's top comes to rest with `to` units above it. */
  function restingTop(drag: Layout, to: number): number {
    if (to > drag.from) return drag.units[to - 1].bottom - drag.height;
    if (to < drag.from) return drag.units[to].top;
    return drag.top;
  }

  function move(event: PointerEvent): void {
    if (!press) return;
    // Released where the page couldn't hear it (a native menu took the mouse-up): no drag.
    if (!(event.buttons & 1)) return abort();
    pointerY = event.clientY;

    if (!dragging.value) {
      if (Math.abs(event.clientY - press.y) < THRESHOLD) return;
      layout = measure(press.key);
      if (!layout) return;
      dragging.value = press.key;
      carried.value = layout.run;
      target = layout.from;
      document.body.classList.add("grabbing");
      scrolling = requestAnimationFrame(autoScroll);
    }

    place();
  }

  /** Held near an edge of a list taller than its box, the list scrolls and the line stays put. */
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

  /** Puts the dragged lines under the pointer and slides the others to make room. */
  function place(): void {
    const root = list.value;
    if (!press || !layout || !root) return;

    const { run, units, from, lo, hi, free, height, padTop, padBottom } = layout;

    // Under the pointer, the list's scrolling included. It may go anywhere in the visible list
    // (a free one in the empty space under the last line too), keeping the list's padding from
    // its edges, and no further than the first and last place it may take.
    const wanted = layout.top + pointerY - press.y + root.scrollTop - press.scroll;
    const visibleTop = root.scrollTop + padTop;
    const visibleBottom = root.scrollTop + root.clientHeight - padBottom - height;
    const floor = free ? Infinity : restingTop(layout, hi);
    const highest = Math.min(Math.max(restingTop(layout, lo), visibleTop), floor);
    const lowest = Math.max(highest, Math.min(visibleBottom, floor));
    const top = Math.min(lowest, Math.max(highest, wanted));
    const bottom = top + height;

    // Its new place: a unit trades places once the run's edge passes its middle (the top edge for
    // units above, the bottom edge for units below). Edges rather than centres, so units of
    // different heights never keep it from the first or the last place.
    const passed = units.reduce((count, unit, i) => {
      const middle = (unit.top + unit.bottom) / 2;
      return (i < from ? top >= middle : bottom > middle) ? count + 1 : count;
    }, 0);
    target = Math.min(hi, Math.max(lo, passed));

    const next: Record<string, number> = {};
    for (const key of run) next[key] = top - layout.top;
    units.forEach((unit, i) => {
      const shift = from < target && i >= from && i < target ? -height : from > target && i >= target && i < from ? height : 0;
      if (shift) for (const key of unit.keys) next[key] = shift;
    });
    offsets.value = next;
  }

  /** The pointer is let go of: nothing follows it any more. */
  function release(): void {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", abort);
    window.removeEventListener("blur", abort);
    cancelAnimationFrame(scrolling);
    document.body.classList.remove("grabbing");
  }

  /** The press is called off (its release never came, the pointer was cancelled, the window lost
   * focus): the lines are back where they were and nothing is told. */
  function abort(): void {
    release();
    press = null;
    layout = null;
    offsets.value = {};
    dragging.value = null;
    carried.value = [];
  }

  function up(): void {
    release();

    const moved = dragging.value;
    const drag = layout;
    press = null;

    if (!moved || !drag) {
      dragging.value = null;
      return;
    }

    // The click that follows the drop must not select or open anything.
    swallowClick = true;
    window.setTimeout(() => (swallowClick = false), 0);

    // Slide the run into the slot the others opened, then tell the new order.
    const rest = restingTop(drag, target) - drag.top;
    settling.value = true;
    const next = { ...offsets.value };
    for (const key of drag.run) next[key] = rest;
    offsets.value = next;

    window.setTimeout(() => void finish(moved, drag, target), SETTLE_MS);
  }

  async function finish(moved: string, drag: Layout, to: number): Promise<void> {
    // The real order takes over without a frame of movement.
    frozen.value = true;
    if (to !== drag.from) {
      const flat = (units: Unit[]): string[] => units.flatMap((unit) => unit.keys);
      const beside = to > drag.from ? drag.units[to - 1] : drag.units[to];
      drop({
        moved,
        keys: [...drag.before, ...flat(drag.units.slice(0, to)), ...drag.run, ...flat(drag.units.slice(to)), ...drag.after],
        beside: beside.keys[0],
        after: to > drag.from,
      });
    }
    offsets.value = {};
    dragging.value = null;
    carried.value = [];
    settling.value = false;
    layout = null;

    await nextTick();
    requestAnimationFrame(() => (frozen.value = false));
  }

  /** Use in a line's click handler: false right after a drag. */
  function isClick(): boolean {
    return !swallowClick;
  }

  function listClass(): Record<string, boolean> {
    return { reordering: dragging.value !== null, frozen: frozen.value };
  }

  /** `dragging`: the line in hand; `carried`: every line that travels (with a head, its group's). */
  function lineClass(key: string): Record<string, boolean> {
    const travels = carried.value.includes(key);
    return { dragging: dragging.value === key, carried: travels, settling: settling.value && travels };
  }

  function lineStyle(key: string): Record<string, string> | undefined {
    const offset = offsets.value[key];
    return offset ? { transform: `translateY(${offset}px)` } : undefined;
  }

  /** How far a line is moved while a drag is on (0: it keeps its place). */
  function offset(key: string): number {
    return offsets.value[key] ?? 0;
  }

  return { dragging, down, isClick, listClass, lineClass, lineStyle, offset };
}
