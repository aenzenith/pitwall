<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { BOLD, ITALIC, UNDERLINE } from "../../lib/highlight";
import { t } from "../../lib/i18n";
import type { GutterMark } from "../../lib/unifiedDiff";

/**
 * A file's text, read only: line numbers, Git's marks in the gutter, the syntax colours once
 * they come (`tokens`: per line, [length, palette index, font style] triples), a line picked by
 * a click (the editor opens there), ⌘F to look through it and a map of the whole on the right.
 * Lines are drawn in blocks of `CHUNK` as they come near the view, and stay.
 */
const props = defineProps<{ text: string; palette: string[]; tokens: number[][] | null; gutter: Map<number, GutterMark>; current: number | null }>();
const emit = defineEmits<{ line: [line: number] }>();

const LH = 19;
const CHUNK = 200;

const lines = computed(() => props.text.split("\n").map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line)));
const chunks = computed(() => Array.from({ length: Math.ceil(lines.value.length / CHUNK) }, (_, i) => i));
const digits = computed(() => String(lines.value.length).length);
const shown = reactive(new Set<number>([0]));
const scroller = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | null = null;

function observe(): void {
  observer?.disconnect();
  shown.clear();
  shown.add(0);
  void nextTick(() => {
    if (!scroller.value) return;
    observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) if (entry.isIntersecting) shown.add(Number((entry.target as HTMLElement).dataset.chunk));
      },
      { root: scroller.value, rootMargin: "1200px 0px" },
    );
    scroller.value.querySelectorAll(".chunk").forEach((chunk) => observer!.observe(chunk));
  });
}

watch(
  () => props.text,
  () => {
    observe();
    scroller.value?.scrollTo({ top: 0, left: 0 });
    closeFind();
  },
);
onMounted(observe);
onBeforeUnmount(() => observer?.disconnect());

const lineRange = (chunk: number) => Array.from({ length: Math.min(CHUNK, lines.value.length - chunk * CHUNK) }, (_, i) => chunk * CHUNK + i);

/* ---------- colours, and what the search found, cut into one line's pieces ---------- */

const styles = new Map<string, string>();
function styleOf(colour: string, font: number): string {
  const key = `${colour}|${font}`;
  let style = styles.get(key);
  if (style === undefined) {
    style = `${colour ? `color:${colour};` : ""}${font & ITALIC ? "font-style:italic;" : ""}${font & BOLD ? "font-weight:600;" : ""}${font & UNDERLINE ? "text-decoration:underline;" : ""}`;
    styles.set(key, style);
  }
  return style;
}

type Piece = { text: string; style: string; hit: 0 | 1 | 2 };

function pieces(n: number): Piece[] {
  const line = lines.value[n];
  const spans: Array<{ start: number; end: number; style: string }> = [];
  const tokens = props.tokens?.[n];
  let at = 0;
  if (tokens) {
    for (let i = 0; i < tokens.length && at < line.length; i += 3) {
      const end = Math.min(at + tokens[i], line.length);
      spans.push({ start: at, end, style: styleOf(props.palette[tokens[i + 1]] ?? "", tokens[i + 2]) });
      at = end;
    }
  }
  if (at < line.length || !spans.length) spans.push({ start: at, end: line.length, style: "" });

  const found = matchesOn.value.get(n);
  if (!found) return spans.map((span) => ({ text: line.slice(span.start, span.end), style: span.style, hit: 0 }));
  const cuts = new Set<number>([0, line.length]);
  for (const span of spans) cuts.add(span.start).add(span.end);
  for (const match of found) cuts.add(match.start).add(match.end);
  const points = [...cuts].sort((a, b) => a - b);
  const out: Piece[] = [];
  for (let i = 0; i < points.length - 1; i++) {
    const [start, end] = [points[i], points[i + 1]];
    if (start === end) continue;
    const span = spans.find((s) => start >= s.start && start < s.end);
    const match = found.find((m) => start >= m.start && start < m.end);
    out.push({ text: line.slice(start, end), style: span?.style ?? "", hit: match ? (match.current ? 2 : 1) : 0 });
  }
  return out;
}

