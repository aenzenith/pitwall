<script setup lang="ts">
import Icon from "../../components/Icon.vue";

/** Why the limits can't be shown, or can't be kept fresh: a tag, a title, what to do. */
defineProps<{ tag: string; title: string }>();
</script>

<template>
  <section class="notice" role="status">
    <span class="badge"><Icon name="fuel" :size="16" /></span>
    <div class="text">
      <span class="section-label">{{ tag }}</span>
      <span class="title">{{ title }}</span>
      <span class="body"><slot /></span>
    </div>
  </section>
</template>

<style scoped>
.notice {
  display: flex;
  align-items: flex-start;
  gap: 14px;
  padding: 16px 18px;
  border-radius: var(--radius-panel);
  background: var(--bg-sidebar);
  border: 1px solid var(--line);
}

.badge {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  background: var(--bg-control);
  color: #b4bac3;
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
  color: #c7ccd3;
}

.body :deep(code) {
  padding: 0 4px;
  border-radius: 4px;
  background: var(--bg-control);
  font-family: var(--font-mono);
  font-size: 11.5px;
}
</style>
