<script setup lang="ts">
import { computed } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import Marquee from "../../components/Marquee.vue";
import Spinner from "../../components/Spinner.vue";
import { editorName, gitLine, phase, turnLine } from "../../lib/format";
import { t } from "../../lib/i18n";
import { sessionTitle } from "../../lib/sessions";
import { api, now, sessions, snapshot } from "../../lib/store";
import { sessionsIn } from "../../lib/track";
import type { Project } from "../../lib/types";

/** The selected project, under the tower: its server, what Claude waits with, and the way to it.
 * Laid out as the project's details on the Projects page are (detail/ServerBlock). */
const props = defineProps<{ project: Project }>();
const emit = defineEmits<{ open: [path: string] }>();

const editor = computed(() => editorName(snapshot.value?.settings.editor));

const server = computed(() => {
  const p = props.project;
  const title = p.status === "busy" ? phase(p) : t(`status.${p.status}`);
  if (p.issue && p.status !== "running") return { title, sub: p.issue.text };
  return { title, sub: [p.runCommand, p.status === "running" && p.port ? `:${p.port}` : ""].filter(Boolean).join(" · ") };
});

/** The sessions that wait on you, each to bring up where it runs; and those at work. One list,
 * under the card's buttons. */
const waiting = computed(() => sessionsIn(props.project, "waiting"));
const working = computed(() => sessionsIn(props.project, "working"));

/** A session's name, as the Sessions page writes it; the start of its id until that page's rows
 * are in. */
function nameOf(id: string): string {
  const row = sessions.value?.sessions.find((entry) => entry.id === id);
  return row ? sessionTitle(row) : id.slice(0, 8);
}

function toggle(): void {
  void api.act(props.project.path, props.project.status === "running" ? "stop" : "start");
}
</script>

<template>
  <section class="card" :aria-label="project.name">
    <div class="card-head">
      <span class="card-name">{{ project.name }}</span>
      <span v-if="project.git" class="card-git">{{ gitLine(project.git) }}</span>
    </div>

    <div class="card-server">
      <div class="server-text">
        <span class="server-title"><span :class="['state', project.status]" aria-hidden="true"></span>{{ server.title }}</span>
        <span :class="['server-sub', { bad: project.status === 'crashed' }]"><Marquee :text="server.sub" /></span>
      </div>
      <button v-if="project.status === 'busy'" type="button" class="control busy" :aria-label="server.title" disabled><Spinner :size="12" /></button>
      <button v-else-if="project.status === 'running'" type="button" class="control stop" @click="toggle"><Icon name="stop" :size="11" /> {{ t("common.stop") }}</button>
      <button v-else type="button" class="control start" @click="toggle"><Icon name="play" :size="11" /> {{ t("common.start") }}</button>
    </div>

    <div class="card-actions">
      <button type="button" class="control grow" @click="api.openBrowser(project.path)">
        <Icon name="external" :size="12" /> <span class="control-text">{{ t("common.openInBrowser") }}</span>
      </button>
      <button v-if="project.status === 'running'" type="button" class="control square" :aria-label="t('common.restart')" :title="t('common.restart')" @click="api.act(project.path, 'restart')">
        <Icon name="restart" :size="13" />
      </button>
    </div>

    <div class="card-actions">
      <button type="button" class="control go grow" @click="emit('open', project.path)">
        <span class="control-text">{{ t("switcher.goToProject") }}</span>
        <!-- Claude waiting: Return in the tower brings its session up instead. -->
        <kbd v-if="!waiting.length" aria-hidden="true">↵</kbd>
      </button>
      <button type="button" class="control" @click="api.openEditor(project.path)"><Icon name="editor" :size="13" /> {{ editor }}</button>
    </div>

    <!-- Each session, by its name, a line each: those that wait first, in Claude's colour, the whole
         line bringing the session up (what it waits with is in its tooltip); then those at work. -->
    <div v-if="waiting.length || working.length || project.claude" class="card-sessions">
      <button
        v-for="session in waiting"
        :key="session.id"
        type="button"
        class="card-session waits"
        :title="[nameOf(session.id), session.turn ? turnLine(session.turn, now) : '', t('sessions.action.bringUpTitle')].filter(Boolean).join(' · ')"
        @click="api.revealClaude(project.path, session.id)"
      >
        <span class="session-mark"><ClaudeLogo :size="12" /></span>
        <span class="session-name">{{ nameOf(session.id) }}</span>
      </button>
      <div v-if="project.claude && !waiting.length" class="card-session">
        <span class="session-mark"><ClaudeLogo :size="12" /></span>
        <span class="session-lines"><span class="session-turn">{{ turnLine(project.claude, now) }}</span></span>
        <button type="button" class="claude-open" @click="api.markSeen(project.path)">{{ t("detail.markSeen") }}</button>
      </div>
      <div v-for="session in working" :key="session.id" class="card-session working" :title="`${nameOf(session.id)}: ${t('detail.claudeWorking')}`">
        <span class="session-mark"><span class="ring" aria-hidden="true"></span></span>
        <span class="session-lines"><span class="session-name">{{ nameOf(session.id) }}</span></span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.card {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: rgba(22, 24, 29, 0.94);
}

