<script setup lang="ts">
import { computed, ref } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { clock } from "../../lib/day";
import type { CardStatus } from "../../lib/board";
import { moneyText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import { places, waitText } from "../../lib/line";
import { stamp } from "../../lib/period";
import { keys } from "../../lib/platform";
import { useGrow } from "../../lib/slide";
import { choiceOf, MODE_LABELS, MODEL_LABELS } from "../../lib/start";
import type { Card, SessionRow } from "../../lib/types";
import OriginLabel from "../sessions/OriginLabel.vue";
import { useBoardActions, type CardBusy } from "./context";

/**
 * One card. `full`: a project's board (title, note, its session, buttons when selected); `row`:
 * the same board drawn as a list (the title with a line of its note under it, cut to the row
 * selected or not, then its session's state and where it runs each in a column of their own);
 * `compact`: a lane of every project's board (one line, its status); `done`: a row of the done
 * column, with when it was done (`dated`: its day too, among every day's).
 * `lifted`: being dragged (its place stays empty); `ghost`: the image that follows the pointer.
 * `givable` false: a card of a folder no longer listed, which is never given to Claude (no button
 * offers it).
 *
 * A card up next that was told to wait (`card.schedule`) says so wherever it is drawn: a tag with
 * its moment and, behind the first of the refill's line, its place there (once its refill came
 * and a card before it went to Claude, that it waits its turn, and where: lib/line), and one each
 * for a model and a mode chosen for it. Selected, it offers to be given now or to
 * wait no longer, in place of the buttons that give it.
 */
const props = withDefaults(
  defineProps<{
    card: Card;
    variant?: "full" | "row" | "compact" | "done";
    status?: CardStatus | null;
    row?: SessionRow | null;
    selected?: boolean;
    busy?: CardBusy;
    error?: string;
    draggable?: boolean;
    lifted?: boolean;
    ghost?: boolean;
    givable?: boolean;
    dated?: boolean;
  }>(),
  { variant: "full", status: null, row: null, selected: false, busy: null, error: "", draggable: false, lifted: false, ghost: false, givable: true, dated: false },
);

const actions = useBoardActions();

/** Its session or terminal can be brought up. */
const revealable = computed(() => !props.status?.closed && (props.card.session !== null || props.card.terminal !== null));

/** Today's cost of its session, as the API would charge it. */
const cost = computed(() => {
  const spend = props.row?.spend;
  return spend && (spend.costUsd > 0 || spend.priced) ? moneyText(spend.costUsd, "USD", language.value) : "";
});

/** Its note shows under the title: on a project's board, as a card or as a row. */
const noted = computed(() => !!props.card.note && (props.variant === "full" || props.variant === "row"));

/** Where its session runs shows, and what it cost today. */
const located = computed(() => (props.variant === "full" || props.variant === "row") && props.card.column === "claude" && !!props.row && !props.status?.closed);

const giveKeys = computed(() => keys("mod+Enter"));
/** The panel of ways to give it opens from the keyboard too. */
const moreKeys = computed(() => keys("mod+shift+Enter"));

/** Told to wait: when it is given (`when`: its moment and its place in the refill's line, or its
 * turn once the refill came), and what was chosen for it. */
const waiting = computed(() => {
  const schedule = props.card.column === "queued" && props.variant !== "done" ? props.card.schedule : null;
  if (!schedule) return null;
  return { when: waitText(schedule, places.value.get(props.card.id) ?? null, language.value), ...choiceOf(schedule.start) };
});

/** The buttons it shows: giving when selected and queued (a card that waits: giving it now, or
 * calling the wait off); ending a closed session's card; a finished card's look and close when
 * selected (or not seen yet, on a project's board). */
const buttons = computed<"give" | "waiting" | "closed" | "review" | "bringUp" | null>(() => {
  if (props.ghost || props.variant === "done") return null;
  const { column } = props.card;
  if (column === "queued") return props.selected && props.givable ? (waiting.value ? "waiting" : "give") : null;
  if (column === "claude") {
    if (props.status?.closed) return "closed";
    return props.variant === "compact" && props.selected && revealable.value ? "bringUp" : null;
  }
  if (column === "review") return props.selected || (props.variant !== "compact" && props.status?.unseen) ? "review" : null;
  return null;
});

const root = ref<HTMLElement | null>(null);

// Its buttons come and go with the selection, an error with what they did: the card grows to hold
// them and shrinks back, rather than jumping there, and the cards under it follow.
useGrow(root, () => `${buttons.value} ${waiting.value !== null} ${props.error}`);

function onClick(): void {
  if (actions.isClick()) actions.select(props.card.id);
}

function onMenu(event: MouseEvent): void {
  event.preventDefault();
  actions.select(props.card.id);
  actions.menu(event, props.card.id);
}

function onPress(event: PointerEvent): void {
  if (props.draggable) actions.press(event, props.card.id);
}

/** A status that can bring its session up does, and stays out of the card's own click and
 * double-click; one that can't is just part of the card. */
function onStatus(event: MouseEvent): void {
  if (!revealable.value) return;
  event.stopPropagation();
  if (event.type === "click") actions.reveal(props.card.id);
}

/** The model, the mode and the moment to give it with, in a panel under the button. */
function giveMenu(event: Event): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  actions.giveMenu({ clientX: box.left, clientY: box.bottom + 4 }, props.card.id);
}
</script>

