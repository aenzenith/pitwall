<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { uptime } from "../../lib/format";
import { t } from "../../lib/i18n";
import type { OutputLine } from "../../lib/outputStream";
import { outputCollapsed, outputShown, type useResizer } from "../../lib/panel";
import { now } from "../../lib/store";
import { tabKey } from "../../lib/tabs";
import type { Project } from "../../lib/types";

type Resizer = ReturnType<typeof useResizer>;

/**
 * The output over the details' lower part: a handle, a tab per output (the dev server, then every
 * command that ran in this session) and the shown one's lines.
 *
 * `tab`: the shown output, `null` for the dev server, otherwise a command id. `height`: the
 * panel's height as shown (collapsed or not). `resizer`: the handle's drag and keys (lib/panel).
 */
const props = defineProps<{ project: Project; tab: string | null; lines: OutputLine[]; height: number; resizer: Resizer }>();
const emit = defineEmits<{ show: [id: string | null] }>();

const log = ref<HTMLElement | null>(null);

const runsHere = computed(() => props.project.status !== "stopped" && !props.project.owner);
/** While the handle is dragged, the panel follows the pointer without animating. */
const dragging = computed(() => props.resizer.dragging.value);

/** Tabs: the dev server, then every command that ran in this session. */
const tabs = computed(() => [
  { id: null as string | null, name: t("common.devServer") },
  ...(props.project.commands ?? []).filter((c) => c.status !== "idle" || c.result).map((c) => ({ id: c.id as string | null, name: c.name })),
]);
/** The one tab in the Tab order: the shown one, else the dev server's. */
const activeTab = computed(() => (tabs.value.some((item) => item.id === props.tab) ? props.tab : null));

/** ←/→, Home and End move between the output tabs. */
function onTabKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, tabs.value.length, at);
  if (next !== null) emit("show", tabs.value[next].id);
}

function toggleOutput(): void {
  outputCollapsed.value = !outputCollapsed.value;
}

/** The handle doesn't drag a collapsed panel; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!outputCollapsed.value) props.resizer.start(event);
}

/** The newest lines in view: on every change, and when the panel comes back from the settings. */
async function scrollDown(): Promise<void> {
  await nextTick();
  if (log.value) log.value.scrollTop = log.value.scrollHeight;
}

watch(
  () => props.lines,
  () => void scrollDown(),
);
onMounted(() => void scrollDown());

// The terminal panel snaps to the output only while the output is there.
outputShown.value = true;
onBeforeUnmount(() => (outputShown.value = false));
</script>

<template>
  <div :class="['out', { animated: !dragging, collapsed: outputCollapsed }]" :style="{ height: `${height}px` }">
    <div
      class="grip"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('detail.resizeOutput')"
      tabindex="0"
      :aria-valuenow="Math.round(height)"
      @pointerdown="startDrag"
      @pointermove="resizer.move"
      @pointerup="resizer.end"
      @pointercancel="resizer.end"
      @keydown="resizer.nudge"
    ></div>
    <div class="tabs" role="tablist" :aria-label="t('detail.output')">
      <button
        v-for="(item, i) in tabs"
        :key="item.id ?? 'server'"
        type="button"
        role="tab"
        :aria-selected="tab === item.id"
        :tabindex="item.id === activeTab ? 0 : -1"
        :class="['tab', { on: tab === item.id }]"
        @click="emit('show', item.id)"
        @keydown="onTabKey($event, i)"
      >
        {{ item.name }}
      </button>
      <span class="grow"></span>
      <span v-if="tab === null && project.status === 'running' && runsHere" class="up">{{ uptime(project.startedAt, now) }}</span>
      <button
        type="button"
        :class="['collapse', { up: outputCollapsed }]"
        :aria-expanded="!outputCollapsed"
        :aria-label="t(outputCollapsed ? 'detail.expandOutput' : 'detail.collapseOutput')"
        :title="t(outputCollapsed ? 'common.expand' : 'common.collapse')"
        @click="toggleOutput"
      >
        <Icon name="chevron" :size="13" />
      </button>
    </div>
    <div ref="log" class="log selectable">
      <template v-if="lines.length">
        <span v-for="line in lines" :key="line.id" :class="line.kind">{{ line.text }}</span>
      </template>
      <span v-else-if="tab === null && project.owner" class="dim">{{ t("detail.noServerOutput") }}</span>
      <span v-else-if="tab === null" class="dim">{{ t("detail.notRunning") }}</span>
      <span v-else class="dim">{{ t("detail.noOutput") }}</span>
    </div>
  </div>
</template>

<style scoped>
/* Pinned to the bottom with the height you gave it; grown, it covers the sections. */
.out {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  overflow: clip;
  background: var(--bg-detail);
  border-top: 1px solid var(--line);
  box-shadow: 0 -12px 24px rgba(0, 0, 0, 0.22);
}

/* Collapsing and expanding slide (COLLAPSE_MS in lib/panel); a drag follows the pointer. */
.out.animated {
  transition: height 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.out.collapsed .grip {
  cursor: default;
}

.out.collapsed .grip::after {
  opacity: 0;
}

.out.collapsed .log {
  opacity: 0;
}

.grow {
  flex-grow: 1;
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.collapse:hover {
  background: #262a33;
  color: var(--text-strong);
}

.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

@media (prefers-reduced-motion: reduce) {
  .out.animated,
  .log,
  .collapse :deep(svg) {
    transition: none;
  }
}

/* Handle and tabs keep their size; only the log gives way when the panel is short. */
.grip {
  flex-shrink: 0;
  height: 9px;
  margin: 0 20px;
  cursor: row-resize;
  position: relative;
  outline: none;
}

.grip::after {
  content: "";
  position: absolute;
  left: 50%;
  top: 3px;
  width: 36px;
  height: 3px;
  margin-left: -18px;
  border-radius: 2px;
  background: #2f343d;
}

.grip:hover::after,
.grip:focus-visible::after {
  background: #4a515c;
}

/* Output tabs: the active one is white, the rest faded; nothing more. */
.tabs {
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 12px 0 14px;
  overflow: hidden;
}

.tab {
  padding: 4px 6px;
  border: 0;
  background: transparent;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.tab:hover {
  color: var(--text-muted);
}

.tab.on {
  color: #fff;
}

.up {
  flex-shrink: 0;
  white-space: nowrap;
  padding-right: 6px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.log {
  flex: 1 1 auto;
  min-height: 0;
  transition: opacity 160ms ease;
  margin: 6px 20px 20px;
  box-sizing: border-box;
  padding: 12px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid #1f2228;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.7;
  color: #a9afb8;
  display: flex;
  flex-direction: column;
  overflow: auto;
}

.log span {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.log .cmd {
  color: var(--text);
}

.log .sys {
  color: var(--claude-text);
}

.log .dim {
  white-space: normal;
  color: var(--text-subtle);
}
</style>
