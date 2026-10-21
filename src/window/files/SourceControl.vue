<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { changesOf, loadHistory, openCommit, openDiff, refreshFiles, type Commit, type ProjectFiles } from "../../lib/explorer";
import { layout, type GraphRow } from "../../lib/gitGraph";
import { language, t } from "../../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../../lib/nativeMenu";
import { api, now } from "../../lib/store";
import type { Project } from "../../lib/types";
import FileIcon from "./FileIcon.vue";

/**
 * The Files window's Source control tab, as VS Code's: what Git sees changed (a click shows the
 * change) over the history of the branch and its upstream drawn as a graph (a click opens the
 * commit), each scrolled on its own with a line between them that moves, and the branch along
 * the foot. Read only: nothing here writes to the repository.
 */
const props = defineProps<{ files: ProjectFiles; project: Project }>();
const emit = defineEmits<{ menu: [event: MouseEvent, path: string] }>();
/** The share of the height Changes takes, History the rest: the window's own. */
const split = defineModel<number>("split", { required: true });

onMounted(() => {
  if (!props.files.history) void loadHistory(props.files);
});

const changes = computed(() => changesOf(props.files));
const changesOpen = ref(true);
const historyOpen = ref(true);

const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const folder = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
const TONE: Record<string, string> = { M: "m-mod", R: "m-mod", D: "m-del", A: "m-new", U: "m-new", "!": "m-bad" };

/* ---------- the graph ---------- */

const WORK = "·work";
const LANE = 12;
const ROW = 24;
const MID = ROW / 2;

type Line = { kind: "work" } | { kind: "commit"; commit: Commit };
const lines = computed<Array<Line & { row: GraphRow }>>(() => {
  const history = props.files.history;
  if (!history) return [];
  const work = changes.value.length > 0 && !!history.head;
  const shapes = [...(work ? [{ id: WORK, parents: [history.head!] }] : []), ...history.commits];
  const rows = layout(shapes);
  return rows.map((row, i) => (work && i === 0 ? { kind: "work" as const, row } : { kind: "commit" as const, commit: history.commits[work ? i - 1 : i], row }));
});
/** Lanes a row draws at most: a history with many branches open at once draws its first lanes
 * and leaves the rest out, rather than pushing the subject out of sight. */
const MAX_LANES = 8;
/** As VS Code draws it: each row as wide as its own lanes, its subject right after them (one wide
 * stretch deep in the history never pushes every row's subject aside). */
const widthOf = (row: GraphRow) => Math.min(Math.max(1, row.width), MAX_LANES) * LANE + 6;

const x = (lane: number) => LANE / 2 + 3 + lane * LANE;
const path = (from: number, to: number, top: number, bottom: number) =>
  from === to ? `M ${x(from)} ${top} V ${bottom}` : `M ${x(from)} ${top} C ${x(from)} ${(top + bottom) / 2} ${x(to)} ${(top + bottom) / 2} ${x(to)} ${bottom}`;
const colour = (index: number) => `var(--lane-${index})`;

/** A ref as a chip: the branch checked out stands out, a tag says so. */
function chips(commit: Commit): Array<{ text: string; current: boolean; tag: boolean }> {
  return commit.refs.slice(0, 3).map((ref) => {
    const current = ref.startsWith("HEAD -> ") || ref === "HEAD";
    const tag = ref.startsWith("tag: ");
    return { text: ref.replace(/^HEAD -> /, "").replace(/^tag: /, ""), current, tag };
  });
}

/** When, short enough for the row: the time today, the day this year, the date before that.
 * The whole date and the author are in the row's tooltip. */
const clock = computed(() => ({
  today: new Intl.DateTimeFormat(language.value, { hour: "2-digit", minute: "2-digit" }),
  year: new Intl.DateTimeFormat(language.value, { day: "numeric", month: "short" }),
  older: new Intl.DateTimeFormat(language.value, { dateStyle: "short" }),
  full: new Intl.DateTimeFormat(language.value, { dateStyle: "medium", timeStyle: "short" }),
}));
function when(seconds: number): string {
  const at = new Date(seconds * 1000);
  const today = new Date(now.value);
  if (at.toDateString() === today.toDateString()) return clock.value.today.format(at);
  return (at.getFullYear() === today.getFullYear() ? clock.value.year : clock.value.older).format(at);
}

