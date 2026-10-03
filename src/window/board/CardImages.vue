<script setup lang="ts">
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import type { Shot } from "../../lib/cardImages";
import { t } from "../../lib/i18n";

/**
 * The images of a card's note, small, each with its number (the `n` of `[Image #n]`). `removable`:
 * each has a button that takes it out of the note (the dialog's).
 */
defineProps<{ images: Shot[]; removable?: boolean }>();
const emit = defineEmits<{ remove: [n: number] }>();
</script>

<template>
  <ul class="shots" :aria-label="t('board.images')">
    <li v-for="image in images" :key="image.n" class="shot" :title="t('board.image', { n: image.n })">
      <img v-if="image.src" :src="image.src" :alt="t('board.image', { n: image.n })" draggable="false" />
      <Spinner v-else :size="11" />
      <span class="shot-n" aria-hidden="true">{{ image.n }}</span>
      <button v-if="removable" type="button" class="shot-remove" :aria-label="t('board.dialog.removeImage', { n: image.n })" @click="emit('remove', image.n)">
        <Icon name="close" :size="10" />
      </button>
    </li>
  </ul>
</template>

<style scoped>
.shots {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.shot {
  position: relative;
  display: grid;
  place-items: center;
  width: 52px;
  height: 52px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text-subtle);
  overflow: hidden;
}

.shot img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

/* Its number, as the note writes it. */
.shot-n {
  position: absolute;
  left: 3px;
  bottom: 3px;
  min-width: 14px;
  padding: 0 3px;
  border-radius: 4px;
  background: rgba(8, 9, 11, 0.78);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 14px;
  text-align: center;
}

.shot-remove {
  position: absolute;
  top: 3px;
  right: 3px;
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: rgba(8, 9, 11, 0.78);
  color: var(--text);
  opacity: 0;
}

.shot:hover .shot-remove,
.shot-remove:focus-visible {
  opacity: 1;
}

.shot-remove:hover {
  background: #2c3039;
}
</style>
