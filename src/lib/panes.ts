import { ref } from "vue";

import { isMac } from "./platform";
import { api } from "./store";
import type { PaneNodeCore, TerminalLayout } from "./types";

/**
 * The terminal panel's panes (window/TerminalPanes): a project's terminals beside and under each
 * other, a tree of splits whose leaves are terminals or empty panes. A project without one shows
 * one terminal at a time (the Düzen menu's Tek). Each project has its own; the core keeps a copy,
 * writes it down at a quit and before an update's restart, and puts it back with the tabs
 * (core/layout.rs).
 *
 * A border between two panes moves only those two (and what lies inside them); it stops where
 * either would get smaller than a pane may be, and pushes nothing further.
 */

/** Between two panes: what a drag takes hold of. */
export const GUTTER = 6;
/** A terminal's cell (12 px Geist Mono at a line height of 1.25, as WebKit draws it), and what a
 * pane takes around its rows: its border, its head and the head's line, the terminal's margins. */
const CELL = { w: 7.2, h: 20 };
const AROUND = { w: 2 + 14, h: 2 + 22 + 1 + 12 };
export type PaneKind = "shell" | "claude";
/** The least a pane may be, in columns and rows: a shell's, and a Claude session's, under which
 * Claude Code's own screen breaks its lines apart. */
export const PANE_LEAST: Record<PaneKind, { cols: number; rows: number }> = { shell: { cols: 40, rows: 5 }, claude: { cols: 60, rows: 8 } };
export function leastPx(kind: PaneKind): { w: number; h: number } {
  return { w: Math.ceil(PANE_LEAST[kind].cols * CELL.w) + AROUND.w, h: PANE_LEAST[kind].rows * CELL.h + AROUND.h };
}
export const PANE_MIN_W = leastPx("shell").w;
export const PANE_MIN_H = leastPx("shell").h;

/** The Claude terminals of the project shown (TerminalPanel keeps it): their panes are larger at
 * least. */
export const claudeTerminals = ref<ReadonlySet<number>>(new Set());

export function kindOf(pane: PaneLeaf): PaneKind {
  return pane.terminal !== null && claudeTerminals.value.has(pane.terminal) ? "claude" : "shell";
}
/** What the open panel takes around its panes: its top line (1), handle (9) and tab row (24), and
 * the panes' margins (6 above, 20 under). */
export const PANES_CHROME = 60;
/** The panel's height with panes, until it is dragged: room for two rows of them. */
export const PANES_HEIGHT = 520;

export type SplitDir = "row" | "col";
export type PaneLeaf = { kind: "pane"; key: string; terminal: number | null };
export type PaneSplit = { kind: "split"; key: string; dir: SplitDir; sizes: number[]; kids: PaneNode[] };
export type PaneNode = PaneLeaf | PaneSplit;
/** A project's panes: `focus` is the pane with the keyboard, `zoom` one that fills the room for now. */
export type PaneLayout = { tree: PaneNode; focus: string; zoom: string | null; rev: number };
/** Where a new terminal goes: beside a pane (`split`), into an empty one (`cell`), or where there
 * is room (`auto`: an empty pane, else beside the focused one, else in its place). */
export type Place = { kind: "split"; key: string; dir: SplitDir; before?: boolean } | { kind: "cell"; key: string } | { kind: "auto" };

let made = 0;
/** A pane's or a split's own name, for as long as the window lives: it keeps its place on the page
 * (and its terminal) whatever moves around it. */
const newKey = (): string => `pane${++made}`;

export const leaf = (terminal: number | null): PaneLeaf => ({ kind: "pane", key: newKey(), terminal });

function split(dir: SplitDir, kids: PaneNode[], sizes?: number[]): PaneSplit {
  return { kind: "split", key: newKey(), dir, kids, sizes: sizes ?? kids.map(() => 1 / kids.length) };
}

function copy(node: PaneNode): PaneNode {
  return node.kind === "pane" ? { ...node } : { ...node, sizes: [...node.sizes], kids: node.kids.map(copy) };
}

