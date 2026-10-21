// The Files window's state (src/window/files/): which project it shows and where it floats, and
// per project what was opened in it: folders, Git marks, tabs and the files' text. All of it in
// this page's memory only, gone when the app quits; the core reads each thing when asked
// (core/explorer.rs).

import { reactive } from "vue";

import { language } from "./i18n";
import { isWindows } from "./platform";
import { api } from "./store";
import { parseDiff, type GutterMark, type ParsedDiff } from "./unifiedDiff";

export type Entry = { name: string; dir: boolean; link: boolean; size: number };
export type Listing = { entries: Entry[]; more: number };
export type FileView = { kind: "text"; text: string; size: number; lossy: boolean } | { kind: "binary"; size: number } | { kind: "large"; size: number };
/** M changed, A added, R renamed, D deleted, U untracked, ! in conflict, I ignored. */
export type Mark = "M" | "A" | "R" | "D" | "U" | "!" | "I";
export type GitMark = { path: string; mark: Mark };

/** A folder's entries shown at first; "N more" asks for the rest. */
export const PAGE = 500;
/** Tabs kept open; the oldest one not shown goes first. */
const TABS = 12;

export type Folder = { state: "loading" | "ready" | "error"; listing: Listing | null; limit: number };
/** A file opened in a tab: `view` undefined while it is read, null when it couldn't be (gone,
 * not a file any more); `diff` read for a file Git has changes in. */
export type Opened = { view: FileView | null | undefined; diff: ParsedDiff | null; read: number };

export type ProjectFiles = {
  path: string;
  expanded: Set<string>;
  folders: Map<string, Folder>;
  /** Null: not a Git work tree, or not read yet. */
  marks: Map<string, Mark> | null;
  /** A folder's strongest mark among what it holds. */
  folderMarks: Map<string, Mark>;
  /** Folders Git ignores or doesn't track as a whole (`node_modules/`, `new/`). */
  wholeFolders: Array<[string, Mark]>;
  tabs: string[];
  /** The preview tab, VS Code's (its name in italics): the one a click opens a file in, taken by
   * the next file a click opens; kept for good at a double click. At most one. */
  preview: string | null;
  /** What the preview tab showed before a click put `by` in it: given back to it when that click
   * was a double click's first (`by` then gets a tab of its own). Until the next tab is opened. */
  replaced: { tab: string; by: string } | null;
  active: string | null;
  opened: Map<string, Opened>;
  /** The tree's row with the keyboard, or the one last clicked. */
  focus: string | null;
  index: { files: string[]; partial: boolean } | "loading" | null;
  /** Files shown as their changes rather than as they are (the status bar's Changes). */
  diffOn: Set<string>;
  /** The Source control tab's history, once it was shown; `loading` while a page is asked for. */
  history: (History & { loading: boolean }) | null;
  commits: Map<string, OpenedCommit>;
};

export type Rect = { x: number; y: number; w: number; h: number };

/** The Source control tab's history (core/explorer.rs: `History`). */
export type Commit = { id: string; parents: string[]; refs: string[]; author: string; time: number; subject: string };
export type Forge = { host: string; commitUrl: string };
export type History = { commits: Commit[]; head: string | null; more: boolean; remote: Forge | null };
export type CommitFile = { path: string; from: string | null; mark: "M" | "A" | "D" | "R" };
/** A commit opened in a tab: its files, the one picked, and that one's change. */
export type OpenedCommit = { files: CommitFile[] | null | undefined; picked: string | null; diff: ParsedDiff | null | undefined };

/** A tab is a file's path, or a commit's id after this. */
export const COMMIT_TAB = "commit:";
/** Commits asked for at a time. */
const LOG_PAGE = 200;

/** A Files window: the project it shows, folded into its sidebar entry or filling the app, where
 * it floats, and its panels. One per project, as many as there are projects. */
export type FilesWin = {
  path: string;
  /** False from its close on: it is drawn until its closing motion is over, then dropped. */
  open: boolean;
  minimized: boolean;
  maximized: boolean;
  rect: Rect | null;
  /** Steps down and to the right of the first place it opens at, so windows don't hide each other. */
  cascade: number;
  tree: number;
  /** The side tab shown beside the file: the tree, the source control, or none (hidden). */
  side: "files" | "scm" | null;
  /** The share of the Source control tab's height Changes takes, History the rest. */
  scmSplit: number;
  /** Its place in the stack: the highest is in front. */
  z: number;
};

