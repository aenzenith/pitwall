<script setup lang="ts">
withDefaults(defineProps<{ size?: number }>(), { size: 13 });
</script>

<template>
  <span class="spinner" :style="{ width: `${size}px`, height: `${size}px` }" aria-hidden="true"></span>
</template>

<style scoped>
.spinner {
  display: inline-block;
  flex-shrink: 0;
  border: 1.6px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: spin 0.75s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
