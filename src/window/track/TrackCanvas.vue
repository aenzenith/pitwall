<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import { SLIDE_MS, startCircuit, type Inset, type Light, type Renderer } from "../../lib/circuit";
import { phase as phaseText } from "../../lib/format";
import { t } from "../../lib/i18n";
import { visible } from "../../lib/store";
import { dash, dashTime, grid, gridPlace, laneOffsets, leaveTime, PIT_LANE, sessionsIn, smooth, type Circuit, type Mover, type Stander, type TrackSettings } from "../../lib/track";
import type { Project, Turn } from "../../lib/types";

/**
 * The circuit and what runs on it: the lights are drawn on a canvas (lib/circuit), their tags and
 * the lights that stand still (Claude waiting at the pit wall, a crashed server) are elements over
 * it, moved each frame to where the circuit puts them. A light drives between the two: Claude's
 * flat out to the wall when Claude starts to wait and out of it when it goes back to work, a
 * server's off the grid at the start line as it starts and into the pit as it stops. For the pointer only: the
 * tower beside it lists the same projects for the keyboard and VoiceOver.
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
const emit = defineEmits<{ select: [path: string]; open: [path: string]; bring: [path: string, session: string] }>();

/** At most this many frames a second: a 120 Hz screen draws every other one. */
const FPS = 60;
/** At the wall, the light that drove there gives way to the one that stands over this long; a light
 * with nothing left to show goes out over `OUT` (seconds). */
const PARK = 0.25;
const OUT = 0.5;
/** A place at the wall nearer than this is reached by going round once more (world units). */
const NEAR = 0.8;
/** A light that had none on the lap comes to the wall from this far off (world units). */
const FROM_AFAR = 6;
/** After a resize, what stands still follows the circuit at once for this long (ms); otherwise it
 * slides to a new place. */
const SETTLE = 250;

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

/** What a light is on the lap: whose, its lane, its place and pace, its trail. */
type Car = Pick<Mover, "path" | "kind" | "warn" | "offset" | "phase" | "speed" | "trail">;

/** A light between the lap and the pit wall, by its mover's key. */
type Run = {
  /** `box`: flat out to its place at the wall; `park`: there, giving way to the light that stands;
   * `leave`: from the wall to its place on the lap; `lap`: on the lap, going out. */
  mode: "box" | "park" | "leave" | "lap";
  /** When the mode began and how long it takes, on the circuit's clock (seconds). */
  since: number;
  takes: number;
  /** Where its head was when the mode began, in laps (whole laps kept). */
  from: number;
  /** Whole laps: on the way in, between its place's `t` and where it stops; on the way out, between
   * its light's laps and its own. */
  turns: number;
  /** Its place at the wall. */
  place: { t: number; offset: number };
  car: Car;
  /** It had no light on the lap: it comes in out of the dark. */
  fresh: boolean;
  /** Going out since then (the circuit's clock): nothing of it is left to show. */
  out: number | null;
};

const runs = new Map<string, Run>();
/** Where each light was last drawn, in laps (whole laps kept), by its mover's key. */
const heads = new Map<string, number>();
/** The lights still on their way to the wall, by their mover's key: the one that stands there waits
 * for it. */
const arriving = reactive(new Set<string>());
/** What stands still slides to a new place; while the window is resized it follows the circuit. */
const settled = ref(true);
/** The straight and a lap are sliding past each other, and what stands on them with them: for the
 * first half what stood on the one left (`left`) goes with it, for the second what stands on the
 * new one comes with it; up to the straight, down from it. */
const sliding = ref<"" | "leave-up" | "leave-down" | "come-up" | "come-down">("");
let slideTimer = 0;
/** A lap is turning into another: what stands at the wall rides with it, the tags that ride the
 * lights wait for theirs to come up. */
const turning = ref(false);
let settling = 0;
let observer: ResizeObserver | null = null;
let motionQuery: MediaQueryList | null = null;

