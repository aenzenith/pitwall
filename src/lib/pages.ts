// The window's pages: its sidebar from top to bottom, which the quick switcher lists after `/`.

import type { Key } from "./i18n";
import type { Project } from "./types";

/** The project list's filters. */
export type Filter = "all" | "running" | "favourites" | "waiting";
/** A page of the window other than the project list. */
export type View = "track" | "day" | "sessions" | "board" | "fuel" | "deps";
/** A sidebar entry: `add` asks for a folder, `settings` opens over the page shown. */
export type Page = Filter | View | "add" | "settings";

export const filters: Array<{ id: Filter; label: Key; icon: "grid" | "pulse" | "star" | "chat"; test: (p: Project) => boolean }> = [
  { id: "all", label: "window.filter.all", icon: "grid", test: () => true },
  { id: "running", label: "window.filter.running", icon: "pulse", test: (p) => p.status === "running" },
  { id: "favourites", label: "window.filter.favourites", icon: "star", test: (p) => p.favourite },
  { id: "waiting", label: "window.filter.waiting", icon: "chat", test: (p) => p.claude !== null },
];

/** At work: its server up or on its way, a command of it running, Claude at work or waiting there.
 * With the favourites, all the Track page draws, and part of what the list's "Favourites and open"
 * shows. */
export function atWork(project: Project): boolean {
  return (
    project.status === "running" ||
    project.status === "busy" ||
    project.commands.some((command) => command.status === "running" || command.status === "busy") ||
    project.claude !== null ||
    project.claudeWorking
  );
}

/** Open or at work: its server crashed too, a VS Code window on it, a terminal of it here. With the
 * favourites, what the list's "Favourites and open" and the terminal panel's picker (Settings ›
 * Projects) show. */
export function openOrAtWork(project: Project): boolean {
  return atWork(project) || project.status === "crashed" || project.openIn !== null || project.terminals.length > 0;
}

/** One row of the switcher's `/`; a filter brings its `test`, for the count the sidebar shows. */
export type PageEntry = {
  id: Page;
  label: Key;
  icon: "grid" | "pulse" | "star" | "chat" | "flag" | "calendar" | "sparkles" | "board" | "fuel" | "tools" | "plus" | "settings";
  test?: (p: Project) => boolean;
};

export const pages: PageEntry[] = [
  ...filters,
  { id: "day", label: "day.nav", icon: "calendar" },
  { id: "sessions", label: "sessions.nav", icon: "sparkles" },
  { id: "board", label: "board.nav", icon: "board" },
  { id: "track", label: "track.nav", icon: "flag" },
  { id: "fuel", label: "fuel.nav", icon: "fuel" },
  { id: "deps", label: "deps.nav", icon: "tools" },
  { id: "add", label: "window.addProject", icon: "plus" },
  { id: "settings", label: "common.settings", icon: "settings" },
];

export function isFilter(page: Page): page is Filter {
  return filters.some((filter) => filter.id === page);
}
