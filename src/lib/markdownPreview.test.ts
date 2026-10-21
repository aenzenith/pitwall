// @vitest-environment jsdom
// The Markdown preview draws a repository's file inside the app's own page, which can call the
// core: what comes out of `renderMarkdown` must hold no script, nothing that leaves the page (a
// live link, a fetched picture) and nothing that names the app's own elements.
import { describe, expect, it } from "vitest";

import { renderMarkdown } from "./markdownPreview";

const titles = { note: "Note", tip: "Tip", important: "Important", warning: "Warning", caution: "Caution" };

/** The preview's HTML, set into an element as the view's `v-html` sets it. */
function drawn(text: string): HTMLElement {
  const article = document.createElement("article");
  article.innerHTML = renderMarkdown(text, titles);
  return article;
}

const all = (root: Element) => [...root.querySelectorAll("*")];

/** What a file may bring, each a way out of the preview or into the app. */
const HOSTILE = [
  // Script, in every place a page runs one.
  "<script>alert(1)</script>",
  '<img src="x.png" onerror="alert(1)"><details open ontoggle="alert(1)"><summary>s</summary></details>',
  '<svg onload="alert(1)"><script>alert(1)</script><a href="javascript:alert(1)"><text>x</text></a><animate attributeName="href" to="javascript:alert(1)"/><set attributeName="href" to="https://evil.test/set"/></svg>',
  '<math><mtext><img src="x" onerror="alert(1)"></mtext></math>',
  // Links that would take the app's own window to another page.
  '<a href="javascript:alert(1)">j</a> <a href="data:text/html,<script>alert(1)</script>">d</a> <a href="https://evil.test/a" target="_top" ping="https://evil.test/ping">a</a>',
  '<svg><a href="https://evil.test/svg-a"><text>x</text></a><a xlink:href="https://evil.test/svg-xlink"><circle r="5"/></a></svg>',
  '<math href="https://evil.test/math"><mi>x</mi></math>',
  '<img src="data:image/png;base64,AAAA" usemap="#m"><map name="m"><area shape="default" href="https://evil.test/area"></map>',
  '<span data-href="javascript:alert(1)">s</span>',
  // Pictures and styles fetched from the web.
  '<img src="https://evil.test/img.png" srcset="https://evil.test/img2.png 2x">',
  '<picture><source srcset="https://evil.test/source.png"><img src="https://evil.test/picture.png"></picture>',
  '<svg><image href="https://evil.test/svg-image.png"/><image xlink:href="https://evil.test/svg-image2.png"/><filter><feImage href="https://evil.test/feimage.png"/></filter><use href="https://evil.test/use.svg#a"/></svg>',
  '<table background="https://evil.test/table.png"><tr><td background="https://evil.test/td.png">a</td></tr></table>',
  '<div style="background: url(https://evil.test/style.png)">s</div><div style="background: \\75rl(https://evil.test/escaped.png)">e</div><div style="background-image: image-set(&quot;https://evil.test/set.png&quot; 1x)">i</div>',
  '<video poster="https://evil.test/poster.png" src="https://evil.test/v.mp4"></video><audio src="https://evil.test/a.mp3"></audio>',
  // Frames, forms, and what reaches past the preview.
  '<iframe src="https://evil.test/frame"></iframe><frameset><frame src="https://evil.test/f"></frameset><object data="https://evil.test/o"></object><embed src="https://evil.test/e">',
  '<form action="https://evil.test/form"><input name="q"><textarea></textarea><select><option>o</option></select><button formaction="https://evil.test/button">b</button></form>',
  '<style>body { display: none }</style><link rel="stylesheet" href="https://evil.test/x.css"><meta http-equiv="refresh" content="0;url=https://evil.test/meta"><base href="https://evil.test/">',
  '<template><script>alert(1)</script></template><dialog open>d</dialog><slot></slot>',
  // Names of the app's own elements, and the app's own hooks.
  '<p id="app">i</p><img name="cookie" src="x.png"><a name="getElementById">n</a><div data-tauri-drag-region data-files-toggle data-tab="x">drag</div>',
  // The same, written as Markdown.
  "[j](javascript:alert(1)) [d](data:text/html,x) ![remote](https://evil.test/md.png) <https://evil.test/auto>",
].join("\n\n");

/** Tags that never come through. */
const FORBIDDEN_TAGS = ["script", "iframe", "frame", "frameset", "object", "embed", "form", "input", "button", "textarea", "select", "option", "style", "link", "meta", "base", "video", "audio", "source", "template", "dialog", "slot", "use", "animate", "set"];
/** Attributes that make the page go somewhere or fetch something, in any namespace (`xlink:href`). */
const LIVE_ATTRIBUTES = ["href", "src", "srcset", "background", "poster", "action", "formaction", "ping", "data", "usemap"];
/** The data attributes the preview itself writes; a file's own never come through. */
const OWN_DATA = ["data-line", "data-lang", "data-href", "data-src", "data-id", "data-tex"];

