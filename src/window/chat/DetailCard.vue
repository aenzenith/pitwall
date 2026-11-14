<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import type { ChangeEntry, ChatModel, DetailTab } from "../../lib/chat/store";
import type { ChatAnswer, ChatPending } from "../../lib/chat/wire";
import { t } from "../../lib/i18n";
import ChangeRow from "./ChangeRow.vue";
import CommandRow from "./CommandRow.vue";
import TaskRow from "./TaskRow.vue";

/**
 * The card over the status line (CK3, the user's pick of 16 Nov): Görevler, Değişiklikler and
 * Komutlar as tabs, one at a time, the card as tall as what it shows and never taller than `most`;
 * what doesn't fit scrolls inside it. A change of tab or of what it holds eases its height (the
 * user's word, 16 Nov: no jump between tabs); a row opening or shutting eases itself (Collapse),
 * and the card follows it frame by frame instead of easing after it. Where it stands and what dims
 * under it is ChatView's.
 */
const props = defineProps<{ id: number; path: string; model: ChatModel; now: number; pendingByTool: Map<string, ChatPending>; tab: DetailTab; most: number }>();
const emit = defineEmits<{ tab: [tab: DetailTab]; close: []; long: [title: string, text: string]; answer: [pending: ChatPending, answer: ChatAnswer] }>();

const TABS: DetailTab[] = ["tasks", "changes", "commands"];
const NAMES: Record<DetailTab, "chat.side.tasks" | "chat.side.changes" | "chat.side.commands"> = { tasks: "chat.side.tasks", changes: "chat.side.changes", commands: "chat.side.commands" };

const ui = computed(() => props.model.ui);

/** A row's place in the chat's order: every key is made from one count, in the order things came. */
function order(key: string): number {
  return Number(key.replace(/^\D+/, "")) || 0;
}

/** Where the user's last message stands in that order; 0 before the first. */
const lastMessage = computed(() => {
  for (const list of [props.model.entries, props.model.history]) {
    for (let i = list.length - 1; i >= 0; i--) if (list[i].kind === "user") return order(list[i].key);
  }
  return 0;
});

/** The rows the card shows: all of them, or with "Son mesajdan beri" on those made after the user's
 * last message (the user's word, 16 Nov). */
function shown<T extends { key: string }>(rows: T[]): T[] {
  if (!ui.value.recent) return rows;
  const from = lastMessage.value;
  return rows.filter((row) => order(row.key) > from);
}
const commands = computed(() => shown(props.model.commands));
const tasks = computed(() => shown(props.model.tasks));

/** The session's changes, file by file in the order they were first touched. */
const files = computed(() => {
  const groups = new Map<string, ChangeEntry[]>();
  for (const change of shown(props.model.changes)) {
    const list = groups.get(change.file);
    if (list) list.push(change);
    else groups.set(change.file, [change]);
  }
  return [...groups.entries()].map(([file, changes]) => ({ file, key: changes[0].key, changes }));
});

const counts = computed<Record<DetailTab, number>>(() => ({ tasks: tasks.value.length, changes: files.value.length, commands: commands.value.length }));

function toggle(key: string): void {
  const open = ui.value.open;
  if (open.has(key)) open.delete(key);
  else open.add(key);
}

/* ---------- its height ---------- */

/** The tab row's height, over the rows' room. */
const HEAD = 33;
const body = ref<HTMLElement | null>(null);
const inner = ref<HTMLElement | null>(null);
const natural = ref<number | null>(null);
const moving = ref(false);
let movingTimer: ReturnType<typeof setTimeout> | undefined;
const height = computed(() => (natural.value === null ? null : Math.min(natural.value, Math.max(60, props.most - HEAD))));

const sizer = new ResizeObserver(() => {
  const next = inner.value?.offsetHeight ?? 0;
  if (natural.value === next) return;
  // The first height is taken as it is: the card comes in with it.
  if (natural.value !== null) {
    moving.value = true;
    clearTimeout(movingTimer);
    movingTimer = setTimeout(() => (moving.value = false), 260);
  }
  natural.value = next;
});
watch(inner, (el, old) => {
  if (old) sizer.unobserve(old);
  if (el) sizer.observe(el);
});
/** A row inside eases its own height: the card's follows it as it goes, not a beat behind. */
const following = ref(false);
let followingTimer: ReturnType<typeof setTimeout> | undefined;
function follow(event: TransitionEvent): void {
  if (event.propertyName !== "height") return;
  following.value = true;
  clearTimeout(followingTimer);
  followingTimer = setTimeout(() => (following.value = false), 260);
}

onBeforeUnmount(() => {
  sizer.disconnect();
  clearTimeout(movingTimer);
  clearTimeout(followingTimer);
});

// A new command comes into sight at the foot of the list, as a terminal's line does.
watch(
  () => props.model.commands.length,
  async () => {
    const scroller = body.value;
    if (!scroller || props.tab !== "commands") return;
    const atEnd = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
    await nextTick();
    if (atEnd) scroller.scrollTop = scroller.scrollHeight;
  },
);

