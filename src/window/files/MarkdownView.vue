<script lang="ts">
/** A heading of another file, opened from a link in a preview: scrolled to once that file's
 * own preview (another instance of this view) is drawn. */
const pending = new Map<string, string>();
</script>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, shallowRef, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import { openFile, revealInTree, type ProjectFiles } from "../../lib/explorer";
import { BOLD, highlight, ITALIC } from "../../lib/highlight";
import { language, t } from "../../lib/i18n";
import { languageOf } from "../../lib/languages";
import { isMac } from "../../lib/platform";
import { api } from "../../lib/store";
import { resolvedTheme } from "../../lib/theme";

/**
 * A Markdown file drawn, as VS Code's preview: GitHub's flavour, maths, Mermaid's diagrams, its
 * code coloured as the code view colours it (lib/markdownPreview). Its links: a heading of the file
 * scrolls there, another file of the project opens in a tab, a web address in the browser.
 * Its pictures: the project's own read through the core, one on the web named and never
 * loaded. A click on a block picks its line for "Open in editor".
 */
const props = defineProps<{ files: ProjectFiles; file: string; text: string; current: number | null }>();
/** `opened`: a link opened that file, in the preview tab (a double click keeps it, in FileViewer). */
const emit = defineEmits<{ line: [line: number]; opened: [file: string] }>();

const root = ref<HTMLElement | null>(null);
const html = shallowRef<string | null>(null);
let lib: typeof import("../../lib/markdownPreview") | null = null;
let run = 0;
/** The pictures read for this file, by their path: drawn again with the theme, read once. */
const pictures = new Map<string, string | null>();

async function render(): Promise<void> {
  const mine = ++run;
  lib ??= await import("../../lib/markdownPreview");
  if (mine !== run) return;
  const keep = root.value?.scrollTop ?? 0;
  const first = html.value === null;
  html.value = lib.renderMarkdown(props.text, {
    note: t("files.md.note"),
    tip: t("files.md.tip"),
    important: t("files.md.important"),
    warning: t("files.md.warning"),
    caution: t("files.md.caution"),
  });
  await nextTick();
  const box = root.value;
  if (mine !== run || !box) return;
  const dark = resolvedTheme.value === "dark";
  showPictures(box);
  void colourCode(box, dark, mine);
  void lib.drawDiagrams(box, dark, t("files.md.diagramFailed"), t("files.md.diagramsCapped", { count: lib.DIAGRAMS }));
  if (!first) box.scrollTop = keep;
  else if (!takePending(box) && props.current) toLine(box, props.current);
}

watch([() => props.text, () => resolvedTheme.value, language], render, { immediate: true });
onBeforeUnmount(() => run++);

/* ---------- addresses: a link's or a picture's, as a path of the project ---------- */

const folder = () => (props.file.includes("/") ? props.file.slice(0, props.file.lastIndexOf("/")) : "");

/** `address` from the file's folder (or the project's, after `/`) as a project path and the part
 * after `#`; null for one that names a scheme (the web, mail…) or climbs out of the project. */
function resolve(address: string): { path: string; hash: string } | null {
  if (/^[a-z][a-z0-9+.-]*:/i.test(address) || address.startsWith("//")) return null;
  const cut = address.indexOf("#");
  const hash = cut < 0 ? "" : decode(address.slice(cut + 1));
  const bare = decode((cut < 0 ? address : address.slice(0, cut)).split("?")[0]);
  const parts = bare.startsWith("/") ? [] : folder().split("/").filter(Boolean);
  if (!bare) return { path: props.file, hash };
  for (const part of bare.split("/")) {
    if (!part || part === ".") continue;
    if (part !== "..") parts.push(part);
    else if (!parts.pop()) return null;
  }
  return { path: parts.join("/"), hash };
}

function decode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/* ---------- pictures ---------- */

