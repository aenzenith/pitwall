<script setup lang="ts">
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Marquee from "../components/Marquee.vue";
import Spinner from "../components/Spinner.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeLine, gitLine, meta, shortcutLabel } from "../lib/format";
import { t } from "../lib/i18n";
import { useReorder } from "../lib/reorder";
import { api, now, snapshot } from "../lib/store";
import type { Project } from "../lib/types";

const MAX_HEIGHT = 620;

const panel = ref<HTMLElement | null>(null);
const list = ref<HTMLElement | null>(null);
const reorder = useReorder(list);
const projects = computed(() => snapshot.value?.projects ?? []);
const waiting = computed(() => projects.value.filter((p) => p.claude));
const anyBusy = computed(() => projects.value.some((p) => p.status === "busy"));
const summary = computed(() => {
  const s = snapshot.value;
  if (!s || s.projects.length === 0) return t("popover.noProjects");
  const parts = [t("popover.running", { count: s.running })];
  if (s.waiting > 0) parts.push(t("popover.waiting", { count: s.waiting }));
  return parts.join(" · ");
});

/** A click on the row: the project's VS Code window comes to the front, or opens. */
function openRow(project: Project): void {
  if (!reorder.isClick()) return;
  void (project.claude ? api.openClaude(project.path) : api.openEditor(project.path));
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

// The window is as tall as the content, up to MAX_HEIGHT; past that only the project list
// scrolls. A window taller than the panel would leave a transparent area that swallows clicks.
async function fitWindow(): Promise<void> {
  await nextTick();
  const el = panel.value;
  if (!el) return;
  // Every part at its own height, the list at its full content height, plus the border.
  const parts = Array.from(el.children) as HTMLElement[];
  const natural = Math.ceil(parts.reduce((sum, part) => sum + (part === list.value ? part.scrollHeight : part.offsetHeight), 2));
  if (natural > 0) await getCurrentWindow().setSize(new LogicalSize(372, Math.min(natural, MAX_HEIGHT)));
}

watch(() => snapshot.value, () => void fitWindow());

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") void api.hidePopover();
}

onMounted(() => {
  window.addEventListener("keydown", onKey);
  void fitWindow();
});

onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div ref="panel" class="panel">
    <header class="head">
      <div class="titles">
        <div class="title">Pitwall</div>
        <div class="summary">{{ summary }}</div>
      </div>
      <button
        type="button"
        class="icon"
        :aria-label="t('common.searchProjects')"
        :title="snapshot?.settings.shortcut ? t('popover.searchWithShortcut', { shortcut: shortcutLabel(snapshot.settings.shortcutKeys) }) : t('common.searchProjects')"
        @click="api.showSwitcher()"
      >
        <Icon name="search" :size="16" />
      </button>
      <button type="button" class="icon" :aria-label="t('common.openWindow')" :title="t('common.openWindow')" @click="api.openWindow()">
        <Icon name="window" :size="16" />
      </button>
      <button type="button" class="icon" :aria-label="t('common.quit')" :title="t('common.quit')" @click="api.quit()">
        <Icon name="power" :size="16" />
      </button>
    </header>

    <section v-if="waiting.length" class="claude" :aria-label="t('popover.claudeWaiting')">
      <div class="claude-label"><span class="claude-dot"></span>{{ t("popover.claudeWaiting") }}</div>
      <button v-for="project in waiting" :key="project.path" type="button" class="claude-row" @click="api.openClaude(project.path)">
        <span class="claude-text">
          <span class="claude-name">{{ project.name }}</span>
          <span class="claude-what">{{ claudeLine(project, now) }}</span>
        </span>
        <span class="claude-open">{{ t("common.open") }} <Icon name="chevron" :size="12" /></span>
      </button>
    </section>

    <template v-if="projects.length">
      <div class="section-label label">{{ t("common.projects") }}</div>
      <ul ref="list" :class="['list', reorder.listClass()]">
        <li
          v-for="project in projects"
          :key="project.path"
          :data-path="project.path"
          :class="['row', reorder.rowClass(project.path)]"
          :style="reorder.rowStyle(project.path)"
          :title="t('popover.rowTitle', { name: project.name })"
          @pointerdown="reorder.down($event, project.path)"
          @click="openRow(project)"
        >
          <StatusIcon :project="project" />
          <div class="text">
            <div class="name">{{ project.name }}</div>
            <div :class="['meta', { bad: project.status === 'crashed' }]">
              <Marquee :text="[meta(project, now), gitLine(project.git)].filter(Boolean).join(' · ')" />
            </div>
          </div>
          <button
            v-if="project.status === 'running'"
            type="button"
            class="icon"
            data-no-drag
            :aria-label="t('popover.openInBrowserName', { name: project.name })"
            :title="t('common.openInBrowser')"
            @click.stop="api.openBrowser(project.path)"
          >
            <Icon name="external" />
          </button>
          <button
            type="button"
            class="icon"
            data-no-drag
            :disabled="project.status === 'busy'"
            :aria-label="t(project.status === 'running' ? 'common.stopName' : 'common.startName', { name: project.name })"
            :title="t(project.status === 'running' ? 'common.stop' : 'common.start')"
            @click.stop="toggle(project)"
          >
            <Spinner v-if="project.status === 'busy'" />
            <Icon v-else :name="project.status === 'running' ? 'stop' : 'play'" :size="14" />
          </button>
        </li>
      </ul>
    </template>

    <section v-else class="empty">
      <p>{{ t("popover.empty") }}</p>
      <button type="button" class="primary" @click="api.pickFolder()">{{ t("common.addProject") }}</button>
    </section>

    <footer class="foot">
      <button type="button" class="foot-btn" :disabled="!projects.length || anyBusy" @click="api.startAll()">{{ t("common.startAll") }}</button>
      <button type="button" class="foot-btn" :disabled="!snapshot?.running || anyBusy" @click="api.stopAll()">{{ t("common.stopAll") }}</button>
      <Spinner v-if="anyBusy" class="foot-spin" :size="12" />
      <div class="grow"></div>
      <button type="button" class="foot-btn" @click="api.openWindow()">
        {{ t("common.openWindow") }}
      </button>
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
  border-radius: var(--radius-panel);
  overflow: hidden;
}

