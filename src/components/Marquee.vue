<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

/** One line of text; when it doesn't fit, it slides to its end and back so it can be read. */
const props = defineProps<{ text: string }>();

/** Pixels per second while sliding. */
const SPEED = 28;

const box = ref<HTMLElement | null>(null);
const track = ref<HTMLElement | null>(null);
/** How far the text sticks out of its box. */
const overflow = ref(0);

const style = computed(() => {
  if (overflow.value <= 0) return undefined;
  // The slide takes 70% of the cycle; the rest is a pause at each end.
  const duration = Math.max(3, overflow.value / SPEED / 0.7);
  return { "--shift": `-${overflow.value}px`, "--duration": `${duration.toFixed(2)}s` };
});

function measure(): void {
  if (!box.value || !track.value) return;
  overflow.value = Math.max(0, Math.ceil(track.value.offsetWidth - box.value.clientWidth));
}

let observer: ResizeObserver | null = null;

onMounted(() => {
  observer = new ResizeObserver(measure);
  if (box.value) observer.observe(box.value);
  if (track.value) observer.observe(track.value);
  measure();
});

onBeforeUnmount(() => observer?.disconnect());

watch(
  () => props.text,
  () => void nextTick(measure),
);
</script>

<template>
  <span ref="box" class="marquee" :title="overflow > 0 ? text : undefined">
    <span ref="track" :class="['track', { moving: overflow > 0 }]" :style="style">{{ text }}</span>
  </span>
</template>

<style scoped>
.marquee {
  display: block;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}

.track {
  display: inline-block;
}

.track.moving {
  animation: slide var(--duration) ease-in-out infinite alternate;
}

@keyframes slide {
  0%,
  15% {
    transform: translateX(0);
  }

  85%,
  100% {
    transform: translateX(var(--shift));
  }
}

@media (prefers-reduced-motion: reduce) {
  .track.moving {
    animation: none;
  }
}
</style>
