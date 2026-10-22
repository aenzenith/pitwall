<script setup lang="ts">
import { LogicalSize } from "@tauri-apps/api/dpi";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Highlight from "../components/Highlight.vue";
import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Marquee from "../components/Marquee.vue";
import Spinner from "../components/Spinner.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { composing, draft } from "../lib/capture";
import { bareUrl, claudeState, commandStatus, gitLine, meta, uptime } from "../lib/format";
import { byRank, fuzzy, searchProjects, type ProjectMatch } from "../lib/fuzzy";
import { t } from "../lib/i18n";
import { generalOpen, openFolderFromOutside, openFromOutside, openTarget } from "../lib/open";
import { pages, type Page, type PageEntry } from "../lib/pages";
import { keys, primary } from "../lib/platform";
import { api, now, snapshot } from "../lib/store";
import type { CommandView, Folder, Project, ProjectLink } from "../lib/types";
import { useCapture } from "./capture";
import CaptureNote from "./CaptureNote.vue";

const WIDTH = 640;
const MAX_HEIGHT = 540;

type FolderMatch = { folder: Folder; score: number; hits: Set<number> };
type CommandMatch = { project: Project; command: CommandView; score: number; hits: Set<number> };
type LinkMatch = { project: Project; link: ProjectLink; score: number; hits: Set<number> };
type PageMatch = { page: PageEntry; score: number; hits: Set<number> };

/** One row of the list: a project, a project's command or link, a page of the window, a folder
 * from the projects folder, or (while a card is written, `+`) a project the card can go to. */
type Item =
  | { kind: "project"; key: string; project: Project; hits: Set<number> }
  | { kind: "card"; key: string; project: Project; hits: Set<number> }
  | { kind: "command"; key: string; project: Project; command: CommandView; hits: Set<number> }
  | { kind: "link"; key: string; project: Project; link: ProjectLink; hits: Set<number> }
  | { kind: "page"; key: string; page: PageEntry; hits: Set<number> }
  | { kind: "folder"; key: string; folder: Folder; hits: Set<number> };

/** A row whose name isn't searched has no letters to mark. */
const NO_HITS = new Set<number>();

/** Commands shown under the projects while typing; `>` lists them all. */
const MIXED_COMMANDS = 6;
/** Links shown under them; `@` lists them all. */
const MIXED_LINKS = 4;

const query = ref("");
/** The highlighted row, by its key: a re-sort (Claude, a server starting) moves the row, not the
 * highlight. */
const selectedKey = ref<string | null>(null);
/** Where the highlighted row last was, for when it leaves the list. */
let lastIndex = 0;
const input = ref<HTMLInputElement | null>(null);
const panel = ref<HTMLElement | null>(null);
const list = ref<HTMLElement | null>(null);
/** Drives the fade-up: off while hidden, on a frame after the window appears. */
const shown = ref(false);

function reveal(): void {
  shown.value = false;
  // Two frames: the hidden state must paint before the transition can run from it.
  requestAnimationFrame(() => requestAnimationFrame(() => (shown.value = true)));
}

/** The window went away (blur, Esc, a pick): be in the hidden state for the next opening. A pick
 * that opens another app conceals at once, so the panel is gone even while the window waits for
 * that app to take focus. */
function conceal(): void {
  shown.value = false;
}

/** The note under the field, while a card's is open. */
const noteView = ref<InstanceType<typeof CaptureNote> | null>(null);

/** `+`: a board card written here (./capture). The field holds its title then, and the list the
 * projects it can go to; the highlight is the project chosen. */
const capture = useCapture({
  query,
  field: () => input.value,
  note: () => noteView.value,
  chosen: () => (current.value?.kind === "card" ? current.value.project.path : null),
  choose: chooseProject,
});
const { on: capturing, noteOpen, looked, counts, error: captureError, busy: captureBusy, waiting: cardWaiting, shots } = capture;
/** The note shows under the field (its project still among those listed). */
const noting = computed(() => capturing.value && noteOpen.value && current.value?.kind === "card");

/** The highlight on the row of the project at `path`, among those a card can go to. */
function chooseProject(path: string | null): void {
  const at = items.value.findIndex((item) => item.kind === "card" && item.project.path === path);
  if (at >= 0) select(at);
}

/** What the field holds: the search, or the title of the card being written. */
const field = computed({
  get: () => (capturing.value ? draft.title : query.value),
  set: (text: string) => {
    if (capturing.value) draft.title = text;
    else query.value = text;
  },
});

const matches = computed<ProjectMatch[]>(() => {
  const projects = snapshot.value?.projects ?? [];
  const q = query.value.trim().toLowerCase();
  if (!q) return byRank(projects).map((project) => ({ project, score: 0, hits: new Set<number>() }));
  return searchProjects(projects, q);
});

