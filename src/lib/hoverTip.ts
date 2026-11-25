import { onBeforeUnmount, shallowRef, type ShallowRef } from "vue";

/** What a tooltip stands by: its key, and the box of the element it was shown over. */
export type HoverTipAt<K> = { key: K; anchor: DOMRect };

export type HoverTip<K> = {
  shown: ShallowRef<HoverTipAt<K> | null>;
  /** The pointer came onto `element`: its tooltip shows after a moment, at once while another
   * one shows or has just gone (moving along a row of them). */
  enter(element: Element, key: K): void;
  /** Over `element` now: the keyboard's way to it. */
  showAt(element: Element, key: K): void;
  /** The pointer left: the next one along still shows at once for a moment. */
  leave(): void;
  /** A press, a drag, a scroll: gone, and the next one waits again. */
  hide(): void;
};

/** How long the pointer rests before a tooltip shows, as the system's do. */
const DELAY_MS = 450;
/** After one went, how long the next one along shows at once. */
const WARM_MS = 300;

/**
 * Pitwall's own tooltip for a row of things (the terminal panel's tabs), in place of the system's
 * `title`: it shows sooner, moves along the row as the pointer does, and the page draws it where it
 * has room. Only when and over what; the page draws what it says.
 */
export function useHoverTip<K>(): HoverTip<K> {
  const shown = shallowRef<HoverTipAt<K> | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let warm = 0;

  function showAt(element: Element, key: K): void {
    clearTimeout(timer);
    shown.value = { key, anchor: element.getBoundingClientRect() };
  }

  function enter(element: Element, key: K): void {
    clearTimeout(timer);
    if (shown.value || performance.now() < warm) showAt(element, key);
    else timer = setTimeout(() => showAt(element, key), DELAY_MS);
  }

  function leave(): void {
    clearTimeout(timer);
    if (shown.value) warm = performance.now() + WARM_MS;
    shown.value = null;
  }

  function hide(): void {
    clearTimeout(timer);
    warm = 0;
    shown.value = null;
  }

  onBeforeUnmount(hide);
  return { shown, enter, showAt, leave, hide };
}
