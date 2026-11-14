<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import FileIcon from "./FileIcon.vue";
import {
  folderMarkOf,
  handOver,
  loadFolder,
  loadIndex,
  markOf,
  openAt,
  openFile,
  revealInTree,
  toggleFolder,
  type Found,
  type FoundLine,
  type Mark,
  type ProjectFiles,
  type SearchOptions,
} from "../../lib/explorer";
import { fuzzy, segments } from "../../lib/fuzzy";
import { t } from "../../lib/i18n";
import { isMac, keys } from "../../lib/platform";
import { api } from "../../lib/store";

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

/* ---------- the search (⌘P, ⇧⌘F): names of files and folders, fuzzy, then the files' text ---------- */

const query = ref("");
const search = ref<HTMLInputElement | null>(null);
const box = ref<HTMLElement | null>(null);
/** The row the keyboard is on, among those that can be picked. */
const picked = ref(0);
/** Names shown before "N more": the text's lines come under them. */
const NAMES = 8;
const allNames = ref(false);
const namesFolded = ref(false);
const textFolded = ref(false);
/** The text's files folded to their row. */
const folded = ref(new Set<string>());

type NameHit = { path: string; name: string; dir: string; hits: Set<number>; mark: Mark | null; folder: boolean };

const nameHits = computed<NameHit[]>(() => {
  const q = query.value.trim();
  const index = props.files.index;
  if (!q || !index || index === "loading") return [];
  const found: Array<NameHit & { score: number }> = [];
  const look = (path: string, folder: boolean): void => {
    const cut = path.lastIndexOf("/");
    const name = path.slice(cut + 1);
    const byName = fuzzy(name, q);
    const match = byName ?? fuzzy(path, q);
    if (!match) return;
    // A name's hits are counted from the name's start, a path's from the path's.
    const hits = byName ? new Set(match.hits) : new Set(match.hits.map((hit) => hit - (cut + 1)).filter((hit) => hit >= 0));
    const mark = folder ? folderMarkOf(props.files, path) : markOf(props.files, path);
    found.push({ path, name, dir: cut < 0 ? "" : path.slice(0, cut), hits, mark, folder, score: match.score + (byName ? 100 : 0) });
  };
  for (const path of index.files) look(path, false);
  for (const path of index.dirs) look(path, true);
  return found.sort((a, b) => b.score - a.score || a.path.length - b.path.length).slice(0, 60);
});

/* The files' text, searched by the core (core/explorer.rs: `explorer_grep`) a moment after the
   last key: the latest answer stays shown while the next one is out. */

const found = shallowRef<Found | null>(null);
const searching = ref(false);
let asked = 0;
let timer: number | undefined;
const WAIT = 220;

function searchText(): void {
  clearTimeout(timer);
  const text = query.value;
  const options = { ...props.files.searchOptions };
  const turn = ++asked;
  if (!text.trim()) {
    found.value = null;
    searching.value = false;
    // An empty search ends the one still going.
    void api.explorerGrep(props.files.path, { text: "", ...options }).catch(() => null);
    return;
  }
  searching.value = true;
  timer = window.setTimeout(async () => {
    const answer = await api.explorerGrep(props.files.path, { text, ...options }).catch(() => null);
    if (turn !== asked) return;
    searching.value = false;
    found.value = answer ?? { files: [], hits: 0, partial: false, bad: false };
  }, WAIT);
}

watch([query, () => props.files.searchOptions.case, () => props.files.searchOptions.word, () => props.files.searchOptions.regex], searchText);
onBeforeUnmount(() => clearTimeout(timer));

type Result =
  | { kind: "head"; key: string; section: "names" | "text"; count: string; open: boolean }
  | { kind: "name"; key: string; at: number; hit: NameHit }
  | { kind: "more"; key: string; count: number }
  | { kind: "file"; key: string; at: number; path: string; name: string; dir: string; count: number; open: boolean; mark: Mark | null }
  | { kind: "line"; key: string; at: number; path: string; line: FoundLine }
  | { kind: "note"; key: string; text: string; tone: "" | "warn" | "bad"; busy: boolean };
type Pickable = Extract<Result, { at: number }>;