/** The Files windows in the order they were opened; the sidebar lists the open ones under Garage. */
export const filesWindows = reactive<FilesWin[]>([]);
let raised = 0;

/** `path`'s window, while it is open. */
export function filesWindowOf(path: string): FilesWin | null {
  return filesWindows.find((win) => win.path === path && win.open) ?? null;
}

/** The window in front: open, not folded, the highest in the stack. */
export function frontFiles(): FilesWin | null {
  let front: FilesWin | null = null;
  for (const win of filesWindows) if (win.open && !win.minimized && (!front || win.z > front.z)) front = win;
  return front;
}

/** Whether `path`'s window is up (open and not folded into the sidebar). */
export function filesShown(path: string): boolean {
  const win = filesWindowOf(path);
  return !!win && !win.minimized;
}

export function raiseFiles(win: FilesWin): void {
  if (frontFiles() !== win) win.z = ++raised;
}

const projects = reactive(new Map<string, ProjectFiles>());

/** What was read of `path`'s files, when the window has shown them. */
export function peekFiles(path: string): ProjectFiles | null {
  return projects.get(path) ?? null;
}

export function projectFiles(path: string): ProjectFiles {
  let files = projects.get(path);
  if (!files) {
    projects.set(path, {
      path,
      expanded: new Set([""]),
      folders: new Map(),
      marks: null,
      folderMarks: new Map(),
      wholeFolders: [],
      tabs: [],
      preview: null,
      replaced: null,
      active: null,
      opened: new Map(),
      focus: null,
      index: null,
      diffOn: new Set(),
      history: null,
      commits: new Map(),
    });
    files = projects.get(path)!;
    void loadFolder(files, "");
    void loadMarks(files);
  }
  return files;
}

/** `path`'s window up and in front: its own when it has one (out of the sidebar when folded),
 * else a new one, a step from the last. */
export function showFiles(path: string): void {
  projectFiles(path);
  const win = filesWindows.find((w) => w.path === path);
  if (win) {
    win.open = true;
    win.minimized = false;
    win.z = ++raised;
    return;
  }
  const cascade = filesWindows.filter((w) => w.open).length % 8;
  filesWindows.push({ path, open: true, minimized: false, maximized: false, rect: null, cascade, tree: 280, side: "files", scmSplit: 0.7, z: ++raised });
}

/** The button and ⇧⌘E: up on `path`, or closed when its window is the one in front. */
export function toggleFiles(path: string): void {
  const win = filesWindowOf(path);
  if (win && frontFiles() === win) closeFiles(path);
  else showFiles(path);
}

export function minimizeFiles(win: FilesWin): void {
  win.minimized = true;
}

/** `path`'s window closed: out of the sidebar at once, dropped once its closing motion is over
 * (at once when it is folded and has none). */
export function closeFiles(path: string): void {
  const win = filesWindowOf(path);
  if (!win) return;
  win.open = false;
  if (win.minimized) dropFiles(path);
  else window.setTimeout(() => dropFiles(path), 400);
}

function dropFiles(path: string): void {
  const at = filesWindows.findIndex((win) => win.path === path && !win.open);
  if (at >= 0) filesWindows.splice(at, 1);
}

/** A project no longer listed takes what was read of it, and its window, along. */
export function forgetFiles(path: string): void {
  projects.delete(path);
  const at = filesWindows.findIndex((win) => win.path === path);
  if (at >= 0) filesWindows.splice(at, 1);
}

export async function loadFolder(files: ProjectFiles, dir: string, limit?: number): Promise<void> {
  const was = files.folders.get(dir);
  const want = limit ?? was?.limit ?? PAGE;
  files.folders.set(dir, { state: was?.listing ? "ready" : "loading", listing: was?.listing ?? null, limit: want });
  const listing = await api.explorerList(files.path, dir, want).catch(() => null);
  files.folders.set(dir, listing ? { state: "ready", listing, limit: want } : { state: "error", listing: null, limit: want });
}