/** On the straight a project's name stands at its lane's start: the lanes begin further in. */
const room = computed<Inset>(() => {
  if (!straight.value) return props.inset;
  const widest = Math.max(0, ...props.projects.map((project) => (props.tags.get(project.path) ?? "").length));
  return { ...props.inset, left: props.inset.left + 28 + widest * 6.8, bottom: props.inset.bottom - 34 };
});

const lanes = computed(() => (straight.value ? laneOffsets(props.projects.length) : []));
/** The sessions that wait on you: each has a place at the wall. */
const waiting = computed(() => props.projects.reduce((count, project) => count + sessionsIn(project, "waiting").length, 0));

/** A waiting light's tag says what Claude waits with while the wall has the room. */
const tagWidth = (long: boolean): number => {
  const letters = props.settings.labels === "name" ? Math.max(3, ...props.projects.filter((p) => sessionsIn(p, "waiting").length).map((p) => p.name.length)) : 3;
  return 34 + letters * 6.8 + (long ? 96 : 0);
};
const long = computed(() => {
  void laidOut.value;
  const width = (canvas.value?.clientWidth ?? 0) - room.value.left - room.value.right;
  const each = tagWidth(true) + 12;
  // A wall along a straight (the street circuit's): everyone's place stays on it.
  const along = props.circuit.wall.room * (renderer?.scale() || 100);
  return !straight.value && waiting.value > 0 && waiting.value * each <= width * 0.9 && (along === 0 || (waiting.value - 1) * each <= along);
});
/** How far apart the wall's places are, in world units. */
const gap = computed(() => {
  void laidOut.value;
  return (tagWidth(long.value) + 12) / (renderer?.scale() || 100);
});
/** Half the pit wall: long enough for everyone waiting. */
const wall = computed(() => Math.min(props.circuit.line.length * 0.2, Math.max(1.3, (waiting.value * gap.value) / 2 + 0.3)));

/** How far apart the grid's places are, in world units: a starting server's tag (its name alone)
 * fits between two. */
const slot = computed(() => {
  void laidOut.value;
  const starting = props.projects.filter((project) => project.status === "busy" && project.phase === "starting…");
  const letters = props.settings.labels === "name" ? Math.max(3, ...starting.map((project) => project.name.length)) : 3;
  return (22 + letters * 6.8) / (renderer?.scale() || 100);
});

const scene = computed(() => grid(props.projects, props.circuit, gap.value, slot.value));

/** The key of the light that drives to where this one stands: its session's for one that waits, the
 * server's for one that starts or stops. */
const driverOf = (stander: { path: string; kind: string; session: string | null }): string => `${stander.path}#${stander.kind === "waiting" ? `claude:${stander.session}` : "server"}`;

/** Every light that drives, by its mover's key: on the lap (a server running, Claude at work) or
 * standing at the wall (a server starting or stopping, Claude waiting). */
const drivers = computed(() => {
  const lapping = new Map<string, Mover>();
  const standing = new Map<string, Stander>();
  for (const mover of scene.value.movers) lapping.set(mover.key, mover);
  for (const stander of scene.value.standers) if (stander.kind === "waiting" || stander.kind === "busy") standing.set(driverOf(stander), stander);
  return { lapping, standing };
});

const byPath = computed(() => new Map(props.projects.map((project) => [project.path, project])));
const tag = (path: string): string => props.tags.get(path) ?? "";
const name = (path: string): string => byPath.value.get(path)?.name ?? "";
/** How many of a project's sessions are at work. Its tag on the lap says so, whether it rides with
 * the server or with Claude: Claude's colour and logo, and with more than one, how many
 * (`PIT (2)`). A server alone keeps the plain tag. */
const atWork = (path: string): number => (byPath.value.get(path)?.claudeSessions ?? []).filter((session) => session.phase === "working").length;

const TURN = { finished: "claude.finished", asking: "claude.asking", permission: "claude.permission" } as const;
const turnText = (turn: Turn | null): string => (turn ? t(TURN[turn.kind]) : "");

