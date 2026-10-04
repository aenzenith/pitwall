// The Track page: its circuits, and what the projects put on them. A running dev server is a green
// light lapping the circuit, each Claude session at work an orange one; a session waiting on you has
// driven to the pit wall and stands there, a crashed server stands where it stopped. A stopped
// project is off the circuit: in the tower only (on the straight, at its lane's start).

import type { Project, Turn } from "./types";

export type CircuitId = "night" | "oval" | "eight" | "street" | "straight";

/** The picker's order, the default first. `straight` gives every project a lane of its own. */
export const CIRCUITS: CircuitId[] = ["street", "night", "oval", "eight", "straight"];

/** The page's own settings, as the core keeps them (`settings.track`). */
export type TrackSettings = {
  /** A circuit, or `shuffle`: another one each time the page opens. */
  circuit: CircuitId | "shuffle";
  /** What a car carries: its project's three letters, its name, or nothing until pointed at. */
  labels: "code" | "name" | "hover";
  /** `calm`: half speed, short trails; `still`: nothing moves. */
  motion: "full" | "calm" | "still";
  /** The lights' soft glow. */
  glow: boolean;
};

export const TRACK_DEFAULTS: TrackSettings = { circuit: "street", labels: "code", motion: "full", glow: true };

/** What the settings file holds, read leniently: an unknown value falls back to the default. */
export function trackSettings(stored: Partial<Record<keyof TrackSettings, unknown>> | null | undefined): TrackSettings {
  const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T => (allowed.includes(value as T) ? (value as T) : fallback);
  return {
    circuit: pick(stored?.circuit, [...CIRCUITS, "shuffle"] as const, TRACK_DEFAULTS.circuit),
    labels: pick(stored?.labels, ["code", "name", "hover"] as const, TRACK_DEFAULTS.labels),
    motion: pick(stored?.motion, ["full", "calm", "still"] as const, TRACK_DEFAULTS.motion),
    glow: typeof stored?.glow === "boolean" ? stored.glow : TRACK_DEFAULTS.glow,
  };
}

/* ---------- circuits ---------- */

/** How many points a circuit's centre line is kept as (the renderer hands them to the GPU). */
export const LINE_SAMPLES = 256;

/**
 * A circuit's centre line: `LINE_SAMPLES` points evenly spaced along it, each with the unit normal
 * to its left. A closed one meets its own start; an open one (the straight) runs from its first
 * point to its last.
 */
export type Line = { closed: boolean; length: number; x: Float32Array; z: Float32Array; nx: Float32Array; nz: Float32Array };

export type Circuit = {
  id: CircuitId;
  line: Line;
  /** Half the road's width; 0 on the straight, whose lanes are the projects'. */
  half: number;
  /** Where the lap comes nearest the viewer: the start line, the pit wall outside it (`side`: which
   * way is outside; `room`: how long the straight it lies along is, in world units, 0 on a bend).
   * On the straight the wall is the far end. */
  wall: { t: number; side: number; room: number };
  /** The camera looks at the circuit from in front and above (world units). */
  camera: { height: number; distance: number; target: number };
};

type Point = [number, number];

const TAU = Math.PI * 2;

/** The website's lap (pitwall-website: resources/js/lib/circuit.ts): small harmonics on an ellipse. */
function night(t: number): Point {
  const th = TAU * t;
  return [
    (Math.cos(th) + 0.28 * Math.cos(2 * th + 0.8) + 0.05 * Math.cos(3 * th + 1.25) - 0.193) * 2.75,
    (0.65 * Math.sin(th) + 0.2 * Math.sin(2 * th + 1.37) - 0.05 * Math.sin(3 * th) + 0.192) * 2.75,
  ];
}

function oval(t: number): Point {
  const th = TAU * t;
  const power = 2 / 3.4;
  const c = Math.cos(th);
  const s = Math.sin(th);
  return [3.5 * Math.sign(c) * Math.abs(c) ** power, 1.85 * Math.sign(s) * Math.abs(s) ** power];
}

function eight(t: number): Point {
  const th = TAU * t;
  return [3.5 * Math.cos(th), 1.9 * Math.sin(2 * th)];
}

