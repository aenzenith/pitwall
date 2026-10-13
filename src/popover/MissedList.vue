<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../components/Icon.vue";
import { ago } from "../lib/format";
import { t } from "../lib/i18n";
import { missed, missedLine, missedName, missedTone } from "../lib/quiet";
import { rowKeys } from "../lib/rows";
import { api, now } from "../lib/store";
import type { Missed } from "../lib/types";

/**
 * Above the popover's projects: what the user wasn't told while it was quiet (`snapshot.missed`),
 * newest first. A line is its own button: it opens what it is about (the core's `missed_open`:
 * Claude's session where it runs, else Pitwall's window) and goes. Many of them scroll in their
 * own room, which gives way when the popover runs out of it (Popover.vue), down to two lines.
 *
 * `leave`: the keyboard was on something that is gone (the last line, "Clear"), and the popover
 * finds it a place.
 */
const emit = defineEmits<{ leave: [] }>();

/** A line by its kind and project: the list holds one of each. */
const id = (entry: Missed): string => `${entry.kind}:${entry.path}`;

/** The line the keyboard is on: the list's one Tab stop (the first line until the arrows move it). */
const focused = ref<string | null>(null);
const tabStop = computed(() => (missed.value.some((entry) => id(entry) === focused.value) ? focused.value : missed.value[0] ? id(missed.value[0]) : null));

function open(entry: Missed): void {
  void api.missedOpen(entry.path, entry.kind);
}

/** ↑/↓ move between lines; ↵ and Space open one, and the keyboard goes on to the line after it
 * (before it, for the last), which stays. */
function onKey(event: KeyboardEvent): void {
  const act = (line: string): void => {
    const entry = missed.value.find((known) => id(known) === line);
    if (!entry) return;
    const from = event.target as HTMLElement;
    const next = (from.nextElementSibling ?? from.previousElementSibling) as HTMLElement | null;
    if (next) next.focus();
    else emit("leave");
    open(entry);
  };
  rowKeys(event, { move: (line) => (focused.value = line), enter: act, space: act });
}

/** Pressed from the keyboard, the button goes with the list it clears. */
function clear(event: MouseEvent): void {
  if (event.detail === 0) emit("leave");
  void api.missedClear();
}
</script>

<template>
  <section v-if="missed.length" class="missed" :style="{ '--count': missed.length }" :aria-label="t('missed.title', { count: missed.length })">
    <div class="head">
      <span class="section-label">{{ t("missed.title", { count: missed.length }) }}</span>
      <button type="button" class="clear" @click="clear">{{ t("missed.clear") }}</button>
    </div>
    <!-- One Tab stop, as the project list is (lib/rows, which knows a line by its `data-path`). -->
    <div class="lines" data-scrolls @keydown="onKey">
      <div
        v-for="entry in missed"
        :key="id(entry)"
        class="line"
        role="button"
        :tabindex="id(entry) === tabStop ? 0 : -1"
        :data-path="id(entry)"
        :title="missedLine(entry)"
        @focus="focused = id(entry)"
        @click="open(entry)"
      >
        <span class="mark" aria-hidden="true"><span :class="['dot', missedTone(entry.kind)]"></span></span>
        <span class="text">
          <span class="name">{{ missedName(entry) }}</span>
          <span class="what">
            <span class="happened">{{ missedLine(entry) }}</span>
            <span class="when">{{ ` · ${ago(entry.at, now)}` }}</span>
          </span>
        </span>
        <Icon name="chevron" :size="12" class="go" />
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Out of room, the popover has it give way before its other parts (Popover.vue: `.claude`), by
   what is over (`--gives`) and never under its head and two of its lines. `--room`: what its
   lines take when nothing is over. */
.missed {
  --room: min(var(--count) * 46px, 200px);
  display: flex;
  flex-direction: column;
  flex: 0 1e12 auto;
  min-height: calc(33px + max(min(var(--room), 92px), var(--room) - var(--gives, 0px)));
  padding-bottom: 6px;
  border-bottom: 1px solid var(--line);
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
  padding: 2px 10px 2px 16px;
}

.clear {
  height: 22px;
  padding: 0 6px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.clear:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* Four and a half lines show; the rest is a scroll away. */
.lines {
  display: flex;
  flex-direction: column;
  min-height: 0;
  max-height: 200px;
  padding: 0 8px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.line {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
  padding: 6px 8px;
  border-radius: var(--radius-control);
}

.line:hover {
  background: var(--bg-hover);
}

/* The keyboard's line: the ring inside, so the room's edge never cuts it. */
.line:focus-visible {
  outline-offset: -2px;
}

/* As wide as a project's status mark in the list under it, so the names line up. */
.mark {
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
}

.dot.claude {
  background: var(--claude);
}

.dot.crash {
  background: var(--crash);
}

.dot.warn {
  background: var(--warn);
}

.dot.run {
  background: var(--run);
}

.text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
}

.name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-name);
}

.name,
.happened {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* What happened gives way to when: the time is never cut. */
.what {
  display: flex;
  font-size: 12px;
  color: var(--text-muted);
}

.when {
  flex-shrink: 0;
  white-space: pre;
}

.go {
  flex-shrink: 0;
  color: var(--text-subtle);
}
</style>
