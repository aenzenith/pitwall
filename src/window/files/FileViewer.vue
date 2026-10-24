<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { absolutePath, closeTab, closeTabs, COMMIT_TAB, gutterOf, handOver, keepTab, markOf, revealInTree, sizeText, type ProjectFiles } from "../../lib/explorer";
import { highlight, type HighlightResult } from "../../lib/highlight";
import { language, t } from "../../lib/i18n";
import { languageOf } from "../../lib/languages";
import { isMac } from "../../lib/platform";
import type { MenuEntry } from "../../lib/nativeMenu";
import { api, snapshot } from "../../lib/store";
import { resolvedTheme } from "../../lib/theme";
import type { DiffLine } from "../../lib/unifiedDiff";
import CodeView from "./CodeView.vue";
import CommitView from "./CommitView.vue";
import DiffView from "./DiffView.vue";
import FileIcon from "./FileIcon.vue";
import ImageView, { type PictureInfo } from "./ImageView.vue";
import MarkdownView from "./MarkdownView.vue";

const props = defineProps<{ files: ProjectFiles; revealLabel: string }>();
const emit = defineEmits<{ menu: [event: MouseEvent, path: string, head: MenuEntry[]] }>();

const active = computed(() => props.files.active);
const opened = computed(() => (active.value ? props.files.opened.get(active.value) : undefined));
const view = computed(() => opened.value?.view);
const text = computed(() => (view.value?.kind === "text" ? view.value.text : ""));
const lineCount = computed(() => (text.value ? text.value.split("\n").length : 0));
const mark = computed(() => (active.value ? markOf(props.files, active.value) : null));
/** A commit's tab (Source control's history) rather than a file's. */
const commitOf = (tab: string) => (tab.startsWith(COMMIT_TAB) ? tab.slice(COMMIT_TAB.length) : null);
const commit = computed(() => (active.value ? commitOf(active.value) : null));
const subjectOf = (id: string) => props.files.history?.commits.find((c) => c.id === id)?.subject ?? id;

/* ---------- colours: asked of the worker once per file, text and theme ---------- */

/** Files larger than this are shown plain: colouring them would keep the worker busy too long. */
const COLOUR_CHARS = 1_000_000;
type Coloured = { file: string; text: string; dark: boolean; result: HighlightResult };
const cache = new Map<string, Coloured>();
const coloured = shallowRef<Coloured | null>(null);

watch(
  [active, text, () => resolvedTheme.value],
  async ([file, body, theme]) => {
    const dark = theme === "dark";
    if (!file || view.value?.kind !== "text") return;
    const hit = cache.get(file);
    if (hit && hit.text === body && hit.dark === dark) {
      coloured.value = hit;
      return;
    }
    if (coloured.value?.file !== file) coloured.value = null;
    const lang = languageOf(file, body.slice(0, body.indexOf("\n") >>> 0));
    const result: HighlightResult = lang && body.length <= COLOUR_CHARS ? await highlight(body, lang, dark) : { id: 0, name: null, palette: [], lines: null };
    if (active.value !== file || text.value !== body) return;
    const entry = { file, text: body, dark, result };
    cache.set(file, entry);
    coloured.value = entry;
  },
  { immediate: true },
);

watch(
  () => [...props.files.tabs],
  (tabs) => {
    for (const file of cache.keys()) if (!tabs.includes(file)) cache.delete(file);
  },
);

const palette = computed(() => (coloured.value?.file === active.value ? coloured.value.result.palette : []));
const tokens = computed(() => (coloured.value?.file === active.value ? coloured.value.result.lines : null));
const languageName = computed(() => {
  if (coloured.value?.file === active.value && coloured.value.result.name) return coloured.value.result.name;
  return active.value && languageOf(active.value) ? languageOf(active.value)! : t("files.plain");
});

/* ---------- File | Diff, and the picked line, per file ---------- */

const lines = ref(new Map<string, number>());
/** Kept with the project (lib/explorer: `diffOn`), so Source control can open a file as its changes. */
const mode = computed(() => (active.value && props.files.diffOn.has(active.value) ? "diff" : "file"));
const current = computed(() => (active.value ? (lines.value.get(active.value) ?? null) : null));
const code = ref<InstanceType<typeof CodeView> | null>(null);
/** The viewer's own box: where the keyboard goes back to when the view that had it goes. */
const viewerRoot = ref<HTMLElement | null>(null);

