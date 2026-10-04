<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import { startCircuit, type Inset, type Light, type Renderer } from "../../lib/circuit";
import { phase as phaseText } from "../../lib/format";
import { t } from "../../lib/i18n";
import { visible } from "../../lib/store";
import { grid, laneOffsets, type Circuit, type Mover, type Stander, type TrackSettings } from "../../lib/track";
import type { Project, Turn } from "../../lib/types";

/**
 * The circuit and what runs on it: the lights are drawn on a canvas (lib/circuit), their tags and
 * the lights that stand still (Claude waiting at the pit wall, a crashed server) are elements over
 * it, moved each frame to where the circuit puts them. For the pointer only: the tower beside it
 * lists the same projects for the keyboard and VoiceOver.
 */
const props = defineProps<{
  circuit: Circuit;
  /** In the tower's order. */
  projects: Project[];
  /** What a project's tag reads, by its path. */
  tags: Map<string, string>;
  settings: TrackSettings;
  selected: string | null;
  /** The project pointed at in the tower. */
  hot: string | null;
  /** The room the page keeps round the circuit. */
  inset: Inset;
}>();
const emit = defineEmits<{ select: [path: string]; open: [path: string] }>();

/** At most this many frames a second: a 120 Hz screen draws every other one. */
const FPS = 60;
/** A light that joins the circuit grows its trail over this long, in seconds. */
const GROW = 1.4;

const GREEN = [0.45, 0.79, 0.57] as const;
const ORANGE = [0.94, 0.53, 0.24] as const;
const AMBER = [0.8, 0.65, 0.0] as const;

const canvas = ref<HTMLCanvasElement | null>(null);
const supported = ref(true);
const reducedMotion = ref(false);
/** Counts up when the canvas or the circuit's place in it changes: what stands still is placed again. */
const laidOut = ref(0);

const straight = computed(() => props.circuit.id === "straight");
const motion = computed(() => (reducedMotion.value ? "still" : props.settings.motion));
const running = computed(() => visible.value && motion.value !== "still" && supported.value);

let renderer: Renderer | null = null;
let frame = 0;
let lastAt = 0;
let lastDrawn = 0;
/** The circuit's time, in seconds: it moves only while the lights do. */
let clock = 0;
/** When each light joined, by its key, on the page's own clock (ms). */
const joined = new Map<string, number>();
let observer: ResizeObserver | null = null;
let motionQuery: MediaQueryList | null = null;

/** On the straight a project's name stands at its lane's start: the lanes begin further in. */
const room = computed<Inset>(() => {
  if (!straight.value) return props.inset;
  const widest = Math.max(0, ...props.projects.map((project) => (props.tags.get(project.path) ?? "").length));
  return { ...props.inset, left: props.inset.left + 28 + widest * 6.8, bottom: props.inset.bottom - 34 };
});

const lanes = computed(() => (straight.value ? laneOffsets(props.projects.length) : []));
const waiting = computed(() => props.projects.filter((project) => project.claude).length);

/** A waiting light's tag says what Claude waits with while the wall has the room. */
const tagWidth = (long: boolean): number => {
  const letters = props.settings.labels === "name" ? Math.max(3, ...props.projects.filter((p) => p.claude).map((p) => p.name.length)) : 3;
  return 34 + letters * 6.8 + (long ? 96 : 0);
};
const long = computed(() => {
  void laidOut.value;
  const width = (canvas.value?.clientWidth ?? 0) - room.value.left - room.value.right;
  return !straight.value && waiting.value > 0 && waiting.value * (tagWidth(true) + 12) <= width * 0.9;
});
/** How far apart the wall's places are, in world units. */
const gap = computed(() => {
  void laidOut.value;
  return (tagWidth(long.value) + 12) / (renderer?.scale() || 100);
});
/** Half the pit wall: long enough for everyone waiting. */
const wall = computed(() => Math.min(props.circuit.line.length * 0.2, Math.max(1.3, (waiting.value * gap.value) / 2 + 0.3)));

