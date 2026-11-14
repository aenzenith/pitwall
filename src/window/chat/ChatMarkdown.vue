<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, shallowRef, watch } from "vue";

import { BOLD, highlight, ITALIC } from "../../lib/highlight";
import { language, t } from "../../lib/i18n";
import { languageOf } from "../../lib/languages";
import { api } from "../../lib/store";
import { resolvedTheme } from "../../lib/theme";

/**
 * Claude's words, drawn as Markdown by the Files window's renderer (lib/markdownPreview: DOMPurify,
 * links and pictures held back), its code coloured as the code view colours it, at the terminal
 * panel's size. A link opens only when clicked: a web address in the browser, a file of the
 * project in the editor (at its line, `file.ts:12` or `#L12`). While the words stream in, they
 * are drawn again a few times a second, not at every word.
 */
const props = defineProps<{ text: string; streaming: boolean; project: string }>();

const root = ref<HTMLElement | null>(null);
const html = shallowRef("");
let lib: typeof import("../../lib/markdownPreview") | null = null;
let run = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let last = 0;

/** How often streaming words are drawn again. */
const STREAM_MS = 90;

async function render(): Promise<void> {
  const mine = ++run;
  lib ??= await import("../../lib/markdownPreview");
  if (mine !== run) return;
  last = Date.now();
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
  for (const img of box.querySelectorAll<HTMLImageElement>("img[data-src]")) {
    // Claude's pictures are never loaded: named, as a web picture is in the Files window.
    const note = document.createElement("span");
    note.className = "md-picture-off";
    note.textContent = img.getAttribute("alt") || img.dataset.src || "";
    img.replaceWith(note);
  }
  addCopy(box);
  if (props.streaming) return;
  void colourCode(box, dark, mine);
  void lib.drawDiagrams(box, dark, t("files.md.diagramFailed"), t("files.md.diagramsCapped", { count: lib.DIAGRAMS }));
}

function schedule(): void {
  if (!props.streaming) {
    clearTimeout(timer);
    timer = undefined;
    void render();
    return;
  }
  if (timer) return;
  timer = setTimeout(() => {
    timer = undefined;
    void render();
  }, Math.max(0, STREAM_MS - (Date.now() - last)));
}

watch(() => props.text, schedule, { immediate: true });
watch([() => props.streaming, () => resolvedTheme.value, language], () => {
  clearTimeout(timer);
  timer = undefined;
  void render();
});
onBeforeUnmount(() => {
  run++;
  clearTimeout(timer);
});

const COPY_SVG =
  '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8.5" y="8.5" width="11" height="11" rx="2"/><path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2"/></svg>';

/** A copy button on each block of code, shown while the pointer is over it. */
function addCopy(box: HTMLElement): void {
  for (const pre of box.querySelectorAll<HTMLElement>("pre.md-code")) {
    if (pre.querySelector(".md-copy") || pre.closest(".md-diagram")) continue;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "md-copy";
    button.title = t("chat.copyCode");
    button.setAttribute("aria-label", t("chat.copyCode"));
    button.dataset.nofind = "";
    button.innerHTML = COPY_SVG;
    pre.append(button);
  }
}

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