const diffLines = computed<DiffLine[] | null>(() => {
  if (mark.value === "U") return text.value.split("\n").map((line, i) => ({ kind: "add", text: line, old: null, new: i + 1 }));
  const diff = opened.value?.diff;
  return diff && diff.lines.length ? diff.lines : null;
});
const counts = computed(() => {
  if (mark.value === "U") return { added: lineCount.value, removed: 0 };
  return { added: opened.value?.diff?.added ?? 0, removed: opened.value?.diff?.removed ?? 0 };
});
const gutter = computed(() => (active.value ? gutterOf(props.files, active.value, lineCount.value) : new Map()));

function setMode(next: "file" | "diff"): void {
  if (!active.value) return;
  if (next === "diff") props.files.diffOn.add(active.value);
  else props.files.diffOn.delete(active.value);
}

function pickLine(line: number): void {
  if (active.value) lines.value.set(active.value, line);
}

/** From the diff: the file at that line. */
function showLine(line: number): void {
  pickLine(line);
  setMode("file");
  setTimeout(() => code.value?.goTo(line));
}

/* ---------- the eye: a Markdown file drawn, an SVG as its picture ---------- */

/** What the eye can show of the shown file: `markdown` or `svg`; nothing for any other file. */
const previewKind = computed<"markdown" | "svg" | null>(() => {
  if (!active.value || commit.value) return null;
  if (/\.svg$/i.test(active.value)) return "svg";
  return languageOf(active.value) === "markdown" ? "markdown" : null;
});
/** Pressed, it stays pressed for every such file until it is pressed again: Pitwall's setting
 * (`filesPreview`), taken at once here until the core's answer comes back. */
const previewPicked = ref<boolean | null>(null);
watch(
  () => snapshot.value?.settings.filesPreview,
  () => (previewPicked.value = null),
);
const previewOn = computed(() => previewPicked.value ?? snapshot.value?.settings.filesPreview === true);
/** A Markdown file longer than this is shown as its text: the preview is drawn on the page's own
 * thread (measured in a browser: 500 KB of Markdown 0.2 s, 1 MB 0.46 s, 5 MB 2.6 s). */
const PREVIEW_CHARS = 512 * 1024;
const previewTooLarge = computed(() => previewKind.value === "markdown" && text.value.length > PREVIEW_CHARS);
/** The preview in place of the text: the eye pressed, the file read as text, not its changes. */
const previewing = computed(() => !!previewKind.value && !previewTooLarge.value && previewOn.value && mode.value === "file" && view.value?.kind === "text");

function togglePreview(): void {
  if (!previewKind.value || previewTooLarge.value) return;
  const root = viewerRoot.value;
  const hadIt = !!root && root.contains(document.activeElement);
  const next = !previewing.value;
  if (next) setMode("file");
  previewPicked.value = next;
  void api.setFilesPreview(next);
  // The view that had the keyboard is gone: the new one takes it, so ⇧⌘V, ⌘F and ⌘P still reach
  // the window (the keys fell to the page's body before).
  // Only when the viewer had it (or it fell to the page): a tree row keeps it.
  void nextTick(() => {
    if (!root || root.contains(document.activeElement)) return;
    if (!hadIt && document.activeElement && document.activeElement !== document.body) return;
    (root.querySelector<HTMLElement>(".markdown-view, .scroller") ?? root).focus({ preventScroll: true });
  });
}

/* ---------- a picture of the project's: drawn at once, no eye ---------- */

/** The pictures drawn as themselves (an SVG is text, drawn under the eye). */
const PICTURE = /\.(png|jpe?g|gif|webp|avif|bmp|ico)$/i;
/** A picture read (not too large to show): drawn in place of the binary file's state. */
const pictureShown = computed(() => !!active.value && !commit.value && PICTURE.test(active.value) && !!view.value && view.value.kind !== "large");
const pictureType = computed(() => {
  const ext = active.value?.slice(active.value.lastIndexOf(".") + 1).toUpperCase() ?? "";
  return ext === "JPG" ? "JPEG" : ext;
});
/** What the shown picture told of itself: its pixels and the scale it is drawn at. */
const picture = ref<PictureInfo | null>(null);
const pictureNow = computed(() => (picture.value?.file === active.value ? picture.value : null));
const percent = (zoom: number) => new Intl.NumberFormat(language.value, { style: "percent", maximumFractionDigits: 0 }).format(zoom);

/* ---------- the bar's actions ---------- */

function openInEditor(): void {
  if (active.value) void api.openReference(props.files.path, active.value, current.value ?? 1, null, true);
}

