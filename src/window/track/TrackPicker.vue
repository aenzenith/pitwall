<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { t, type Key } from "../../lib/i18n";
import { CIRCUITS, circuit, LINE_SAMPLES, type CircuitId, type TrackSettings } from "../../lib/track";

/**
 * The circuit picker: the circuit shown and, under it, every circuit as a small drawing of
 * itself, with `shuffle` for another one each time the page opens. Drawn in the page, as the board
 * picker is: a native menu holds no drawings.
 */
const props = defineProps<{
  /** What the settings hold. */
  picked: TrackSettings["circuit"];
  /** The circuit on the page: `shuffle`'s pick. */
  shown: CircuitId;
}>();
const emit = defineEmits<{ pick: [circuit: TrackSettings["circuit"]] }>();

type Choice = TrackSettings["circuit"];

const NAMES: Record<Choice, Key> = {
  night: "track.circuit.night",
  oval: "track.circuit.oval",
  eight: "track.circuit.eight",
  street: "track.circuit.street",
  straight: "track.circuit.straight",
  shuffle: "track.circuit.shuffle",
};
const HINTS: Record<Choice, Key> = {
  night: "track.circuit.nightHint",
  oval: "track.circuit.ovalHint",
  eight: "track.circuit.eightHint",
  street: "track.circuit.streetHint",
  straight: "track.circuit.straightHint",
  shuffle: "track.circuit.shuffleHint",
};

const WIDTH = 160;
const HEIGHT = 88;
/** Three to a row. */
const COLUMNS = 3;

/** A circuit as paths in the drawing's box: a lap is one, the straight four lanes. */
function sketch(id: CircuitId): string[] {
  if (id === "straight") return [18, 35, 53, 70].map((y) => `M14 ${y}H${WIDTH - 14}`);
  const { line } = circuit(id);
  let left = Infinity;
  let right = -Infinity;
  let top = Infinity;
  let bottom = -Infinity;
  for (let i = 0; i < LINE_SAMPLES; i++) {
    left = Math.min(left, line.x[i]);
    right = Math.max(right, line.x[i]);
    top = Math.min(top, line.z[i]);
    bottom = Math.max(bottom, line.z[i]);
  }
  const scale = Math.min((WIDTH - 32) / (right - left), (HEIGHT - 26) / (bottom - top));
  const points: string[] = [];
  for (let i = 0; i < LINE_SAMPLES; i += 2) {
    const x = WIDTH / 2 + (line.x[i] - (left + right) / 2) * scale;
    const y = HEIGHT / 2 + (line.z[i] - (top + bottom) / 2) * scale;
    points.push(`${x.toFixed(1)} ${y.toFixed(1)}`);
  }
  return [`M${points.join("L")}Z`];
}

/** The lights of a drawing: which path, green or orange, where along it. */
const LAP_LIGHTS = [
  { path: 0, claude: false, at: 0.08 },
  { path: 0, claude: true, at: 0.44 },
  { path: 0, claude: false, at: 0.72 },
];
const LANE_LIGHTS = [
  { path: 0, claude: false, at: 0.2 },
  { path: 1, claude: true, at: 0.62 },
  { path: 2, claude: false, at: 0.8 },
  { path: 3, claude: false, at: 0.42 },
];

const options = computed(() =>
  ([...CIRCUITS, "shuffle"] as Choice[]).map((id) => ({
    id,
    name: t(NAMES[id]),
    hint: t(HINTS[id]),
    paths: id === "shuffle" ? [] : sketch(id),
    lights: id === "straight" ? LANE_LIGHTS : LAP_LIGHTS,
    /** A lap takes longer than a lane. */
    seconds: id === "straight" ? 3.6 : 9,
  })),
);

const name = computed(() => t(NAMES[props.picked === "shuffle" ? props.shown : props.picked]));
const label = computed(() => t("track.picker", { name: name.value }));

/* ---------- the list ---------- */

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const list = ref<HTMLElement | null>(null);

const open = ref(false);
/** The option under the pointer or the keyboard. */
const active = ref(0);
/** Under the picker, their right edges level. */
const place = ref<Record<string, string>>({});

function optionId(index: number): string {
  return `track-picker-${index}`;
}

function show(): void {
  const box = button.value?.getBoundingClientRect();
  if (!box || open.value) return;
  place.value = {
    top: `${box.bottom + 4}px`,
    right: `${window.innerWidth - box.right}px`,
    maxHeight: `${Math.max(160, window.innerHeight - box.bottom - 16)}px`,
  };
  open.value = true;
  active.value = Math.max(0, options.value.findIndex((option) => option.id === props.picked));
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => list.value?.focus());
}

