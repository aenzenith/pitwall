// A Markdown file as the Files window's preview draws it: GitHub's flavour (tables, task lists,
// footnotes, alerts, emoji, heading anchors), KaTeX's maths and Mermaid's diagrams. Loaded with
// the first preview only (MarkdownView imports it when it is needed). Not `lib/markdown`, which
// draws what Claude wrote, with no HTML at all.
//
// What a file holds is never trusted: its HTML goes through DOMPurify (and the app's CSP runs no
// script of it anyway); links and pictures leave the page's reach (`data-href`, `data-src`) for
// the view to handle, ids become `data-id` so nothing of the file can name the app's own
// elements, and nothing is fetched.

import markdownItKatex from "@vscode/markdown-it-katex";
import DOMPurify from "dompurify";
import "katex/dist/katex.min.css";
import MarkdownIt, { type PluginSimple, type PluginWithOptions } from "markdown-it";
import anchor from "markdown-it-anchor";
import { full as emoji } from "markdown-it-emoji";
import footnote from "markdown-it-footnote";
import frontMatter from "markdown-it-front-matter";
import alerts from "markdown-it-github-alerts";
import taskLists from "markdown-it-task-lists";

/** The alerts' titles (`> [!NOTE]`), in the app's language. */
export type AlertTitles = Record<"note" | "tip" | "important" | "warning" | "caution", string>;

/** GitHub's heading anchors: lower case, letters (any script), digits, `-` and `_` kept, spaces
 * made `-`; markdown-it-anchor numbers a name met twice. */
const slug = (text: string) =>
  text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");

/** A plugin as its CommonJS package gives it: the function, or a module whose default it is. */
const plugin = <T>(module: unknown): T => (typeof module === "function" ? module : (module as { default: unknown }).default) as T;

/** markdown-it 14.3.2 looks for the end of an unclosed `<!--`, `<?`, `<!X` or `<![CDATA[` through
 * the rest of its paragraph each time it meets one, so a paragraph of them freezes the page
 * (GHSA-vfgh-jcmw-g37q: 80 KB of `<!--` took 0.9 s, 160 KB 3.5 s). Here each kind's end is looked
 * for once per paragraph, and one that has none is text, as markdown-it 14.3.3 has it. */
function unclosedAsText(md: MarkdownIt): void {
  const ends = new WeakMap<object, Map<string, number>>();
  md.inline.ruler.before("html_inline", "pitwall_unclosed_html", (state, silent) => {
    const { src, pos } = state;
    if (src.charCodeAt(pos) !== 0x3c /* < */) return false;
    let open: number;
    let end: string;
    if (src.startsWith("<!--", pos)) {
      // `<!-->` and `<!--->` are whole comments.
      if (src.startsWith(">", pos + 4) || src.startsWith("->", pos + 4)) return false;
      [open, end] = [4, "-->"];
    } else if (src.startsWith("<![CDATA[", pos)) [open, end] = [9, "]]>"];
    else if (src.startsWith("<?", pos)) [open, end] = [2, "?>"];
    else if (src.startsWith("<!", pos) && /[A-Za-z]/.test(src.charAt(pos + 2))) [open, end] = [3, ">"];
    else return false;
    let known = ends.get(state);
    if (!known) ends.set(state, (known = new Map()));
    let at = known.get(end);
    // The first end after an earlier start is the first after this one too, unless it lies before.
    if (at === undefined || (at >= 0 && at < pos + open)) {
      at = src.indexOf(end, pos + open);
      known.set(end, at);
    }
    if (at >= 0 && at + end.length <= state.posMax) return false;
    if (!silent) state.pending += "<";
    state.pos++;
    return true;
  });
}

let made: { titles: string; md: MarkdownIt } | null = null;