/** A tag shows when its project is selected or pointed at, and otherwise as the settings say. */
function shows(path: string): boolean {
  return props.settings.labels !== "hover" || path === props.selected || path === props.hot;
}

/** The tags that rode the circuit left, while it slides away. */
const leftRiders = shallowRef<Mover[] | null>(null);
/** The riders as last listed: what `leftRiders` takes when the circuit changes. */
let rode: Mover[] = [];

const riders = computed(() => {
  if (leftRiders.value) return leftRiders.value;
  rode = scene.value.movers.filter((mover) => mover.tagged);
  return rode;
});

type Spot = { key: string; id: string; path: string; kind: Stander["kind"] | "lane"; session: string | null; x: number; y: number; tagX: number; tagY: number; text: string; label: string; side: "under" | "beside" | "after" | "before" };

/** What stands still, where the circuit puts it: placed when the layout or the projects change, not
 * each frame. */
/** What stood on the circuit left, while it slides away. */
const left = shallowRef<Spot[] | null>(null);
/** The spots as last placed: what `left` takes when the circuit changes. */
let placed: Spot[] = [];

const spots = computed<Spot[]>(() => {
  void laidOut.value;
  const drawn = renderer;
  if (!drawn || !supported.value) return [];
  const { half, wall: at } = props.circuit;
  const list: Spot[] = [];

  for (const stander of scene.value.standers) {
    const [x, y] = drawn.project(stander.t, stander.offset);
    const project = byPath.value.get(stander.path);
    // What waits at the wall rides with it from one lap to the next; anything else that stands goes
    // with its circuit and comes with the new one.
    const base = { key: stander.key, id: stander.kind === "waiting" ? stander.key : `${props.circuit.id}:${stander.key}`, path: stander.path, kind: stander.kind, session: stander.session, x, y };
    if (stander.kind === "waiting") {
      const what = turnText(stander.turn);
      // Under the pit wall, centred on its place.
      const [, wallY] = straight.value ? [x, y] : drawn.project(stander.t, at.side * half * 2.3);
      list.push({ ...base, tagX: x, tagY: wallY + 10, text: long.value ? `${tag(stander.path)} · ${what}` : straight.value ? "" : tag(stander.path), label: `${name(stander.path)}: ${what}`, side: "under" });
    } else if (stander.kind === "crashed") {
      list.push({ ...base, tagX: x, tagY: y, text: straight.value ? t("status.crashed") : `${tag(stander.path)} · ${t("status.crashed")}`, label: `${name(stander.path)}: ${t("status.crashed")}`, side: straight.value ? "after" : "beside" });
    } else if (stander.kind === "busy") {
      const doing = project ? phaseText(project) : "";
      // On the grid a tag is the name alone: the places are close, and the light says the rest.
      const text = straight.value ? doing : project?.phase === "starting…" ? tag(stander.path) : `${tag(stander.path)} · ${doing}`;
      list.push({ ...base, tagX: x, tagY: y, text, label: `${name(stander.path)}: ${doing}`, side: straight.value ? "after" : "beside" });
    } else {
      list.push({ ...base, tagX: x, tagY: y, text: "", label: name(stander.path), side: "beside" });
    }
  }

  // The straight: every project's tag before its lane's start.
  if (straight.value) {
    props.projects.forEach((project, index) => {
      const [x, y] = drawn.project(0, lanes.value[index]);
      list.push({ key: `${project.path}#lane`, id: `${props.circuit.id}:${project.path}#lane`, path: project.path, kind: "lane", session: null, x, y, tagX: x, tagY: y, text: tag(project.path), label: project.name, side: "before" });
    });
  }
  placed = list;
  return list;
});

const standing = computed(() => (left.value ?? spots.value).filter((spot) => spot.kind !== "lane"));

/** A double click: what Claude waits with is brought up where that session runs; anything else goes
 * to its project. */
