<script setup lang="ts">
import { computed, shallowRef, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import { pickCommitFile, type ProjectFiles } from "../../lib/explorer";
import { highlight } from "../../lib/highlight";
import { language, t } from "../../lib/i18n";
import { languageOf } from "../../lib/languages";
import { resolvedTheme } from "../../lib/theme";
import DiffView from "./DiffView.vue";
import FileIcon from "./FileIcon.vue";

/**
 * A commit of the history in a tab of its own: its message's first line, who wrote it and
 * when, the files it changed, and the picked one's change against the commit's first parent.
 */
const props = defineProps<{ files: ProjectFiles; id: string }>();

const opened = computed(() => props.files.commits.get(props.id));
const info = computed(() => props.files.history?.commits.find((c) => c.id === props.id) ?? null);
const when = computed(() => (info.value ? new Intl.DateTimeFormat(language.value, { dateStyle: "medium", timeStyle: "short" }).format(new Date(info.value.time * 1000)) : ""));

const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);
const folder = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
const TONE = { M: "m-mod", R: "m-mod", A: "m-new", D: "m-bad" } as const;

/* The change's colours: the lines of the file as the commit left it, coloured as one text and
   handed back to their numbers (the diff view looks a line's colours up by its new number). */
const coloured = shallowRef<{ palette: string[]; tokens: number[][] } | null>(null);
watch(
  [() => opened.value?.diff, () => opened.value?.picked, () => resolvedTheme.value],
  async ([diff, picked, theme]) => {
    coloured.value = null;
    if (!diff || !picked) return;
    const lang = languageOf(picked);
    const kept = diff.lines.filter((line) => line.new !== null);
    if (!lang || !kept.length) return;
    const result = await highlight(kept.map((line) => line.text).join("\n"), lang, theme === "dark");
    if (opened.value?.diff !== diff || !result.lines) return;
    const tokens: number[][] = [];
    kept.forEach((line, i) => (tokens[line.new! - 1] = result.lines![i] ?? []));
    coloured.value = { palette: result.palette, tokens };
  },
  { immediate: true },
);
</script>

<template>
  <div class="commit-view">
    <header class="head">
      <div class="subject">{{ info?.subject ?? id }}</div>
      <div class="meta">
        <span class="mono">{{ id.slice(0, 7) }}</span>
        <template v-if="info"><span>·</span><span>{{ info.author }}</span><span>·</span><span>{{ when }}</span></template>
      </div>
    </header>

    <div v-if="opened?.files === undefined" class="state"><Spinner /></div>
    <div v-else-if="opened.files === null" class="state">{{ t("files.unreadable") }}</div>
    <template v-else>
      <div class="list" role="listbox" :aria-label="t('scm.commitFiles', { count: opened.files.length })">
        <div
          v-for="file in opened.files"
          :key="file.path"
          role="option"
          :aria-selected="file.path === opened.picked"
          :class="['row', TONE[file.mark], { on: file.path === opened.picked }]"
          :title="file.from ? `${file.from} → ${file.path}` : file.path"
          @click="pickCommitFile(files, id, file.path)"
        >
          <FileIcon :path="file.path" class="fico" />
          <span class="name">{{ name(file.path) }}</span>
          <span class="dir">{{ folder(file.path) }}</span>
          <span class="letter">{{ file.mark }}</span>
        </div>
      </div>
      <div v-if="opened.picked && opened.diff === undefined" class="state"><Spinner /></div>
      <DiffView v-else-if="opened.diff" :lines="opened.diff.lines" :palette="coloured?.palette ?? []" :tokens="coloured?.tokens ?? null" />
    </template>
  </div>
</template>

<style scoped>
.commit-view {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-log);
}

.head {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 14px 18px 12px;
}

.subject {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-strong);
  -webkit-user-select: text;
  user-select: text;
}

.meta {
  display: flex;
  gap: 6px;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.mono {
  font-family: var(--font-mono);
}

.list {
  flex-shrink: 0;
  max-height: 38%;
  overflow: auto;
  padding: 0 10px 8px;
  border-bottom: 1px solid var(--line-soft);
}

.row {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 24px;
  padding: 0 8px;
  border-radius: 5px;
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
}

.row:hover {
  background: var(--bg-row-hover);
}

.row.on {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

/* A file's own icon (FileIcon, 16 px): the name where it was beside the 13 px glyph. */
.fico {
  margin: 0 -1px 0 -2px;
}

.dir {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
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

.m-bad .name {
  color: var(--crash-text);
  text-decoration: line-through;
}

.m-bad .letter {
  color: var(--crash);
}

.state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12.5px;
  color: var(--text-faint);
}
</style>
