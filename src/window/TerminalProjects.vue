<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import Highlight from "../components/Highlight.vue";
import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { WAIT_LABELS } from "../lib/board";
import { agentKey, namesAgent, rowAgent } from "../lib/format";
import { fuzzy, searchProjects } from "../lib/fuzzy";
import { t } from "../lib/i18n";
import { openOrAtWork } from "../lib/pages";
import { api, snapshot } from "../lib/store";
import type { Agent, Folder, Project, Turn } from "../lib/types";

/**
 * The terminals' project column (window/TerminalPanel), on the terminal's left as the history is on
 * its right: the projects the panel's picker would list (Settings › Projects › "Projects in the
 * terminal"), the open ones first, then the favourites and, with every project, the others. A click
 * shows a project in the panel; its right click is Pitwall's menu of it (ProjectMenu). Typed into
 * its search, every listed project that matches, then the folders of the projects folder that
 * aren't listed yet (`project_folders`: their names alone, read as a search begins); a folder
 * picked is added to Pitwall without a star, as the quick switcher adds one, and shown. `overlay`:
 * a narrow panel's, over the terminal's left side, gone once a project is picked.
 */
const props = defineProps<{
  /** The panel's project. */
  project: Project;
  /** What the picker would list. */
  projects: Project[];
  /** What Claude (or Codex) waits on you with in each project, by its path. */
  waits: Map<string, Turn["kind"]>;
  overlay?: boolean;
}>();
const emit = defineEmits<{
  /** A project to show in the panel. */
  pick: [path: string];
  /** A row's right click: the project's or the folder's menu, where the pointer is. */
  menu: [event: MouseEvent, target: { kind: "project"; path: string } | { kind: "folder"; folder: Folder }];
  close: [];
  /** Esc with nothing typed: back to the terminal. */
  leave: [];
}>();

type Line =
  | { kind: "project"; key: string; project: Project; hits: Set<number> | null }
  | { kind: "folder"; key: string; folder: Folder; hits: Set<number> };
type Group = { key: string; label: string; lines: Line[] };

const query = ref("");
const field = ref<HTMLInputElement | null>(null);
const listbox = ref<HTMLElement | null>(null);
/** The line the keyboard is on, by its key. */
const picked = ref<string | null>(null);
/** The folders of the projects folder, read as a search begins. */
const folders = ref<Folder[]>([]);
/** A folder being added, until it is a project; why one wasn't, by its path. */
const adding = ref<string | null>(null);
const failed = ref<Record<string, string>>({});

const projectsDir = computed(() => snapshot.value?.settings.projectsDir ?? null);
const dirName = computed(() => projectsDir.value?.split(/[\\/]/).filter(Boolean).pop() ?? "");
const typed = computed(() => query.value.trim().toLowerCase());

let asked = 0;
async function loadFolders(): Promise<void> {
  const ask = ++asked;
  const list = projectsDir.value ? await api.projectFolders().catch(() => []) : [];
  if (ask === asked) folders.value = list;
}
watch(typed, (now, before) => {
  if (now && !before) void loadFolders();
  picked.value = null;
});
watch(projectsDir, () => {
  if (typed.value) void loadFolders();
});

/** Its row's line: open or at work, a favourite, or one more of every project. */
const groups = computed<Group[]>(() => {
  const q = typed.value;
  if (!q) {
    const open: Line[] = [];
    const favourites: Line[] = [];
    const others: Line[] = [];
    for (const project of props.projects) {
      const line: Line = { kind: "project", key: project.path, project, hits: null };
      if (project.path === props.project.path || openOrAtWork(project)) open.push(line);
      else if (project.favourite) favourites.push(line);
      else others.push(line);
    }
    return [
      { key: "open", label: t("terminal.projectsOpen"), lines: open },
      { key: "favourites", label: t("window.filter.favourites"), lines: favourites },
      { key: "others", label: t("terminal.projectsOthers"), lines: others },
    ].filter((group) => group.lines.length);
  }

  // A search covers every listed project, whatever the picker would list.
  const all = snapshot.value?.projects ?? [];
  const found: Line[] = searchProjects(all, q).map((match) => ({ kind: "project", key: match.project.path, project: match.project, hits: match.hits }));
  const listed = new Set(all.map((p) => p.path));
  const dirs: Array<{ line: Line; score: number }> = [];
  for (const folder of folders.value) {
    if (listed.has(folder.path)) continue;
    const hit = fuzzy(folder.name, q);
    if (hit) dirs.push({ line: { kind: "folder", key: `folder:${folder.path}`, folder, hits: new Set(hit.hits) }, score: hit.score });
  }
  dirs.sort((a, b) => b.score - a.score);
  return [
    { key: "found", label: t("common.projects"), lines: found },
    { key: "folders", label: t("switcher.foldersIn", { folder: dirName.value }), lines: dirs.slice(0, 50).map((d) => d.line) },
  ].filter((group) => group.lines.length);
});