function showPictures(box: HTMLElement): void {
  for (const img of box.querySelectorAll<HTMLImageElement>("img[data-src]")) {
    const src = img.dataset.src!;
    if (src.startsWith("data:image/")) {
      img.src = src;
      continue;
    }
    const target = resolve(src);
    if (!target) {
      stand(img, t("files.md.pictureOff", { host: hostOf(src) }));
      continue;
    }
    const known = pictures.get(target.path);
    if (known !== undefined) {
      if (known) img.src = known;
      else stand(img, t("files.md.pictureMissing", { name: target.path }));
      continue;
    }
    img.classList.add("reading");
    api
      .explorerImage(props.files.path, target.path)
      .catch(() => null)
      .then((url) => {
        pictures.set(target.path, url);
        if (!img.isConnected) return;
        img.classList.remove("reading");
        if (url) img.src = url;
        else stand(img, t("files.md.pictureMissing", { name: target.path }));
      });
  }
}

const hostOf = (address: string) => {
  try {
    return new URL(address, "https://x.invalid").host || address;
  } catch {
    return address;
  }
};

/** A picture that isn't shown: its alternative text and why, in its place. */
function stand(img: HTMLImageElement, why: string): void {
  const note = document.createElement("span");
  note.className = "md-picture-off";
  note.title = img.dataset.src ?? "";
  const alt = img.getAttribute("alt");
  note.textContent = alt ? `${alt} · ${why}` : why;
  img.replaceWith(note);
}

/* ---------- code, coloured by the worker the code view uses ---------- */

async function colourCode(box: HTMLElement, dark: boolean, mine: number): Promise<void> {
  for (const pre of box.querySelectorAll<HTMLElement>("pre.md-code[data-lang]")) {
    const code = pre.querySelector("code");
    if (!code || pre.closest(".md-diagram")) continue;
    const text = (code.textContent ?? "").replace(/\n$/, "");
    const asked = pre.dataset.lang!;
    let result = await highlight(text, asked, dark);
    const named = languageOf(`x.${asked}`);
    if (!result.lines && named && named !== asked) result = await highlight(text, named, dark);
    if (mine !== run) return;
    if (!result.lines) continue;
    const lines = text.split("\n");
    const out = document.createDocumentFragment();
    result.lines.forEach((tokens, i) => {
      const line = lines[i] ?? "";
      let at = 0;
      for (let k = 0; k < tokens.length; k += 3) {
        const span = document.createElement("span");
        span.textContent = line.slice(at, at + tokens[k]);
        at += tokens[k];
        const colour = result.palette[tokens[k + 1]];
        if (colour) span.style.color = colour;
        if (tokens[k + 2] & ITALIC) span.style.fontStyle = "italic";
        if (tokens[k + 2] & BOLD) span.style.fontWeight = "600";
        out.append(span);
      }
      if (at < line.length) out.append(line.slice(at));
      if (i < lines.length - 1) out.append("\n");
    });
    code.replaceChildren(out);
  }
}

/* ---------- links, and the line a click picks ---------- */

const pendingKey = (path: string) => `${props.files.path}\n${path}`;

function takePending(box: HTMLElement): boolean {
  const hash = pending.get(pendingKey(props.file));
  if (hash === undefined) return false;
  pending.delete(pendingKey(props.file));
  if (!toAnchor(box, hash, false)) return false;
  holdAnchor(box, hash);
  return true;
}

/** Where `element` is in the preview's own scroll, so only the preview scrolls: `scrollIntoView`
 * would scroll whatever holds it too (the Files window, by the few pixels of its edges). */
const offsetIn = (box: HTMLElement, element: HTMLElement) => element.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop;

function toAnchor(box: HTMLElement, hash: string, smooth = true): boolean {
  const target = box.querySelector<HTMLElement>(`[data-id="${CSS.escape(hash)}"]`) ?? box.querySelector<HTMLElement>(`[data-id="${CSS.escape(hash.toLowerCase())}"]`);
  if (target) box.scrollTo({ top: offsetIn(box, target) - 8, behavior: smooth && !matchMedia("(prefers-reduced-motion: reduce)").matches ? "smooth" : "auto" });
  return !!target;
}

