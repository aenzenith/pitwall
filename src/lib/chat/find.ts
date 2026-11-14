/**
 * ⌘F in a chat: the terminal's search bar (TerminalFind, lib/terminals `finds`) over the
 * conversation's text as drawn. The matches are marked with the CSS Custom Highlight API where the
 * web view has it (WebKit 17.2+, Chromium); the one the search stands on is also selected, so it
 * shows (and ⌘C copies it) where the API is missing. Only what is drawn is searched: a folded
 * output isn't.
 */

/** The search's choices (lib/terminals `findOptions`). */
export type FindChoices = { caseSensitive: boolean; wholeWord: boolean; regex: boolean };

type Match = Range;

const ALL = "chat-find";
const NOW = "chat-find-now";

type Highlights = { set: (name: string, value: unknown) => void; delete: (name: string) => void };
type HighlightCtor = new (...ranges: Range[]) => unknown;

function highlights(): { registry: Highlights; Highlight: HighlightCtor } | null {
  const registry = (globalThis.CSS as unknown as { highlights?: Highlights } | undefined)?.highlights;
  const Highlight = (globalThis as unknown as { Highlight?: HighlightCtor }).Highlight;
  return registry && Highlight ? { registry, Highlight } : null;
}

/** The pattern the query and the choices make; null when a regular expression doesn't parse. It
 * parses as the terminal's search parses it (no `u` flag: `\-`, a lone `{` are taken there too). */
export function patternOf(query: string, choices: FindChoices): RegExp | null {
  const body = choices.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const flags = choices.caseSensitive ? "g" : "gi";
  try {
    new RegExp(body, flags);
  } catch {
    return null;
  }
  if (!choices.wholeWord) return new RegExp(body, flags);
  try {
    // Unicode's word edges: `\p{…}` needs the `u` flag (a body it refuses gets ASCII's edges).
    return new RegExp(`(?<![\\p{L}\\p{N}_])(?:${body})(?![\\p{L}\\p{N}_])`, `${flags}u`);
  } catch {
    // A web view without lookbehind (WebKit before 16.4): ASCII word edges.
    return new RegExp(`\\b(?:${body})\\b`, flags);
  }
}

/** One chat's search: what it found last and where it stands. */
export class ChatFind {
  private matches: Match[] = [];
  private index = -1;
  private key = "";

  constructor(
    private readonly root: () => HTMLElement | null,
    private readonly limit: number,
  ) {}

  /** Looks for `query` again; `step` moves from the match it stands on (`previous`: up, to older
   * messages), none keeps it there. A new query starts from the newest match. */
  search(query: string, choices: FindChoices, step: "next" | "previous" | null): { index: number; count: number; invalid: boolean } {
    const box = this.root();
    const pattern = query ? patternOf(query, choices) : null;
    if (!box || !pattern) {
      this.clear();
      return { index: -1, count: 0, invalid: !!query && !pattern };
    }
    const key = `${pattern.source}/${pattern.flags}`;
    const was = this.matches[this.index] ?? null;
    this.matches = collect(box, pattern, this.limit);
    const count = this.matches.length;
    if (!count) {
      this.index = -1;
    } else if (key !== this.key || this.index < 0) {
      this.index = count - 1;
      if (step === "next") this.index = 0;
    } else {
      // Where it stood, found again among the new matches (the conversation may have grown).
      let at = was ? this.matches.findIndex((range) => same(range, was)) : -1;
      if (at < 0) at = Math.min(this.index, count - 1);
      if (step === "previous") at = (at - 1 + count) % count;
      else if (step === "next") at = (at + 1) % count;
      this.index = at;
    }
    this.key = key;
    this.paint(box);
    return { index: this.index, count, invalid: false };
  }

  /** The text selected in the conversation, on one line: what ⌘F looks for. */
  selection(): string {
    const box = this.root();
    const picked = getSelection();
    if (!box || !picked || picked.isCollapsed || !box.contains(picked.anchorNode)) return "";
    const text = picked.toString();
    return text.trim() && !text.includes("\n") ? text : "";
  }

  /** The marks go; the match it stood on stays selected. */
  clear(): void {
    const api = highlights();
    api?.registry.delete(ALL);
    api?.registry.delete(NOW);
    this.matches = [];
    this.index = -1;
    this.key = "";
  }

  private paint(box: HTMLElement): void {
    const api = highlights();
    const current = this.matches[this.index];
    if (api) {
      api.registry.set(ALL, new api.Highlight(...this.matches));
      if (current) api.registry.set(NOW, new api.Highlight(current));
      else api.registry.delete(NOW);
    }
    if (!current) return;
    const picked = getSelection();
    if (picked && !api) {
      picked.removeAllRanges();
      picked.addRange(current.cloneRange());
    }
    reveal(box, current);
  }
}

function same(a: Range, b: Range): boolean {
  return a.startContainer === b.startContainer && a.startOffset === b.startOffset && a.endOffset === b.endOffset;
}

/** The matches in the conversation's text, in its order: text inside `[data-find]`, never in
 * `[data-nofind]` (buttons, chips). */
function collect(box: HTMLElement, pattern: RegExp, limit: number): Range[] {
  const found: Range[] = [];
  const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = node.parentElement;
      if (!parent || !node.nodeValue) return NodeFilter.FILTER_REJECT;
      if (!parent.closest("[data-find]") || parent.closest("[data-nofind]")) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  for (let node = walker.nextNode(); node && found.length < limit; node = walker.nextNode()) {
    const text = node.nodeValue ?? "";
    pattern.lastIndex = 0;
    for (let match = pattern.exec(text); match && found.length < limit; match = pattern.exec(text)) {
      if (!match[0].length) {
        pattern.lastIndex++;
        continue;
      }
      const range = document.createRange();
      range.setStart(node, match.index);
      range.setEnd(node, match.index + match[0].length);
      found.push(range);
    }
  }
  return found;
}

/** The match brought into the middle of its scroll box, when it is out of sight. */
function reveal(box: HTMLElement, range: Range): void {
  const scroller = box.closest<HTMLElement>("[data-scroll]") ?? box;
  const at = range.getBoundingClientRect();
  const view = scroller.getBoundingClientRect();
  if (at.top >= view.top + 24 && at.bottom <= view.bottom - 24) return;
  scroller.scrollTop += at.top - view.top - view.height / 2;
}
