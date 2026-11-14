<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import Spinner from "../../../components/Spinner.vue";
import { language, t } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import { resolvedTheme } from "../../../lib/theme";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { errorText, projectName, shownPath, useAsk, within, type DialogProps } from "./shared";

/**
 * Plan: the plan the session wrote in plan mode (`get_plan`), drawn as the Files window draws a
 * Markdown file (lib/markdownPreview: cleaned, nothing fetched); a sentence when there is none.
 * A web address in it opens in the browser only when clicked; a picture is named, never loaded.
 * "Open in editor" only for a plan file inside the project; any other is named by its path.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Plan = { exists?: boolean; content?: string; path?: string };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const page = ref<HTMLElement | null>(null);
const plan = useAsk<Plan>(() => props.id);
onMounted(() => void plan.load({ kind: "plan" }));

const content = computed(() => (plan.value.value?.exists !== false ? (plan.value.value?.content ?? "") : ""));
const file = computed(() => plan.value.value?.path ?? "");
const inside = computed(() => (file.value ? within(props.path, file.value) : null));

const html = shallowRef<string | null>(null);
let lib: typeof import("../../../lib/markdownPreview") | null = null;
let run = 0;

async function render(): Promise<void> {
  const mine = ++run;
  if (!content.value.trim()) return void (html.value = null);
  lib ??= await import("../../../lib/markdownPreview");
  if (mine !== run) return;
  html.value = lib.renderMarkdown(content.value, {
    note: t("files.md.note"),
    tip: t("files.md.tip"),
    important: t("files.md.important"),
    warning: t("files.md.warning"),
    caution: t("files.md.caution"),
  });
  await nextTick();
  const box = page.value;
  if (mine !== run || !box) return;
  for (const img of box.querySelectorAll<HTMLImageElement>("img")) {
    const note = document.createElement("span");
    note.className = "md-picture-off";
    const alt = img.getAttribute("alt");
    note.textContent = alt ? `${alt} · ${t("chatDialog.plan.picture")}` : t("chatDialog.plan.picture");
    img.replaceWith(note);
  }
  void lib.drawDiagrams(box, resolvedTheme.value === "dark", t("files.md.diagramFailed"), t("files.md.diagramsCapped", { count: lib.DIAGRAMS }));
}
watch([content, () => resolvedTheme.value, language], render);
onBeforeUnmount(() => run++);

function follow(href: string): void {
  if (/^(https?|mailto):/i.test(href)) return void api.openUrl(href);
  if (!href.startsWith("#")) return;
  const target = page.value?.querySelector<HTMLElement>(`[data-id="${CSS.escape(decodeURIComponent(href.slice(1)))}"]`);
  target?.scrollIntoView({ block: "start" });
}

function onClick(event: MouseEvent): void {
  const target = event.target as HTMLElement;
  // Nothing in the plan takes the app's page anywhere.
  if (target.closest("a, area, [href]")) event.preventDefault();
  const link = target.closest<HTMLElement>("[data-href]");
  if (link && page.value?.contains(link)) follow(link.dataset.href!);
}

function onKey(event: KeyboardEvent): void {
  const link = (event.target as HTMLElement).closest<HTMLElement>("[data-href]");
  if (event.key === "Enter" && link) {
    event.preventDefault();
    follow(link.dataset.href!);
  }
}

const failed = ref<string | null>(null);
async function openInEditor(): Promise<void> {
  if (!inside.value) return;
  failed.value = null;
  try {
    const opened = await api.openReference(props.path, inside.value, 1, null, true);
    if (!opened) failed.value = t("chatDialog.plan.notOpened");
  } catch (reason) {
    failed.value = errorText(reason);
  }
}

const copied = ref(false);
let copiedTimer = 0;
async function copy(): Promise<void> {
  if (!content.value) return;
  failed.value = null;
  try {
    await api.copyText(content.value);
    copied.value = true;
    window.clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => (copied.value = false), 1600);
  } catch (reason) {
    failed.value = errorText(reason);
  }
}
onBeforeUnmount(() => window.clearTimeout(copiedTimer));

const sub = computed(() => (file.value ? shownPath(props.path, file.value) : projectName(props.path)));
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.plan.title')" :sub="sub" :width="640" @close="emit('close')">
    <div v-if="plan.busy.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="plan.error.value" :id="id" :state="state" :error="plan.error.value" @done="frame?.close()" />
    <p v-else-if="!content.trim()" class="state">{{ t("chatDialog.plan.none") }}</p>
    <div v-else ref="page" class="scroll plan selectable" tabindex="0" data-focus :aria-label="t('chatDialog.plan.title')" @click="onClick" @keydown="onKey">
      <div v-if="html === null" class="state"><Spinner :size="14" /></div>
      <!-- Cleaned in lib/markdownPreview (DOMPurify), and the app's CSP runs no script of it. -->
      <!-- eslint-disable-next-line vue/no-v-html -->
      <article v-else class="md-page" v-html="html"></article>
    </div>

    <p v-if="failed" class="error" role="alert">{{ failed }}</p>

    <template #foot>
      <template v-if="content.trim()">
        <button v-if="inside" type="button" class="control" @click="openInEditor">{{ t("common.openInEditor") }}</button>
        <button type="button" class="control" @click="copy">
          <span class="stack">
            <span :class="{ hidden: copied }">{{ t("chatDialog.export.copy") }}</span>
            <span :class="{ hidden: !copied }" aria-live="polite">{{ t("chatDialog.export.copied") }}</span>
          </span>
        </button>
      </template>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.plan {
  max-height: min(520px, 62vh);
  outline: none;
}

.stack {
  display: inline-grid;
}

.stack > * {
  grid-area: 1 / 1;
  text-align: center;
}

/* The plan's own markup (v-html): reached with :deep, every rule under this dialog's page. */
.plan :deep(.md-page) {
  padding: 2px 20px 8px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text);
  overflow-wrap: break-word;
}

