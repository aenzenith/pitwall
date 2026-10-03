import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu, type MenuItemOptions } from "@tauri-apps/api/menu";
import { LogicalPosition } from "@tauri-apps/api/window";
import { onBeforeUnmount } from "vue";

/** A submenu: its name, and its own lines. */
export type SubmenuEntry = { text: string; enabled?: boolean; items: MenuEntry[] };

/** An item with a check mark beside it when `checked`: the choice in force. */
export type CheckEntry = MenuItemOptions & { checked: boolean };

/** A line of a context menu: an item, a separator between items, or a submenu. */
export type MenuEntry = MenuItemOptions | CheckEntry | "separator" | SubmenuEntry;

/** Where a menu opens, in the window: a click's point, or a row's corner for the keyboard. */
export type MenuPoint = { clientX: number; clientY: number };

type Resource = { close(): Promise<void> };

/** One line, made on its own (a submenu's lines first), kept in `made` to be freed later. */
async function build(entry: MenuEntry, made: Resource[]): Promise<MenuItem | CheckMenuItem | PredefinedMenuItem | Submenu> {
  if (entry === "separator") {
    const separator = await PredefinedMenuItem.new({ item: "Separator" });
    made.push(separator);
    return separator;
  }
  if ("items" in entry) {
    const items = await Promise.all(entry.items.map((child) => build(child, made)));
    const submenu = await Submenu.new({ text: entry.text, enabled: entry.enabled ?? true, items });
    made.push(submenu);
    return submenu;
  }
  if ("checked" in entry) {
    const check = await CheckMenuItem.new(entry);
    made.push(check);
    return check;
  }
  const item = await MenuItem.new(entry);
  made.push(item);
  return item;
}

/**
 * Right-click menus: the native menu with what a row can do, opened by `popup`.
 *
 * - Items are made one by one: items given inline to `Menu.new` lose their click handlers
 *   (Tauri 2.12 drops them right after building the menu, and dropping one unregisters it).
 *   So are a submenu's, before the submenu that holds them.
 * - It opens at the click point: popped up without a position, macOS leaves it stuck open
 *   (clicks outside move it instead of closing it) and the app frozen behind it.
 * - The last menu and its items are freed when the next one opens, or when the component goes:
 *   freeing them as soon as the popup returns could drop an item's click handler before its
 *   click arrives.
 */
export function useNativeMenu(): { popup: (at: MenuPoint, entries: MenuEntry[]) => Promise<void> } {
  let last: Resource[] = [];
  let gone = false;

  function free(): void {
    for (const resource of last.splice(0)) void resource.close();
  }

  async function popup(at: MenuPoint, entries: MenuEntry[]): Promise<void> {
    free();

    const made: Resource[] = [];
    const items = await Promise.all(entries.map((entry) => build(entry, made)));
    const menu = await Menu.new({ items });
    last = [menu, ...made];
    // The component went while the menu was being made: it never opens.
    if (gone) return free();
    await menu.popup(new LogicalPosition(at.clientX, at.clientY));
  }

  onBeforeUnmount(() => {
    gone = true;
    free();
  });

  return { popup };
}