<template>
  <div
    ref="root"
    :class="['bcard', variant, { selected, lifted, ghost, closed: status?.closed, stated: !!status }]"
    role="listitem"
    :data-card="ghost ? undefined : card.id"
    :tabindex="ghost ? undefined : selected ? 0 : -1"
    :aria-current="selected ? 'true' : undefined"
    :aria-hidden="ghost ? 'true' : undefined"
    :title="ghost ? undefined : t(draggable ? 'board.cardTitleDrag' : 'board.cardTitle')"
    @pointerdown="onPress"
    @click="onClick"
    @dblclick="actions.edit(card.id)"
    @contextmenu="onMenu"
  >
    <!-- Done: a check, the title, when. -->
    <template v-if="variant === 'done'">
      <Icon name="check" :size="13" class="bcard-check" />
      <span class="bcard-done-title">{{ card.title }}</span>
      <span class="bcard-time">{{ dated ? stamp(card.movedAt, Date.now()) : clock(card.movedAt) }}</span>
    </template>

    <template v-else>
      <!-- A row lays these out itself; on a card they are the card's own lines (bcard-text). -->
      <div class="bcard-text">
        <span class="bcard-title">{{ card.title }}</span>
        <span v-if="noted" class="bcard-note">{{ card.note }}</span>
      </div>

      <!-- Told to wait: when it is given (or that it waits its turn), and the model and the mode
           chosen for it. -->
      <div v-if="waiting" class="bcard-tags">
        <span class="btag clock"><Icon name="clock" :size="11" /><span class="btag-text">{{ waiting.when }}</span></span>
        <span v-if="waiting.model" class="btag">{{ t(MODEL_LABELS[waiting.model]) }}</span>
        <span v-if="waiting.mode !== 'normal'" class="btag">{{ t(MODE_LABELS[waiting.mode]) }}</span>
      </div>

      <!-- Its session's state; a click brings the session up where it runs. -->
      <component
        :is="revealable ? 'button' : 'span'"
        v-if="status"
        :type="revealable ? 'button' : undefined"
        :tabindex="revealable ? (selected ? 0 : -1) : undefined"
        :class="['bcard-status', status.tone, { link: revealable }]"
        :title="revealable ? t('sessions.action.bringUpTitle') : status.closed ? t('board.closedTitle') : undefined"
        @click="onStatus"
        @dblclick="onStatus"
        data-dblopen
      >
        <span v-if="status.mark" :class="['bmark', status.mark]" aria-hidden="true"></span>
        <span class="bcard-status-text">{{ status.text }}</span>
      </component>

      <!-- A row's last column (bcard-end), kept by every row that has a state so the states line
           up: where it runs and what it cost today, or its buttons. -->
      <div v-if="located || buttons || (variant === 'row' && status)" class="bcard-end">
        <span v-if="located && row" class="bcard-where">
          <OriginLabel :origin="row.origin" :project="row.project" />
          <span v-if="cost" class="bcard-cost" :title="t('board.costTitle')">{{ cost }}</span>
        </span>

        <div v-if="buttons" class="bcard-actions" data-no-drag @dblclick.stop>
          <template v-if="buttons === 'give'">
            <!-- One button in two parts: giving it as the project starts its sessions, and the
                 panel of other ways. -->
            <span class="bsplit">
              <button type="button" class="bbtn claude" :disabled="busy === 'give'" :title="giveKeys" @click.stop="actions.give(card.id, 'new')">
                <Spinner v-if="busy === 'give'" :size="11" />
                <ClaudeLogo v-else :size="12" />
                <span class="bbtn-text">{{ t("board.give") }}</span>
              </button>
              <button
                type="button"
                :class="['bbtn', 'claude', 'square', { open: actions.panelFor() === card.id }]"
                data-give-more
                aria-haspopup="dialog"
                :aria-expanded="actions.panelFor() === card.id"
                :aria-label="t('board.giveOptions')"
                :title="actions.panelFor() === card.id ? undefined : `${t('board.giveOptions')} (${moreKeys})`"
                :disabled="busy === 'give'"
                @click.stop="giveMenu"
                @keydown.down.prevent.stop="actions.panelFor() === card.id || giveMenu($event)"
              >
                <Icon name="chevron-down" :size="11" />
              </button>
            </span>
            <span v-if="variant !== 'compact'" class="bcard-keys" aria-hidden="true">{{ giveKeys }}</span>
          </template>

          <template v-else-if="buttons === 'waiting'">
            <button type="button" class="bbtn quiet strong" :disabled="!!busy" :title="giveKeys" @click.stop="actions.giveNow(card.id)">
              <Spinner v-if="busy === 'give'" :size="11" class="bbtn-over" />
              <span :class="['bbtn-text', { unseen: busy === 'give' }]">{{ t("start.giveNow") }}</span>
            </button>
            <button type="button" class="bbtn quiet" :disabled="!!busy" @click.stop="actions.unschedule(card.id)">
              <span class="bbtn-text">{{ t("common.cancel") }}</span>
            </button>
          </template>

          <template v-else-if="buttons === 'closed'">
            <button v-if="givable" type="button" class="bbtn claude" :disabled="busy === 'give'" @click.stop="actions.giveAgain(card.id)">
              <Spinner v-if="busy === 'give'" :size="11" />
              <span class="bbtn-text">{{ t("board.giveAgain") }}</span>
            </button>
            <button type="button" class="bbtn" :disabled="busy === 'done'" @click.stop="actions.done(card.id)">
              <Spinner v-if="busy === 'done'" :size="11" />
              <span class="bbtn-text">{{ t("board.markDone") }}</span>
            </button>
          </template>

          <template v-else-if="buttons === 'review'">
            <button v-if="revealable" type="button" class="bbtn" :disabled="busy === 'reveal'" :title="t('sessions.action.bringUpTitle')" @click.stop="actions.reveal(card.id)">
              <Spinner v-if="busy === 'reveal'" :size="11" />
              <Icon v-else name="external" :size="12" />
              <span class="bbtn-text">{{ t("board.openSession") }}</span>
            </button>
            <button type="button" class="bbtn" :disabled="busy === 'done'" @click.stop="actions.done(card.id)">
              <Spinner v-if="busy === 'done'" :size="11" />
              <Icon v-else name="check" :size="12" />
              <span class="bbtn-text">{{ t("board.markDone") }}</span>
            </button>
          </template>

          <template v-else-if="buttons === 'bringUp'">
            <button type="button" class="bbtn claude" :disabled="busy === 'reveal'" :title="t('sessions.action.bringUpTitle')" @click.stop="actions.reveal(card.id)">
              <Spinner v-if="busy === 'reveal'" :size="11" />
              <span class="bbtn-text">{{ t("sessions.action.bringUp") }}</span>
            </button>
          </template>
        </div>
      </div>

      <p v-if="error" class="bcard-error" role="alert">{{ error }}</p>
    </template>

    <Spinner v-if="busy === 'move'" :size="11" class="bcard-spin" />
  </div>