function open(spot: Spot): void {
  if (spot.kind === "waiting" && spot.session) emit("bring", spot.path, spot.session);
  else emit("open", spot.path);
}
const named = computed(() => (left.value ?? spots.value).filter((spot) => spot.text));

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

function colour(car: Car): readonly [number, number, number] {
  if (car.kind === "claude") return ORANGE;
  return car.warn ? AMBER : GREEN;
}

/** A place as a light drives to it (0–1 of the line). An open line: its lane's start is reached
 * through the far end; Claude's places are at that end, and back from it. */
function reach(t: number): number {
  return props.circuit.line.closed ? t : t || 1;
}

/** Where a light on its way in stops, in laps. */
function stopOf(run: Run): number {
  return run.turns + reach(run.place.t);
}

/** Where a light with no place to set off from does: a server from the grid, in its lane behind the
 * start line; Claude from the pit exit, just past the wall. On the straight, the lane's start. */
function setOff(mover: Mover): Run["place"] {
  const { line, wall, half } = props.circuit;
  if (mover.kind === "server" || !line.closed) return gridPlace(props.circuit, mover.offset);
  return { t: wall.t + 1.5 / line.length, offset: wall.side * half * 1.6 };
}

/** Sets a light off for its place at the wall, from `from` (laps): flat out, round the lap if the
 * place is behind it. With no light on the lap (`from` null) it comes in from afar. */
function box(key: string, stander: Stander, car: Car, from: number | null): void {
  const { line } = props.circuit;
  const place = reach(stander.t);
  let start: number;
  let turns: number;
  if (from === null) {
    start = place - Math.min(line.closed ? 0.45 : 0.9, FROM_AFAR / line.length);
    turns = 0;
  } else if (line.closed) {
    start = from;
    turns = Math.ceil(start - place);
    if ((turns + place - start) * line.length < NEAR) turns++;
  } else {
    start = from;
    turns = Math.floor(start);
    // Already past its place (one back from the lane's end): out of the far end, and in again.
    if (turns + place < start) turns++;
  }
  runs.set(key, { mode: "box", since: clock, takes: dashTime((turns + place - start) * line.length), from: start, turns, place: { t: stander.t, offset: stander.offset }, car, fresh: from === null, out: null });
  arriving.add(key);
}

/** Sets a light off from its place at the wall (`start`, in laps) for its place on the lap. */
function leave(key: string, mover: Mover, place: Run["place"], start: number): void {
  const { line } = props.circuit;
  const laps = mover.phase + (clock * mover.speed) / line.length;
  const turns = Math.ceil(start - laps);
  runs.set(key, { mode: "leave", since: clock, takes: leaveTime((laps + turns - start) * line.length), from: start, turns, place, car: mover, fresh: false, out: null });
  arriving.delete(key);
}

/**
 * One light for this frame: on its lap, or between the lap and the wall. Null once it has gone.
 * `mover`: its light on the lap, while its server runs or Claude works.
 */
