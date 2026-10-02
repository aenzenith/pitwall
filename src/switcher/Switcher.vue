<script setup lang="ts">
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Marquee from "../components/Marquee.vue";
import Spinner from "../components/Spinner.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeState, gitLine, meta } from "../lib/format";
import { t } from "../lib/i18n";
import { api, now, snapshot } from "../lib/store";
import type { Folder, Project } from "../lib/types";

const WIDTH = 640;
const MAX_HEIGHT = 540;

type Match = { project: Project; score: number; hits: Set<number> };
type FolderMatch = { folder: Folder; score: number; hits: Set<number> };

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

const count = computed(() => matches.value.length || folderMatches.value.length);
const selected = computed(() => matches.value[index.value]?.project ?? null);
const selectedFolder = computed(() => (matches.value.length ? null : (folderMatches.value[index.value]?.folder ?? null)));

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

function onKey(event: KeyboardEvent): void {
  const total = count.value;

  if (event.key === "Escape") {
    event.preventDefault();
    void api.hideSwitcher();
  } else if (event.key === "ArrowDown") {
    event.preventDefault();
    if (total) index.value = (index.value + 1) % total;
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    if (total) index.value = (index.value - 1 + total) % total;
  } else if (event.key === "Enter") {
    event.preventDefault();
    if (selectedFolder.value) openFolder(selectedFolder.value, event.metaKey);
    else if (event.metaKey) toggle(selected.value);
    else open(selected.value);
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

watch(query, () => (index.value = 0));
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

    <ul v-if="matches.length" id="results" ref="list" class="list" role="listbox" :aria-label="t('common.projects')">
      <li
        v-for="(match, i) in matches"
        :key="match.project.path"
        :class="['row', { on: i === index }]"
        role="option"
        :aria-selected="i === index"
        @mousemove="index = i"
        @click="open(match.project)"
      >
        <StatusIcon :project="match.project" :ring="i === index ? '#23272f' : 'var(--bg-panel)'" />
        <div class="text">
          <div class="name">
            <span v-for="(seg, j) in segments(match.project.name, match.hits)" :key="j" :class="{ hit: seg.hit }">{{ seg.text }}</span>
          </div>
          <div :class="['meta', { bad: match.project.status === 'crashed' }]"><Marquee :text="detail(match.project)" /></div>
        </div>
        <span v-if="match.project.claude || match.project.claudeWorking" :class="['claude', { quiet: !match.project.claude }]">
          {{ claudeState(match.project, now) }}
        </span>
        <span v-if="match.project.status === 'busy'" class="busy"><Spinner :size="12" /></span>
        <span v-if="i === index" class="enter">↵</span>
      </li>
    </ul>

    <!-- No project matched: folders in the projects folder that aren't projects yet. -->
    <ul v-else-if="folderMatches.length" id="results" ref="list" class="list" role="listbox" :aria-label="t('switcher.foldersIn', { folder: dirName })">
      <li
        v-for="(match, i) in folderMatches"
        :key="match.folder.path"
        :class="['row', { on: i === index }]"
        role="option"
        :aria-selected="i === index"
        @mousemove="index = i"
        @click="openFolder(match.folder, false)"
      >
        <span class="folder-icon"><Icon name="folder" :size="16" /></span>
        <div class="text">
          <div class="name">
            <span v-for="(seg, j) in segments(match.folder.name, match.hits)" :key="j" :class="{ hit: seg.hit }">{{ seg.text }}</span>
          </div>
          <div class="meta">{{ match.folder.path }}</div>
        </div>
        <span v-if="i === index" class="enter">↵</span>
      </li>
    </ul>

    <div v-else class="empty">
      <template v-if="query.trim() && projectsDir">{{ t("switcher.noMatchAnywhere", { folder: dirName, query }) }}</template>
      <template v-else-if="query.trim()">
        {{ t("switcher.noMatch", { query }) }}
        <span class="hint">{{ t("switcher.chooseFolderHint") }}</span>
      </template>
      <template v-else>{{ t("switcher.noProjects") }}</template>
    </div>

    <footer v-if="selectedFolder" class="foot">
      <span><kbd>↵</kbd> {{ t("common.openInEditor") }}</span>
      <span><kbd>⌘↵</kbd> {{ t("switcher.addToPitwall") }}</span>
      <span class="grow"></span>
      <span><kbd>⌘P</kbd> {{ t("switcher.openPitwall") }}</span>
    </footer>
    <footer v-else class="foot">
      <span><kbd>↵</kbd> {{ t(selected?.claude ? "switcher.openMarkSeen" : "common.openInEditor") }}</span>
      <span><kbd>⌘↵</kbd> {{ t(selected?.status === "running" ? "common.stop" : "common.start") }}</span>
      <span><kbd>⌘B</kbd> {{ t("switcher.browser") }}</span>
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
