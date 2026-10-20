<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { bareUrl } from "../../lib/format";
import { t } from "../../lib/i18n";
import { useLinkDialog, type LinksChange } from "../../lib/linkDialog";
import { useNativeMenu } from "../../lib/nativeMenu";
import { projectSettings, updateProjectSettings } from "../../lib/projectSettings";
import { api } from "../../lib/store";
import type { LinkSuggestion, Project } from "../../lib/types";
import LinkDialog from "../LinkDialog.vue";

/** `found`: addresses the project names itself (git remote, .env, package.json). */
const props = defineProps<{ project: Project; found: LinkSuggestion[] }>();

/** The project's links, with a change not in the snapshot yet on top (lib/projectSettings). */
const links = computed(() => projectSettings(props.project).links ?? []);
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
function changeLinks(path: string, change: LinksChange): void {
  void updateProjectSettings(path, (s) => ({ ...s, links: change(s.links ?? []) }));
}

/** The link dialog, which writes into the project it was opened for (lib/linkDialog). */
const { dialog: linkDialog, open: openLink, save: saveLink, remove: removeLink, close: closeLink } = useLinkDialog(() => props.project.path, changeLinks);

function addSuggestion(s: LinkSuggestion): void {
  changeLinks(props.project.path, (list) => (list.some((l) => sameUrl(l.url, s.url)) ? list : [...list, { name: s.name, url: s.url }]));
}

/** Copies `url`, the address of the row at `at` (its row shows it was copied, for a moment). */
function copyLink(at: number, url: string): void {
  void api.copyText(url);
  copied.value = at;
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = null), 1200);
}

/** Right-click on a link: the native menu with what the row can do, to the project it was
 * opened on (the shown one can change while it is up). */
function linkMenu(at: number, event: MouseEvent): void {
  const link = links.value[at];
  const path = props.project.path;
  void menu.popup(event, [
    { text: t("common.openInBrowser"), action: () => void api.openUrl(link.url) },
    {
      text: t("link.copyAddress"),
      action: () => {
        if (path === props.project.path) copyLink(at, link.url);
        else void api.copyText(link.url);
      },
    },
    {
      text: t("detail.menuEdit"),
      action: () => {
        if (path === props.project.path) openLink(at);
      },
    },
    "separator",
    { text: t("common.delete"), action: () => changeLinks(path, (list) => list.filter((_, i) => i !== at)) },
  ]);
}

onBeforeUnmount(() => window.clearTimeout(copiedTimer));
</script>

<template>
  <section class="block links" :aria-label="t('common.links')">
    <!-- The project's other addresses: a click opens one (its tab comes forward if open). -->
    <div class="commands-head">
      <div class="section-label">{{ t("common.links") }}</div>
      <button type="button" class="text-button" @click="openLink(null)">{{ t("detail.add") }}</button>
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
        @click="copyLink(i, l.url)"
      >
        <Icon :name="copied === i ? 'check' : 'copy'" :size="13" />
      </button>
      <button type="button" class="icon small link-action" :aria-label="t('common.editName', { name: l.name })" :title="t('common.edit')" @click="openLink(i)">
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
      @remove="removeLink"
      @close="closeLink"
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
  background: var(--bg-row-hover);
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
  border: 1px dashed var(--line-bold);
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-nav);
  white-space: nowrap;
}

.chip:hover {
  border-style: solid;
  background: var(--bg-well);
  color: var(--text-strong);
}
</style>