export function leaves(node: PaneNode): PaneLeaf[] {
  return node.kind === "pane" ? [node] : node.kids.flatMap(leaves);
}

export function findLeaf(node: PaneNode, key: string): PaneLeaf | undefined {
  return leaves(node).find((pane) => pane.key === key);
}

export function leafOf(node: PaneNode, terminal: number): PaneLeaf | undefined {
  return leaves(node).find((pane) => pane.terminal === terminal);
}

/** The split holding the node `key`, and the node's place in it; none for the root. */
function parentOf(node: PaneNode, key: string): { parent: PaneSplit; at: number } | null {
  if (node.kind === "pane") return null;
  const at = node.kids.findIndex((kid) => kid.key === key);
  if (at >= 0) return { parent: node, at };
  for (const kid of node.kids) {
    const found = parentOf(kid, key);
    if (found) return found;
  }
  return null;
}

/** The tree with the node `key` given way to `by` (none: taken out of its split). */
function replaced(node: PaneNode, key: string, by: PaneNode): PaneNode {
  if (node.key === key) return by;
  if (node.kind === "pane") return node;
  return { ...node, kids: node.kids.map((kid) => replaced(kid, key, by)) };
}

/* ---------- where everything goes ---------- */

export type Rect = { x: number; y: number; w: number; h: number };
export type PlacedPane = Rect & { leaf: PaneLeaf };
/** The border after a split's kid `at`. */
export type Gutter = Rect & { split: string; at: number; dir: SplitDir };
export type Placed = { panes: PlacedPane[]; gutters: Gutter[]; splits: Map<string, Rect> };

/**
 * The lengths a split gives its kids along its way out of `total`: their shares, except that a kid
 * below its least is brought up to it, the room coming from the kids that have more than theirs in
 * proportion to what they have over it. The shares themselves stay as they are: given the room
 * again, the panes are as they were. With no room for every least, the shares alone.
 */
function lengths(node: PaneSplit, total: number): number[] {
  const out = node.sizes.map((share) => share * total);
  const least = node.kids.map((kid) => (node.dir === "row" ? minSize(kid).w : minSize(kid).h));
  if (least.reduce((sum, each) => sum + each, 0) > total) return out;
  for (let round = 0; round < node.kids.length; round++) {
    const short = out.map((each, i) => least[i] - each).filter((need) => need > 0.5);
    if (!short.length) break;
    const need = short.reduce((sum, each) => sum + each, 0);
    const over = out.map((each, i) => Math.max(0, each - least[i]));
    const spare = over.reduce((sum, each) => sum + each, 0);
    out.forEach((each, i) => (out[i] = each < least[i] ? least[i] : each - (need * over[i]) / spare));
  }
  return out;
}

/** Every pane's and border's room in a `w` × `h` room, in whole pixels. */
export function place(root: PaneNode, w: number, h: number): Placed {
  const out: Placed = { panes: [], gutters: [], splits: new Map() };
  const walk = (node: PaneNode, x: number, y: number, width: number, height: number): void => {
    if (node.kind === "pane") {
      out.panes.push({ leaf: node, x, y, w: width, h: height });
      return;
    }
    out.splits.set(node.key, { x, y, w: width, h: height });
    const across = node.dir === "row";
    const total = (across ? width : height) - GUTTER * (node.kids.length - 1);
    const end = across ? x + width : y + height;
    const shares = lengths(node, total);
    let at = across ? x : y;
    node.kids.forEach((kid, i) => {
      const length = i === node.kids.length - 1 ? end - at : Math.round(shares[i]);
      if (across) walk(kid, at, y, length, height);
      else walk(kid, x, at, width, length);
      at += length;
      if (i < node.kids.length - 1) {
        out.gutters.push(across ? { split: node.key, at: i, dir: "row", x: at, y, w: GUTTER, h: height } : { split: node.key, at: i, dir: "col", x, y: at, w: width, h: GUTTER });
        at += GUTTER;
      }
    });
  };
  walk(root, 0, 0, w, h);
  return out;
}

