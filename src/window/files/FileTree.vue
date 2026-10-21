<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import FileIcon from "./FileIcon.vue";
import { folderMarkOf, handOver, loadFolder, loadIndex, markOf, openFile, revealInTree, toggleFolder, type Mark, type ProjectFiles } from "../../lib/explorer";
import { fuzzy, segments } from "../../lib/fuzzy";
import { t } from "../../lib/i18n";
import { keys } from "../../lib/platform";

const props = defineProps<{ files: ProjectFiles }>();
const emit = defineEmits<{ menu: [event: MouseEvent | { clientX: number; clientY: number }, path: string, dir: boolean] }>();

const STEP = 14;
const BASE = 8;

type Row =
  | { kind: "entry"; path: string; name: string; dir: boolean; link: boolean; depth: number; open: boolean; mark: Mark | null }
  | { kind: "loading" | "empty" | "error"; path: string; depth: number }
  | { kind: "more"; path: string; depth: number; count: number };

/** The tree as rows: open folders' entries under them, each folder's state where it has none. */
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  const walk = (dir: string, depth: number): void => {
    const folder = props.files.folders.get(dir);
    if (!folder || (folder.state === "loading" && !folder.listing)) {
      out.push({ kind: "loading", path: `${dir}/…loading`, depth });
      return;
    }
    if (folder.state === "error" || !folder.listing) {
      out.push({ kind: "error", path: `${dir}/…error`, depth });
      return;
    }
    if (!folder.listing.entries.length) out.push({ kind: "empty", path: `${dir}/…empty`, depth });
    for (const entry of folder.listing.entries) {
      const path = dir ? `${dir}/${entry.name}` : entry.name;
      const open = entry.dir && props.files.expanded.has(path);
      const mark = entry.dir ? folderMarkOf(props.files, path) : markOf(props.files, path);
      out.push({ kind: "entry", path, name: entry.name, dir: entry.dir, link: entry.link, depth, open, mark });
      if (open) walk(path, depth + 1);
    }
    if (folder.listing.more) out.push({ kind: "more", path: `${dir}/…more`, depth, count: folder.listing.more });
  };
  walk("", 0);
  return out;
});

const entries = computed(() => rows.value.filter((row): row is Extract<Row, { kind: "entry" }> => row.kind === "entry"));
const focused = computed(() => props.files.focus ?? entries.value[0]?.path ?? null);
const list = ref<HTMLElement | null>(null);

/** A file in the preview tab; a double click's second click adds nothing, the double click keeps
 * it in a tab of its own (`keepRow`). */
function clickRow(event: MouseEvent, row: Extract<Row, { kind: "entry" }>): void {
  props.files.focus = row.path;
  if (row.dir) toggleFolder(props.files, row.path);
  else if (event.detail < 2) openFile(props.files, row.path);
}

function keepRow(row: Extract<Row, { kind: "entry" }>): void {
  if (!row.dir) openFile(props.files, row.path, true);
}

function focusRow(path: string): void {
  props.files.focus = path;
  void nextTick(() => list.value?.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus());
}

function onKey(event: KeyboardEvent): void {
  const all = entries.value;
  const at = all.findIndex((row) => row.path === focused.value);
  const row = all[at];
  if (!row) return;
  const parent = row.path.includes("/") ? row.path.slice(0, row.path.lastIndexOf("/")) : null;
  switch (event.key) {
    case "ArrowDown":
      if (all[at + 1]) focusRow(all[at + 1].path);
      break;
    case "ArrowUp":
      if (at > 0) focusRow(all[at - 1].path);
      break;
    case "Home":
      focusRow(all[0].path);
      break;
    case "End":
      focusRow(all[all.length - 1].path);
      break;
    case "ArrowRight":
      if (row.dir && !row.open) toggleFolder(props.files, row.path, true);
      else if (row.dir && all[at + 1]?.path.startsWith(`${row.path}/`)) focusRow(all[at + 1].path);
      break;
    case "ArrowLeft":
      if (row.dir && row.open) toggleFolder(props.files, row.path, false);
      else if (parent !== null) focusRow(parent);
      break;
    case "Enter":
    case " ":
      if (row.dir) toggleFolder(props.files, row.path);
      else openFile(props.files, row.path);
      break;
    default:
      return;
  }
  event.preventDefault();
}