</template>

<style scoped>
.bcard {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
  padding: 10px 12px;
  border-radius: var(--radius-card);
  background: var(--bg-card);
  border: 1px solid var(--line-card);
  cursor: pointer;
}

.bcard:hover {
  background: var(--bg-card-hover);
}

.bcard.selected {
  background: var(--bg-hover);
  border-color: var(--line-card-on);
}

/* The keyboard's card: the ring inside, so the column's edge never cuts it. */
.bcard:focus-visible {
  outline-offset: -2px;
}

/* On its way to a new height (useGrow), as its buttons come or go: it shows them as far as it has
   got, and its lines keep their own height meanwhile rather than being squeezed into it. */
.bcard[data-growing] {
  overflow: hidden;
}

.bcard:not(.row)[data-growing] :is(.bcard-title, .bcard-note, .bcard-tags, .bcard-status, .bcard-where, .bcard-actions, .bcard-error) {
  flex-shrink: 0;
}

.bcard.compact {
  gap: 4px;
  padding: 7px 10px;
  border-radius: var(--radius-control);
}

/* On a card the title and note, and where it runs and its buttons, are lines of the card itself;
   a row gives each pair a box of its own. */
.bcard-text,
.bcard-end {
  display: contents;
}

/* Being dragged: its place goes (the column shows where it would land instead). */
.bcard.lifted {
  display: none;
}