/** The least room a node takes: every pane in it at its least. */
export function minSize(node: PaneNode): { w: number; h: number } {
  if (node.kind === "pane") return leastPx(kindOf(node));
  const kids = node.kids.map(minSize);
  const gaps = GUTTER * (kids.length - 1);
  return node.dir === "row"
    ? { w: kids.reduce((sum, kid) => sum + kid.w, gaps), h: Math.max(...kids.map((kid) => kid.h)) }
    : { w: Math.max(...kids.map((kid) => kid.w)), h: kids.reduce((sum, kid) => sum + kid.h, gaps) };
}

export function fits(node: PaneNode, w: number, h: number): boolean {
  const least = minSize(node);
  return least.w <= w && least.h <= h;
}

/** The most panes a project has, however large the screen: the core keeps no layout of more
 * (core/layout.rs). */
export const MAX_PANES = 16;

/** The tree may take one pane more. With that many, nothing splits any more: a terminal that comes
 * goes into a pane, the one there waiting. */
export function canAddPane(tree: PaneNode): boolean {
  return leaves(tree).length < MAX_PANES;
}

/** A pane of that room can be split that way: both halves at their least, the one there and the
 * one coming (each a shell's or a Claude session's). */
export function canSplit(rect: Rect, dir: SplitDir, kinds: [PaneKind, PaneKind] = ["shell", "shell"]): boolean {
  const [one, other] = kinds.map(leastPx);
  return dir === "row" ? rect.w >= one.w + other.w + GUTTER : rect.h >= one.h + other.h + GUTTER;
}

/** How a tree is shown in a room: as it is; its panes dealt out again in rows when the room is too
 * small for it as it is (fewer columns, say); or, when even that can't fit, `squeezed`: the pane
 * with the keyboard fills the room for now. The tree itself never changes for it: given the room
 * again, it shows as it was. */
export type Shown = { tree: PaneNode; mode: "as-is" | "rearranged" | "squeezed" };

export function shownIn(tree: PaneNode, w: number, h: number): Shown {
  if (w <= 0 || h <= 0 || fits(tree, w, h)) return { tree, mode: "as-is" };
  const panes = leaves(tree);
  const least = panes.reduce((most, pane) => {
    const each = minSize(pane);
    return { w: Math.max(most.w, each.w), h: Math.max(most.h, each.h) };
  }, { w: 0, h: 0 });
  const rows = autoRows(panes.length, w, h, least);
  if (!rows) return { tree, mode: "squeezed" };
  let from = 0;
  const lines: PaneNode[] = rows.map((size, i) => {
    const kids = panes.slice(from, (from += size));
    return kids.length === 1 ? kids[0] : { kind: "split", key: `dealt-row-${i}`, dir: "row", kids, sizes: kids.map(() => 1 / kids.length) };
  });
  return { tree: lines.length === 1 ? lines[0] : { kind: "split", key: "dealt", dir: "col", kids: lines, sizes: lines.map(() => 1 / lines.length) }, mode: "rearranged" };
}

/* ---------- the Düzen menu's layouts ---------- */

export type PresetId = "1" | "2" | "2s" | "3" | "4" | "6" | "8";
export const PRESETS: { id: PresetId; count: number }[] = [
  { id: "1", count: 1 },
  { id: "2", count: 2 },
  { id: "2s", count: 2 },
  { id: "3", count: 3 },
  { id: "4", count: 4 },
  { id: "6", count: 6 },
  { id: "8", count: 8 },
];

/** How many panes each row of `n` gets in a `w` × `h` room, each pane at least `least`: as few rows
 * as fit, never more than √n + 1 columns, the fuller rows first. None: `n` panes don't fit. */
export function autoRows(n: number, w: number, h: number, least = { w: PANE_MIN_W, h: PANE_MIN_H }): number[] | null {
  const most = Math.ceil(Math.sqrt(n)) + 1;
  for (let rows = 1; rows <= n; rows++) {
    const columns = Math.ceil(n / rows);
    if (columns > most) continue;
    const width = (w - (columns - 1) * GUTTER) / columns;
    const height = (h - (rows - 1) * GUTTER) / rows;
    if (width >= least.w && height >= least.h) {
      const each = Math.floor(n / rows);
      const extra = n % rows;
      return Array.from({ length: rows }, (_, i) => each + (i < extra ? 1 : 0));
    }
  }
  return null;
}

