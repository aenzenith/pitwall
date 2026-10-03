// Markdown as blocks and spans, for showing what Claude wrote (a board card's last message). The
// subset Claude writes: headings, paragraphs, lists, fenced code, tables, quotes, rules; bold,
// italic, strike and inline code. A link shows its text and an image its alt: nothing is opened or
// fetched. No HTML is made here or by what draws it (components/Markdown builds elements), so
// markup in the text stays text.

export type Span = { kind: "text" | "code"; text: string } | { kind: "strong" | "em" | "strike"; spans: Span[] };

export type Block =
  | { kind: "heading"; level: number; spans: Span[] }
  | { kind: "paragraph"; spans: Span[] }
  /** A list's item: `depth` under the items above it, `marker` its number as written or a bullet. */
  | { kind: "item"; depth: number; marker: string; spans: Span[] }
  | { kind: "quote"; spans: Span[] }
  | { kind: "code"; text: string }
  | { kind: "table"; head: Span[][] | null; rows: Span[][][] }
  | { kind: "rule" };

/** Inline code, bold, strike, a link or image, then italic with `*` and with `_` (each after the
 * character before it, which stays text: a `*` or `_` inside a word is no italic). */
const SPAN =
  /`([^`\n]+)`|\*\*(\S(?:[\s\S]*?\S)?)\*\*|~~(\S(?:[\s\S]*?\S)?)~~|!?\[([^\]\n]*)\]\([^)\s]*\)|(^|[^\w*])\*(\S(?:[^*\n]*?\S)?)\*(?![\w*])|(^|[^\w_])_(\S(?:[^_\n]*?\S)?)_(?![\w_])/g;

export function spans(text: string): Span[] {
  const out: Span[] = [];
  const plain = (part: string): void => {
    if (!part) return;
    const last = out[out.length - 1];
    if (last?.kind === "text") last.text += part;
    else out.push({ kind: "text", text: part });
  };

  let at = 0;
  for (const found of text.matchAll(SPAN)) {
    plain(text.slice(at, found.index));
    at = found.index + found[0].length;
    if (found[1] !== undefined) out.push({ kind: "code", text: found[1] });
    else if (found[2] !== undefined) out.push({ kind: "strong", spans: spans(found[2]) });
    else if (found[3] !== undefined) out.push({ kind: "strike", spans: spans(found[3]) });
    else if (found[4] !== undefined) plain(found[4]);
    else {
      plain(found[5] ?? found[7] ?? "");
      out.push({ kind: "em", spans: spans(found[6] ?? found[8] ?? "") });
    }
  }
  plain(text.slice(at));
  return out;
}

const FENCE = /^\s*(```+|~~~+)/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;
const ROW = /^\s*\|(.+)\|\s*$/;
/** A table's line under its head: `|---|:--:|`. */
const DIVIDER = /^\s*\|(\s*:?-+:?\s*\|)+\s*$/;

/** The line begins a block of its own, so the paragraph or item above it ends. */
function begins(line: string): boolean {
  return FENCE.test(line) || HEADING.test(line) || RULE.test(line) || ITEM.test(line) || QUOTE.test(line) || ROW.test(line);
}

export function blocks(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  /** The indents of the items a next item may sit under; empty outside a list. */
  let indents: number[] = [];
  let i = 0;

  /** This line and those under it that go on with it: up to a blank line or another block. */
  const run = (first: string): string => {
    const body = [first];
    i++;
    while (i < lines.length && lines[i].trim() && !begins(lines[i])) body.push(lines[i++].trim());
    return body.join("\n");
  };

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }

    const item = RULE.test(line) ? null : ITEM.exec(line);
    if (item) {
      const indent = item[1].replace(/\t/g, "  ").length;
      while (indents.length && indents[indents.length - 1] >= indent) indents.pop();
      const depth = indents.length;
      indents.push(indent);
      out.push({ kind: "item", depth, marker: /^\d/.test(item[2]) ? item[2].replace(")", ".") : "•", spans: spans(run(item[3])) });
      continue;
    }
    indents = [];

    const fence = FENCE.exec(line);
    const heading = HEADING.exec(line);
    if (fence) {
      // Up to its closing fence; one left open (a message cut off) runs to the end.
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trimStart().startsWith(fence[1])) body.push(lines[i++]);
      i++;
      out.push({ kind: "code", text: body.join("\n") });
    } else if (heading) {
      out.push({ kind: "heading", level: heading[1].length, spans: spans(heading[2]) });
      i++;
    } else if (RULE.test(line)) {
      out.push({ kind: "rule" });
      i++;
    } else if (ROW.test(line)) {
      const table: string[] = [];
      while (i < lines.length && ROW.test(lines[i])) table.push(lines[i++]);
      const cells = (row: string): Span[][] =>
        row
          .trim()
          .slice(1, -1)
          .split("|")
          .map((cell) => spans(cell.trim()));
      const headed = table.length > 1 && DIVIDER.test(table[1]);
      out.push({ kind: "table", head: headed ? cells(table[0]) : null, rows: table.slice(headed ? 2 : 0).map(cells) });
    } else if (QUOTE.test(line)) {
      const body: string[] = [];
      for (let quote = QUOTE.exec(lines[i]); quote; quote = i < lines.length ? QUOTE.exec(lines[i]) : null) {
        body.push(quote[1]);
        i++;
      }
      out.push({ kind: "quote", spans: spans(body.join("\n")) });
    } else {
      out.push({ kind: "paragraph", spans: spans(run(line.trim())) });
    }
  }

  return out;
}