describe("the Markdown preview's cleaning", () => {
  const article = drawn(HOSTILE);
  const elements = all(article);

  it("lets no script, frame, form or page-wide tag through", () => {
    for (const tag of FORBIDDEN_TAGS) expect(article.querySelectorAll(tag), tag).toHaveLength(0);
    for (const element of elements) for (const attribute of element.attributes) expect(attribute.name, element.outerHTML).not.toMatch(/^on/i);
  });

  it("leaves no live address on any element: links and pictures are the view's to follow", () => {
    for (const element of elements)
      for (const attribute of element.attributes) {
        expect(LIVE_ATTRIBUTES, `${attribute.name} on ${element.outerHTML}`).not.toContain(attribute.localName);
        // The web's addresses stay only where the view reads them and never loads them itself.
        if (/evil\.test/.test(attribute.value)) expect(["data-href", "data-src", "title"], `${attribute.name} on ${element.outerHTML}`).toContain(attribute.name);
      }
  });

  it("moves every link to data-href, and never a script's", () => {
    const links = [...article.querySelectorAll("[data-href]")].map((link) => link.getAttribute("data-href"));
    expect(links).toEqual(expect.arrayContaining(["https://evil.test/a", "https://evil.test/svg-a", "https://evil.test/svg-xlink", "https://evil.test/auto"]));
    for (const link of links) expect(link).not.toMatch(/^\s*(javascript|data|vbscript):/i);
    // An SVG link is followed as an HTML one is.
    expect(article.querySelector("svg a[data-href]")?.getAttribute("role")).toBe("link");
  });

  it("moves every picture to data-src: none is given a live src", () => {
    const pictures = [...article.querySelectorAll("img")];
    expect(pictures.length).toBeGreaterThan(0);
    for (const img of pictures) {
      expect(img.hasAttribute("src"), img.outerHTML).toBe(false);
      expect(img.hasAttribute("srcset"), img.outerHTML).toBe(false);
    }
    const sources = pictures.map((img) => img.getAttribute("data-src"));
    expect(sources).toEqual(expect.arrayContaining(["https://evil.test/img.png", "https://evil.test/md.png", "x.png"]));
  });

  it("names none of the app's elements and sets off none of its hooks", () => {
    for (const element of elements) {
      expect(element.hasAttribute("id"), element.outerHTML).toBe(false);
      expect(element.hasAttribute("name"), element.outerHTML).toBe(false);
      for (const attribute of element.attributes) if (attribute.name.startsWith("data-")) expect(OWN_DATA, `${attribute.name} on ${element.outerHTML}`).toContain(attribute.name);
    }
    expect(article.querySelector('[data-id="app"]')?.textContent).toBe("i");
  });

  it("draws a task list's boxes as marks, not inputs", () => {
    const list = drawn("- [x] done\n- [ ] todo");
    expect(list.querySelectorAll("input")).toHaveLength(0);
    expect(list.querySelectorAll(".task-list-item .md-check")).toHaveLength(2);
    expect(list.querySelectorAll(".task-list-item .md-check.done")).toHaveLength(1);
  });
});

describe("the Markdown preview's drawing", () => {
  const article = drawn(
    [
      "# Getting started",
      "",
      "Text with a [link](./docs/setup.md#install) and a footnote.[^1]",
      "",
      "| Name | Value |",
      "| ---- | ----- |",
      "| a    | 1     |",
      "",
      "```ts",
      "const answer = 42;",
      "```",
      "",
      "```mermaid",
      "graph TD; A-->B",
      "```",
      "",
      "Inline $x^2$ and",
      "",
      "$$",
      "\\int_0^1 x\\,dx",
      "$$",
      "",
      "> [!NOTE]",
      "> Read this first.",
      "",
      "[^1]: The footnote.",
    ].join("\n"),
  );

  it("still draws GitHub's Markdown, its maths and its diagrams' text", () => {
    const heading = article.querySelector("h1");
    expect(heading?.textContent).toContain("Getting started");
    expect(heading?.getAttribute("data-id")).toBe("getting-started");
    expect(heading?.getAttribute("data-line")).toBe("1");
    expect(article.querySelector("[data-href]")?.getAttribute("data-href")).toBe("./docs/setup.md#install");
    expect(article.querySelectorAll("table th")).toHaveLength(2);
    expect(article.querySelector('pre.md-code[data-lang="ts"] code')?.textContent).toBe("const answer = 42;\n");
    expect(article.querySelector(".md-diagram pre code")?.textContent).toContain("graph TD; A-->B");
    expect(article.querySelector(".katex")).not.toBeNull();
    expect(article.querySelector(".katex-display")).not.toBeNull();
    expect(article.querySelector(".markdown-alert-note .markdown-alert-title")?.textContent).toContain("Note");
    expect(article.querySelector(".footnotes")?.textContent).toContain("The footnote.");
    expect(article.querySelector(".footnote-ref [data-href]")?.getAttribute("data-href")).toMatch(/^#fn/);
  });
});
