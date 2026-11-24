// A card's note, written as a page (window/board/CardDialog): CodeMirror 6 over the note's own text.
// The text is the note itself, character for character: Markdown and `[Image #n]` stay as they are
// written, and only how they are drawn changes. Headings, emphasis, code, quotes, lists and links are
// drawn as they read, their marks never shown: nothing moves as the caret goes from line to line. A
// line's block marks (`## `, `- `, `> `, `1. `) are one step for the caret, which stands after them
// (Backspace there turns the block into plain text). A `[Image #n]` the card holds is drawn as its
// picture; a task's `[ ]` as a box that ticks. A `/` where a block may begin opens the block menu (window/board/NoteBlockMenu).
// Nothing is fetched: a link is text that ⌘-click opens in the browser (`open_url`), a web image is
// its Markdown.
//
// Only what a note needs is loaded: the Markdown parser (GFM), not lang-markdown's `markdown()`,
// which would bring HTML, CSS and JavaScript support with it.

import { Compartment, EditorSelection, EditorState, Prec, RangeSet, StateEffect, StateField, Transaction, type Extension, type Line, type Range, type SelectionRange } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, keymap, placeholder as placeholderText, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { LanguageSupport, syntaxTree } from "@codemirror/language";
import { insertNewlineContinueMarkupCommand, markdownLanguage } from "@codemirror/lang-markdown";

import { changeBetween, noteLimit } from "./noteText";

/* ---------- the blocks the menu offers ---------- */

export type NoteBlock = "heading" | "bullets" | "numbers" | "todo" | "code" | "quote";

/** What each block begins with. */
const MARKUP: Record<Exclude<NoteBlock, "code">, string> = { heading: "## ", bullets: "- ", numbers: "1. ", todo: "- [ ] ", quote: "> " };

/** Where the menu stands: the `/` (`from`), what was typed after it, and the block's start on its
 * line (after the indent), all of which the chosen block replaces; `left`/`top`/`bottom` the `/`'s
 * place on the screen. */
export type SlashAt = { from: number; to: number; start: number; query: string; left: number; top: number; bottom: number };

/** A `/` where a block may begin: at a line's start, or after a list's, a quote's or a heading's
 * mark; what is typed after it (a word at most) is the menu's search. */
const SLASH = /^(\s*)(?:(?:[-*+]|\d{1,9}[.)])\s+(?:\[[ xX]\]\s+)?|(?:>\s?)+|#{1,6}\s+)?\/([^\s/]{0,24})$/u;

/** The `/` the caret stands after, where a block may begin; not in code. */
function slashAt(state: EditorState): Omit<SlashAt, "left" | "top" | "bottom"> | null {
  const { main } = state.selection;
  if (!main.empty || state.selection.ranges.length > 1) return null;
  const line = state.doc.lineAt(main.head);
  const before = line.text.slice(0, main.head - line.from);
  const match = SLASH.exec(before);
  if (!match) return null;
  const from = main.head - match[2].length - 1;
  if (inCode(state, from, 1)) return null;
  return { from, to: main.head, start: line.from + match[1].length, query: match[2] };
}

/** Whether `pos` is in code (a code block, or a span of code), where nothing is Markdown. */
function inCode(state: EditorState, pos: number, side: -1 | 1): boolean {
  for (let node: { name: string; parent: unknown } | null = syntaxTree(state).resolveInner(pos, side); node; node = node.parent as typeof node) {
    if (node.name === "FencedCode" || node.name === "CodeBlock" || node.name === "InlineCode") return true;
  }
  return false;
}

/** A list item or a quote with nothing written in it yet, at the start of its line. */
const EMPTY_BLOCK = /^(?:(?:[-*+]|\d{1,9}[.)])(?:\s+\[[ xX]\])?\s*|>\s?)$/;
/** A line that goes on with a list or a quote. */
const GOES_ON = /^\s*(?:(?:[-*+]|\d{1,9}[.)])(?:\s|$)|>)/;

/**
 * Return on the last item of a list, or the last line of a quote, while nothing is written in it:
 * the list or the quote ends there, as in a block editor, a blank line after it so that what is
 * written next is a paragraph of its own (a line right under one would still belong to it). An
 * indented one, or one with more of its block below, is lang-markdown's to handle.
 */
function endBlock(view: EditorView): boolean {
  const { state } = view;
  const range = state.selection.main;
  if (state.selection.ranges.length > 1 || !range.empty) return false;
  const line = state.doc.lineAt(range.from);
  if (range.from !== line.to || !EMPTY_BLOCK.test(line.text) || inCode(state, range.from, -1)) return false;
  if (line.number < state.doc.lines && GOES_ON.test(state.doc.line(line.number + 1).text)) return false;
  const above = line.number > 1 ? state.doc.line(line.number - 1).text : "";
  const insert = /\S/.test(above) ? state.lineBreak : "";
  view.dispatch({ changes: { from: line.from, to: line.to, insert }, selection: EditorSelection.cursor(line.from + insert.length), userEvent: "delete", scrollIntoView: true });
  return true;
}