/** A heading reached from another file stays at the top while the diagrams and pictures above it
 * come in and push it down, until the reader scrolls or a few seconds have passed. */
let hold: { stop: () => void } | null = null;
function holdAnchor(box: HTMLElement, hash: string): void {
  hold?.stop();
  const body = box.querySelector(".md-page");
  if (!body) return;
  const again = () => toAnchor(box, hash, false);
  const watcher = new ResizeObserver(again);
  watcher.observe(body);
  const user = ["wheel", "pointerdown", "keydown"];
  const stop = () => {
    watcher.disconnect();
    clearTimeout(timer);
    for (const kind of user) box.removeEventListener(kind, stop);
    if (hold?.stop === stop) hold = null;
  };
  const timer = window.setTimeout(stop, 4000);
  for (const kind of user) box.addEventListener(kind, stop, { once: true, passive: true });
  hold = { stop };
}
onBeforeUnmount(() => hold?.stop());

/** The last block that starts at or before `line`, in the middle of the view. */
function toLine(box: HTMLElement, line: number): void {
  let found: HTMLElement | null = null;
  for (const block of box.querySelectorAll<HTMLElement>("[data-line]")) {
    if (Number(block.dataset.line) <= line) found = block;
    else break;
  }
  if (found) box.scrollTop = offsetIn(box, found) - box.clientHeight / 2 + found.offsetHeight / 2;
}

/** A copy of maths takes it once, as drawn: not again from KaTeX's hidden MathML, which a copy
 * in WebKit takes for all its being out of view. The rest of the selection as the page shows it. */
function onCopy(event: ClipboardEvent): void {
  const selection = getSelection();
  if (!selection || selection.isCollapsed || !event.clipboardData || !root.value) return;
  const range = selection.getRangeAt(0);
  if (!root.value.contains(range.commonAncestorContainer)) return;
  const copy = range.cloneContents();
  if (!copy.querySelector(".katex-mathml, .katex")) return;
  // A formula as its TeX (`$…$`, `$$…$$` on a line of its own), as it was written: its drawing
  // laid out as text would scatter its sub- and superscripts over lines.
  copy.querySelectorAll<HTMLElement>(".katex").forEach((math) => {
    const tex = math.dataset.tex;
    if (tex !== undefined) math.replaceWith(math.closest(".katex-display") ? `$$${tex}$$` : `$${tex}$`);
  });
  copy.querySelectorAll(".katex-mathml").forEach((hidden) => hidden.remove());
  // `innerText` gives the lines as laid out: only of what is in the page.
  const holder = document.createElement("div");
  holder.className = "md-page";
  holder.style.cssText = "position:fixed;left:-99999px;top:0;white-space:normal";
  holder.append(copy);
  root.value.append(holder);
  const text = holder.innerText;
  holder.remove();
  event.clipboardData.setData("text/plain", text);
  event.preventDefault();
}

