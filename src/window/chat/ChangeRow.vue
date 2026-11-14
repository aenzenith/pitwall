<script setup lang="ts">
import { emitTo } from "@tauri-apps/api/event";
import { computed, ref, shallowRef, watch } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { openFile, projectFiles, revealInTree, showFiles } from "../../lib/explorer";
import type { ChangeEntry } from "../../lib/chat/store";
import { changeMeta, diffLines, relPath } from "../../lib/chat/tools";
import { highlight } from "../../lib/highlight";
import { t } from "../../lib/i18n";
import { languageOf } from "../../lib/languages";
import { api } from "../../lib/store";
import { resolvedTheme } from "../../lib/theme";
import { parseDiff, type DiffLine } from "../../lib/unifiedDiff";
import DiffView from "../files/DiffView.vue";
import ChatIcon from "./ChatIcon.vue";

/**
 * A file of Değişiklikler (TS3): its path from the project's folder and what the session did to it
 * in all ("+3 −1", "yeni +18"); opened, each call's diff as the Files window draws one, coloured as
 * the file is. Hovered, a soft overlay slides in from the right: "Editörde aç" at the first line
 * changed, "Dosyalar'da göster" in the Files window. "Kayıttan beri" shows the file's whole change
 * since the last commit instead (Git's own diff). `bare`: its opened part alone, under the call's
 * own row in the conversation, the two ways out at the end of its switch's line.
 */
const props = defineProps<{ path: string; file: string; changes: ChangeEntry[]; open: boolean; bare?: boolean }>();
const emit = defineEmits<{ toggle: [] }>();

const rel = computed(() => relPath(props.path, props.file));
const inside = computed(() => rel.value !== props.file);
const name = computed(() => rel.value.slice(rel.value.lastIndexOf("/") + 1));
const folder = computed(() => (rel.value.includes("/") ? rel.value.slice(0, rel.value.lastIndexOf("/") + 1) : ""));

const sum = computed(() => {
  const made = props.changes[0]?.kind === "create";
  const done = props.changes.filter((each) => each.state !== "failed");
  let added = done.reduce((all, each) => all + each.added, 0);
  let removed = done.reduce((all, each) => all + each.removed, 0);
  // A file made in this session: its lines now, whatever its edits after did.
  if (made) {
    added = Math.max(0, added - removed);
    removed = 0;
  }
  const running = props.changes.some((each) => each.state === "running");
  const failed = props.changes.every((each) => each.state === "failed");
  return { meta: changeMeta({ kind: made ? "create" : "edit", added, removed, state: running ? "running" : failed ? "failed" : "done" }), running, failed, added };
});

/* ---------- the diffs ---------- */

type Drawn = { key: string; lines: DiffLine[]; palette: string[]; tokens: number[][] | null };
const drawn = shallowRef<Drawn[]>([]);
let run = 0;

async function draw(): Promise<void> {
  const mine = ++run;
  const lang = languageOf(rel.value);
  const dark = resolvedTheme.value === "dark";
  const out: Drawn[] = [];
  for (const change of props.changes) {
    if (!change.hunks.length) continue;
    const lines = diffLines(change.hunks);
    out.push({ key: change.key, lines, palette: [], tokens: null });
  }
  drawn.value = out;
  if (!lang) return;
  // Each call's lines as they are now, coloured together, put back by their line numbers.
  const coloured: Drawn[] = [];
  for (const each of out) {
    const now = each.lines.filter((line) => line.new !== null);
    const result = await highlight(now.map((line) => line.text).join("\n"), lang, dark);
    if (mine !== run) return;
    const tokens: number[][] = [];
    if (result.lines) now.forEach((line, i) => (tokens[line.new! - 1] = result.lines![i] ?? []));
    coloured.push({ ...each, palette: result.palette, tokens: result.lines ? tokens : null });
  }
  drawn.value = coloured;
}

watch(
  () => [props.open, props.changes.map((each) => `${each.key}:${each.state}:${each.hunks.length}`).join(","), resolvedTheme.value] as const,
  ([open]) => {
    if (open) void draw();
  },
  { immediate: true },
);

/** "Kayıttan beri": the file's change since the last commit, as Git writes it. */
const sinceCommit = ref(false);
const gitDiff = shallowRef<DiffLine[] | null | undefined>(undefined);
const gitBusy = ref(false);

async function toggleSince(): Promise<void> {
  sinceCommit.value = !sinceCommit.value;
  if (!sinceCommit.value) return;
  gitBusy.value = true;
  try {
    const text = await api.explorerDiff(props.path, rel.value);
    gitDiff.value = text ? parseDiff(text).lines : null;
  } catch {
    gitDiff.value = null;
  } finally {
    gitBusy.value = false;
  }
}

/* ---------- the overlay's two ways out ---------- */

const firstLine = computed(() => props.changes.find((each) => each.state === "done")?.line ?? 1);

/** Only a file of the project's own folder opens (the core refuses any other). */
function openInEditor(line = firstLine.value): void {
  if (!inside.value) return;
  void api.openReference(props.path, rel.value, line, null, true).catch(() => false);
}

function showInFiles(): void {
  if (!inside.value) return;
  // In the terminals' own window there are no Files windows: Pitwall's window shows it.
  if (document.documentElement.dataset.surface === "terminals") {
    void emitTo("main", "reveal-files", { path: props.path, file: rel.value });
    void api.openWindow();
    return;
  }
  showFiles(props.path);
  const files = projectFiles(props.path);
  revealInTree(files, rel.value);
  openFile(files, rel.value, true);
}

</script>