/** A street circuit: straight blocks, tight corners, one chicane. */
function street(): Point[] {
  const corners: Point[] = [
    [-3.5, -1.8],
    [0.9, -1.8],
    [1.6, -0.7],
    [3.5, -0.7],
    [3.5, 1.8],
    [0.5, 1.8],
    [-0.1, 0.6],
    [-1.7, 0.6],
    [-2.3, 1.8],
    [-3.5, 1.8],
  ];
  const radius = 0.5;
  const points: Point[] = [];
  corners.forEach((corner, i) => {
    const before = corners[(i - 1 + corners.length) % corners.length];
    const after = corners[(i + 1) % corners.length];
    const toBefore = Math.hypot(before[0] - corner[0], before[1] - corner[1]);
    const toAfter = Math.hypot(after[0] - corner[0], after[1] - corner[1]);
    const cutBefore = Math.min(radius, toBefore / 2) / toBefore;
    const cutAfter = Math.min(radius, toAfter / 2) / toAfter;
    const from: Point = [corner[0] + (before[0] - corner[0]) * cutBefore, corner[1] + (before[1] - corner[1]) * cutBefore];
    const to: Point = [corner[0] + (after[0] - corner[0]) * cutAfter, corner[1] + (after[1] - corner[1]) * cutAfter];
    for (let k = 0; k <= 12; k++) {
      const u = k / 12;
      points.push([(1 - u) ** 2 * from[0] + 2 * u * (1 - u) * corner[0] + u * u * to[0], (1 - u) ** 2 * from[1] + 2 * u * (1 - u) * corner[1] + u * u * to[1]]);
    }
  });
  return points;
}

/** A closed polyline as a `Line`: resampled evenly along its length. */
function closedLine(points: Point[]): Line {
  const count = points.length;
  const along = [0];
  for (let i = 0; i < count; i++) {
    const a = points[i];
    const b = points[(i + 1) % count];
    along.push(along[i] + Math.hypot(b[0] - a[0], b[1] - a[1]));
  }
  const length = along[count];
  const line = emptyLine(true, length);
  let j = 0;
  for (let k = 0; k < LINE_SAMPLES; k++) {
    const at = (length * k) / LINE_SAMPLES;
    while (along[j + 1] < at) j++;
    const f = (at - along[j]) / (along[j + 1] - along[j] || 1);
    const a = points[j];
    const b = points[(j + 1) % count];
    line.x[k] = a[0] + (b[0] - a[0]) * f;
    line.z[k] = a[1] + (b[1] - a[1]) * f;
  }
  for (let k = 0; k < LINE_SAMPLES; k++) {
    const before = (k - 1 + LINE_SAMPLES) % LINE_SAMPLES;
    const after = (k + 1) % LINE_SAMPLES;
    const tx = line.x[after] - line.x[before];
    const tz = line.z[after] - line.z[before];
    const l = Math.hypot(tx, tz) || 1;
    line.nx[k] = -tz / l;
    line.nz[k] = tx / l;
  }
  return line;
}

function emptyLine(closed: boolean, length: number): Line {
  return { closed, length, x: new Float32Array(LINE_SAMPLES), z: new Float32Array(LINE_SAMPLES), nx: new Float32Array(LINE_SAMPLES), nz: new Float32Array(LINE_SAMPLES) };
}

/** The straight: left to right in front of the viewer, its lanes one behind the other. */
function straightLine(): Line {
  const from = -3.6;
  const to = 3.6;
  const line = emptyLine(false, to - from);
  for (let k = 0; k < LINE_SAMPLES; k++) {
    line.x[k] = from + ((to - from) * k) / (LINE_SAMPLES - 1);
    line.nz[k] = 1;
  }
  return line;
}

const sampled = (shape: (t: number) => Point): Point[] => Array.from({ length: 1440 }, (_, i) => shape(i / 1440));

const HALF = 0.17;
const LAP_CAMERA = { height: 8.2, distance: 4.6, target: 0.5 };

function lap(id: CircuitId, points: Point[]): Circuit {
  const line = closedLine(points);
  let nearest = 0;
  for (let k = 1; k < LINE_SAMPLES; k++) if (line.z[k] > line.z[nearest]) nearest = k;
  // Nearest along a straight (the street circuit's): the middle of it, so the wall lies along the
  // straight and not round the corner it starts at.
  const level = (k: number): boolean => line.z[((k % LINE_SAMPLES) + LINE_SAMPLES) % LINE_SAMPLES] > line.z[nearest] - 0.001;
  let first = nearest;
  let last = nearest;
  while (last - first < LINE_SAMPLES && level(first - 1)) first--;
  while (last - first < LINE_SAMPLES && level(last + 1)) last++;
  nearest = (((first + last) >> 1) + LINE_SAMPLES) % LINE_SAMPLES;
  const room = ((last - first) / LINE_SAMPLES) * line.length;
  return { id, line, half: HALF, wall: { t: nearest / LINE_SAMPLES, side: line.nz[nearest] >= 0 ? 1 : -1, room: room >= 1 ? room : 0 }, camera: LAP_CAMERA };
}