/** Subfolders of the projects folder (Settings), read each time the switcher opens. */
const folders = ref<Folder[]>([]);
const projectsDir = computed(() => snapshot.value?.settings.projectsDir ?? null);
const dirName = computed(() => projectsDir.value?.split(/[\\/]/).filter(Boolean).pop() ?? "");

/** When no project matches, the folders in the projects folder that aren't projects yet. */
const folderMatches = computed<FolderMatch[]>(() => {
  const q = query.value.trim().toLowerCase();
  if (!q || matches.value.length) return [];

  const listed = new Set((snapshot.value?.projects ?? []).map((p) => p.path));
  const found: FolderMatch[] = [];

  for (const folder of folders.value) {
    if (listed.has(folder.path)) continue;
    const hit = fuzzy(folder.name, q);
    if (hit) found.push({ folder, score: hit.score, hits: new Set(hit.hits) });
  }

  return found.sort((a, b) => b.score - a.score).slice(0, 50);
});

/** `>` at the start lists only commands, as in VS Code's command palette; `@` only links; `/` the
 * window's pages. */
const commandMode = computed(() => query.value.trimStart().startsWith(">"));
const linkMode = computed(() => query.value.trimStart().startsWith("@"));
const pageMode = computed(() => query.value.trimStart().startsWith("/"));
const needle = computed(() => (commandMode.value || linkMode.value || pageMode.value ? query.value.trimStart().slice(1) : query.value).trim().toLowerCase());
const anyCommands = computed(() => (snapshot.value?.projects ?? []).some((p) => p.commands.length));
const anyLinks = computed(() => (snapshot.value?.projects ?? []).some((p) => p.settings.links?.length));
const anyProjects = computed(() => (snapshot.value?.projects ?? []).length > 0);

/**
 * Every project's commands that match: by the command's name, by project and name together
 * ("cog mig"), or by the command line. Running ones first among equals; with `>` and nothing
 * typed, all of them in list order.
 */
const commandMatches = computed<CommandMatch[]>(() => {
  const q = needle.value;
  if (linkMode.value || (!commandMode.value && !q)) return [];

  const found: CommandMatch[] = [];

  (snapshot.value?.projects ?? []).forEach((project, order) => {
    project.commands.forEach((command, at) => {
      if (!q) {
        found.push({ project, command, score: -(order * 1000 + at), hits: new Set() });
        return;
      }

      const byName = fuzzy(command.name, q);
      const byBoth = fuzzy(`${project.name} ${command.name}`, q);
      const byLine = fuzzy(command.command, q);
      const best = Math.max(byName?.score ?? -Infinity, (byBoth?.score ?? -Infinity) * 0.8, (byLine?.score ?? -Infinity) * 0.5);

      if (best > -Infinity) {
        found.push({ project, command, score: best + (command.status === "running" ? 0.5 : 0), hits: new Set(byName?.hits ?? []) });
      }
    });
  });

  found.sort((a, b) => b.score - a.score);
  return commandMode.value ? found : found.slice(0, MIXED_COMMANDS);
});

/**
 * Every project's links that match: by the link's name, by project and name together
 * ("tel stag"), or by the address. With `@` and nothing typed, all of them in list order.
 */
const linkMatches = computed<LinkMatch[]>(() => {
  const q = needle.value;
  if (commandMode.value || (!linkMode.value && !q)) return [];

  const found: LinkMatch[] = [];

  (snapshot.value?.projects ?? []).forEach((project, order) => {
    (project.settings.links ?? []).forEach((link, at) => {
      if (!q) {
        found.push({ project, link, score: -(order * 1000 + at), hits: new Set() });
        return;
      }

      const byName = fuzzy(link.name, q);
      const byBoth = fuzzy(`${project.name} ${link.name}`, q);
      const byUrl = fuzzy(bareUrl(link.url), q);
      const best = Math.max(byName?.score ?? -Infinity, (byBoth?.score ?? -Infinity) * 0.8, (byUrl?.score ?? -Infinity) * 0.5);

      if (best > -Infinity) found.push({ project, link, score: best, hits: new Set(byName?.hits ?? []) });
    });
  });

  found.sort((a, b) => b.score - a.score);
  return linkMode.value ? found : found.slice(0, MIXED_LINKS);
});

/**
 * The window's pages that match, by their names in the language shown; with `/` and nothing
 * typed, all of them as the sidebar lists them.
 */
const pageMatches = computed<PageMatch[]>(() => {
  const q = needle.value;
  if (!q) return pages.map((page) => ({ page, score: 0, hits: new Set<number>() }));

  const found: PageMatch[] = [];

  for (const page of pages) {
    const hit = fuzzy(t(page.label), q);
    if (hit) found.push({ page, score: hit.score, hits: new Set(hit.hits) });
  }

  return found.sort((a, b) => b.score - a.score);
});