function copyPath(): void {
  if (active.value) void api.copyText(absolutePath(props.files.path, active.value));
}

/* ---------- tabs ---------- */

const name = (file: string) => {
  const id = commitOf(file);
  return id ? id.slice(0, 7) : file.slice(file.lastIndexOf("/") + 1);
};
const folder = (file: string) => (file.includes("/") ? file.slice(0, file.lastIndexOf("/")) : "");
/** Two tabs of one name: each says its folder. */
const twins = computed(() => {
  const seen = new Map<string, number>();
  for (const file of props.files.tabs) seen.set(name(file), (seen.get(name(file)) ?? 0) + 1);
  return seen;
});
const crumbs = computed(() => (commit.value ? [t("scm.history"), commit.value.slice(0, 7)] : active.value ? active.value.split("/") : []));
const tone = (file: string) => ({ M: "m-mod", R: "m-mod", D: "m-mod", A: "m-new", U: "m-new", "!": "m-bad", I: "m-ign" })[markOf(props.files, file) ?? "I"] ?? "";

function onTabKey(event: KeyboardEvent, file: string): void {
  const at = props.files.tabs.indexOf(file);
  if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
    const next = props.files.tabs[at + (event.key === "ArrowRight" ? 1 : -1)];
    if (next) {
      props.files.active = next;
      (event.currentTarget as HTMLElement).closest('[role="tablist"]')?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(next)}"]`)?.focus();
    }
  } else if (event.key === "Delete" || event.key === "Backspace") closeTab(props.files, file);
  else return;
  event.preventDefault();
}

/** Where the clicks of a double click began: the tab, and whether on its ×. A × clicked twice
 * closes its tab at the first click, and the second lands on the tab that slid under the
 * pointer: neither shows that one nor, at the double click, keeps it. */
let pressed: { file: string; x: boolean } | null = null;

function onTabDown(event: MouseEvent, file: string): void {
  if (event.detail <= 1) pressed = { file, x: !!(event.target as HTMLElement).closest(".x") };
}

const ownPress = (event: MouseEvent, file: string) => event.detail <= 1 || (pressed?.file === file && !pressed.x);

/** A tab's own lines at the top of its menu, as VS Code's: close it, the others, those to its
 * left or right, or all. */
function tabMenu(file: string): MenuEntry[] {
  const tabs = props.files.tabs;
  const at = tabs.indexOf(file);
  return [
    { text: t("common.close"), action: () => closeTab(props.files, file) },
    { text: t("files.closeOthers"), enabled: tabs.length > 1, action: () => closeTabs(props.files, file, "others") },
    { text: t("files.closeLeft"), enabled: at > 0, action: () => closeTabs(props.files, file, "left") },
    { text: t("files.closeRight"), enabled: at < tabs.length - 1, action: () => closeTabs(props.files, file, "right") },
    "separator",
    { text: t("files.closeAll"), action: () => closeTabs(props.files, file, "all") },
  ];
}

/* ---------- the tab strip: the wheel and two arrows move it along; the shown tab stays in view ---------- */

const strip = ref<HTMLElement | null>(null);
const canLeft = ref(false);
const canRight = ref(false);

function measureTabs(): void {
  const el = strip.value;
  if (!el) return;
  canLeft.value = el.scrollLeft > 1;
  canRight.value = el.scrollLeft + el.clientWidth < el.scrollWidth - 1;
}