/* ---------- what the editor tells, and is told ---------- */

export type NoteEditorOptions = {
  /** The note as it starts. */
  doc: string;
  placeholder: string;
  /** The field's name, for assistive technology. */
  label: string;
  /** An image's name ("Image 2"), and its remove button's. */
  imageLabel: (n: number) => string;
  removeLabel: (n: number) => string;
  /** The note changed (typed, pasted, a block or a box). */
  onChange: (text: string) => void;
  /** ⌘↵. */
  onSubmit: () => void;
  /** A paste: takes the images it carries (and calls `preventDefault`), or leaves it as text. */
  onPaste: (event: ClipboardEvent) => void;
  /** An image's remove button. */
  onRemoveImage: (n: number) => void;
  /** A link ⌘-clicked. */
  onOpenLink: (url: string) => void;
  /** The block menu opened, moved or closed (`null`). */
  onSlash: (at: SlashAt | null) => void;
  /** Keys while the menu is open: ↑/↓ move its choice, Return or Tab takes it. `false` when the menu
   * has nothing to offer (what was typed matches no block): the key does what it always does. */
  onSlashMove: (by: 1 | -1) => boolean;
  onSlashPick: () => boolean;
};

/** A note's editor, as the dialog holds it. */
export type NoteEditor = {
  view: EditorView;
  /** The note at the caret, as `useNoteImages` puts a pasted image there (a text field's members). */
  field: { readonly value: string; readonly selectionStart: number; readonly selectionEnd: number; setRangeText(text: string, start: number, end: number, mode: "end"): void };
  /** Puts the note's text right when something else changed it (an image taken out): one change,
   * the caret kept, left out of the undo history. */
  setText(text: string): void;
  /** The pictures of the note's images, by number (`null` while one is read). */
  setImages(images: ReadonlyMap<number, string | null>): void;
  /** What an empty note says (it names the agent the card goes to, which the project decides). */
  setPlaceholder(text: string): void;
  /** Turns the `/` and what was typed after it into `block`. */
  applyBlock(block: NoteBlock): void;
  /** The menu's place again (the page scrolled). */
  placeSlash(): void;
  closeSlash(): void;
  focus(): void;
  destroy(): void;
};

/* ---------- images ---------- */

const setImages = StateEffect.define<ReadonlyMap<number, string | null>>();

/** The pictures the note's `[Image #n]` stand for: those the card holds and those pasted. */
const imagesField = StateField.define<ReadonlyMap<number, string | null>>({
  create: () => new Map(),
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setImages)) value = effect.value;
    return value;
  },
});