const scene = computed(() => grid(props.projects, props.circuit, gap.value));

const byPath = computed(() => new Map(props.projects.map((project) => [project.path, project])));
const tag = (path: string): string => props.tags.get(path) ?? "";
const name = (path: string): string => byPath.value.get(path)?.name ?? "";

const TURN = { finished: "claude.finished", asking: "claude.asking", permission: "claude.permission" } as const;
const turnText = (turn: Turn | null): string => (turn ? t(TURN[turn.kind]) : "");

/** A tag shows when its project is selected or pointed at, and otherwise as the settings say. */
function shows(path: string): boolean {
  return props.settings.labels !== "hover" || path === props.selected || path === props.hot;
}

const riders = computed(() => scene.value.movers.filter((mover) => mover.tagged));

type Spot = { key: string; path: string; kind: Stander["kind"] | "lane"; x: number; y: number; tagX: number; tagY: number; text: string; label: string; side: "under" | "beside" | "after" | "before" };

/** What stands still, where the circuit puts it: placed when the layout or the projects change, not
 * each frame. */
const spots = computed<Spot[]>(() => {
  void laidOut.value;
  const drawn = renderer;
  if (!drawn || !supported.value) return [];
  const { half, wall: at } = props.circuit;
  const list: Spot[] = [];

  for (const stander of scene.value.standers) {
    const [x, y] = drawn.project(stander.t, stander.offset);
    const project = byPath.value.get(stander.path);
    const base = { key: stander.key, path: stander.path, kind: stander.kind, x, y };
    if (stander.kind === "waiting") {
      const what = turnText(stander.turn);
      // Under the pit wall, centred on its place.
      const [, wallY] = straight.value ? [x, y] : drawn.project(stander.t, at.side * half * 2.3);
      list.push({ ...base, tagX: x, tagY: wallY + 10, text: long.value ? `${tag(stander.path)} · ${what}` : straight.value ? "" : tag(stander.path), label: `${name(stander.path)}: ${what}`, side: "under" });
    } else if (stander.kind === "crashed") {
      list.push({ ...base, tagX: x, tagY: y, text: straight.value ? t("status.crashed") : `${tag(stander.path)} · ${t("status.crashed")}`, label: `${name(stander.path)}: ${t("status.crashed")}`, side: straight.value ? "after" : "beside" });
    } else if (stander.kind === "busy") {
      const doing = project ? phaseText(project) : "";
      list.push({ ...base, tagX: x, tagY: y, text: straight.value ? doing : `${tag(stander.path)} · ${doing}`, label: `${name(stander.path)}: ${doing}`, side: straight.value ? "after" : "beside" });
    } else {
      list.push({ ...base, tagX: x, tagY: y, text: "", label: name(stander.path), side: "beside" });
    }
  }

  // The straight: every project's tag before its lane's start.
  if (straight.value) {
    props.projects.forEach((project, index) => {
      const [x, y] = drawn.project(0, lanes.value[index]);
      list.push({ key: `${project.path}#lane`, path: project.path, kind: "lane", x, y, tagX: x, tagY: y, text: tag(project.path), label: project.name, side: "before" });
    });
  }
  return list;
});

/** The pit wall's name, under the tags of whoever waits at it. */
const wallName = computed(() => {
  void laidOut.value;
  if (!renderer || !supported.value || straight.value) return null;
  const { half, wall: at } = props.circuit;
  const [x, y] = renderer.project(at.t, at.side * half * 2.3);
  return { x, y: y + (waiting.value ? 38 : 12) };
});

/** Where "nothing runs" stands: the middle of the room the circuit has. */
const empty = computed(() => !straight.value && supported.value && scene.value.movers.length === 0 && scene.value.standers.length === 0);

/* ---------- the tags that ride ---------- */

const riderEls = new Map<string, HTMLElement>();