/** A Düzen layout for a `w` × `h` room, its panes holding `terminals` in order and empty past
 * them. None for Tek, or one that doesn't fit. */
export function preset(id: PresetId, terminals: number[], w: number, h: number): PaneNode | null {
  const count = PRESETS.find((each) => each.id === id)?.count ?? 1;
  if (count < 2) return null;
  const panes = Array.from({ length: count }, (_, i) => leaf(terminals[i] ?? null));
  if (id === "2s") {
    const tree = split("col", panes);
    return fits(tree, w, h) ? tree : null;
  }
  // Every pane as large as the largest of those it holds needs: a Claude session among them, a
  // Claude session's.
  const least = leastPx(panes.some((pane) => kindOf(pane) === "claude") ? "claude" : "shell");
  const rows = autoRows(count, w, h, least);
  if (!rows) return null;
  let from = 0;
  const lines = rows.map((size) => {
    const kids = panes.slice(from, (from += size));
    return kids.length === 1 ? kids[0] : split("row", kids);
  });
  return lines.length === 1 ? lines[0] : split("col", lines);
}

/** The Düzen layout a tree reads as: by its panes' count, two side by side or one under the other. */
export function presetOf(node: PaneNode | null): PresetId | null {
  if (!node) return "1";
  const count = leaves(node).length;
  if (count === 2) return node.kind === "split" && node.dir === "col" ? "2s" : "2";
  return PRESETS.find((each) => each.count === count && each.id !== "2s")?.id ?? null;
}

/* ---------- changes ---------- */

/** The pane `key` split in two, `pane` coming beside it (`before`: to its left, or above it). In a
 * split of the same way it only shares out its own room: its neighbours keep theirs. */
export function splitPane(root: PaneNode, key: string, dir: SplitDir, pane: PaneLeaf, before = false): PaneNode {
  const tree = copy(root);
  const found = parentOf(tree, key);
  if (found && found.parent.dir === dir) {
    const { parent, at } = found;
    const half = parent.sizes[at] / 2;
    parent.sizes.splice(at, 1, half, half);
    parent.kids.splice(before ? at : at + 1, 0, pane);
    return tree;
  }
  const old = found ? found.parent.kids[found.at] : tree;
  const pair = split(dir, before ? [pane, old] : [old, pane]);
  return found ? replaced(tree, key, pair) : pair;
}

/** The pane `key` taken out: its room goes to the neighbour before it in its split (the one after
 * it when it was the first), and a split left with one kid gives way to it. `neighbour`: the pane
 * that has its room now, for the keyboard. */
export function removePane(root: PaneNode, key: string): { tree: PaneNode | null; neighbour: string | null } {
  const tree = copy(root);
  const found = parentOf(tree, key);
  if (!found) return { tree: tree.key === key ? null : tree, neighbour: null };
  const { parent, at } = found;
  const share = parent.sizes[at];
  parent.kids.splice(at, 1);
  parent.sizes.splice(at, 1);
  const next = at > 0 ? at - 1 : 0;
  parent.sizes[next] += share;
  const heir = parent.kids[next];
  const heirs = leaves(heir);
  const neighbour = (at > 0 ? heirs[heirs.length - 1] : heirs[0])?.key ?? null;
  if (parent.kids.length > 1) return { tree, neighbour };
  return { tree: replaced(tree, parent.key, parent.kids[0]), neighbour };
}

/** Two panes change places, each with its terminal. */
export function swapPanes(root: PaneNode, a: string, b: string): PaneNode {
  const one = findLeaf(root, a);
  const other = findLeaf(root, b);
  if (!one || !other || a === b) return root;
  const swap = (node: PaneNode): PaneNode => {
    if (node.kind === "pane") return node.key === a ? { ...other } : node.key === b ? { ...one } : node;
    return { ...node, sizes: [...node.sizes], kids: node.kids.map(swap) };
  };
  return swap(root);
}

export type Side = "left" | "right" | "top" | "bottom";