const IMAGE_TOKEN = /\[Image #(\d+)\]/g;
/** One `[Image #n]` and nothing else. */
const IMAGE_ALONE = /^\[Image #\d+\]$/;

const CLOSE_SVG =
  '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/></svg>';
const CHECK_SVG =
  '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>';

/** `[Image #n]`, drawn as its picture: large on a line of its own, as a chip within a sentence. */
class ImageWidget extends WidgetType {
  constructor(
    readonly n: number,
    readonly src: string | null,
    readonly alone: boolean,
    readonly options: NoteEditorOptions,
  ) {
    super();
  }

  eq(other: ImageWidget): boolean {
    return other.n === this.n && other.src === this.src && other.alone === this.alone;
  }

  toDOM(): HTMLElement {
    const box = document.createElement("span");
    box.className = this.alone ? "cm-note-image" : "cm-note-image chip";
    box.setAttribute("role", "img");
    box.setAttribute("aria-label", this.options.imageLabel(this.n));
    if (this.src) {
      const picture = document.createElement("img");
      picture.src = this.src;
      picture.alt = "";
      picture.draggable = false;
      box.append(picture);
    } else {
      const wait = document.createElement("span");
      wait.className = "cm-note-image-wait";
      box.append(wait);
    }
    const caption = document.createElement("span");
    caption.className = "cm-note-image-caption";
    caption.textContent = this.options.imageLabel(this.n);
    box.append(caption);
    if (this.alone) {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "cm-note-image-remove";
      remove.innerHTML = CLOSE_SVG;
      remove.setAttribute("aria-label", this.options.removeLabel(this.n));
      remove.title = this.options.removeLabel(this.n);
      remove.addEventListener("mousedown", (event) => {
        event.preventDefault();
        this.options.onRemoveImage(this.n);
      });
      box.append(remove);
    }
    return box;
  }

  get estimatedHeight(): number {
    return this.alone ? 190 : -1;
  }
}

/** How far in two columns of an item's indent put it (a level under the item above), and how wide
 * the column of its bullet, number or box is: the text starts after both, on every line of it. */
const INDENT_EM = 0.75;
const MARK_EM = 1.7;

/** A list item's marks, its indent with them, drawn as its bullet, its number or its box (a task's
 * `[ ]`, which a click ticks): one step for the caret, the same width on each line of the item. */
class ItemWidget extends WidgetType {
  constructor(
    readonly kind: "bullet" | "number" | "task",
    readonly label: string,
    readonly checked: boolean,
    readonly indent: number,
  ) {
    super();
  }

  eq(other: ItemWidget): boolean {
    return other.kind === this.kind && other.label === this.label && other.checked === this.checked && other.indent === this.indent;
  }

  toDOM(view: EditorView): HTMLElement {
    const item = document.createElement("span");
    item.className = `cm-note-item ${this.kind}`;
    if (this.indent) item.style.paddingLeft = `${this.indent * INDENT_EM}em`;
    if (this.kind !== "task") {
      item.textContent = this.kind === "bullet" ? "•" : this.label;
      return item;
    }
    const box = document.createElement("span");
    box.className = this.checked ? "cm-note-check on" : "cm-note-check";
    box.setAttribute("role", "checkbox");
    box.setAttribute("aria-checked", String(this.checked));
    if (this.checked) box.innerHTML = CHECK_SVG;
    box.addEventListener("mousedown", (event) => {
      event.preventDefault();
      const at = view.posAtDOM(item);
      const marker = /\[[ xX]\]/.exec(view.state.sliceDoc(at, view.state.doc.lineAt(at).to));
      if (!marker) return;
      const tick = at + marker.index + 1;
      view.dispatch({ changes: { from: tick, to: tick + 1, insert: this.checked ? " " : "x" }, userEvent: "input" });
    });
    item.append(box);
    return item;
  }
}

const HIDDEN = Decoration.replace({});
const MARK = Decoration.mark({ class: "cm-note-mark" });

/* ---------- drawing the Markdown ---------- */

/* ---------- a line's block marks ---------- */

/** The marks a line's block begins with: a quote's `>`, a list's `-` or `1.` (and a task's `[ ]`),
 * a heading's `##`. `end`: where the line's text begins, after them and their spaces; `start`: where
 * the last of them begins, its indent included (what Backspace takes); `heading`: that last one is a
 * heading's. */
type Prefix = { start: number; end: number; heading: boolean };

const BLOCK_MARKS = new Set(["QuoteMark", "ListMark", "TaskMarker", "HeaderMark"]);

const blank = (char: string): boolean => char === " " || char === "\t";

/** `line`'s block marks; null when it has none (a paragraph, a line of code). A heading's or a
 * list's mark is one once a space follows it: a `#` or a `-` alone is still being typed. */
function blockPrefix(state: EditorState, line: Line): Prefix | null {
  const tree = syntaxTree(state);
  let found: Prefix | null = null;
  let last = "";
  for (let pos = line.from; ; ) {
    let at = pos;
    while (at < line.to && blank(line.text.charAt(at - line.from))) at++;
    if (at >= line.to) break;
    const node = tree.resolveInner(at, 1);
    if (node.from !== at || !BLOCK_MARKS.has(node.name)) break;
    if (node.name === "HeaderMark" && !node.parent?.name.startsWith("ATX")) break;
    if ((node.name === "HeaderMark" || node.name === "ListMark") && !blank(line.text.charAt(node.to - line.from))) break;
    let end = node.to;
    while (end < line.to && blank(line.text.charAt(end - line.from))) end++;
    // A task's box goes with its list's mark: Backspace takes both, as one block.
    const start: number = node.name === "TaskMarker" && last === "ListMark" && found ? found.start : pos;
    found = { start, end, heading: node.name === "HeaderMark" };
    last = node.name;
    pos = end;
  }
  return found;
}

/** The caret never stands among a line's block marks, where what is typed would undo the block: a
 * click there, ↑ or ↓ or Home leave it where the line's text begins. */
const caretPastMarks = EditorState.transactionFilter.of((tr) => {
  if (!tr.selection) return tr;
  const { state } = tr;
  let moved = false;
  const ranges = state.selection.ranges.map((range) => {
    if (!range.empty) return range;
    const line = state.doc.lineAt(range.head);
    const prefix = blockPrefix(state, line);
    if (!prefix || range.head >= prefix.end) return range;
    moved = true;
    return EditorSelection.cursor(prefix.end);
  });
  return moved ? [tr, { selection: EditorSelection.create(ranges, state.selection.mainIndex), sequential: true }] : tr;
});

/** The block whose text the one caret begins, with its line; null elsewhere. */
function atBlockStart(state: EditorState): { line: Line; prefix: Prefix } | null {
  const range = state.selection.main;
  if (state.selection.ranges.length > 1 || !range.empty) return null;
  const line = state.doc.lineAt(range.head);
  const prefix = blockPrefix(state, line);
  return prefix && range.head === prefix.end ? { line, prefix } : null;
}

/** ← at the start of a block's text goes on to the end of the line before, past the marks. */
function leftPastMarks(view: EditorView): boolean {
  const at = atBlockStart(view.state);
  if (!at || at.line.number === 1) return false;
  view.dispatch({ selection: EditorSelection.cursor(at.line.from - 1), scrollIntoView: true, userEvent: "select" });
  return true;
}

/** Backspace at the start of a block's text turns it into plain text: its last mark goes (a
 * heading's `##`, a list item's mark and box, a quote's `>`), the text stays. */
function dropMarks(view: EditorView): boolean {
  const at = atBlockStart(view.state);
  if (!at) return false;
  view.dispatch({ changes: { from: at.prefix.start, to: at.prefix.end }, selection: EditorSelection.cursor(at.prefix.start), userEvent: "delete.backward", scrollIntoView: true });
  return true;
}

/** Delete at a line's end, when the next line is a block: that block's text joins this line, its
 * marks gone (they would otherwise come up as text in the middle of the line). */
function joinPastMarks(view: EditorView): boolean {
  const { state } = view;
  const range = state.selection.main;
  if (state.selection.ranges.length > 1 || !range.empty) return false;
  const line = state.doc.lineAt(range.head);
  if (range.head !== line.to || line.number === state.doc.lines) return false;
  const prefix = blockPrefix(state, state.doc.line(line.number + 1));
  if (!prefix) return false;
  view.dispatch({ changes: { from: line.to, to: prefix.end }, userEvent: "delete.forward", scrollIntoView: true });
  return true;
}

/** Return at the start of a heading's text: an empty line goes in above it, the heading stays. */
function lineAboveHeading(view: EditorView): boolean {
  const at = atBlockStart(view.state);
  if (!at?.prefix.heading || at.prefix.end === at.line.to) return false;
  const { state } = view;
  view.dispatch({ changes: { from: at.line.from, insert: state.lineBreak }, selection: EditorSelection.cursor(at.prefix.end + state.lineBreak.length), userEvent: "input", scrollIntoView: true });
  return true;
}

/* ---------- drawing the Markdown ---------- */

type Drawn = { decorations: DecorationSet; atomic: RangeSet<Decoration> };

/** Stands for a stretch the caret steps over whole. */
const STEP = Decoration.mark({});

function draw(view: EditorView, options: NoteEditorOptions): Drawn {
  const { state } = view;
  const { doc } = state;
  const marks: Range<Decoration>[] = [];
  const atomic: Range<Decoration>[] = [];
  const lineClass = (pos: number, cls: string): void => {
    marks.push(Decoration.line({ class: cls }).range(doc.lineAt(pos).from));
  };
  const linesOf = (from: number, to: number, cls: string): void => {
    for (let n = doc.lineAt(from).number, last = doc.lineAt(to).number; n <= last; n++) marks.push(Decoration.line({ class: cls }).range(doc.line(n).from));
  };
  /** A mark, never shown (with the space after it, when `space`), one step for the caret. */
  const syntax = (from: number, to: number, space = false): void => {
    if (from >= to) return;
    const end = space && blank(doc.sliceString(to, to + 1)) ? to + 1 : to;
    const hidden = HIDDEN.range(from, end);
    marks.push(hidden);
    atomic.push(hidden);
  };
  /** Whether a space follows a heading's or a list's mark: then it is one. */
  const opened = (to: number): boolean => blank(doc.sliceString(to, to + 1));

  const tree = syntaxTree(state);
  for (const { from, to } of view.visibleRanges) {
    tree.iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        // A heading once its `#` has its space; a setext one (a line under it) is drawn as written, so
        // a `-` typed under a line never makes a heading of it for a moment.
        const heading = /^ATXHeading(\d)$/.exec(name);
        if (heading) {
          const mark = node.node.firstChild;
          if (mark?.name === "HeaderMark" && opened(mark.to)) lineClass(node.from, `cm-note-h cm-note-h${Math.min(3, Number(heading[1]))}`);
          return;
        }
        switch (name) {
          case "HeaderMark": {
            // An ATX mark (`## `, and a closing `##`), hidden once it is one; a setext underline dim.
            const parent = node.node.parent;
            if (!parent?.name.startsWith("ATX")) marks.push(MARK.range(node.from, node.to));
            else if (node.from !== parent.from || opened(node.to)) syntax(node.from, node.to, true);
            return;
          }
          case "StrongEmphasis":
            marks.push(Decoration.mark({ class: "cm-note-strong" }).range(node.from, node.to));
            return;
          case "Emphasis":
            marks.push(Decoration.mark({ class: "cm-note-em" }).range(node.from, node.to));
            return;
          case "Strikethrough":
            marks.push(Decoration.mark({ class: "cm-note-strike" }).range(node.from, node.to));
            return;
          case "EmphasisMark":
          case "StrikethroughMark":
            syntax(node.from, node.to);
            return;
          case "InlineCode":
            marks.push(Decoration.mark({ class: "cm-note-code" }).range(node.from, node.to));
            return;
          case "CodeMark":
            if (node.node.parent?.name === "InlineCode") syntax(node.from, node.to);
            else marks.push(MARK.range(node.from, node.to));
            return;
          case "FencedCode":
          case "CodeBlock":
            linesOf(node.from, node.to, "cm-note-pre");
            lineClass(node.from, "cm-note-pre-first");
            lineClass(node.to, "cm-note-pre-last");
            // Its fence marks and language are dim; nothing inside is Markdown.
            for (let child = node.node.firstChild; child; child = child.nextSibling) if (child.name !== "CodeText") marks.push(MARK.range(child.from, child.to));
            return false;
          case "Blockquote":
            linesOf(node.from, node.to, "cm-note-quote");
            return;
          case "QuoteMark":
            syntax(node.from, node.to, true);
            return;
          case "ListMark": {
            // Its mark (and a task's box) with the spaces after them, and its indent when nothing but
            // the indent is before it: one widget; the line hangs its wrapped lines under the text.
            if (!opened(node.to)) return;
            const line = doc.lineAt(node.from);
            const before = doc.sliceString(line.from, node.from);
            const first = /^[ \t]*$/.test(before);
            const indent = first ? before.replace(/\t/g, "    ").length : 0;
            const taskNode = node.node.nextSibling?.name === "Task" ? node.node.nextSibling.firstChild : null;
            const marker = taskNode?.name === "TaskMarker" ? taskNode : null;
            let end = marker ? marker.to : node.to;
            while (end < line.to && blank(doc.sliceString(end, end + 1))) end++;
            const checked = !!marker && /x/i.test(doc.sliceString(marker.from, marker.to));
            const kind = marker ? "task" : node.node.parent?.parent?.name === "OrderedList" ? "number" : "bullet";
            const label = doc.sliceString(node.from, node.to).replace(")", ".");
            const item = Decoration.replace({ widget: new ItemWidget(kind, label, checked, indent) }).range(first ? line.from : node.from, end);
            marks.push(item);
            atomic.push(item);
            if (first) {
              const hang = indent * INDENT_EM + MARK_EM;
              marks.push(Decoration.line({ attributes: { style: `padding-left: ${hang}em; text-indent: -${hang}em` } }).range(line.from));
            }
            const task = marker?.parent;
            if (checked && task && task.to > end) marks.push(Decoration.mark({ class: "cm-note-done" }).range(end, task.to));
            return;
          }
          case "Link": {
            // `[Image #n]` reads as a link with no address: an image's, drawn below.
            if (IMAGE_ALONE.test(doc.sliceString(node.from, node.to))) return false;
            const url = node.node.getChild("URL");
            const href = url ? doc.sliceString(url.from, url.to) : null;
            const linkMarks = node.node.getChildren("LinkMark");
            const label = linkMarks.length >= 2 ? { from: linkMarks[0].to, to: linkMarks[1].from } : null;
            if (!label || label.to <= label.from) return false;
            marks.push(Decoration.mark({ class: "cm-note-link", attributes: href ? { "data-url": href, title: href } : {} }).range(label.from, label.to));
            syntax(node.from, label.from);
            syntax(label.to, node.to);
            return false;
          }
          case "Image":
            // Never loaded: its Markdown, dim.
            marks.push(MARK.range(node.from, node.to));
            return false;
          case "URL":
            marks.push(Decoration.mark({ class: "cm-note-link", attributes: { "data-url": doc.sliceString(node.from, node.to) } }).range(node.from, node.to));
            return;
          case "HorizontalRule":
            lineClass(node.from, "cm-note-rule");
            marks.push(MARK.range(node.from, node.to));
            return;
        }
      },
    });

    // The images: `[Image #n]` the note holds a picture for.
    const images = state.field(imagesField);
    if (images.size) {
      const text = doc.sliceString(from, to);
      for (const match of text.matchAll(IMAGE_TOKEN)) {
        const n = Number(match[1]);
        if (!images.has(n)) continue;
        const start = from + (match.index ?? 0);
        const end = start + match[0].length;
        const line = doc.lineAt(start);
        const alone = line.text.trim() === match[0];
        const image = Decoration.replace({ widget: new ImageWidget(n, images.get(n) ?? null, alone, options) }).range(start, end);
        marks.push(image);
        atomic.push(image);
      }
    }

    // A line's block marks, its indent and spaces with them: one step for the caret.
    for (let n = doc.lineAt(from).number, last = doc.lineAt(to).number; n <= last; n++) {
      const line = doc.line(n);
      const prefix = blockPrefix(state, line);
      if (prefix && prefix.end > line.from) atomic.push(STEP.range(line.from, prefix.end));
    }
  }

  return { decorations: Decoration.set(marks, true), atomic: Decoration.set(atomic, true) };
}