/** What the list shows: with `/` pages only, with `>` commands only, with `@` links only; else
 * projects, then matching commands and links; else folders. */
const items = computed<Item[]>(() => {
  if (capturing.value) return capture.projects.value.map((project) => ({ kind: "card", key: `k:${project.path}`, project, hits: NO_HITS }));
  if (pageMode.value) return pageMatches.value.map(({ page, hits }) => ({ kind: "page", key: `g:${page.id}`, page, hits }));

  const commands: Item[] = commandMatches.value.map(({ project, command, hits }) => ({ kind: "command", key: `c:${project.path}:${command.id}`, project, command, hits }));
  if (commandMode.value) return commands;

  const links: Item[] = linkMatches.value.map(({ project, link, hits }, at) => ({ kind: "link", key: `l:${project.path}:${at}:${link.url}`, project, link, hits }));
  if (linkMode.value) return links;

  const projects: Item[] = matches.value.map(({ project, hits }) => ({ kind: "project", key: `p:${project.path}`, project, hits }));
  if (projects.length || commands.length || links.length) return [...projects, ...commands, ...links];

  return folderMatches.value.map(({ folder, hits }) => ({ kind: "folder", key: `f:${folder.path}`, folder, hits }));
});

const count = computed(() => items.value.length);
/** The highlighted row's place: where its key is now; if it left the list, the row that took its
 * place (the last one when the list got shorter). */
const index = computed(() => {
  const at = items.value.findIndex((item) => item.key === selectedKey.value);
  return at >= 0 ? at : Math.max(0, Math.min(lastIndex, count.value - 1));
});
const current = computed<Item | null>(() => items.value[index.value] ?? null);

/** Highlights the row at `at` (↑/↓, the pointer, a new query). */
function select(at: number): void {
  lastIndex = at;
  selectedKey.value = items.value[at]?.key ?? null;
}

// The list changed (a state event, a query): the highlight stays on its row, or takes the row
// that fell into its place.
watch(items, () => select(index.value));

// The project a card would go to is another one: the card's note closes when its project left
// the list (./capture: chosenChanged), so the row that took its place is seen before any Return.
watch(() => (current.value?.kind === "card" ? current.value.project.path : null), capture.chosenChanged);

/** The id the input's `aria-activedescendant` points at. */
function optionId(item: Item): string {
  return `option-${encodeURIComponent(item.key)}`;
}
const selected = computed(() => (current.value?.kind === "project" ? current.value.project : null));
/** A command marked "ask first" waits here for a second ↵. */
const confirming = ref<string | null>(null);

async function loadFolders(): Promise<void> {
  folders.value = await api.projectFolders();
}

function detail(p: Project): string {
  return [meta(p, now.value), gitLine(p.git)].filter(Boolean).join("  ·  ");
}

/** ↵ on a project: with Claude waiting on you there, its waiting session; else the project as
 * Settings › Projects says for the switcher (lib/open). */
function open(p: Project | null): void {
  if (!p) return;
  conceal();
  if (p.claude) void api.openClaude(p.path);
  else openFromOutside(p.path);
}

/** What ↵ on the selected project does, for the foot. */
const OPEN_HINTS = { terminal: "open.terminal", list: "switcher.goToProject", editor: "common.openInEditor", files: "files.show" } as const;

function toggle(p: Project | null): void {
  if (!p || p.status === "busy") return;
  void api.act(p.path, p.status === "running" ? "stop" : "start");
}

/** A folder from the projects folder: ↵ opens it as Settings › Projects says for the switcher (in
 * Pitwall's window it is added first, lib/open), ⌘↵ (Ctrl+Enter) only adds it to Pitwall. */
function openFolder(folder: Folder | null, add: boolean): void {
  if (!folder) return;
  if (add) {
    void api.addProject(folder.path);
    return;
  }
  conceal();
  void openFolderFromOutside(folder.path);
}

/** What ↵ on a folder does, for the foot. */
const FOLDER_HINTS = { terminal: "switcher.addAndFocusTerminal", list: "switcher.addAndGo", editor: "common.openInEditor", files: "switcher.addAndShowFiles" } as const;

function mark(command: CommandView): string {
  return { running: "●", busy: "", ok: "✓", failed: "✕", idle: "·" }[commandStatus(command, now.value)];
}

/** Pitwall's window, its terminal panel showing this command's output, on the page shown. */
function showOutput(path: string, job: string): void {
  void emitTo("main", "reveal-output", { path, job });
  void api.openWindow();
}