function driven(key: string, mover: Mover | null, length: number): Light | null {
  const { line } = props.circuit;
  const run = runs.get(key);
  const car = mover ?? run?.car;
  if (!car) return null;
  const lit = car.path === props.selected || car.path === props.hot;
  const laps = car.phase + (clock * car.speed) / line.length;
  const whole = (car.trail / line.length) * length;

  let head = laps;
  let trail = whole;
  let gain = 1;
  let offset = car.offset;
  let turn: Light["turn"];
  let ends = 1;
  let tail = 1;
  /** An open line runs 0–1: the laps to take off the head. */
  let lap = Math.floor(laps);

  if (run?.mode === "box") {
    const stander = drivers.value.standing.get(key);
    if (stander) run.place = { t: stander.t, offset: stander.offset };
    const to = stopOf(run);
    const u = (clock - run.since) / run.takes;
    if (u >= 1) {
      // There. Back on the lap meanwhile (Claude at work again, the server restarted): straight
      // out again. Otherwise the light that stands takes over (or nothing does, and it goes out).
      const back = drivers.value.lapping.get(key);
      if (back) {
        leave(key, back, run.place, to);
        return driven(key, back, length);
      }
      run.mode = "park";
      run.since = clock;
      run.takes = PARK;
      arriving.delete(key);
      return driven(key, mover, length);
    }
    head = run.from + (to - run.from) * dash(u);
    // Its trail stretches with its speed, and is gone as it stands.
    trail = whole * (1 + 0.35 * Math.sin(Math.PI * u)) * (1 - smooth(0.6, 1, u));
    if (run.fresh) trail = Math.min(trail, head - run.from);
    gain = run.fresh ? smooth(0, 0.4, u) : 1;
    if (line.closed) turn = { from: Math.max(to - PIT_LANE / line.length, run.from), to, offset: run.place.offset };
    // The straight: Claude's place is at its lane's far end, and it comes to it in full light; a
    // server's is the lane's start, and it goes out into the far end as any light does.
    if (run.place.t > 0) ends = 1 - smooth(0, 0.3, u);
    lap = run.turns;
  } else if (run?.mode === "park") {
    const u = (clock - run.since) / run.takes;
    if (u >= 1) {
      runs.delete(key);
      return null;
    }
    head = stopOf(run);
    trail = 0;
    gain = 1 - u;
    offset = run.place.offset;
    if (run.place.t > 0) ends = 0;
    lap = run.turns;
  } else if (run?.mode === "leave") {
    const u = (clock - run.since) / run.takes;
    head = run.from + (laps + run.turns - run.from) * (u >= 1 ? 1 : smooth(0, 1, u));
    // Its trail comes out of the wall behind it.
    trail = Math.min(whole, head - run.from);
    gain = 1;
    offset = run.place.offset;
    if (line.closed) turn = { from: run.from, to: run.from + PIT_LANE / line.length, offset: car.offset };
    lap = Math.floor(head);
    // Level with its place on the lap, its trail back on the road: a light like any other again.
    if (u >= 1 && run.out === null && head - trail > run.from + PIT_LANE / line.length) runs.delete(key);
  }
  if (run && run.mode !== "lap") tail = Math.min(1, (trail * line.length) / 0.5);

  if (run && run.out !== null) {
    const left = 1 - (clock - run.out) / OUT;
    if (left <= 0) {
      runs.delete(key);
      return null;
    }
    gain *= left;
  }

  heads.set(key, head);
  return { head: line.closed ? head : head - lap, trail, gain: (lit ? 1.3 : 1) * gain, offset, colour: colour(car), turn, ends, tail };
}

/** Where a light's head is across the road: on its way through a change of lane, part of the way. */
function across(light: Light): number {
  return light.turn ? light.offset + (light.turn.offset - light.offset) * smooth(light.turn.from, light.turn.to, light.head) : light.offset;
}

