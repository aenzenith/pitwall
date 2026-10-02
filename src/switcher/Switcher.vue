<script setup lang="ts">
import { LogicalSize } from "@tauri-apps/api/dpi";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Marquee from "../components/Marquee.vue";
import Spinner from "../components/Spinner.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeState, gitLine, meta, uptime } from "../lib/format";
import { t } from "../lib/i18n";
import { api, now, snapshot } from "../lib/store";
import type { CommandView, Folder, Project } from "../lib/types";

const WIDTH = 640;
const MAX_HEIGHT = 540;

type Match = { project: Project; score: number; hits: Set<number> };
type FolderMatch = { folder: Folder; score: number; hits: Set<number> };
type CommandMatch = { project: Project; command: CommandView; score: number; hits: Set<number> };

/** One row of the list: a project, a project's command, or a folder from the projects folder. */
type Item =
  | { kind: "project"; key: string; project: Project; hits: Set<number> }
  | { kind: "command"; key: string; project: Project; command: CommandView; hits: Set<number> }
  | { kind: "folder"; key: string; folder: Folder; hits: Set<number> };

/** Commands shown under the projects while typing; `>` lists them all. */
const MIXED_COMMANDS = 6;

const query = ref("");
const index = ref(0);
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

/** The window went away (blur, Esc, a pick): be in the hidden state for the next opening. */
function conceal(): void {
  shown.value = false;
}

/** Fuzzy: every query character in order; runs, word starts and prefixes score higher. */
function fuzzy(text: string, q: string): { score: number; hits: number[] } | null {
  const lower = text.toLowerCase();
  const hits: number[] = [];
  let from = 0;
  let last = -2;
  let score = 0;

  for (const ch of q) {
    const i = lower.indexOf(ch, from);
    if (i < 0) return null;
    score += i === last + 1 ? 3 : 1;
    if (i === 0 || /[-_ ./]/.test(lower[i - 1])) score += 2;
    hits.push(i);
    last = i;
    from = i + 1;
  }

  if (lower.startsWith(q)) score += 8;
  return { score: score - lower.length * 0.01, hits };
}

function rank(p: Project): number {
  if (p.claude) return 0;
  if (p.claudeWorking) return 1;
  if (p.status === "running" || p.status === "busy") return 2;
  return 3;
}