function setRider(key: string, el: unknown): void {
  if (el instanceof HTMLElement) riderEls.set(key, el);
  else riderEls.delete(key);
}

function colour(mover: Mover): readonly [number, number, number] {
  if (mover.kind === "claude") return ORANGE;
  return mover.warn ? AMBER : GREEN;
}

/** One frame: the lights, then the tags that ride with them. */
function render(): void {
  const drawn = renderer;
  if (!drawn) return;
  const { line } = props.circuit;
  const calm = motion.value === "calm";
  const still = motion.value === "still";
  const length = still ? 0.3 : calm ? 0.6 : 1;
  const at = performance.now();
  const movers = scene.value.movers;

  const lights: Light[] = movers.map((mover) => {
    let since = joined.get(mover.key);
    if (since === undefined) {
      since = at;
      joined.set(mover.key, since);
    }
    const grown = still ? 1 : Math.min(1, (at - since) / 1000 / GROW);
    const lit = mover.path === props.selected || mover.path === props.hot;
    const laps = mover.phase + (clock * mover.speed) / line.length;
    return { head: laps - Math.floor(laps), trail: (mover.trail / line.length) * length * grown, gain: (lit ? 1.3 : 1) * (0.25 + 0.75 * grown), offset: mover.offset, colour: colour(mover) };
  });
  drawn.draw(clock, lights, props.settings.glow);

  movers.forEach((mover, index) => {
    const el = riderEls.get(mover.key);
    if (!el) return;
    const [x, y] = drawn.project(lights[index].head, mover.offset);
    el.style.transform = `translate3d(${x.toFixed(1)}px, ${y.toFixed(1)}px, 0)`;
  });

  // A light that left is forgotten: it grows again when it comes back.
  if (joined.size > movers.length) {
    const keys = new Set(movers.map((mover) => mover.key));
    for (const key of joined.keys()) if (!keys.has(key)) joined.delete(key);
  }
}

function tick(at: number): void {
  frame = requestAnimationFrame(tick);
  if (at - lastDrawn < 1000 / FPS - 2) return;
  // A long gap (the window was hidden) is not raced through.
  const step = Math.min(0.1, (at - (lastAt || at)) / 1000);
  lastAt = at;
  lastDrawn = at;
  clock += step * (motion.value === "calm" ? 0.5 : 1);
  render();
}

function stop(): void {
  cancelAnimationFrame(frame);
  frame = 0;
}

/** Runs while there is something to move and someone to see it; otherwise one frame, as it stands. */
function play(): void {
  stop();
  if (!renderer) return;
  if (!running.value) {
    render();
    return;
  }
  lastAt = 0;
  frame = requestAnimationFrame(tick);
}

/** What the circuit's marks were last drawn for. */
let marked: { circuit: Circuit; lanes: number; wall: number } | null = null;

/** The circuit's own marks, drawn again when the circuit, the number of lanes or the wall's length
 * is no longer what they were drawn for. */
function mark(): void {
  if (!renderer) return;
  if (marked && marked.circuit === props.circuit && marked.lanes === lanes.value.length && marked.wall === wall.value) return;
  marked = { circuit: props.circuit, lanes: lanes.value.length, wall: wall.value };
  renderer.setCircuit(props.circuit, lanes.value, wall.value);
}

/** Fits the circuit into its room. The wall is as long as the tags of whoever waits need, and the
 * scale decides that: its marks follow. */
function layout(): void {
  if (!renderer) return;
  renderer.layout(room.value);
  laidOut.value++;
  mark();
}

function setCircuit(): void {
  mark();
  layout();
}

function start(): void {
  const target = canvas.value;
  if (!target) return;
  renderer = startCircuit(target);
  supported.value = renderer !== null;
  marked = null;
  if (!renderer) return;
  setCircuit();
  play();
}

/** The GPU took the context back (sleep, a driver reset): nothing is drawn until it returns. */
function onLost(event: Event): void {
  event.preventDefault();
  stop();
  renderer = null;
}

