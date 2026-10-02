<script setup lang="ts">
withDefaults(defineProps<{ size?: number; lamps?: string }>(), { size: 16, lamps: "currentColor" });
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    aria-hidden="true"
  >
    <path d="M3 12.5a9 9 0 0 1 18 0" />
    <path d="M6.5 12.8v1.4M12 12.8v1.4M17.5 12.8v1.4" />
    <circle cx="6.5" cy="17.4" r="2.2" :fill="lamps" stroke="none" />
    <circle cx="12" cy="17.4" r="2.2" :fill="lamps" stroke="none" />
    <circle cx="17.5" cy="17.4" r="2.2" :fill="lamps" stroke="none" />
  </svg>
</template>
