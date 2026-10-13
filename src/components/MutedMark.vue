<script setup lang="ts">
// Beside a project's name, in the popover's list and the window's: nothing is told about it.
import { t } from "../lib/i18n";
import Icon from "./Icon.vue";
</script>

<template>
  <span class="muted-mark" role="img" :aria-label="t('notifications.mutedMark')" :title="t('notifications.mutedMark')">
    <Icon name="bell-off" :size="12" />
  </span>
</template>

<style scoped>
.muted-mark {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-faint);
}
</style>