/**
 * A command: ↵ runs it (stops it while it runs) and the switcher goes away; ⌘↵ also shows its
 * output in Pitwall's window. One marked "ask first" wants a second ↵.
 */
function runCommand(item: Extract<Item, { kind: "command" }>, show: boolean): void {
  const { project, command } = item;
  if (command.status === "busy") return;

  if (command.status === "running") {
    if (show) showOutput(project.path, command.id);
    else {
      void api.stopCommand(project.path, command.id);
      void api.hideSwitcher();
    }
    return;
  }

  if (command.confirm && confirming.value !== item.key) {
    confirming.value = item.key;
    return;
  }

  confirming.value = null;
  void api.runCommand(project.path, command.id);
  if (show) showOutput(project.path, command.id);
  else void api.hideSwitcher();
}

/** Pitwall's window, on the project. */
function showProject(path: string): void {
  void emitTo("main", "reveal-project", { path });
  void api.openWindow();
}

/** A link: ↵ opens it (its tab comes forward if open) and the switcher goes; ⌘↵ shows its project in Pitwall's window. */
function openLink(item: Extract<Item, { kind: "link" }>, show: boolean): void {
  if (show) showProject(item.project.path);
  else {
    conceal();
    void api.openUrl(item.link.url);
  }
}

/** A page: Pitwall's window, on it. */
function openPage(page: Page): void {
  void emitTo("main", "reveal-page", { page });
  void api.openWindow();
}

/** A filter's count, as the sidebar shows it. */
function pageCount(page: PageEntry): number {
  return (snapshot.value?.projects ?? []).filter(page.test ?? (() => true)).length;
}

/** A click does what ↵ does; on a project a card can go to, with ⌘ (Ctrl) what ⌘↵ does. */
function pick(item: Item, event: MouseEvent): void {
  if (item.kind === "card") capture.addTo(item.project.path, primary(event));
  else if (item.kind === "project") open(item.project);
  else if (item.kind === "command") runCommand(item, false);
  else if (item.kind === "link") openLink(item, false);
  else if (item.kind === "page") openPage(item.page.id);
  else openFolder(item.folder, false);
}

function onKey(event: KeyboardEvent): void {
  // A key of an input method's (the Return that ends a composition) is none of the switcher's.
  if (composing(event)) return;
  // While a card is written, its own keys.
  if (capture.onKey(event)) return;

  const total = count.value;

  if (event.key === "Escape") {
    event.preventDefault();
    // Esc first takes back a pending "run?", then closes.
    if (confirming.value) confirming.value = null;
    else void api.hideSwitcher();
  } else if (event.key === "ArrowDown") {
    event.preventDefault();
    if (total) select((index.value + 1) % total);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    if (total) select((index.value - 1 + total) % total);
  } else if (event.key === "Enter") {
    event.preventDefault();
    const item = current.value;
    if (!item) return;
    const mod = primary(event);
    if (item.kind === "folder") openFolder(item.folder, mod);
    else if (item.kind === "command") runCommand(item, mod);
    else if (item.kind === "link") openLink(item, mod);
    else if (item.kind === "page") openPage(item.page.id);
    else if (item.kind === "card") return;
    else if (mod) toggle(item.project);
    else open(item.project);
  } else if (primary(event) && event.code === "KeyC" && current.value?.kind === "link" && !hasSelection()) {
    // ⌘C (Ctrl+C) on a link copies its address (text selected in the search box copies as usual).
    event.preventDefault();
    void api.copyText(current.value.link.url);
    void api.hideSwitcher();
  } else if (primary(event) && event.key.toLowerCase() === "b" && selected.value) {
    // The browser takes focus; the switcher goes as it opens.
    event.preventDefault();
    conceal();
    void api.openBrowser(selected.value.path);
  } else if (primary(event) && event.code === "KeyP") {
    // ⌘P (Ctrl+P): open Pitwall's window; the switcher closes as the window takes focus.
    event.preventDefault();
    void api.openWindow();
  }
}

function hasSelection(): boolean {
  const el = input.value;
  return !!el && el.selectionStart !== el.selectionEnd;
}

async function fitWindow(): Promise<void> {
  await nextTick();
  const el = panel.value;
  if (!el) return;
  const parts = Array.from(el.children) as HTMLElement[];
  // What scrolls inside (the list, a card's note) counts with all it holds.
  const natural = Math.ceil(parts.reduce((sum, part) => sum + (part === list.value || part.dataset.scrolls !== undefined ? part.scrollHeight : part.offsetHeight), 2));
  // The note's open project list hangs over what is under it, and may need more.
  const wanted = Math.max(natural, noteView.value?.reach() ?? 0);
  await getCurrentWindow().setSize(new LogicalSize(WIDTH, Math.min(wanted, MAX_HEIGHT)));
}