function livePreview(options: NoteEditorOptions): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      drawn: Drawn;

      constructor(view: EditorView) {
        this.drawn = draw(view, options);
      }

      // The caret moving redraws nothing: what is drawn doesn't depend on where it is.
      update(update: ViewUpdate): void {
        const images = update.transactions.some((tr) => tr.effects.some((effect) => effect.is(setImages)));
        if (update.docChanged || update.viewportChanged || images || syntaxTree(update.startState) !== syntaxTree(update.state)) this.drawn = draw(update.view, options);
      }
    },
    { decorations: (value) => value.drawn.decorations },
  );
  return [plugin, EditorView.atomicRanges.of((view) => view.plugin(plugin)?.drawn.atomic ?? Decoration.none)];
}

/* ---------- the block menu ---------- */

/** The block menu's watch: opens the menu when a `/` is typed where a block may begin, follows what
 * is typed after it, and closes it once the caret leaves it, the line no longer reads so, or Esc. */
function slashMenu(options: NoteEditorOptions) {
  class SlashWatch {
    open: Omit<SlashAt, "left" | "top" | "bottom"> | null = null;

    constructor(readonly view: EditorView) {}

    update(update: ViewUpdate): void {
      if (!update.docChanged && !update.selectionSet) return;
      const typed = update.transactions.some((tr) => tr.isUserEvent("input.type") && insertedSlash(tr));
      const found = slashAt(update.state);
      if (found && (this.open ? found.from === this.open.from : typed && found.query === "")) {
        this.open = found;
        this.place();
      } else if (this.open) {
        this.close();
      }
    }

    /** Measures where the `/` stands (never during an update) and tells the dialog. */
    place(): void {
      const open = this.open;
      if (!open) return;
      this.view.requestMeasure({
        read: (view) => view.coordsAtPos(open.from),
        write: (coords) => {
          if (this.open !== open) return;
          if (coords) options.onSlash({ ...open, left: coords.left, top: coords.top, bottom: coords.bottom });
          else this.close();
        },
      });
    }

    close(): void {
      this.open = null;
      options.onSlash(null);
    }

    destroy(): void {
      if (this.open) options.onSlash(null);
    }
  }

  const plugin = ViewPlugin.fromClass(SlashWatch);
  const opened = (view: EditorView): boolean => !!view.plugin(plugin)?.open;
  const keys = Prec.highest(
    keymap.of([
      { key: "ArrowDown", run: (view) => opened(view) && options.onSlashMove(1) },
      { key: "ArrowUp", run: (view) => opened(view) && options.onSlashMove(-1) },
      { key: "Enter", run: (view) => opened(view) && options.onSlashPick() },
      { key: "Tab", run: (view) => opened(view) && options.onSlashPick() },
      {
        key: "Escape",
        run: (view) => {
          if (!opened(view)) return false;
          view.plugin(plugin)?.close();
          return true;
        },
      },
    ]),
  );
  return { extension: [plugin, keys] as Extension, plugin };
}