/* The image under the pointer while dragging. */
.bcard.ghost {
  pointer-events: none;
  background: var(--bg-hover);
  border-color: var(--line-card-on);
  box-shadow: var(--shadow-menu);
}

.bcard-title {
  font-weight: 500;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.compact .bcard-title {
  font-size: 12.5px;
  font-weight: 400;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bcard-note {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-subtle);
  white-space: pre-line;
  overflow-wrap: anywhere;
  display: -webkit-box;
  -webkit-line-clamp: 4;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* A card told to wait: when it is given in Claude's colours, what was chosen for it plain. A tag
   keeps to one line; a card too narrow for them all sets them on a second. */
.bcard-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
}

.btag {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  height: 20px;
  padding: 0 8px;
  border-radius: 10px;
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-strong);
  font-size: 11px;
  font-weight: 500;
  color: var(--text-soft);
  white-space: nowrap;
}

.btag.clock {
  background: var(--claude-bg);
  box-shadow: inset 0 0 0 1px var(--claude-line);
  color: var(--claude-text);
}

.btag svg {
  flex-shrink: 0;
}

.btag-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Its session's state: a filled dot waits on you, a breathing ring works, a grey ring is quiet. */
.bcard-status {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
  background: transparent;
  font: inherit;
  font-size: 12px;
  text-align: left;
  white-space: nowrap;
}

.bcard-status.hot {
  color: var(--claude-text);
}

.bcard-status.live {
  color: var(--text-muted);
}

.bcard-status.quiet {
  color: var(--text-subtle);
}

.bcard-status.link:hover .bcard-status-text {
  text-decoration: underline;
  text-underline-offset: 2px;
}

.bcard-status-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bmark {
  width: 8px;
  height: 8px;
  flex-shrink: 0;
  box-sizing: border-box;
  border-radius: 50%;
}

.compact .bmark {
  width: 7px;
  height: 7px;
}

.bmark.waiting {
  background: var(--claude);
}

.bmark.working {
  border: 1.5px solid var(--claude);
  animation: bcard-breathe 1.6s ease-in-out infinite;
}

.bmark.idle {
  width: 7px;
  height: 7px;
  border: 1.5px solid var(--idle-ring);
}

@keyframes bcard-breathe {
  50% {
    opacity: 0.35;
  }
}

@media (prefers-reduced-motion: reduce) {
  .bmark.working {
    animation: none;
  }
}

.bcard-where {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.bcard-where .origin {
  flex-shrink: 1;
  overflow: hidden;
}

.bcard-cost {
  flex-shrink: 0;
  margin-left: auto;
  font-family: var(--font-mono);
}

.bcard-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  margin-top: 2px;
}

.compact .bcard-actions {
  margin-top: 4px;
}

/* A label never wraps; a narrow card shortens it. */
.bbtn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
  white-space: nowrap;
}

.compact .bbtn {
  height: 26px;
  padding: 0 9px;
}

.bbtn:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.bbtn:disabled {
  opacity: 0.7;
}

.bbtn svg,
.bbtn .spinner {
  flex-shrink: 0;
}

.bbtn-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Claude's: its colours, as everything that belongs to Claude. */
.bbtn.claude {
  border-color: var(--btn-claude-line);
  background: var(--btn-claude-bg);
  color: var(--claude-soft);
}

.bbtn.claude:hover:not(:disabled) {
  background: var(--btn-claude-hover);
}

.bbtn.square {
  flex-shrink: 0;
  width: 28px;
  padding: 0;
  justify-content: center;
}