/** A mouse wheel turned up or down moves the strip sideways. */
function onTabsWheel(event: WheelEvent): void {
  const el = strip.value;
  if (!el || Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
  el.scrollLeft += event.deltaY;
  event.preventDefault();
}

function scrollTabs(direction: -1 | 1): void {
  const el = strip.value;
  const smooth = !matchMedia("(prefers-reduced-motion: reduce)").matches;
  el?.scrollBy({ left: direction * Math.max(120, el.clientWidth * 0.6), behavior: smooth ? "smooth" : "auto" });
}

watch(
  () => [props.files.tabs.length, props.files.active],
  () =>
    nextTick(() => {
      strip.value?.querySelector<HTMLElement>(".tab.on")?.scrollIntoView({ block: "nearest", inline: "nearest" });
      measureTabs();
    }),
);

let stripSize: ResizeObserver | null = null;
onMounted(() => {
  stripSize = new ResizeObserver(measureTabs);
  if (strip.value) stripSize.observe(strip.value);
  measureTabs();
});
onBeforeUnmount(() => stripSize?.disconnect());

/** A link in the Markdown preview double clicked: by its second click the file it opened is shown
 * under the pointer, and the double click keeps that file in a tab of its own. */
const linkClicks = handOver(() => props.files);

defineExpose({ openFind: () => code.value?.openFind(), togglePreview });
</script>

<template>
  <div ref="viewerRoot" class="viewer" tabindex="-1" @mousedown.capture="linkClicks.down" @click.capture="linkClicks.click" @dblclick.capture="linkClicks.double">
    <div class="tabs-wrap">
      <div ref="strip" class="tabs" role="tablist" :aria-label="t('files.tabs')" @scroll.passive="measureTabs" @wheel="onTabsWheel">
        <div
          v-for="file in files.tabs"
          :key="file"
          :class="['tab', { on: file === active }]"
          :title="commitOf(file) ? subjectOf(commitOf(file)!) : file"
          @mousedown.left="onTabDown($event, file)"
          @click="ownPress($event, file) && (files.active = file)"
          @dblclick="ownPress($event, file) && keepTab(files, file)"
          @mousedown.middle.prevent="closeTab(files, file)"
          @contextmenu.prevent="emit('menu', $event, file, tabMenu(file))"
        >
          <div role="tab" class="tab-label" :data-tab="file" :tabindex="file === active ? 0 : -1" :aria-selected="file === active" @keydown="onTabKey($event, file)">
            <Icon v-if="commitOf(file)" name="commit" :size="13" class="ico" />
            <FileIcon v-else :path="file" class="fico" />
            <span :class="['tab-name', commitOf(file) ? '' : tone(file), { preview: file === files.preview }]">{{ name(file) }}</span>
            <span v-if="(twins.get(name(file)) ?? 0) > 1" class="tab-dir">{{ folder(file) }}</span>
          </div>
          <button type="button" class="x" tabindex="-1" :aria-label="t('files.closeTab', { name: name(file) })" @click.stop="closeTab(files, file)"><Icon name="close" :size="11" /></button>
        </div>
        <span class="tabs-rest"></span>
      </div>
      <!-- Where tabs go on past an edge: an arrow on the strip's own ground fading into the tabs. -->
      <button v-if="canLeft" type="button" class="tab-arrow left" tabindex="-1" :aria-label="t('files.scrollLeft')" :title="t('files.scrollLeft')" @click="scrollTabs(-1)"><Icon name="back" :size="13" /></button>
      <button v-if="canRight" type="button" class="tab-arrow right" tabindex="-1" :aria-label="t('files.scrollRight')" :title="t('files.scrollRight')" @click="scrollTabs(1)"><Icon name="chevron" :size="13" /></button>
    </div>

    <template v-if="active">
      <div class="crumbs">
        <nav :aria-label="t('files.path')" class="trail">
          <template v-for="(part, i) in crumbs" :key="i">
            <Icon v-if="i" name="chevron" :size="10" class="sep" />
            <span v-if="commit && i < crumbs.length - 1" class="crumb">{{ part }}</span>
            <button v-else-if="i < crumbs.length - 1" type="button" class="crumb" @click="revealInTree(files, crumbs.slice(0, i + 1).join('/'))">{{ part }}</button>
            <span v-else :class="['crumb', 'last', commit ? '' : tone(active)]">{{ part }}</span>
          </template>
        </nav>
        <!-- A Markdown or SVG file: its preview in place of its text, kept for the next ones. -->
        <button
          v-if="previewKind"
          type="button"
          :class="['icon', 'eye', { on: previewing, off: previewTooLarge }]"
          :aria-pressed="previewing"
          :aria-disabled="previewTooLarge"
          :aria-label="t('files.preview')"
          :title="previewTooLarge ? t('files.previewTooLarge', { size: sizeText(PREVIEW_CHARS) }) : `${t('files.preview')} (${isMac ? '⇧⌘V' : 'Ctrl+Shift+V'})`"
          @click="togglePreview"
        >
          <Icon name="eye" :size="14" />
        </button>
      </div>

      <CommitView v-if="commit" :key="commit" :files="files" :id="commit" />
      <div v-else-if="view === undefined" class="state"><Spinner /></div>
      <!-- A file gone from the folder (deleted, in Source control): its changes, if Git has them. -->
      <DiffView v-else-if="view === null && diffLines" :lines="diffLines" :palette="[]" :tokens="null" />
      <div v-else-if="view === null" class="state">{{ t("files.unreadable") }}</div>
      <!-- A picture: itself, at once; what it is and its buttons in the status bar. -->
      <ImageView v-else-if="pictureShown" :key="active" :files="files" :file="active" @info="picture = $event" />
      <div v-else-if="view.kind !== 'text'" class="state big">
        <Icon name="file" :size="34" />
        <span class="state-name">{{ name(active) }}</span>
        <span class="state-sub">{{ t(view.kind === "binary" ? "files.binary" : "files.large", { size: sizeText(view.size) }) }}</span>
        <span class="state-actions">
          <button type="button" class="control" @click="openInEditor"><Icon name="editor" :size="13" />{{ t("common.openInEditor") }}</button>
          <button type="button" class="control" @click="api.explorerReveal(files.path, active)">{{ revealLabel }}</button>
        </span>
      </div>
      <MarkdownView v-else-if="previewing && previewKind === 'markdown'" :key="active" :files="files" :file="active" :text="text" :current="current" @line="pickLine" @opened="linkClicks.arm" />
      <ImageView v-else-if="previewing && previewKind === 'svg'" :key="active" :files="files" :file="active" />
      <DiffView v-else-if="mode === 'diff' && diffLines" :lines="diffLines" :palette="palette" :tokens="tokens" @line="showLine" />
      <CodeView v-else ref="code" :text="text" :palette="palette" :tokens="tokens" :gutter="gutter" :current="current" @line="pickLine" />

      <div class="status">
        <span class="item"><Icon name="lock" :size="11" />{{ t("files.readOnly") }}</span>
        <template v-if="pictureShown && view">
          <span class="item">{{ pictureType }}</span>
          <span v-if="pictureNow" class="item">{{ t("files.pixels", { width: pictureNow.width, height: pictureNow.height }) }}</span>
          <span class="item">{{ sizeText(view.size) }}</span>
          <span v-if="pictureNow?.zoomable" class="item">{{ percent(pictureNow.zoom) }}</span>
        </template>
        <template v-else-if="view?.kind === 'text'">
          <span class="item">{{ languageName }}</span>
          <span class="item">{{ view.lossy ? t("files.notUtf8") : "UTF-8" }}</span>
          <span class="item">{{ t("files.lines", { count: lineCount }) }}</span>
          <!-- What Git changed in the file: pressed, its changes in place of the file; again, the file. -->
          <button
            v-if="diffLines"
            type="button"
            :class="['diff', { on: mode === 'diff' }]"
            :aria-pressed="mode === 'diff'"
            :aria-label="t('files.view.diffCount', { added: counts.added, removed: counts.removed })"
            @click="setMode(mode === 'diff' ? 'file' : 'diff')"
          >
            <Icon name="diff" :size="12" />{{ t("files.view.diff") }}<span class="count"><span class="plus">+{{ counts.added }}</span> <span class="minus">−{{ counts.removed }}</span></span>
          </button>
        </template>
        <span v-if="commit" class="item mono">{{ commit.slice(0, 12) }}</span>
        <span class="grow"></span>
        <template v-if="!commit">
          <button type="button" class="open" @click="openInEditor"><Icon name="editor" :size="12" />{{ current ? t("files.openAt", { line: current }) : t("common.openInEditor") }}</button>
          <button type="button" class="icon" :aria-label="revealLabel" :title="revealLabel" @click="api.explorerReveal(files.path, active)"><Icon name="folder" :size="13" /></button>
          <button type="button" class="icon" :aria-label="t('files.copyPath')" :title="t('files.copyPath')" @click="copyPath"><Icon name="copy" :size="13" /></button>
        </template>
      </div>
    </template>
    <!-- No tab open: what to do, in the middle of the room the file takes. -->
    <div v-else class="state big placeholder">
      <Icon name="files" :size="34" class="placeholder-ico" />
      <span class="state-sub">{{ t("files.pick") }}</span>
    </div>
  </div>
</template>

<style scoped src="../detail/detail.css"></style>
<style scoped>
.viewer {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-log);
  outline: none;
}