<template>
  <div :class="['change-row', { open, bare }]">
    <div v-if="!bare" class="line-wrap">
      <button type="button" class="line" :aria-expanded="open" :title="file" @click="emit('toggle')">
        <span :class="['chev', { down: open }]"><Icon name="chevron" :size="11" /></span>
        <span class="icon"><Spinner v-if="sum.running" :size="10" /><ChatIcon v-else name="file" :size="13" /></span>
        <span class="name" data-find><span class="folder">{{ folder }}</span>{{ name }}</span>
        <span :class="['meta', { add: sum.added && !sum.failed, bad: sum.failed }]">{{ sum.meta }}</span>
      </button>
      <!-- The ways out of the row, sliding in over its right end while it is hovered. -->
      <div v-if="inside" class="overlay" data-nofind>
        <button type="button" class="out" @click="openInEditor()"><Icon name="editor" :size="12" />{{ t("chat.change.openEditor") }}</button>
        <button type="button" class="out" @click="showInFiles"><Icon name="files" :size="12" />{{ t("chat.change.showFiles") }}</button>
      </div>
    </div>
    <Collapse :show="bare || open">
      <div class="body">
        <div v-if="inside" class="bar">
          <div class="since">
            <button type="button" :class="['seg', { on: !sinceCommit }]" :aria-pressed="!sinceCommit" @click="sinceCommit && toggleSince()">{{ t("chat.change.thisSession") }}</button>
            <button type="button" :class="['seg', { on: sinceCommit }]" :aria-pressed="sinceCommit" @click="!sinceCommit && toggleSince()">{{ t("chat.change.sinceCommit") }}</button>
          </div>
          <template v-if="bare">
            <span class="grow"></span>
            <button type="button" class="out" @click="openInEditor()"><Icon name="editor" :size="12" />{{ t("chat.change.openEditor") }}</button>
            <button type="button" class="out" @click="showInFiles"><Icon name="files" :size="12" />{{ t("chat.change.showFiles") }}</button>
          </template>
        </div>
        <template v-if="sinceCommit">
          <div v-if="gitBusy" class="none"><Spinner :size="11" /></div>
          <div v-else-if="!gitDiff" class="none">{{ t("chat.change.noGit") }}</div>
          <DiffView v-else class="diff" :lines="gitDiff" :palette="[]" :tokens="null" @line="openInEditor" />
        </template>
        <template v-else>
          <div v-if="!drawn.length" class="none">{{ sum.running ? t("chat.change.running") : t("chat.change.noDiff") }}</div>
          <DiffView v-for="each in drawn" :key="each.key" class="diff" :lines="each.lines" :palette="each.palette" :tokens="each.tokens" @line="openInEditor" />
        </template>
      </div>
    </Collapse>
  </div>
</template>

<style scoped>
.change-row {
  border-bottom: 1px solid var(--line-soft);
}

/* Under its row in the conversation, in that row's box: no rule of its own. */
.change-row.bare {
  border-bottom: 0;
}

.line-wrap {
  position: relative;
  overflow: hidden;
}

.line {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  min-width: 0;
  height: 30px;
  padding: 0 12px 0 6px;
  border: 0;
  background: none;
  color: var(--text);
  font: inherit;
  text-align: left;
}

.line-wrap:hover .line {
  background: var(--bg-row-hover);
}

.line:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.chev {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-faint);
  transition: transform 0.15s;
}

.chev.down {
  transform: rotate(90deg);
}

.icon {
  display: inline-flex;
  width: 13px;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-muted);
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: ltr;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-strong);
}

.folder {
  color: var(--text-subtle);
}

.meta {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.meta.add {
  color: var(--run-text);
}

.meta.bad {
  color: var(--crash-text);
}

/* A soft ground sliding in from the right over the row's end, plain words on it. */
.overlay {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 6px 0 48px;
  background: linear-gradient(to right, transparent, var(--bg-pane-head) 40px);
  transform: translateX(100%);
  opacity: 0;
  transition:
    transform 0.16s ease,
    opacity 0.16s ease;
  pointer-events: none;
}

.line-wrap:hover .overlay,
.overlay:focus-within {
  transform: none;
  opacity: 1;
  pointer-events: auto;
}

.out {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 7px;
  border: 0;
  border-radius: 5px;
  background: none;
  color: var(--text-strong);
  font: inherit;
  font-size: 12px;
  white-space: nowrap;
}

.out:hover {
  background: var(--bg-control-hover);
}

.body {
  padding: 0 0 8px;
}

.bar {
  display: flex;
  align-items: center;
  gap: 2px;
  margin: 2px 8px 6px 26px;
}

.bare .bar {
  margin: 2px 8px 6px 32px;
}

.grow {
  flex: 1;
}

/* Plain words, quieter than the overlay's, lit when hovered. */
.bare .out {
  color: var(--text-muted);
}

.bare .out:hover {
  color: var(--text-strong);
}

.since {
  display: inline-flex;
  padding: 2px;
  border-radius: 6px;
  background: var(--bg-seg);
}

.seg {
  height: 20px;
  padding: 0 8px;
  border: 0;
  border-radius: 5px;
  background: none;
  color: var(--text-subtle);
  font: inherit;
  font-size: 11px;
}

.seg.on {
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
  color: var(--text-strong);
}

.none {
  display: flex;
  padding: 2px 0 4px 26px;
  font-size: 12px;
  color: var(--text-faint);
}

/* The Files window's diff, at the terminal panel's size, as tall as it is. */
.diff {
  flex: none;
  max-height: 360px;
  margin: 0 8px 4px;
  border-radius: 6px;
  font-size: 12px;
  line-height: 18px;
}

.diff :deep(.lines) {
  padding: 2px 0 4px;
}

.diff :deep(.ln) {
  height: 18px;
}

.bare .none {
  padding-left: 32px;
}

.bare .diff {
  margin: 0 10px 4px 32px;
}

.bare .body {
  padding-bottom: 6px;
}
</style>
