<script setup lang="ts">
import Icon from "../../components/Icon.vue";

/** What the page can't show or do, and why: a tag, a title, what it means; a button in the
 * `action` slot. `compact` for the session details' narrower column. */
withDefaults(defineProps<{ tag: string; title: string; icon?: "spark" | "info" | "lock"; compact?: boolean }>(), { icon: "spark", compact: false });
</script>

<template>
  <section :class="['notice', { compact }]" role="note" :aria-label="title">
    <span class="badge"><Icon :name="icon" :size="compact ? 14 : 15" /></span>
    <div class="text">
      <span class="section-label">{{ tag }}</span>
      <span class="title">{{ title }}</span>
      <span class="body"><slot /></span>
    </div>
    <div v-if="$slots.action" class="action"><slot name="action" /></div>
  </section>
</template>

<style scoped>
/* In a narrow column the button drops under the text, at the right. */
.notice {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  gap: 14px;
  padding: 16px 18px;
  border-radius: var(--radius-panel);
  background: var(--bg-sidebar);
  border: 1px solid var(--line);
}

.notice.compact {
  gap: 12px;
  padding: 12px 14px;
  border-radius: var(--radius-card);
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

.compact .badge {
  width: 28px;
  height: 28px;
  border-radius: 7px;
}

.text {
  flex: 1 1 200px;
  display: flex;
  flex-direction: column;
  gap: 4px;
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

.action {
  flex-shrink: 0;
  margin-left: auto;
  align-self: center;
}
</style>