export function toggleFolder(files: ProjectFiles, dir: string, open = !files.expanded.has(dir)): void {
  if (!open) {
    files.expanded.delete(dir);
    return;
  }
  files.expanded.add(dir);
  if (files.folders.get(dir)?.state !== "ready") void loadFolder(files, dir);
}

export function collapseAll(files: ProjectFiles): void {
  files.expanded.clear();
  files.expanded.add("");
}

/** Every folder above `file` opened, so its row shows. */
export function revealInTree(files: ProjectFiles, file: string): void {
  const parts = file.split("/");
  for (let i = 1; i < parts.length; i++) toggleFolder(files, parts.slice(0, i).join("/"), true);
  files.focus = file;
}

const STRENGTH: Record<Mark, number> = { "!": 4, M: 3, R: 3, D: 3, A: 2, U: 2, I: 0 };

export async function loadMarks(files: ProjectFiles): Promise<void> {
  const list = await api.explorerStatus(files.path).catch(() => null);
  if (!list) {
    files.marks = null;
    files.folderMarks = new Map();
    files.wholeFolders = [];
    return;
  }
  const marks = new Map<string, Mark>();
  const folderMarks = new Map<string, Mark>();
  const wholeFolders: Array<[string, Mark]> = [];
  for (const { path, mark } of list) {
    const folder = path.endsWith("/");
    const clean = folder ? path.slice(0, -1) : path;
    if (folder) wholeFolders.push([`${clean}/`, mark]);
    else marks.set(clean, mark);
    if (mark === "I") {
      if (folder) folderMarks.set(clean, "I");
      continue;
    }
    // The folder itself (an untracked one) and every folder above.
    const parts = clean.split("/");
    for (let i = folder ? parts.length : parts.length - 1; i > 0; i--) {
      const above = parts.slice(0, i).join("/");
      const had = folderMarks.get(above);
      if (!had || STRENGTH[mark] > STRENGTH[had]) folderMarks.set(above, mark);
    }
  }
  files.marks = marks;
  files.folderMarks = folderMarks;
  files.wholeFolders = wholeFolders;
}

/** A file's mark: its own, or that of a folder Git takes as a whole. */
export function markOf(files: ProjectFiles, file: string): Mark | null {
  const own = files.marks?.get(file);
  if (own) return own;
  for (const [prefix, mark] of files.wholeFolders) if (file.startsWith(prefix)) return mark;
  return null;
}

/** A folder's mark: the strongest of what it holds, or ignored as a whole (`I`). */
export function folderMarkOf(files: ProjectFiles, dir: string): Mark | null {
  const own = files.folderMarks.get(dir);
  if (own) return own;
  for (const [prefix, mark] of files.wholeFolders) if (`${dir}/`.startsWith(prefix)) return mark;
  return null;
}

/** Opens `file` (or shows its tab) and reads it: a click's open in the preview tab, a double
 * click's (`keep`) in a tab of its own. */
export function openFile(files: ProjectFiles, file: string, keep = false): void {
  placeTab(files, file, keep);
  files.focus = file;
  if (!files.opened.has(file)) void readFile(files, file);
}

/** `tab` shown. One open already is just shown; at a double click, the preview tab showing it is
 * kept for good, unless the double click's first click put it there: then the preview tab gets
 * back what it showed and `tab` opens as a new one would. A click's new one takes the preview
 * tab's place (what was read for the one there dropped, as at its close) or opens as the preview
 * tab; a double click's opens for good. New tabs open at the end. */
function placeTab(files: ProjectFiles, tab: string, keep: boolean): void {
  const replaced = files.replaced;
  files.replaced = null;
  if (files.tabs.includes(tab)) {
    if (keep && files.preview === tab) {
      files.preview = null;
      if (replaced?.by === tab && !files.tabs.includes(replaced.tab)) {
        files.tabs.splice(files.tabs.indexOf(tab), 1, replaced.tab);
        files.tabs.push(tab);
        files.preview = replaced.tab;
        readTab(files, replaced.tab);
        limitTabs(files, [tab, replaced.tab]);
      }
    }
  } else if (!keep && files.preview) {
    const at = files.tabs.indexOf(files.preview);
    files.replaced = { tab: files.preview, by: tab };
    forgetTab(files, files.preview);
    files.tabs.splice(at, 0, tab);
    files.preview = tab;
  } else {
    files.tabs.push(tab);
    if (!keep) files.preview = tab;
    limitTabs(files, [tab]);
  }
  files.active = tab;
}