function onScroll(event: Event): void {
  const box = event.currentTarget as HTMLElement;
  const history = props.files.history;
  if (history?.more && !history.loading && box.scrollTop + box.clientHeight > box.scrollHeight - 200) void loadHistory(props.files, true);
}

/* ---------- Changes over History, the line between them moved ---------- */

/** The least an open pane keeps: its heading and two rows. */
const PANE_MIN = 76;
const DEFAULT_SPLIT = 0.7;
const changesPane = ref<HTMLElement | null>(null);
const historyPane = ref<HTMLElement | null>(null);
/** The line moves only while both panes are open; a folded one is its heading alone. */
const sizable = computed(() => changesOpen.value && historyOpen.value);
const sashing = ref(false);

function paneFlex(open: boolean, share: number): Record<string, string> {
  if (!open) return { flex: "0 0 auto" };
  return { flex: sizable.value ? `${share} 1 0px` : "1 1 0px" };
}

/** The panes' height together, and a share kept within what leaves each its least. */
function within(share: number): number {
  const total = (changesPane.value?.offsetHeight ?? 0) + (historyPane.value?.offsetHeight ?? 0);
  if (total <= PANE_MIN * 2) return 0.5;
  return Math.min(Math.max(share, PANE_MIN / total), 1 - PANE_MIN / total);
}

let sash: { y: number; from: number; total: number } | null = null;

function beginSash(event: PointerEvent): void {
  if (!sizable.value || !changesPane.value || !historyPane.value) return;
  sash = { y: event.clientY, from: changesPane.value.offsetHeight, total: changesPane.value.offsetHeight + historyPane.value.offsetHeight };
  sashing.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  event.preventDefault();
}

function moveSash(event: PointerEvent): void {
  if (sash) split.value = within((sash.from + event.clientY - sash.y) / sash.total);
}

function endSash(): void {
  sash = null;
  sashing.value = false;
}

function sashKey(event: KeyboardEvent): void {
  if (!sizable.value) return;
  const by = event.key === "ArrowUp" ? -0.05 : event.key === "ArrowDown" ? 0.05 : 0;
  if (!by) return;
  event.preventDefault();
  split.value = within(split.value + by);
}

/* ---------- menus ---------- */

const menu = useNativeMenu();

function commitMenu(event: MouseEvent, commit: Commit): void {
  const remote = props.files.history?.remote;
  const entries: MenuEntry[] = [
    { text: t("scm.copyId"), action: () => void api.copyText(commit.id) },
    { text: t("scm.copyMessage"), action: () => void api.copyText(commit.subject) },
  ];
  if (remote) entries.push("separator", { text: t("scm.openOn", { host: remote.host }), action: () => void api.openUrl(remote.commitUrl.replace("{id}", commit.id)) });
  void menu.popup(event, entries);
}

const branch = computed(() => props.project.git);
</script>