const built = new Map<CircuitId, Circuit>();

export function circuit(id: CircuitId): Circuit {
  let made = built.get(id);
  if (!made) {
    made =
      id === "straight"
        ? { id, line: straightLine(), half: 0, wall: { t: 1, side: 1, room: 0 }, camera: { height: 9.5, distance: 0.2, target: 0 } }
        : lap(id, id === "night" ? sampled(night) : id === "oval" ? sampled(oval) : id === "eight" ? sampled(eight) : street());
    built.set(id, made);
  }
  return made;
}

/** The point `t` (0–1) along the line, `offset` world units to its left. */
export function onLine(line: Line, t: number, offset: number): Point {
  const last = LINE_SAMPLES - 1;
  const f = line.closed ? (t - Math.floor(t)) * LINE_SAMPLES : Math.min(1, Math.max(0, t)) * last;
  const i = Math.min(last, Math.floor(f));
  const j = line.closed ? (i + 1) % LINE_SAMPLES : Math.min(last, i + 1);
  const u = f - i;
  const nx = line.nx[i] + (line.nx[j] - line.nx[i]) * u;
  const nz = line.nz[i] + (line.nz[j] - line.nz[i]) * u;
  const l = Math.hypot(nx, nz) || 1;
  return [line.x[i] + (line.x[j] - line.x[i]) * u + (nx / l) * offset, line.z[i] + (line.z[j] - line.z[i]) * u + (nz / l) * offset];
}

/** The first place on the grid: where a server that was never seen starting sets off from. */
export function gridPlace(on: Circuit, offset: number): { t: number; offset: number } {
  return on.id === "straight" ? { t: 0, offset } : { t: on.wall.t - GRID / on.line.length, offset };
}

/** The straight's lanes: each project's distance from the centre line, the first at the back. */
export function laneOffsets(count: number): number[] {
  const gap = Math.min(0.6, 4.6 / Math.max(1, count - 1));
  return Array.from({ length: count }, (_, i) => (i - (count - 1) / 2) * gap);
}

/* ---------- what runs ---------- */

/** A project's Claude sessions in one phase: at work, or waiting on you. Each has a light of its own. */
export function sessionsIn(project: Project, phase: "working" | "waiting"): Project["claudeSessions"] {
  return project.claudeSessions.filter((session) => session.phase === phase);
}

/** A light on the move: a dev server (green; amber with an issue) or a Claude session at work
 * (orange). */
export type Mover = {
  key: string;
  path: string;
  kind: "server" | "claude";
  /** A running server with an issue. */
  warn: boolean;
  /** World units to the left of the centre line. */
  offset: number;
  /** Where it is at time 0, in laps. */
  phase: number;
  /** World units a second, at full speed. */
  speed: number;
  /** How much of the line its trail covers, in world units. */
  trail: number;
  /** It carries its project's tag (a project's lights share one). */
  tagged: boolean;
};

/** A light standing still: a Claude session waiting on you at the pit wall, a crashed server where it
 * stopped, a server starting on the grid (in its lane, behind the start line), one stopping in the
 * pit just past the wall, a stopped project at its lane's start. */
export type Stander = {
  key: string;
  path: string;
  kind: "waiting" | "crashed" | "busy" | "stopped";
  t: number;
  offset: number;
  /** While waiting: on what, and which session. */
  turn: Turn | null;
  session: string | null;
  /** While waiting: the lane Claude's light laps in and its trail there (world units), for its way
   * to the wall when it had no light on the lap. */
  lane: number;
  trail: number;
};

export type Grid = { movers: Mover[]; standers: Stander[] };

/** A number in [0, 1) that stays the same for the same text: a project's place and pace. */
function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % 100000) / 100000;
}

/** World units a second: the website's lap, in about 34 s. */
const SPEED = 0.46;
/** A project's lights run in formation: its server ahead, Claude's sessions beside it, each place
 * this far behind the one before. Two abreast are each `PAIR` off their lane's middle, three are
 * `TRIO` apart, so the trails stay apart (world units). */
const TANDEM = 0.3;
const PAIR = 0.035;
const TRIO = 0.06;
/** The straight: a project's waiting lights stand this far apart, from its lane's far end back
 * (world units). */
const STACK = 0.22;
/** The lanes of a lap, in half road widths. */
const LANES = [-0.55, 0, 0.55];
/** The grid: the first place this far behind the start line, the next ones this far apart, or as
 * far as a tag is wide, up to `GRID_REACH` (world units). */
