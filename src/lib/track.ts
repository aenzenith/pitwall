// The Track page: its circuits, and what the projects put on them. A running dev server is a green
// light lapping the circuit, Claude at work an orange one; Claude waiting on you stands at the pit
// wall, a crashed server stands where it stopped, and a stopped project waits in the pit.

import type { Project, Turn } from "./types";

export type CircuitId = "night" | "oval" | "eight" | "street" | "straight";

/** The picker's order. `straight` gives every project a lane of its own. */
export const CIRCUITS: CircuitId[] = ["night", "oval", "eight", "street", "straight"];

/** The page's own settings, as the core keeps them (`settings.track`). */
export type TrackSettings = {
  /** A circuit, or `shuffle`: another one each time the page opens. */
  circuit: CircuitId | "shuffle";
  /** What a car carries: its project's three letters, its name, or nothing until pointed at. */
  labels: "code" | "name" | "hover";
  /** `calm`: half speed, short trails; `still`: nothing moves. */
  motion: "full" | "calm" | "still";
  /** Stopped projects wait in the pit, under the circuit. */
  pit: boolean;
  /** The lights' soft glow. */
  glow: boolean;
};

export const TRACK_DEFAULTS: TrackSettings = { circuit: "night", labels: "code", motion: "full", pit: true, glow: true };

/** What the settings file holds, read leniently: an unknown value falls back to the default. */
export function trackSettings(stored: Partial<Record<keyof TrackSettings, unknown>> | null | undefined): TrackSettings {
  const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T => (allowed.includes(value as T) ? (value as T) : fallback);
  return {
    circuit: pick(stored?.circuit, [...CIRCUITS, "shuffle"] as const, TRACK_DEFAULTS.circuit),
    labels: pick(stored?.labels, ["code", "name", "hover"] as const, TRACK_DEFAULTS.labels),
    motion: pick(stored?.motion, ["full", "calm", "still"] as const, TRACK_DEFAULTS.motion),
    pit: typeof stored?.pit === "boolean" ? stored.pit : TRACK_DEFAULTS.pit,
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
   * way is outside). On the straight the wall is the far end. */
  wall: { t: number; side: number };
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
  return { id, line, half: HALF, wall: { t: nearest / LINE_SAMPLES, side: line.nz[nearest] >= 0 ? 1 : -1 }, camera: LAP_CAMERA };
}

const built = new Map<CircuitId, Circuit>();

export function circuit(id: CircuitId): Circuit {
  let made = built.get(id);
  if (!made) {
    made =
      id === "straight"
        ? { id, line: straightLine(), half: 0, wall: { t: 1, side: 1 }, camera: { height: 9.5, distance: 0.2, target: 0 } }
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

/** The straight's lanes: each project's distance from the centre line, the first at the back. */
export function laneOffsets(count: number): number[] {
  const gap = Math.min(0.6, 4.6 / Math.max(1, count - 1));
  return Array.from({ length: count }, (_, i) => (i - (count - 1) / 2) * gap);
}

/* ---------- what runs ---------- */

/** A light on the move: a dev server (green; amber with an issue) or Claude at work (orange). */
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
  /** It carries its project's tag (a project's two lights share one). */
  tagged: boolean;
};

/** A light standing still: Claude waiting on you at the pit wall, a crashed server where it
 * stopped, a server starting or stopping at the pit exit, a stopped project at its lane's start. */
export type Stander = {
  key: string;
  path: string;
  kind: "waiting" | "crashed" | "busy" | "stopped";
  t: number;
  offset: number;
  /** While waiting: on what. */
  turn: Turn | null;
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
/** Claude's light runs beside its project's server: this far behind it, and each this far off
 * their lane's middle, so the two trails stay two (world units). */
const TANDEM = 0.3;
const PAIR = 0.035;
/** The lanes of a lap, in half road widths. */
const LANES = [-0.55, 0, 0.55];

/**
 * The projects (in the tower's order) as lights on `on`. `gap`: how far apart the pit wall's
 * places are, in world units (the page knows how wide a tag is on screen).
 */
export function grid(projects: Project[], on: Circuit, gap: number): Grid {
  const movers: Mover[] = [];
  const standers: Stander[] = [];
  const straight = on.id === "straight";
  const lanes = straight ? laneOffsets(projects.length) : [];
  const waiting = projects.filter((project) => project.claude);
  // Places round the lap are dealt evenly, by path: they stay put while the tower's order changes,
  // and no two projects start on top of each other.
  const places = new Map([...projects.map((project) => project.path)].sort().map((path, place) => [path, place]));
  let starting = 0;

  projects.forEach((project, index) => {
    const seed = ((places.get(project.path) ?? 0) + 0.35 * hash(project.path)) / projects.length;
    const offset = straight ? lanes[index] : LANES[Math.floor(hash(`${project.path}#lane`) * LANES.length)] * on.half;
    const speed = SPEED * (0.82 + 0.4 * hash(`${project.path}#pace`));
    const running = project.status === "running";
    const working = project.claudeWorking && !project.claude;

    if (running) {
      movers.push({ key: `${project.path}#server`, path: project.path, kind: "server", warn: project.issue !== null, offset: working ? offset - PAIR : offset, phase: seed, speed, trail: 3.2 + 1.6 * hash(`${project.path}#trail`), tagged: !straight });
    } else if (project.status === "crashed") {
      standers.push({ key: `${project.path}#crashed`, path: project.path, kind: "crashed", t: straight ? 0.25 + 0.5 * seed : seed, offset, turn: null });
    } else if (project.status === "busy") {
      // At the pit exit, just past the wall; on the straight, on its own start line.
      const t = straight ? 0 : on.wall.t + (1.5 + gap * (0.5 + starting++)) / on.line.length;
      standers.push({ key: `${project.path}#busy`, path: project.path, kind: "busy", t, offset: straight ? offset : on.wall.side * on.half * 1.6, turn: null });
    } else if (straight) {
      standers.push({ key: `${project.path}#stopped`, path: project.path, kind: "stopped", t: 0, offset, turn: null });
    }

    if (working) {
      // Beside its server and a little behind; alone, it has the lane and carries the tag itself.
      const behind = running ? TANDEM / on.line.length : 0;
      movers.push({ key: `${project.path}#claude`, path: project.path, kind: "claude", warn: false, offset: running ? offset + PAIR : offset, phase: seed - behind, speed, trail: 2.6 + 1.2 * hash(`${project.path}#trail`), tagged: !straight && !running });
    }

    if (project.claude) {
      const place = waiting.indexOf(project);
      const t = straight ? 1 : on.wall.t + ((place - (waiting.length - 1) / 2) * gap) / on.line.length;
      standers.push({ key: `${project.path}#waiting`, path: project.path, kind: "waiting", t, offset: straight ? offset : on.wall.side * on.half * 1.6, turn: project.claude });
    }
  });

  return { movers, standers };
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
