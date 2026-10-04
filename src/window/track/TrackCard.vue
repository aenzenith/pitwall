<script setup lang="ts">
import { computed } from "vue";

import ClaudeLogo from "../../components/ClaudeLogo.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { gitLine, phase, turnLine } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, now } from "../../lib/store";
import type { Project } from "../../lib/types";

/** The selected project, under the tower: its server, what Claude waits with, and the way to it. */
const props = defineProps<{ project: Project }>();
const emit = defineEmits<{ open: [path: string] }>();

const server = computed(() => {
  const p = props.project;
  const title = p.status === "busy" ? phase(p) : t(`status.${p.status}`);
  if (p.issue && p.status !== "running") return { title, sub: p.issue.text };
  return { title, sub: [p.runCommand, p.status === "running" && p.port ? `:${p.port}` : ""].filter(Boolean).join(" · ") };
});

/** The session that waits on you, to bring up where it runs. */
const waiting = computed(() => props.project.claudeSessions.find((session) => session.phase === "waiting")?.id ?? null);

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
      <span :class="['state', project.status]" aria-hidden="true"></span>
      <span class="server-title">{{ server.title }}</span>
      <span :class="['server-sub', { bad: project.status === 'crashed' }]" :title="server.sub">{{ server.sub }}</span>
    </div>

    <div class="card-actions">
      <button v-if="project.status === 'busy'" type="button" class="control" :aria-label="server.title" disabled><Spinner :size="12" /></button>
      <button v-else-if="project.status === 'running'" type="button" class="control" @click="toggle"><Icon name="stop" :size="11" /> {{ t("common.stop") }}</button>
      <button v-else type="button" class="control" @click="toggle"><Icon name="play" :size="11" /> {{ t("common.start") }}</button>
      <button v-if="project.status === 'running'" type="button" class="control grow" @click="api.openBrowser(project.path)">
        <Icon name="external" :size="12" /> <span class="control-text">{{ t("common.openInBrowser") }}</span>
      </button>
    </div>

    <div v-if="project.claude" class="card-claude">
      <ClaudeLogo :size="12" />
      <span class="claude-text">{{ turnLine(project.claude, now) }}</span>
      <button v-if="waiting" type="button" class="claude-open" :title="t('sessions.action.bringUpTitle')" @click="api.revealClaude(project.path, waiting)">
        {{ t("sessions.action.bringUp") }}
      </button>
      <button v-else type="button" class="claude-open" @click="api.markSeen(project.path)">{{ t("detail.markSeen") }}</button>
    </div>
    <div v-else-if="project.claudeWorking" class="card-working"><span class="ring" aria-hidden="true"></span>{{ t("detail.claudeWorking") }}</div>

    <button type="button" class="control go" @click="emit('open', project.path)">
      <span class="control-text">{{ t("switcher.goToProject") }}</span>
      <kbd aria-hidden="true">↵</kbd>
    </button>
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

.card-server {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-size: 12px;
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
}

.state.crashed {
  border: 0;
  background: var(--crash);
}

.state.busy {
  border-color: var(--text-muted);
}

.server-title {
  flex-shrink: 0;
}

.server-sub {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
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
  height: 30px;
}

kbd {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.card-claude {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 8px 8px 8px 10px;
  border: 1px solid var(--claude-line);
  border-radius: 8px;
  background: var(--claude-bg);
  font-size: 12px;
}

.card-claude svg {
  flex-shrink: 0;
}

.claude-text {
  flex-grow: 1;
  min-width: 0;
  color: var(--claude-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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

.card-working {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
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