/** The rows under the box: the names (folders among them), then the text's files and lines. */
const results = computed<Result[]>(() => {
  const out: Result[] = [];
  const names = nameHits.value;
  const answer = found.value;
  let at = 0;
  if (props.files.index === "loading" && !names.length) out.push({ kind: "note", key: "loading", text: t("files.loading"), tone: "", busy: true });
  if (names.length) {
    out.push({ kind: "head", key: "head:names", section: "names", count: String(names.length), open: !namesFolded.value });
    if (!namesFolded.value) {
      for (const hit of allNames.value ? names : names.slice(0, NAMES)) out.push({ kind: "name", key: `name:${hit.path}`, at: at++, hit });
      if (!allNames.value && names.length > NAMES) out.push({ kind: "more", key: "names:more", count: names.length - NAMES });
    }
  }
  const nothing = !names.length && props.files.index !== "loading" && !searching.value && !!answer && !answer.bad && !answer.files.length;
  if (nothing) {
    out.push({ kind: "note", key: "none", text: t("files.noMatch"), tone: "", busy: false });
    return out;
  }
  const count = answer && !answer.bad ? `${t("files.found.hits", { count: answer.hits })} · ${t("files.found.files", { count: answer.files.length })}` : "";
  out.push({ kind: "head", key: "head:text", section: "text", count, open: !textFolded.value });
  if (textFolded.value) return out;
  if (answer?.bad) out.push({ kind: "note", key: "bad", text: t("files.found.bad"), tone: "bad", busy: false });
  else if (!answer) out.push({ kind: "note", key: "searching", text: t("files.found.searching"), tone: "", busy: true });
  else {
    if (answer.partial) out.push({ kind: "note", key: "partial", text: t("files.found.partial", { count: answer.hits }), tone: "warn", busy: false });
    for (const file of answer.files) {
      const cut = file.path.lastIndexOf("/");
      const open = !folded.value.has(file.path);
      const count = file.lines.reduce((sum, line) => sum + line.ranges.length, 0);
      out.push({ kind: "file", key: `file:${file.path}`, at: at++, path: file.path, name: file.path.slice(cut + 1), dir: cut < 0 ? "" : file.path.slice(0, cut), count, open, mark: markOf(props.files, file.path) });
      if (open) for (const line of file.lines) out.push({ kind: "line", key: `line:${file.path}:${line.line}`, at: at++, path: file.path, line });
    }
  }
  return out;
});

const pickable = computed(() => results.value.filter((row): row is Pickable => "at" in row));

watch(query, () => {
  picked.value = 0;
  allNames.value = false;
});
watch(found, () => (folded.value = new Set()));

/** A found line's text with its matches marked, `…` before it when its start was left out. */
function pieces(line: FoundLine): Array<{ text: string; hit: boolean }> {
  const out: Array<{ text: string; hit: boolean }> = line.cut ? [{ text: "…", hit: false }] : [];
  let at = 0;
  for (const [from, to] of line.ranges) {
    if (from > at) out.push({ text: line.text.slice(at, from), hit: false });
    out.push({ text: line.text.slice(from, to), hit: true });
    at = to;
  }
  if (at < line.text.length) out.push({ text: line.text.slice(at), hit: false });
  return out;
}

function onSearchFocus(): void {
  void loadIndex(props.files);
}

const OPTION_KEYS: Record<string, keyof SearchOptions> = { KeyC: "case", KeyW: "word", KeyR: "regex" };
/** VS Code's keys for the switches: ⌥⌘C, ⌥⌘W, ⌥⌘R on a Mac, Alt+C, Alt+W, Alt+R elsewhere. */
const optionKeys = (letter: string) => keys(isMac ? `alt+mod+${letter}` : `alt+${letter}`);

function toggleOption(option: keyof SearchOptions): void {
  props.files.searchOptions[option] = !props.files.searchOptions[option];
}

function move(by: number): void {
  const count = pickable.value.length;
  if (!count) return;
  picked.value = Math.min(Math.max(picked.value + by, 0), count - 1);
  void nextTick(keepInView);
}