/** One frame: the lights, then the tags that ride with them. */
function render(): void {
  const drawn = renderer;
  if (!drawn) return;
  const calm = motion.value === "calm";
  const still = motion.value === "still";
  const length = still ? 0.3 : calm ? 0.6 : 1;
  const movers = scene.value.movers;

  const lights: Light[] = [];
  /** Each mover's light, for the tag that rides with it. */
  const ridden = new Map<string, Light>();
  // A light on the lap is whole from the first frame: one that comes to it sets off from the grid or
  // the pit (a run), and the page opens on the lights as they are.
  for (const mover of movers) {
    const light = driven(mover.key, mover, length);
    if (!light) continue;
    lights.push(light);
    ridden.set(mover.key, light);
  }
  // The lights with no place on the lap any more: on their way to the wall, or going out.
  for (const key of [...runs.keys()]) {
    if (drivers.value.lapping.has(key)) continue;
    const light = driven(key, null, length);
    if (light) lights.push(light);
  }
  drawn.draw(clock, lights, props.settings.glow);
  if (turning.value) follow();

  // The tags of the circuit left stay where they were: they slide away with it.
  for (const [key, el] of leftRiders.value ? [] : riderEls) {
    const light = ridden.get(key);
    if (!light) continue;
    const [x, y] = drawn.project(light.head, across(light), true);
    el.style.transform = `translate3d(${x.toFixed(1)}px, ${y.toFixed(1)}px, 0)`;
  }

  if (heads.size > drivers.value.lapping.size + runs.size) {
    for (const key of heads.keys()) if (!drivers.value.lapping.has(key) && !runs.has(key)) heads.delete(key);
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
  // Drawn at once: no frame shows an empty circuit, or a tag before it has its place.
  render();
  if (!running.value) return;
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
  // Drawn before, and someone watching: the circuit turns into the new one.
  const animate = marked !== null && running.value;
  const other = marked !== null && marked.circuit !== props.circuit;
  marked = { circuit: props.circuit, lanes: lanes.value.length, wall: wall.value };
  // What stands on the circuit left and what rides it, as last placed (nothing has asked for the
  // new places yet).
  const stood = placed;
  const ridden = rode;
  renderer.setCircuit(props.circuit, lanes.value, wall.value, animate);
  if (other) slide(stood, ridden);
  follow();
}

/** The straight and a lap sliding past each other: what stood on the one left leaves with it, then
 * what stands on the new one comes with it. */
function slide(stood: Spot[], ridden: Mover[]): void {
  window.clearTimeout(slideTimer);
  left.value = null;
  leftRiders.value = null;
  sliding.value = "";
  if (renderer?.moving() !== "slide") return;
  const up = props.circuit.id === "straight";
  left.value = stood;
  leftRiders.value = ridden;
  sliding.value = up ? "leave-up" : "leave-down";
  slideTimer = window.setTimeout(() => {
    unsettle();
    left.value = null;
    leftRiders.value = null;
    sliding.value = up ? "come-up" : "come-down";
    slideTimer = window.setTimeout(() => (sliding.value = ""), SLIDE_MS / 2);
  }, SLIDE_MS / 2);
}

/** While a lap turns into another, what stands still is placed again each frame. */
function follow(): void {
  const moving = renderer?.moving() ?? null;
  const turned = turning.value && moving !== "morph";
  turning.value = moving === "morph";
  // Each frame of the way, and once more where it ends.
  if (moving === "morph") unsettle();
  if (moving === "morph" || turned) laidOut.value++;
}

/** Fits the circuit into its room. Another circuit is taken first: the one it turns from keeps the
 * place it had. The wall is as long as the tags of whoever waits need, and the scale decides that:
 * its marks follow. */
function layout(): void {
  if (!renderer) return;
  mark();
  renderer.layout(room.value);
  laidOut.value++;
  mark();
}

function setCircuit(): void {
  mark();
  layout();
}

/** The circuit moved under what stands still (a resize, another circuit): it follows at once. */
function unsettle(): void {
  settled.value = false;
  window.clearTimeout(settling);
  settling = window.setTimeout(() => (settled.value = true), SETTLE);
}

/** Nothing is on its way any more: every light is where it belongs. */
function arrive(): void {
  runs.clear();
  heads.clear();
  arriving.clear();
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
  arrive();
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
      unsettle();
      layout();
      if (!frame) render();
    });
    observer.observe(canvas.value);
  }
});

onBeforeUnmount(() => {
  stop();
  window.clearTimeout(settling);
  window.clearTimeout(slideTimer);
  observer?.disconnect();
  motionQuery?.removeEventListener("change", onMotion);
  canvas.value?.removeEventListener("webglcontextlost", onLost);
  canvas.value?.removeEventListener("webglcontextrestored", start);
  renderer?.stop();
  renderer = null;
});

// The circuit's own marks: another circuit, another number of lanes, a longer wall.
watch([() => props.circuit, lanes, wall], ([next], [before]) => {
  if (next !== before) {
    unsettle();
    arrive();
  }
  setCircuit();
});
watch(
  room,
  () => {
    unsettle();
    layout();
  },
  { deep: true },
);
watch(running, play);
watch(motion, (now) => {
  if (now === "still") arrive();
});

