<script setup lang="ts">
import { computed } from "vue";

import ClaudeDot from "../../components/ClaudeDot.vue";
import { bounds, clock, clockRange, duration, projectTiles, sessionShare, type Live } from "../../lib/day";
import { turnLine } from "../../lib/format";
import { t } from "../../lib/i18n";
import { dragRegion } from "../../lib/platform";
import { api, now, snapshot } from "../../lib/store";
import type { DayProject } from "../../lib/types";

/** `selected`: the project shown. */
const props = defineProps<{ selected: DayProject | null }>();

/** Claude, live, per session, whatever day is shown: waiting on you or working. */
const liveSessions = computed(() => {
  const found = new Map<string, Live>();
  for (const p of snapshot.value?.projects ?? []) {
    for (const s of p.claudeSessions) {
      found.set(s.id, s.phase === "waiting" ? { phase: "waiting", text: s.turn ? turnLine(s.turn, now.value) : "" } : { phase: "working", text: t("claude.working") });
    }
  }
  return found;
});

const selectedSpan = computed(() => {
  const span = props.selected ? bounds([props.selected]) : null;
  return span ? clockRange(span[0], span[1]) : "";
});

const tiles = computed(() => (props.selected ? projectTiles(props.selected) : []));

const commits = computed(() => [...(props.selected?.commits ?? [])].reverse());
</script>

<template>
  <aside class="side" :aria-label="selected?.name">
    <!-- Full height like the project details; its header line meets the toolbar's. -->
    <!-- Always drawn, so its line doesn't pop in when the day arrives. -->
    <div class="side-head" :data-tauri-drag-region="dragRegion">
      <template v-if="selected">
        <span class="side-name">{{ selected.name }}</span>
        <span class="side-span">{{ selectedSpan }}</span>
      </template>
    </div>

    <template v-if="selected">
      <div class="side-body">
        <div class="tiles">
          <div v-for="tile in tiles" :key="tile.label" class="tile">
            <span class="tile-label">{{ tile.label }}</span>
            <span :class="['tile-value', { none: !tile.value }]">{{ tile.value || "—" }}</span>
            <span v-if="tile.note" class="tile-note">{{ tile.note }}</span>
          </div>
        </div>

        <section v-if="selected.sessions.length" class="group">
          <span class="section-label group-title">{{ t("day.sessions") }}</span>
          <div
            v-for="session in selected.sessions"
            :key="session.id"
            class="session"
            role="button"
            tabindex="0"
            :title="t('day.sessionTitle')"
            @click="api.revealClaude(selected.path, session.id)"
            @keydown.enter.prevent="api.revealClaude(selected.path, session.id)"
            @keydown.space.prevent="api.revealClaude(selected.path, session.id)"
          >
            <span class="mark"><ClaudeDot :live="liveSessions.get(session.id)" /></span>
            <div class="session-body">
              <div class="session-top">
                <span :class="['session-name', { untitled: !session.title }]" :title="session.title ?? ''">{{ session.title || t("day.untitled") }}</span>
                <span v-if="session.turns" class="session-turns">{{ t("day.turns", { count: session.turns }) }}</span>
              </div>
              <div class="session-bottom">
                <span class="mini">
                  <i class="mini-work" :style="{ width: sessionShare(session.work, session) }"></i>
                  <i class="mini-wait" :style="{ width: sessionShare(session.wait, session) }"></i>
                </span>
                <span class="mono session-length">{{ duration(session.work + session.wait) }}</span>
              </div>
            </div>
          </div>
        </section>

        <section v-if="commits.length" class="group">
          <span class="section-label group-title">{{ t("day.commits") }}</span>
          <div v-for="commit in commits" :key="commit.at + commit.subject" class="line">
            <span class="mono">{{ clock(commit.at) }}</span>
            <span class="line-text" :title="commit.subject">{{ commit.subject }}</span>
          </div>
        </section>

        <section v-if="selected.commands.length" class="group">
          <span class="section-label group-title">{{ t("day.commands") }}</span>
          <div v-for="command in selected.commands" :key="command.name" class="line">
            <span class="line-text">{{ command.name }}</span>
            <span class="mono">{{ command.runs }}×</span>
            <span v-if="command.failed" class="failed">{{ t("day.failed", { count: command.failed }) }}</span>
          </div>
        </section>
      </div>
    </template>
  </aside>
</template>

<style scoped>
/* As wide as the project details and as tall, so the panel line stays where it was. */
.side {
  width: 380px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
}

/* The toolbar's height and line, so the two bottom lines meet. */
.side-head {
  height: 56px;
  flex-shrink: 0;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1px;
  min-width: 0;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.side-body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 18px 20px;
  overflow-y: auto;
}

.side-name {
  min-width: 0;
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.side-span,
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.tiles {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.tile {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 10px 12px;
  border-radius: var(--radius-card);
  background: #181b20;
  border: 1px solid var(--line);
}

.tile-label {
  font-size: 11px;
  color: var(--text-subtle);
}

.tile-value {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-strong);
}

.tile-value.none {
  color: var(--text-faint);
}

.tile-note {
  font-size: 11px;
  color: var(--crash-text);
}

.group {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

/* Claude's dot centred on the session's two lines, spaced as the project list spaces its status:
   in a 16px slot, 10 from the row's edge and 10 from the name. Clickable as a whole: the hover
   background reaches past the dot to the row's edge. */
.session {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0 -10px;
  padding: 4px 10px;
  border-radius: 6px;
}

.session:hover {
  background: var(--bg-hover);
}

.session:focus-visible {
  outline-offset: 0;
}

.mark {
  width: 16px;
  flex-shrink: 0;
  display: inline-flex;
  justify-content: center;
}

.session-body {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.session-top {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.session-name {
  flex-grow: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.session-name.untitled {
  color: var(--text-muted);
  font-style: italic;
}

.session-turns {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.session-bottom {
  display: flex;
  align-items: center;
  gap: 8px;
}

.session-bottom .mono {
  font-size: 11px;
}

/* Same width on every row, so the bars line up at both ends. */
.session-length {
  flex-shrink: 0;
  min-width: 6ch;
  text-align: right;
}

.mini {
  flex-grow: 1;
  height: 6px;
  display: flex;
  border-radius: 3px;
  overflow: hidden;
  background: #1c1f25;
}

.mini-work {
  background: linear-gradient(180deg, #f6a560, #e3792e);
}

.mini-wait {
  background: repeating-linear-gradient(135deg, rgba(240, 136, 62, 0.55) 0 2px, transparent 2px 4px);
}

.line {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  font-size: 12px;
}

.line-text {
  flex-grow: 1;
  min-width: 0;
  color: #c7ccd3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.line .mono {
  flex-shrink: 0;
}

.failed {
  flex-shrink: 0;
  color: var(--crash-text);
}
</style>
