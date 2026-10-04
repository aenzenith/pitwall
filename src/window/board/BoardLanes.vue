<script setup lang="ts">
import { computed } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import StatusIcon from "../../components/StatusIcon.vue";
import { COLUMN_ICONS, COLUMN_LABELS, LANE_LIMIT, laneDrawn, type CardStatus, type Lane } from "../../lib/board";
import { gitLine, phase } from "../../lib/format";
import { t } from "../../lib/i18n";
import type { MenuPoint } from "../../lib/nativeMenu";
import type { BoardColumn, Card, Project, SessionRow } from "../../lib/types";
import BoardCard from "./BoardCard.vue";
import type { CardBusy } from "./context";

/**
 * Every project's board, a lane each: its up next, with Claude and to review (a few cards each),
 * and how many it finished today. A project with nothing in Claude's column offers its next card.
 * Under them, the folders that are no longer among the projects but still have cards: a lane each
 * too, with every card (there is no board to open for the rest), nothing to give, and a way to
 * move its cards to a project.
 */
const props = defineProps<{
  lanes: Lane[];
  /** The lanes of folders no longer listed. */
  unlisted: Lane[];
  /** There is a project their cards can go to. */
  canRehome: boolean;
  /** Projects left out for having no cards; `showHidden` shows them too. */
  hidden: number;
  showHidden: boolean;
  selected: string | null;
  statuses: Map<string, CardStatus | null>;
  rowOf: (card: Card) => SessionRow | null;
  /** By card id; a folder's move to a project by its path. */
  busy: Record<string, CardBusy>;
  errors: Record<string, string>;
}>();
const emit = defineEmits<{ open: [path: string]; toggleHidden: []; giveNext: [id: string]; rehome: [at: MenuPoint, path: string] }>();

const CELLS: Array<Exclude<BoardColumn, "done">> = ["queued", "claude", "review"];

/** The projects' lanes, then the folders' under their heading: one row's markup for both. */
const groups = computed(() => [
  { listed: true, lanes: props.lanes },
  { listed: false, lanes: props.unlisted },
]);

/** The lane has the selected card. */
function holds(lane: Lane, id: string | null): boolean {
  return id !== null && [lane.queued, lane.claude, lane.review].some((cards) => cards.some((card) => card.id === id));
}

/** `:1420 · main`, `stopped · main · 4 changed`. */
function laneMeta(project: Project): string {
  const server =
    project.status === "running"
      ? project.port
        ? `:${project.port}`
        : t("phase.starting")
      : project.status === "busy"
        ? phase(project)
        : project.status === "crashed"
          ? t("status.crashed")
          : t("server.stopped");
  return [server, gitLine(project.git)].filter(Boolean).join(" · ");
}

/** The projects a folder's cards can go to, in a menu under its button. */
function rehome(event: MouseEvent, path: string): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  emit("rehome", { clientX: box.left, clientY: box.bottom + 4 }, path);
}
</script>