/** The circuit the lights were last followed on. */
let followed = props.circuit;

// A light takes its place at the wall (Claude starts to wait, a server stops): it makes for it. It
// takes its place on the lap (Claude at work, a server up): it leaves the wall for it, from the pit
// exit if it had no place there. Neither any more: it goes out. Nobody watching, or nothing
// moving: everything is where it belongs at once.
watch(drivers, (now, before) => {
  if (!running.value || !renderer || followed !== props.circuit) {
    followed = props.circuit;
    arrive();
    return;
  }
  const state = (of: typeof now, key: string): "standing" | "lapping" | null => (of.standing.has(key) ? "standing" : of.lapping.has(key) ? "lapping" : null);
  for (const key of new Set([...before.lapping.keys(), ...before.standing.keys(), ...now.lapping.keys(), ...now.standing.keys()])) {
    const was = state(before, key);
    const is = state(now, key);
    if (was === is) continue;
    const run = runs.get(key);

    if (is === "standing") {
      const stander = now.standing.get(key)!;
      if (run?.mode === "box") {
        // Already on its way there.
        run.out = null;
        arriving.add(key);
      } else if (run?.mode === "park") {
        runs.delete(key);
      } else {
        const car = before.lapping.get(key) ?? run?.car;
        if (car) box(key, stander, car, heads.get(key) ?? car.phase + (clock * car.speed) / props.circuit.line.length);
        // Claude with no light on the lap comes in from afar; a server about to start has none
        // yet: its standing light simply shows.
        else if (stander.kind === "waiting") box(key, stander, { path: stander.path, kind: "claude", warn: false, offset: stander.lane, trail: stander.trail, phase: 0, speed: 0 }, null);
      }
    } else if (is === "lapping") {
      const mover = now.lapping.get(key)!;
      arriving.delete(key);
      if (!run) {
        const stood = before.standing.get(key);
        const place = stood ? { t: stood.t, offset: stood.offset } : setOff(mover);
        leave(key, mover, place, place.t);
      } else if (run.mode === "park") {
        leave(key, mover, run.place, stopOf(run));
      } else if (run.mode === "lap") {
        // It was going out on the lap: it stays.
        runs.delete(key);
      } else {
        // On its way in, it goes on to its place and leaves from there; on its way out, it goes on.
        run.out = null;
        run.car = mover;
      }
    } else {
      arriving.delete(key);
      const car = before.lapping.get(key);
      // On its way in, it goes out once it is there.
      if (run) run.out ??= run.mode === "box" ? null : clock;
      else if (car) runs.set(key, { mode: "lap", since: clock, takes: OUT, from: 0, turns: 0, place: { t: 0, offset: 0 }, car, fresh: false, out: clock });
    }
  }
});

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
    <div v-if="supported" :class="['over', sliding, { settled, turning }]" aria-hidden="true">
      <div v-for="mover in riders" :key="mover.key" :ref="(el) => setRider(mover.key, el)" class="anchor">
        <button
          v-show="shows(mover.path)"
          type="button"
          tabindex="-1"
          :class="['tag', 'rides', { on: mover.path === selected, claude: mover.kind === 'claude' || atWork(mover.path) > 0 }]"
          :title="name(mover.path)"
          @click="emit('select', mover.path)"
          @dblclick="emit('open', mover.path)"
        >
          <ClaudeLogo v-if="mover.kind === 'claude' || atWork(mover.path) > 0" :size="10" />
          {{ tag(mover.path) }}<template v-if="atWork(mover.path) > 1"> ({{ atWork(mover.path) }})</template>
        </button>
      </div>

      <!-- What stands still comes and goes softly. A light at the wall shows once the one that
           drives has reached its place. Claude's tag is there from the start (who waits, and with
           what); a stopping server's comes with its light: a stop is over before it would be read. -->
      <Transition v-for="spot in standing" :key="spot.id" name="stand">
        <span
          :class="['light', 'spot', spot.kind, { away: arriving.has(driverOf(spot)) }]"
          :style="{ transform: `translate3d(${spot.x.toFixed(1)}px, ${spot.y.toFixed(1)}px, 0)` }"
          :title="spot.kind === 'waiting' ? spot.label : undefined"
          @click="spot.kind === 'waiting' && emit('select', spot.path)"
          @dblclick="spot.kind === 'waiting' && open(spot)"
        ></span>
      </Transition>
      <Transition v-for="spot in named" :key="spot.id" name="stand">
        <div :class="['anchor', 'spot', { away: spot.kind === 'busy' && arriving.has(driverOf(spot)) }]" :style="{ transform: `translate3d(${spot.tagX.toFixed(1)}px, ${spot.tagY.toFixed(1)}px, 0)` }">
          <button
            type="button"
            tabindex="-1"
            :class="['tag', spot.side, spot.kind, { on: spot.path === selected }]"
            :title="spot.label"
            @click="emit('select', spot.path)"
            @dblclick="open(spot)"
          >
            <ClaudeLogo v-if="spot.kind === 'waiting'" :size="10" />
            {{ spot.text }}
          </button>
        </div>
      </Transition>

      <div v-if="wallName" class="anchor spot" :style="{ transform: `translate3d(${wallName.x.toFixed(1)}px, ${wallName.y.toFixed(1)}px, 0)` }">
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

