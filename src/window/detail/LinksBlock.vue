<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { bareUrl } from "../../lib/format";
import { t } from "../../lib/i18n";
import { useNativeMenu } from "../../lib/nativeMenu";
import { projectSettings, updateProjectSettings } from "../../lib/projectSettings";
import { api } from "../../lib/store";
import type { LinkSuggestion, Project, ProjectLink } from "../../lib/types";
import LinkDialog from "../LinkDialog.vue";

/** `found`: addresses the project names itself (git remote, .env, package.json). */
const props = defineProps<{ project: Project; found: LinkSuggestion[] }>();

/** The project's links, with a change not in the snapshot yet on top (lib/projectSettings). */
const links = computed(() => projectSettings(props.project).links ?? []);
/** The link dialog: the link being edited, by its place, or `null` for a new one. */
const linkDialog = ref<{ index: number | null } | null>(null);
/** The row whose address was just copied, for a moment. */
const copied = ref<number | null>(null);
let copiedTimer = 0;
const menu = useNativeMenu();

function sameUrl(a: string, b: string): boolean {
  return a.replace(/\/$/, "") === b.replace(/\/$/, "");
}

/** Found addresses that aren't links yet. */
const suggestions = computed(() => props.found.filter((s) => !links.value.some((l) => sameUrl(l.url, s.url))));

/** Links are saved on their own, like commands, through the one writer (lib/projectSettings). */
function changeLinks(change: (list: ProjectLink[]) => ProjectLink[]): void {
  void updateProjectSettings(props.project.path, (s) => ({ ...s, links: change(s.links ?? []) }));
}

function saveLink(link: ProjectLink): void {
  const at = linkDialog.value?.index ?? null;
  changeLinks((list) => (at === null ? [...list, link] : list.map((l, i) => (i === at ? link : l))));
  linkDialog.value = null;
}

function removeLink(at: number | null): void {
  if (at !== null) changeLinks((list) => list.filter((_, i) => i !== at));
  linkDialog.value = null;
}

function addSuggestion(s: LinkSuggestion): void {
  changeLinks((list) => (list.some((l) => sameUrl(l.url, s.url)) ? list : [...list, { name: s.name, url: s.url }]));
}

function copyLink(at: number): void {
  void api.copyText(links.value[at].url);
  copied.value = at;
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = null), 1200);
}

/** Right-click on a link: the native menu with what the row can do. */
function linkMenu(at: number, event: MouseEvent): void {
  const link = links.value[at];
  void menu.popup(event, [
    { text: t("common.openInBrowser"), action: () => void api.openUrl(link.url) },
    { text: t("link.copyAddress"), action: () => copyLink(at) },
    { text: t("detail.menuEdit"), action: () => (linkDialog.value = { index: at }) },
    "separator",
    { text: t("common.delete"), action: () => removeLink(at) },
  ]);
}

onBeforeUnmount(() => window.clearTimeout(copiedTimer));
</script>

<template>
  <section class="block links" :aria-label="t('common.links')">
    <!-- The project's other addresses: a click opens one (its tab comes forward if open). -->
    <div class="commands-head">
      <div class="section-label">{{ t("common.links") }}</div>
      <button type="button" class="text-button" @click="linkDialog = { index: null }">{{ t("detail.add") }}</button>
    </div>
    <p v-if="!links.length" class="empty-commands">{{ t("detail.noLinks") }}</p>
    <div v-for="(l, i) in links" :key="`${i}:${l.url}`" class="link-row" @contextmenu="linkMenu(i, $event)">
      <button type="button" class="link-open" :title="l.url" @click="api.openUrl(l.url)">
        <span class="link-icon"><Icon name="link" :size="13" /></span>
        <span class="link-name">{{ l.name }}</span>
        <span class="link-url">{{ bareUrl(l.url) }}</span>
      </button>
      <button
        type="button"
        class="icon small link-action"
        :aria-label="t('link.copyAddress')"
        :title="t(copied === i ? 'link.copied' : 'link.copyAddress')"
        @click="copyLink(i)"
      >
        <Icon :name="copied === i ? 'check' : 'copy'" :size="13" />
      </button>
      <button type="button" class="icon small link-action" :aria-label="t('common.editName', { name: l.name })" :title="t('common.edit')" @click="linkDialog = { index: i }">
        <Icon name="pencil" :size="13" />
      </button>
    </div>
    <div v-if="suggestions.length" class="suggest">
      <span>{{ t("link.suggested") }}</span>
      <button v-for="s in suggestions.slice(0, 3)" :key="s.url" type="button" class="chip" :title="s.url" @click="addSuggestion(s)">+ {{ s.name }}</button>
    </div>

    <!-- A modal <dialog>: it shows over the whole window, wherever it sits in the page. -->
    <LinkDialog
      v-if="linkDialog"
      :link="linkDialog.index === null ? null : (links[linkDialog.index] ?? null)"
      :project-name="project.name"
      :suggestions="suggestions"
      @save="saveLink"
      @remove="removeLink(linkDialog?.index ?? null)"
      @close="linkDialog = null"
    />
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
/* Links sit close, like a list; the head and the suggestions keep their own room. */
.links {
  gap: 2px;
}

/* A link: one button across the row opens it; copy and edit show on hover. */
.link-row {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 30px;
  margin: 0 -8px;
  padding: 0 4px 0 8px;
  border-radius: var(--radius-control);
}

.link-row:hover {
  background: #1b1e24;
}

.link-open {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  height: 30px;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: left;
}

.link-icon {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-subtle);
}

.link-name {
  flex-shrink: 0;
  max-width: 45%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link-url {
  flex-grow: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link-action {
  opacity: 0;
}

.link-row:hover .link-action,
.link-action:focus-visible {
  opacity: 1;
}

/* Found in the project, not added yet: one click adds. */
.suggest {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-subtle);
}

.chip {
  height: 24px;
  padding: 0 9px;
  border: 1px dashed #3a3f48;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: #b4bac3;
  white-space: nowrap;
}

.chip:hover {
  border-style: solid;
  background: #1c1f25;
  color: var(--text-strong);
}
</style>