/* "Give to Claude" and the arrow that opens its panel: one button in two parts. */
.bsplit {
  display: inline-flex;
  min-width: 0;
}

.bsplit > .bbtn:first-child {
  border-right: 0;
  border-radius: var(--radius-control) 0 0 var(--radius-control);
}

.bsplit > .bbtn.square {
  width: 26px;
  border-radius: 0 var(--radius-control) var(--radius-control) 0;
}

.bsplit > .bbtn.square.open {
  background: var(--btn-claude-hover);
}

/* A waiting card's own: no box, as the links of a line are. The first is the one to press. */
.bbtn.quiet {
  position: relative;
  height: 26px;
  padding: 0 8px;
  border-color: transparent;
  background: transparent;
  color: var(--text-muted);
}

.bbtn.quiet.strong {
  color: var(--text);
}

/* Its label starts where the card's lines do. */
.bcard-actions > .bbtn.quiet:first-child {
  margin-left: -8px;
}

.row .bcard-actions > .bbtn.quiet:first-child {
  margin-left: 0;
}

/* Russian's two are the longest and a lane's card is the narrowest: there they sit closer, so
   neither is shortened. */
:lang(ru) .compact .bcard-actions:has(> .bbtn.quiet) {
  gap: 0;
}

:lang(ru) .compact .bcard-actions > .bbtn.quiet {
  padding: 0 6px;
}

:lang(ru) .compact .bcard-actions > .bbtn.quiet:first-child {
  margin-left: -6px;
}

.bbtn.quiet:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Busy, its label stays, unseen, under the spinner: the button keeps its width. */
.bbtn-text.unseen {
  visibility: hidden;
}

.bbtn-over {
  position: absolute;
  inset: 0;
  margin: auto;
}

.bcard-keys {
  flex-shrink: 0;
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.bcard-error {
  margin: 0;
  font-size: 12px;
  line-height: 1.4;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.bcard-spin {
  position: absolute;
  top: 8px;
  right: 8px;
  color: var(--text-muted);
}

/* A row of the board's list: the title, then its session's state and where it runs (or its
   buttons) each in a column of their own, so they line up down the list. */
.bcard.row {
  flex-direction: row;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 12px;
  min-height: 36px;
  padding: 3px 8px;
  border-color: transparent;
  border-radius: var(--radius-control);
  background: transparent;
}

.bcard.row:hover {
  background: var(--bg-row-hover);
}

.bcard.row.selected {
  background: var(--bg-selected);
}

/* Dragged: solid, as a done row is. */
.bcard.row.ghost {
  background: var(--bg-hover);
  border-color: var(--line-card-on);
}

.row .bcard-text {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 5px 0;
}

.row .bcard-title {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Under the title: one line, as much as fits. Selected, both stay cut: the details show them
   whole. */
.row .bcard-note {
  display: block;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.row .bcard-status {
  flex: 0 0 184px;
}

/* A row's tags follow its title on the same line. */
.row .bcard-tags {
  flex: 0 0 auto;
  flex-wrap: nowrap;
}

.row .bcard-end {
  flex: 0 0 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

/* After a state: at least as wide as what it usually holds, so the states line up. */
.row.stated .bcard-end {
  min-width: 180px;
}

.row .bcard-where {
  flex: 1 1 auto;
}

.row .bcard-actions {
  margin-top: 0;
}

.row .bcard-error {
  flex: 0 0 100%;
}

.bcard.row .bcard-spin {
  position: static;
}

/* Done: a compact row. */
.bcard.done {
  flex-direction: row;
  align-items: center;
  gap: 8px;
  padding: 8px 6px;
  border-color: transparent;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 12.5px;
  color: var(--text-muted);
}

.bcard.done:hover {
  background: var(--bg-row-hover);
}

.bcard.done.selected {
  background: var(--bg-selected);
  border-color: var(--line-selected);
}

/* Dragged: solid like any card under the pointer, so the rows it passes don't show through. */
.bcard.done.ghost {
  background: var(--bg-hover);
  border-color: var(--line-card-on);
}

.bcard.done .bcard-spin {
  position: static;
}

.bcard-check {
  flex-shrink: 0;
  color: var(--idle-ring);
}

.bcard-done-title {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bcard-time {
  flex-shrink: 0;
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}
</style>
