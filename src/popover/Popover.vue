<script setup lang="ts">
import { LogicalSize } from "@tauri-apps/api/dpi";
import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Marquee from "../components/Marquee.vue";
import MutedMark from "../components/MutedMark.vue";
import Spinner from "../components/Spinner.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { claudeLine, gitLine, meta, shortcutLabel } from "../lib/format";
import { rowKey, useGroupReorder } from "../lib/groupReorder";
import { t } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import { missed, notifications, quiet, quietLine, waits } from "../lib/quiet";
import { rowKeys } from "../lib/rows";
import { api, connectFuel, fuel, now, snapshot } from "../lib/store";
import { applyDrop, teamOf, teams } from "../lib/teams";
import type { Project } from "../lib/types";
import FuelStrip from "./FuelStrip.vue";
import MissedList from "./MissedList.vue";
import QuietBanner from "./QuietBanner.vue";
import ResumeBanner from "./ResumeBanner.vue";

const MAX_HEIGHT = 620;
/** Three rows of the project list with the room under them: what it keeps before anything else
 * gives way. */
const LIST_KEEPS = 146;
/** Kept between the popover's foot and the screen's, when the fuel strip makes it taller. */
const SCREEN_GAP = 16;

const panel = ref<HTMLElement | null>(null);
const strip = ref<InstanceType<typeof FuelStrip> | null>(null);
const list = ref<HTMLElement | null>(null);
/** Drag to reorder, as the window's list drawn without its heads: no group is folded here. */
const reorder = useGroupReorder(list, (change) => applyDrop(change, () => false));
const projects = computed(() => snapshot.value?.projects ?? []);
/** A turn held while it was quiet is listed under what was missed, not here a second time. */
const held = computed(() => new Set(missed.value.filter((entry) => waits(entry.kind)).map((entry) => entry.path)));
const waiting = computed(() => projects.value.filter((p) => p.claude && !held.value.has(p.path)));
const summary = computed(() => {
  const s = snapshot.value;
  if (!s || s.projects.length === 0) return t("popover.noProjects");
  const parts = [t("popover.running", { count: s.running })];
  if (s.waiting > 0) parts.push(t("popover.waiting", { count: s.waiting }));
  return parts.join(" · ");
});

/** A click on the row: the project's VS Code window comes to the front, or opens. With Claude
 * waiting on you there, its waiting session comes up where it runs (the core's `open_claude`). */
function openRow(project: Project): void {
  if (!reorder.isClick()) return;
  void (project.claude ? api.openClaude(project.path) : api.openEditor(project.path));
}

/** While there are teams a row is dragged among its own team's rows only, or among those of the
 * projects in no team (lib/groupReorder: `data-zone`); with no team it goes anywhere. */