// A command whose card waits opens by itself, its buttons in sight.
watch(
  () => [...props.pendingByTool.keys()].join(","),
  () => {
    for (const command of props.model.commands) if (command.tool && props.pendingByTool.has(command.tool)) ui.value.open.add(command.key);
  },
  { immediate: true },
);
</script>

<template>
  <section class="detail-card" role="dialog" :aria-label="t('chat.side.title')">
    <div class="head" role="tablist">
      <button
        v-for="each in TABS"
        :key="each"
        type="button"
        role="tab"
        :aria-selected="tab === each"
        :class="['tab', { on: tab === each }]"
        @click="emit('tab', each)"
      >
        {{ t(NAMES[each]) }}<span class="count">{{ counts[each] }}</span>
      </button>
      <span class="grow"></span>
      <button type="button" :class="['recent', { on: ui.recent }]" :aria-pressed="ui.recent" @click="ui.recent = !ui.recent">
        <Icon name="filter" :size="12" class="funnel" />{{ t("chat.side.recent") }}
      </button>
      <button type="button" class="close" :aria-label="t('common.close')" :title="t('common.close')" @click="emit('close')">
        <Icon name="close" :size="12" />
      </button>
    </div>
    <div ref="body" :class="['body', { moving, following }]" :style="height === null ? undefined : { height: `${height}px` }">
      <div ref="inner" class="inner" role="tabpanel" @transitionrun="follow">
        <template v-if="tab === 'commands'">
          <CommandRow
            v-for="command in commands"
            :id="id"
            :key="command.key"
            :path="path"
            :command="command"
            :open="ui.open.has(command.key)"
            :now="now"
            :pending="command.tool ? (pendingByTool.get(command.tool) ?? null) : null"
            @toggle="toggle(command.key)"
            @long="(title, text) => emit('long', title, text)"
            @answer="(pending, answer) => emit('answer', pending, answer)"
          />
          <p v-if="!commands.length" class="empty">{{ t(ui.recent ? "chat.side.noCommandsRecent" : "chat.side.noCommands") }}</p>
        </template>
        <template v-else-if="tab === 'changes'">
          <ChangeRow v-for="group in files" :key="group.key" :path="path" :file="group.file" :changes="group.changes" :open="ui.open.has(group.key)" @toggle="toggle(group.key)" />
          <p v-if="!files.length" class="empty">{{ t(ui.recent ? "chat.side.noChangesRecent" : "chat.side.noChanges") }}</p>
        </template>
        <template v-else>
          <TaskRow
            v-for="task in tasks"
            :id="id"
            :key="task.key"
            :task="task"
            :open="ui.open.has(task.key)"
            :now="now"
            @toggle="toggle(task.key)"
            @long="(title, text) => emit('long', title, text)"
          />
          <p v-if="!tasks.length" class="empty">{{ t(ui.recent ? "chat.side.noTasksRecent" : "chat.side.noTasks") }}</p>
        </template>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* On the panels' ground, raised over the dimmed conversation. */
.detail-card {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-panel);
  box-shadow: var(--shadow-dialog);
  user-select: none;
}

.head {
  display: flex;
  align-items: center;
  flex: none;
  height: 33px;
  padding: 0 4px 0 6px;
  box-sizing: border-box;
  border-bottom: 1px solid var(--line-soft);
}

/* Words with no ground; the one shown brighter, a line under it. */
.tab {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 9px;
  border: 0;
  background: none;
  color: var(--text-subtle);
  font: inherit;
  font-size: 12.5px;
  white-space: nowrap;
  transition: color 0.18s ease;
}

.tab:hover,
.tab.on {
  color: var(--text-strong);
}

.tab.on::after {
  content: "";
  position: absolute;
  left: 9px;
  right: 9px;
  bottom: 0;
  height: 2px;
  border-radius: 1px;
  background: var(--text-soft);
}

/* "Son mesajdan beri": words with no ground, as the tabs are, a filter mark before them (the user's
   word, 16 Nov: a filter, not a tick); on, both brighter. */
.recent {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 24px;
  padding: 0 8px 0 6px;
  border: 0;
  background: none;
  color: var(--text-subtle);
  font: inherit;
  font-size: 12px;
  white-space: nowrap;
  transition: color 0.18s ease;
}

.recent:hover,
.recent.on {
  color: var(--text-strong);
}

.recent .funnel {
  flex-shrink: 0;
}

.tab:focus-visible,
.recent:focus-visible,
.close:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
  border-radius: var(--radius-control);
}

.count {
  font-size: 11px;
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}

.grow {
  flex: 1;
}

.close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-muted);
}

.close:hover {
  color: var(--text);
}

.body {
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  transition: height 0.22s cubic-bezier(0.2, 0, 0, 1);
}

/* While the height eases, no scroll bar comes and goes. */
.body.moving {
  overflow: hidden;
}

.body.following {
  transition: none;
}

@media (prefers-reduced-motion: reduce) {
  .body {
    transition: none;
  }
}

.inner {
  display: flow-root;
}

.empty {
  margin: 12px 14px;
  font-size: 12px;
  color: var(--text-faint);
}
</style>