/** The pane `key` taken from its place and put at a side of the pane `target`. */
export function movePane(root: PaneNode, key: string, target: string, side: Side): PaneNode {
  const moving = findLeaf(root, key);
  if (!moving || key === target) return root;
  const { tree } = removePane(root, key);
  if (!tree || !findLeaf(tree, target)) return root;
  return splitPane(tree, target, side === "left" || side === "right" ? "row" : "col", { ...moving }, side === "left" || side === "top");
}

/** The pane `key` holding `terminal` (none: empty). */
export function setTerminal(root: PaneNode, key: string, terminal: number | null): PaneNode {
  const pane = findLeaf(root, key);
  return pane ? replaced(root, key, { ...pane, terminal }) : root;
}

/**
 * The border after kid `at` of the split `key` moved by `delta` pixels (right, or down): only the
 * kids on either side of it change, and it stops where one of them would get smaller than its
 * panes may be. None while the split has no room to change in.
 */
export function moveBorder(root: PaneNode, key: string, at: number, delta: number, placed: Placed): PaneNode | null {
  const tree = copy(root);
  const room = placed.splits.get(key);
  const found = findSplit(tree, key);
  if (!room || !found || at < 0 || at >= found.kids.length - 1) return null;
  const across = found.dir === "row";
  const total = (across ? room.w : room.h) - GUTTER * (found.kids.length - 1);
  const least = (node: PaneNode): number => (across ? minSize(node).w : minSize(node).h);
  // From where the panes are drawn (a pane brought up to its least included), which the split's
  // shares take on from now.
  const drawn = lengths(found, total);
  found.sizes = drawn.map((length) => length / total);
  const a = drawn[at];
  const b = drawn[at + 1];
  const lowest = least(found.kids[at]) - a;
  const highest = b - least(found.kids[at + 1]);
  if (lowest > highest) return null;
  const step = Math.min(highest, Math.max(lowest, delta));
  found.sizes[at] = (a + step) / total;
  found.sizes[at + 1] = (b - step) / total;
  return tree;
}

function findSplit(node: PaneNode, key: string): PaneSplit | undefined {
  if (node.kind === "pane") return undefined;
  if (node.key === key) return node;
  for (const kid of node.kids) {
    const found = findSplit(kid, key);
    if (found) return found;
  }
  return undefined;
}

/** The kids on either side of a border share their room equally (a double click on it). */
export function evenBorder(root: PaneNode, key: string, at: number): PaneNode {
  const tree = copy(root);
  const found = findSplit(tree, key);
  if (!found || at < 0 || at >= found.kids.length - 1) return root;
  const both = (found.sizes[at] + found.sizes[at + 1]) / 2;
  found.sizes[at] = both;
  found.sizes[at + 1] = both;
  return tree;
}

/** Every split shares its room equally among its kids. */
export function evenAll(root: PaneNode): PaneNode {
  if (root.kind === "pane") return root;
  return { ...root, sizes: root.kids.map(() => 1 / root.kids.length), kids: root.kids.map(evenAll) };
}

/* ---------- the keys ---------- */

export type PaneAction = "right" | "down" | "zoom" | "even" | "left-pane" | "right-pane" | "up-pane" | "down-pane";

const ARROWS: Record<string, PaneAction> = { ArrowLeft: "left-pane", ArrowRight: "right-pane", ArrowUp: "up-pane", ArrowDown: "down-pane" };

/**
 * The panel's keys for its panes. macOS: ⌘D splits to the right, ⇧⌘D down, ⇧⌘↩ zooms, ⌥⌘0
 * equalizes, ⌥⌘ and an arrow moves to the next pane. Windows and Linux, where Ctrl+letter is the
 * shell's: Ctrl+Shift+D, Ctrl+Shift+E, Ctrl+Shift+Enter, Ctrl+Shift+0, and Alt and an arrow, as
 * Windows Terminal moves between its panes.
 */