function onMenuKey(event: KeyboardEvent, row: Extract<Row, { kind: "entry" }>): void {
  if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
  event.preventDefault();
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  emit("menu", { clientX: box.left + 24, clientY: box.bottom }, row.path, row.dir);
}

const MARK_NAMES: Record<Mark, string> = { M: "files.mark.changed", A: "files.mark.added", R: "files.mark.renamed", D: "files.mark.deleted", U: "files.mark.untracked", "!": "files.mark.conflict", I: "files.mark.ignored" };
const markTitle = (mark: Mark) => t(MARK_NAMES[mark] as Parameters<typeof t>[0]);
const tone = (mark: Mark | null) => (mark ? `m-${{ M: "mod", R: "mod", D: "mod", A: "new", U: "new", "!": "bad", I: "ign" }[mark]}` : "");

/* ---------- the search (⌘P): every file of the project, fuzzy, by name first ---------- */

const query = ref("");
const search = ref<HTMLInputElement | null>(null);
const picked = ref(0);

type Hit = { path: string; name: string; dir: string; hits: Set<number>; mark: Mark | null };

const results = computed<Hit[]>(() => {
  const q = query.value.trim();
  const index = props.files.index;
  if (!q || !index || index === "loading") return [];
  const found: Array<Hit & { score: number }> = [];
  for (const path of index.files) {
    const cut = path.lastIndexOf("/");
    const name = path.slice(cut + 1);
    const byName = fuzzy(name, q);
    const byPath = byName ? null : fuzzy(path, q);
    const match = byName ?? byPath;
    if (!match) continue;
    // A name's hits are counted from the name's start, a path's from the path's.
    const hits = byName ? new Set(match.hits) : new Set(match.hits.map((hit) => hit - (cut + 1)).filter((hit) => hit >= 0));
    found.push({ path, name, dir: cut < 0 ? "" : path.slice(0, cut), hits, mark: markOf(props.files, path), score: match.score + (byName ? 100 : 0) });
  }
  return found.sort((a, b) => b.score - a.score || a.path.length - b.path.length).slice(0, 60);
});

watch(query, () => (picked.value = 0));

function onSearchFocus(): void {
  void loadIndex(props.files);
}

function onSearchKey(event: KeyboardEvent): void {
  if (event.key === "ArrowDown") picked.value = Math.min(picked.value + 1, results.value.length - 1);
  else if (event.key === "ArrowUp") picked.value = Math.max(picked.value - 1, 0);
  else if (event.key === "Enter" && results.value[picked.value]) choose(results.value[picked.value].path);
  else if (event.key === "Escape" && query.value) query.value = "";
  else return;
  event.preventDefault();
}

function choose(path: string): void {
  // Found by the search: shown in the tree too, its folders opened, as VS Code reveals it.
  revealInTree(props.files, path);
  openFile(props.files, path);
  query.value = "";
}

/** A hit clicked: the tree is under the pointer by the second click of a double click, which keeps
 * the hit's file in a tab of its own rather than doing anything to the row there. */
const hitClicks = handOver(() => props.files);

function clickHit(path: string): void {
  choose(path);
  hitClicks.arm(path);
}

/** ⌘P from the window: the search takes the keyboard. */
function focusSearch(): void {
  search.value?.focus();
  search.value?.select();
}

defineExpose({ focusSearch });
</script>

