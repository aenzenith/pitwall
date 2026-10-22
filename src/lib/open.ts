// Where a project opens from each place (Settings › Projects, a project's settings): its Files
// window, its editor, or the project in Pitwall's list. Pitwall's choice is in its settings, a
// project's own in its settings; both are read leniently here, a value not known being the
// default.

import { emitTo } from "@tauri-apps/api/event";
import { ref } from "vue";

import { showFiles } from "./explorer";
import { projectSettings } from "./projectSettings";
import { api, snapshot } from "./store";
import type { OpenSettings, OpenTarget } from "./types";

/** A double click or ↵ on its row in the list (`list`), a double click on it on the window's
 * other pages (`pages`: the Track page, the Today page), and from outside the window (`outside`:
 * the menu bar's popover, the quick switcher, the mini tower). */
export type OpenFrom = keyof OpenSettings;

/** What each place can open, its default first. The list doesn't open a project in itself. */
export const OPEN_TARGETS: Record<OpenFrom, readonly OpenTarget[]> = {
  list: ["files", "editor"],
  pages: ["list", "editor", "files"],
  outside: ["list", "editor", "files"],
};

export const OPEN_LABELS = { files: "open.files", editor: "open.editor", list: "open.list" } as const;

/** A page asks the window for the project in the list (WindowApp takes it). */
export const listRequest = ref<{ path: string; at: number } | null>(null);

function known(from: OpenFrom, value: unknown): OpenTarget | undefined {
  return OPEN_TARGETS[from].find((target) => target === value);
}

/** Pitwall's choice for `from`. */
export function generalOpen(from: OpenFrom): OpenTarget {
  return known(from, snapshot.value?.settings.open?.[from]) ?? OPEN_TARGETS[from][0];
}

/** Where the project at `path` opens from `from`: its own choice, else Pitwall's. One that isn't
 * listed has no Files window and no row: its editor. */
export function openTarget(path: string, from: OpenFrom): OpenTarget {
  const project = snapshot.value?.projects.find((p) => p.path === path);
  if (!project) return "editor";
  return known(from, projectSettings(project).open?.[from]) ?? generalOpen(from);
}

/** Opened in Pitwall's window: from its row in the list, or from another page. */
export function openInWindow(path: string, from: "list" | "pages"): void {
  const target = openTarget(path, from);
  if (target === "editor") void api.openEditor(path);
  else if (target === "files") showFiles(path);
  else listRequest.value = { path, at: Date.now() };
}

/** Opened from outside the window: its editor, or Pitwall's window on its Files window or on the
 * project in the list. */
export function openFromOutside(path: string): void {
  const target = openTarget(path, "outside");
  if (target === "editor") void api.openEditor(path);
  else revealOutside(path, target);
}

/** A folder of the projects folder that isn't listed, opened from the quick switcher: as Pitwall's
 * choice for outside the window says. Its editor; else it is added to Pitwall first, as the window
 * shows a listed project only. */
export async function openFolderFromOutside(path: string): Promise<void> {
  const target = generalOpen("outside");
  if (target === "editor") {
    void api.openEditor(path);
    return;
  }
  await api.addProject(path);
  revealOutside(path, target);
}

function revealOutside(path: string, target: "list" | "files"): void {
  void emitTo("main", target === "files" ? "reveal-files" : "reveal-project", { path });
  void api.openWindow();
}
