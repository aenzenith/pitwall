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
import { bareUrl, claudeState, gitLine, meta, uptime } from "../lib/format";
import { byRank, fuzzy, searchProjects, type ProjectMatch } from "../lib/fuzzy";
import { t } from "../lib/i18n";
import { keys, primary } from "../lib/platform";
import { api, now, snapshot } from "../lib/store";
import type { CommandView, Folder, Project, ProjectLink } from "../lib/types";

const WIDTH = 640;
const MAX_HEIGHT = 540;

type FolderMatch = { folder: Folder; score: number; hits: Set<number> };
type CommandMatch = { project: Project; command: CommandView; score: number; hits: Set<number> };
type LinkMatch = { project: Project; link: ProjectLink; score: number; hits: Set<number> };

/** One row of the list: a project, a project's command or link, or a folder from the projects folder. */
type Item =
  | { kind: "project"; key: string; project: Project; hits: Set<number> }
  | { kind: "command"; key: string; project: Project; command: CommandView; hits: Set<number> }
  | { kind: "link"; key: string; project: Project; link: ProjectLink; hits: Set<number> }
  | { kind: "folder"; key: string; folder: Folder; hits: Set<number> };

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

/** `>` at the start lists only commands, as in VS Code's command palette; `@` only links. */
const commandMode = computed(() => query.value.trimStart().startsWith(">"));
const linkMode = computed(() => query.value.trimStart().startsWith("@"));
const needle = computed(() => (commandMode.value || linkMode.value ? query.value.trimStart().slice(1) : query.value).trim().toLowerCase());
const anyCommands = computed(() => (snapshot.value?.projects ?? []).some((p) => p.commands.length));
const anyLinks = computed(() => (snapshot.value?.projects ?? []).some((p) => p.settings.links?.length));

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

/** What the list shows: with `>` commands only, with `@` links only; else projects, then matching
 * commands and links; else folders. */