/** No more than TABS tabs: the oldest go first, never one just opened (`spare`). */
function limitTabs(files: ProjectFiles, spare: string[]): void {
  while (files.tabs.length > TABS) {
    const old = files.tabs.find((tab) => !spare.includes(tab));
    if (!old) return;
    closeTab(files, old);
  }
}

/** A double click on the preview tab: kept for good. */
export function keepTab(files: ProjectFiles, tab: string): void {
  if (files.preview === tab) files.preview = null;
  files.replaced = null;
}

/** A double click whose first click opened a file and so put something else under the pointer (a
 * search hit picked, a link in the preview followed): its second click and the double click land
 * on what came in its place. A pane gives them to these listeners (in the capture phase), which
 * keep them from it and, at the double click, the file in a tab of its own. */
export function handOver(files: () => ProjectFiles) {
  let file: string | null = null;
  return {
    /** The file the first click opened. */
    arm(opened: string): void {
      file = opened;
    },
    down(event: MouseEvent): void {
      if (event.detail < 2) file = null;
      else if (file) event.preventDefault();
    },
    click(event: MouseEvent): void {
      if (file && event.detail >= 2) event.stopPropagation();
    },
    double(event: MouseEvent): void {
      if (!file) return;
      event.stopPropagation();
      openFile(files(), file, true);
      file = null;
    },
  };
}

export function closeTab(files: ProjectFiles, file: string): void {
  const at = files.tabs.indexOf(file);
  if (at < 0) return;
  forgetTab(files, file);
  if (files.active === file) files.active = files.tabs[Math.min(at, files.tabs.length - 1)] ?? null;
}

/** A tab's menu: the other tabs, those left or right of it, or all closed. The shown one stays
 * when it is left open; otherwise the tab the menu was opened on is shown. */
export function closeTabs(files: ProjectFiles, file: string, which: "others" | "left" | "right" | "all"): void {
  const at = files.tabs.indexOf(file);
  if (at < 0) return;
  const drop =
    which === "all" ? files.tabs.slice() : which === "left" ? files.tabs.slice(0, at) : which === "right" ? files.tabs.slice(at + 1) : files.tabs.filter((tab) => tab !== file);
  for (const tab of drop) forgetTab(files, tab);
  if (!files.active || !files.tabs.includes(files.active)) files.active = files.tabs.includes(file) ? file : null;
}

function forgetTab(files: ProjectFiles, file: string): void {
  files.tabs.splice(files.tabs.indexOf(file), 1);
  if (files.preview === file) files.preview = null;
  files.opened.delete(file);
  files.commits.delete(file.slice(COMMIT_TAB.length));
}

/** Reads asked for, counted across projects: a read's answer is shown only while it is the latest
 * one of its file (two can cross: a refresh while a read is out, a tab closed and opened again). */
let reads = 0;

export async function readFile(files: ProjectFiles, file: string): Promise<void> {
  const was = files.opened.get(file);
  const read = ++reads;
  files.opened.set(file, { view: was?.view, diff: was?.diff ?? null, read });
  const [view, diff] = await Promise.all([api.explorerRead(files.path, file).catch(() => null), readDiff(files, file)]);
  if (!files.tabs.includes(file) || files.opened.get(file)?.read !== read) return;
  files.opened.set(file, { view, diff, read });
}

async function readDiff(files: ProjectFiles, file: string): Promise<ParsedDiff | null> {
  const mark = markOf(files, file);
  if (!mark || mark === "I" || mark === "U") return null;
  const diff = await api.explorerDiff(files.path, file).catch(() => null);
  return diff ? parseDiff(diff) : null;
}

/** The gutter's marks of a file: from its diff, or every line for one Git doesn't track yet. */
export function gutterOf(files: ProjectFiles, file: string, lines: number): Map<number, GutterMark> {
  if (markOf(files, file) === "U") return new Map(Array.from({ length: lines }, (_, i) => [i + 1, "add" as const]));
  return files.opened.get(file)?.diff?.gutter ?? new Map();
}