const continueMarkup = insertNewlineContinueMarkupCommand({ nonTightLists: false });

function insertedSlash(tr: Transaction): boolean {
  let slash = false;
  tr.changes.iterChanges((_fromA, _toA, _fromB, _toB, inserted) => {
    if (inserted.toString() === "/") slash = true;
  });
  return slash;
}

/** The block's text in place of the `/` (and the list or quote mark before it). */
function blockChange(state: EditorState, at: { start: number; to: number }, block: NoteBlock): { changes: { from: number; to: number; insert: string }; selection: SelectionRange } {
  if (block === "code") {
    const line = state.doc.lineAt(at.start);
    const indent = line.text.slice(0, at.start - line.from);
    const insert = "```\n" + indent + "\n" + indent + "```";
    return { changes: { from: at.start, to: at.to, insert }, selection: EditorSelection.cursor(at.start + 4 + indent.length) };
  }
  const insert = MARKUP[block];
  return { changes: { from: at.start, to: at.to, insert }, selection: EditorSelection.cursor(at.start + insert.length) };
}

/* ---------- the look ---------- */

const theme = EditorView.theme({
  "&": { color: "var(--text)", fontSize: "14px", backgroundColor: "transparent" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font)", lineHeight: "1.62", overflow: "visible" },
  ".cm-content": { padding: "0 0 4px", minHeight: "132px", caretColor: "var(--text-strong)", whiteSpace: "pre-wrap", overflowWrap: "anywhere" },
  ".cm-line": { padding: "0" },
  ".cm-placeholder": { color: "var(--text-faint)" },
  ".cm-note-mark": { color: "var(--text-faint)", fontWeight: "400", fontStyle: "normal", textDecoration: "none" },
  ".cm-note-h": { fontWeight: "600", color: "var(--text-strong)", letterSpacing: "-0.005em" },
  ".cm-note-h1": { fontSize: "19px", lineHeight: "1.45", paddingTop: "10px" },
  ".cm-note-h2": { fontSize: "16.5px", lineHeight: "1.5", paddingTop: "8px" },
  ".cm-note-h3": { fontSize: "15px", lineHeight: "1.55", paddingTop: "6px" },
  ".cm-note-strong": { fontWeight: "600", color: "var(--text-strong)" },
  ".cm-note-em": { fontStyle: "italic" },
  ".cm-note-strike": { textDecoration: "line-through", color: "var(--text-subtle)" },
  ".cm-note-code": { fontFamily: "var(--font-mono)", fontSize: "12.5px", background: "var(--bg-tag)", borderRadius: "4px", padding: "1px 4px" },
  ".cm-note-pre": { fontFamily: "var(--font-mono)", fontSize: "12.5px", lineHeight: "1.6", background: "var(--bg-well)", padding: "0 12px" },
  ".cm-note-pre-first": { borderTopLeftRadius: "6px", borderTopRightRadius: "6px", paddingTop: "4px" },
  ".cm-note-pre-last": { borderBottomLeftRadius: "6px", borderBottomRightRadius: "6px", paddingBottom: "4px" },
  ".cm-note-quote": { borderLeft: "2px solid var(--line-bold)", paddingLeft: "12px", color: "var(--text-muted)" },
  ".cm-note-item": { display: "inline-block", width: `${MARK_EM}em`, boxSizing: "content-box", textIndent: "0", whiteSpace: "nowrap", color: "var(--text-subtle)", fontVariantNumeric: "tabular-nums" },
  ".cm-note-item.bullet": { textAlign: "center" },
  ".cm-note-done": { color: "var(--text-subtle)", textDecoration: "line-through" },
  ".cm-note-link": { color: "var(--text-link)", textDecoration: "underline", textDecorationColor: "color-mix(in srgb, var(--text-link) 40%, transparent)", textUnderlineOffset: "2px" },
  ".cm-note-rule": { color: "var(--text-faint)" },
  ".cm-note-check": {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "15px",
    height: "15px",
    margin: "0 0 0 1px",
    verticalAlign: "-2px",
    border: "1.5px solid var(--line-dotted)",
    borderRadius: "4px",
    cursor: "pointer",
    boxSizing: "border-box",
  },
  ".cm-note-check.on": { background: "var(--btn-primary-line)", borderColor: "var(--btn-primary-line)", color: "var(--btn-primary-text)" },
  ".cm-note-image": {
    position: "relative",
    display: "inline-flex",
    flexDirection: "column",
    maxWidth: "min(100%, 360px)",
    margin: "6px 0",
    border: "1px solid var(--line-strong)",
    borderRadius: "8px",
    overflow: "hidden",
    verticalAlign: "top",
    background: "var(--bg-panel)",
  },
  ".cm-note-image img": { display: "block", maxWidth: "100%", maxHeight: "220px", objectFit: "contain", background: "var(--bg-input)" },
  ".cm-note-image-wait": { display: "block", width: "240px", height: "120px", background: "var(--bg-input)" },
  ".cm-note-image-caption": { height: "26px", display: "flex", alignItems: "center", padding: "0 9px", fontSize: "11.5px", color: "var(--text-subtle)", borderTop: "1px solid var(--line)" },
  ".cm-note-image-remove": {
    position: "absolute",
    top: "6px",
    right: "6px",
    width: "22px",
    height: "22px",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    padding: "0",
    border: "1px solid var(--line-strong)",
    borderRadius: "6px",
    background: "var(--bg-dialog)",
    color: "var(--text-soft)",
    opacity: "0",
    transition: "opacity 0.12s",
  },
  ".cm-note-image:hover .cm-note-image-remove, .cm-note-image-remove:focus-visible": { opacity: "1" },
  ".cm-note-image.chip": { flexDirection: "row", alignItems: "center", gap: "6px", maxWidth: "none", margin: "0 1px", padding: "0 7px 0 3px", height: "20px", borderRadius: "5px", verticalAlign: "-4px" },
  ".cm-note-image.chip img, .cm-note-image.chip .cm-note-image-wait": { width: "14px", height: "14px", borderRadius: "3px", objectFit: "cover" },
  ".cm-note-image.chip .cm-note-image-caption": { height: "auto", padding: "0", border: "0", fontSize: "12px" },
});