function zone(project: Project): string | undefined {
  return teams.value.length ? (teamOf(project.path)?.id ?? "") : undefined;
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

const menu = useNativeMenu();
/** The bell: where the keyboard goes when what it was on is gone (the quiet banner's "Turn off",
 * the last of what was missed). */
const bell = ref<HTMLElement | null>(null);

/** The bell's menu, under the button: quiet for a while (or over, while it is), what quiet lets
 * through, and the way to the quiet hours. */
function bellMenu(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const { questionsThrough, soundsToo, quietHours } = notifications.value;
  const entries: MenuEntry[] = quiet.value ? [{ text: t("quiet.end"), action: () => void api.quietEnd() }, "separator"] : [];
  entries.push(
    { text: t("quiet.hour"), action: () => void api.quietStart("hour") },
    { text: t("quiet.morning"), action: () => void api.quietStart("morning") },
    { text: t("quiet.open"), action: () => void api.quietStart("open") },
    "separator",
    { text: t("quiet.questions"), checked: questionsThrough, action: () => void api.quietOption("questions", !questionsThrough) },
    { text: t("quiet.sounds"), checked: soundsToo, action: () => void api.quietOption("sounds", !soundsToo) },
    "separator",
    { text: quietHours.on ? t("quiet.hoursSet", { from: quietHours.from, to: quietHours.to }) : t("quiet.hours"), action: openQuietHours },
  );
  void menu.popup({ clientX: box.left, clientY: box.bottom + 4 }, entries);
}

/** Pitwall's window, on Settings › Notifications: the way the switcher's `/` opens a page. */
function openQuietHours(): void {
  void emitTo("main", "reveal-page", { page: "settings", tab: "notifications" });
  void api.openWindow();
}

/** Right-click on a row: nothing more is told about the project, or it is again. */
function rowMenu(event: MouseEvent, project: Project): void {
  void menu.popup(event, [{ text: t(project.muted ? "notifications.unmute" : "notifications.mute"), action: () => void api.muteProject(project.path, !project.muted) }]);
}

/** Pressed from the keyboard, the button turns busy (disabled) and would drop the focus: the row keeps it. */
function toggleFrom(event: MouseEvent, project: Project): void {
  toggle(project);
  if (event.detail === 0) ((event.currentTarget as HTMLElement).closest("[data-path]") as HTMLElement | null)?.focus();
}

/** The row the keyboard is on: the list's one Tab stop (the first row until the arrows move it). */
const focusedPath = ref<string | null>(null);
const tabStop = computed(() => (projects.value.some((p) => p.path === focusedPath.value) ? focusedPath.value : (projects.value[0]?.path ?? null)));

/** ↑/↓ move between rows, ↵ does what a click does, Space starts or stops. */
function onListKey(event: KeyboardEvent): void {
  const find = (path: string) => projects.value.find((p) => p.path === path);
  rowKeys(event, {
    move: (path) => (focusedPath.value = path),
    enter: (path) => {
      const project = find(path);
      if (project) void (project.claude ? api.openClaude(project.path) : api.openEditor(project.path));
    },
    space: (path) => {
      const project = find(path);
      if (project && project.status !== "busy") toggle(project);
    },
  });
}

// The window is as tall as the content, up to MAX_HEIGHT; past that the parts that scroll inside
// themselves give way: the project list as far as three rows, then what was missed, the list
// again, the Claude card last (see `.claude`). The fuel strip adds its own height to that limit,
// as far as the screen goes, so the list keeps the room it has without it. A window taller than
// the panel would leave a transparent area that swallows clicks.
async function fitWindow(): Promise<void> {
  await nextTick();
  const el = panel.value;
  if (!el) return;
  // Every part at its own height with its margins, plus the border...
  const outer = (Array.from(el.children) as HTMLElement[]).reduce((sum, part) => {
    const style = getComputedStyle(part);
    return sum + part.offsetHeight + parseFloat(style.marginTop) + parseFloat(style.marginBottom);
  }, 2);
  // ...and what the parts that scroll hold out of sight, each as far as its own limit.
  const unseen = Array.from(el.querySelectorAll<HTMLElement>("[data-scrolls]")).reduce((sum, box) => {
    const limit = parseFloat(getComputedStyle(box).maxHeight) || Infinity;
    return sum + Math.min(box.scrollHeight, limit) - box.clientHeight;
  }, 0);
  const natural = Math.ceil(outer + unseen);
  // No strip: its place holds a comment, which has no height.
  const fuelHeight = (strip.value?.$el as HTMLElement | undefined)?.offsetHeight ?? 0;
  const limit = Math.max(MAX_HEIGHT, Math.min(MAX_HEIGHT + fuelHeight, window.screen.availHeight - SCREEN_GAP));
  const height = Math.min(natural, limit);
  // What is over once the project list is down to its three rows is what was missed's to give
  // (MissedList's `--gives`).
  const rows = list.value?.scrollHeight ?? 0;
  el.style.setProperty("--gives", `${Math.max(0, natural - height - (rows - Math.min(rows, LIST_KEEPS)))}px`);
  if (natural > 0) await getCurrentWindow().setSize(new LogicalSize(372, height));
}

watch(() => snapshot.value, () => void fitWindow());
// The fuel strip comes, goes and takes a line more with what the core last read of the limits.
watch(fuel, () => void fitWindow());

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") void api.hidePopover();
}

let unlistenFuel: (() => void) | null = null;

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  void fitWindow();
  // The limits, for the strip in the foot: sent to this window too.
  unlistenFuel = await connectFuel();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  unlistenFuel?.();
});
</script>

