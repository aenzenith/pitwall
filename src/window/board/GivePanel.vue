<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { language, t } from "../../lib/i18n";
import { boardPlaceAtRefill, placeParams } from "../../lib/line";
import { MODE_LABELS, MODEL_LABELS, MODELS, MODES, refillAt, timeParams, weekLeft } from "../../lib/start";
import { fuel, now } from "../../lib/store";
import type { ClaudeMode, ClaudeModel, StartChoice } from "../../lib/types";

/**
 * "Give to Claude"'s choices for one card, under the card's button: the model (with what is left
 * of its week, when the Fuel reading has a limit for it), the mode, and when: now, or once the
 * 5-hour session refills (offered while the reading knows that moment). At its foot, linking the
 * card to a session already open, and the button that gives it.
 *
 * Drawn in the page, as the board picker's list is (components/ProjectPicker): a native menu
 * holds neither a radio list with a figure beside each line nor a row of segments. The keyboard
 * rests on one of its parts at a time: Tab goes from part to part and stays inside, the arrows
 * pick within the part, Return gives, Esc closes. A press anywhere else closes it too.
 */
const props = defineProps<{
  /** The button it hangs under, in the window. */
  anchor: { left: number; top: number; bottom: number };
  /** What a plain click on "Give to Claude" would start the card with. */
  initial: StartChoice;
  /** The card's project has a session open to link it to. */
  linkable: boolean;
  /** The card's id. */
  card: string;
}>();
// `keyed` on an answer: it came from the keyboard (Return, or Space on its button), which goes
// back to the card's button as the panel closes.
const emit = defineEmits<{
  give: [choice: StartChoice, keyed: boolean];
  /** At the session's refill, as the core reads it. */
  schedule: [choice: StartChoice, keyed: boolean];
  /** The sessions to link to, listed from this point of the window. */
  link: [at: { clientX: number; clientY: number }, keyed: boolean];
  /** `keyed`: closed from the keyboard, which goes back to the card's button. */
  close: [keyed: boolean];
}>();

const model = ref<ClaudeModel | null>(props.initial.model);
const mode = ref<ClaudeMode>(props.initial.mode);
const when = ref<"now" | "refill">("now");

/** Claude Code's own choice, then the models, each with what is left of its week. */
const models = computed(() => [
  { model: null, name: t("start.model.auto"), week: null },
  ...MODELS.map((id) => ({ model: id, name: t(MODEL_LABELS[id]), week: weekLeft(fuel.value, id, now.value, language.value) })),
]);
const modelAt = computed(() => Math.max(0, models.value.findIndex((option) => option.model === model.value)));

/** When the session refills, while the reading knows. */
const refill = computed(() => refillAt(fuel.value, now.value));
const refillTime = computed(() => (refill.value === null ? null : timeParams(refill.value, language.value)));

// The moment passed, or the reading lost it, while the panel was open: there is only now.
watch(refill, (at) => {
  if (at === null) when.value = "now";
});

/** Where the card would stand in the refill's line, among the cards that wait for it already. */
const linePlace = computed(() => (refill.value === null ? null : boardPlaceAtRefill(props.card, refill.value)));

/** Giving at the refill: at its moment as the first in its line, else at the place it would take. */
const refillLabel = computed(() => {
  if (!refillTime.value) return null;
  return linePlace.value !== null && linePlace.value > 1 ? t("start.giveInLine", placeParams(linePlace.value)) : t("start.giveAt", refillTime.value);
});

const waits = computed(() => when.value === "refill" && refillTime.value !== null);
const giveLabel = computed(() => (waits.value && refillLabel.value ? refillLabel.value : t("start.give")));

function choice(): StartChoice {
  return { model: model.value, mode: mode.value };
}

function submit(keyed: boolean): void {
  if (waits.value) emit("schedule", choice(), keyed);
  else emit("give", choice(), keyed);
}

/** A click's `detail` is 0 when Return or Space pressed the button. */
function link(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  emit("link", { clientX: box.left, clientY: box.bottom + 4 }, event.detail === 0);
}

/* ---------- where it hangs ---------- */

const panel = ref<HTMLElement | null>(null);
const place = ref<Record<string, string>>({ left: "0px", top: "0px", visibility: "hidden" });

/** Under the button, their left edges level; over it when the window has no room below, and
 * never past the window's edges. */
function position(): void {
  const el = panel.value;
  if (!el) return;
  const edge = 8;
  const gap = 6;
  const { width, height } = el.getBoundingClientRect();
  const below = props.anchor.bottom + gap;
  const top = below + height <= window.innerHeight - edge ? below : Math.max(edge, props.anchor.top - gap - height);
  const left = Math.max(edge, Math.min(props.anchor.left, window.innerWidth - width - edge));
  place.value = { left: `${left}px`, top: `${top}px` };
}

// The session's refill comes into the reading, or leaves it: the panel's height changes; the
// line moves while it is open: its button's width may.
watch(refillLabel, () => void nextTick(position));

/* ---------- the keyboard ---------- */

