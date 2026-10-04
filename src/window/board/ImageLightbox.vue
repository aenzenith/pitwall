<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import type { Shot } from "../../lib/cardImages";
import { t } from "../../lib/i18n";

/**
 * An image of a card's note, large, over the window. It grows out of its small one in the strip
 * and goes back into it. ← and → (or the arrows) show the note's other images; Esc, the close
 * button or a click beside the image closes. `start`: the image to open on; `thumb`: the small
 * image of `n` in the strip, where its large one comes from and returns to.
 */
const props = defineProps<{ images: Shot[]; start: number; thumb: (n: number) => HTMLElement | null }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);

const current = ref(props.start);
/** Which way the last step went: the images slide that way. */
const direction = ref<"next" | "prev">("next");
/** The large image isn't read yet: nothing of it shows before it can leave its small one. */
const waiting = ref(true);
let opening: Animation | null = null;
let closing = false;

/** The images that have their picture. */
const shown = computed(() => props.images.filter((image) => image.src));
const index = computed(() => shown.value.findIndex((image) => image.n === current.value));
const image = computed(() => shown.value[index.value] ?? null);
const label = computed(() => t("board.image", { n: current.value }));

const OPEN: KeyframeAnimationOptions = { duration: 280, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" };
const CLOSE: KeyframeAnimationOptions = { duration: 210, easing: "cubic-bezier(0.4, 0, 0.2, 1)", fill: "forwards" };
/** The image in its place, as the stylesheet draws it. */
const PLACED: Keyframe = { transform: "none", clipPath: "inset(0px 0px round 8px)" };
/** Without a small image to leave from or go to: a short step back. */
const AWAY: Keyframe = { transform: "scale(0.96)", opacity: 0 };
/** The corners of a small image, inside its tile's line. */
const THUMB_RADIUS = 6;

function still(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function picture(): HTMLImageElement | null {
  return dialog.value?.querySelector<HTMLImageElement>(`.lb-image[data-n="${current.value}"]`) ?? null;
}

/** The shade and the controls: they fade while the image flies. */
function surround(): HTMLElement[] {
  return Array.from(dialog.value?.querySelectorAll<HTMLElement>(".lb-shade, .lb-chrome") ?? []);
}

/**
 * `large` drawn where `small` is: moved and scaled onto it, cut to the square the small one shows
 * of it. Null when the small one is out of sight.
 */
function atThumb(large: HTMLElement, small: HTMLElement): Keyframe | null {
  const to = large.getBoundingClientRect();
  const from = small.getBoundingClientRect();
  if (!to.width || !to.height || !from.width || !from.height) return null;
  if (from.bottom < 0 || from.right < 0 || from.top > window.innerHeight || from.left > window.innerWidth) return null;

  // The small one covers its tile: the larger of the two ratios.
  const scale = Math.max(from.width / to.width, from.height / to.height);
  const x = from.left + from.width / 2 - (to.left + to.width / 2);
  const y = from.top + from.height / 2 - (to.top + to.height / 2);
  const cutX = (to.width - from.width / scale) / 2;
  const cutY = (to.height - from.height / scale) / 2;
  return { transform: `translate(${x}px, ${y}px) scale(${scale})`, clipPath: `inset(${cutY}px ${cutX}px round ${THUMB_RADIUS / scale}px)` };
}

onMounted(async () => {
  const box = dialog.value;
  if (!box) return;
  box.showModal();
  // The keyboard is the dialog's own, not its first button's: the arrow keys draw no ring.
  box.focus({ preventScroll: true });

  const calm = still();
  if (!calm) for (const el of surround()) el.animate([{ opacity: 0 }, { opacity: 1 }], OPEN);

  const large = picture();
  // Measured once it has its size.
  await large?.decode().catch(() => undefined);
  waiting.value = false;
  await nextTick();
  if (calm || closing || !large?.isConnected) return;

  const small = props.thumb(current.value);
  const from = small && atThumb(large, small);
  opening = large.animate(from ? [from, PLACED] : [AWAY, { transform: "none", opacity: 1 }], OPEN);
});

/** Back into its small image, then the dialog closes (which tells the strip). */
async function close(): Promise<void> {
  const box = dialog.value;
  if (closing || !box?.open) return;
  closing = true;

  if (!still()) {
    const runs = surround().map((el) => el.animate({ opacity: 0 }, CLOSE));
    const large = picture();
    if (large && opening?.playState === "running") {
      // Closed while it still grew: the same way back.
      opening.effect?.updateTiming({ fill: "both" });
      opening.reverse();
      runs.push(opening);
    } else if (large) {
      const small = props.thumb(current.value);
      const to = small && atThumb(large, small);
      runs.push(large.animate(to ? [PLACED, to] : [{ transform: "none", opacity: 1 }, AWAY], CLOSE));
    }
    await Promise.all(runs.map((run) => run.finished)).catch(() => undefined);
  }
  box.close();
}

function step(by: number): void {
  const next = shown.value[index.value + by];
  if (!next || closing) return;
  direction.value = by > 0 ? "next" : "prev";
  current.value = next.n;
}

function onKey(event: KeyboardEvent): void {
  if (event.key === "ArrowLeft") step(-1);
  else if (event.key === "ArrowRight") step(1);
  else if (event.key === "Escape") void close();
  else return;
  event.preventDefault();
}

// Its image left the note while it was open.
watch(index, (at) => {
  if (at < 0) dialog.value?.close();
});
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog" :class="['lightbox', { waiting }]" tabindex="-1" :aria-label="label" @close="emit('close')" @cancel.prevent="close" @keydown="onKey">
      <div class="lb-shade" @click="close"></div>
      <div class="lb-stage">
        <Transition :name="`lb-${direction}`">
          <img v-if="image?.src" :key="image.n" class="lb-image" :data-n="image.n" :src="image.src" :alt="label" draggable="false" />
        </Transition>
      </div>
      <div class="lb-chrome">
        <button type="button" class="lb-control lb-close" :aria-label="t('common.close')" :title="t('common.close')" @click="close">
          <Icon name="close" :size="16" />
        </button>
        <template v-if="shown.length > 1">
          <button type="button" class="lb-control lb-prev" :disabled="index <= 0" :aria-label="t('board.imagePrevious')" :title="t('board.imagePrevious')" @click="step(-1)">
            <Icon name="back" :size="18" />
          </button>
          <button type="button" class="lb-control lb-next" :disabled="index >= shown.length - 1" :aria-label="t('board.imageNext')" :title="t('board.imageNext')" @click="step(1)">
            <Icon name="chevron" :size="18" />
          </button>
        </template>
        <div class="lb-caption" aria-hidden="true">{{ label }}</div>
      </div>
    </dialog>
  </Teleport>
</template>

<style scoped>
/* The whole window: the shade is its own layer, so it can fade out before the dialog closes. */
.lightbox {
  position: fixed;
  inset: 0;
  width: 100vw;
  height: 100vh;
  max-width: none;
  max-height: none;
  margin: 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text);
  overflow: hidden;
  outline: none;
}

.lightbox::backdrop {
  background: transparent;
}

.lb-shade {
  position: absolute;
  inset: 0;
  background: rgba(8, 9, 11, 0.88);
}

/* A click beside the image falls through to the shade. */
.lb-stage,
.lb-chrome {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.lb-stage {
  display: grid;
  place-items: center;
}

/* Never larger than it is; room around it for the controls. Two share the cell while one
   follows the other. */
.lb-image {
  grid-area: 1 / 1;
  display: block;
  max-width: calc(100vw - 128px);
  max-height: calc(100vh - 112px);
  border-radius: 8px;
  pointer-events: auto;
}

.waiting .lb-image {
  visibility: hidden;
}

.lb-control {
  position: absolute;
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.08);
  color: var(--text);
  pointer-events: auto;
  transition: background-color 0.12s ease;
}

.lb-control:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.16);
}

.lb-control:disabled {
  opacity: 0.3;
}

.lb-close {
  top: 16px;
  right: 16px;
}

.lb-prev,
.lb-next {
  top: 50%;
  width: 36px;
  height: 36px;
  margin-top: -18px;
}

.lb-prev {
  left: 16px;
}

.lb-next {
  right: 16px;
}

/* Its number, as the note writes it. */
.lb-caption {
  position: absolute;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

/* One image follows another: both fade, sliding the way the step went. */
.lb-next-enter-active,
.lb-next-leave-active,
.lb-prev-enter-active,
.lb-prev-leave-active {
  transition:
    opacity 0.22s ease,
    transform 0.22s cubic-bezier(0.2, 0.8, 0.2, 1);
}

.lb-next-enter-from,
.lb-prev-leave-to {
  opacity: 0;
  transform: translateX(28px);
}

.lb-next-leave-to,
.lb-prev-enter-from {
  opacity: 0;
  transform: translateX(-28px);
}

@media (prefers-reduced-motion: reduce) {
  .lb-next-enter-active,
  .lb-next-leave-active,
  .lb-prev-enter-active,
  .lb-prev-leave-active,
  .lb-control {
    transition: none;
  }
}
</style>