function hide(): void {
  open.value = false;
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

/** Closed with the keyboard: it goes back to the picker. */
function close(): void {
  hide();
  button.value?.focus();
}

/** A press anywhere else closes it; one on the picker is the picker's own to answer. */
function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

function pick(id: Choice, keyed = false): void {
  if (keyed) close();
  else hide();
  if (id !== props.picked) emit("pick", id);
}

/** In the list: arrows move (three to a row), ↵ or Space picks, Esc and Tab close; a shortcut
 * closes it and stays the page's. */
function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const last = options.value.length - 1;
  const move = (to: number): void => {
    if (to >= 0 && to <= last) active.value = to;
  };

  switch (event.key) {
    case "ArrowRight":
      move(active.value + 1);
      break;
    case "ArrowLeft":
      move(active.value - 1);
      break;
    case "ArrowDown":
      move(active.value + COLUMNS);
      break;
    case "ArrowUp":
      move(active.value - COLUMNS);
      break;
    case "Home":
      move(0);
      break;
    case "End":
      move(last);
      break;
    case "Enter":
    case " ":
      pick(options.value[active.value].id, true);
      break;
    case "Escape":
    case "Tab":
      close();
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

onBeforeUnmount(hide);
</script>

<template>
  <div ref="root" class="tpick">
    <button
      ref="button"
      type="button"
      class="tpick-button"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-label="label"
      :title="open ? undefined : label"
      @click="open ? hide() : show()"
      @keydown.down.prevent="show"
      @keydown.up.prevent="show"
    >
      <Icon name="flag" :size="13" />
      <span class="tpick-text">{{ name }}</span>
      <Icon name="chevron-down" :size="11" />
    </button>

    <!-- A press on an option leaves the keyboard in the list. -->
    <div
      v-if="open"
      ref="list"
      class="tpick-list"
      role="listbox"
      tabindex="-1"
      :aria-label="t('track.picker.title')"
      :aria-activedescendant="optionId(active)"
      :style="place"
      @keydown="onKey"
      @mousedown.prevent
    >
      <div
        v-for="(option, i) in options"
        :id="optionId(i)"
        :key="option.id"
        :class="['tpick-option', { on: i === active, picked: option.id === picked }]"
        role="option"
        :aria-selected="option.id === picked"
        :aria-label="`${option.name}: ${option.hint}`"
        @mousemove="active = i"
        @click="pick(option.id)"
      >
        <span class="tpick-sketch" aria-hidden="true">
          <Icon v-if="option.id === 'shuffle'" name="shuffle" :size="26" />
          <svg v-else class="sketch" :viewBox="`0 0 ${WIDTH} ${HEIGHT}`" fill="none">
            <path v-for="d in option.paths" :key="d" :d="d" class="road" />
            <path
              v-for="(light, n) in option.lights"
              :key="n"
              :d="option.paths[light.path]"
              pathLength="1"
              :class="['glide', { claude: light.claude }]"
              :style="{ animationDuration: `${option.seconds}s`, animationDelay: `${(-light.at * option.seconds).toFixed(2)}s` }"
            />
          </svg>
        </span>
        <span class="tpick-name">
          <span class="tpick-name-text">{{ option.name }}</span>
          <Icon v-if="option.id === picked" name="check" :size="13" />
        </span>
        <span class="tpick-hint">{{ option.hint }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* A long circuit name gives way; the arrow never does. */
.tpick {
  flex-shrink: 1;
  display: flex;
  min-width: 80px;
  max-width: 200px;
}

.tpick-button {
  flex: 1 1 auto;
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
  color: #c7ccd3;
  white-space: nowrap;
}

.tpick-button:hover,
.tpick-button[aria-expanded="true"] {
  background: #2c3039;
}

.tpick-button svg {
  flex-shrink: 0;
}

.tpick-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Over the page, three drawings to a row; in a short window the list scrolls. */
.tpick-list {
  position: fixed;
  z-index: 60;
  display: grid;
  grid-template-columns: repeat(3, 178px);
  gap: 8px;
  max-width: calc(100vw - 16px);
  padding: 10px;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-panel);
  background: var(--bg-panel);
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.55);
}

/* The keyboard's place is the lit option, not a ring round the list. */
.tpick-list:focus-visible {
  outline: none;
}

.tpick-option {
  display: flex;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
  padding: 8px 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--bg-sidebar);
  cursor: pointer;
}

.tpick-option.on {
  border-color: #3a404a;
  background: #1b1e24;
}

.tpick-option.picked {
  border-color: #5b626d;
  background: var(--bg-selected);
}

.tpick-sketch {
  display: flex;
  align-items: center;
  justify-content: center;
  aspect-ratio: 160 / 88;
  border-radius: 7px;
  background: var(--bg-log);
  color: var(--text-subtle);
}

.sketch {
  width: 100%;
  height: 100%;
}

.road {
  stroke: #262b33;
  stroke-width: 5;
  stroke-linejoin: round;
  stroke-linecap: round;
}

/* A light: a short dash that runs the path round (`pathLength` makes every path one long). */
.glide {
  stroke: var(--run);
  stroke-width: 2.6;
  stroke-linecap: round;
  stroke-dasharray: 0.13 0.87;
  animation: tpick-glide linear infinite;
}

.glide.claude {
  stroke: var(--claude);
}

@keyframes tpick-glide {
  from {
    stroke-dashoffset: 1;
  }
  to {
    stroke-dashoffset: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .glide {
    animation: none;
  }
}

.tpick-name {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 2px;
  font-weight: 500;
  color: var(--text);
}

.tpick-name-text {
  flex-grow: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tpick-name svg {
  flex-shrink: 0;
  color: var(--text-muted);
}

.tpick-hint {
  padding: 0 2px;
  font-size: 12px;
  line-height: 1.4;
  color: var(--text-subtle);
}
</style>