/* ---------- ⌘F ---------- */

const finding = ref(false);
const needle = ref("");
const at = ref(0);
const findInput = ref<HTMLInputElement | null>(null);
const LIMIT = 10_000;

const matches = computed(() => {
  const out: Array<{ line: number; start: number; end: number }> = [];
  if (!needle.value) return out;
  const pattern = new RegExp(needle.value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
  lines.value.forEach((line, n) => {
    if (out.length >= LIMIT) return;
    for (const hit of line.matchAll(pattern)) {
      out.push({ line: n, start: hit.index, end: hit.index + hit[0].length });
      if (out.length >= LIMIT) break;
    }
  });
  return out;
});

const matchesOn = computed(() => {
  const byLine = new Map<number, Array<{ start: number; end: number; current: boolean }>>();
  matches.value.forEach((match, i) => {
    const list = byLine.get(match.line) ?? [];
    list.push({ start: match.start, end: match.end, current: i === at.value });
    byLine.set(match.line, list);
  });
  return byLine;
});

watch(needle, () => {
  at.value = 0;
  if (matches.value.length) reveal(matches.value[0].line);
});

function openFind(): void {
  finding.value = true;
  const picked = window.getSelection()?.toString();
  if (picked && !picked.includes("\n")) needle.value = picked;
  void nextTick(() => {
    findInput.value?.focus();
    findInput.value?.select();
  });
}

function closeFind(): void {
  finding.value = false;
  needle.value = "";
}

function step(by: number): void {
  const count = matches.value.length;
  if (!count) return;
  at.value = (at.value + by + count) % count;
  reveal(matches.value[at.value].line);
}

function onFindKey(event: KeyboardEvent): void {
  if (event.key === "Enter") step(event.shiftKey ? -1 : 1);
  else if (event.key === "Escape") {
    closeFind();
    scroller.value?.focus();
  } else return;
  event.preventDefault();
}

/** Line `n` (from 0) in view, a third of the way down. */
function reveal(n: number): void {
  const box = scroller.value;
  if (!box) return;
  shown.add(Math.floor(n / CHUNK));
  const top = n * LH;
  if (top < box.scrollTop || top > box.scrollTop + box.clientHeight - LH * 2) box.scrollTop = Math.max(0, top - box.clientHeight / 3);
}

/* ---------- ⌘A then ⌘C: the whole text, drawn or not ---------- */

let allPicked = false;

function onKey(event: KeyboardEvent): void {
  const mod = event.metaKey || event.ctrlKey;
  if (mod && event.code === "KeyF") {
    event.preventDefault();
    openFind();
  } else if (mod && event.code === "KeyG") {
    event.preventDefault();
    step(event.shiftKey ? -1 : 1);
  } else if (mod && event.code === "KeyA" && scroller.value && !(event.target as HTMLElement).closest("input, textarea")) {
    // The file's text; in the find box, ⌘A takes the box's own text as anywhere else.
    event.preventDefault();
    const range = document.createRange();
    range.selectNodeContents(scroller.value.querySelector(".lines")!);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    allPicked = true;
  } else if (event.key === "Escape" && finding.value) {
    closeFind();
  }
}

function onCopy(event: ClipboardEvent): void {
  if (!allPicked || !event.clipboardData) return;
  event.clipboardData.setData("text/plain", props.text);
  event.preventDefault();
}

/* ---------- the map ---------- */

const map = ref<HTMLCanvasElement | null>(null);
const view = reactive({ top: 0, height: 0, scale: 2 });
let resize: ResizeObserver | null = null;

function draw(): void {
  const canvas = map.value;
  if (!canvas) return;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  const g = canvas.getContext("2d");
  if (!g) return;
  g.scale(ratio, ratio);
  g.clearRect(0, 0, width, height);

  const count = lines.value.length;
  const scale = Math.min(2, height / Math.max(count, 1));
  view.scale = scale;
  const stride = Math.max(1, Math.ceil(count / (height * 2)));
  const plain = getComputedStyle(canvas).color;
  g.globalAlpha = 0.6;
  for (let n = 0; n < count; n += stride) {
    const line = lines.value[n];
    const y = n * scale;
    const h = Math.max(scale * 0.75, 0.6);
    const tokens = props.tokens?.[n];
    let col = 0;
    const paint = (text: string, colour: string): void => {
      const lead = text.length - text.trimStart().length;
      const body = text.trim().length;
      if (body) {
        g.fillStyle = colour || plain;
        g.fillRect(4 + (col + lead) * 0.8, y, Math.min(body * 0.8, width - 6), h);
      }
      col += text.length;
    };
    if (tokens) for (let i = 0; i < tokens.length && col < 70; i += 3) paint(line.slice(col, col + tokens[i]), props.palette[tokens[i + 1]] ?? "");
    else paint(line.slice(0, 70), "");
  }
  // Git's marks along the left edge.
  g.globalAlpha = 0.9;
  for (const [n, mark] of props.gutter) {
    g.fillStyle = getComputedStyle(canvas).getPropertyValue(`--g-${mark}`) || plain;
    g.fillRect(0, (n - 1) * scale, 2, Math.max(scale, 2));
  }
  onScroll();
}

function onScroll(): void {
  const box = scroller.value;
  if (!box) return;
  view.top = (box.scrollTop / LH) * view.scale;
  view.height = Math.max((box.clientHeight / LH) * view.scale, 8);
}

let dragging = false;
function mapTo(event: PointerEvent): void {
  const box = scroller.value;
  const canvas = map.value;
  if (!box || !canvas) return;
  const y = event.clientY - canvas.getBoundingClientRect().top;
  const line = y / view.scale;
  box.scrollTop = Math.max(0, line * LH - box.clientHeight / 2);
}

function mapDown(event: PointerEvent): void {
  dragging = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  mapTo(event);
}

watch(() => [props.text, props.tokens, props.gutter], () => void nextTick(draw));
onMounted(() => {
  draw();
  resize = new ResizeObserver(() => draw());
  if (map.value) resize.observe(map.value);
});
onBeforeUnmount(() => resize?.disconnect());

/** The line `n` (from 1) in view and picked, as from a click. */
function goTo(n: number): void {
  reveal(n - 1);
  emit("line", n);
}

defineExpose({ openFind, goTo });
</script>

<template>
  <div class="code-view" @keydown="onKey">
    <div ref="scroller" class="scroller" tabindex="0" @scroll.passive="onScroll" @mousedown="allPicked = false" @copy="onCopy">
      <div class="lines selectable" :style="{ '--digits': digits }">
        <div v-for="chunk in chunks" :key="chunk" class="chunk" :data-chunk="chunk" :style="{ minHeight: `${lineRange(chunk).length * LH}px` }">
          <template v-if="shown.has(chunk)">
            <div v-for="n in lineRange(chunk)" :key="n" :class="['ln', { cur: current === n + 1 }]" @click="emit('line', n + 1)">
              <span class="gut" aria-hidden="true"><span class="num">{{ n + 1 }}</span><span :class="['gm', gutter.get(n + 1)]"></span></span>
              <span class="code"><span v-for="(piece, k) in pieces(n)" :key="k" :style="piece.style" :class="{ hit: piece.hit === 1, here: piece.hit === 2 }">{{ piece.text }}</span></span>
            </div>
          </template>
        </div>
      </div>
    </div>

    <div class="minimap" aria-hidden="true" @pointerdown="mapDown" @pointermove="dragging && mapTo($event)" @pointerup="dragging = false" @pointercancel="dragging = false">
      <canvas ref="map"></canvas>
      <span class="slider" :style="{ top: `${view.top}px`, height: `${view.height}px` }"></span>
    </div>

    <div v-if="finding" class="find" role="search">
      <input ref="findInput" v-model="needle" type="search" :placeholder="t('files.find')" :aria-label="t('files.find')" @keydown="onFindKey" />
      <span class="count">{{ needle ? (matches.length ? t("files.findCount", { current: at + 1, total: matches.length }) : t("files.findNone")) : "" }}</span>
      <button type="button" class="icon" :disabled="!matches.length" :aria-label="t('output.searchPrevious')" :title="t('output.searchPrevious')" @click="step(-1)"><Icon name="chevron-down" :size="13" class="up" /></button>
      <button type="button" class="icon" :disabled="!matches.length" :aria-label="t('output.searchNext')" :title="t('output.searchNext')" @click="step(1)"><Icon name="chevron-down" :size="13" /></button>
      <button type="button" class="icon" :aria-label="t('output.searchClose')" :title="t('output.searchClose')" @click="closeFind"><Icon name="close" :size="12" /></button>
    </div>
  </div>
</template>

<style scoped src="../detail/detail.css"></style>
<style scoped>
.code-view {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  background: var(--bg-log);
  /* Git's colours in the gutter and the map. */
  --g-add: var(--run);
  --g-mod: var(--warn);
  --g-del: var(--crash);
}

.scroller {
  flex: 1;
  min-width: 0;
  overflow: auto;
  outline: none;
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 19px;
}

.lines {
  width: max-content;
  min-width: 100%;
  padding: 4px 0 40px;
}

.ln {
  display: flex;
  height: 19px;
  cursor: text;
}

.ln.cur,
.ln.cur .gut {
  background: var(--bg-code-line);
}

/* The numbers and marks stay as the text scrolls sideways, and are no part of a copy. */
.gut {
  position: sticky;
  left: 0;
  z-index: 1;
  display: flex;
  flex-shrink: 0;
  background: var(--bg-log);
  -webkit-user-select: none;
  user-select: none;
}

.num {
  width: calc(var(--digits) * 1ch + 22px);
  padding-right: 10px;
  box-sizing: border-box;
  text-align: right;
  color: var(--text-faint);
  opacity: 0.75;
}

.ln.cur .num {
  color: var(--text-soft);
  opacity: 1;
}

.gm {
  width: 3px;
  margin-right: 12px;
}

.gm.add {
  background: var(--g-add);
}

.gm.mod {
  background: var(--g-mod);
}

/* Lines removed just above: a small wedge at the line's top. */
.gm.del {
  background: linear-gradient(to bottom, var(--g-del) 0 4px, transparent 4px);
}

.code {
  white-space: pre;
  padding-right: 24px;
  color: var(--text);
}

.hit {
  border-radius: 2px;
  background: var(--bg-code-hit);
}

.here {
  border-radius: 2px;
  background: var(--bg-code-here);
  box-shadow: 0 0 0 1px var(--warn);
}

.minimap {
  position: relative;
  width: 64px;
  flex-shrink: 0;
  border-left: 1px solid var(--line-soft);
  color: var(--text-subtle);
  cursor: pointer;
}

.minimap canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

.slider {
  position: absolute;
  left: 0;
  right: 0;
  background: var(--bg-code-slider);
  pointer-events: none;
}

.find {
  position: absolute;
  top: 8px;
  right: 76px;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 2px;
  height: 30px;
  padding: 0 4px 0 8px;
  border-radius: 8px;
  background: var(--bg-control);
  border: 1px solid var(--line-dialog);
  box-shadow: var(--shadow-tip);
}

.find input {
  width: 160px;
  border: 0;
  padding: 0;
  background: transparent;
  font: inherit;
  font-size: 12px;
  color: var(--text);
  outline: none;
}

.find .count {
  min-width: 52px;
  padding: 0 6px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
  text-align: right;
}

.find .icon {
  width: 24px;
  height: 24px;
}

.up {
  transform: rotate(180deg);
}
</style>