/* ---------- the editor ---------- */

/** The note's editor in `parent`. */
export function createNoteEditor(parent: HTMLElement, options: NoteEditorOptions): NoteEditor {
  const slash = slashMenu(options);
  const hint = new Compartment();
  let shownHint = options.placeholder;
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: options.doc,
      extensions: [
        history(),
        Prec.highest(
          keymap.of([
            {
              key: "Mod-Enter",
              run: () => {
                options.onSubmit();
                return true;
              },
            },
          ]),
        ),
        slash.extension,
        // Return goes on with a list or a quote and ends one on its empty last line (endBlock), an
        // empty item within a list ending there too, never making the list a loose one; at a
        // heading's start it opens a line above. Backspace at a block's start makes it plain text,
        // ← there goes to the line before, Delete at a line's end joins the next block's text.
        Prec.high(
          keymap.of([
            { key: "Enter", run: (view) => lineAboveHeading(view) || endBlock(view) || continueMarkup(view) },
            { key: "Backspace", run: dropMarks },
            { key: "Delete", run: joinPastMarks },
            { key: "ArrowLeft", run: leftPastMarks },
          ]),
        ),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        new LanguageSupport(markdownLanguage),
        imagesField,
        livePreview(options),
        caretPastMarks,
        noteLimit(),
        EditorView.lineWrapping,
        hint.of(placeholderText(options.placeholder)),
        EditorView.contentAttributes.of({ "aria-label": options.label, "aria-multiline": "true", spellcheck: "true", class: "selectable" }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) options.onChange(update.state.doc.toString());
        }),
        EditorView.domEventHandlers({
          paste: (event) => {
            options.onPaste(event);
            return event.defaultPrevented;
          },
          mousedown: (event) => {
            if (!(event.metaKey || event.ctrlKey)) return false;
            const link = (event.target as HTMLElement | null)?.closest<HTMLElement>("[data-url]");
            const url = link?.dataset.url;
            if (!url) return false;
            event.preventDefault();
            options.onOpenLink(url);
            return true;
          },
        }),
        theme,
      ],
    }),
  });

  return {
    view,
    field: {
      get value() {
        return view.state.doc.toString();
      },
      get selectionStart() {
        return view.state.selection.main.from;
      },
      get selectionEnd() {
        return view.state.selection.main.to;
      },
      setRangeText(text: string, start: number, end: number) {
        // An image pasted on a line of its own stays a block of its own: the caret goes on below
        // it, so what is typed next doesn't run into it (saving trims a last line left empty).
        const line = view.state.doc.lineAt(start);
        const alone = IMAGE_ALONE.test(text) && start === line.from && end === line.to;
        const insert = alone ? `${text}\n` : text;
        view.dispatch({ changes: { from: start, to: end, insert }, selection: EditorSelection.cursor(start + insert.length), userEvent: "input.paste", scrollIntoView: true });
      },
    },
    setText(text: string) {
      const change = changeBetween(view.state.doc.toString(), text);
      // Not undone with ⌘Z: an image taken out leaves the note with its picture, which an undo
      // wouldn't bring back (a text field's value set from outside clears its undo the same way).
      if (change) view.dispatch({ changes: change, userEvent: "delete", annotations: Transaction.addToHistory.of(false) });
    },
    setImages(images) {
      view.dispatch({ effects: setImages.of(images) });
    },
    applyBlock(block) {
      const watch = view.plugin(slash.plugin);
      const open = watch?.open;
      if (!watch || !open) return;
      watch.close();
      view.dispatch({ ...blockChange(view.state, open, block), userEvent: "input", scrollIntoView: true });
      view.focus();
    },
    placeSlash() {
      view.plugin(slash.plugin)?.place();
    },
    closeSlash() {
      const watch = view.plugin(slash.plugin);
      if (watch?.open) watch.close();
    },
    focus() {
      view.focus();
    },
    setPlaceholder(text: string) {
      if (text !== shownHint) {
        shownHint = text;
        view.dispatch({ effects: hint.reconfigure(placeholderText(text)) });
      }
    },
    destroy() {
      view.destroy();
    },
  };
}
