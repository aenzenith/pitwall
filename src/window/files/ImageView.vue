<script lang="ts">
/** What the status bar says of a picture shown: its size in pixels, the scale it is drawn at and
 * whether a click changes that (a picture larger than the room). */
export type PictureInfo = { file: string; width: number; height: number; zoom: number; zoomable: boolean };

/** Each picture read, by the view of the file it was read for (`Opened.view`): a tab shown again
 * shows it at once, a file read again (a new view) is asked for again, and it goes with its tab.
 * This page's memory only. */
const pictures = new WeakMap<object, string | null>();
</script>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRaw, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import type { ProjectFiles } from "../../lib/explorer";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";

/** A picture of the project's (a PNG, a JPEG… or an SVG under the eye), as the core reads it: a
 * `data:` URL drawn by an <img> only, so nothing in it runs and nothing is fetched. Fitted to the
 * room, never past 100 %; a click on a larger one shows it at 100 %, scrolled, and back. */
const props = defineProps<{ files: ProjectFiles; file: string }>();
const emit = defineEmits<{ info: [info: PictureInfo | null] }>();

/** The room's margin round the picture. */
const PAD = 20;

/* ---------- the picture, read ---------- */

const view = computed(() => props.files.opened.get(props.file)?.view);
/** The picture as a `data:` URL: undefined while it is read, null when it can't be. */
const url = ref<string | null | undefined>(undefined);
/** Read, but not a picture the page can draw. */
const broken = ref(false);

/* ---------- its size, the room's, and the scale it is drawn at ---------- */

const box = ref<HTMLElement | null>(null);
const img = ref<HTMLImageElement | null>(null);
/** The picture's own size in pixels, once it is decoded; 0 × 0 for an SVG that names none. */
const natural = ref<{ w: number; h: number } | null>(null);
const room = ref({ w: 0, h: 0 });
/** Shown at 100 % rather than fitted. */
const actual = ref(false);

watch(
  view,
  async (shown) => {
    // Still being read (the old picture stays meanwhile), or gone: the viewer says so.
    if (!shown) return;
    const key = toRaw(shown);
    if (pictures.has(key)) {
      url.value = pictures.get(key);
      return;
    }
    const answer = (await api.explorerImage(props.files.path, props.file).catch(() => null)) ?? null;
    pictures.set(key, answer);
    if (toRaw(view.value) !== key) return;
    if (answer !== url.value) {
      broken.value = false;
      natural.value = null;
    }
    url.value = answer;
  },
  { immediate: true },
);

function measure(): void {
  const el = box.value;
  if (el) room.value = { w: Math.max(0, el.clientWidth - PAD * 2), h: Math.max(0, el.clientHeight - PAD * 2) };
}

let sizing: ResizeObserver | null = null;
onMounted(() => {
  sizing = new ResizeObserver(measure);
  if (box.value) sizing.observe(box.value);
  measure();
});
onBeforeUnmount(() => sizing?.disconnect());

const sized = computed(() => (natural.value && natural.value.w > 0 && natural.value.h > 0 ? natural.value : null));
const fit = computed(() => {
  const own = sized.value;
  if (!own || room.value.w <= 0 || room.value.h <= 0) return 1;
  return Math.min(1, room.value.w / own.w, room.value.h / own.h);
});
/** Larger than the room: a click shows it at 100 % and back. */
const zoomable = computed(() => fit.value < 1);
const scale = computed(() => (actual.value || !zoomable.value ? 1 : fit.value));
const drawn = computed(() => {
  const own = sized.value;
  // An SVG with no size of its own takes the room, its shape kept.
  if (!own) return { width: "100%", height: "100%" };
  return { width: `${Math.max(1, Math.floor(own.w * scale.value))}px`, height: `${Math.max(1, Math.floor(own.h * scale.value))}px` };
});
const cursor = computed(() => (!zoomable.value ? "" : actual.value ? "zoom-out" : "zoom-in"));
const name = computed(() => props.file.slice(props.file.lastIndexOf("/") + 1));

function onLoad(): void {
  const el = img.value;
  if (!el) return;
  broken.value = false;
  // Its own size from a copy that isn't laid out: WebKit gives an SVG drawn in the room the
  // room's size as its natural one.
  const source = el.currentSrc || el.src;
  const own = new Image();
  own.src = source;
  own
    .decode()
    .then(() => {
      if (img.value === el && (el.currentSrc || el.src) === source) natural.value = { w: own.naturalWidth, h: own.naturalHeight };
    })
    .catch(() => {
      if (img.value === el) natural.value = { w: el.naturalWidth, h: el.naturalHeight };
    });
}

function onError(): void {
  broken.value = true;
}

/** Fit ↔ 100 %: going to 100 %, the point clicked stays under the pointer. */
async function toggle(event: MouseEvent): Promise<void> {
  const el = img.value;
  const scroller = box.value;
  if (!zoomable.value || !el || !scroller) return;
  const picture = el.getBoundingClientRect();
  const frame = scroller.getBoundingClientRect();
  const fx = (event.clientX - picture.left) / picture.width;
  const fy = (event.clientY - picture.top) / picture.height;
  actual.value = !actual.value;
  if (!actual.value) return;
  await nextTick();
  scroller.scrollLeft = el.offsetLeft + fx * el.offsetWidth - (event.clientX - frame.left);
  scroller.scrollTop = el.offsetTop + fy * el.offsetHeight - (event.clientY - frame.top);
}

watch(
  [sized, scale, zoomable, broken, url],
  () => {
    const own = sized.value;
    emit("info", own && url.value && !broken.value ? { file: props.file, width: own.w, height: own.h, zoom: scale.value, zoomable: zoomable.value } : null);
  },
  { immediate: true },
);
</script>

<template>
  <div ref="box" class="image-view">
    <Spinner v-if="url === undefined" class="quiet" />
    <span v-else-if="url === null || broken" class="quiet">{{ t("files.unreadable") }}</span>
    <img
      v-else
      ref="img"
      :src="url"
      :alt="name"
      :class="['picture', cursor, { waiting: !natural, sized: !!sized }]"
      :style="drawn"
      draggable="false"
      decoding="async"
      @load="onLoad"
      @error="onError"
      @click="toggle"
    />
  </div>
</template>

<style scoped>
.image-view {
  position: relative;
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  overflow: auto;
  padding: 20px;
  background: var(--bg-log);
}

.quiet {
  margin: auto;
  font-size: 12.5px;
  color: var(--text-faint);
}

/* Centred while it fits; larger (at 100 %), it scrolls from its own edge. */
.picture {
  display: block;
  flex-shrink: 0;
  margin: auto;
  object-fit: contain;
  user-select: none;
  -webkit-user-select: none;
  -webkit-user-drag: none;
}

/* What a transparent picture leaves is shown on two close tones of the code's grounds. */
.picture.sized {
  background-color: var(--bg-code-head);
  background-image: conic-gradient(var(--bg-code-tabs) 25%, transparent 0 50%, var(--bg-code-tabs) 0 75%, transparent 0);
  background-size: 16px 16px;
}

.picture.waiting {
  visibility: hidden;
}

.zoom-in {
  cursor: zoom-in;
}

.zoom-out {
  cursor: zoom-out;
}
</style>