.head,
.claude,
.label,
.empty,
.foot {
  flex-shrink: 0;
}

.head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 14px 10px 12px 16px;
}

.titles {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
}

.title {
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0.01em;
}

.summary {
  font-size: 12px;
  color: var(--text-muted);
}

.icon {
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.icon:hover:not(:disabled) {
  background: #262a33;
  color: var(--text-strong);
}

.icon:disabled {
  opacity: 0.6;
  cursor: default;
}

.claude {
  margin: 0 8px 8px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: var(--claude-bg);
  border: 1px solid var(--claude-line);
  border-radius: var(--radius-card);
}

.claude-label {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px 6px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--claude-text);
}

:lang(zh) .claude-label,
:lang(ja) .claude-label {
  letter-spacing: 0;
}

.claude-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--claude);
}

.claude-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  text-align: left;
}

.claude-row:hover {
  background: #3a2a1d;
}

.claude-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.claude-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-strong);
}

.claude-what {
  font-size: 12px;
  color: #c9a88c;
}

.claude-open {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--claude-text);
}

.label {
  padding: 6px 16px 4px;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0 8px 8px;
  display: flex;
  flex-direction: column;
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 4px 6px 8px;
  border-radius: var(--radius-control);
}

.row:hover {
  background: var(--bg-hover);
}

/* Drag to reorder: the dragged row lifts and follows the pointer, the others slide aside to
   show where it lands, and on release it slides into place (lib/reorder). */
.list.reordering .row {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.list.reordering .row:hover:not(.dragging) {
  background: transparent;
}

/* The shadow falls below the row only: sideways it would be cut at the list's edges. */
.row.dragging {
  position: relative;
  z-index: 2;
  background: var(--bg-hover);
  box-shadow: 0 12px 20px -12px rgba(0, 0, 0, 0.8);
}

.list.reordering .row.dragging {
  transition: none;
}

.list.reordering .row.dragging.settling {
  transition: transform 160ms ease-out;
}

.list.frozen .row {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .list.reordering .row,
  .list.reordering .row.dragging.settling {
    transition: none;
  }
}

.text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
}

.name {
  font-size: 13px;
  font-weight: 500;
  color: #eef0f3;
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

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  padding: 28px 32px;
  text-align: center;
  color: var(--text-muted);
}

.empty p {
  margin: 0;
  line-height: 1.5;
}

.primary {
  height: 32px;
  padding: 0 14px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.primary:hover {
  background: var(--bg-hover);
}

.foot {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px;
  border-top: 1px solid var(--line);
  background: var(--bg-footer);
  flex-shrink: 0;
}

.grow {
  flex-grow: 1;
}

.foot-spin {
  margin-left: 4px;
  color: var(--text-muted);
}

.foot-btn {
  height: 30px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 12px;
  color: #c3c8cf;
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.foot-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-strong);
}

.foot-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

kbd {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}
</style>