.tabs {
  height: 34px;
  flex-shrink: 0;
  display: flex;
  overflow-x: auto;
  overflow-y: hidden;
  background: var(--bg-code-tabs);
  scrollbar-width: none;
}

.tabs-wrap {
  position: relative;
  flex-shrink: 0;
}

.tabs::-webkit-scrollbar {
  display: none;
}

.tab-arrow {
  position: absolute;
  top: 0;
  bottom: 1px;
  z-index: 1;
  width: 44px;
  display: flex;
  align-items: center;
  padding: 0 8px;
  border: 0;
  color: var(--text-muted);
}

.tab-arrow:hover {
  color: var(--text-strong);
}

.tab-arrow.left {
  left: 0;
  justify-content: flex-start;
  background: linear-gradient(to right, var(--bg-code-tabs) 45%, transparent);
}

.tab-arrow.right {
  right: 0;
  justify-content: flex-end;
  background: linear-gradient(to left, var(--bg-code-tabs) 45%, transparent);
}

.tab {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  max-width: 220px;
  border-right: 1px solid var(--line-soft);
  border-bottom: 1px solid var(--line-soft);
  cursor: pointer;
}

/* The shown tab and the path bar under it are one lighter band over the code. */
.tab.on {
  background: var(--bg-code-head);
  border-bottom-color: transparent;
}