/** A link's address: the web's in the browser; else a file of the project, in the editor. */
function follow(href: string): void {
  if (/^(https?|mailto):/i.test(href)) {
    void api.openUrl(href).catch(() => undefined);
    return;
  }
  // Another scheme's address (`ftp://…`, `javascript:`…) opens nothing; `package.json:12`,
  // `main.rs:10` and `C:/…` are files.
  if (/^[a-z][a-z0-9+.-]+:\/\//i.test(href) || /^(javascript|vbscript|data|blob|about|file|tel|sms|urn|news|magnet):/i.test(href) || href.startsWith("#")) return;
  let file = href;
  let line = 1;
  const anchor = /#L(\d+)/.exec(file);
  if (anchor) line = Number(anchor[1]);
  file = file.replace(/[#?].*$/, "");
  const suffix = /:(\d+)(?::\d+)?$/.exec(file);
  if (suffix) {
    line = Number(suffix[1]);
    file = file.slice(0, suffix.index);
  }
  try {
    file = decodeURIComponent(file);
  } catch {
    // As written.
  }
  // Claude runs in the project's own folder: its paths are from there.
  if (file) void api.openReference(props.project, file, line, null, true).catch(() => false);
}

function onClick(event: MouseEvent): void {
  const target = event.target as HTMLElement;
  if (target.closest("a, area, [href]")) event.preventDefault();
  const copy = target.closest<HTMLElement>(".md-copy");
  if (copy) {
    const text = copy.closest("pre")?.querySelector("code")?.textContent ?? "";
    void api.copyText(text.replace(/\n$/, "")).catch(() => undefined);
    copy.classList.add("done");
    setTimeout(() => copy.classList.remove("done"), 1200);
    return;
  }
  const link = target.closest<HTMLElement>("[data-href]");
  if (link && root.value?.contains(link)) {
    event.preventDefault();
    follow(link.dataset.href!);
  }
}

function onKey(event: KeyboardEvent): void {
  const link = (event.target as HTMLElement).closest<HTMLElement>("[data-href]");
  if (event.key === "Enter" && link) {
    event.preventDefault();
    follow(link.dataset.href!);
  }
}
</script>

<template>
  <!-- Cleaned in lib/markdownPreview (DOMPurify), and the app's CSP runs no script of it. -->
  <!-- eslint-disable-next-line vue/no-v-html -->
  <div ref="root" :class="['chat-md', { streaming }]" data-find @click="onClick" @keydown="onKey" v-html="html"></div>
</template>

<!-- Claude's markup: not scoped (v-html), every rule under `.chat-md`. -->
<style>
/* No paint containment: WebKit kept such a box's drawing at its old size while the streamed words
   grew it, so its new lines stayed blank until a scroll repainted them (the user's word, 17 Nov:
   "ekranda son yazılar gözükmüyor, scroll yapınca kendine geliyor"). */
.chat-md {
  position: relative;
  font-size: 13.5px;
  line-height: 1.6;
  color: var(--text);
  overflow-wrap: break-word;
}

.chat-md > :first-child {
  margin-top: 0;
}

.chat-md > :last-child {
  margin-bottom: 0;
}

.chat-md h1,
.chat-md h2,
.chat-md h3,
.chat-md h4,
.chat-md h5,
.chat-md h6 {
  margin: 1.2em 0 0.5em;
  font-weight: 600;
  line-height: 1.3;
  color: var(--text-strong);
}

.chat-md h1 {
  font-size: 1.3em;
}

.chat-md h2 {
  font-size: 1.18em;
}

.chat-md h3 {
  font-size: 1.06em;
}

.chat-md h4,
.chat-md h5,
.chat-md h6 {
  font-size: 1em;
}

.chat-md p,
.chat-md ul,
.chat-md ol,
.chat-md blockquote,
.chat-md table,
.chat-md pre,
.chat-md details,
.chat-md .md-diagram,
.chat-md .markdown-alert {
  margin: 0 0 0.8em;
}

.chat-md ul,
.chat-md ol {
  padding-left: 1.6em;
}

.chat-md li + li {
  margin-top: 0.2em;
}

.chat-md li > ul,
.chat-md li > ol,
.chat-md li > p {
  margin: 0.2em 0 0;
}

.chat-md .task-list-item {
  list-style: none;
}

.chat-md .task-list-item .md-check {
  display: inline-block;
  width: 12px;
  height: 12px;
  margin: 0 0.45em 0 -1.4em;
  border: 1px solid var(--line-strong);
  border-radius: 3px;
  vertical-align: -1px;
}

.chat-md .task-list-item .md-check.done {
  border-color: var(--run);
  background: var(--run);
}

.chat-md [data-href] {
  color: var(--text-link);
  text-decoration: none;
  cursor: pointer;
}

.chat-md [data-href]:hover {
  text-decoration: underline;
  text-underline-offset: 2px;
}

.chat-md strong {
  font-weight: 600;
  color: var(--text-strong);
}

.chat-md hr {
  height: 1px;
  margin: 1.2em 0;
  border: 0;
  background: var(--line-soft);
}

.chat-md blockquote {
  padding: 0 0.9em;
  border-left: 3px solid var(--line-strong);
  color: var(--text-soft);
}

.chat-md code {
  padding: 0.1em 0.38em;
  border-radius: 5px;
  background: var(--bg-tag);
  font-family: var(--font-mono);
  font-size: 12px;
}

.chat-md pre {
  position: relative;
  overflow: auto;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-code-head);
  box-shadow: inset 0 0 0 1px var(--line-soft);
  line-height: 1.5;
}

/* The terminal panel's size for code (lib/terminals: 12px). */
.chat-md pre code {
  padding: 0;
  background: none;
  font-size: 12px;
  white-space: pre;
}

/* A block of code scrolls inside its frame, not the frame itself: its copy button stays at the
   frame's corner while the lines move under it. */
.chat-md pre.md-code {
  overflow: hidden;
  padding: 0;
}

.chat-md pre.md-code > code {
  display: block;
  overflow: auto;
  padding: 10px 12px;
}

/* Chrome, not words: never taken into a selection of the answer around it. */
.chat-md .md-copy,
.chat-md .md-copy * {
  -webkit-user-select: none;
  user-select: none;
}

.chat-md .md-copy {
  position: absolute;
  top: 6px;
  right: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: var(--bg-control);
  color: var(--text-muted);
  opacity: 0;
  transition: opacity 0.12s;
}

.chat-md pre:hover .md-copy,
.chat-md .md-copy:focus-visible,
.chat-md .md-copy.done {
  opacity: 1;
}

.chat-md .md-copy:hover {
  background: var(--bg-control-hover);
  color: var(--text);
}

.chat-md .md-copy.done {
  color: var(--run-text);
}

.chat-md table {
  display: block;
  width: max-content;
  max-width: 100%;
  overflow: auto;
  border-collapse: collapse;
  font-size: 12.5px;
}

.chat-md th,
.chat-md td {
  padding: 5px 10px;
  border: 1px solid var(--line-soft);
}

.chat-md th {
  font-weight: 600;
  background: var(--bg-code-head);
  color: var(--text-strong);
}

.chat-md .md-picture-off {
  display: inline-flex;
  padding: 1px 7px;
  border-radius: 5px;
  background: var(--bg-tag);
  font-size: 12px;
  color: var(--text-subtle);
}

.chat-md .markdown-alert {
  padding: 6px 12px;
  border-left: 3px solid var(--line-strong);
}

.chat-md .markdown-alert-title {
  font-weight: 600;
}

.chat-md .footnotes {
  font-size: 0.88em;
  color: var(--text-soft);
}
</style>