function follow(href: string): void {
  const box = root.value;
  if (!box) return;
  if (href.startsWith("#")) return void toAnchor(box, decode(href.slice(1)));
  if (/^(https?|mailto):/i.test(href)) return void api.openUrl(href);
  const target = resolve(href);
  if (!target) return;
  if (target.path === props.file) return void (target.hash && toAnchor(box, target.hash));
  if (href.split(/[?#]/)[0].endsWith("/") || props.files.folders.has(target.path)) return revealInTree(props.files, target.path);
  if (target.hash) pending.set(pendingKey(target.path), target.hash);
  openFile(props.files, target.path);
  emit("opened", target.path);
}

function onClick(event: MouseEvent): void {
  const target = event.target as HTMLElement;
  // Nothing in the preview takes the app's own page anywhere, whatever the cleaning let through.
  if (target.closest("a, area, [href]")) event.preventDefault();
  const link = target.closest<HTMLElement>("[data-href]");
  if (link && root.value?.contains(link)) {
    event.preventDefault();
    follow(link.dataset.href!);
    return;
  }
  if (getSelection()?.toString()) return;
  const block = target.closest<HTMLElement>("[data-line]");
  if (block) emit("line", Number(block.dataset.line));
}

function onKey(event: KeyboardEvent): void {
  // ⌘A takes the preview's text, not the whole window's.
  if ((isMac ? event.metaKey : event.ctrlKey) && !event.shiftKey && !event.altKey && event.code === "KeyA") {
    const body = root.value?.querySelector(".md-page");
    if (body) {
      event.preventDefault();
      getSelection()?.selectAllChildren(body);
    }
    return;
  }
  const link = (event.target as HTMLElement).closest<HTMLElement>("[data-href]");
  if (event.key === "Enter" && link) {
    event.preventDefault();
    follow(link.dataset.href!);
  }
}
</script>

<template>
  <!-- `selectable`: the app selects no text (tokens.css) but here, to be copied. -->
  <div ref="root" class="markdown-view selectable" tabindex="-1" @click="onClick" @keydown="onKey" @copy="onCopy">
    <div v-if="html === null" class="md-state"><Spinner /></div>
    <!-- Cleaned in lib/markdownPreview (DOMPurify), and the app's CSP runs no script of it. -->
    <!-- eslint-disable-next-line vue/no-v-html -->
    <article v-else class="md-page" v-html="html"></article>
  </div>
</template>

<style scoped>
.markdown-view {
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: var(--bg-log);
  outline: none;
}

.md-state {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
</style>

<!-- The file's own markup: not scoped (v-html), every rule under `.md-page`, a class no other
     component has: these rules reach the whole window once a Files window was shown. Whatever the
     file's HTML sets on its elements stays inside: `contain` keeps even a fixed one in the preview. -->
<style>
.md-page {
  contain: layout paint;
  position: relative;
  max-width: 880px;
  margin: 0 auto;
  padding: 22px 32px 48px;
  font-size: 14px;
  line-height: 1.65;
  color: var(--text);
  overflow-wrap: break-word;
}

.md-page > :first-child {
  margin-top: 0;
}

.md-page h1,
.md-page h2,
.md-page h3,
.md-page h4,
.md-page h5,
.md-page h6 {
  margin: 1.5em 0 0.6em;
  font-weight: 600;
  line-height: 1.3;
  color: var(--text-strong);
}

.md-page h1 {
  padding-bottom: 0.3em;
  border-bottom: 1px solid var(--line-soft);
  font-size: 1.9em;
}

.md-page h2 {
  padding-bottom: 0.3em;
  border-bottom: 1px solid var(--line-soft);
  font-size: 1.45em;
}

.md-page h3 {
  font-size: 1.2em;
}

.md-page h4 {
  font-size: 1.05em;
}

.md-page h5,
.md-page h6 {
  font-size: 0.95em;
  color: var(--text-soft);
}

.md-page p,
.md-page ul,
.md-page ol,
.md-page blockquote,
.md-page table,
.md-page pre,
.md-page details,
.md-page .md-diagram,
.md-page .markdown-alert {
  margin: 0 0 1em;
}

.md-page ul,
.md-page ol {
  padding-left: 2em;
}

.md-page li + li {
  margin-top: 0.25em;
}

.md-page li > ul,
.md-page li > ol,
.md-page li > p {
  margin: 0.25em 0 0;
}

.md-page .task-list-item {
  list-style: none;
}

.md-page .task-list-item .md-check {
  display: inline-block;
  width: 13px;
  height: 13px;
  margin: 0 0.45em 0 -1.5em;
  border: 1px solid var(--line-strong);
  border-radius: 3px;
  vertical-align: -1px;
}

.md-page .task-list-item .md-check.done {
  border-color: var(--run);
  background:
    no-repeat center / 9px url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='3.4' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m5 12.5 4.5 4.5L19 7.5'/%3E%3C/svg%3E"),
    var(--run);
}

.md-page [data-href] {
  color: var(--text-link);
  text-decoration: none;
  cursor: pointer;
}

.md-page [data-href]:hover {
  text-decoration: underline;
  text-underline-offset: 2px;
}

.md-page strong {
  font-weight: 600;
  color: var(--text-strong);
}

.md-page hr {
  height: 1px;
  margin: 1.6em 0;
  border: 0;
  background: var(--line-soft);
}

.md-page blockquote {
  padding: 0 1em;
  border-left: 3px solid var(--line-strong);
  color: var(--text-soft);
}

.md-page code {
  padding: 0.15em 0.4em;
  border-radius: 5px;
  background: var(--bg-tag);
  font-family: var(--font-mono);
  font-size: 0.86em;
}

.md-page pre {
  overflow: auto;
  padding: 12px 14px;
  border-radius: 8px;
  background: var(--bg-code-head);
  box-shadow: inset 0 0 0 1px var(--line-soft);
  line-height: 1.5;
}

.md-page pre code {
  padding: 0;
  background: none;
  font-size: 12.5px;
  white-space: pre;
}

.md-page .md-front {
  color: var(--text-subtle);
}

.md-page kbd {
  padding: 0.1em 0.45em;
  border-radius: 5px;
  background: var(--bg-key);
  box-shadow: inset 0 -1px 0 var(--line-strong), 0 0 0 1px var(--line-soft);
  font-family: var(--font-mono);
  font-size: 0.82em;
}

.md-page table {
  display: block;
  width: max-content;
  max-width: 100%;
  overflow: auto;
  border-collapse: collapse;
}

.md-page th,
.md-page td {
  padding: 6px 12px;
  border: 1px solid var(--line-soft);
}

.md-page th {
  font-weight: 600;
  background: var(--bg-code-head);
  color: var(--text-strong);
}

.md-page tr:nth-child(2n) td {
  background: color-mix(in srgb, var(--bg-code-head) 55%, transparent);
}

.md-page img {
  max-width: 100%;
  height: auto;
  border-radius: 4px;
  vertical-align: middle;
}

.md-page img.reading {
  min-width: 48px;
  min-height: 24px;
  background: var(--bg-tag);
}

.md-page .md-picture-off {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 2px 8px;
  border-radius: 5px;
  background: var(--bg-tag);
  font-size: 12px;
  color: var(--text-subtle);
}

.md-page details > summary {
  cursor: pointer;
}

.md-page .footnotes {
  font-size: 0.88em;
  color: var(--text-soft);
}

.md-page .footnotes-sep {
  margin-top: 2em;
}

/* GitHub's alerts: the edge and the title in the alert's own colour. */
.md-page .markdown-alert {
  padding: 8px 16px;
  border-left: 3px solid var(--md-alert);
}

.md-page .markdown-alert > :last-child {
  margin-bottom: 0;
}

.md-page .markdown-alert-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
  font-weight: 600;
  color: var(--md-alert);
}

.md-page .markdown-alert-title svg {
  fill: currentColor;
}

.md-page .markdown-alert-note {
  --md-alert: var(--lane-0);
}

.md-page .markdown-alert-tip {
  --md-alert: var(--run);
}

.md-page .markdown-alert-important {
  --md-alert: var(--lane-1);
}

.md-page .markdown-alert-warning {
  --md-alert: var(--warn);
}

.md-page .markdown-alert-caution {
  --md-alert: var(--crash);
}

/* Mermaid's diagrams: centred, never wider than the page; the text until drawn. */
.md-page .md-diagram.drawn {
  display: flex;
  justify-content: center;
  overflow: auto;
}

.md-page .md-diagram svg {
  max-width: 100%;
  height: auto;
}

.md-page .md-diagram-error,
.md-page .md-diagram-note {
  margin-bottom: 6px;
  font-size: 12px;
  color: var(--crash-text);
}

.md-page .md-diagram-note {
  color: var(--text-subtle);
}

/* KaTeX: display maths scroll on their own when wider than the page; a copy takes the maths once,
   as drawn, not again from its hidden MathML. */
.md-page .katex-mathml {
  -webkit-user-select: none;
  user-select: none;
}

.md-page .katex-display {
  overflow-x: auto;
  overflow-y: hidden;
  padding: 2px 0;
}
</style>