.tab-label {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  height: 100%;
  padding: 0 4px 0 12px;
  font-size: 12.5px;
  color: var(--text-muted);
}

.tab.on .tab-label {
  color: var(--text-strong);
}

.tab-label:focus-visible {
  outline-offset: -2px;
}

.ico {
  flex-shrink: 0;
  color: var(--text-subtle);
}

/* A file's own icon (FileIcon, 16 px), its ink where the commit glyph's is. */
.fico {
  margin: 0 -1px 0 -2px;
}

.tab-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The preview tab, VS Code's: the next file a click opens takes its place. */
.tab-name.preview {
  font-style: italic;
}

.tab-dir {
  flex-shrink: 0;
  font-size: 11.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.x {
  width: 20px;
  height: 20px;
  margin-right: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--text-subtle);
  opacity: 0;
}

.tab.on .x,
.tab:hover .x {
  opacity: 1;
}

.x:hover {
  background: var(--bg-hover);
}

.tabs-rest {
  flex: 1;
  border-bottom: 1px solid var(--line-soft);
}

.crumbs {
  height: 30px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 10px;
  background: var(--bg-code-head);
  border-bottom: 1px solid var(--line-soft);
}

.crumbs .eye {
  width: 24px;
  height: 22px;
  flex-shrink: 0;
}

.crumbs .eye.on {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

/* Too large to preview: dimmed, its tooltip says why. */
.crumbs .eye.off,
.crumbs .eye.off:hover {
  background: transparent;
  color: var(--text-muted);
  opacity: 0.4;
}

.trail {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  overflow: hidden;
  white-space: nowrap;
}

.sep {
  flex-shrink: 0;
  color: var(--text-faint);
}

.crumb {
  height: 22px;
  padding: 0 4px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font: inherit;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

button.crumb:hover {
  background: var(--bg-row-hover);
  color: var(--text-soft);
}

.crumb.last {
  display: inline-flex;
  align-items: center;
  color: var(--text-soft);
  cursor: default;
}

.state {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12.5px;
  color: var(--text-faint);
}

.state.big {
  flex-direction: column;
  gap: 8px;
  color: var(--text-subtle);
}

.state-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}

.state-sub {
  font-size: 12px;
}

.placeholder {
  padding: 0 24px;
  text-align: center;
}

.placeholder-ico {
  color: var(--text-faint);
}

.placeholder .state-sub {
  font-size: 12.5px;
}

.state-actions {
  display: flex;
  gap: 6px;
  margin-top: 6px;
}

.status {
  height: 28px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 6px 0 12px;
  border-top: 1px solid var(--line-soft);
  background: var(--bg-files-bar);
  font-size: 11.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.status .item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.status .mono {
  font-family: var(--font-mono);
}

.status .grow {
  flex: 1;
}

/* The changes' switch, among what the bar says of the file: plain until pressed. */
.status .diff {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 20px;
  margin-left: -4px;
  padding: 0 7px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font: inherit;
  font-size: 11.5px;
  color: var(--text-soft);
}

.status .diff:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.status .diff.on {
  background: var(--bg-control-on);
  box-shadow: inset 0 0 0 1px var(--line-selected);
  color: var(--text-strong);
}

.status .count {
  font-family: var(--font-mono);
  font-size: 10.5px;
}

.status .plus {
  color: var(--run-text);
}

.status .minus {
  color: var(--crash-text);
}

/* A plain button, as the icons beside it: no frame, a ground only under the pointer. */
.status .open {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 22px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font: inherit;
  font-size: 11.5px;
  color: var(--text-soft);
}

.status .open:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.status .icon {
  width: 24px;
  height: 22px;
  margin-left: -10px;
}

.m-mod {
  color: var(--warn-text);
}

.m-new {
  color: var(--run-text);
}

.m-bad {
  color: var(--crash-text);
}
</style>
