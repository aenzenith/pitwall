<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { burnRate, left, LOW_LEFT } from "../../lib/fuel";
import { visible } from "../../lib/store";
import type { UsageWindow } from "../../lib/types";

/**
 * The Fuel page's ground between its bars, drawn with WebGL: warm smoke and embers rising behind
 * the page, the smoke lit round each dial's pod (`[data-ring]` in the canvas's parent) and sparks
 * circling them. The glow
 * climbs as high as the session has fuel left, the embers and sparks thicken and quicken with its
 * burn rate; red when low, grey and still when stale. Drawn for the eye only; the dials say the
 * same. `readAt`: when `window` was read.
 */
const props = defineProps<{ window: UsageWindow | null; readAt: number; stale: boolean }>();

/** At most this many frames a second: smoke needs no more, the battery thanks us. */
const FPS = 30;
/** Pixels drawn per device pixel: half a retina pixel, never under one CSS pixel, keeps sparks crisp. */
const SCALE = 0.5;
/** How often the pods' places are read again while running, in ms. */
const MEASURE_MS = 500;
/** Pods lit at most: the cluster's three dials. */
const RINGS = 3;
/** The burn rate, in %/h, at which the embers are at their thickest and quickest. */
const HOT_RATE = 25;

const VERTEX = `
attribute vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }
`;

const FRAGMENT = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform vec2 uRes;
uniform float uTime;
uniform vec3 uBase;
uniform vec3 uTint;
uniform float uLevel;
uniform float uHeat;
// Each pod: its centre in canvas pixels (from the bottom left) and its radius; 0 when absent.
uniform vec3 uRings[${RINGS}];

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  mat2 m = mat2(1.6, 1.2, -1.2, 1.6);
  for (int i = 0; i < 5; i++) {
    v += a * noise(p);
    p = m * p;
    a *= 0.5;
  }
  return v;
}

// One layer of embers: a spark in some cells of a grid that drifts up, sways and flickers.
float embers(vec2 uv, float aspect, float scale, float speed, float seed) {
  vec2 p = vec2(uv.x * aspect, uv.y) * scale;
  p.y -= uTime * speed;
  p.x += sin(p.y * 0.9 + seed) * 0.25;
  vec2 id = floor(p);
  vec2 f = fract(p) - 0.5;
  float h = hash(id + seed);
  vec2 at = (vec2(hash(id + seed + 1.3), hash(id + seed + 7.1)) - 0.5) * 0.6;
  float size = 0.035 + 0.05 * hash(id + seed + 3.7);
  float flicker = 0.55 + 0.45 * sin(uTime * 2.0 + h * 40.0);
  float lit = step(0.82 - 0.3 * uHeat, h);
  float d = length(f - at);
  // A hot core in a soft halo.
  return lit * flicker * (smoothstep(size, 0.0, d) + 0.35 * exp(-d * d / (size * size * 6.0)));
}

// One layer of sparks circling a pod just outside its bezel, drawn like the rising embers (round, a
// hot core in a soft halo, the same sizes and flicker): n slots, some lit, the layer turning by
// turns of the circle per unit of time. Each spark also wanders a little ahead of and behind its
// slot's centre at a pace of its own, so no two keep step. v runs from the pod's centre; band is
// how far out, as a share of the radius.
float orbit(vec2 v, float band, float radius, float n, float turns, float seed) {
  float d = length(v);
  float s = (atan(v.y, v.x) / 6.2831853 + 0.5) * n - uTime * n * turns;
  float id = mod(floor(s), n);
  float h = hash(vec2(id, seed));
  float wander = 0.1 * sin(uTime * (0.12 + 0.36 * hash(vec2(id, seed + 5.3))) + h * 31.0);
  float reach = hash(vec2(id, seed + 9.1));
  float lane = 0.035 + 0.24 * reach * reach + 0.015 * sin(uTime * 0.4 + h * 17.0);
  float along = (fract(s) - 0.5 - wander) * 6.2831853 * d / n;
  float across = (band - lane) * radius;
  // The embers' sizes, in the embers' units: a share of the canvas's height.
  float size = (0.035 + 0.05 * hash(vec2(id, seed + 3.1))) * uRes.y / 16.0;
  float lit = step(0.6 - 0.3 * uHeat, hash(vec2(id, seed + 7.7)));
  float dist = length(vec2(along, across));
  float flicker = 0.55 + 0.45 * sin(uTime * 2.0 + h * 40.0);
  // Faded out at its slot's ends, so no edge cuts a spark's glow.
  float inSlot = smoothstep(0.0, 0.2, fract(s)) * smoothstep(1.0, 0.8, fract(s));
  return lit * flicker * inSlot * (smoothstep(size, 0.0, dist) + 0.35 * exp(-dist * dist / (size * size * 6.0)));
}

