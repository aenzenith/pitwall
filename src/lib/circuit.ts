// The Track page's circuit, drawn with WebGL as additive point sprites, after the website's night
// circuit (pitwall-website: resources/js/lib/circuit.ts). Lines and trails are points packed far
// closer than they are wide, so they read as strokes. Everything moves on the GPU: a point knows
// where along its light's trail it sits, the vertex shader looks the circuit's centre line up and
// places it. A frame costs a handful of uniforms.

import { LINE_SAMPLES, onLine, type Circuit } from "./track";

/** A light on the circuit for one frame. */
export type Light = {
  /** Where its head is, in laps (0–1 along the line). */
  head: number;
  /** How much of the line its trail covers (0–1). */
  trail: number;
  /** Its brightness: 0 hides it. */
  gain: number;
  /** World units to the left of the centre line. */
  offset: number;
  colour: readonly [number, number, number];
};

/** The room the page keeps round the circuit, in CSS pixels from the canvas's edges. */
export type Inset = { top: number; right: number; bottom: number; left: number };

export interface Renderer {
  /** The circuit drawn from now on. `lanes`: the straight's lanes (their offsets); `wall`: half
   * the pit wall's length, in world units. */
  setCircuit(circuit: Circuit, lanes: readonly number[], wall: number): void;
  /** Fits the circuit into the canvas, inside `inset`. Call when either changes. */
  layout(inset: Inset): void;
  draw(time: number, lights: readonly Light[], glow: boolean): void;
  /** Where a point of the circuit lands on the canvas, in CSS pixels. */
  project(t: number, offset: number): [number, number];
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
uniform float uTime;
uniform float uPx;
uniform float uGlow;
uniform float uClosed;
uniform vec2 uFade;
uniform vec2 uView;
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

void main() {
    float kind = aA.z;
    vec3 pos; vec3 col; float size; float alpha;
    if (kind < .5) {
        // A mark of the circuit: where it is, how big; aB = alpha, its place for the slow pulse
        // that runs round the edges (-1: none), how white.
        pos = vec3(aA.x, 0., aA.y);
        float pulse = aB.y < 0. ? 1. : .5 + .5 * sin(TAU * (aB.y * 6. - uTime * .05));
        col = mix(vec3(.50, .56, .66), vec3(.92), aB.z);
        size = aA.w;
        alpha = aB.x * (.5 + .5 * pulse);
    } else if (kind < 2.5) {
        vec4 light = uLightA[int(aB.x)];
        vec4 tint = uLightB[int(aB.x)];
        if (kind < 1.5) {
            // A trail: aA.x = how far behind the head, aA.y = across its width, aA.w = 1 for the
            // wide soft halo.
            float lag = aA.x;
            float halo = aA.w;
            float t = light.x - lag * light.y;
            vec2 p = onLine(t, light.w + aA.y * .0085);
            pos = vec3(p.x, 0., p.y);
            col = tint.rgb;
            size = mix(mix(6.4, 1.3, lag), mix(30., 12., lag), halo);
            // tint.a: fewer points to a unit of a longer trail, each gives that much more light.
            alpha = pow(1. - lag, 1.35) * mix(.2, .05, halo) * light.z * tint.a * mix(1., uGlow, halo);
            // An open line's trail starts at its start: nothing comes round from the far end.
            if (uClosed < .5 && t < 0.) alpha = 0.;
        } else {
            // The car: a bright head, and its glow (aB.y = 1).
            vec2 p = onLine(light.x, light.w);
            pos = vec3(p.x, .01, p.y);
            col = mix(tint.rgb, vec3(1.), .55);
            size = aA.w;
            alpha = aA.x * light.z * mix(1., uGlow, aB.y);
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

/** The circuit's own marks, eight floats a point (aA, aB): the road's edges and centre dashes, the
 * start line, the pit wall; on the straight, each lane's line and the two lines across them. */
function marks(circuit: Circuit, lanes: readonly number[], wall: number): number[] {
  const { line, half } = circuit;
  const data: number[] = [];
  const put = (point: readonly [number, number], size: number, alpha: number, pulse: number, white: number): void => {
    data.push(point[0], point[1], 0, size, alpha, pulse, white, 0);
  };

  if (line.closed) {
    const edge = Math.round(line.length * 330);
    for (let i = 0; i < edge; i++) {
      put(onLine(line, i / edge, -half), 2.2, 0.3, i / edge, 0);
      put(onLine(line, i / edge, half), 2.2, 0.3, i / edge, 0);
    }
    const centre = Math.round(line.length * 267);
    const dashes = Math.round(line.length * 9);
    for (let i = 0; i < centre; i++) {
      const dash = ((i / centre) * dashes) % 1;
      const alpha = 0.09 * smooth(0, 0.1, dash) * smooth(0.62, 0.52, dash);
      if (alpha > 0.004) put(onLine(line, i / centre, 0), 1.5, alpha, i / centre, 0);
    }
    for (let i = 0; i <= 80; i++) put(onLine(line, circuit.wall.t, half * (i / 40 - 1)), 3, 0.5, -1, 1);
    const along = Math.round(wall * 2 * 330);
    for (let i = 0; i <= along; i++) {
      put(onLine(line, circuit.wall.t + ((i / along) * 2 - 1) * (wall / line.length), circuit.wall.side * half * 2.3), 3.4, 0.6, -1, 0.5);
    }
  } else {
    const dots = Math.round(line.length * 120);
    const dashes = Math.round(line.length * 6);
    for (const lane of lanes) {
      for (let i = 0; i < dots; i++) {
        const dash = ((i / dots) * dashes) % 1;
        const alpha = 0.42 * smooth(0, 0.1, dash) * smooth(0.62, 0.52, dash);
        if (alpha > 0.004) put(onLine(line, i / dots, lane), 2, alpha, -1, 0);
      }
    }
    const from = Math.min(0, ...lanes) - 0.25;
    const to = Math.max(0, ...lanes) + 0.25;
    const across = Math.round((to - from) * 330);
    for (let i = 0; i <= across; i++) {
      const lane = from + ((to - from) * i) / across;
      put(onLine(line, 0, lane), 2.6, 0.34, -1, 0.4);
      put(onLine(line, 1, lane), 3.4, 0.6, -1, 0.5);
    }
  }
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

/** Null where WebGL isn't to be had: the page's lists work without the circuit. */
export function startCircuit(canvas: HTMLCanvasElement): Renderer | null {
  const gl = canvas.getContext("webgl", { antialias: false, alpha: true, premultipliedAlpha: true, depth: false, stencil: false, powerPreference: "low-power" });
  if (!gl) return null;

  // The centre line and the lights go to the shader as uniforms: a GPU with little room for
  // them gets a coarser line and fewer lights.
  const room = Number(gl.getParameter(gl.MAX_VERTEX_UNIFORM_VECTORS)) || 128;
  const lineCount = room >= 360 ? LINE_SAMPLES : 64;
  const lightCount = room >= 360 ? 32 : 12;

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
    time: at("uTime"),
    px: at("uPx"),
    glow: at("uGlow"),
    closed: at("uClosed"),
    fade: at("uFade"),
    view: at("uView"),
  };

  const steady = fixed(lightCount);
  const lightA = new Float32Array(lightCount * 4);
  const lightB = new Float32Array(lightCount * 4);
  const matrix = new Float32Array(16);

  let circuit: Circuit | null = null;
  /** The straight's lanes: they widen what the camera has to take in. */
  let offsets: readonly number[] = [];
  let wallHalf = 1;
  let markCount = 0;
  let inset: Inset = { top: 0, right: 0, bottom: 0, left: 0 };
  /** CSS pixels to a world unit at the wall (0 until laid out), and the fit's own scale. */
  let wallScale = 0;
  let fit = 1;
  let fade: [number, number] = [100, 1];
  let stopped = false;

  /** The camera's view of a ground point, before the fit: clip x, y and w. */
  function view(x: number, z: number, aspect: number): [number, number, number] {
    const { height, distance, target } = circuit!.camera;
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

  function layout(next: Inset): void {
    inset = next;
    if (!circuit) return;
    const [width, height] = size();
    const aspect = width / height;
    const { line, half } = circuit;

    // Everything the frame has to hold: the road's outer edges and the pit wall, or the lanes.
    let left = Infinity;
    let right = -Infinity;
    let top = -Infinity;
    let bottom = Infinity;
    let near = Infinity;
    let far = -Infinity;
    const reach = line.closed ? [half * 2.4, -half * 2.4] : [Math.min(0, ...offsets) - 0.3, Math.max(0, ...offsets) + 0.3];
    for (let i = 0; i <= 96; i++) {
      for (const offset of reach) {
        const [x, z] = onLine(line, i / 96, offset);
        const [cx, cy, w] = view(x, z, aspect);
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
    fit = Math.min(roomX / (((right - left) * width) / 2), roomY / (((top - bottom) * height) / 2), 2.4);
    const centreX = ((inset.left + roomX / 2) / width) * 2 - 1;
    const centreY = 1 - ((inset.top + roomY / 2) / height) * 2;
    const moveX = centreX - (fit * (left + right)) / 2;
    const moveY = centreY - (fit * (top + bottom)) / 2;

    // Projection × view for a camera on the z axis looking down at the circuit, then the fit.
    const { height: up, distance, target } = circuit.camera;
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
      matrix[c * 4] = fit * rows[0][c] + moveX * rows[3][c];
      matrix[c * 4 + 1] = fit * rows[1][c] + moveY * rows[3][c];
      matrix[c * 4 + 2] = rows[2][c];
      matrix[c * 4 + 3] = rows[3][c];
    }

    const span = Math.max(0.5, far - near);
    fade = [far + 1.6 * span, 2.6 * span];
    const [wx, wz] = onLine(line, circuit.wall.t, 0);
    wallScale = (fit * FOCAL * height) / 2 / view(wx, wz, aspect)[2];
  }

  function setCircuit(next: Circuit, lanes: readonly number[], wall: number): void {
    circuit = next;
    offsets = lanes;
    wallHalf = wall;
    const drawn = marks(next, lanes, wallHalf);
    markCount = drawn.length / 8 + DUST;
    gl!.bufferData(gl!.ARRAY_BUFFER, new Float32Array(drawn.concat(steady)), gl!.STATIC_DRAW);

    // The centre line, thinned to what the shader holds.
    const line = new Float32Array(lineCount * 4);
    for (let i = 0; i < lineCount; i++) {
      const k = next.line.closed ? Math.round((i * LINE_SAMPLES) / lineCount) % LINE_SAMPLES : Math.round((i * (LINE_SAMPLES - 1)) / (lineCount - 1));
      line.set([next.line.x[k], next.line.z[k], next.line.nx[k], next.line.nz[k]], i * 4);
    }
    gl!.uniform4fv(uniforms.line, line);
    gl!.uniform1f(uniforms.closed, next.line.closed ? 1 : 0);
    layout(inset);
  }

  function draw(time: number, lights: readonly Light[], glow: boolean): void {
    if (stopped || !circuit) return;
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

    const count = Math.min(lights.length, lightCount);
    for (let i = 0; i < count; i++) {
      const light = lights[i];
      lightA.set([light.head, light.trail, light.gain, light.offset], i * 4);
      // A trail's points lie this much further apart than the ones the alpha was set for.
      lightB.set([light.colour[0], light.colour[1], light.colour[2], Math.max(0.05, (light.trail * circuit.line.length) / 3.5)], i * 4);
    }

    gl!.uniformMatrix4fv(uniforms.vp, false, matrix);
    gl!.uniform4fv(uniforms.lightA, lightA);
    gl!.uniform4fv(uniforms.lightB, lightB);
    gl!.uniform1f(uniforms.time, time);
    gl!.uniform1f(uniforms.px, (SPRITE * fit * FOCAL * bufferHeight) / 2);
    gl!.uniform1f(uniforms.glow, glow ? 1 : 0);
    gl!.uniform2f(uniforms.fade, fade[0], fade[1]);
    gl!.uniform2f(uniforms.view, bufferWidth, bufferHeight);
    gl!.clear(gl!.COLOR_BUFFER_BIT);
    gl!.drawArrays(gl!.POINTS, 0, markCount);
    if (count) gl!.drawArrays(gl!.POINTS, markCount, count * PER_LIGHT);
  }

  function project(t: number, offset: number): [number, number] {
    if (!circuit) return [0, 0];
    const [width, height] = size();
    const [x, z] = onLine(circuit.line, t, offset);
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
    scale: () => wallScale,
    stop() {
      stopped = true;
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