<template>
  <div class="tree-pane" @mousedown.capture="hitClicks.down" @click.capture="hitClicks.click" @dblclick.capture="hitClicks.double">
    <label class="search">
      <Icon name="search" :size="12" />
      <input
        ref="search"
        v-model="query"
        type="search"
        :placeholder="t('files.search')"
        :aria-label="t('files.search')"
        @focus="onSearchFocus"
        @keydown="onSearchKey"
      />
      <span v-if="!query" class="hint">{{ keys("mod+P") }}</span>
    </label>

    <div v-if="query.trim()" class="results" role="listbox" :aria-label="t('files.search')">
      <div v-if="files.index === 'loading'" class="state"><Spinner /> {{ t("files.loading") }}</div>
      <div v-else-if="!results.length" class="state">{{ t("files.noMatch") }}</div>
      <div
        v-for="(hit, i) in results"
        :key="hit.path"
        role="option"
        :aria-selected="i === picked"
        :class="['hit', { on: i === picked }]"
        :title="hit.path"
        @mousemove="picked = i"
        @click="clickHit(hit.path)"
      >
        <FileIcon :path="hit.path" class="fico" />
        <!-- The name, and under it the folder it is in: a long path never pushes the name out. -->
        <span class="hit-text">
          <span :class="['name', tone(hit.mark)]"><span v-for="(part, k) in segments(hit.name, hit.hits)" :key="k" :class="{ match: part.hit }">{{ part.text }}</span></span>
          <span v-if="hit.dir" class="dir">{{ hit.dir }}</span>
        </span>
        <span v-if="hit.mark && hit.mark !== 'I'" :class="['letter', tone(hit.mark)]">{{ hit.mark === "U" ? "U" : hit.mark }}</span>
      </div>
    </div>

    <div v-else ref="list" class="rows" role="tree" :aria-label="t('files.tree')" @keydown="onKey">
      <template v-for="row in rows" :key="row.path">
        <div
          v-if="row.kind === 'entry'"
          role="treeitem"
          :aria-level="row.depth + 1"
          :aria-expanded="row.dir ? row.open : undefined"
          :aria-selected="!row.dir && row.path === files.active"
          :tabindex="row.path === focused ? 0 : -1"
          :data-path="row.path"
          :class="['row', tone(row.mark), { active: !row.dir && row.path === files.active }]"
          :style="{ paddingLeft: `${BASE + row.depth * STEP}px` }"
          @click="clickRow($event, row)"
          @dblclick="keepRow(row)"
          @contextmenu.prevent="emit('menu', $event, row.path, row.dir)"
          @keydown="onMenuKey($event, row)"
          @focus="files.focus = row.path"
        >
          <span v-for="level in row.depth" :key="level" class="guide" :style="{ left: `${BASE + (level - 1) * STEP + 7}px` }" aria-hidden="true"></span>
          <span class="twisty" aria-hidden="true"><Icon v-if="row.dir" :name="row.open ? 'chevron-down' : 'chevron'" :size="12" /></span>
          <Icon v-if="row.dir" :name="row.open ? 'folder-open' : 'folder'" :size="14" class="ico" />
          <FileIcon v-else :path="row.path" class="fico" />
          <span class="name">{{ row.name }}</span>
          <span v-if="row.link" class="link" aria-hidden="true">↗</span>
          <span class="grow"></span>
          <template v-if="row.mark && row.mark !== 'I'">
            <span v-if="row.dir" class="dot" :title="markTitle(row.mark)"></span>
            <span v-else class="letter" :title="markTitle(row.mark)">{{ row.mark }}</span>
          </template>
        </div>
        <div v-else-if="row.kind === 'more'" class="note" :style="{ paddingLeft: `${BASE + row.depth * STEP + 21}px` }">
          <button type="button" class="more" @click="loadFolder(files, row.path.slice(0, row.path.lastIndexOf('/…')), 1_000_000)">{{ t("files.more", { count: row.count }) }}</button>
        </div>
        <div v-else class="note" :style="{ paddingLeft: `${BASE + row.depth * STEP + 21}px` }">
          <template v-if="row.kind === 'loading'"><Spinner /> {{ t("files.loading") }}</template>
          <template v-else-if="row.kind === 'empty'">{{ t("files.empty") }}</template>
          <template v-else>{{ t("files.unreadable") }}</template>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.tree-pane {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--bg-sidebar);
}