<template>
  <aside class="scm" :aria-label="t('scm.title')">
    <div v-if="!branch" class="state">{{ t("scm.notRepo") }}</div>
    <div v-else class="panes">
      <section ref="changesPane" :class="['pane', { open: changesOpen }]" :style="paneFlex(changesOpen, split)">
        <button type="button" class="group" :aria-expanded="changesOpen" @click="changesOpen = !changesOpen">
          <Icon :name="changesOpen ? 'chevron-down' : 'chevron'" :size="12" class="ico" />
          <span class="group-name">{{ t("scm.changes") }}</span>
          <span class="count">{{ changes.length }}</span>
        </button>
        <div v-if="changesOpen" class="list">
          <div v-if="!changes.length" class="note">{{ t("scm.noChanges") }}</div>
          <div
            v-for="change in changes"
            :key="change.path"
            :class="['change', TONE[change.mark], { on: change.path === files.active }]"
            :title="change.path"
            @click="$event.detail < 2 && openDiff(files, change.path)"
            @dblclick="openDiff(files, change.path, true)"
            @contextmenu.prevent="emit('menu', $event, change.path)"
          >
            <FileIcon :path="change.path" class="fico" />
            <span class="name">{{ name(change.path) }}</span>
            <span class="dir">{{ folder(change.path) }}</span>
            <span class="letter">{{ change.mark }}</span>
          </div>
        </div>
      </section>

      <div
        :class="['sash', { live: sizable, dragging: sashing }]"
        role="separator"
        aria-orientation="horizontal"
        :tabindex="sizable ? 0 : -1"
        :aria-label="t('scm.resizePanes')"
        :aria-valuenow="Math.round(split * 100)"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-disabled="!sizable"
        @pointerdown="beginSash"
        @pointermove="moveSash"
        @pointerup="endSash"
        @pointercancel="endSash"
        @dblclick="sizable && (split = within(DEFAULT_SPLIT))"
        @keydown="sashKey"
      ></div>

      <section ref="historyPane" :class="['pane', { open: historyOpen }]" :style="paneFlex(historyOpen, 1 - split)">
        <button type="button" class="group" :aria-expanded="historyOpen" @click="historyOpen = !historyOpen">
          <Icon :name="historyOpen ? 'chevron-down' : 'chevron'" :size="12" class="ico" />
          <span class="group-name">{{ t("scm.history") }}</span>
        </button>
        <div v-if="historyOpen" class="list" @scroll.passive="onScroll">
          <div v-if="files.history?.loading && !files.history.commits.length" class="note"><Spinner /> {{ t("files.loading") }}</div>
          <div v-else-if="files.history && !files.history.commits.length" class="note">{{ t("scm.noCommits") }}</div>
          <div
            v-for="(line, i) in lines"
            :key="line.kind === 'work' ? 'work' : line.commit.id"
            :class="['commit', { on: line.kind === 'commit' && files.active === `commit:${line.commit.id}`, work: line.kind === 'work' }]"
            :title="line.kind === 'commit' ? `${line.commit.subject}\n${line.commit.author} · ${clock.full.format(new Date(line.commit.time * 1000))} · ${line.commit.id.slice(0, 7)}` : undefined"
            @click="line.kind === 'commit' && $event.detail < 2 && openCommit(files, line.commit.id)"
            @dblclick="line.kind === 'commit' && openCommit(files, line.commit.id, true)"
            @contextmenu.prevent="line.kind === 'commit' && commitMenu($event, line.commit)"
          >
            <svg class="lanes" :width="widthOf(line.row)" :height="ROW" aria-hidden="true">
              <path v-for="(pass, k) in line.row.pass" :key="`p${k}`" :d="path(pass.lane, pass.lane, 0, ROW)" :stroke="colour(pass.colour)" />
              <path v-for="(seg, k) in line.row.into" :key="`i${k}`" :d="path(seg.from, seg.to, 0, MID)" :stroke="colour(seg.colour)" />
              <path v-for="(seg, k) in line.row.out" :key="`o${k}`" :d="path(seg.from, seg.to, MID, ROW)" :stroke="colour(seg.colour)" :stroke-dasharray="line.kind === 'work' ? '2 2' : undefined" />
              <circle
                :cx="x(line.row.lane)"
                :cy="MID"
                :r="line.kind === 'work' || (files.history?.head && line.kind === 'commit' && line.commit.id === files.history.head) ? 4 : 3.5"
                :fill="line.kind === 'work' ? 'var(--bg-sidebar)' : colour(line.row.colour)"
                :stroke="colour(line.row.colour)"
                :stroke-width="line.kind === 'work' ? 1.6 : 0"
              />
            </svg>
            <template v-if="line.kind === 'work'">
              <span class="subject">{{ t("scm.worktree", { count: changes.length }) }}</span>
            </template>
            <template v-else>
              <span class="subject">{{ line.commit.subject }}</span>
              <span v-for="chip in chips(line.commit)" :key="chip.text" :class="['chip', { current: chip.current, tag: chip.tag }]">{{ chip.text }}</span>
              <span class="when">{{ when(line.commit.time) }}</span>
            </template>
            <span v-if="i === lines.length - 1 && files.history?.loading" class="more"><Spinner /></span>
          </div>
        </div>
      </section>
    </div>

    <!-- The branch along the foot, level with the file's status bar beside it, and Refresh. -->
    <div v-if="branch" class="foot">
      <Icon name="source-control" :size="12" class="ico" />
      <span class="branch-name">{{ branch.branch }}</span>
      <span v-if="branch.ahead || branch.behind" class="sync">↑{{ branch.ahead }} ↓{{ branch.behind }}</span>
      <span class="grow"></span>
      <button type="button" class="icon" :aria-label="t('scm.refresh')" :title="t('scm.refresh')" @click="refreshFiles(files)"><Icon name="restart" :size="12" /></button>
    </div>
  </aside>