watch(query, (text) => {
  // `+` first: what follows is a card's title, not a search.
  if (capture.take(text)) return;
  select(0);
  confirming.value = null;
});
// Another row: a pending "run?" is taken back.
watch(selectedKey, () => (confirming.value = null));
watch(index, async () => {
  await nextTick();
  list.value?.querySelector(".row.on")?.scrollIntoView({ block: "nearest" });
  // A card's first project: the label over it comes into sight with it.
  if (capturing.value && index.value === 0 && list.value) list.value.scrollTop = 0;
});
watch(count, () => void fitWindow());
// A card's parts come and go under the field: its projects, its note, an image, a refusal.
watch([capturing, noting, () => shots.value.length, captureError], () => void fitWindow());
// The `+` was deleted: the search starts at its first row again.
watch(capturing, (on) => {
  if (!on) select(0);
});

/** While a card is written, a press on anything but a field leaves the keyboard where it is. */
function onPress(event: MouseEvent): void {
  if (capturing.value && !(event.target instanceof Element && event.target.closest("input, textarea"))) event.preventDefault();
}

/** How many cards wait in a project's "Up next"; a space until the board has answered, so the row
 * keeps its two lines. */
function queuedLine(path: string): string {
  const known = counts.value;
  if (!known) return "\u00a0";
  const waiting = known.get(path) ?? 0;
  return waiting ? t("capture.queued", { count: waiting }) : t("capture.queuedNone");
}

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("blur", conceal);
  unlisten = await listen("switcher-opened", async () => {
    capture.reset();
    query.value = "";
    select(0);
    void loadFolders();
    await fitWindow();
    reveal();
    input.value?.focus();
  });
  void fitWindow();
  void loadFolders();
  input.value?.focus();
  // Listening for `switcher-opened` now: on its first opening the window comes up with this.
  void api.windowReady();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("blur", conceal);
  unlisten?.();
});
</script>

