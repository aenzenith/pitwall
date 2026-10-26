<script setup lang="ts">
import Icon from "../../components/Icon.vue";

/** Why the limits can't be shown, or can't be kept fresh: a tag, a title, what to do; `actions`,
 * the buttons that do it. */
defineProps<{ tag: string; title: string }>();
</script>

<template>
  <section class="notice glass" role="status">
    <span class="badge"><Icon name="fuel" :size="16" /></span>
    <div class="text">
      <span class="section-label">{{ tag }}</span>
      <span class="title">{{ title }}</span>
      <span class="body"><slot /></span>
      <div v-if="$slots.actions" class="actions"><slot name="actions" /></div>
    </div>
  </section>
</template>

<style scoped>
.notice {
  display: flex;
  align-items: flex-start;
  gap: 14px;
  padding: 16px 18px;
}

.badge {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  background: var(--bg-well);
  color: var(--text-nav);
}

.text {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex-grow: 1;
  min-width: 0;
}

.title {
  font-weight: 600;
}

.body {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-soft);
}

/* What can be done about it, under what it says. */
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 6px;
}

.body :deep(code) {
  padding: 0 4px;
  border-radius: 4px;
  background: var(--bg-well);
  font-family: var(--font-mono);
  font-size: 11.5px;
}
</style>