const modelList = ref<HTMLElement | null>(null);
const modeGroup = ref<HTMLElement | null>(null);
const whenGroup = ref<HTMLElement | null>(null);
const linkButton = ref<HTMLButtonElement | null>(null);
const giveButton = ref<HTMLButtonElement | null>(null);

/** Its parts, in Tab's order. */
function stops(): HTMLElement[] {
  return [modelList.value, modeGroup.value, whenGroup.value, props.linkable ? linkButton.value : null, giveButton.value].filter((el): el is HTMLElement => el !== null);
}

function step<T>(list: readonly T[], at: number, by: number): T {
  return list[Math.min(list.length - 1, Math.max(0, at + by))];
}

/** An arrow in the part that has the keyboard: the next of its choices, or the other button. */
function arrow(part: HTMLElement, by: number, vertical: boolean): void {
  if (part === modelList.value) model.value = step(models.value, modelAt.value, by).model;
  else if (part === modeGroup.value) mode.value = step(MODES, MODES.indexOf(mode.value), by);
  else if (part === whenGroup.value) when.value = by > 0 ? "refill" : "now";
  else if (!vertical) {
    const buttons = stops().filter((el) => el instanceof HTMLButtonElement);
    step(buttons, buttons.findIndex((el) => el === part), by).focus();
  }
}

