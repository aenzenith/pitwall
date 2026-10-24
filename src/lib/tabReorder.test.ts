// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { ref } from "vue";

import { useTabReorder, type Elsewhere } from "./tabReorder";

/** A row of three tabs on the page. */
function row(): HTMLElement {
  const list = document.createElement("div");
  for (const id of [1, 2, 3]) {
    const tab = document.createElement("div");
    tab.dataset.tabId = String(id);
    list.append(tab);
  }
  document.body.replaceChildren(list);
  return list;
}

/** Tab 2 pressed, dragged to (`x`, `y`) and let go there. */
function drag(list: HTMLElement, press: (event: PointerEvent) => void, x: number, y: number): void {
  const tab = list.querySelector<HTMLElement>('[data-tab-id="2"]')!;
  tab.addEventListener("pointerdown", press, { once: true });
  tab.dispatchEvent(new PointerEvent("pointerdown", { button: 0, buttons: 1, clientX: 10, clientY: 10, bubbles: true }));
  for (let step = 1; step <= 4; step++) window.dispatchEvent(new PointerEvent("pointermove", { buttons: 1, clientX: 10 + ((x - 10) * step) / 4, clientY: 10 + ((y - 10) * step) / 4 }));
  window.dispatchEvent(new PointerEvent("pointerup", { button: 0, clientX: x, clientY: y }));
}

describe("a tab dragged in the terminal panel's row", () => {
  // The panel gives the keyboard back to the terminal shown when it hears the drag is over: left on
  // the tab's label, the next Backspace would close that tab and end its shell.
  it("says it is over wherever it is let go: on a terminal that doesn't take it, one that does, or in the row", async () => {
    for (const where of ["not taken", "taken", "row"] as const) {
      const list = row();
      let over = 0;
      // The terminals lie under the row (y past 100); the middle of the shown one takes no tab.
      const terminals: Elsewhere = { over: (_id, _x, y) => y > 100, drop: () => where === "taken", leave: () => undefined };
      const tabs = useTabReorder(ref(list), () => undefined, terminals, () => over++);
      drag(list, (event) => tabs.down(event, 2), where === "row" ? 60 : 10, where === "row" ? 10 : 300);
      expect(over, where).toBe(1);
      // The tab slides into its place in the row before the drag is all done.
      await new Promise((resolve) => setTimeout(resolve, 250));
      expect(tabs.dragging.value, where).toBeNull();
    }
  });
});
