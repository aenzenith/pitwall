<script setup lang="ts">
import { computed } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import type { TodoItem } from "../../lib/chat/store";
import { t } from "../../lib/i18n";

/**
 * Claude's to-do list (TodoWrite, or the Task* tools), pinned over the composer (V135): how many
 * are done, the one under way; unfolded, every item with its state.
 */
const props = defineProps<{ todos: TodoItem[]; folded: boolean }>();
const emit = defineEmits<{ fold: [] }>();

const done = computed(() => props.todos.filter((todo) => todo.state === "completed").length);
const current = computed(() => props.todos.find((todo) => todo.state === "in_progress") ?? null);
</script>

<template>
  <div v-if="todos.length" :class="['todo-list', { folded }]">
    <button type="button" class="head" :aria-expanded="!folded" @click="emit('fold')">
      <span :class="['chev', { down: !folded }]"><Icon name="chevron" :size="11" /></span>
      <span class="title">{{ t("chat.todos.title") }}</span>
      <span class="count">{{ t("chat.todos.count", { done, total: todos.length }) }}</span>
      <span v-if="folded && current" class="now">{{ current.active || current.content }}</span>
    </button>
    <Collapse :show="!folded" fit>
      <ul class="items">
        <li v-for="(todo, i) in todos" :key="i" :class="todo.state">
          <span class="box" aria-hidden="true"><Icon v-if="todo.state === 'completed'" name="check" :size="10" /></span>
          <span class="text" data-find>{{ todo.state === "in_progress" && todo.active ? todo.active : todo.content }}</span>
        </li>
      </ul>
    </Collapse>
  </div>
</template>

<style scoped>
/* Given less height than it holds (ChatView's room over the composer, beside a question's card), its
   items scroll under its head, which always shows: its least height is the head and its edge (30),
   and open, one item (19) with the padding under it, easing with its fold. */
.todo-list {
  display: flex;
  flex-direction: column;
  min-height: 30px;
  transition: min-height 0.2s cubic-bezier(0.2, 0, 0, 1);
  border: 1px solid var(--line-inset);
  border-radius: 8px;
  background: var(--bg-panel);
  font-size: 12.5px;
}

.todo-list:not(.folded) {
  min-height: 57px;
}

@media (prefers-reduced-motion: reduce) {
  .todo-list {
    transition: none;
  }
}

.head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  min-width: 0;
  height: 28px;
  padding: 0 10px 0 8px;
  border: 0;
  background: none;
  color: var(--text);
  font: inherit;
  text-align: left;
}

.chev {
  display: inline-flex;
  color: var(--text-faint);
  transition: transform 0.15s;
}

.chev.down {
  transform: rotate(90deg);
}

.title {
  font-weight: 600;
  color: var(--text-strong);
}

.count {
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
}

.now {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--claude-text);
}

.items {
  max-height: 180px;
  margin: 0;
  padding: 0 12px 8px 12px;
  overflow: auto;
  list-style: none;
}

.items li {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 2px 0;
  color: var(--text-soft);
}

.box {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 12px;
  height: 12px;
  margin-top: 3px;
  box-sizing: border-box;
  border: 1.5px solid var(--line-strong);
  border-radius: 3px;
}

.completed .box {
  border-color: var(--run);
  background: var(--run);
  color: var(--bg-app);
}

.completed .text {
  color: var(--text-faint);
  text-decoration: line-through;
}

.in_progress .box {
  border-color: var(--claude);
}

.in_progress .text {
  color: var(--text-strong);
}
</style>
