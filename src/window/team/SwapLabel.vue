<script setup lang="ts">
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";

/**
 * A button's label that changes ("Start team", "Stop team") in a box that doesn't: every label
 * it may show lies in the same place, only `show` is seen, so the button keeps the width of the
 * widest. `show` null: a spinner in their place.
 */
defineProps<{
  labels: Array<{ id: string; text: string; icon: "play" | "stop" }>;
  show: string | null;
  /** The icons' size; the spinner's is a little larger, as the server's own button has it. */
  icon?: number;
}>();
</script>

<template>
  <span class="swap">
    <span v-for="label in labels" :key="label.id" :class="['swap-label', { on: label.id === show }]" :aria-hidden="label.id === show ? undefined : 'true'">
      <Icon :name="label.icon" :size="icon ?? 11" /> {{ label.text }}
    </span>
    <Spinner v-if="show === null" class="swap-spin" :size="(icon ?? 11) + 1" />
  </span>
</template>

<style scoped>
.swap {
  display: inline-grid;
  align-items: center;
  justify-items: center;
}

.swap-label,
.swap-spin {
  grid-area: 1 / 1;
}

.swap-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  visibility: hidden;
}

.swap-label.on {
  visibility: visible;
}
</style>