void main() {
  vec2 uv = gl_FragCoord.xy / uRes;
  float aspect = uRes.x / uRes.y;
  vec2 p = vec2(uv.x * aspect, uv.y) * 1.5;
  float t = uTime;

  // Smoke: noise folded into itself, drifting up.
  vec2 q = vec2(fbm(p + vec2(0.0, -t * 0.08)), fbm(p + vec2(5.2, 1.3) - t * 0.05));
  vec2 r = vec2(fbm(p + 3.5 * q + vec2(1.7, 9.2) + t * 0.07), fbm(p + 3.5 * q + vec2(8.3, 2.8) - t * 0.06));
  float f = fbm(p + 3.0 * r - vec2(0.0, t * 0.1));
  float smoke = smoothstep(0.4, 0.95, f);
  smoke *= smoke;
  float veins = pow(clamp(length(r) * 0.9, 0.0, 1.0), 5.0);

  // The glow from below reaches as high as the fuel left.
  float rise = exp(-uv.y * mix(7.0, 1.6, uLevel));
  // Round each pod: the smoke lit by it, a rim of light that breathes, sparks circling.
  float spill = 0.0;
  float rims = 0.0;
  float sparks = 0.0;
  for (int i = 0; i < ${RINGS}; i++) {
    vec3 ring = uRings[i];
    if (ring.z <= 0.0) continue;
    vec2 v = gl_FragCoord.xy - ring.xy;
    float d = length(v);
    spill += exp(-abs(d - ring.z) / ring.z * 3.5);
    rims += exp(-abs(d - ring.z) / (ring.z * 0.03));
    float band = (d - ring.z) / ring.z;
    if (band > 0.0 && band < 0.34) {
      // Each pod at a pace of its own, neighbours turning opposite ways; three layers at three speeds.
      float seed = float(i) * 17.0;
      float turns = 0.012 * (0.6 + 0.8 * hash(vec2(seed, 2.9))) * (mod(float(i), 2.0) < 0.5 ? 1.0 : -1.0);
      float layers = orbit(v, band, ring.z, 30.0, turns * 0.55, seed)
        + orbit(v, band, ring.z, 26.0, turns, seed + 101.0)
        + orbit(v, band, ring.z, 22.0, turns * 1.6, seed + 211.0);
      sparks += layers * (1.0 - band / 0.34);
    }
  }
  rims *= 0.75 + 0.25 * sin(t * 0.9);

  // Only the smoke is lit from below, never a flat haze: no band along the bottom.
  float k = smoke * (0.35 + 0.9 * rise) + veins * 0.9 * rise + spill * (0.1 + 0.9 * smoke) * 0.7;

  float e = embers(uv, aspect, 9.0, 0.28, 0.0) + embers(uv, aspect, 14.0, 0.42, 11.0) * 0.7 + embers(uv, aspect, 22.0, 0.6, 23.0) * 0.45;
  e *= smoothstep(1.0, 0.15, uv.y);

  vec3 col = uBase + uTint * k * 0.55;
  // The densest smoke and the embers burn whiter.
  col += vec3(1.0, 0.9, 0.75) * pow(smoke, 3.0) * rise * 0.08;
  col += mix(uTint, vec3(1.0, 0.92, 0.8), 0.45) * e * 1.1;
  col += uTint * rims * 0.22;
  col += mix(uTint, vec3(1.0, 0.92, 0.8), 0.45) * sparks * 1.1;

  // Darker towards the edges, so the panel's border stays calm.
  vec2 v = (uv - 0.5) * vec2(0.9, 1.1);
  col = mix(uBase, col, smoothstep(0.85, 0.2, length(v)));

  // A whisper of grain against banding in the dark.
  col += (hash(gl_FragCoord.xy + fract(t)) - 0.5) * (1.5 / 255.0);
  gl_FragColor = vec4(col, 1.0);
}
`;

const canvas = ref<HTMLCanvasElement | null>(null);
/** The first frame is up: the canvas fades in over the plain panel. */
const ready = ref(false);
const reducedMotion = ref(false);

const level = computed(() => (props.window ? left(props.window.used) : 100));
const low = computed(() => level.value <= LOW_LEFT);
const heat = computed(() => {
  const rate = props.window ? burnRate(props.window, props.readAt) : null;
  return Math.min(1, Math.max(0, (rate ?? 0) / HOT_RATE));
});
const running = computed(() => visible.value && !props.stale && !reducedMotion.value);

type Scene = {
  gl: WebGLRenderingContext;
  program: WebGLProgram;
  uniforms: Record<"res" | "time" | "base" | "tint" | "level" | "heat" | "rings", WebGLUniformLocation | null>;
};

let scene: Scene | null = null;
let frame = 0;
let lastAt = 0;
let lastDrawn = 0;
/** Scene time: it only moves while running, and faster when the session burns hot. */
let time = 0;
let observer: ResizeObserver | null = null;
let motionQuery: MediaQueryList | null = null;
/** The panel's colour and the smoke's, read from the tokens when they may have changed, not each frame. */
let base: [number, number, number] = [0, 0, 0];
let tint: [number, number, number] = [0, 0, 0];
/** The pods, as the shader takes them: x, y, radius each, in canvas pixels. */
const rings = new Float32Array(RINGS * 3);
let lastMeasured = 0;

/** A colour token as 0–1 RGB; the tokens are `#rrggbb`. */
function token(name: string): [number, number, number] {
  const hex = getComputedStyle(document.documentElement).getPropertyValue(name).trim().replace("#", "");
  const value = Number.parseInt(hex.length === 3 ? [...hex].map((digit) => digit + digit).join("") : hex, 16);
  if (Number.isNaN(value)) return [0, 0, 0];
  return [((value >> 16) & 255) / 255, ((value >> 8) & 255) / 255, (value & 255) / 255];
}