const GRID = 0.14;
const GRID_GAP = 0.42;
const GRID_REACH = 1.2;

/** Each project's row of Claude lights as last dealt: a session's place in it, by its id. */
const rows = new Map<string, Map<string, number>>();

/**
 * A project's sessions' places in its row of lights, the first at the front. A session keeps its
 * place for as long as it works or waits, so no light moves along the lap when another of its
 * project's comes or goes; one that comes takes the first place free.
 */
function rowOf(project: Project): Map<string, number> {
  const before = rows.get(project.path);
  const row = new Map<string, number>();
  for (const session of project.claudeSessions) {
    const place = before?.get(session.id);
    if (place !== undefined) row.set(session.id, place);
  }
  const taken = new Set(row.values());
  let free = 0;
  for (const session of project.claudeSessions) {
    if (row.has(session.id)) continue;
    while (taken.has(free)) free++;
    taken.add(free);
    row.set(session.id, free);
  }
  if (row.size) rows.set(project.path, row);
  else rows.delete(project.path);
  return row;
}

/**
 * The projects (in the tower's order) as lights on `on`. `gap`: how far apart the pit wall's
 * places are, in world units (the page knows how wide a tag is on screen); `slot`: the same for the
 * grid's, where a tag is a name alone.
 */
export function grid(projects: Project[], on: Circuit, gap: number, slot = 0): Grid {
  const movers: Mover[] = [];
  const standers: Stander[] = [];
  const straight = on.id === "straight";
  const lanes = straight ? laneOffsets(projects.length) : [];
  /** How many wait at the wall, and how many of them have their place so far. */
  const waiting = projects.reduce((count, project) => count + sessionsIn(project, "waiting").length, 0);
  let placed = 0;
  // Places round the lap are dealt evenly, by path: they stay put while the tower's order changes,
  // and no two projects start on top of each other.
  const places = new Map([...projects.map((project) => project.path)].sort().map((path, place) => [path, place]));
  let starting = 0;
  let stopping = 0;

  projects.forEach((project, index) => {
    const seed = ((places.get(project.path) ?? 0) + 0.35 * hash(project.path)) / projects.length;
    const offset = straight ? lanes[index] : LANES[Math.floor(hash(`${project.path}#lane`) * LANES.length)] * on.half;
    const speed = SPEED * (0.82 + 0.4 * hash(`${project.path}#pace`));
    const running = project.status === "running";
    // The formation: the server's column, then Claude's two, as far as the project's sessions fill
    // them (those at work and those at the wall, whose place is kept); the lane's middle between
    // them all. Claude's sessions take the two columns in turn, in the order of their places.
    const row = rowOf(project);
    const order = [...row.values()].sort((a, b) => a - b);
    const abreast = (running ? 1 : 0) + Math.min(2, order.length);
    const column = (nth: number): number => offset + (nth - (abreast - 1) / 2) * (abreast > 2 ? TRIO : 2 * PAIR);

    if (running) {
      movers.push({ key: `${project.path}#server`, path: project.path, kind: "server", warn: project.issue !== null, offset: column(0), phase: seed, speed, trail: 3.2 + 1.6 * hash(`${project.path}#trail`), tagged: !straight });
    } else if (project.status === "crashed") {
      standers.push({ key: `${project.path}#crashed`, path: project.path, kind: "crashed", t: straight ? 0.25 + 0.5 * seed : seed, offset, turn: null, session: null, lane: offset, trail: 0 });
    } else if (project.status === "busy") {
      // Starting: on the grid, in its lane behind the start line, from where it sets off. Stopping
      // (or restarting): in the pit, just past the wall. On the straight, its own start line.
      const onGrid = project.phase === "starting…";
      const t = straight ? 0 : onGrid ? on.wall.t - (GRID + Math.max(GRID_GAP, Math.min(slot, GRID_REACH)) * starting++) / on.line.length : on.wall.t + (1.5 + gap * (0.5 + stopping++)) / on.line.length;
      standers.push({ key: `${project.path}#busy`, path: project.path, kind: "busy", t, offset: straight || onGrid ? offset : on.wall.side * on.half * 1.6, turn: null, session: null, lane: offset, trail: 0 });
    } else if (straight) {
      standers.push({ key: `${project.path}#stopped`, path: project.path, kind: "stopped", t: 0, offset, turn: null, session: null, lane: offset, trail: 0 });
    }

    // Claude's lights, one a session: beside the server and a little behind, the next one beside
    // that and a little behind again. The project's tag rides with its server, else with the first
    // of them at work.
    const trail = 2.6 + 1.2 * hash(`${project.path}#trail`);
    const lead = Math.min(...sessionsIn(project, "working").map((session) => row.get(session.id) ?? 0));
    let stood = 0;
    project.claudeSessions.forEach((session) => {
      const place = row.get(session.id) ?? 0;
      const lane = column((running ? 1 : 0) + (order.indexOf(place) % 2));
      if (session.phase === "working") {
        const behind = (((running ? 1 : 0) + place) * TANDEM) / on.line.length;
        movers.push({ key: `${project.path}#claude:${session.id}`, path: project.path, kind: "claude", warn: false, offset: lane, phase: seed - behind, speed, trail, tagged: !straight && !running && place === lead });
      } else if (session.phase === "waiting") {
        // A place each at the wall; on the straight, at its lane's far end and back from it.
        const t = straight ? 1 - (stood++ * STACK) / on.line.length : on.wall.t + ((placed++ - (waiting - 1) / 2) * gap) / on.line.length;
        standers.push({ key: `${project.path}#waiting:${session.id}`, path: project.path, kind: "waiting", t, offset: straight ? offset : on.wall.side * on.half * 1.6, turn: session.turn, session: session.id, lane, trail });
      }
    });
  });

  return { movers, standers };
}