function renderer(titles: AlertTitles): MarkdownIt {
  const key = JSON.stringify(titles);
  if (made?.titles === key) return made.md;
  const md = new MarkdownIt({ html: true, linkify: true });
  // Footnotes', front matter's and KaTeX's types are written for an older markdown-it's; the
  // plugins themselves take this one.
  md.use(plugin<PluginSimple>(footnote))
    .use(plugin<PluginWithOptions<{ enabled?: boolean }>>(taskLists), { enabled: false })
    .use(anchor, { slugify: slug, tabIndex: false })
    .use(alerts, { titles })
    .use(emoji)
    .use(plugin<PluginWithOptions<(meta: string) => void>>(frontMatter), () => undefined)
    .use(plugin<PluginWithOptions<{ throwOnError?: boolean }>>(markdownItKatex), { throwOnError: false });
  unclosedAsText(md);

  const escape = md.utils.escapeHtml;
  // Each block says the line it starts on: a click there picks it for "Open in editor".
  md.core.ruler.push("pitwall_lines", (state) => {
    for (const token of state.tokens) if (token.map && token.nesting >= 0 && token.type !== "inline") token.attrSet("data-line", String(token.map[0] + 1));
  });
  md.renderer.rules.fence = (tokens, at) => {
    const token = tokens[at];
    const lang = token.info.trim().split(/\s+/)[0]?.toLowerCase() ?? "";
    const line = token.map ? ` data-line="${token.map[0] + 1}"` : "";
    if (lang === "mermaid") return `<div class="md-diagram"${line}><pre class="md-code"><code>${escape(token.content)}</code></pre></div>\n`;
    return `<pre class="md-code"${line}${lang ? ` data-lang="${escape(lang)}"` : ""}><code>${escape(token.content)}</code></pre>\n`;
  };
  md.renderer.rules.front_matter = (tokens, at) => `<pre class="md-code md-front" data-line="1" data-lang="yaml"><code>${escape(tokens[at].meta as string)}</code></pre>\n`;
  made = { titles: key, md };
  return md;
}

/** Tags a file's HTML may never bring: forms, frames, embedded media and anything that would
 * style or reach beyond the preview. */
const FORBID_TAGS = ["style", "link", "meta", "base", "form", "input", "button", "textarea", "select", "option", "iframe", "frame", "frameset", "object", "embed", "source", "video", "audio", "track", "dialog", "template", "slot"];

/** What an element fetches or takes the page to, in any namespace (`xlink:href`), never left on
 * one: a map's area, an SVG or MathML `href`, a table's background would each take the app's own
 * window to another page or fetch from the web. */
const LIVE = ["href", "xlink:href", "src", "srcset", "background", "poster", "action", "formaction", "ping", "data", "usemap", "lowsrc", "dynsrc"];
/** A style that names a picture to fetch (not one of the page's own, `url(#…)`), or hides its
 * words behind CSS escapes. */