<template>
  <div ref="panel" :class="['panel', { shown }]" @mousedown="onPress">
    <div class="search">
      <PitwallGlyph :size="20" lamps="var(--run)" class="glyph" />
      <!-- A card is written: the `+` that began it, before its title. -->
      <span v-if="capturing" class="plus" aria-hidden="true">+</span>
      <label class="sr" for="q">{{ t(capturing ? "board.dialog.title" : "switcher.label") }}</label>
      <input
        id="q"
        ref="input"
        v-model="field"
        :placeholder="t(capturing ? 'board.dialog.titlePlaceholder' : 'switcher.placeholder')"
        :maxlength="capturing ? 200 : undefined"
        autocomplete="off"
        spellcheck="false"
        role="combobox"
        aria-controls="results"
        aria-autocomplete="list"
        :aria-expanded="count > 0 && !noting"
        :aria-activedescendant="current && !noting ? optionId(current) : undefined"
        @focus="capture.titleFocused"
      />
      <!-- Nothing typed: what a first character narrows the list to, here where it is typed. A
           click types it; the keyboard stays in the field. -->
      <div v-if="!query && !capturing" class="modes">
        <button v-if="anyCommands" type="button" tabindex="-1" @mousedown.prevent @click="query = '>'"><kbd>&gt;</kbd> {{ t("common.commands") }}</button>
        <button v-if="anyLinks" type="button" tabindex="-1" @mousedown.prevent @click="query = '@'"><kbd>@</kbd> {{ t("common.links") }}</button>
        <button type="button" tabindex="-1" @mousedown.prevent @click="query = '/'"><kbd>/</kbd> {{ t("switcher.pages") }}</button>
        <!-- `+` writes a board card; lit while one was begun and left. -->
        <button v-if="anyProjects" type="button" tabindex="-1" :class="{ kept: cardWaiting }" @mousedown.prevent @click="query = '+'"><kbd>+</kbd> {{ t("capture.chip") }}</button>
      </div>
      <span v-if="capturing" class="card-tag">
        <span class="card-mark"><Spinner v-if="captureBusy" :size="10" /><template v-else>+</template></span>
        {{ t("capture.chip") }}
      </span>
      <kbd v-else class="esc">{{ keys("Escape") }}</kbd>
    </div>

    <CaptureNote
      v-if="noting && current?.kind === 'card'"
      ref="noteView"
      v-model:note="draft.note"
      :projects="capture.projects.value"
      :path="current.project.path"
      :shots="shots"
      :room="MAX_HEIGHT"
      @pick="chooseProject"
      @paste="capture.onPaste"
      @remove="capture.removeImage"
      @resized="fitWindow"
    />

    <ul
      v-else-if="items.length"
      id="results"
      ref="list"
      class="list"
      role="listbox"
      :aria-label="t(capturing ? 'capture.where' : commandMode ? 'common.commands' : linkMode ? 'common.links' : pageMode ? 'switcher.pages' : 'common.projects')"
    >
      <template v-for="(item, i) in items" :key="item.key">
        <!-- Under the projects, the commands that match get their own heading. -->
        <li v-if="item.kind === 'command' && !commandMode && i > 0 && items[i - 1].kind !== 'command'" class="group" role="presentation">
          {{ t("common.commands") }}
        </li>
        <li v-if="item.kind === 'link' && !linkMode && i > 0 && items[i - 1].kind !== 'link'" class="group" role="presentation">
          {{ t("common.links") }}
        </li>
        <li v-if="item.kind === 'card' && i === 0" class="group" role="presentation">{{ t("capture.where") }}</li>
        <li
          :id="optionId(item)"
          :class="['row', { on: i === index, compact: item.kind === 'page' }]"
          role="option"
          :aria-selected="i === index"
          @mousemove="i !== index && select(i)"
          @click="pick(item, $event)"
        >
          <template v-if="item.kind === 'project'">
            <StatusIcon :project="item.project" :ring="i === index ? 'var(--bg-lit)' : 'var(--bg-panel)'" />
            <div class="text">
              <div class="name">
                <Highlight :text="item.project.name" :hits="item.hits" />
              </div>
              <div :class="['meta', { bad: item.project.status === 'crashed' }]"><Marquee :text="detail(item.project)" /></div>
            </div>
            <span v-if="item.project.claude || item.project.claudeWorking" :class="['claude', { quiet: !item.project.claude }]">
              {{ claudeState(item.project, now) }}
            </span>
            <span v-if="item.project.status === 'busy'" class="busy"><Spinner :size="12" /></span>
          </template>

          <!-- A project a card can go to: how full its "Up next" is, and which one was looked at last. -->
          <template v-else-if="item.kind === 'card'">
            <StatusIcon :project="item.project" :ring="i === index ? 'var(--bg-lit)' : 'var(--bg-panel)'" />
            <div class="text">
              <div class="name">{{ item.project.name }}</div>
              <div class="meta">{{ queuedLine(item.project.path) }}</div>
            </div>
            <span v-if="item.project.path === looked" class="looked">{{ t("capture.lastLooked") }}</span>
          </template>

          <template v-else-if="item.kind === 'command'">
            <span :class="['cmd-mark', commandStatus(item.command, now)]">
              <Spinner v-if="item.command.status === 'busy'" :size="12" />
              <template v-else>{{ mark(item.command) }}</template>
            </span>
            <div class="text">
              <div class="name">
                <Highlight :text="item.command.name" :hits="item.hits" />
              </div>
              <div v-if="confirming === item.key" class="meta confirm">{{ t("switcher.confirmAgain", { name: item.command.name, key: keys("Enter") }) }}</div>
              <div v-else class="meta"><Marquee :text="`${item.project.name}  ·  ${item.command.command}`" /></div>
            </div>
            <span v-if="item.command.status === 'running'" class="uptime">{{ uptime(item.command.startedAt, now) }}</span>
          </template>

          <template v-else-if="item.kind === 'link'">
            <span class="folder-icon"><Icon name="link" :size="16" /></span>
            <div class="text">
              <div class="name">
                <span class="link-project">{{ item.project.name }} › </span>
                <Highlight :text="item.link.name" :hits="item.hits" />
              </div>
              <div class="meta"><Marquee :text="bareUrl(item.link.url)" /></div>
            </div>
          </template>

          <template v-else-if="item.kind === 'page'">
            <span class="folder-icon"><Icon :name="item.page.icon" :size="16" /></span>
            <div class="text">
              <div class="name">
                <Highlight :text="t(item.page.label)" :hits="item.hits" />
              </div>
            </div>
            <span v-if="item.page.test" :class="['count', { hot: item.page.id === 'waiting' && pageCount(item.page) > 0 }]">{{ pageCount(item.page) }}</span>
          </template>

          <template v-else>
            <span class="folder-icon"><Icon name="folder" :size="16" /></span>
            <div class="text">
              <div class="name">
                <Highlight :text="item.folder.name" :hits="item.hits" />
              </div>
              <div class="meta">{{ item.folder.path }}</div>
            </div>
          </template>

          <span v-if="i === index" class="enter">↵</span>
        </li>
      </template>
    </ul>

    <div v-else class="empty">
      <template v-if="commandMode && needle">{{ t("switcher.noCommandMatch", { query: needle }) }}</template>
      <template v-else-if="commandMode">{{ t("switcher.noCommands") }}</template>
      <template v-else-if="linkMode && needle">{{ t("switcher.noLinkMatch", { query: needle }) }}</template>
      <template v-else-if="linkMode">{{ t("switcher.noLinks") }}</template>
      <template v-else-if="pageMode">{{ t("switcher.noPageMatch", { query: needle }) }}</template>
      <template v-else-if="query.trim() && projectsDir">{{ t("switcher.noMatchAnywhere", { folder: dirName, query }) }}</template>
      <template v-else-if="query.trim()">
        {{ t("switcher.noMatch", { query }) }}
        <span class="hint">{{ t("switcher.chooseFolderHint") }}</span>
      </template>
      <template v-else>{{ t("switcher.noProjects") }}</template>
    </div>

    <p v-if="capturing && captureError" class="refusal" role="alert">{{ captureError }}</p>

    <!-- A card's keys: in its note, and in its title over the projects. -->
    <footer v-if="noting" class="foot foot-note">
      <span><kbd>{{ keys("Shift+Enter") }}</kbd> {{ t("capture.add") }}</span>
      <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t("capture.addGive") }}</span>
      <span><kbd>{{ keys("mod+V") }}</kbd> {{ t("capture.pasteImage") }}</span>
      <span class="grow"></span>
      <span><kbd>{{ keys("Escape") }}</kbd> {{ t("common.back") }}</span>
    </footer>
    <footer v-else-if="capturing" class="foot">
      <template v-if="current">
        <span><kbd>{{ keys("Enter") }}</kbd> {{ t("capture.add") }}</span>
        <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t("capture.addGive") }}</span>
        <span><kbd>{{ keys("Tab") }}</kbd> {{ t("capture.note") }}</span>
      </template>
      <span class="grow"></span>
      <span><kbd>{{ keys("Escape") }}</kbd> {{ t("common.close") }}</span>
    </footer>
    <footer v-else-if="current?.kind === 'folder'" class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t(FOLDER_HINTS[generalOpen("outside")]) }}</span>
      <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t("switcher.addToPitwall") }}</span>
      <span class="grow"></span>
      <span><kbd>{{ keys("mod+P") }}</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else-if="current?.kind === 'command'" class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t(current.command.status === "running" ? "common.stop" : "common.run") }}</span>
      <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t(current.command.status === "running" ? "switcher.showOutput" : "switcher.runAndShow") }}</span>
      <span class="grow"></span>
      <span><kbd>{{ keys("mod+P") }}</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else-if="current?.kind === 'link'" class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t("common.openInBrowser") }}</span>
      <span><kbd>{{ keys("mod+C") }}</kbd> {{ t("link.copyAddress") }}</span>
      <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t("switcher.goToProject") }}</span>
      <span class="grow"></span>
      <span><kbd>{{ keys("mod+P") }}</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else-if="current?.kind === 'page'" class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t("common.open") }}</span>
      <span class="grow"></span>
      <span><kbd>{{ keys("mod+P") }}</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t(selected?.claude ? "switcher.openMarkSeen" : selected ? OPEN_HINTS[openTarget(selected.path, "outside")] : "common.openInEditor") }}</span>
      <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t(selected?.status === "running" ? "common.stop" : "common.start") }}</span>
      <span><kbd>{{ keys("mod+B") }}</kbd> {{ t("switcher.browser") }}</span>
      <span class="grow"></span>
      <span><kbd>{{ keys("mod+P") }}</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
  </div>
