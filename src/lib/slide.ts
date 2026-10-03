import { watch, type Ref, type WatchSource } from "vue";

/** How long an element takes to slide to its new place (ms), and how: as a project list's rows
 * make room for a dragged one. */
const SLIDE = { duration: 180, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" };

/** How long a box takes to reach its new height (ms), and how. Slower than a slide and gentle at
 * its start: a height changes by far more than a row's step, and a slide's curve would cover
 * most of it within two frames, which reads as the jump it is there to replace. */
const GROW = { duration: 240, easing: "cubic-bezier(0.2, 0, 0, 1)" };

/** The system asks for less motion: everything takes its place at once. */
export function stillMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Elements that slide to their new place rather than jump there: every `[data-slide]` under
 * `root`, known by that attribute's value. Whenever `order` changes, each one drawn both before
 * and after under the same name starts where it was and slides to where it now is; a new one
 * just takes its place. A slide under way goes on from where it had got to.
 *
 * The box they sit in, as tall as they are (`[data-slide-box]`), grows or shrinks along with them
 * rather than at once, and shows no scrollbar meanwhile: a card on its way would otherwise stick
 * out of a box already shrunk, or be cut off by it.
 */
export function useSlide(root: Ref<HTMLElement | null>, order: WatchSource<unknown>): void {
  let places = new Map<string, DOMRect>();
  let heights = new Map<HTMLElement, number>();
  const sliding = new WeakMap<HTMLElement, Animation>();
  const resizing = new WeakMap<HTMLElement, Animation>();

  const all = (selector: string): HTMLElement[] => Array.from(root.value?.querySelectorAll<HTMLElement>(selector) ?? []);
  const drawn = (box: DOMRect): boolean => box.width > 0 || box.height > 0;
  const name = (el: HTMLElement): string => el.dataset.slide ?? "";

  // Before the new order is drawn: where each one is, and how tall its box.
  watch(
    order,
    () => {
      places = new Map();
      heights = new Map();
      if (stillMotion()) return;
      for (const el of all("[data-slide]")) {
        const box = el.getBoundingClientRect();
        if (drawn(box)) places.set(name(el), box);
      }
      for (const box of all("[data-slide-box]")) heights.set(box, box.getBoundingClientRect().height);
    },
    { flush: "pre" },
  );

  // Once it is drawn: from there to its new place.
  watch(
    order,
    () => {
      const was = places;
      const tall = heights;
      places = new Map();
      heights = new Map();
      if (!was.size && !tall.size) return;

      // What was on its way stops, so each is measured at rest.
      const moved = all("[data-slide]").filter((el) => was.has(name(el)));
      const boxes = all("[data-slide-box]").filter((box) => tall.has(box));
      for (const el of moved) sliding.get(el)?.cancel();
      for (const box of boxes) {
        resizing.get(box)?.cancel();
        resizing.delete(box);
        box.style.overflowY = "";
      }
      const now = moved.map((el) => el.getBoundingClientRect());
      const grown = boxes.map((box) => box.getBoundingClientRect().height);

      moved.forEach((el, i) => {
        const from = was.get(name(el));
        const to = now[i];
        if (!from || !drawn(to)) return;
        const dx = from.left - to.left;
        const dy = from.top - to.top;
        if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return;
        sliding.set(el, el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], SLIDE));
      });

      boxes.forEach((box, i) => {
        const from = tall.get(box) ?? grown[i];
        if (Math.abs(from - grown[i]) < 0.5) return;
        const run = box.animate([{ height: `${from}px` }, { height: `${grown[i]}px` }], SLIDE);
        const rest = (): void => {
          if (resizing.get(box) !== run) return;
          resizing.delete(box);
          box.style.overflowY = "";
        };
        run.onfinish = rest;
        run.oncancel = rest;
        resizing.set(box, run);
        box.style.overflowY = "hidden";
      });
    },
    { flush: "post" },
  );
}

/**
 * A box as tall as what it holds, growing or shrinking to its new height rather than jumping
 * there: whenever `content` changes, it starts as tall as it was. A resize under way goes on from
 * where it had got to.
 *
 * Meanwhile the box carries `data-growing`, for what scrolls inside it to show no scrollbar: the
 * new content, squeezed into the old height for a moment, would flash one.
 */
export function useGrow(box: Ref<HTMLElement | null>, content: WatchSource<unknown>): void {
  let from: number | null = null;
  let run: Animation | null = null;

  /** The layout's height: a scale on the box (as a dialog pops in) doesn't count. */
  const height = (el: HTMLElement): number => parseFloat(getComputedStyle(el).height);

  // Before the new content is drawn: how tall the box is.
  watch(
    content,
    () => {
      from = box.value && !stillMotion() ? height(box.value) : null;
    },
    { flush: "pre" },
  );

  // Once it is drawn: from there to its new height.
  watch(
    content,
    () => {
      const el = box.value;
      const was = from;
      from = null;
      if (!el || was === null) return;

      // What was on its way stops, so the box is measured at rest.
      run?.cancel();
      run = null;
      delete el.dataset.growing;
      const to = height(el);
      if (!(Math.abs(was - to) >= 0.5)) return;

      const mine = el.animate([{ height: `${was}px` }, { height: `${to}px` }], GROW);
      const rest = (): void => {
        if (run !== mine) return;
        run = null;
        delete el.dataset.growing;
      };
      mine.onfinish = rest;
      mine.oncancel = rest;
      run = mine;
      el.dataset.growing = "";
    },
    { flush: "post" },
  );
}
