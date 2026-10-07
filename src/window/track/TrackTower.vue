<script setup lang="ts">
import StatusIcon from "../../components/StatusIcon.vue";
import { claudeState, meta } from "../../lib/format";
import { t } from "../../lib/i18n";
import { rowKeys } from "../../lib/rows";
import { api, now } from "../../lib/store";
import { sessionsIn } from "../../lib/track";
import type { Project } from "../../lib/types";

/**
 * The timing tower: every project in the circuit's order (who waits on you first), each with its
 * three letters. The circuit beside it is for the eye; this is where the keyboard and VoiceOver
 * reach the projects, and a row never moves away from under the pointer.
 */
const props = defineProps<{
  projects: Project[];
  codes: Map<string, string>;
  selected: string | null;
}>();
const emit = defineEmits<{ select: [path: string]; open: [path: string]; toggle: [project: Project]; hover: [path: string | null] }>();

/** Claude's state in a project; with several sessions waiting on you, or none and several at work,
 * how many (the row has room for one of the two: the card under the tower tells both). */
function claudeText(project: Project): string {
  const waits = sessionsIn(project, "waiting").length;
  if (waits > 1) return t("popover.waiting", { count: waits });
  const works = sessionsIn(project, "working").length;
  return !waits && works > 1 ? t("track.working", { count: works }) : claudeState(project, now.value);
}

/** Where it runs, then Claude's state when there is one to tell, else how long it has been up. */
function line(project: Project): { server: string; claude: string } {
  const claude = claudeText(project);
  if (!claude) return { server: meta(project, now.value), claude: "" };
  return { server: project.status === "running" && project.port ? `:${project.port}` : meta(project, now.value), claude };
}

/** One Tab stop: arrows move the selection, ↵ goes to the project (to its session, when Claude
 * waits on you there), Space starts or stops it. A double click opens the editor, as in the
 * project list. */
function onKey(event: KeyboardEvent): void {
  rowKeys(event, {
    move: (path) => emit("select", path),
    enter: (path) => emit("open", path),
    space: (path) => {
      const project = props.projects.find((p) => p.path === path);
      if (project && project.status !== "busy") emit("toggle", project);
    },
  });
}
</script>

<template>
  <section class="tower" :aria-label="t('track.tower')">
    <div class="tower-head">
      <span class="section-label">{{ t("track.tower") }}</span>
      <span class="tower-count">{{ projects.length }}</span>
    </div>
    <div class="tower-rows" role="grid" :aria-label="t('common.projects')" @keydown="onKey" @mouseleave="emit('hover', null)">
      <div
        v-for="(project, index) in projects"
        :key="project.path"
        role="row"
        :data-path="project.path"
        :tabindex="project.path === selected || (!selected && index === 0) ? 0 : -1"
        :aria-selected="project.path === selected"
        :class="['tower-row', { on: project.path === selected }]"
        @click="emit('select', project.path)"
        @dblclick="api.openEditor(project.path)"
        @mouseenter="emit('hover', project.path)"
      >
        <span class="place" role="gridcell">{{ index + 1 }}</span>
        <span role="gridcell" class="mark"><StatusIcon :project="project" :ring="project.path === selected ? 'var(--bg-selected)' : '#13151a'" /></span>
        <span class="who" role="gridcell">
          <span class="who-line">
            <span class="who-name">{{ project.name }}</span>
            <span v-if="project.terminals.length" class="term-badge" role="img" :aria-label="t('window.openTerminals', { count: project.terminals.length })" :title="t('window.openTerminals', { count: project.terminals.length })">>_ {{ project.terminals.length }}</span>
          </span>
          <span class="who-meta">
            <span :class="{ bad: project.status === 'crashed' }">{{ line(project).server }}</span>
            <template v-if="line(project).claude">
              <span aria-hidden="true">·</span>
              <span :class="project.claude ? 'waits' : 'works'">{{ line(project).claude }}</span>
            </template>
          </span>
        </span>
        <span class="code" role="gridcell">{{ codes.get(project.path) }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.tower {
  flex: 0 1 auto;
  min-height: 118px;
  display: flex;
  flex-direction: column;
  padding: 6px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(19, 21, 26, 0.9);
}

.tower-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 7px;
  height: 28px;
  padding: 0 10px;
}

.tower-count {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

/* Only the rows scroll: the head stays, and the card under the tower keeps its place. */
.tower-rows {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.tower-row {
  flex-shrink: 0;
  display: grid;
  grid-template-columns: 14px 16px minmax(0, 1fr) auto;
  align-items: center;
  gap: 9px;
  height: 46px;
  padding: 0 10px;
  border-radius: var(--radius-control);
  cursor: pointer;
}

.tower-row:hover {
  background: #1b1e24;
}

.tower-row.on {
  background: var(--bg-selected);
}

.place {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
  text-align: right;
}

.mark {
  display: inline-flex;
}

.who {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

/* Name and terminal badge on one line, as in the project list: a long name gives way, the badge
   never does. */
.who-line {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
}

.who-name {
  min-width: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.term-badge {
  flex-shrink: 0;
  padding: 1px 6px;
  border-radius: 6px;
  background: var(--bg-control);
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
}

.who-meta {
  display: flex;
  gap: 5px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
}

/* The port or "stopped" stays whole; what follows it gives way. */
.who-meta > span {
  flex-shrink: 0;
}

.who-meta > span:last-child {
  flex-shrink: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.who-meta .bad {
  color: var(--crash-text);
}

.who-meta .waits {
  color: var(--claude-text);
}

.who-meta .works {
  color: var(--text-muted);
}

.code {
  flex-shrink: 0;
  padding: 1px 5px;
  border-radius: 5px;
  background: var(--bg-control);
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
  white-space: nowrap;
}

.tower-row.on .code {
  color: var(--text-strong);
}
</style>
