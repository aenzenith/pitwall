/**
 * The road behind the first run's steps: light trails racing along a straight, seen from beside
 * it, drawn in WebGL as additive point sprites. Green trails are running dev servers, orange ones
 * Claude sessions. The site's download section draws the same road (pitwall-website,
 * `resources/js/lib/circuit.ts`, its `straight` course): the shaders, the cars and the camera are
 * that file's, the closed lap left out. Everything moves on the GPU: a point knows where along
 * the road it sits and the vertex shader places it.
 */

export interface Road {
  /** Off screen or covered, nothing is drawn. Each call draws one frame, so `false` also serves as "redraw". */
  setRunning(on: boolean): void;
  stop(): void;
}

const VERTEX = `
attribute vec4 aA;
attribute vec4 aB;
uniform mat4 uVP;
uniform float uTime;
uniform float uPx;
uniform float uIntro;
uniform float uGain;
uniform float uFit;
uniform float uHalf;
uniform vec2 uView;
varying vec4 vCol;
const float TAU = 6.28318530718;
// How long the straight is: most of it lies out of the frame at either end, so a car that leaves
// by the far end is not seen coming back in at the near one.
const float STRAIGHT = 24.;
// Where the distance has taken all the light.
const float FAR = 16.;
// The site's lap is 15.73 long: along the straight the same points lie further apart, and each
// gives that much more light.
const float SPARSE = STRAIGHT / 15.73;

vec3 onTrack(float t, float lane) {
    return vec3((t - .3) * STRAIGHT, 0., lane * uHalf);
}

void main() {
    float kind = aA.z;
    vec3 pos; vec3 col; float size; float alpha;
    vec3 green = vec3(.45, .79, .57);
    vec3 orange = vec3(.94, .53, .24);
    if (kind < .5) {
        // Track marks: both edges, each one line, and the centre dashes. A slow pulse runs along them.
        pos = onTrack(aA.x, aA.y);
        float edge = step(.5, abs(aA.y));
        float pulse = .5 + .5 * sin(TAU * (aA.x * 6. - uTime * .05));
        float dash = fract(aA.x * 140.);
        col = vec3(.50, .56, .66);
        size = mix(1.6, 2.4, edge);
        alpha = mix(.07 * smoothstep(0., .1, dash) * smoothstep(.62, .52, dash), .23, edge) * (.5 + .5 * pulse) * SPARSE;
    } else if (kind < 1.5) {
        // A trail: aA.x = its length, aA.w = 1 for the wide soft halo; aB = phase, speed, lag, hue.
        float head = aB.x + uTime * aB.y;
        float lag = aB.z;
        pos = onTrack(fract(head - lag * aA.x * uIntro), aA.y);
        col = mix(green, orange, aB.w);
        float halo = step(.5, aA.w);
        size = mix(mix(6.4, 1.3, lag), mix(30., 12., lag), halo);
        alpha = pow(1. - lag, 1.35) * mix(.092, .05, halo) * uIntro * SPARSE;
    } else if (kind < 2.5) {
        // The car: a bright head.
        float head = aB.x + uTime * aB.y;
        pos = onTrack(fract(head), aA.y);
        pos.y += .01;
        col = mix(mix(green, orange, aB.w), vec3(1.), .55);
        size = aA.w;
        alpha = aA.x * uIntro;
    } else {
        // Dust drifting above the road.
        pos = vec3(aA.x, fract(aA.y + uTime * .006 * (.4 + aA.w)) * 2.6, aB.x);
        col = vec3(.62, .68, .78);
        size = .9 + aA.w;
        alpha = .10 + .14 * aA.w;
    }
    gl_Position = uVP * vec4(pos, 1.);
    float w = max(gl_Position.w, .001);
    alpha *= clamp((FAR * uFit - w) / (7. * uFit), 0., 1.) * uGain;
    // Dust right in front of the lens would be a blurred disc: it thins out there.
    if (kind > 2.5) alpha *= smoothstep(2.5, 5., w);
    // Nothing is drawn finer than four pixels across, where a sprite would shimmer: a smaller one
    // keeps its light and spreads it.
    float px = size * uPx * 11. / w;
    alpha *= min(1., px * px / 16.);
    px = max(px, 4.);
    // A sprite is dropped whole once its centre leaves the frame: it dims as it comes to the edge,
    // so a wide glow goes out there rather than vanishing.
    vec2 room = (1. - abs(gl_Position.xy / w)) * uView * .5;
    alpha *= clamp(min(room.x, room.y) / (px * .5), 0., 1.);
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

/** Cars: phase, laps per second, lane, hue (0 = a running server, 1 = Claude), trail length. */
const CARS: ReadonlyArray<readonly [number, number, number, number, number]> = [
  [0.02, 0.034, -0.55, 0, 0.26],
  [0.2, 0.027, 0.45, 1, 0.2],
  [0.33, 0.041, 0.05, 0, 0.31],
  [0.47, 0.03, -0.35, 0, 0.22],
  [0.58, 0.037, 0.6, 1, 0.28],
  [0.71, 0.025, -0.6, 0, 0.19],
  [0.83, 0.044, 0.25, 1, 0.33],
  [0.93, 0.032, -0.1, 0, 0.23],
];

/** The site's download section: the camera beside the road, low, and the light a little down. */
const RADIUS = 5.0;
const HEIGHT = 2.6;
const GAIN = 0.8;

/** The lens: half the vertical field of view is 0.36 rad. */
const FOCAL = 1 / Math.tan(0.36);

/** The intro, as the site's: the trails grow over 2.8 s, the cars race in from 11 s back over 3.4 s. */
const GROW = 2.8;
const ARRIVE = 3.4;

function compile(gl: WebGLRenderingContext, type: number, source: string): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  return gl.getShaderParameter(shader, gl.COMPILE_STATUS) ? shader : null;
}

/**
 * Every point as eight floats (aA, aB). A fixed seed: the same race every time. Lines and trails
 * are points packed far closer than they are wide, so they read as strokes, not as beads; their
 * alpha in the shader is that much lower.
 */
function points(): Float32Array {
  let seed = 7;
  const rnd = (): number => {
    seed = (seed * 16807) % 2147483647;
    return (seed - 1) / 2147483646;
  };
  const data: number[] = [];
  const put = (...values: number[]): void => {
    data.push(...values);
  };

  for (let i = 0; i < 5200; i++) {
    put(i / 5200, -1, 0, 0, 0, 0, 0, 0);
    put(i / 5200, 1, 0, 0, 0, 0, 0, 0);
  }
  for (let i = 0; i < 4200; i++) put(i / 4200, 0, 0, 0, 0, 0, 0, 0);
  for (const [phase, speed, lane, hue, length] of CARS) {
    const n = 3200;
    // Across its width a trail is filled evenly (golden-ratio steps), not at random: no clumps.
    for (let i = 0; i < n; i++) put(length, lane + (((i * 0.6180339887) % 1) - 0.5) * 0.05, 1, 0, phase, speed, i / n, hue);
    for (let i = 0; i < 70; i++) put(length, lane, 1, 1, phase, speed, i / 70, hue);
    put(1.0, lane, 2, 9, phase, speed, 0, hue);
    put(0.22, lane, 2, 46, phase, speed, 0, hue);
  }
  for (let i = 0; i < 520; i++) put((rnd() - 0.5) * 12, rnd(), 3, rnd(), (rnd() - 0.5) * 9, 0, 0, 0);

  return new Float32Array(data);
}

/**
 * Starts the road on `canvas`; null where WebGL isn't there (the steps show without it).
 * `intro`: the trails grow and the cars race in, as on the site; without it (and with reduced
 * motion) the road is there at once. `still`: one frame and no more, nothing moving.
 */
export function startRoad(canvas: HTMLCanvasElement, { intro = false, still = false } = {}): Road | null {
  const gl = canvas.getContext("webgl", { antialias: false, alpha: true, premultipliedAlpha: true });
  if (!gl) return null;

  const vertex = compile(gl, gl.VERTEX_SHADER, VERTEX);
  const fragment = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
  const program = gl.createProgram();
  if (!vertex || !fragment || !program) return null;
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.bindAttribLocation(program, 0, "aA");
  gl.bindAttribLocation(program, 1, "aB");
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
  gl.useProgram(program);

  const data = points();
  const count = data.length / 8;
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 4, gl.FLOAT, false, 32, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 32, 16);
  gl.disable(gl.DEPTH_TEST);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE);
  gl.clearColor(0, 0, 0, 0);

  const uVP = gl.getUniformLocation(program, "uVP");
  const uTime = gl.getUniformLocation(program, "uTime");
  const uPx = gl.getUniformLocation(program, "uPx");
  const uIntro = gl.getUniformLocation(program, "uIntro");
  const uFit = gl.getUniformLocation(program, "uFit");
  const uHalf = gl.getUniformLocation(program, "uHalf");
  const uView = gl.getUniformLocation(program, "uView");
  gl.uniform1f(gl.getUniformLocation(program, "uGain"), GAIN);

  const matrix = new Float32Array(16);

  /** Projection × view, looking from a point on a circle round the road's centre down at it. */
  function viewProjection(aspect: number, angle: number, fit: number): Float32Array {
    const f = FOCAL;
    const near = 0.1;
    const far = 120;
    const ex = Math.sin(angle) * RADIUS * fit;
    const ey = HEIGHT * fit;
    const ez = Math.cos(angle) * RADIUS * fit;
    let zx = ex;
    let zy = ey;
    let zz = ez;
    const zl = Math.hypot(zx, zy, zz);
    zx /= zl;
    zy /= zl;
    zz /= zl;
    let xx = zz;
    let xz = -zx;
    const xl = Math.hypot(xx, xz);
    xx /= xl;
    xz /= xl;
    const yx = zy * xz;
    const yy = zz * xx - zx * xz;
    const yz = -zy * xx;
    const view = [xx, yx, zx, 0, 0, yy, zy, 0, xz, yz, zz, 0, -(xx * ex + xz * ez), -(yx * ex + yy * ey + yz * ez), -(zx * ex + zy * ey + zz * ez), 1];
    const proj = [f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (far + near) / (near - far), -1, 0, 0, (2 * far * near) / (near - far), 0];
    for (let c = 0; c < 4; c++) {
      for (let r = 0; r < 4; r++) {
        matrix[c * 4 + r] = proj[r] * view[c * 4] + proj[4 + r] * view[c * 4 + 1] + proj[8 + r] * view[c * 4 + 2] + proj[12 + r] * view[c * 4 + 3];
      }
    }
    return matrix;
  }

  /** Seconds of racing, and of the intro (none: it is over). */
  let clock = 14;
  let since = intro && !still ? 0 : ARRIVE;
  let last = 0;
  let frame = 0;
  let running = !still;
  let stopped = false;

  function draw(now: number): void {
    frame = 0;
    if (stopped) return;
    // Two buffer pixels to a CSS pixel on every screen (a 1x display gets the frame supersampled,
    // which is what keeps a hairline smooth there), less on a canvas too wide for that.
    const dpr = Math.min(2, 4096 / Math.max(1, canvas.clientWidth));
    const width = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const height = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    gl!.viewport(0, 0, width, height);

    // A long gap (a hidden window, a pause) is not raced through.
    const step = last ? Math.min((now - last) / 1000, 0.1) : 0;
    last = now;
    clock += step;
    since += step;
    // The site tweens these with GSAP (power2.out, power3.out): the same curves, by hand.
    const grow = Math.min(1, since / GROW);
    const arrive = Math.min(1, since / ARRIVE);
    const shown = 1 - (1 - grow) ** 3;
    const boost = -11 * (1 - arrive) ** 4;

    // The road is watched from beside it, turned so it makes for a point past the right edge (2.2
    // half-widths from the centre) whatever the canvas's shape: it always crosses the whole frame.
    // The narrower the canvas, the more the camera looks along the road and the less of it it
    // sees: it backs off to keep the same stretch in the frame.
    const aspect = width / height;
    const sway = Math.sin(clock * 0.06);
    const reach = Math.hypot(RADIUS, HEIGHT);
    const tilt = HEIGHT / reach;
    let angle = -Math.atan((FOCAL * reach) / (2.2 * aspect * RADIUS));
    const fit = -2 * Math.sin(angle);
    // The road is as wide as suits the canvas, and what runs on it is drawn to that scale.
    const across = Math.min(0.29 * canvas.clientHeight, 0.34 * canvas.clientWidth);
    gl!.uniform1f(uHalf, (across * fit * reach * Math.hypot(Math.cos(angle), Math.sin(angle) * tilt)) / (Math.max(1, canvas.clientHeight) * FOCAL * tilt));
    const px = 1.25 * (across / 205) * fit;
    angle += 0.03 * sway;

    gl!.uniformMatrix4fv(uVP, false, viewProjection(aspect, angle, fit));
    gl!.uniform1f(uTime, clock + boost);
    gl!.uniform1f(uPx, dpr * px);
    gl!.uniform1f(uFit, fit);
    gl!.uniform2f(uView, width, height);
    gl!.uniform1f(uIntro, Math.max(0.0001, shown));
    gl!.clear(gl!.COLOR_BUFFER_BIT);
    gl!.drawArrays(gl!.POINTS, 0, count);

    if (running) frame = requestAnimationFrame(draw);
  }

  function schedule(): void {
    if (!frame && !stopped) frame = requestAnimationFrame(draw);
  }

  const lost = (event: Event): void => {
    event.preventDefault();
    stopped = true;
  };
  canvas.addEventListener("webglcontextlost", lost);
  schedule();

  return {
    setRunning(on) {
      running = on && !still;
      last = 0;
      // One frame either way: a paused road is still redrawn at its new size.
      schedule();
    },
    stop() {
      stopped = true;
      cancelAnimationFrame(frame);
      canvas.removeEventListener("webglcontextlost", lost);
      // The context goes with the canvas at once, not whenever the page lets go of it.
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    },
  };
}