const FETCHING_STYLE = /\\|\b(?:image|image-set|cross-fade|element|src)\s*\(|\burl\s*\(\s*(?!['"]?#)/i;

/** An element's addresses taken out of the page's reach: a link's (HTML's or SVG's `<a>`) to
 * `data-href`, a picture's to `data-src`, for the view to follow or read through the core; every
 * other one dropped, and a style that would fetch one. */
function disarm(node: Element): void {
  const link = node.localName === "a" ? (node.getAttribute("href") ?? node.getAttribute("xlink:href")) : null;
  const picture = node.localName === "img" ? node.getAttribute("src") : null;
  for (const name of LIVE) node.removeAttribute(name);
  node.removeAttribute("target");
  if (link) {
    node.setAttribute("data-href", link);
    node.setAttribute("role", "link");
    node.setAttribute("tabindex", "0");
    if (!node.getAttribute("title") && !link.startsWith("#")) node.setAttribute("title", link);
  }
  if (picture) node.setAttribute("data-src", picture);
  const style = node.getAttribute("style");
  if (style && FETCHING_STYLE.test(style)) node.removeAttribute("style");
}

let purify: ReturnType<typeof DOMPurify> | null = null;

function cleaner(): ReturnType<typeof DOMPurify> {
  if (purify) return purify;
  purify = DOMPurify(window);
  // KaTeX's TeX source rides in an `<annotation>`, which goes: kept on its formula for a copy.
  purify.addHook("uponSanitizeElement", (node, data) => {
    if (data.tagName === "annotation" && node instanceof Element && node.getAttribute("encoding") === "application/x-tex") {
      node.closest(".katex")?.setAttribute("data-tex", node.textContent ?? "");
    }
  });
  purify.addHook("afterSanitizeAttributes", (node) => {
    if (!(node instanceof Element)) return;
    // Nothing of the file names the app's elements: its ids (an `<a name>` anchor's too) are
    // `data-id`, for its own links to scroll to.
    const id = node.getAttribute("id") ?? (node.localName === "a" ? node.getAttribute("name") : null);
    node.removeAttribute("id");
    node.removeAttribute("name");
    if (id !== null) node.setAttribute("data-id", id);
    disarm(node);
  });
  return purify;
}

/** `text` as the preview's HTML, cleaned. Task lists' boxes are drawn by the view's own marks
 * (`input` is never let through), from the plugin's `checked` class. */
export function renderMarkdown(text: string, titles: AlertTitles): string {
  const html = renderer(titles)
    .render(text)
    // The task lists' boxes, as marks of the list item before `input` is taken out.
    .replace(/<input class="task-list-item-checkbox"\s*(checked="")?[^>]*>/g, (_, checked) => `<span class="md-check${checked ? " done" : ""}" aria-hidden="true"></span>`);
  // A file's own data attributes never come through: the app's code reads some (Tauri's
  // `data-tauri-drag-region` would have a click in the preview drag the app's window).
  return cleaner().sanitize(html, { FORBID_TAGS, ALLOW_DATA_ATTR: false, ADD_ATTR: ["data-line", "data-lang"] });
}

let mermaid: Promise<typeof import("mermaid").default> | null = null;
let svgCleaner: ReturnType<typeof DOMPurify> | null = null;
let drawn = 0;

/** A preview draws this many diagrams at most: each holds the page a moment, dozens would hold it
 * up for seconds. The rest keep their text. */
export const DIAGRAMS = 20;

/** Mermaid's diagrams in `root` drawn in place of their text, in the theme's colours; one that
 * can't be drawn keeps its text under `failed`'s words, one past `DIAGRAMS` under `capped`'s.
 * Mermaid's strict mode: no click handlers, labels cleaned; its links are the view's to follow, as
 * the file's own are. */
export async function drawDiagrams(root: HTMLElement, dark: boolean, failed: string, capped: string): Promise<void> {
  const found = [...root.querySelectorAll<HTMLElement>(".md-diagram")];
  if (!found.length) return;
  // Drawn again (the theme or the language changed and the page kept its HTML): from the source
  // kept on the block the first time, never from the drawing now in it, and without the notes
  // of the last time.
  for (const block of found) {
    block.dataset.source ??= block.textContent ?? "";
    block.querySelectorAll(":scope > .md-diagram-note, :scope > .md-diagram-error").forEach((note) => note.remove());
  }
  for (const block of found.slice(DIAGRAMS)) block.prepend(diagramNote("md-diagram-note", capped));
  const blocks = found.slice(0, DIAGRAMS);
  mermaid ??= import("mermaid").then((module) => module.default);
  const draw = await mermaid;
  draw.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "default", fontFamily: getComputedStyle(root).fontFamily });
  for (const block of blocks) {
    if (!block.isConnected) return;
    const source = block.dataset.source ?? "";
    // Its own: another preview may be drawing at the same time.
    const id = `pitwall-diagram-${++drawn}`;
    try {
      const { svg } = await draw.render(id, source);
      // Its own ids stay: its arrows and its styles name them.
      if (!svgCleaner) {
        svgCleaner = DOMPurify(window);
        svgCleaner.addHook("afterSanitizeAttributes", (node) => node instanceof Element && disarm(node));
      }
      const clean = svgCleaner.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true, html: true }, ADD_TAGS: ["foreignObject"], HTML_INTEGRATION_POINTS: { foreignobject: true } });
      block.innerHTML = clean;
      block.classList.add("drawn");
    } catch (error) {
      // Its text again under the note, should a drawing have been there.
      if (block.classList.contains("drawn")) {
        const code = document.createElement("pre");
        code.className = "md-code";
        code.append(Object.assign(document.createElement("code"), { textContent: source }));
        block.replaceChildren(code);
        block.classList.remove("drawn");
      }
      const reason = error instanceof Error ? error.message.split("\n")[0].slice(0, 200) : "";
      block.prepend(diagramNote("md-diagram-error", `${failed} ${reason}`.trim()));
      // Mermaid leaves its failed attempt in the page's body.
      document.getElementById(`d${id}`)?.remove();
    }
  }
}

function diagramNote(kind: string, text: string): HTMLElement {
  const note = document.createElement("div");
  note.className = kind;
  note.textContent = text;
  return note;
}