/* ---------- to the pit wall and back ---------- */

/** A light leaves the road this far before its place at the wall, and is back on it this far after
 * (world units). */
export const PIT_LANE = 1.3;

const clamp = (x: number, from: number, to: number): number => Math.min(to, Math.max(from, x));

/** 0 to 1 over `from` to `to`, gently off and gently in. */
export function smooth(from: number, to: number, x: number): number {
  const u = clamp((x - from) / (to - from), 0, 1);
  return u * u * (3 - 2 * u);
}

/** The dash to the wall, 0–1 of the way over 0–1 of its time: flat out for the first third, on the
 * brakes for the rest, and standing at the end. */
export function dash(u: number): number {
  const lift = 0.35;
  const top = 6 / (2 + lift);
  const x = clamp(u, 0, 1);
  return x <= lift ? (top / (2 * lift)) * x * x : 1 - (top / (3 * (1 - lift) ** 2)) * (1 - x) ** 3;
}

/** Seconds the dash takes over `distance` world units: a full lap in about three. */
export function dashTime(distance: number): number {
  return clamp(0.9 + 0.11 * distance, 1, 3.2);
}

/** Seconds a light takes from the wall to its place on the lap, `distance` world units ahead. */
export function leaveTime(distance: number): number {
  return clamp(1.2 + 0.1 * distance, 1.2, 3);
}

/** The tower's order: who waits on you, then Claude at work, then what runs, crashed, and the rest;
 * within each, the list's own order. */
export function towerOrder(projects: Project[]): Project[] {
  const rank = (project: Project): number => {
    if (project.claude) return 0;
    if (project.claudeWorking) return 1;
    if (project.status === "running" || project.status === "busy") return 2;
    if (project.status === "crashed") return 3;
    return 4;
  };
  return projects.map((project, index) => ({ project, index })).sort((a, b) => rank(a.project) - rank(b.project) || a.index - b.index).map((entry) => entry.project);
}

/**
 * Three letters for each project, as a timing tower writes its drivers: the first three letters
 * of the name, and for a name already taken, its first letter with the next free pair, then with a
 * number. By path.
 */
export function codes(projects: Project[]): Map<string, string> {
  const taken = new Set<string>();
  const result = new Map<string, string>();
  for (const project of projects) {
    const letters = project.name.toLocaleUpperCase("en").replace(/[^\p{L}\p{N}]/gu, "");
    const chars = [...letters];
    const candidates = [chars.slice(0, 3).join("")];
    // The first letter of each part: `pitlane-docs` is `PD` and one more letter.
    const parts = project.name.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
    if (parts.length > 1) {
      const initials = parts.map((part) => [...part.toLocaleUpperCase("en")][0]).join("");
      candidates.push([...(initials + chars.slice(1).join(""))].slice(0, 3).join(""));
    }
    for (let i = 2; i < chars.length - 1; i++) candidates.push(chars[0] + chars[i] + chars[i + 1]);
    for (let n = 2; n < 100; n++) candidates.push(`${chars.slice(0, 2).join("") || "P"}${n}`);
    const code = candidates.find((candidate) => candidate.length > 0 && !taken.has(candidate)) ?? "?";
    taken.add(code);
    result.set(project.path, code);
  }
  return result;
}