/** Everything shown read again: marks, open folders, open files (a window that comes back, Git's
 * state of the project changed). */
export async function refreshFiles(files: ProjectFiles): Promise<void> {
  await loadMarks(files);
  for (const dir of files.expanded) void loadFolder(files, dir);
  for (const file of files.tabs) if (!file.startsWith(COMMIT_TAB)) void readFile(files, file);
  if (files.index && files.index !== "loading") files.index = null;
  if (files.history) void loadHistory(files);
}

/** The changes Git sees, as the Source control tab lists them (what it ignores left out). */
export function changesOf(files: ProjectFiles): Array<{ path: string; mark: Mark }> {
  return [...(files.marks ?? [])].filter(([, mark]) => mark !== "I").map(([path, mark]) => ({ path, mark })).sort((a, b) => a.path.localeCompare(b.path));
}

/** A file shown as its changes: opened (`keep` as for `openFile`), with Changes pressed. */
export function openDiff(files: ProjectFiles, file: string, keep = false): void {
  files.diffOn.add(file);
  openFile(files, file, keep);
}

/** The history's first page again, or (`more`) the next one under what is shown. */
export async function loadHistory(files: ProjectFiles, more = false): Promise<void> {
  const shown = files.history;
  if (shown?.loading) return;
  const skip = more && shown ? shown.commits.length : 0;
  if (shown) shown.loading = true;
  else files.history = { commits: [], head: null, more: false, remote: null, loading: true };
  const page = await api.explorerLog(files.path, skip, LOG_PAGE).catch(() => null);
  const commits = more && shown ? [...shown.commits, ...(page?.commits ?? [])] : (page?.commits ?? []);
  files.history = { commits, head: page?.head ?? null, more: page?.more ?? false, remote: page?.remote ?? null, loading: false };
}

/** A commit in a tab (the preview tab, or `keep` one of its own, as a file's): its files read, the
 * first one's change shown. */
export function openCommit(files: ProjectFiles, id: string, keep = false): void {
  placeTab(files, COMMIT_TAB + id, keep);
  loadCommit(files, id);
}

/** A tab's file or commit read (a tab given back to the preview tab). */
function readTab(files: ProjectFiles, tab: string): void {
  if (tab.startsWith(COMMIT_TAB)) loadCommit(files, tab.slice(COMMIT_TAB.length));
  else void readFile(files, tab);
}

function loadCommit(files: ProjectFiles, id: string): void {
  if (files.commits.has(id)) return;
  files.commits.set(id, { files: undefined, picked: null, diff: undefined });
  void api
    .explorerCommit(files.path, id)
    .catch(() => null)
    .then((list) => {
      const opened = files.commits.get(id);
      if (!opened) return;
      opened.files = list;
      if (list?.length) void pickCommitFile(files, id, list[0].path);
    });
}

export async function pickCommitFile(files: ProjectFiles, id: string, path: string): Promise<void> {
  const opened = files.commits.get(id);
  const file = opened?.files?.find((f) => f.path === path);
  if (!opened || !file) return;
  opened.picked = path;
  opened.diff = undefined;
  const diff = await api.explorerCommitDiff(files.path, id, path, file.from).catch(() => null);
  if (opened.picked === path) opened.diff = diff ? parseDiff(diff) : null;
}

export async function loadIndex(files: ProjectFiles): Promise<void> {
  if (files.index) return;
  files.index = "loading";
  files.index = (await api.explorerFiles(files.path).catch(() => null)) ?? { files: [], partial: false };
}

/** `file` (from the project's folder, `/` between its parts) as the system writes its whole path. */
export function absolutePath(project: string, file: string): string {
  const root = project.replace(/[\\/]+$/, "");
  return isWindows ? `${root}\\${file.split("/").join("\\")}` : `${root}/${file}`;
}

/** A size as the window writes it: 840 B, 6.2 KB, 4.8 MB. */
export function sizeText(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const number = new Intl.NumberFormat(language.value, { maximumFractionDigits: unit && value < 10 ? 1 : 0 }).format(value);
  return `${number} ${units[unit]}`;
}