.card-head {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.card-name {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.card-git {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The server's state over its command, its start or stop beside them. */
.card-server {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.server-text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
  white-space: nowrap;
}

.state {
  flex-shrink: 0;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  border: 1.5px solid var(--idle-ring);
}

.state.running {
  border: 0;
  background: var(--run);
  box-shadow: 0 0 0 3px rgba(115, 201, 145, 0.15);
}

.state.crashed {
  border: 0;
  background: var(--crash);
}

.state.busy {
  border-color: var(--text-muted);
}

/* The dot sits on the title's line; the line below lines up with the title text. */
.server-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 500;
}

.server-sub {
  min-width: 0;
  padding-left: 16px;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.server-sub.bad {
  color: var(--crash-text);
}

.card-actions {
  display: flex;
  gap: 6px;
  min-width: 0;
}

.control {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
}

.control:hover:not(:disabled) {
  background: #2c3039;
}

.control svg {
  flex-shrink: 0;
}

/* Start, stop and the wait between them, coloured as in the project's details. */
.control.start {
  background: #1d2a22;
  border-color: #2d4537;
  color: #b9e6c9;
}

.control.stop {
  background: #2a1b1b;
  border-color: #4a2b2b;
  color: #f2b8b8;
}

.control.busy {
  color: var(--text-muted);
  opacity: 0.6;
}

.control.square {
  width: 28px;
  padding: 0;
}

/* A long label gives way before the button beside it does. */
.control.grow {
  flex: 1 1 auto;
}

.control-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.control.go {
  justify-content: space-between;
}

kbd {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

/* The sessions, a line each: its mark, its name, and across from it the way to one that waits. */
.card-sessions {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.card-session {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-size: 12px;
}

/* Claude's logo or the ring of one at work, in one column. */
.session-mark {
  flex-shrink: 0;
  display: flex;
  justify-content: center;
  width: 12px;
}

/* One that waits on you: a line as the others, Claude's logo and its name in Claude's colour, no
   ground under it; the whole line brings the session up. */
.card-session.waits {
  padding: 0;
  border: 0;
  background: none;
  text-align: left;
}

.card-session.waits .session-name {
  flex-grow: 1;
  font-weight: 400;
  color: var(--claude-text);
}

.card-session.waits:hover .session-name {
  color: #f3c29b;
}

/* A line's text. */
.session-lines {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.session-name,
.session-turn {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.session-name {
  font-weight: 500;
  color: var(--text-strong);
}

.card-session.working .session-name {
  font-weight: 400;
  color: var(--text-muted);
}

.session-turn {
  color: var(--claude-text);
}

.claude-open {
  flex-shrink: 0;
  height: 24px;
  padding: 0 8px;
  border: 1px solid #4a3523;
  border-radius: 6px;
  background: #2a1f17;
  color: #f3c29b;
  font-size: 12px;
}

.claude-open:hover {
  background: #36281d;
}

.ring {
  flex-shrink: 0;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  border: 1.5px solid var(--claude);
  animation: card-breathe 1.6s ease-in-out infinite;
}

@keyframes card-breathe {
  50% {
    opacity: 0.35;
  }
}
</style>