.plan :deep(.md-page > :first-child) {
  margin-top: 0;
}

.plan :deep(h1),
.plan :deep(h2),
.plan :deep(h3),
.plan :deep(h4),
.plan :deep(h5),
.plan :deep(h6) {
  margin: 1.3em 0 0.5em;
  font-weight: 600;
  line-height: 1.3;
  color: var(--text-strong);
}

.plan :deep(h1) {
  font-size: 1.5em;
}

.plan :deep(h2) {
  padding-bottom: 0.25em;
  border-bottom: 1px solid var(--line-soft);
  font-size: 1.25em;
}

.plan :deep(h3) {
  font-size: 1.1em;
}

.plan :deep(h4),
.plan :deep(h5),
.plan :deep(h6) {
  font-size: 1em;
}

.plan :deep(p),
.plan :deep(ul),
.plan :deep(ol),
.plan :deep(blockquote),
.plan :deep(table),
.plan :deep(pre),
.plan :deep(.md-diagram),
.plan :deep(.markdown-alert) {
  margin: 0 0 0.9em;
}

.plan :deep(ul),
.plan :deep(ol) {
  padding-left: 1.8em;
}

.plan :deep(li + li) {
  margin-top: 0.2em;
}

.plan :deep(li > ul),
.plan :deep(li > ol),
.plan :deep(li > p) {
  margin: 0.2em 0 0;
}

.plan :deep(.task-list-item) {
  list-style: none;
}

.plan :deep(.md-check) {
  display: inline-block;
  width: 12px;
  height: 12px;
  margin: 0 0.45em 0 -1.5em;
  border: 1px solid var(--line-strong);
  border-radius: 3px;
  vertical-align: -1px;
}

.plan :deep(.md-check.done) {
  border-color: var(--run);
  background: var(--run);
}

.plan :deep([data-href]) {
  color: var(--text-link);
  cursor: pointer;
}

.plan :deep([data-href]:hover) {
  text-decoration: underline;
  text-underline-offset: 2px;
}

.plan :deep(strong) {
  font-weight: 600;
  color: var(--text-strong);
}

.plan :deep(hr) {
  height: 1px;
  margin: 1.4em 0;
  border: 0;
  background: var(--line-soft);
}

.plan :deep(blockquote) {
  padding: 0 1em;
  border-left: 3px solid var(--line-strong);
  color: var(--text-soft);
}

.plan :deep(code) {
  padding: 0.15em 0.4em;
  border-radius: 5px;
  background: var(--bg-tag);
  font-family: var(--font-mono);
  font-size: 0.88em;
}

.plan :deep(pre) {
  overflow: auto;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-soft);
  line-height: 1.5;
}

.plan :deep(pre code) {
  padding: 0;
  background: none;
  font-size: 12px;
  white-space: pre;
}

.plan :deep(table) {
  display: block;
  width: max-content;
  max-width: 100%;
  overflow: auto;
  border-collapse: collapse;
}

.plan :deep(th),
.plan :deep(td) {
  padding: 5px 10px;
  border: 1px solid var(--line-soft);
}

.plan :deep(th) {
  font-weight: 600;
  background: var(--bg-well);
  color: var(--text-strong);
}

.plan :deep(.md-picture-off) {
  display: inline-flex;
  padding: 2px 8px;
  border-radius: 5px;
  background: var(--bg-tag);
  font-size: 12px;
  color: var(--text-subtle);
}

.plan :deep(.md-front) {
  color: var(--text-subtle);
}

.plan :deep(.markdown-alert) {
  padding: 6px 14px;
  border-left: 3px solid var(--md-alert, var(--line-strong));
}

.plan :deep(.markdown-alert-title) {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
  font-weight: 600;
  color: var(--md-alert, var(--text-strong));
}

.plan :deep(.markdown-alert-title svg) {
  fill: currentColor;
}

.plan :deep(.markdown-alert-note) {
  --md-alert: var(--lane-0);
}

.plan :deep(.markdown-alert-tip) {
  --md-alert: var(--run);
}

.plan :deep(.markdown-alert-important) {
  --md-alert: var(--lane-1);
}

.plan :deep(.markdown-alert-warning) {
  --md-alert: var(--warn);
}

.plan :deep(.markdown-alert-caution) {
  --md-alert: var(--crash);
}

.plan :deep(.md-diagram svg) {
  max-width: 100%;
  height: auto;
}

.plan :deep(.katex-mathml) {
  -webkit-user-select: none;
  user-select: none;
}
</style>