</template>

<style scoped>
.panel {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg-panel);
  /* The window is see-through and casts no shadow: this line is the panel's edge on any wallpaper. */
  border: 1px solid var(--line-panel);
  border-radius: 14px;
  overflow: hidden;
  /* Hidden state; .shown fades it up into place. */
  opacity: 0;
  transform: translateY(10px) scale(0.985);
  transform-origin: 50% 0;
}

/* Linux: square corners on the solid ground the page paints there (tokens.css), as the window may
   not be see-through. */
:root[data-platform="linux"] .panel {
  border-radius: 0;
}

.panel.shown {
  opacity: 1;
  transform: none;
  transition:
    opacity 160ms ease-out,
    transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .panel {
    transform: none;
  }
}

.search {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  height: 58px;
  padding: 0 16px;
  border-bottom: 1px solid var(--line);
}

.glyph {
  color: var(--text-brand);
  flex-shrink: 0;
}

.sr {
  position: absolute;
  left: -9999px;
}

input {
  flex-grow: 1;
  min-width: 0;
  height: 100%;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text-strong);
  font: inherit;
  font-size: 18px;
  caret-color: var(--run);
}

input::placeholder {
  color: var(--text-faint);
}

kbd {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
  padding: 1px 5px;
  border: 1px solid var(--line-strong);
  border-radius: 4px;
  background: var(--bg-key);
}

