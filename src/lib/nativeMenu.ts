import { Menu, MenuItem, PredefinedMenuItem, type MenuItemOptions } from "@tauri-apps/api/menu";
import { LogicalPosition } from "@tauri-apps/api/window";
import { onBeforeUnmount } from "vue";

/** A line of a context menu: an item, or a separator between items. */
export type MenuEntry = MenuItemOptions | "separator";

/**
 * Right-click menus: the native menu with what a row can do, opened by `popup`.
 *
 * - Items are made one by one: items given inline to `Menu.new` lose their click handlers
 *   (Tauri 2.12 drops them right after building the menu, and dropping one unregisters it).
 * - It opens at the click point: popped up without a position, macOS leaves it stuck open
 *   (clicks outside move it instead of closing it) and the app frozen behind it.
 * - The last menu and its items are freed when the next one opens, or when the component goes:
 *   freeing them as soon as the popup returns could drop an item's click handler before its
 *   click arrives.
 */
export function useNativeMenu(): { popup: (event: MouseEvent, entries: MenuEntry[]) => Promise<void> } {
  let last: { close(): Promise<void> }[] = [];
  let gone = false;

  function free(): void {
    for (const resource of last.splice(0)) void resource.close();
  }

  async function popup(event: MouseEvent, entries: MenuEntry[]): Promise<void> {
    free();

    const items = await Promise.all(
      entries.map((entry) => (entry === "separator" ? PredefinedMenuItem.new({ item: "Separator" }) : MenuItem.new(entry))),
    );
    const menu = await Menu.new({ items });
    last = [menu, ...items];
    // The component went while the menu was being made: it never opens.
    if (gone) return free();
    await menu.popup(new LogicalPosition(event.clientX, event.clientY));
  }

  onBeforeUnmount(() => {
    gone = true;
    free();
  });

  return { popup };
}