function compile(gl: WebGLRenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
  console.warn("fuel backdrop:", gl.getShaderInfoLog(shader));
  gl.deleteShader(shader);
  return null;
}

function setUp(target: HTMLCanvasElement): Scene | null {
  const gl = target.getContext("webgl", { alpha: false, antialias: false, depth: false, stencil: false, powerPreference: "low-power" });
  if (!gl) return null;
  const vertex = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fragment = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  const program = gl.createProgram();
  if (!vertex || !fragment || !program) return null;
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
  gl.useProgram(program);

  // One triangle over the whole canvas.
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, "aPos");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

  const at = (name: string) => gl.getUniformLocation(program, name);
  return {
    gl,
    program,
    uniforms: { res: at("uRes"), time: at("uTime"), base: at("uBase"), tint: at("uTint"), level: at("uLevel"), heat: at("uHeat"), rings: at("uRings") },
  };
}

function colour(): void {
  base = token("--bg-input");
  tint = token(props.stale ? "--idle-ring" : low.value ? "--crash" : "--claude");
}

/** Where the pods sit on the canvas; they move with the window and when dials come or go. */
function measure(): void {
  rings.fill(0);
  const target = canvas.value;
  const box = target?.getBoundingClientRect();
  if (!target || !box?.width) return;
  const ratio = target.width / box.width;
  const pods = target.parentElement?.querySelectorAll<HTMLElement>("[data-ring]") ?? [];
  [...pods].slice(0, RINGS).forEach((pod, index) => {
    const at = pod.getBoundingClientRect();
    rings.set([(at.left + at.width / 2 - box.left) * ratio, (box.bottom - at.top - at.height / 2) * ratio, (at.width / 2) * ratio], index * 3);
  });
}