const lines = computed(() => groups.value.flatMap((group) => group.lines));
/** The heads and the lines as one list, so that a line going from one group to another slides there. */
const items = computed<Array<Line | { kind: "group"; key: string; label: string }>>(() =>
  groups.value.flatMap((group) => [{ kind: "group" as const, key: `group:${group.key}`, label: group.label }, ...group.lines]),
);

/**
 * The list changing by itself (a project opened, closed, starred, added or forgotten, a Git line
 * coming) moves: the lines that stay slide to their new places, one that comes fades in once its
 * room has opened, one that goes fades and folds up where it stood. A search answers at once, as it
 * is typed and as it is emptied: `still` holds until the frame after the list was drawn, its lines'
 * enter hooks having run by then.
 */
const still = ref(false);
watch(query, () => {
  still.value = true;
  requestAnimationFrame(() => (still.value = !!typed.value));
});

const EASE = "cubic-bezier(0.4, 0, 0.2, 1)";

function calm(): boolean {
  return still.value || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function lineEnter(el: Element, done: () => void): void {
  if (calm()) return done();
  const motion = (el as HTMLElement).animate([{ opacity: 0 }, { opacity: 1 }], { duration: 180, delay: 120, easing: EASE, fill: "backwards" });
  motion.onfinish = () => done();
  motion.oncancel = () => done();
}

function lineLeave(el: Element, done: () => void): void {
  if (calm()) return done();
  const line = el as HTMLElement;
  // It stays in the flow and folds up, so that the lines after it close its place as it goes.
  const style = getComputedStyle(line);
  const full = { height: `${line.offsetHeight}px`, paddingTop: style.paddingTop, paddingBottom: style.paddingBottom };
  const none = { height: "0px", paddingTop: "0px", paddingBottom: "0px" };
  Object.assign(line.style, { minHeight: "0px", overflow: "hidden", pointerEvents: "none" });
  const motion = line.animate([{ ...full, opacity: 1 }, { opacity: 0, offset: 0.6 }, { ...none, opacity: 0 }], { duration: 220, easing: EASE, fill: "forwards" });
  motion.onfinish = () => done();
  motion.oncancel = () => done();
}

/** No projects folder to look in: a search says so under what it found. */
const noFolder = computed(() => !!typed.value && !projectsDir.value);

/** Claude's (or Codex's) dot before the name: waiting on you, or at work. */
function live(project: Project): { phase: "waiting" | "working"; text: string } | null {
  const whose: Agent = namesAgent.value ? (rowAgent(project) ?? "claude") : "claude";
  const wait = props.waits.get(project.path);
  if (wait) return { phase: "waiting", text: t(agentKey(WAIT_LABELS[wait], whose), { name: project.name }) };
  if (project.claudeWorking) return { phase: "working", text: t(agentKey("board.picker.working", whose), { name: project.name }) };
  return null;
}

/** The dev server's state, where no agent's dot stands. */
function server(project: Project): "on" | "bad" | "busy" | null {
  if (project.status === "running") return "on";
  if (project.status === "crashed") return "bad";
  if (project.status === "busy") return "busy";
  return null;
}

function rowId(key: string): string {
  return `projects-${props.project.path}-${key}`.replace(/[^\w-]/g, "_");
}

function choose(line: Line): void {
  if (line.kind === "project") {
    emit("pick", line.project.path);
    if (query.value) query.value = "";
    return;
  }
  void add(line.folder);
}

/** A folder picked: added to Pitwall without a star, then shown in the panel. */
async function add(folder: Folder): Promise<void> {
  if (adding.value) return;
  adding.value = folder.path;
  const { [folder.path]: _, ...rest } = failed.value;
  failed.value = rest;
  try {
    await api.addProject(folder.path);
    query.value = "";
    emit("pick", folder.path);
  } catch (reason) {
    failed.value = { ...failed.value, [folder.path]: String(reason) };
  } finally {
    adding.value = null;
  }
}

function move(by: number): void {
  const all = lines.value;
  if (!all.length) return;
  const at = all.findIndex((line) => line.key === picked.value);
  const next = at < 0 ? (by > 0 ? 0 : all.length - 1) : Math.max(0, Math.min(all.length - 1, at + by));
  picked.value = all[next].key;
  void nextTick(() => listbox.value?.querySelector(`#${rowId(picked.value ?? "")}`)?.scrollIntoView({ block: "nearest" }));
}

/** In the search: ↑/↓ go along the lines, ↵ picks one (the first when none is), Esc empties the
 * search, then goes back to the terminal. */
function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return;
  switch (event.key) {
    case "ArrowDown":
      move(1);
      break;
    case "ArrowUp":
      move(-1);
      break;
    case "Enter": {
      const line = lines.value.find((each) => each.key === picked.value) ?? (typed.value ? lines.value[0] : undefined);
      if (line) choose(line);
      break;
    }
    case "Escape":
      if (query.value) query.value = "";
      else if (props.overlay) emit("close");
      else emit("leave");
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

function rowMenu(event: MouseEvent, line: Line): void {
  event.preventDefault();
  picked.value = line.key;
  emit("menu", event, line.kind === "project" ? { kind: "project", path: line.project.path } : { kind: "folder", folder: line.folder });
}

function focus(): void {
  field.value?.focus();
}

/** The search emptied (a folder the menu added is a project now). */
function clear(): void {
  query.value = "";
}

defineExpose({ focus, clear });
</script>

<template>
  <div :class="['projects', { overlay }]" :role="overlay ? 'dialog' : undefined" :aria-label="t('common.projects')">
    <label class="search">
      <Icon name="search" :size="13" />
      <input
        ref="field"
        v-model="query"
        type="text"
        role="combobox"
        spellcheck="false"
        autocomplete="off"
        aria-autocomplete="list"
        aria-expanded="true"
        :aria-controls="rowId('list')"
        :aria-activedescendant="picked ? rowId(picked) : undefined"
        :aria-label="t('common.searchProjects')"
        :placeholder="t('common.searchProjects')"
        @keydown="onKey"
      />
    </label>
    <div :id="rowId('list')" ref="listbox" class="rows" role="listbox" :aria-label="t('common.projects')">
      <div v-if="typed && !lines.length" class="note">
        {{ projectsDir ? t("switcher.noMatchAnywhere", { folder: dirName, query: query.trim() }) : t("switcher.noMatch", { query: query.trim() }) }}
      </div>
      <TransitionGroup :name="still ? 'still' : 'line'" :css="false" @enter="lineEnter" @leave="lineLeave">
        <template v-for="line in items" :key="line.key">
          <div v-if="line.kind === 'group'" class="group" role="presentation">{{ line.label }}</div>
          <div
            v-else-if="line.kind === 'project'"
            :id="rowId(line.key)"
            :class="['row', { on: line.project.path === project.path, picked: picked === line.key, repo: line.project.git }]"
            role="option"
            :aria-selected="line.project.path === project.path"
            :title="line.project.name"
            @click="choose(line)"
            @contextmenu="rowMenu($event, line)"
          >
            <span class="mark">
              <ClaudeDot v-if="live(line.project)" :live="live(line.project)" />
              <span v-else-if="server(line.project)" :class="['dot', server(line.project)]" aria-hidden="true"></span>
            </span>
            <span class="name"><Highlight v-if="line.hits" :text="line.project.name" :hits="line.hits" /><template v-else>{{ line.project.name }}</template></span>
            <span v-if="line.project.terminals.length" class="meta" :aria-label="t('window.openTerminals', { count: line.project.terminals.length })">
              <Icon name="terminal" :size="11" />{{ line.project.terminals.length }}
            </span>
            <span v-if="line.project.git" class="git">
              <span class="branch">{{ line.project.git.branch }}</span>
              <span v-if="line.project.git.changes" class="changes">· {{ t("git.changed", { count: line.project.git.changes }) }}</span>
              <span v-if="line.project.git.ahead || line.project.git.behind" class="sync">
                ·
                <span v-if="line.project.git.ahead" :title="t('git.ahead', { count: line.project.git.ahead })">↑{{ line.project.git.ahead }}</span>
                <span v-if="line.project.git.behind" :title="t('git.behind', { count: line.project.git.behind })">↓{{ line.project.git.behind }}</span>
              </span>
            </span>
          </div>
          <div
            v-else
            :id="rowId(line.key)"
            :class="['row', 'folder', { picked: picked === line.key, failed: failed[line.folder.path] }]"
            role="option"
            :aria-selected="false"
            :title="line.folder.path"
            @click="choose(line)"
            @contextmenu="rowMenu($event, line)"
          >
            <span class="mark"><Icon name="folder" :size="13" /></span>
            <span class="name"><Highlight :text="line.folder.name" :hits="line.hits" /></span>
            <Spinner v-if="adding === line.folder.path" :size="12" />
            <span v-else-if="failed[line.folder.path]" class="error" :title="failed[line.folder.path]">{{ failed[line.folder.path] }}</span>
            <span v-else class="add">{{ t("suggest.add") }}</span>
          </div>
        </template>
      </TransitionGroup>
      <div v-if="noFolder" class="hint">{{ t("switcher.chooseFolderHint") }}</div>
    </div>
  </div>
</template>

<style scoped>
/* Beside the terminal, in the room the panel opens for it on the left: the history's twin. */
.projects {
  position: absolute;
  top: 6px;
  bottom: var(--edge, 20px);
  left: var(--edge, 20px);
  width: var(--projects-width, 220px);
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: hidden;
}

/* A narrow panel's: over the terminal's left side, on a ground of its own. */
.projects.overlay {
  z-index: 1;
  left: var(--edge, 20px);
  width: min(var(--projects-width, 220px) + 16px, calc(100% - 80px));
  padding: 10px 8px 8px;
  border-radius: 8px;
  background: var(--bg-dialog);
  border: 1px solid var(--line-dialog);
  box-shadow: var(--shadow-menu);
}

.search {
  height: 30px;
  flex-shrink: 0;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  margin-bottom: 4px;
  border-radius: 7px;
  background: var(--bg-input);
  border: 1px solid var(--line-dialog);
  color: var(--text-faint);
  cursor: text;
}

/* No ring when it has the keyboard, as the history's search: the caret is enough. */
.search input {
  flex: 1 1 auto;
  min-width: 0;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--text);
}

.search input::placeholder {
  color: var(--text-faint);
}

.rows {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* A line that stays slides to its new place (the TransitionGroup's FLIP); none while a search is
   typed, nor with reduced motion. */
.line-move {
  transition: transform 260ms cubic-bezier(0.4, 0, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .line-move {
    transition: none;
  }
}

.note {
  padding: 28px 12px;
  text-align: center;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.hint {
  padding: 12px 8px 4px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.group {
  padding: 10px 8px 3px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.group:first-child,
.note + .group {
  padding-top: 4px;
}

:lang(zh) .group,
:lang(ja) .group {
  letter-spacing: 0;
}

.row {
  height: 28px;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 6px;
  border-radius: 6px;
  cursor: pointer;
}

/* A project in a Git repository takes a second line under its name: its branch, its changes and
   the commits going out and coming in. The dot stays on the name's line. */
.row.repo {
  height: auto;
  min-height: 28px;
  display: grid;
  grid-template-columns: 13px minmax(0, 1fr) auto;
  grid-template-areas:
    "mark name meta"
    ". git meta";
  column-gap: 8px;
  row-gap: 1px;
  padding-top: 5px;
  padding-bottom: 5px;
}

.row.repo .mark {
  grid-area: mark;
  height: 18px;
}

.row.repo .name {
  grid-area: name;
  line-height: 18px;
}

/* The tab count is the row's, between its two lines: never over a count of the Git line. */
.row.repo .meta {
  grid-area: meta;
  align-self: center;
}

.git {
  grid-area: git;
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: 4px;
  overflow: hidden;
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  line-height: 15px;
  color: var(--text-faint);
  white-space: nowrap;
}

/* A long branch gives way; the counts never do. */
.branch {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.changes,
.sync {
  flex-shrink: 0;
  color: var(--text-subtle);
}

.row:hover,
.row.picked {
  background: var(--bg-row-hover);
}

/* The panel's own project: the history's picked row's ground. */
.row.on {
  background: var(--bg-seg-on);
}

.mark {
  width: 13px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-subtle);
}

.dot {
  width: 6px;
  height: 6px;
  box-sizing: border-box;
  border-radius: 50%;
}

.dot.on {
  background: var(--run);
}

.dot.bad {
  background: var(--crash);
}

.dot.busy {
  border: 1px solid var(--text-faint);
}

.name {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 12.5px;
  color: var(--text-soft);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row:hover .name,
.row.picked .name,
.row.on .name {
  color: var(--text-strong);
}

.folder .name {
  color: var(--text-muted);
}

.meta {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
}

/* A folder's "Add", in sight while the row is under the pointer or the keyboard. */
.add {
  display: none;
  flex-shrink: 0;
  padding: 1px 7px;
  border: 1px solid var(--line-strong);
  border-radius: 5px;
  background: var(--bg-control);
  font-size: 11.5px;
  color: var(--text);
  white-space: nowrap;
}

.row:hover .add,
.row.picked .add {
  display: inline-flex;
}

.error {
  flex-shrink: 1;
  min-width: 0;
  max-width: 55%;
  font-size: 11.5px;
  color: var(--crash-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