/** The picked row in the list's view, scrolled no further than it takes (never the window). */
function keepInView(): void {
  const list = box.value;
  const row = list?.querySelector<HTMLElement>(".on");
  if (!list || !row) return;
  if (row.offsetTop < list.scrollTop) list.scrollTop = row.offsetTop - 4;
  else if (row.offsetTop + row.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = row.offsetTop + row.offsetHeight - list.clientHeight + 4;
}

function onSearchKey(event: KeyboardEvent): void {
  const option = OPTION_KEYS[event.code];
  if (option && event.altKey && (isMac ? event.metaKey : !event.ctrlKey && !event.metaKey)) toggleOption(option);
  else if (event.key === "ArrowDown") move(1);
  else if (event.key === "ArrowUp") move(-1);
  else if (event.key === "Enter" && pickable.value[picked.value]) pick(pickable.value[picked.value]);
  else if (event.key === "Escape" && query.value) query.value = "";
  else return;
  event.preventDefault();
}

function pick(row: Pickable): void {
  if (row.kind === "name") {
    if (row.hit.folder) showFolder(row.hit.path);
    else choose(row.hit.path);
  } else if (row.kind === "file") toggleFile(row.path);
  else openAt(props.files, row.path, row.line.line, query.value);
}

function choose(path: string): void {
  // Found by the search: shown in the tree too, its folders opened, as VS Code reveals it.
  revealInTree(props.files, path);
  openFile(props.files, path);
  query.value = "";
}

/** A folder found by its name: opened in the tree, which takes the keyboard on its row. */
function showFolder(dir: string): void {
  folderAt = performance.now();
  revealInTree(props.files, dir);
  toggleFolder(props.files, dir, true);
  query.value = "";
  focusRow(dir);
}

function toggleFile(path: string): void {
  const next = new Set(folded.value);
  if (!next.delete(path)) next.add(path);
  folded.value = next;
}

/** A hit clicked: the tree is under the pointer by the second click of a double click, which keeps
 * the hit's file in a tab of its own rather than doing anything to the row there. A found line
 * stays under it, and its double click keeps the file the same way. */
const hitClicks = handOver(() => props.files);
/** When a folder was picked: the second click of that double click lands on the tree's row. */
let folderAt = 0;
const swallowed = (event: MouseEvent) => event.detail >= 2 && performance.now() - folderAt < 700;

function onPaneDown(event: MouseEvent): void {
  if (swallowed(event)) event.preventDefault();
  else hitClicks.down(event);
}

function onPaneClick(event: MouseEvent): void {
  if (swallowed(event)) event.stopPropagation();
  else hitClicks.click(event);
}

function onPaneDouble(event: MouseEvent): void {
  if (swallowed(event)) event.stopPropagation();
  else hitClicks.double(event);
}

function clickResult(row: Pickable): void {
  picked.value = row.at;
  pick(row);
  if (row.kind === "name" && !row.hit.folder) hitClicks.arm(row.hit.path);
  else if (row.kind === "line") hitClicks.arm(row.path);
}

/** ⌘P or ⇧⌘F from the window: the search takes the keyboard. */
function focusSearch(): void {
  search.value?.focus();
  search.value?.select();
}

defineExpose({ focusSearch });
</script>

<template>
  <div class="tree-pane" @mousedown.capture="onPaneDown" @click.capture="onPaneClick" @dblclick.capture="onPaneDouble">
    <label class="search">
      <Icon name="search" :size="12" />
      <input
        ref="search"
        v-model="query"
        type="search"
        :placeholder="t('files.search')"
        :aria-label="t('files.search')"
        :title="`${t('files.search')} (${keys('mod+P')})`"
        @focus="onSearchFocus"
        @keydown="onSearchKey"
      />
      <span class="options">
        <button
          v-for="option in (['case', 'word', 'regex'] as const)"
          :key="option"
          type="button"
          :class="['option', option, { on: files.searchOptions[option] }]"
          :aria-pressed="files.searchOptions[option]"
          :aria-label="t(`files.option.${option}`)"
          :title="`${t(`files.option.${option}`)} (${optionKeys({ case: 'C', word: 'W', regex: 'R' }[option])})`"
          @mousedown.prevent
          @click="toggleOption(option)"
        >
          {{ { case: "Aa", word: "ab", regex: ".*" }[option] }}
        </button>
      </span>
    </label>

    <div v-if="query.trim()" ref="box" class="results" role="listbox" :aria-label="t('files.search')">
      <template v-for="row in results" :key="row.key">
        <button
          v-if="row.kind === 'head'"
          type="button"
          tabindex="-1"
          class="group"
          :aria-expanded="row.open"
          @click="row.section === 'names' ? (namesFolded = !namesFolded) : (textFolded = !textFolded)"
        >
          <Icon :name="row.open ? 'chevron-down' : 'chevron'" :size="12" class="twisty" />
          <span class="group-name">{{ t(row.section === "names" ? "files.found.names" : "files.found.text") }}</span>
          <Spinner v-if="row.section === 'text' && searching && found" />
          <span class="group-count">{{ row.count }}</span>
        </button>
        <div
          v-else-if="row.kind === 'name'"
          role="option"
          :aria-selected="row.at === picked"
          :class="['hit', { on: row.at === picked }]"
          :title="row.hit.path"
          @mousemove="picked = row.at"
          @click="clickResult(row)"
        >
          <Icon v-if="row.hit.folder" name="folder" :size="14" class="ico" />
          <FileIcon v-else :path="row.hit.path" class="fico" />
          <span :class="['name', tone(row.hit.mark)]"><span v-for="(part, k) in segments(row.hit.name, row.hit.hits)" :key="k" :class="{ match: part.hit }">{{ part.text }}</span></span>
          <span class="dir">{{ row.hit.dir }}</span>
          <template v-if="row.hit.mark && row.hit.mark !== 'I'">
            <span v-if="row.hit.folder" :class="['dot', tone(row.hit.mark)]"></span>
            <span v-else :class="['letter', tone(row.hit.mark)]">{{ row.hit.mark }}</span>
          </template>
        </div>
        <div v-else-if="row.kind === 'more'" class="note indent">
          <button type="button" class="more" @click="allNames = true">{{ t("files.more", { count: row.count }) }}</button>
        </div>
        <div
          v-else-if="row.kind === 'file'"
          role="option"
          :aria-selected="row.at === picked"
          :aria-expanded="row.open"
          :class="['hit', 'found-file', { on: row.at === picked }]"
          :title="row.path"
          @mousemove="picked = row.at"
          @click="clickResult(row)"
        >
          <Icon :name="row.open ? 'chevron-down' : 'chevron'" :size="12" class="twisty" />
          <FileIcon :path="row.path" class="fico" />
          <span :class="['name', tone(row.mark)]">{{ row.name }}</span>
          <span class="dir">{{ row.dir }}</span>
          <span class="count">{{ row.count }}</span>
        </div>
        <div
          v-else-if="row.kind === 'line'"
          role="option"
          :aria-selected="row.at === picked"
          :class="['hit', 'found-line', { on: row.at === picked }]"
          @mousemove="picked = row.at"
          @click="clickResult(row)"
        >
          <span class="line-text"><span v-for="(part, k) in pieces(row.line)" :key="k" :class="{ found: part.hit }">{{ part.text }}</span></span>
          <span class="line-no">{{ row.line.line }}</span>
        </div>
        <div v-else-if="row.kind === 'note'" :class="['state', row.tone]"><Spinner v-if="row.busy" /> {{ row.text }}</div>
      </template>
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

/* VS Code's three switches at the box's end: lit while on. */
.options {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}

.option {
  width: 22px;
  height: 20px;
  padding: 0;
  border: 0;
  border-radius: 4px;
  background: transparent;
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 600;
  color: var(--text-subtle);
}

.option:hover {
  background: var(--bg-row-hover);
  color: var(--text-soft);
}

.option.word {
  text-decoration: underline;
  text-underline-offset: 2px;
}

.option.on {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
  color: var(--text-strong);
}

.rows,
.results {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 4px 4px 8px;
}

/* The picked row is kept in view by its offset from here. */
.results {
  position: relative;
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

/* A name found: on one line with its folder after it, as the text's files are. */
.hit {
  padding-left: 8px;
}

.hit .name {
  flex-shrink: 0;
  max-width: 70%;
}

.hit .dir {
  flex: 1;
  margin-left: 6px;
  font-size: 11.5px;
}

.hit .ico {
  margin: 0 5px 0 0;
}

/* The text's files: a row to fold, the count of their matches at its end. */
.found-file {
  padding-left: 2px;
}

.found-file .twisty {
  margin-right: 2px;
}

.count {
  min-width: 18px;
  height: 16px;
  flex-shrink: 0;
  box-sizing: border-box;
  margin-left: 6px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--bg-control-on);
  font-size: 10px;
  line-height: 16px;
  text-align: center;
  color: var(--text);
}

/* A line found: under its file, its number at the end; what matched on the find colour. */
.found-line {
  gap: 8px;
  padding-left: 34px;
}

.line-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: pre;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
  tab-size: 2;
}

.found {
  border-radius: 2px;
  background: var(--find-bg);
  color: var(--text-strong);
}

.line-no {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-faint);
}

/* A part of the results, as Source control's are headed. */
.group {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 4px;
  height: 24px;
  margin-top: 2px;
  padding: 0 8px 0 2px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font: inherit;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-soft);
  text-align: left;
}

.group + .group,
.hit + .group,
.note + .group,
.state + .group {
  margin-top: 8px;
}

.group-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.group-count {
  flex-shrink: 0;
  font-size: 11px;
  font-weight: 400;
  color: var(--text-subtle);
  white-space: nowrap;
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

.state.warn {
  color: var(--warn-text);
}

.state.bad {
  color: var(--crash-text);
}

.note.indent {
  padding-left: 26px;
}
</style>