<template>
  <div ref="panel" class="panel">
    <header class="head">
      <div class="titles">
        <div class="title">Pitwall</div>
        <div class="summary">{{ summary }}</div>
        <!-- The list's keys, read with it; here rather than a child of the panel, which would count in its height. -->
        <span id="rows-keys" class="sr-only">{{ t("common.rowKeys") }}</span>
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
      <button
        ref="bell"
        type="button"
        :class="['icon', { on: quiet }]"
        aria-haspopup="menu"
        :aria-label="quiet ? quietLine(quiet, now) : t('quiet.bell')"
        :title="quiet ? quietLine(quiet, now) : t('quiet.bell')"
        @click="bellMenu"
      >
        <Icon :name="quiet ? 'bell-off' : 'bell'" :size="16" />
      </button>
      <button type="button" class="icon" :aria-label="t('common.openWindow')" :title="t('common.openWindow')" @click="api.openWindow()">
        <Icon name="window" :size="16" />
      </button>
      <button type="button" class="icon" :aria-label="t('common.quit')" :title="t('common.quit')" @click="api.quit()">
        <Icon name="power" :size="16" />
      </button>
    </header>

    <!-- While it is quiet: until when, and the way out. It comes and goes with the snapshot, which
         fits the window again. -->
    <QuietBanner v-if="quiet" @leave="bell?.focus()" />

    <!-- What was open at the last quit, while that is still to be answered: it comes and goes with
         the snapshot, which fits the window again. -->
    <ResumeBanner v-if="snapshot?.resume" />

    <section v-if="waiting.length" class="claude" data-scrolls :aria-label="t('popover.claudeWaiting')">
      <div class="claude-label"><span class="claude-dot" aria-hidden="true"></span>{{ t("popover.claudeWaiting") }}</div>
      <button v-for="project in waiting" :key="project.path" type="button" class="claude-row" @click="api.openClaude(project.path)">
        <span class="claude-text">
          <span class="claude-name">{{ project.name }}</span>
          <span class="claude-what">{{ claudeLine(project, now) }}</span>
        </span>
        <span class="claude-open">{{ t("common.open") }} <Icon name="chevron" :size="12" /></span>
      </button>
    </section>

    <!-- What wasn't told while it was quiet. -->
    <MissedList v-if="missed.length" @leave="bell?.focus()" />

    <template v-if="projects.length">
      <div class="section-label label">{{ t("common.projects") }}</div>
      <!-- One Tab stop: ↑/↓ move between rows, ←/→ reach a row's buttons. -->
      <ul
        ref="list"
        :class="['list', reorder.listClass()]"
        data-scrolls
        role="grid"
        :aria-label="t('common.projects')"
        aria-describedby="rows-keys"
        @keydown="onListKey"
      >
        <li
          v-for="project in projects"
          :key="project.path"
          role="row"
          :tabindex="project.path === tabStop ? 0 : -1"
          :data-path="project.path"
          :data-zone="zone(project)"
          :class="['row', reorder.lineClass(rowKey(project.path))]"
          :style="reorder.lineStyle(rowKey(project.path))"
          :title="t('popover.rowTitle', { name: project.name })"
          @pointerdown="reorder.down($event, rowKey(project.path))"
          @focus="focusedPath = project.path"
          @click="openRow(project)"
          @contextmenu="rowMenu($event, project)"
        >
          <span class="cell-status" role="gridcell"><StatusIcon :project="project" /></span>
          <div class="text" role="gridcell">
            <div class="name">{{ project.name }}<MutedMark v-if="project.muted" class="name-mark" /></div>
            <div :class="['meta', { bad: project.status === 'crashed' }]">
              <Marquee :text="[meta(project, now), gitLine(project.git)].filter(Boolean).join(' · ')" />
            </div>
          </div>
          <span class="cell-actions" role="gridcell" data-no-drag>
            <button
              v-if="project.status === 'running'"
              type="button"
              class="icon"
              tabindex="-1"
              :aria-label="t('popover.openInBrowserName', { name: project.name })"
              :title="t('common.openInBrowser')"
              @click.stop="api.openBrowser(project.path)"
            >
              <Icon name="external" />
            </button>
            <button
              type="button"
              class="icon"
              tabindex="-1"
              :disabled="project.status === 'busy'"
              :aria-label="t(project.status === 'running' ? 'common.stopName' : 'common.startName', { name: project.name })"
              :title="t(project.status === 'running' ? 'common.stop' : 'common.start')"
              @click.stop="toggleFrom($event, project)"
            >
              <Spinner v-if="project.status === 'busy'" />
              <Icon v-else :name="project.status === 'running' ? 'stop' : 'play'" :size="14" />
            </button>
          </span>
        </li>
      </ul>
    </template>

    <section v-else class="empty">
      <p>{{ t("popover.empty") }}</p>
      <button type="button" class="primary" @click="api.pickFolder()">{{ t("common.addProject") }}</button>
    </section>

    <!-- The foot, when there is fuel to show: Claude's limits. -->
    <FuelStrip ref="strip" />
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
  border-radius: var(--radius-panel);
  overflow: hidden;
}

/* Linux: square corners on the solid ground the page paints there (tokens.css), as the window may
   not be see-through. */
:root[data-platform="linux"] .panel {
  border-radius: 0;
}

.head,
.label,
.empty {
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
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.icon:disabled {
  opacity: 0.6;
  cursor: default;
}

/* The bell while it is quiet: lit, as a button that is on. */
.icon.on {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Out of room, the popover's parts give way in turn, each scrolling inside itself: the projects
   as far as three rows (`fitWindow`), what was missed down to two lines (MissedList), the
   projects down to one row, this card last and no further than its first row. The foot is never
   cut. */
.claude {
  margin: 0 8px 8px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 0 1 auto;
  min-height: 91px;
  overflow-y: auto;
  overscroll-behavior: contain;
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
  background: var(--claude-bg-hover);
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
  color: var(--claude-muted);
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
  flex: 0 1e6 auto;
  /* One row, and the room under it. */
  min-height: 54px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
  padding: 6px 4px 6px 8px;
  border-radius: var(--radius-control);
}

.row:hover {
  background: var(--bg-hover);
}

/* The keyboard's row: the ring inside, so the list's edge never cuts it. */
.row:focus-visible {
  outline-offset: -2px;
}

.cell-status {
  display: inline-flex;
  flex-shrink: 0;
}

/* The row's buttons, as far apart as the row's other parts. */
.cell-actions {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

/* Drag to reorder: the dragged row lifts and follows the pointer, the others slide aside to
   show where it lands, and on release it slides into place (lib/groupReorder). */
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
  box-shadow: 0 12px 20px -12px var(--shade-drag);
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
  color: var(--text-name);
}

/* A muted project's mark, on the name's line. */
.name-mark {
  margin-left: 6px;
  vertical-align: -1px;
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
</style>
