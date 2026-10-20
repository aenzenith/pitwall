// The project details' link dialog: which project's link it edits, and where its Save and Delete
// write. No component state and nothing of the core's: the writer is handed in.

import { ref, watch } from "vue";

import type { ProjectLink } from "./types";

/** A change to a project's links: the list as it is in, the list to save out. */
export type LinksChange = (list: ProjectLink[]) => ProjectLink[];

/**
 * The dialog holds the project it was opened for with the link, by its place in that project's
 * list (`index`; `null` for a new one), and its Save and Delete write into that project only. The
 * shown project can change while it is open, by itself too (a filter it drops out of, a
 * notification's project brought up): the dialog then closes, as the command dialog does.
 */
export function useLinkDialog(shown: () => string, write: (path: string, change: LinksChange) => void) {
  const dialog = ref<{ path: string; index: number | null } | null>(null);

  watch(shown, () => (dialog.value = null));

  function open(index: number | null): void {
    dialog.value = { path: shown(), index };
  }

  function save(link: ProjectLink): void {
    const asked = dialog.value;
    dialog.value = null;
    if (!asked) return;
    const at = asked.index;
    write(asked.path, (list) => (at === null ? [...list, link] : list.map((l, i) => (i === at ? link : l))));
  }

  function remove(): void {
    const asked = dialog.value;
    dialog.value = null;
    const at = asked?.index ?? null;
    if (asked && at !== null) write(asked.path, (list) => list.filter((_, i) => i !== at));
  }

  function close(): void {
    dialog.value = null;
  }

  return { dialog, open, save, remove, close };
}
