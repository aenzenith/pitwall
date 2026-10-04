// The Track page's circuit, drawn with WebGL as additive point sprites, after the website's night
// circuit (pitwall-website: resources/js/lib/circuit.ts). Lines and trails are points packed far
// closer than they are wide, so they read as strokes. Everything moves on the GPU: a point knows
// where along its light's trail it sits, the vertex shader looks the circuit's centre line up and
// places it. A frame costs a handful of uniforms. The circuit's own marks are placed the same way,
// so one circuit can turn into another: the line the shader holds is mixed from the two.

import { LINE_SAMPLES, onLine, type Circuit } from "./track";

/** A light on the circuit for one frame. */
export type Light = {
  /** Where its head is, in laps: on a closed line whole laps count for nothing, an open one runs
   * 0–1. */
  head: number;
  /** How much of the line its trail covers (0–1). */
  trail: number;
  /** Its brightness: 0 hides it. */
  gain: number;
  /** World units to the left of the centre line. */
  offset: number;
  colour: readonly [number, number, number];
  /** A change of lane on its way (to the pit wall, or back from it): `offset` up to `from`, this
   * `offset` from `to` on, both in laps as `head` is. Its trail bends where it did. */
  turn?: { from: number; to: number; offset: number };
  /** How much an open line's two ends dim it (1 unless given): it goes out into the far end while
   * it comes up at the near one. 0 for a light that stops at the end. */
  ends?: number;
  /** Its trail's own brightness (1 unless given). */
  tail?: number;
};

/** The room the page keeps round the circuit, in CSS pixels from the canvas's edges. */
export type Inset = { top: number; right: number; bottom: number; left: number };

export interface Renderer {
  /** The circuit drawn from now on. `lanes`: the straight's lanes (their offsets); `wall`: half
   * the pit wall's length, in world units. `animate`: the one drawn until now turns into it. A lap
   * into another lap: the road takes the new shape, the start line and the pit wall move along it
   * to their new place. The straight and a lap slide past each other: to the straight upwards,
   * from it downwards. The same circuit: its wall grows or shrinks, other lanes fade over. */
  setCircuit(circuit: Circuit, lanes: readonly number[], wall: number, animate?: boolean): void;
  /** Fits the circuit into the canvas, inside `inset`. Call when either changes. */
  layout(inset: Inset): void;
  draw(time: number, lights: readonly Light[], glow: boolean): void;
  /** Where a point of the circuit lands on the canvas, in CSS pixels. While a lap turns into
   * another, where it is on the way: with the pit wall (what stands at it rides with it), or,
   * `onRoad`, with the road (where the lights are drawn). */
  project(t: number, offset: number, onRoad?: boolean): [number, number];
  /** One circuit is turning into another: a lap into a lap (`morph`), or the straight and a lap
   * sliding past each other (`slide`). */
  moving(): "morph" | "slide" | null;
  /** CSS pixels to a world unit, at the pit wall; 0 before the first layout. */
  scale(): number;
  stop(): void;
}

/** A trail's points, its halo's, and the two of its head. */
const TRAIL = 1400;
const HALO = 70;
const PER_LIGHT = TRAIL + HALO + 2;
/** World units to a sprite's size unit: a trail's head (6.4) is about a third of the road wide. */
const SPRITE = 0.0095;
/** The lens: half the vertical field of view is 0.36 rad. */
const FOCAL = 1 / Math.tan(0.36);