/* The straight and a lap slide past each other (lib/circuit: SLIDE, SLIDE_MS), and what stands on
   them goes and comes with them: half the way and half the time each. */
.over.leave-up {
  animation: over-leave-up 0.4s ease-in forwards;
}

.over.leave-down {
  animation: over-leave-down 0.4s ease-in forwards;
}

.over.come-up {
  animation: over-come-up 0.4s ease-out;
}

.over.come-down {
  animation: over-come-down 0.4s ease-out;
}

@keyframes over-leave-up {
  to {
    opacity: 0;
    transform: translateY(-15%);
  }
}

@keyframes over-leave-down {
  to {
    opacity: 0;
    transform: translateY(15%);
  }
}

@keyframes over-come-up {
  from {
    opacity: 0;
    transform: translateY(15%);
  }
}

@keyframes over-come-down {
  from {
    opacity: 0;
    transform: translateY(-15%);
  }
}

/* A lap turning into another: the tags that ride come back with their lights. */
.tag.rides {
  transition: opacity 0.3s;
}

.over.turning .tag.rides {
  opacity: 0;
  transition: none;
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

/* A light standing still, centred on its place. */
.light {
  position: absolute;
  left: -5px;
  top: -5px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  transition: opacity 0.2s;
}

/* The light that drives is still on its way to this place (over the kinds' own animations). */
.over .light.away {
  opacity: 0;
  animation: none;
}

.over .anchor.away {
  opacity: 0;
}

.anchor.away .tag {
  pointer-events: none;
}

/* What stands still slides to a new place (the wall makes room for one more); while the window is
   resized it follows the circuit. */
.over.settled .spot {
  transition:
    opacity 0.2s,
    transform 0.35s cubic-bezier(0.2, 0.8, 0.2, 1);
}

.stand-enter-from,
.stand-leave-to {
  opacity: 0;
}

.over .spot.stand-enter-active,
.over .spot.stand-leave-active {
  transition: opacity 0.2s;
}

/* Claude's waiting light takes the pointer, as its tag does (the straight shows no tag for it). */
.light.waiting {
  background: var(--claude);
  animation: track-ping 1.8s ease-out infinite;
  pointer-events: auto;
  cursor: pointer;
}

.light.waiting::after {
  content: "";
  position: absolute;
  inset: -7px;
  border-radius: 50%;
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
  .light {
    animation: none;
  }

  .over .spot,
  .over .spot.stand-enter-active,
  .over .spot.stand-leave-active {
    transition: none;
  }
}
</style>