<template>
  <div class="blanes">
    <div class="blanes-head" aria-hidden="true">
      <span>{{ t("window.col.project") }}</span>
      <span v-for="cell in CELLS" :key="cell" class="blanes-col">
        <span class="blanes-mark">
          <ClaudeLogo v-if="cell === 'claude'" :size="11" />
          <Icon v-else :name="COLUMN_ICONS[cell]" :size="13" />
        </span>
        <span class="blanes-name">{{ t(COLUMN_LABELS[cell]) }}</span>
      </span>
      <span class="blanes-col blanes-done">
        <span class="blanes-mark"><Icon :name="COLUMN_ICONS.done" :size="13" /></span>
        <span class="blanes-name">{{ t(COLUMN_LABELS.done) }}</span>
      </span>
    </div>

    <!-- Only the lanes scroll, never the page. -->
    <div class="blanes-scroll" data-column-body>
      <template v-for="group in groups" :key="group.listed ? 'listed' : 'unlisted'">
        <h2 v-if="!group.listed && group.lanes.length" class="blanes-group">{{ t("board.unlisted.title") }}</h2>

        <section v-for="lane in group.lanes" :key="lane.path" :class="['blane', { current: holds(lane, selected) }]" :aria-label="lane.name">
          <div class="blane-project">
            <template v-if="lane.project">
              <button type="button" class="blane-name" :title="t('board.openLane', { project: lane.name })" @click="emit('open', lane.path)">
                <StatusIcon :project="lane.project" :ring="holds(lane, selected) ? 'var(--bg-sidebar)' : 'var(--bg-app)'" />
                <span class="blane-name-text">{{ lane.name }}</span>
              </button>
              <span class="blane-meta">{{ laneMeta(lane.project) }}</span>
            </template>
            <!-- A folder no longer listed: no board to open and no server to tell of; its cards can
                 go to a project. -->
            <template v-else>
              <span class="blane-folder" :title="lane.path">{{ lane.name }}</span>
              <button
                type="button"
                class="blane-rehome"
                :title="t('board.unlisted.moveTitle')"
                :disabled="!canRehome || busy[lane.path] === 'move'"
                @click="rehome($event, lane.path)"
              >
                <Spinner v-if="busy[lane.path] === 'move'" :size="10" />
                <span class="blane-rehome-text">{{ t("board.unlisted.move") }}</span>
              </button>
              <p v-if="errors[lane.path]" class="blane-error" role="alert">{{ errors[lane.path] }}</p>
            </template>
          </div>

          <div v-for="(cell, x) in CELLS" :key="cell" class="blane-cell" :data-colx="x">
            <div v-if="lane[cell].length" class="blane-list" role="list" :aria-label="`${lane.name} · ${t(COLUMN_LABELS[cell])}`">
              <BoardCard
                v-for="card in laneDrawn(lane, cell)"
                :key="card.id"
                :card="card"
                variant="compact"
                :status="statuses.get(card.id) ?? null"
                :row="rowOf(card)"
                :selected="card.id === selected"
                :busy="busy[card.id] ?? null"
                :error="errors[card.id] ?? ''"
                :givable="lane.project !== null"
              />
            </div>
            <button v-if="lane.project && lane[cell].length > LANE_LIMIT" type="button" class="blane-more" @click="emit('open', lane.path)">
              {{ t("board.more", { count: lane[cell].length - LANE_LIMIT }) }}
            </button>

            <!-- Nothing with Claude, whatever the search shows: idle, and the next card one click away. -->
            <div v-if="cell === 'claude' && lane.project && lane.idle" class="blane-idle">
              <span class="blane-idle-text">{{ t("board.idle") }}</span>
              <button
                v-if="lane.next"
                type="button"
                class="blane-give"
                :title="t('board.giveNextTitle', { title: lane.next.title })"
                :disabled="busy[lane.next.id] === 'give'"
                @click="emit('giveNext', lane.next.id)"
              >
                <Spinner v-if="busy[lane.next.id] === 'give'" :size="10" />
                <ClaudeLogo v-else :size="11" />
                <span class="blane-give-text">{{ t("board.giveNext") }}</span>
              </button>
            </div>
            <span v-else-if="!lane[cell].length" class="blane-none" aria-hidden="true">—</span>
          </div>

          <span :class="['blane-done', { none: !lane.done }]">{{ lane.done }}</span>
        </section>

        <button v-if="group.listed && hidden" type="button" class="blanes-hidden" @click="emit('toggleHidden')">
          {{ showHidden ? t("board.hide") : `${t("board.hiddenProjects", { count: hidden })} · ${t("board.show")}` }}
        </button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.blanes {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.blanes-head,
.blane {
  display: grid;
  grid-template-columns: 196px repeat(3, minmax(0, 1fr)) 72px;
  gap: 12px;
}

/* 44px: its line is level with the one under the details' head, as a project's board's is. */
.blanes-head {
  flex-shrink: 0;
  align-items: center;
  height: 44px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
  white-space: nowrap;
}

:lang(zh) .blanes-head,
:lang(ja) .blanes-head {
  letter-spacing: 0;
}

.blanes-head > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* A column's mark and its name; the name gives way, the mark never does. */
.blanes-col {
  display: flex;
  align-items: center;
  gap: 7px;
}

.blanes-mark {
  flex-shrink: 0;
  display: grid;
  place-items: center;
  width: 13px;
}

.blanes-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.blanes-done {
  justify-content: flex-end;
}

.blanes-scroll {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow-x: hidden;
  overflow-y: auto;
}

/* The folders no longer listed, under the projects: a heading as the columns' own above. */
.blanes-group {
  flex-shrink: 0;
  margin: 0;
  padding: 20px 20px 8px;
  border-bottom: 1px solid #1e2127;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

:lang(zh) .blanes-group,
:lang(ja) .blanes-group {
  letter-spacing: 0;
}

.blane {
  align-items: start;
  padding: 14px 20px;
  border-bottom: 1px solid #1e2127;
}

/* The lane of the selected card. */
.blane.current {
  background: var(--bg-sidebar);
}

.blane-project {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  padding-top: 2px;
}

.blane-name {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  margin: 0 0 0 -4px;
  padding: 2px 4px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-weight: 500;
  text-align: left;
}

.blane-name:hover {
  background: #1f2228;
}

.blane-name-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.blane-meta {
  padding-left: 18px;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* A folder no longer listed: its name alone, not a way to a board. */
.blane-folder {
  min-width: 0;
  padding: 2px 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Its cards' way to a project: a control as quiet as the page's others. */
.blane-rehome {
  align-self: flex-start;
  max-width: 100%;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  margin-top: 3px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

.blane-rehome:hover:not(:disabled) {
  background: #2c3039;
}

.blane-rehome:disabled {
  opacity: 0.7;
}

.blane-rehome .spinner {
  flex-shrink: 0;
}

.blane-rehome-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.blane-error {
  margin: 3px 0 0;
  font-size: 12px;
  line-height: 1.4;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.blane-cell {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.blane-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.blane-more {
  align-self: flex-start;
  max-width: 100%;
  height: 22px;
  padding: 0 4px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
  overflow: hidden;
  text-overflow: ellipsis;
}

.blane-more:hover {
  background: #1f2228;
  color: var(--text-muted);
}

.blane-idle {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  min-height: 36px;
  padding: 4px 4px 4px 10px;
  border: 1px dashed var(--line-strong);
  border-radius: var(--radius-control);
}

.blane-idle-text {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

/* Claude's colours, as everything that belongs to Claude; a narrow lane shortens its label. */
.blane-give {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  height: 26px;
  padding: 0 9px;
  border: 1px solid #4a3523;
  border-radius: var(--radius-control);
  background: #2a1f17;
  color: #f3c29b;
  font-size: 12px;
}

.blane-give:hover:not(:disabled) {
  background: #35271c;
}

.blane-give svg,
.blane-give .spinner {
  flex-shrink: 0;
}

.blane-give-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.blane-none {
  padding-top: 7px;
  font-size: 12px;
  color: var(--text-faint);
}

.blane-done {
  padding-top: 7px;
  text-align: right;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}

.blane-done.none {
  color: var(--text-faint);
}

.blanes-hidden {
  align-self: flex-start;
  max-width: calc(100% - 28px);
  margin: 12px 14px;
  height: 30px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
  overflow: hidden;
  text-overflow: ellipsis;
}

.blanes-hidden:hover {
  background: #1f2228;
  color: var(--text-muted);
}
</style>