</template>

<style scoped src="../detail/detail.css"></style>
<style scoped>
.scm {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--bg-sidebar);
}

.panes {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* Each pane its heading over its own scroll; an open one keeps its heading and two rows. */
.pane {
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 4px 4px 0;
}

.pane.open {
  min-height: 76px;
}

.list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding-bottom: 6px;
}

/* The line between the panes, wider to the pointer than to the eye; it shows the selection's
   colour while it can be moved and the pointer is on it. */
.sash {
  position: relative;
  z-index: 1;
  flex-shrink: 0;
  height: 1px;
  background: var(--line-soft);
  transition: background-color 0.12s ease;
}

.sash::before {
  content: "";
  position: absolute;
  inset: -3px 0;
}

.sash.live {
  cursor: row-resize;
}

.sash.live:hover,
.sash.dragging,
.sash:focus-visible {
  background: var(--line-selected);
  transition-delay: 0.15s;
}

.sash.dragging {
  transition-delay: 0s;
}

.sash:focus-visible {
  outline: none;
}

/* As the file's status bar (FileViewer): one bar along the window's foot. */
.foot {
  height: 28px;
  flex-shrink: 0;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 0 4px 0 12px;
  border-top: 1px solid var(--line-soft);
  background: var(--bg-files-bar);
  font-size: 11.5px;
  color: var(--text-soft);
  white-space: nowrap;
}

.foot .grow {
  flex: 1;
}

.foot .icon {
  width: 24px;
  height: 22px;
}

.branch-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
}

.sync {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.group {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 4px;
  height: 24px;
  padding: 0 8px 0 6px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font: inherit;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-soft);
  text-align: left;
}

.group-name {
  flex: 1;
}

.count {
  min-width: 18px;
  height: 16px;
  box-sizing: border-box;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--bg-control-on);
  font-size: 10px;
  line-height: 16px;
  text-align: center;
  color: var(--text);
}

.ico {
  flex-shrink: 0;
  color: var(--text-subtle);
}

/* A file's own icon (FileIcon, 16 px): the name where it was beside the 13 px glyph. */
.fico {
  margin: 0 -1px 0 -2px;
}

.change,
.commit {
  display: flex;
  align-items: center;
  gap: 6px;
  border-radius: 5px;
  cursor: pointer;
  white-space: nowrap;
}

.change {
  height: 22px;
  padding: 0 8px 0 22px;
  font-size: 13px;
}

.change:hover,
.commit:hover {
  background: var(--bg-row-hover);
}

.change.on,
.commit.on {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

.name {
  color: var(--text-soft);
}

.dir {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 11.5px;
  color: var(--text-subtle);
}

.letter {
  width: 12px;
  text-align: center;
  font-family: var(--font-mono);
  font-size: 11px;
}

.m-mod .name {
  color: var(--warn-text);
}

.m-mod .letter {
  color: var(--warn);
}

.m-new .name {
  color: var(--run-text);
}

.m-new .letter {
  color: var(--run);
}

.m-del .name {
  color: var(--crash-text);
  text-decoration: line-through;
}

.m-del .letter,
.m-bad .letter,
.m-bad .name {
  color: var(--crash-text);
}

.commit {
  height: 24px;
  padding: 0 8px 0 2px;
  gap: 4px;
}

.lanes {
  flex-shrink: 0;
}

.lanes path {
  fill: none;
  stroke-width: 1.6;
}

.subject {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12.5px;
  color: var(--text);
}

.work .subject {
  font-style: italic;
  color: var(--text-muted);
}

.chip {
  flex-shrink: 0;
  max-width: 84px;
  overflow: hidden;
  text-overflow: ellipsis;
  height: 16px;
  padding: 0 6px;
  border-radius: 8px;
  background: var(--bg-control-on);
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 16px;
  color: var(--text-muted);
}

/* The branch checked out (a modifier of its own: a shared name like `.head` once laid it out
   as a flex box, and its "…" went). */
.chip.current {
  color: var(--text-strong);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

.when {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-subtle);
}

.more {
  flex-shrink: 0;
}

.note,
.state {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px 6px 22px;
  font-size: 12px;
  color: var(--text-faint);
}

.state {
  padding: 16px 14px;
}
</style>