function onMotion(): void {
  reducedMotion.value = motionQuery?.matches ?? false;
}

onMounted(() => {
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  motionQuery.addEventListener("change", onMotion);
  onMotion();
  canvas.value?.addEventListener("webglcontextlost", onLost);
  canvas.value?.addEventListener("webglcontextrestored", start);
  start();
  if (canvas.value) {
    observer = new ResizeObserver(() => {
      layout();
      if (!frame) render();
    });
    observer.observe(canvas.value);
  }
});

onBeforeUnmount(() => {
  stop();
  observer?.disconnect();
  motionQuery?.removeEventListener("change", onMotion);
  canvas.value?.removeEventListener("webglcontextlost", onLost);
  canvas.value?.removeEventListener("webglcontextrestored", start);
  renderer?.stop();
  renderer = null;
});

// The circuit's own marks: another circuit, another number of lanes, a longer wall.
watch([() => props.circuit, lanes, wall], setCircuit);
watch(room, layout, { deep: true });
watch(running, play);
// Standing still, or between frames: what changed shows at once (the tags are in the page by then).
watch([scene, () => props.selected, () => props.hot, () => props.settings.glow, () => props.settings.labels, motion], () => {
  void nextTick(() => {
    if (!frame) render();
  });
});
</script>

<template>
  <div class="circuit">
    <canvas v-show="supported" ref="canvas" aria-hidden="true"></canvas>

    <!-- For the pointer: the tower lists the same projects for the keyboard and VoiceOver. -->
    <div v-if="supported" class="over" aria-hidden="true">
      <div v-for="mover in riders" :key="mover.key" :ref="(el) => setRider(mover.key, el)" class="anchor">
        <button
          v-show="shows(mover.path)"
          type="button"
          tabindex="-1"
          :class="['tag', 'rides', { on: mover.path === selected, claude: mover.kind === 'claude' }]"
          :title="name(mover.path)"
          @click="emit('select', mover.path)"
          @dblclick="emit('open', mover.path)"
        >
          <ClaudeLogo v-if="mover.kind === 'claude'" :size="10" />
          {{ tag(mover.path) }}
          <span v-if="mover.kind === 'server' && byPath.get(mover.path)?.claudeWorking" class="working"></span>
        </button>
      </div>

      <template v-for="spot in spots" :key="spot.key">
        <span v-if="spot.kind !== 'lane'" :class="['light', spot.kind]" :style="{ transform: `translate3d(${spot.x.toFixed(1)}px, ${spot.y.toFixed(1)}px, 0)` }"></span>
        <div v-if="spot.text" class="anchor" :style="{ transform: `translate3d(${spot.tagX.toFixed(1)}px, ${spot.tagY.toFixed(1)}px, 0)` }">
          <button
            type="button"
            tabindex="-1"
            :class="['tag', spot.side, spot.kind, { on: spot.path === selected }]"
            :title="spot.label"
            @click="emit('select', spot.path)"
            @dblclick="emit('open', spot.path)"
          >
            <ClaudeLogo v-if="spot.kind === 'waiting'" :size="10" />
            {{ spot.text }}
          </button>
        </div>
      </template>

      <div v-if="wallName" class="anchor" :style="{ transform: `translate3d(${wallName.x.toFixed(1)}px, ${wallName.y.toFixed(1)}px, 0)` }">
        <span class="wall-name">{{ t("track.wall") }}</span>
      </div>
    </div>

    <p v-if="empty" class="hint" :style="{ left: `${inset.left}px`, right: `${inset.right}px`, top: `${inset.top}px`, bottom: `${inset.bottom}px` }">{{ t("track.empty") }}</p>
    <p v-if="!supported" class="hint" :style="{ left: `${inset.left}px`, right: `${inset.right}px`, top: `${inset.top}px`, bottom: `${inset.bottom}px` }">{{ t("track.unsupported") }}</p>
  </div>
