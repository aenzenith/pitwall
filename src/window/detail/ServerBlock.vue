<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import Marquee from "../../components/Marquee.vue";
import Spinner from "../../components/Spinner.vue";
import { editorName, phase, uptime } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, now, snapshot } from "../../lib/store";
import type { Project } from "../../lib/types";

/** `address`: where "Open in browser" goes, shown as its tooltip; `null` while unknown. */
const props = defineProps<{ project: Project; address: string | null }>();

const editor = computed(() => editorName(snapshot.value?.settings.editor));

const server = computed(() => {
  const p = props.project;
  const title = p.status === "busy" ? phase(p) : t(`status.${p.status}`);
  // The command the dev server runs, as the core runs it (npm, pnpm, yarn or bun).
  const sub = [p.runCommand];
  if (p.status === "running" && p.port) sub.push(`:${p.port}`);
  if (p.status === "running" && p.startedAt) sub.push(uptime(p.startedAt, now.value));
  return { title, sub: p.issue && p.status !== "running" ? p.issue.text : sub.filter(Boolean).join(" · ") };
});

function toggleServer(): void {
  void api.act(props.project.path, props.project.status === "running" ? "stop" : "start");
}
</script>

<template>
  <section class="block" :aria-label="t('common.devServer')">
    <div class="section-label">{{ t("common.devServer") }}</div>
    <div class="server-row">
      <div class="server-text">
        <span class="server-title"><span :class="['state-dot', project.status]"></span>{{ server.title }}</span>
        <span :class="['server-sub', { bad: project.status === 'crashed' }]"><Marquee :text="server.sub" /></span>
      </div>
      <button v-if="project.status === 'busy'" type="button" class="control busy" :aria-label="server.title" disabled>
        <Spinner :size="12" />
      </button>
      <button v-else-if="project.status === 'running'" type="button" class="control stop" @click="toggleServer">
        <Icon name="stop" :size="11" /> {{ t("common.stop") }}
      </button>
      <button v-else type="button" class="control start" @click="toggleServer">
        <Icon name="play" :size="11" /> {{ t("common.start") }}
      </button>
    </div>
    <div class="server-links">
      <button type="button" class="control link" :title="address ?? t('common.openInBrowser')" @click="api.openBrowser(project.path)">
        <Icon name="external" :size="13" />
        <span class="link-text">{{ t("common.openInBrowser") }}</span>
      </button>
      <button v-if="project.status === 'running'" type="button" class="control square" :aria-label="t('common.restart')" :title="t('common.restart')" @click="api.act(project.path, 'restart')">
        <Icon name="restart" :size="14" />
      </button>
      <button type="button" class="control fixed" @click="api.openEditor(project.path)"><Icon name="editor" :size="13" /> {{ editor }}</button>
    </div>
    <p v-if="!project.owner && project.openIn && project.status !== 'running'" class="note">{{ t("detail.openInWindow", { window: project.openIn }) }}</p>
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
.link-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Dev server: straight in the section, no card around it. */
.server-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.state-dot {
  width: 9px;
  height: 9px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 1.5px solid var(--idle-ring);
}

.state-dot.running {
  border: 0;
  background: var(--run);
  box-shadow: 0 0 0 4px rgba(115, 201, 145, 0.15);
}

.state-dot.crashed {
  border: 0;
  background: var(--crash);
}

.state-dot.busy {
  border-top-color: transparent;
  animation: spin 0.8s linear infinite;
}

.server-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
}

/* The dot sits on the title's line; the line below lines up with the title text. */
.server-title {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 500;
}

.server-sub {
  padding-left: 19px;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.server-sub.bad {
  color: var(--crash-text);
}

.server-links {
  display: flex;
  gap: 6px;
}

.note {
  margin: 0;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.5;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
