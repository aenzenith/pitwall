<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

import { startRoad, type Road } from "../lib/road";
import { stillMotion } from "../lib/slide";

/**
 * The road of lib/road.ts on a canvas that fills its parent. `intro`: it starts as the site's
 * does, the trails growing and the cars racing in. `running`: false holds it where it is (the
 * window hidden, something over it). With reduced motion it is one still frame.
 */
const props = withDefaults(defineProps<{ intro?: boolean; running?: boolean }>(), { intro: false, running: true });

const canvas = ref<HTMLCanvasElement | null>(null);
let road: Road | null = null;
let resized: ResizeObserver | null = null;

onMounted(() => {
  if (!canvas.value) return;
  road = startRoad(canvas.value, { intro: props.intro, still: stillMotion() });
  road?.setRunning(props.running);
  // A road that stands still is drawn again at its new size; a running one is every frame anyway.
  resized = new ResizeObserver(() => road?.setRunning(props.running));
  resized.observe(canvas.value);
});

watch(
  () => props.running,
  (on) => road?.setRunning(on),
);

onBeforeUnmount(() => {
  resized?.disconnect();
  road?.stop();
  road = null;
});
</script>

<template>
  <canvas ref="canvas" class="road" aria-hidden="true"></canvas>
</template>

<style scoped>
.road {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  display: block;
  pointer-events: none;
}
</style>
