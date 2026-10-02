import { inject, nextTick, onBeforeUnmount, provide, ref, type InjectionKey, type Ref } from "vue";

import type { Tip } from "./day";

/**
 * The day view's one tooltip, over whatever the pointer or the keyboard is on, and what the
 * keyboard just showed for VoiceOver to read out. The view makes it (`provideTooltip`) and draws
 * it; its parts reach it with `useTooltip`.
 */
export type Tooltip = {
  /** What it shows, and where: its anchor's top centre, in window pixels. */
  tip: Ref<(Tip & { x: number; y: number }) | null>;
  /** What the keyboard just showed, for a polite live region. */
  announced: Ref<string>;
  /** Over the element the pointer came onto. */
  show(event: PointerEvent, shown: Tip): void;
  /** Over `element`: the keyboard's way to it as well as the pointer's. */
  showAt(element: Element, shown: Tip): void;
  /** Read out `shown`, as the keyboard reached it. */
  announce(shown: Tip): void;
  hide(): void;
  /** `callback` runs on every `hide`, while the calling component is there. */
  onHide(callback: () => void): void;
};

const KEY: InjectionKey<Tooltip> = Symbol("tooltip");

/** Makes the tooltip and hands it to the components below; `el` is the drawn tooltip, kept
 * inside the window. */
export function provideTooltip(el: Ref<HTMLElement | null>): Tooltip {
  const tip: Tooltip["tip"] = ref(null);
  const announced = ref("");
  const hooks = new Set<() => void>();

  function showAt(element: Element, shown: Tip): void {
    const rect = element.getBoundingClientRect();
    tip.value = { ...shown, x: rect.left + rect.width / 2, y: rect.top };
    void nextTick(() => {
      // Kept inside the window.
      const drawn = el.value;
      if (!drawn || !tip.value) return;
      const half = drawn.offsetWidth / 2;
      tip.value.x = Math.min(Math.max(tip.value.x, half + 8), window.innerWidth - half - 8);
    });
  }

  const tooltip: Tooltip = {
    tip,
    announced,
    show: (event, shown) => showAt(event.currentTarget as HTMLElement, shown),
    showAt,
    announce(shown) {
      announced.value = [shown.title, shown.detail].filter(Boolean).join(" ");
    },
    hide() {
      tip.value = null;
      announced.value = "";
      for (const hook of hooks) hook();
    },
    onHide(callback) {
      hooks.add(callback);
      onBeforeUnmount(() => hooks.delete(callback));
    },
  };

  provide(KEY, tooltip);
  return tooltip;
}

/** The tooltip `provideTooltip` made above. */
export function useTooltip(): Tooltip {
  const tooltip = inject(KEY);
  if (!tooltip) throw new Error("useTooltip() needs provideTooltip() above it");
  return tooltip;
}