const vertexSource = (line: number, lights: number): string => `
#define LINE ${line}
#define LIGHTS ${lights}
attribute vec4 aA;
attribute vec4 aB;
uniform mat4 uVP;
uniform vec4 uLine[LINE];
uniform vec4 uLightA[LIGHTS];
uniform vec4 uLightB[LIGHTS];
uniform vec4 uLightC[LIGHTS];
uniform float uTime;
uniform float uPx;
uniform float uGlow;
uniform float uClosed;
uniform vec2 uFade;
uniform vec2 uView;
uniform vec2 uMark;
uniform vec4 uWall;
varying vec4 vCol;
const float TAU = 6.28318530718;

// The centre line at t, moved off to its left.
vec2 onLine(float t, float off) {
    float last = float(LINE - 1);
    float f = uClosed > .5 ? fract(t) * float(LINE) : clamp(t, 0., 1.) * last;
    float i = min(floor(f), last);
    float j = uClosed > .5 ? mod(i + 1., float(LINE)) : min(i + 1., last);
    vec4 p = mix(uLine[int(i)], uLine[int(j)], f - i);
    return p.xy + normalize(p.zw) * off;
}

// An open line's two ends: a light goes out into the far one as it comes up at the near one.
float ends(float t) {
    return smoothstep(0., .09, t) * (1. - smoothstep(.91, 1., t));
}

void main() {
    float kind = aA.z;
    vec3 pos; vec3 col; float size; float alpha;
    if (kind < .5) {
        // A mark of the circuit: where along the line, how far off it, how big; aB = alpha, its
        // place for the slow pulse that runs round the edges (-1: none), how white. uMark: its
        // set's shift along the line and its alpha (two sets cross while a circuit changes).
        float at = aA.x + uMark.x;
        float off = aA.y;
        if (aB.y < -1.5) {
            // The start line and the pit wall, about a place of their own (uWall: where along the
            // line, half the wall's length in laps, which side is outside, half the road's width):
            // they move along the road while it changes.
            at = uWall.x + aA.x * uWall.y;
            off = aA.y * uWall.z * uWall.w;
        }
        vec2 p = onLine(at, off);
        pos = vec3(p.x, 0., p.y);
        float pulse = aB.y < 0. ? 1. : .5 + .5 * sin(TAU * (aB.y * 6. - uTime * .05));
        col = mix(vec3(.50, .56, .66), vec3(.92), aB.z);
        size = aA.w;
        alpha = aB.x * (.5 + .5 * pulse) * uMark.y;
    } else if (kind < 2.5) {
        vec4 light = uLightA[int(aB.x)];
        vec4 tint = uLightB[int(aB.x)];
        // Where it changes lane (x to y along the line, to the offset z), and how much an open
        // line's ends dim it (w).
        vec4 turn = uLightC[int(aB.x)];
        if (kind < 1.5) {
            // A trail: aA.x = how far behind the head, aA.y = across its width, aA.w = 1 for the
            // wide soft halo.
            float lag = aA.x;
            float halo = aA.w;
            float t = light.x - lag * light.y;
            float off = mix(light.w, turn.z, smoothstep(turn.x, turn.y, t));
            // An open line: what is behind its start has still to go out at the far end.
            if (uClosed < .5 && t < 0.) t += 1.;
            vec2 p = onLine(t, off + aA.y * .0085);
            pos = vec3(p.x, 0., p.y);
            col = tint.rgb;
            size = mix(mix(6.4, 1.3, lag), mix(30., 12., lag), halo);
            // tint.a: fewer points to a unit of a longer trail, each gives that much more light.
            alpha = pow(1. - lag, 1.35) * mix(.2, .05, halo) * light.z * tint.a * mix(1., uGlow, halo);
            if (uClosed < .5) alpha *= mix(1., ends(t), turn.w);
        } else {
            // The car: a bright head, and its glow (aB.y = 1).
            vec2 p = onLine(light.x, mix(light.w, turn.z, smoothstep(turn.x, turn.y, light.x)));
            pos = vec3(p.x, .01, p.y);
            col = mix(tint.rgb, vec3(1.), .55);
            size = aA.w;
            alpha = aA.x * light.z * mix(1., uGlow, aB.y);
            if (uClosed < .5) alpha *= mix(1., ends(light.x), turn.w);
        }
    } else {
        // Dust drifting above the circuit.
        pos = vec3(aA.x, fract(aA.y + uTime * .006 * (.4 + aA.w)) * 2.6, aB.x);
        col = vec3(.62, .68, .78);
        size = .9 + aA.w;
        alpha = (.10 + .14 * aA.w) * uGlow;
    }
    gl_Position = uVP * vec4(pos, 1.);
    float w = max(gl_Position.w, .001);
    // The far side of the circuit is dimmer.
    alpha *= clamp((uFade.x - w) / uFade.y, 0., 1.);
    if (kind > 2.5) alpha *= smoothstep(2.5, 5., w);
    // Nothing is drawn finer than four pixels across, where a sprite would shimmer: a smaller one
    // keeps its light and spreads it.
    float px = size * uPx / w;
    alpha *= min(1., px * px / 16.);
    px = max(px, 4.);
    // A sprite is dropped whole once its centre leaves the frame: it dims as it comes to the edge.
    vec2 room = (1. - abs(gl_Position.xy / w)) * uView * .5;
    alpha *= clamp(min(room.x, room.y) / (px * .5), 0., 1.);
    if (alpha <= 0.) {
        gl_Position = vec4(2., 2., 2., 1.);
        gl_PointSize = 1.;
        vCol = vec4(0.);
        return;
    }
    gl_PointSize = px;
    vCol = vec4(col, alpha);
}`;

const FRAGMENT = `
precision mediump float;
varying vec4 vCol;
void main() {
    float r = length(gl_PointCoord - .5) * 2.;
    float a = smoothstep(1., 0., r);
    a *= a * vCol.a;
    gl_FragColor = vec4(vCol.rgb * a, a);
}`;