function onKey(event: KeyboardEvent): void {
  const part = stops().find((el) => el === event.target) ?? null;

  // The keys that opened it (⇧⌘↵ on the card), again: it closes, as a second press on its arrow
  // does, and nothing is given.
  if (event.key === "Enter" && event.shiftKey && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    event.stopPropagation();
    emit("close", true);
    return;
  }
  if (event.key === "Enter") {
    // On "Link to an open session…" Return is that button's own.
    if (part !== linkButton.value || event.metaKey || event.ctrlKey) {
      event.preventDefault();
      submit(true);
    }
    event.stopPropagation();
    return;
  }
  // A modifier going down is no shortcut yet: ⌘↵ gives from here, as it does on the card.
  if (["Meta", "Control", "Alt", "Shift"].includes(event.key)) return;
  // Any other shortcut is the page's, and the panel gives way to it.
  if (event.metaKey || event.ctrlKey || event.altKey) return emit("close", false);

  switch (event.key) {
    case "Escape":
      emit("close", true);
      break;
    case "Tab": {
      const all = stops();
      const at = part ? all.indexOf(part) : -1;
      all[(at + (event.shiftKey ? -1 : 1) + all.length) % all.length]?.focus();
      break;
    }
    case "ArrowDown":
    case "ArrowUp":
      if (part) arrow(part, event.key === "ArrowDown" ? 1 : -1, true);
      break;
    case "ArrowRight":
    case "ArrowLeft":
      if (part) arrow(part, event.key === "ArrowRight" ? 1 : -1, false);
      break;
    case "Home":
    case "End":
      if (part === modelList.value) model.value = step(models.value, 0, event.key === "Home" ? 0 : models.value.length).model;
      else return;
      break;
    case " ":
      // On a button Space is that button's own.
      if (part instanceof HTMLButtonElement) return;
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

/* ---------- closing ---------- */

/** A press anywhere else closes it; one on the card's own button is that button's to answer. */
function onOutside(event: PointerEvent): void {
  const target = event.target instanceof Element ? event.target : null;
  if (target && (panel.value?.contains(target) || target.closest("[data-give-more]"))) return;
  emit("close", false);
}

/** Whatever it hangs from scrolls away under it. */
function onScroll(event: Event): void {
  if (!(event.target instanceof Node && panel.value?.contains(event.target))) emit("close", false);
}

function away(): void {
  emit("close", false);
}

onMounted(() => {
  position();
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("blur", away);
  window.addEventListener("resize", away);
  // Once it is in its place and in sight: until then it takes no focus.
  void nextTick(() => modelList.value?.focus({ preventScroll: true }));
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("scroll", onScroll, true);
  window.removeEventListener("blur", away);
  window.removeEventListener("resize", away);
});
</script>

<template>
  <!-- A press on a choice leaves the keyboard on the part it was on. -->
  <div ref="panel" class="gp" role="dialog" :aria-label="t('board.giveOptions')" :style="place" @keydown="onKey" @mousedown.prevent @click.stop @dblclick.stop>
    <div class="gp-part">
      <span id="gp-model" class="section-label">{{ t("start.model") }}</span>
      <div ref="modelList" class="gp-models" role="listbox" tabindex="0" aria-labelledby="gp-model" :aria-activedescendant="`gp-model-${modelAt}`">
        <div
          v-for="(option, i) in models"
          :id="`gp-model-${i}`"
          :key="option.model ?? ''"
          :class="['gp-model', { on: option.model === model }]"
          role="option"
          :aria-selected="option.model === model"
          @click="model = option.model"
        >
          <span class="gp-radio" aria-hidden="true"></span>
          <span class="gp-model-name" :data-name="option.name">{{ option.name }}</span>
          <span v-if="option.week" :class="['gp-week', option.week.tone]">{{ option.week.text }}</span>
        </div>
      </div>
    </div>

    <div class="gp-part">
      <span id="gp-mode" class="section-label">{{ t("start.mode") }}</span>
      <div ref="modeGroup" class="seg" role="group" tabindex="0" aria-labelledby="gp-mode">
        <button v-for="option in MODES" :key="option" type="button" tabindex="-1" :aria-pressed="mode === option" @click="mode = option">{{ t(MODE_LABELS[option]) }}</button>
      </div>
    </div>

    <div v-if="refillTime" class="gp-part">
      <span id="gp-when" class="section-label">{{ t("start.when") }}</span>
      <div class="gp-when">
        <div ref="whenGroup" class="seg" role="group" tabindex="0" aria-labelledby="gp-when">
          <button type="button" tabindex="-1" :aria-pressed="when === 'now'" @click="when = 'now'">{{ t("start.when.now") }}</button>
          <button type="button" tabindex="-1" :aria-pressed="when === 'refill'" @click="when = 'refill'">{{ t("start.when.refill") }}</button>
        </div>
        <span class="gp-time">{{ refillTime.time }}</span>
      </div>
    </div>

    <div class="gp-foot">
      <button ref="linkButton" type="button" class="gp-link" tabindex="-1" :disabled="!linkable" @click="link">{{ t("start.link") }}</button>
      <span class="gp-grow"></span>
      <!-- Both of its labels in one place, so the wider one sets its width. -->
      <button ref="giveButton" type="button" class="gp-give" tabindex="-1" @click="submit($event.detail === 0)">
        <span class="gp-sizer" aria-hidden="true">{{ t("start.give") }}</span>
        <span v-if="refillLabel" class="gp-sizer" aria-hidden="true">{{ refillLabel }}</span>
        <span class="gp-give-text">{{ giveLabel }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped src="../settings/settings.css"></style>
<style scoped>
/* Over the board, wherever its card is; as wide as its longest line asks for. */
.gp {
  position: fixed;
  z-index: 60;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 14px;
  width: max-content;
  min-width: 340px;
  max-width: calc(100vw - 16px);
  padding: 14px;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  cursor: default;
}

.gp-part {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  min-width: 0;
}

.gp-models {
  align-self: stretch;
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-control);
}

.gp-model {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 32px;
  padding: 0 8px;
  border-radius: var(--radius-control);
  font-size: 13px;
  color: var(--text-soft);
  white-space: nowrap;
}

/* On the panel's own ground a row's hover is the panel's colour in the dark set: the rail's two
   steps show on it in both, the row under the pointer and, a step further, the one picked. */
.gp-model:hover {
  background: var(--bg-rail-hover);
}

.gp-model.on {
  background: var(--bg-rail-on);
  color: var(--text);
  font-weight: 500;
}

/* The one picked: filled, in the green every box to tick has. */
.gp-radio {
  flex-shrink: 0;
  box-sizing: border-box;
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--idle-ring);
  border-radius: 50%;
}

.gp-model.on .gp-radio {
  border: 4px solid var(--run);
}

/* As wide as its name is when picked (set heavier), so picking one never moves the panel's edge. */
.gp-model-name {
  flex-grow: 1;
  display: inline-flex;
  flex-direction: column;
}

.gp-model-name::after {
  content: attr(data-name);
  height: 0;
  overflow: hidden;
  visibility: hidden;
  font-weight: 500;
}

/* What is left of the model's week: Claude's colour under half, red when it is nearly gone. */
.gp-week {
  flex-shrink: 0;
  margin-left: 12px;
  font-family: var(--font-mono);
  font-size: 11.5px;
  font-weight: 400;
  color: var(--text-subtle);
}

.gp-week.under {
  color: var(--claude-text);
}

.gp-week.low {
  color: var(--crash-text);
}

/* On the picked row's ground the plain figure is a step stronger, so it reads as well as it does
   beside the others. */
.gp-model.on .gp-week.plain {
  color: var(--text-muted);
}

.gp-when {
  display: flex;
  align-items: center;
  gap: 10px;
}

.gp-time {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-soft);
  white-space: nowrap;
}

.gp-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--line);
}

.gp-grow {
  flex-grow: 1;
  min-width: 8px;
}

.gp-link {
  flex-shrink: 0;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.gp-link:hover:not(:disabled) {
  color: var(--text-strong);
}

.gp-link:disabled {
  opacity: 0.5;
}

.gp-give {
  flex-shrink: 0;
  display: inline-grid;
  align-items: center;
  justify-items: center;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--btn-primary-line);
  border-radius: var(--radius-control);
  background: var(--btn-primary-bg);
  font-size: 13px;
  color: var(--btn-primary-text);
  white-space: nowrap;
}

.gp-give:hover {
  background: var(--btn-primary-hover);
}

.gp-give > * {
  grid-area: 1 / 1;
}

.gp-sizer {
  visibility: hidden;
}
</style>
