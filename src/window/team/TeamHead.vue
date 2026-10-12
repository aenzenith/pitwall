<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";
import { startTeam, stopTeam, teamLine, type ClaudeNote } from "../../lib/teams";
import type { Team } from "../../lib/types";
import SwapLabel from "./SwapLabel.vue";
import TeamClaude from "./TeamClaude.vue";

/**
 * A group's head in the project list: the fold arrow, the name, how many projects it has and how
 * many run, what Claude is at in them (`claude`: folded, its rows don't say), and for a team the
 * button that starts or stops it. `team` null: the head of the
 * projects in no team, which only folds. A click anywhere else on a team's head selects the
 * team (`select`), and a double click there folds or unfolds the group as its arrow does
 * (`fold`); the list drags a head by the same place (the buttons at its ends stay out of all
 * three). The head stands on a ground of its own, a shade off the list's, so the teams tell
 * apart from the rows between them.
 *
 * `foldable` false: the team's own view, where its rows always show. `busy`: a start or a stop
 * of the team is under way, and the button spins until the servers are really up or down.
 * `tabStop`: this head is the list's Tab stop. The head itself takes the keyboard, as a row does,
 * never a button of its own: WebKit's Tab passes buttons by unless the system's keyboard
 * navigation is on. Return and Space on it do what its own button did (`press`).
 */
const props = defineProps<{ team: Team | null; total: number; running: number; claude: ClaudeNote[]; busy: boolean; folded: boolean; selected: boolean; foldable: boolean; tabStop: boolean }>();
const emit = defineEmits<{ select: []; fold: [] }>();

const name = computed(() => props.team?.name ?? t("team.loose"));
const line = computed(() => teamLine(props.total, props.running));

/** Both labels, so the button is as wide with either. */
const labels = computed(() => [
  { id: "start", text: t("team.start"), icon: "play" as const },
  { id: "stop", text: t("team.stop"), icon: "stop" as const },
]);

/** Return or Space on the head: the team is selected; the head with no team to select folds. */
function press(): void {
  if (props.team) emit("select");
  else emit("fold");
}

function toggle(): void {
  if (!props.team) return;
  if (props.running) stopTeam(props.team.id);
  else startTeam(props.team.id);
}
</script>

<template>
  <div class="ghead" role="row" :tabindex="tabStop ? 0 : -1" :data-group="team?.id ?? ''" :aria-selected="team ? selected : undefined" @keydown.enter.self.prevent="press" @keydown.space.self.prevent="press">
    <div :class="['ghead-box', { selected }]" role="gridcell" aria-colspan="5" @click="team && emit('select')" @dblclick="foldable && emit('fold')">
      <!-- The arrow and the team's button are reached with ←/→ from the head, like a row's buttons. -->
      <button
        v-if="foldable"
        type="button"
        class="ghead-fold"
        data-no-drag
        tabindex="-1"
        :aria-expanded="!folded"
        :aria-label="t(folded ? 'team.unfoldName' : 'team.foldName', { name })"
        :title="t(folded ? 'common.expand' : 'common.collapse')"
        @click.stop="emit('fold')"
        @dblclick.stop
      >
        <Icon :name="folded ? 'chevron' : 'chevron-down'" :size="13" />
      </button>
      <span v-else class="ghead-mark"><Icon name="users" :size="13" /></span>

      <span class="ghead-main">
        <span class="ghead-name">{{ name }}</span>
        <span class="ghead-sub">{{ line }}<span v-if="claude.length" class="sep" aria-hidden="true">·</span></span>
        <TeamClaude :notes="claude" />
      </span>

      <button
        v-if="team"
        type="button"
        class="ghead-act"
        data-no-drag
        tabindex="-1"
        :disabled="busy || !total"
        :aria-busy="busy"
        :aria-label="t(running ? 'team.stopName' : 'team.startName', { name })"
        @click.stop="toggle"
        @dblclick.stop
      >
        <SwapLabel :labels="labels" :show="busy ? null : running ? 'stop' : 'start'" :icon="12" />
      </button>
    </div>
  </div>
</template>

<style scoped>
/* The room above a head is its own padding, not a margin: every line of the list begins where
   the one before ends, which the drag counts on (lib/groupReorder). */
.ghead {
  padding-top: 6px;
}

.ghead-box {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 6px 0 10px;
  border-radius: 8px;
  background: var(--bg-group);
  cursor: pointer;
}

.ghead-box:hover {
  background: var(--bg-group-hover);
}

/* The rows' selected look. */
.ghead-box.selected {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

/* The keyboard's place is the whole head: the ring is its box's, inside as on a row. */
.ghead:focus-visible {
  outline: none;
}

.ghead:focus-visible .ghead-box {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

/* A larger box around the 13px arrow, which stays where the head's padding puts it. */
.ghead-fold {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  margin: 0 -3.5px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.ghead-fold:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.ghead-mark {
  flex-shrink: 0;
  display: inline-flex;
  color: var(--text-subtle);
}

/* The name and the count: all the room between the arrow and the button, so a click there
   selects the team. A long name gives way after the count has. */
.ghead-main {
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
  display: flex;
  align-items: center;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 12px;
  text-align: left;
}

/* Its room from the count is its own, so the count and what follows it stay one line of words. */
.ghead-name {
  min-width: 0;
  margin-right: 8px;
  font-weight: 600;
  color: var(--text-nav);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ghead-box.selected .ghead-name {
  color: var(--text-strong);
}

.ghead-sub {
  flex: 0 1000 auto;
  min-width: 0;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The dot between the count and what Claude is at (team/TeamClaude), the count's own: a space
   before it (the one after is TeamClaude's), and gone with the count once that is squeezed away. */
.sep {
  margin-left: 0.25em;
}

.ghead-act {
  flex-shrink: 0;
  height: 24px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.ghead-act:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Nothing to start: dimmed. Spinning, it keeps its colour. */
.ghead-act:disabled:not([aria-busy="true"]) {
  opacity: 0.4;
}

/* In hand: lifted like a dragged row, the shadow below only. */
.ghead.dragging .ghead-box {
  background: var(--bg-selected);
  box-shadow:
    0 14px 22px -14px var(--shade-drag),
    inset 0 0 0 1px var(--line-selected);
}
</style>