.esc {
  flex-shrink: 0;
}

/* The `+` a card began with, before its title: the field's own size, close to its text. */
.plus {
  flex-shrink: 0;
  margin-right: -5px;
  font-family: var(--font-mono);
  font-size: 18px;
  color: var(--run);
}

/* What the field holds while a card is written, where the prefixes were. */
.card-tag {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 22px;
  padding: 0 7px;
  border-radius: 6px;
  background: var(--run-bg);
  font-size: 12px;
  color: var(--run-text);
  white-space: nowrap;
}

/* Its `+`, and the spinner in its place while the card is added. */
.card-mark {
  width: 10px;
  display: inline-flex;
  justify-content: center;
  font-family: var(--font-mono);
  color: var(--run);
}

.modes {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 14px;
}

.modes button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0;
  border: 0;
  background: transparent;
  font: inherit;
  font-size: 12px;
  color: var(--text-subtle);
}

.modes button:hover {
  color: var(--text);
}

/* Russian's names for the four are the longest: a little closer, so the field keeps its
   placeholder whole beside them. */
:lang(ru) .modes {
  gap: 10px;
}

/* A card was begun and left: its prefix says so. */
.modes button.kept {
  color: var(--run-text);
}

.modes button.kept kbd {
  border-color: var(--run-line);
  background: var(--run-bg);
  color: var(--run);
}

.list {
  flex: 0 1 auto;
  min-height: 0;
  margin: 0;
  padding: 6px;
  list-style: none;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 50px;
  padding: 0 12px;
  border-radius: 9px;
}

.row.on {
  background: var(--bg-lit);
}

/* A page: one line, so all of them show at once. */
.row.compact {
  min-height: 38px;
}

.text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.name {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-name);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.meta {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.meta.bad {
  color: var(--crash-text);
}

.claude {
  flex-shrink: 0;
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--claude-bg);
  border: 1px solid var(--claude-line);
  font-size: 12px;
  color: var(--claude-text);
  white-space: nowrap;
}

.claude.quiet {
  background: transparent;
  border-color: var(--line-strong);
  color: var(--text-muted);
}

.busy {
  color: var(--text-muted);
  display: inline-flex;
}

/* The project looked at last, said quietly at its row's end. */
.looked {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

/* Why a card wasn't added or given, or an image not taken. */
.refusal {
  flex-shrink: 0;
  margin: 0;
  padding: 0 16px 10px;
  font-size: 12px;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.enter {
  flex-shrink: 0;
  width: 18px;
  text-align: center;
  font-size: 14px;
  color: var(--text-muted);
}

.empty {
  flex-shrink: 0;
  padding: 26px 20px;
  text-align: center;
  color: var(--text-muted);
}

.empty .hint {
  display: block;
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-subtle);
}

.folder-icon {
  width: 16px;
  display: inline-flex;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-muted);
}

/* A link's project before its name, quieter. */
.link-project {
  font-weight: 400;
  color: var(--text-muted);
}

/* Commands: the heading under the projects, a state mark where projects have their dot. */
.group {
  padding: 8px 12px 4px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
}

.cmd-mark {
  width: 16px;
  display: inline-flex;
  justify-content: center;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-faint);
}

.cmd-mark.running {
  color: var(--run);
  font-size: 10px;
}

.cmd-mark.ok {
  color: var(--run);
}

.cmd-mark.failed {
  color: var(--crash-text);
}

.cmd-mark.busy {
  color: var(--text-muted);
}

/* "Press ↵ again": the command asks before it runs. */
.meta.confirm {
  font-family: inherit;
  color: var(--claude-soft);
}

.uptime {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}

/* A filter's count, as in the window's sidebar. */
.count {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
}

.count.hot {
  color: var(--claude);
  font-weight: 700;
}

.foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 16px;
  height: 38px;
  padding: 0 16px;
  border-top: 1px solid var(--line);
  background: var(--bg-footer);
  font-size: 12px;
  color: var(--text-subtle);
}

.foot span {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

/* Under a card's note it stays at the window's foot while the note's open project list makes the
   window taller than the note. */
.foot-note {
  margin-top: auto;
}

/* Windows and Linux spell the keys out (Ctrl+Enter), which takes room: each hint stays on one
   line, and those that don't fit drop off the end (they wrap onto a line out of sight). */
:root:not([data-platform="mac"]) .foot {
  flex-wrap: wrap;
  gap: 0 12px;
  overflow: hidden;
}

:root:not([data-platform="mac"]) .foot .grow {
  order: 1;
}

:root:not([data-platform="mac"]) .foot span {
  height: 100%;
  white-space: nowrap;
}

.grow {
  flex-grow: 1;
}
</style>