function compile(gl: WebGLRenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
  console.warn("track circuit:", gl.getShaderInfoLog(shader));
  gl.deleteShader(shader);
  return null;
}

const smooth = (from: number, to: number, x: number): number => {
  const u = Math.min(1, Math.max(0, (x - from) / (to - from)));
  return u * u * (3 - 2 * u);
};

/** The road's marks, eight floats a point (aA, aB): its edges and centre dashes; on the straight,
 * each lane's line and the two lines across them. Each is a place along the line and a distance
 * off it: the shader puts it there. */
function roadMarks(circuit: Circuit, lanes: readonly number[]): number[] {
  const { line, half } = circuit;
  const data: number[] = [];
  const put = (t: number, offset: number, size: number, alpha: number, pulse: number, white: number): void => {
    data.push(t, offset, 0, size, alpha, pulse, white, 0);
  };

  if (line.closed) {
    const edge = Math.round(line.length * 330);
    for (let i = 0; i < edge; i++) {
      put(i / edge, -half, 2.2, 0.3, i / edge, 0);
      put(i / edge, half, 2.2, 0.3, i / edge, 0);
    }
    const centre = Math.round(line.length * 267);
    const dashes = Math.round(line.length * 9);
    for (let i = 0; i < centre; i++) {
      const dash = ((i / centre) * dashes) % 1;
      const alpha = 0.09 * smooth(0, 0.1, dash) * smooth(0.62, 0.52, dash);
      if (alpha > 0.004) put(i / centre, 0, 1.5, alpha, i / centre, 0);
    }
  } else {
    const dots = Math.round(line.length * 120);
    const dashes = Math.round(line.length * 6);
    for (const lane of lanes) {
      for (let i = 0; i < dots; i++) {
        const dash = ((i / dots) * dashes) % 1;
        const alpha = 0.42 * smooth(0, 0.1, dash) * smooth(0.62, 0.52, dash);
        if (alpha > 0.004) put(i / dots, lane, 2, alpha, -1, 0);
      }
    }
    const from = Math.min(0, ...lanes) - 0.25;
    const to = Math.max(0, ...lanes) + 0.25;
    const across = Math.round((to - from) * 330);
    for (let i = 0; i <= across; i++) {
      const lane = from + ((to - from) * i) / across;
      put(0, lane, 2.6, 0.34, -1, 0.4);
      put(1, lane, 3.4, 0.6, -1, 0.5);
    }
  }
  return data;
}

/** The start line's points, and the pit wall's: one set for every lap. */
const START_POINTS = 81;
const WALL_POINTS = 1401;
/** The points to a world unit the wall's brightness is set for. */
const WALL_DENSITY = 330;

/** The start line across the road and the pit wall outside it, about a place the shader is told
 * (pulse -2): along the line in half wall lengths, off it in half road widths. */
function wallMarks(): number[] {
  const data: number[] = [];
  for (let i = 0; i < START_POINTS; i++) data.push(0, i / 40 - 1, 0, 3, 0.5, -2, 1, 0);
  for (let i = 0; i < WALL_POINTS; i++) data.push((i / (WALL_POINTS - 1)) * 2 - 1, 2.3, 0, 3.4, 0.6, -2, 0.5, 0);
  return data;
}

/** What never changes: the dust, then every light's trail, halo and head. A fixed seed: the same
 * dust on every opening. */
function fixed(lights: number): number[] {
  let seed = 7;
  const random = (): number => {
    seed = (seed * 16807) % 2147483647;
    return (seed - 1) / 2147483646;
  };
  const data: number[] = [];
  for (let i = 0; i < 420; i++) data.push((random() - 0.5) * 12, random(), 3, random(), (random() - 0.5) * 9, 0, 0, 0);
  for (let light = 0; light < lights; light++) {
    // Across its width a trail is filled evenly (golden-ratio steps), not at random: no clumps.
    for (let i = 0; i < TRAIL; i++) data.push(i / TRAIL, ((i * 0.6180339887) % 1) - 0.5, 1, 0, light, 0, 0, 0);
    for (let i = 0; i < HALO; i++) data.push(i / HALO, 0, 1, 1, light, 0, 0, 0);
    data.push(1, 0, 2, 9, light, 0, 0, 0);
    data.push(0.22, 0, 2, 46, light, 1, 0, 0);
  }
  return data;
}

const DUST = 420;

/** A lap turns into another over this long; the straight and a lap slide past each other; the same
 * circuit's marks (a longer wall, another lane) change (ms). */
const MORPH_MS = 900;
export const SLIDE_MS = 800;
const MARKS_MS = 320;
/** How far the straight and a lap slide, of the canvas's height. */
const SLIDE = 0.3;