/* The box is the band itself: as tall as the tab strip beside it and level with it, no frame of
   its own; a step lighter while it has the keyboard. */
.search {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 34px;
  box-sizing: border-box;
  padding: 0 10px 0 12px;
  border-bottom: 1px solid var(--line-soft);
  background: var(--bg-code-tabs);
  color: var(--text-faint);
  flex-shrink: 0;
  cursor: text;
}

.search:focus-within {
  background: var(--bg-input);
  color: var(--text-subtle);
}

.search input {
  min-width: 0;
  flex: 1;
  border: 0;
  padding: 0;
  background: transparent;
  font: inherit;
  font-size: 12px;
  color: var(--text);
  outline: none;
}

.search .hint {
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-subtle);
  flex-shrink: 0;
}

.rows,
.results {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 4px 4px 8px;
}

.row,
.hit {
  position: relative;
  display: flex;
  align-items: center;
  height: 22px;
  padding-right: 8px;
  border-radius: 5px;
  font-size: 13px;
  color: var(--text-soft);
  cursor: pointer;
  white-space: nowrap;
}

.row:hover,
.hit:hover {
  background: var(--bg-row-hover);
}

/* The global ring (tokens.css), drawn inside: the list clips what goes past its edges. */
.row:focus-visible {
  outline-offset: -2px;
}

.row.active,
.hit.on {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

.guide {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  background: var(--line-soft);
}

.twisty {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-subtle);
}

.ico {
  flex-shrink: 0;
  margin: 0 5px 0 2px;
  color: var(--text-subtle);
}

/* A file's own icon (FileIcon, 16 px): its name where a folder's is. */
.fico {
  margin-right: 5px;
}

.hit .fico {
  margin: 0.5px 4px 0 6px;
}

/* Two lines: the name over its folder. */
.hit {
  height: auto;
  min-height: 22px;
  align-items: flex-start;
  padding-top: 4px;
  padding-bottom: 4px;
}

.hit .letter {
  margin-top: 2px;
}

.hit-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
  line-height: 17px;
}

.hit .dir {
  margin-left: 0;
  font-size: 11.5px;
  line-height: 15px;
}

.name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link {
  margin-left: 6px;
  font-size: 11px;
  color: var(--text-subtle);
}

.grow {
  flex: 1;
}

.dir {
  min-width: 0;
  margin-left: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
  color: var(--text-subtle);
}

.match {
  color: var(--text-strong);
  font-weight: 600;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.letter {
  width: 14px;
  flex-shrink: 0;
  margin-left: 6px;
  text-align: center;
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 500;
}

.dot {
  width: 6px;
  height: 6px;
  margin: 0 4px 0 10px;
  border-radius: 50%;
  flex-shrink: 0;
  background: currentColor;
}

/* Git's state, as VS Code shows it: a changed file amber, a new one green, a conflict red, what
   Git ignores faint. The name takes the colour; its letter or dot says it once more. */
.m-mod {
  color: var(--warn-text);
}

.m-mod .letter,
.m-mod .dot {
  color: var(--warn);
}

.m-new {
  color: var(--run-text);
}

.m-new .letter,
.m-new .dot {
  color: var(--run);
}

.m-bad {
  color: var(--crash-text);
}

.m-ign,
.m-ign .ico {
  color: var(--text-faint);
}

.note {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 22px;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.more {
  height: 22px;
  margin-left: -8px;
  padding: 0 8px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font: inherit;
  font-size: 12px;
  color: var(--text-muted);
}

.more:hover {
  background: var(--bg-row-hover);
}

.state {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  font-size: 12px;
  color: var(--text-faint);
}
</style>
