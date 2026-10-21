<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";
import { startTeam, stopTeam } from "../../lib/teams";
import type { Team } from "../../lib/types";

/**
 * A team's head in the project list: no band of its own, a pill on the line that ties the team's
 * projects together. The pill holds the name in capitals and how many of its projects run; a team
 * of one project shows the count alone (its name is the pill's title and its spoken label). From
 * the pill's left end the line turns down with a round corner into the axis the rows' branches
 * leave (WindowApp: `.trow.branch`), and from its right end it runs on across the list, fading
 * out. Under the pointer anywhere on the team (`lit`), while it is selected or holds the
 * keyboard, the team's start or stop shows at the line's far right, over the rows' own.
 *
 * `team` null: the place where the projects in no team begin. It draws nothing and takes no
 * keyboard; it is there so a row dragged below it leaves its team (lib/groupReorder counts the
 * heads to tell a row's group).
 *
 * A click anywhere on the head selects the team (`select`); the list drags it by the same place.
 * `busy`: a start or a stop of the team is under way, and the button spins until the servers are
 * really up or down. `tabStop`: this head is the list's Tab stop. The head itself takes the
 * keyboard, as a row does, never its button: WebKit's Tab passes buttons by unless the system's
 * keyboard navigation is on. Return and Space on it select the team.
 */
const props = defineProps<{ team: Team | null; total: number; running: number; busy: boolean; selected: boolean; lit: boolean; tabStop: boolean }>();
const emit = defineEmits<{ select: [] }>();

const name = computed(() => props.team?.name ?? "");
const count = computed(() => `${props.running}/${props.total}`);

function toggle(): void {
  if (!props.team) return;
  if (props.running) stopTeam(props.team.id);
  else startTeam(props.team.id);
}
</script>

<template>
  <div v-if="!team" class="ghead loose" data-group="" role="presentation"></div>
  <div
    v-else
    :class="['ghead', { selected, lit, busy }]"
    role="row"
    :tabindex="tabStop ? 0 : -1"
    :data-group="team.id"
    :aria-selected="selected"
    @click="emit('select')"
    @keydown.enter.self.prevent="emit('select')"
    @keydown.space.self.prevent="emit('select')"
  >
    <div class="ghead-box" role="gridcell" aria-colspan="5">
      <span class="tie" aria-hidden="true"></span>
      <span class="pill" :title="name">
        <span v-if="total !== 1" class="pill-name">{{ name }}</span>
        <span v-else class="sr">{{ name }}</span>
        <span :class="['pill-count', { live: running > 0 }]" :aria-label="t('team.runningOf', { running, total })">{{ count }}</span>
      </span>
      <span class="trail" aria-hidden="true"></span>
      <!-- At the line's far end, over the rows' own start and stop. Reached with ←/→ from the
           head, like a row's buttons. -->
      <button
        type="button"
        class="act"
        data-no-drag
        tabindex="-1"
        :disabled="busy || !total"
        :aria-busy="busy"
        :aria-label="t(running ? 'team.stopName' : 'team.startName', { name })"
        :title="t(running ? 'team.stop' : 'team.start')"
        @click.stop="toggle"
      >
        <Spinner v-if="busy" />
        <Icon v-else :name="running ? 'stop' : 'play'" :size="14" />
      </button>
    </div>
  </div>
</template>

<style scoped>
/* The lines' colour: faint at rest, a step up while the pointer is on the team, bright while it is
   selected. The rows of the team take the same (WindowApp: `.trow.branch`). */
.ghead {
  --tie: var(--team-line);
  position: relative;
  height: 40px;
  padding-top: 10px;
  box-sizing: border-box;
  cursor: pointer;
}

.ghead.lit {
  --tie: var(--team-line-hover);
}

.ghead.selected {
  --tie: var(--team-line-selected);
}

/* The projects in no team begin here: the room between groups, and nothing drawn. */
.ghead.loose {
  height: 10px;
  padding: 0;
  cursor: default;
}

.ghead:focus-visible {
  outline: none;
}

.ghead-box {
  height: 30px;
  display: flex;
  align-items: flex-start;
  padding-left: 42px;
}

/* From the pill's left end, round down into the axis at x 8; the first row carries it on. */
.tie {
  position: absolute;
  left: 7px;
  top: 21px;
  bottom: 0;
  width: 37px;
  box-sizing: border-box;
  border-left: 2px solid var(--tie);
  border-top: 2px solid var(--tie);
  border-top-left-radius: 10px;
}

.pill {
  position: relative;
  min-width: 0;
  height: 20px;
  margin-top: 2px;
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 0 9px;
  border: 2px solid var(--tie);
  border-radius: 10px;
  background: var(--bg-app);
}

/* A long name gives way; the count never does. */
.pill-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-nav);
}

:lang(zh) .pill-name,
:lang(ja) .pill-name {
  letter-spacing: 0;
}

.ghead.selected .pill-name {
  color: var(--text-strong);
}

.pill-count {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.pill-count.live {
  color: var(--run-text);
}

.sr {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

/* The team's start or stop at the line's far end, centred over the rows' own (a row's is 30px,
   its middle 25px from the row's right edge): only while the team is under the pointer,
   selected, holds the keyboard or is starting or stopping. The line runs on under it. */
.act {
  position: absolute;
  right: 10px;
  top: 7px;
  width: 30px;
  height: 30px;
  display: none;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 7px;
  background: var(--bg-app);
  color: var(--text-muted);
}

.ghead:is(.lit, .selected, .busy, :focus-within) .act {
  display: inline-flex;
}

.act:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Nothing to start: dimmed. Spinning, it keeps its colour. */
.act:disabled:not([aria-busy="true"]) {
  opacity: 0.4;
}

/* From the pill's right end the line runs on across the list, fading out. */
.trail {
  flex: 1 1 0;
  min-width: 24px;
  height: 2px;
  margin-top: 11px;
  background: linear-gradient(90deg, var(--tie), transparent);
}

/* The keyboard's place is the whole head; the ring goes round the pill. */
.ghead:focus-visible .pill {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}

/* In hand: the pill lifts, the shadow below only. */
.ghead.dragging .pill {
  background: var(--bg-selected);
  box-shadow: 0 10px 18px -10px var(--shade-drag);
}
</style>