</template>

<style scoped>
.circuit,
.over {
  position: absolute;
  inset: 0;
}

.circuit {
  overflow: hidden;
}

canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

/* Only the tags take the pointer: the canvas and the room between them don't. */
.over {
  pointer-events: none;
}

/* A point of the circuit: what hangs off it is placed from here. */
.anchor {
  position: absolute;
  left: 0;
  top: 0;
  width: 0;
  height: 0;
  will-change: transform;
}

.tag {
  position: absolute;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 20px;
  padding: 0 6px;
  border: 1px solid var(--line-strong);
  border-radius: 6px;
  background: rgba(17, 19, 23, 0.88);
  color: #c7ccd3;
  font-family: var(--font-mono);
  font-size: 11px;
  white-space: nowrap;
  pointer-events: auto;
}

.tag:hover,
.tag.on {
  border-color: #5b626d;
  color: var(--text-strong);
}

/* Above the light, off to its right: the light itself stays in sight. */
.tag.rides,
.tag.beside {
  left: 9px;
  top: -27px;
}

.tag.under {
  left: 0;
  top: 0;
  transform: translateX(-50%);
}

/* The straight: beside the light, on its lane. */
.tag.after {
  left: 12px;
  top: -10px;
}

/* The straight: a project's tag ends where its lane starts. */
.tag.before {
  right: 10px;
  top: -10px;
}

.tag.waiting,
.tag.claude {
  border-color: var(--claude-line);
  color: var(--claude-text);
}

.tag.crashed {
  border-color: #4a2626;
  color: var(--crash-text);
}

.tag.busy {
  color: var(--text-muted);
}

.tag.waiting:hover,
.tag.waiting.on,
.tag.claude:hover,
.tag.claude.on {
  border-color: #6b4a30;
}

.tag.crashed:hover,
.tag.crashed.on {
  border-color: #7a3434;
}

/* Claude at work in a project whose server runs: the status mark's ring, on its tag. */
.working {
  width: 6px;
  height: 6px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 1.5px solid var(--claude);
  animation: track-breathe 1.6s ease-in-out infinite;
}

/* A light standing still, centred on its place. */
.light {
  position: absolute;
  left: -5px;
  top: -5px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
}

.light.waiting {
  background: var(--claude);
  animation: track-ping 1.8s ease-out infinite;
}

.light.crashed {
  background: var(--crash);
  animation: track-alarm 2.2s ease-out infinite;
}

.light.busy {
  background: var(--run);
  animation: track-breathe 1.1s ease-in-out infinite;
}

.light.stopped {
  left: -4px;
  top: -4px;
  width: 8px;
  height: 8px;
  border: 1.5px solid var(--idle-ring);
}

.wall-name {
  position: absolute;
  left: 0;
  top: 0;
  transform: translateX(-50%);
  font-family: var(--font-mono);
  font-size: 10.5px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-faint);
  white-space: nowrap;
}

:lang(zh) .wall-name,
:lang(ja) .wall-name {
  letter-spacing: 0;
}

.hint {
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0;
  padding: 0 24px;
  font-size: 13px;
  color: var(--text-subtle);
  text-align: center;
  pointer-events: none;
}

@keyframes track-ping {
  from {
    box-shadow: 0 0 0 0 rgba(240, 136, 62, 0.6);
  }
  to {
    box-shadow: 0 0 0 13px rgba(240, 136, 62, 0);
  }
}

@keyframes track-alarm {
  from {
    box-shadow: 0 0 0 0 rgba(241, 76, 76, 0.55);
  }
  to {
    box-shadow: 0 0 0 12px rgba(241, 76, 76, 0);
  }
}

@keyframes track-breathe {
  50% {
    opacity: 0.35;
  }
}

@media (prefers-reduced-motion: reduce) {
  .light,
  .working {
    animation: none;
  }
}
</style>