const items = computed<Item[]>(() => {
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

function open(p: Project | null): void {
  if (!p) return;
  conceal();
  void (p.claude ? api.openClaude(p.path) : api.openEditor(p.path));
}

function toggle(p: Project | null): void {
  if (!p || p.status === "busy") return;
  void api.act(p.path, p.status === "running" ? "stop" : "start");
}

/** A folder from the projects folder: ↵ opens it in the editor, ⌘↵ (Ctrl+Enter) adds it to Pitwall. */
function openFolder(folder: Folder | null, add: boolean): void {
  if (!folder) return;
  if (!add) conceal();
  void (add ? api.addProject(folder.path) : api.openEditor(folder.path));
}

function mark(command: CommandView): string {
  return { running: "●", busy: "", ok: "✓", failed: "✕", idle: "·" }[command.status];
}

/** Pitwall's window, on the project, showing this command's output. */
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

/** A click does what ↵ does. */
function pick(item: Item): void {
  if (item.kind === "project") open(item.project);
  else if (item.kind === "command") runCommand(item, false);
  else if (item.kind === "link") openLink(item, false);
  else openFolder(item.folder, false);
}

function onKey(event: KeyboardEvent): void {
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
  const natural = Math.ceil(parts.reduce((sum, part) => sum + (part === list.value ? part.scrollHeight : part.offsetHeight), 2));
  await getCurrentWindow().setSize(new LogicalSize(WIDTH, Math.min(natural, MAX_HEIGHT)));
}

watch(query, () => {
  select(0);
  confirming.value = null;
});
// Another row: a pending "run?" is taken back.
watch(selectedKey, () => (confirming.value = null));
watch(index, async () => {
  await nextTick();
  list.value?.querySelector(".row.on")?.scrollIntoView({ block: "nearest" });
});
watch(count, () => void fitWindow());

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("blur", conceal);
  unlisten = await listen("switcher-opened", async () => {
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
  <div ref="panel" :class="['panel', { shown }]">
    <div class="search">
      <PitwallGlyph :size="20" lamps="var(--run)" class="glyph" />
      <label class="sr" for="q">{{ t("switcher.label") }}</label>
      <input
        id="q"
        ref="input"
        v-model="query"
        :placeholder="t('switcher.placeholder')"
        autocomplete="off"
        spellcheck="false"
        role="combobox"
        aria-controls="results"
        aria-autocomplete="list"
        :aria-expanded="count > 0"
        :aria-activedescendant="current ? optionId(current) : undefined"
      />
      <kbd class="esc">{{ keys("Escape") }}</kbd>
    </div>

    <ul
      v-if="items.length"
      id="results"
      ref="list"
      class="list"
      role="listbox"
      :aria-label="t(commandMode ? 'common.commands' : linkMode ? 'common.links' : 'common.projects')"
    >
      <template v-for="(item, i) in items" :key="item.key">
        <!-- Under the projects, the commands that match get their own heading. -->
        <li v-if="item.kind === 'command' && !commandMode && i > 0 && items[i - 1].kind !== 'command'" class="group" role="presentation">
          {{ t("common.commands") }}
        </li>
        <li v-if="item.kind === 'link' && !linkMode && i > 0 && items[i - 1].kind !== 'link'" class="group" role="presentation">
          {{ t("common.links") }}
        </li>
        <li
          :id="optionId(item)"
          :class="['row', { on: i === index }]"
          role="option"
          :aria-selected="i === index"
          @mousemove="i !== index && select(i)"
          @click="pick(item)"
        >
          <template v-if="item.kind === 'project'">
            <StatusIcon :project="item.project" :ring="i === index ? '#23272f' : 'var(--bg-panel)'" />
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

          <template v-else-if="item.kind === 'command'">
            <span :class="['cmd-mark', item.command.status]">
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
      <template v-else-if="query.trim() && projectsDir">{{ t("switcher.noMatchAnywhere", { folder: dirName, query }) }}</template>
      <template v-else-if="query.trim()">
        {{ t("switcher.noMatch", { query }) }}
        <span class="hint">{{ t("switcher.chooseFolderHint") }}</span>
      </template>
      <template v-else>{{ t("switcher.noProjects") }}</template>
    </div>

    <footer v-if="current?.kind === 'folder'" class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t("common.openInEditor") }}</span>
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
    <footer v-else class="foot">
      <span><kbd>{{ keys("Enter") }}</kbd> {{ t(selected?.claude ? "switcher.openMarkSeen" : "common.openInEditor") }}</span>
      <span><kbd>{{ keys("mod+Enter") }}</kbd> {{ t(selected?.status === "running" ? "common.stop" : "common.start") }}</span>
      <span><kbd>{{ keys("mod+B") }}</kbd> {{ t("switcher.browser") }}</span>
      <span v-if="anyCommands" class="prefix"><kbd>&gt;</kbd> {{ t("common.commands") }}</span>
      <span v-if="anyLinks" class="prefix"><kbd>@</kbd> {{ t("common.links") }}</span>
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
  border: 1px solid var(--line-strong);
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
  color: #d7dae0;
  flex-shrink: 0;
}

.sr {
  position: absolute;
  left: -9999px;
}

input {
  flex-grow: 1;
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
  background: #1b1e24;
}

.esc {
  flex-shrink: 0;
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
  background: #23272f;
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
  color: #eef0f3;
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
  color: #f3c29b;
}

.uptime {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
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

/* Windows and Linux spell the keys out (Ctrl+Enter), which takes room: each hint stays on one
   line, the keys first and the search prefixes (> @) last, and those that don't fit drop off the
   end (they wrap onto a line out of sight). */
:root:not([data-platform="mac"]) .foot {
  flex-wrap: wrap;
  gap: 0 12px;
  overflow: hidden;
}

:root:not([data-platform="mac"]) .foot .prefix {
  order: 1;
}

:root:not([data-platform="mac"]) .foot .grow {
  order: 2;
}

:root:not([data-platform="mac"]) .foot span {
  height: 100%;
  white-space: nowrap;
}

.grow {
  flex-grow: 1;
}
</style>