function draw(): void {
  const target = canvas.value;
  if (!scene || !target) return;
  const { gl, uniforms } = scene;
  gl.viewport(0, 0, target.width, target.height);
  gl.uniform2f(uniforms.res, target.width, target.height);
  gl.uniform1f(uniforms.time, time);
  gl.uniform3fv(uniforms.base, base);
  gl.uniform3fv(uniforms.tint, tint);
  gl.uniform1f(uniforms.level, level.value / 100);
  gl.uniform1f(uniforms.heat, heat.value);
  gl.uniform3fv(uniforms.rings, rings);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  ready.value = true;
}

function tick(at: number): void {
  frame = requestAnimationFrame(tick);
  if (at - lastDrawn < 1000 / FPS - 2) return;
  const step = Math.min(0.1, (at - (lastAt || at)) / 1000);
  lastAt = at;
  lastDrawn = at;
  time += step * (0.6 + 1.2 * heat.value);
  if (at - lastMeasured > MEASURE_MS) {
    lastMeasured = at;
    measure();
  }
  draw();
}

function play(): void {
  stop();
  if (!scene) return;
  if (!running.value) {
    draw();
    return;
  }
  lastAt = 0;
  frame = requestAnimationFrame(tick);
}

function stop(): void {
  cancelAnimationFrame(frame);
  frame = 0;
}

function fit(): void {
  const target = canvas.value;
  if (!target) return;
  const device = Math.min(window.devicePixelRatio || 1, 2);
  const ratio = Math.min(device, Math.max(1, device * SCALE));
  target.width = Math.max(1, Math.round(target.clientWidth * ratio));
  target.height = Math.max(1, Math.round(target.clientHeight * ratio));
  measure();
  if (!frame) draw();
}

function onScroll(): void {
  measure();
  if (!frame) draw();
}

function onMotion(event: MediaQueryListEvent): void {
  reducedMotion.value = event.matches;
}

onMounted(() => {
  const target = canvas.value;
  if (!target) return;
  scene = setUp(target);
  if (!scene) return;
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion.value = motionQuery.matches;
  motionQuery.addEventListener("change", onMotion);
  target.addEventListener("webglcontextlost", (event) => {
    event.preventDefault();
    stop();
    scene = null;
    ready.value = false;
  });
  colour();
  observer = new ResizeObserver(fit);
  observer.observe(target);
  // The pods move when the page scrolls; scroll events don't bubble, a capturing listener sees them.
  target.parentElement?.addEventListener("scroll", onScroll, { capture: true, passive: true });
  fit();
  play();
});

// Running or still, and a still frame redrawn whenever what it shows changes.
watch(running, play);
watch(
  [level, low, heat, () => props.stale],
  () => {
    colour();
    measure();
    if (!frame) draw();
  },
  { flush: "post" },
);

onBeforeUnmount(() => {
  stop();
  observer?.disconnect();
  canvas.value?.parentElement?.removeEventListener("scroll", onScroll, { capture: true });
  motionQuery?.removeEventListener("change", onMotion);
  scene?.gl.getExtension("WEBGL_lose_context")?.loseContext();
  scene = null;
});
</script>

<template>
  <canvas ref="canvas" :class="['backdrop', { ready }]" aria-hidden="true"></canvas>
</template>

<style scoped>
.backdrop {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  opacity: 0;
  transition: opacity 1.2s ease;
}

.backdrop.ready {
  opacity: 1;
}

@media (prefers-reduced-motion: reduce) {
  .backdrop {
    transition: none;
  }
}
</style>
