<script setup lang="ts">
// Claude's spark, marking what belongs to Claude: its card, its terminal sessions. The same
// rays are drawn into src-tauri/icons/claude-mark.png for notifications.
withDefaults(defineProps<{ size?: number; color?: string }>(), { size: 14, color: "#d97757" });
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    :stroke="color"
    stroke-width="2.2"
    stroke-linecap="round"
    aria-hidden="true"
    class="claude-mark"
  >
    <path
      d="M12 12L12.00 1.50 M12 12L15.85 4.76 M12 12L20.66 7.00 M12 12L20.59 12.30 M12 12L21.18 16.88 M12 12L15.76 19.06 M12 12L11.64 22.19 M12 12L7.80 19.27 M12 12L2.99 17.20 M12 12L3.80 11.71 M12 12L3.34 7.00 M12 12L7.60 4.38"
    />
  </svg>
</template>

<style scoped>
.claude-mark {
  flex-shrink: 0;
}
</style>