export function paneAction(event: KeyboardEvent): PaneAction | null {
  if (isMac) {
    if (!event.metaKey || event.ctrlKey) return null;
    if (event.altKey) return event.shiftKey ? null : event.code === "Digit0" ? "even" : (ARROWS[event.key] ?? null);
    if (event.code === "KeyD") return event.shiftKey ? "down" : "right";
    return event.shiftKey && event.key === "Enter" ? "zoom" : null;
  }
  if (event.altKey && !event.ctrlKey && !event.shiftKey && !event.metaKey) return ARROWS[event.key] ?? null;
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) return null;
  return ({ KeyD: "right", KeyE: "down", Enter: "zoom", NumpadEnter: "zoom", Digit0: "even" } as Record<string, PaneAction>)[event.code] ?? null;
}

/** The keys as menus and tooltips name them, and as a native menu's item takes them. */
export const PANE_KEYS = isMac
  ? { right: "⌘D", down: "⇧⌘D", zoom: "⇧⌘↩", even: "⌥⌘0", close: "⌘W" }
  : { right: "Ctrl+Shift+D", down: "Ctrl+Shift+E", zoom: "Ctrl+Shift+Enter", even: "Ctrl+Shift+0", close: "Ctrl+Shift+W" };
export const PANE_ACCELERATORS = isMac
  ? { right: "Cmd+D", down: "Cmd+Shift+D", zoom: "Cmd+Shift+Enter", close: "Cmd+W" }
  : { right: "Ctrl+Shift+D", down: "Ctrl+Shift+E", zoom: "Ctrl+Shift+Enter", close: "Ctrl+Shift+W" };

/* ---------- the core's copy ---------- */

function fromCoreNode(node: PaneNodeCore): PaneNode {
  return node.kind === "pane" ? leaf(node.terminal) : split(node.dir, node.kids.map(fromCoreNode), [...node.sizes]);
}

function toCoreNode(node: PaneNode): PaneNodeCore {
  return node.kind === "pane" ? { kind: "pane", terminal: node.terminal } : { kind: "split", dir: node.dir, sizes: [...node.sizes], kids: node.kids.map(toCoreNode) };
}

export function fromCore(layout: TerminalLayout): PaneLayout {
  const tree = fromCoreNode(layout.tree);
  const panes = leaves(tree);
  const of = (terminal: number | null) => (terminal === null ? undefined : panes.find((pane) => pane.terminal === terminal));
  return { tree, focus: (of(layout.focus) ?? panes[0]).key, zoom: of(layout.zoom)?.key ?? null, rev: layout.rev };
}

export function toCore(layout: PaneLayout): TerminalLayout {
  const terminal = (key: string | null) => (key === null ? null : (findLeaf(layout.tree, key)?.terminal ?? null));
  return { tree: toCoreNode(layout.tree), focus: terminal(layout.focus), zoom: terminal(layout.zoom), rev: layout.rev };
}

/** Each project's panes, by its path; a project without is shown one terminal at a time. */
export const paneLayouts = ref<Record<string, PaneLayout>>({});

/** The panel's height while it shows panes; one terminal at a time has its own (lib/panel). */
export const panesHeight = ref(PANES_HEIGHT);

/** The latest `rev` taken from the core, per project: a layout it laid is taken once. */
const taken: Record<string, number> = {};

/** A project's panes changed (none: one terminal at a time). `tell`: the core hears it now; a
 * border being dragged tells once it is let go. */
export function setPanes(path: string, layout: PaneLayout | null, tell = true): void {
  const next = { ...paneLayouts.value };
  if (layout) next[path] = layout;
  else delete next[path];
  paneLayouts.value = next;
  if (tell) tellPanes(path);
}

export function tellPanes(path: string): void {
  const layout = paneLayouts.value[path];
  void api.setTerminalLayout(path, layout ? toCore(layout) : null).catch(() => undefined);
}

/** Panes the core laid itself (put back after a quit or an update) are taken over what is here. */
export function takeCoreLayouts(projects: { path: string; layout: TerminalLayout | null }[]): void {
  for (const project of projects) {
    const theirs = project.layout;
    if (!theirs || theirs.rev <= (taken[project.path] ?? 0)) continue;
    taken[project.path] = theirs.rev;
    setPanes(project.path, fromCore(theirs), false);
  }
}