/** A centre line as the renderer keeps it: LINE_SAMPLES × (x, z, nx, nz). */
function lineData(line: Circuit["line"]): Float32Array {
  const data = new Float32Array(LINE_SAMPLES * 4);
  for (let i = 0; i < LINE_SAMPLES; i++) data.set([line.x[i], line.z[i], line.nx[i], line.nz[i]], i * 4);
  return data;
}

/** How many samples to turn `to` by, so each of its points is as near as can be to the one of
 * `from` it comes from: a lap then turns into another without twisting. */
function align(from: Float32Array, to: Float32Array): number {
  let best = 0;
  let least = Infinity;
  for (let shift = 0; shift < LINE_SAMPLES; shift++) {
    let sum = 0;
    for (let i = 0; i < LINE_SAMPLES && sum < least; i += 2) {
      const j = ((i + shift) % LINE_SAMPLES) * 4;
      const dx = from[i * 4] - to[j];
      const dz = from[i * 4 + 1] - to[j + 1];
      sum += dx * dx + dz * dz;
    }
    if (sum < least) {
      least = sum;
      best = shift;
    }
  }
  return best;
}

/** Null where WebGL isn't to be had: the page's lists work without the circuit. */
export function startCircuit(canvas: HTMLCanvasElement): Renderer | null {
  const gl = canvas.getContext("webgl", { antialias: false, alpha: true, premultipliedAlpha: true, depth: false, stencil: false, powerPreference: "low-power" });
  if (!gl) return null;

  // The centre line and the lights go to the shader as uniforms: a GPU with little room for
  // them gets a coarser line and fewer lights.
  const room = Number(gl.getParameter(gl.MAX_VERTEX_UNIFORM_VECTORS)) || 128;
  const lineCount = room >= 400 ? LINE_SAMPLES : 64;
  const lightCount = room >= 400 ? 32 : 12;

  const vertex = compile(gl, gl.VERTEX_SHADER, vertexSource(lineCount, lightCount));
  const fragment = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  const program = gl.createProgram();
  if (!vertex || !fragment || !program) return null;
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.bindAttribLocation(program, 0, "aA");
  gl.bindAttribLocation(program, 1, "aB");
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.warn("track circuit:", gl.getProgramInfoLog(program));
    return null;
  }
  gl.useProgram(program);

  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 4, gl.FLOAT, false, 32, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 32, 16);
  gl.disable(gl.DEPTH_TEST);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE);
  gl.clearColor(0, 0, 0, 0);

  const at = (name: string): WebGLUniformLocation | null => gl.getUniformLocation(program, name);
  const uniforms = {
    vp: at("uVP"),
    line: at("uLine"),
    lightA: at("uLightA"),
    lightB: at("uLightB"),
    lightC: at("uLightC"),
    time: at("uTime"),
    px: at("uPx"),
    glow: at("uGlow"),
    closed: at("uClosed"),
    fade: at("uFade"),
    view: at("uView"),
    mark: at("uMark"),
    wall: at("uWall"),
  };

  /** A circuit as it is drawn: its centre line, its marks, and how it sits in the canvas. */
  type Stage = {
    circuit: Circuit;
    /** The straight's lanes: they widen what the camera has to take in. */
    lanes: readonly number[];
    line: Float32Array;
    road: Float32Array;
    /** Half the pit wall's length, in laps. */
    wall: number;
    matrix: Float32Array;
    fade: [number, number];
    /** The fit's own scale, and CSS pixels to a world unit at the wall. */
    fit: number;
    wallScale: number;
  };

  /** One circuit turning into the next. `morph`: the line is mixed from the two (`shift`: how far
   * the new one is turned for that, in laps), their road marks cross, the wall moves along;
   * `lights`: the lights of the one left go out where they were, the new ones come up. `slide`: the
   * straight and a lap pass each other. */
  type Change = { kind: "morph" | "slide"; since: number; takes: number; shift: number; lights: boolean; ghosts: number; ghostA: Float32Array; ghostB: Float32Array; ghostC: Float32Array };

  // The buffer: the dust and every light's points, the start line and the wall, then the road
  // marks of the circuit drawn, then (while it changes) those of the one it comes from.
  const steady = new Float32Array([...fixed(lightCount), ...wallMarks()]);
  const wallFirst = steady.length / 8 - START_POINTS - WALL_POINTS;
  const roadFirst = steady.length / 8;
  const lightA = new Float32Array(lightCount * 4);
  const lightB = new Float32Array(lightCount * 4);
  const lightC = new Float32Array(lightCount * 4);
  /** The lights of the last frame, as the shader got them. */
  let lit = 0;
  const mixedLine = new Float32Array(LINE_SAMPLES * 4);
  const mixedMatrix = new Float32Array(16);
  const movedMatrix = new Float32Array(16);
  const thinned = new Float32Array(lineCount * 4);
  /** The mixed line as the page's own `onLine` reads it. */
  const mixedRoad: Circuit["line"] = { closed: true, length: 1, x: new Float32Array(LINE_SAMPLES), z: new Float32Array(LINE_SAMPLES), nx: new Float32Array(LINE_SAMPLES), nz: new Float32Array(LINE_SAMPLES) };

  let shown: Stage | null = null;
  let before: Stage | null = null;
  let change: Change | null = null;
  /** While a lap turns into another: how far along the mixed line the new lap's places are, on the
   * road and with the wall (laps). */
  let mixing: { road: number; wall: number } | null = null;
  /** The stage whose line the shader holds; null while it holds a mix. */
  let held: Stage | null = null;
  let inset: Inset = { top: 0, right: 0, bottom: 0, left: 0 };
  let stopped = false;

  /** The camera's view of a ground point, before the fit: clip x, y and w. */
  function view(camera: Circuit["camera"], x: number, z: number, aspect: number): [number, number, number] {
    const { height, distance, target } = camera;
    const l = Math.hypot(height, distance - target);
    const zy = height / l;
    const zz = (distance - target) / l;
    const yc = zz * -height - zy * (z - distance);
    const zc = zy * -height + zz * (z - distance);
    return [(FOCAL / aspect) * x, FOCAL * yc, -zc];
  }

  function size(): [number, number] {
    return [Math.max(1, canvas.clientWidth), Math.max(1, canvas.clientHeight)];
  }

  /** Fits a stage's circuit into the canvas, inside the inset. */
  function fitStage(stage: Stage): void {
    const [width, height] = size();
    const aspect = width / height;
    const { line, half, camera } = stage.circuit;

    // Everything the frame has to hold: the road's outer edges and the pit wall, or the lanes.
    let left = Infinity;
    let right = -Infinity;
    let top = -Infinity;
    let bottom = Infinity;
    let near = Infinity;
    let far = -Infinity;
    const reach = line.closed ? [half * 2.4, -half * 2.4] : [Math.min(0, ...stage.lanes) - 0.3, Math.max(0, ...stage.lanes) + 0.3];
    for (let i = 0; i <= 96; i++) {
      for (const offset of reach) {
        const [x, z] = onLine(line, i / 96, offset);
        const [cx, cy, w] = view(camera, x, z, aspect);
        left = Math.min(left, cx / w);
        right = Math.max(right, cx / w);
        bottom = Math.min(bottom, cy / w);
        top = Math.max(top, cy / w);
        near = Math.min(near, w);
        far = Math.max(far, w);
      }
    }

    // The same scale both ways, so the circuit keeps its shape; never so large that a short lap
    // fills a wide window with road.
    const roomX = Math.max(80, width - inset.left - inset.right);
    const roomY = Math.max(80, height - inset.top - inset.bottom);
    const fit = Math.min(roomX / (((right - left) * width) / 2), roomY / (((top - bottom) * height) / 2), 2.4);
    const centreX = ((inset.left + roomX / 2) / width) * 2 - 1;
    const centreY = 1 - ((inset.top + roomY / 2) / height) * 2;
    const moveX = centreX - (fit * (left + right)) / 2;
    const moveY = centreY - (fit * (top + bottom)) / 2;

    // Projection × view for a camera on the z axis looking down at the circuit, then the fit.
    const { height: up, distance, target } = camera;
    const l = Math.hypot(up, distance - target);
    const zy = up / l;
    const zz = (distance - target) / l;
    const nearPlane = 0.1;
    const farPlane = 120;
    const depth = (farPlane + nearPlane) / (nearPlane - farPlane);
    const depthMove = (2 * farPlane * nearPlane) / (nearPlane - farPlane);
    // Camera space: x stays, y = zz * (py - up) - zy * (pz - distance), z = zy * (py - up) + zz * (pz - distance).
    const eyeY = zz * -up + zy * distance;
    const eyeZ = zy * -up - zz * distance;
    const rows = [
      [FOCAL / aspect, 0, 0, 0],
      [0, FOCAL * zz, -FOCAL * zy, FOCAL * eyeY],
      [0, depth * zy, depth * zz, depth * eyeZ + depthMove],
      [0, -zy, -zz, -eyeZ],
    ];
    for (let c = 0; c < 4; c++) {
      stage.matrix[c * 4] = fit * rows[0][c] + moveX * rows[3][c];
      stage.matrix[c * 4 + 1] = fit * rows[1][c] + moveY * rows[3][c];
      stage.matrix[c * 4 + 2] = rows[2][c];
      stage.matrix[c * 4 + 3] = rows[3][c];
    }

    const span = Math.max(0.5, far - near);
    stage.fit = fit;
    stage.fade = [far + 1.6 * span, 2.6 * span];
    const [wx, wz] = onLine(line, stage.circuit.wall.t, 0);
    stage.wallScale = (fit * FOCAL * height) / 2 / view(camera, wx, wz, aspect)[2];
  }

  function layout(next: Inset): void {
    inset = next;
    if (shown) fitStage(shown);
  }

  function setCircuit(next: Circuit, lanes: readonly number[], wall: number, animate = false): void {
    const from = shown;
    const stage: Stage = { circuit: next, lanes, line: lineData(next.line), road: new Float32Array(roadMarks(next, lanes)), wall: wall / next.line.length, matrix: new Float32Array(16), fade: [100, 1], fit: 1, wallScale: 0 };
    shown = stage;
    fitStage(stage);

    // The circuit a change leads to, with other marks (its wall's length follows the fit): the
    // change goes on. Any other gives way to the next one: it is over where it was going.
    const under = change !== null && before !== null && from?.circuit === next && performance.now() - change.since < change.takes;
    if (!under) {
      before = null;
      change = null;
    }
    if (!under && animate && from && !stopped) {
      const same = from.circuit === next;
      const morph = same || (from.circuit.line.closed && next.line.closed);
      before = from;
      change = {
        kind: morph ? "morph" : "slide",
        since: performance.now(),
        takes: same ? MARKS_MS : morph ? MORPH_MS : SLIDE_MS,
        shift: same || !morph ? 0 : align(from.line, stage.line) / LINE_SAMPLES,
        lights: !same,
        ghosts: same ? 0 : lit,
        ghostA: lightA.slice(),
        ghostB: lightB.slice(),
        ghostC: lightC.slice(),
      };
    }

    const buffer = new Float32Array(steady.length + stage.road.length + (before ? before.road.length : 0));
    buffer.set(steady);
    buffer.set(stage.road, steady.length);
    if (before) buffer.set(before.road, steady.length + stage.road.length);
    gl!.bufferData(gl!.ARRAY_BUFFER, buffer, gl!.STATIC_DRAW);
    held = null;
    // Where everything is as the change begins, for whoever asks before the next frame.
    mix(progress());
  }

  /** Hands the shader a centre line, thinned to what it holds. */
  function hold(line: Float32Array, closed: boolean): void {
    for (let i = 0; i < lineCount; i++) {
      const k = closed ? Math.round((i * LINE_SAMPLES) / lineCount) % LINE_SAMPLES : Math.round((i * (LINE_SAMPLES - 1)) / (lineCount - 1));
      thinned.set(line.subarray(k * 4, k * 4 + 4), i * 4);
    }
    gl!.uniform4fv(uniforms.line, thinned);
    gl!.uniform1f(uniforms.closed, closed ? 1 : 0);
  }

  function holdStage(stage: Stage): void {
    if (held === stage) return;
    hold(stage.line, stage.circuit.line.closed);
    held = stage;
  }

  /** How far the change under way has come, 0–1; 1 with none. A finished one is let go. */
  function progress(): number {
    if (!change || !before) return 1;
    const p = (performance.now() - change.since) / change.takes;
    if (p < 1) return p;
    change = null;
    before = null;
    mixing = null;
    return 1;
  }

  /** While a lap turns into another: the line and the matrix part of the way (the change has come
   * `p` of it), and where the new lap's places are on that line. Returns `p`, eased. */
  function mix(p: number): number {
    if (!change || !before || !shown || change.kind !== "morph") {
      mixing = null;
      return p;
    }
    const e = smooth(0, 1, p);
    for (let i = 0; i < 16; i++) mixedMatrix[i] = before.matrix[i] + (shown.matrix[i] - before.matrix[i]) * e;
    if (before.circuit === shown.circuit) {
      mixing = null;
      return e;
    }
    // Each point on its way from where it was to where its nearest of the new lap is.
    const turned = Math.round(change.shift * LINE_SAMPLES);
    for (let i = 0; i < LINE_SAMPLES; i++) {
      const j = ((i + turned) % LINE_SAMPLES) * 4;
      for (let c = 0; c < 4; c++) mixedLine[i * 4 + c] = before.line[i * 4 + c] + (shown.line[j + c] - before.line[i * 4 + c]) * e;
      mixedRoad.x[i] = mixedLine[i * 4];
      mixedRoad.z[i] = mixedLine[i * 4 + 1];
      mixedRoad.nx[i] = mixedLine[i * 4 + 2];
      mixedRoad.nz[i] = mixedLine[i * 4 + 3];
    }
    mixedRoad.length = before.circuit.line.length + (shown.circuit.line.length - before.circuit.line.length) * e;
    // The wall goes the short way round from its old place to its new one.
    const from = before.circuit.wall.t;
    let way = shown.circuit.wall.t - change.shift - from;
    way -= Math.round(way);
    mixing = { road: -change.shift, wall: from + way * e - shown.circuit.wall.t };
    return e;
  }

  /** The matrix moved up the canvas by `dy` (of its height). */
  function moved(matrix: Float32Array, dy: number): Float32Array {
    movedMatrix.set(matrix);
    for (let c = 0; c < 4; c++) movedMatrix[c * 4 + 1] += 2 * dy * matrix[c * 4 + 3];
    return movedMatrix;
  }

  function draw(time: number, lights: readonly Light[], glow: boolean): void {
    if (stopped || !shown) return;
    const [width, height] = size();
    // Two buffer pixels to a CSS pixel on every screen (a 1x display gets the frame supersampled,
    // which keeps a hairline smooth there), less on a canvas too wide for that.
    const ratio = Math.min(2, 4096 / width);
    const bufferWidth = Math.round(width * ratio);
    const bufferHeight = Math.round(height * ratio);
    if (canvas.width !== bufferWidth || canvas.height !== bufferHeight) {
      canvas.width = bufferWidth;
      canvas.height = bufferHeight;
    }
    gl!.viewport(0, 0, bufferWidth, bufferHeight);
    gl!.uniform1f(uniforms.time, time);
    gl!.uniform1f(uniforms.glow, glow ? 1 : 0);
    gl!.uniform2f(uniforms.view, bufferWidth, bufferHeight);
    gl!.clear(gl!.COLOR_BUFFER_BIT);

    /** How the circuit sits in the canvas, for what is drawn next. */
    const frame = (matrix: Float32Array, fit: number, fade: readonly [number, number]): void => {
      gl!.uniformMatrix4fv(uniforms.vp, false, matrix);
      gl!.uniform1f(uniforms.px, (SPRITE * fit * FOCAL * bufferHeight) / 2);
      gl!.uniform2f(uniforms.fade, fade[0], fade[1]);
    };
    const road = (first: number, count: number, shift: number, alpha: number): void => {
      if (alpha <= 0 || !count) return;
      gl!.uniform2f(uniforms.mark, shift, alpha);
      gl!.drawArrays(gl!.POINTS, first, count);
    };
    /** The start line and the wall: at `at` along the line, the wall `half` laps each way, on a
     * line `length` long. */
    const wall = (of: Circuit, at: number, half: number, length: number, alpha: number): void => {
      if (alpha <= 0 || !of.line.closed) return;
      gl!.uniform4f(uniforms.wall, at, half, of.wall.side, of.half);
      gl!.uniform2f(uniforms.mark, 0, alpha);
      gl!.drawArrays(gl!.POINTS, wallFirst, START_POINTS);
      // The same points whatever its length: each gives the light its share of the line needs.
      gl!.uniform2f(uniforms.mark, 0, alpha * Math.min(2, (2 * half * length * WALL_DENSITY) / WALL_POINTS));
      gl!.drawArrays(gl!.POINTS, wallFirst + START_POINTS, WALL_POINTS);
    };
    /** The lights given, from the `count`th on: `shift` laps along a `line`. */
    const given = (count: number, line: Circuit["line"], shift: number, gain: number): number => {
      if (gain <= 0) return count;
      for (const light of lights) {
        if (count === lightCount) break;
        // Whole laps are dropped here, from the head and from where it changes lane alike.
        const head = light.head + (line.closed ? shift : 0);
        const lap = line.closed ? Math.floor(head) : 0;
        lightA.set([head - lap, light.trail, light.gain * gain, light.offset], count * 4);
        // A trail's points lie this much further apart than the ones the alpha was set for.
        lightB.set([light.colour[0], light.colour[1], light.colour[2], Math.max(0.05, (light.trail * line.length) / 3.5) * (light.tail ?? 1)], count * 4);
        // No change of lane: one that starts where no light ever gets.
        const turn = light.turn;
        lightC.set(turn ? [turn.from + shift - lap, turn.to + shift - lap, turn.offset, light.ends ?? 1] : [8, 9, 0, light.ends ?? 1], count * 4);
        count++;
      }
      return count;
    };
    /** The lights of the circuit left, where they were, on their way out. */
    const ghosts = (count: number, gain: number): number => {
      if (!change || gain <= 0) return count;
      for (let i = 0; i < change.ghosts && count < lightCount; i++, count++) {
        lightA.set(change.ghostA.subarray(i * 4, i * 4 + 4), count * 4);
        lightA[count * 4 + 2] *= gain;
        lightB.set(change.ghostB.subarray(i * 4, i * 4 + 4), count * 4);
        lightC.set(change.ghostC.subarray(i * 4, i * 4 + 4), count * 4);
      }
      return count;
    };
    const lightsOn = (count: number): void => {
      if (!count) return;
      gl!.uniform4fv(uniforms.lightA, lightA);
      gl!.uniform4fv(uniforms.lightB, lightB);
      gl!.uniform4fv(uniforms.lightC, lightC);
      gl!.drawArrays(gl!.POINTS, DUST, count * PER_LIGHT);
    };
    const mixed = (a: number, b: number, e: number): number => a + (b - a) * e;

    const marked = shown.road.length / 8;
    const p = progress();
    const e = mix(p);

    if (change && before && change.kind === "slide") {
      // The straight and a lap pass each other: to the straight upwards, from it downwards. The
      // one left goes out on its way, the new one comes up on its own.
      const up = shown.circuit.line.closed ? -1 : 1;
      const gone = 1 - smooth(0, 0.7, p);
      const come = smooth(0.3, 1, p);
      const slid = smooth(0, 1, p);
      for (let i = 0; i < 16; i++) mixedMatrix[i] = mixed(before.matrix[i], shown.matrix[i], slid);
      frame(mixedMatrix, mixed(before.fit, shown.fit, slid), [mixed(before.fade[0], shown.fade[0], slid), mixed(before.fade[1], shown.fade[1], slid)]);
      gl!.drawArrays(gl!.POINTS, 0, DUST);

      holdStage(before);
      frame(moved(before.matrix, up * SLIDE * slid), before.fit, before.fade);
      road(roadFirst + marked, before.road.length / 8, 0, gone);
      wall(before.circuit, before.circuit.wall.t, before.wall, before.circuit.line.length, gone);
      lightsOn(ghosts(0, gone));

      holdStage(shown);
      frame(moved(shown.matrix, -up * SLIDE * (1 - slid)), shown.fit, shown.fade);
      road(roadFirst, marked, 0, come);
      wall(shown.circuit, shown.circuit.wall.t, shown.wall, shown.circuit.line.length, come);
      lightsOn(given(0, shown.circuit.line, 0, come));
      return;
    }

    if (change && before) {
      // A lap into another, or the same circuit with other marks.
      const other = before.circuit !== shown.circuit;
      if (other) {
        hold(mixedLine, true);
        held = null;
      } else {
        holdStage(shown);
      }
      frame(mixedMatrix, mixed(before.fit, shown.fit, e), [mixed(before.fade[0], shown.fade[0], e), mixed(before.fade[1], shown.fade[1], e)]);
      gl!.drawArrays(gl!.POINTS, 0, DUST);
      road(roadFirst, marked, mixing?.road ?? 0, e);
      road(roadFirst + marked, before.road.length / 8, 0, 1 - e);
      const length = other ? mixedRoad.length : shown.circuit.line.length;
      wall(shown.circuit, shown.circuit.wall.t + (mixing?.wall ?? 0), mixed(before.wall, shown.wall, e), length, 1);
      const line = other ? mixedRoad : shown.circuit.line;
      const count = given(0, line, mixing?.road ?? 0, change.lights ? smooth(0.5, 1, p) : 1);
      lightsOn(ghosts(count, change.lights ? 1 - smooth(0, 0.3, p) : 0));
      return;
    }

    holdStage(shown);
    frame(shown.matrix, shown.fit, shown.fade);
    gl!.drawArrays(gl!.POINTS, 0, DUST);
    road(roadFirst, marked, 0, 1);
    wall(shown.circuit, shown.circuit.wall.t, shown.wall, shown.circuit.line.length, 1);
    // What a change that starts now would leave: the lights as they are.
    lit = given(0, shown.circuit.line, 0, 1);
    lightsOn(lit);
  }

  function project(t: number, offset: number, onRoad = false): [number, number] {
    if (!shown) return [0, 0];
    const [width, height] = size();
    const matrix = mixing ? mixedMatrix : shown.matrix;
    const [x, z] = mixing ? onLine(mixedRoad, t + (onRoad ? mixing.road : mixing.wall), offset) : onLine(shown.circuit.line, t, offset);
    const w = matrix[2 * 4 + 3] * z + matrix[3 * 4 + 3];
    const cx = matrix[0] * x + matrix[2 * 4] * z + matrix[3 * 4];
    const cy = matrix[1] * x + matrix[2 * 4 + 1] * z + matrix[3 * 4 + 1];
    return [((cx / w) * 0.5 + 0.5) * width, (0.5 - (cy / w) * 0.5) * height];
  }

  return {
    setCircuit,
    layout,
    draw,
    project,
    scale: () => shown?.wallScale ?? 0,
    moving() {
      if (progress() >= 1 || !change || !before || before.circuit === shown?.circuit) return null;
      return change.kind;
    },
    stop() {
      stopped = true;
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
