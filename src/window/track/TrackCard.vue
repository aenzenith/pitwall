<script setup lang="ts">
import { computed } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import Marquee from "../../components/Marquee.vue";
import { serverLines, useAddress } from "../../lib/address";
import { filesShown, toggleFiles } from "../../lib/explorer";
import { editorName, gitLine, heldLine, turnLine } from "../../lib/format";
import { t } from "../../lib/i18n";
import { keys } from "../../lib/platform";
import { sessionTitle } from "../../lib/sessions";
import { api, now, sessions, snapshot } from "../../lib/store";
import { sessionsIn } from "../../lib/track";
import type { Project } from "../../lib/types";
import AddressLine from "../detail/AddressLine.vue";

/** The selected project, under the tower: its server, what Claude waits with, and the way to it.
 * Laid out as the project's details on the Projects page are (detail/DetailHeader, ServerBlock). */
const props = defineProps<{ project: Project }>();
const emit = defineEmits<{ open: [path: string] }>();

const editorLabel = computed(() => t("detail.openInEditorName", { editor: editorName(snapshot.value?.settings.editor) }));

const address = useAddress(() => props.project);
const server = computed(() => serverLines(props.project, address.value, now.value));

/** The commands that started with the server and wait for the user: a box each, under it. */
const held = computed(() => props.project.commands.flatMap((command) => (command.held ? [{ ...command, why: heldLine(command.held) }] : [])));

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
</script>

<template>
  <section class="card" :aria-label="project.name">
    <div class="card-head">
      <!-- The way to the project (↵ in the tower), its editor and its files, as the details' head has them. -->
      <div class="card-title">
        <span class="card-name">{{ project.name }}</span>
        <button type="button" class="icon" :aria-label="t('switcher.goToProject')" :title="t('switcher.goToProject')" @click="emit('open', project.path)">
          <Icon name="arrow-up-right" :size="14" />
        </button>
        <button type="button" class="icon" :aria-label="editorLabel" :title="editorLabel" @click="api.openEditor(project.path)">
          <Icon name="editor" :size="14" />
        </button>
        <button
          type="button"
          :class="['icon', { on: filesShown(project.path) }]"
          :aria-pressed="filesShown(project.path)"
          data-files-toggle
          :aria-label="t('files.title')"
          :title="t('files.toggle', { keys: keys('mod+Shift+E') })"
          @click="toggleFiles(project.path)"
        >
          <Icon name="folder" :size="14" />
        </button>
      </div>
      <span v-if="project.git" class="card-git">{{ gitLine(project.git) }}</span>
    </div>

    <div class="server-text">
      <span class="server-title"><span :class="['state', project.status]" aria-hidden="true"></span>{{ server.title }}</span>
      <span :class="['server-sub', { bad: project.status === 'crashed' }]"><Marquee :text="server.sub" /></span>
    </div>
    <AddressLine :project="project" :address="address" compact />

    <!-- A command that started with the server and waits: what it is and why in its tooltip, the
         way to run it or let it be. -->
    <div v-for="command in held" :key="command.id" class="card-held" role="group" :aria-label="t('detail.held.name', { name: command.name })" :title="`${command.command} · ${command.why}`">
      <span class="held-head">
        <Icon name="clock" :size="12" />
        <span class="held-name">{{ t("detail.held.name", { name: command.name }) }}</span>
      </span>
      <div class="card-actions">
        <button type="button" class="control half" @click="api.skipHeld(project.path, command.id)"><span class="control-text">{{ t("detail.skip") }}</span></button>
        <button type="button" :class="['control', 'half', { run: command.held === 'user' }]" @click="api.runCommand(project.path, command.id)">
          <span class="control-text">{{ t("common.run") }}</span>
        </button>
      </div>
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
  /* The panel's ground on the stage, the circuit showing through a little. */
  background: var(--bg-stage-card);
}

.card-head {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

/* The name, and its three buttons at the card's right edge. */
.card-title {
  display: flex;
  align-items: center;
  gap: 1px;
  min-width: 0;
  height: 22px;
  margin-right: -6px;
}

.icon {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
}

.icon:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* The files' window shows this project. */
.icon.on {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

.card-name {
  flex-grow: 1;
  min-width: 0;
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

/* The server's state over its command; its address line under them (detail/AddressLine). */
.server-text {
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
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--run) 15%, transparent);
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
  background: var(--bg-control-hover);
}

.control svg {
  flex-shrink: 0;
}

.control-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* A command that waits beside the server: its name in Claude's soft colour over its two buttons,
   in a box of its own; a name longer than the box wraps, and the box grows. */
.card-held {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-tile);
}

.held-head {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  min-width: 0;
  color: var(--claude-soft);
}

/* Level with the name's first line. */
.held-head svg {
  flex-shrink: 0;
  margin-top: 2px;
}

.held-name {
  min-width: 0;
  overflow-wrap: anywhere;
  font-weight: 500;
  line-height: 16px;
}

/* The box's room goes to the two by what their labels need: Skip is the longer one in German. */
/* Skip and Run share the row, neither narrower than its name. */
.control.half {
  flex: 1 0 auto;
}

.control.run {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.run:hover {
  background: var(--btn-primary-hover);
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
  color: var(--claude-soft);
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
  border: 1px solid var(--btn-claude-line);
  border-radius: 6px;
  background: var(--btn-claude-bg);
  color: var(--claude-soft);
  font-size: 12px;
}

.claude-open:hover {
  background: var(--btn-claude-hover);
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