const matches = computed<Match[]>(() => {
  const projects = snapshot.value?.projects ?? [];
  const q = query.value.trim().toLowerCase();

  if (!q) {
    return [...projects]
      .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
      .map((project) => ({ project, score: 0, hits: new Set<number>() }));
  }

  const found: Match[] = [];

  for (const project of projects) {
    const byName = fuzzy(project.name, q);
    const byBranch = project.git ? fuzzy(project.git.branch, q) : null;
    const byPath = fuzzy(project.path, q);
    const best = Math.max(byName?.score ?? -Infinity, (byBranch?.score ?? -Infinity) * 0.6, (byPath?.score ?? -Infinity) * 0.4);

    if (best > -Infinity) {
      found.push({ project, score: best - rank(project) * 0.5, hits: new Set(byName?.hits ?? []) });
    }
  }

  return found.sort((a, b) => b.score - a.score);
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

/** `>` at the start lists only commands, as in VS Code's command palette. */
const commandMode = computed(() => query.value.trimStart().startsWith(">"));
const needle = computed(() => (commandMode.value ? query.value.trimStart().slice(1) : query.value).trim().toLowerCase());
const anyCommands = computed(() => (snapshot.value?.projects ?? []).some((p) => p.commands.length));

/**
 * Every project's commands that match: by the command's name, by project and name together
 * ("cog mig"), or by the command line. Running ones first among equals; with `>` and nothing
 * typed, all of them in list order.
 */
const commandMatches = computed<CommandMatch[]>(() => {
  const q = needle.value;
  if (!commandMode.value && !q) return [];

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

/** What the list shows: with `>` commands only; else projects, then matching commands; else folders. */
const items = computed<Item[]>(() => {
  const commands: Item[] = commandMatches.value.map(({ project, command, hits }) => ({ kind: "command", key: `c:${project.path}:${command.id}`, project, command, hits }));
  if (commandMode.value) return commands;

  const projects: Item[] = matches.value.map(({ project, hits }) => ({ kind: "project", key: `p:${project.path}`, project, hits }));
  if (projects.length || commands.length) return [...projects, ...commands];

  return folderMatches.value.map(({ folder, hits }) => ({ kind: "folder", key: `f:${folder.path}`, folder, hits }));
});

const count = computed(() => items.value.length);
const current = computed<Item | null>(() => items.value[index.value] ?? null);
const selected = computed(() => (current.value?.kind === "project" ? current.value.project : null));
/** A command marked "ask first" waits here for a second ↵. */
const confirming = ref<string | null>(null);

async function loadFolders(): Promise<void> {
  folders.value = await api.projectFolders();
}


function segments(name: string, hits: Set<number>): Array<{ text: string; hit: boolean }> {
  return [...name].map((ch, i) => ({ text: ch, hit: hits.has(i) }));
}

function detail(p: Project): string {
  return [meta(p, now.value), gitLine(p.git)].filter(Boolean).join("  ·  ");
}

function open(p: Project | null): void {
  if (!p) return;
  void (p.claude ? api.openClaude(p.path) : api.openEditor(p.path));
}

function toggle(p: Project | null): void {
  if (!p || p.status === "busy") return;
  void api.act(p.path, p.status === "running" ? "stop" : "start");
}

/** A folder from the projects folder: ↵ opens it in the editor, ⌘↵ adds it to Pitwall. */
function openFolder(folder: Folder | null, add: boolean): void {
  if (!folder) return;
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

/** A click does what ↵ does. */
function pick(item: Item): void {
  if (item.kind === "project") open(item.project);
  else if (item.kind === "command") runCommand(item, false);
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
    if (total) index.value = (index.value + 1) % total;
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    if (total) index.value = (index.value - 1 + total) % total;
  } else if (event.key === "Enter") {
    event.preventDefault();
    const item = current.value;
    if (!item) return;
    if (item.kind === "folder") openFolder(item.folder, event.metaKey);
    else if (item.kind === "command") runCommand(item, event.metaKey);
    else if (event.metaKey) toggle(item.project);
    else open(item.project);
  } else if (event.metaKey && event.key.toLowerCase() === "b" && selected.value) {
    // The browser takes focus; the switcher closes on its own as it loses it.
    event.preventDefault();
    void api.openBrowser(selected.value.path);
  } else if (event.metaKey && event.code === "KeyP") {
    // ⌘P: open Pitwall's window; the switcher closes as the window takes focus.
    event.preventDefault();
    void api.openWindow();
  }
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
  index.value = 0;
  confirming.value = null;
});
watch(index, async () => {
  confirming.value = null;
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
    index.value = 0;
    void loadFolders();
    await fitWindow();
    reveal();
    input.value?.focus();
  });
  void fitWindow();
  void loadFolders();
  input.value?.focus();
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
        :aria-expanded="count > 0"
      />
      <kbd class="esc">esc</kbd>
    </div>

    <ul v-if="items.length" id="results" ref="list" class="list" role="listbox" :aria-label="t(commandMode ? 'common.commands' : 'common.projects')">
      <template v-for="(item, i) in items" :key="item.key">
        <!-- Under the projects, the commands that match get their own heading. -->
        <li v-if="item.kind === 'command' && !commandMode && i > 0 && items[i - 1].kind !== 'command'" class="group" role="presentation">
          {{ t("common.commands") }}
        </li>
        <li :class="['row', { on: i === index }]" role="option" :aria-selected="i === index" @mousemove="index = i" @click="pick(item)">
          <template v-if="item.kind === 'project'">
            <StatusIcon :project="item.project" :ring="i === index ? '#23272f' : 'var(--bg-panel)'" />
            <div class="text">
              <div class="name">
                <span v-for="(seg, j) in segments(item.project.name, item.hits)" :key="j" :class="{ hit: seg.hit }">{{ seg.text }}</span>
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
                <span v-for="(seg, j) in segments(item.command.name, item.hits)" :key="j" :class="{ hit: seg.hit }">{{ seg.text }}</span>
              </div>
              <div v-if="confirming === item.key" class="meta confirm">{{ t("switcher.confirmAgain", { name: item.command.name }) }}</div>
              <div v-else class="meta"><Marquee :text="`${item.project.name}  ·  ${item.command.command}`" /></div>
            </div>
            <span v-if="item.command.status === 'running'" class="uptime">{{ uptime(item.command.startedAt, now) }}</span>
          </template>

          <template v-else>
            <span class="folder-icon"><Icon name="folder" :size="16" /></span>
            <div class="text">
              <div class="name">
                <span v-for="(seg, j) in segments(item.folder.name, item.hits)" :key="j" :class="{ hit: seg.hit }">{{ seg.text }}</span>
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
      <template v-else-if="query.trim() && projectsDir">{{ t("switcher.noMatchAnywhere", { folder: dirName, query }) }}</template>
      <template v-else-if="query.trim()">
        {{ t("switcher.noMatch", { query }) }}
        <span class="hint">{{ t("switcher.chooseFolderHint") }}</span>
      </template>
      <template v-else>{{ t("switcher.noProjects") }}</template>
    </div>

    <footer v-if="current?.kind === 'folder'" class="foot">
      <span><kbd>↵</kbd> {{ t("common.openInEditor") }}</span>
      <span><kbd>⌘↵</kbd> {{ t("switcher.addToPitwall") }}</span>
      <span class="grow"></span>
      <span><kbd>⌘P</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else-if="current?.kind === 'command'" class="foot">
      <span><kbd>↵</kbd> {{ t(current.command.status === "running" ? "common.stop" : "common.run") }}</span>
      <span><kbd>⌘↵</kbd> {{ t(current.command.status === "running" ? "switcher.showOutput" : "switcher.runAndShow") }}</span>
      <span class="grow"></span>
      <span><kbd>⌘P</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else class="foot">
      <span><kbd>↵</kbd> {{ t(selected?.claude ? "switcher.openMarkSeen" : "common.openInEditor") }}</span>
      <span><kbd>⌘↵</kbd> {{ t(selected?.status === "running" ? "common.stop" : "common.start") }}</span>
      <span><kbd>⌘B</kbd> {{ t("switcher.browser") }}</span>
      <span v-if="anyCommands"><kbd>&gt;</kbd> {{ t("common.commands") }}</span>
      <span class="grow"></span>
      <span><kbd>⌘P</kbd> {{ t("switcher.openPitwall") }}</span>
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
  color: #6c727c;
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

.name .hit {
  color: var(--run);
  font-weight: 600;
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
  color: #6c727c;
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

.grow {
  flex-grow: 1;
}
</style>
