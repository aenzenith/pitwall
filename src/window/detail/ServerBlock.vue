<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Marquee from "../../components/Marquee.vue";
import { bareUrl, phase, uptime } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, now } from "../../lib/store";
import type { Project } from "../../lib/types";
import SwapLabel from "../team/SwapLabel.vue";

/** `address`: where "Open in browser" goes, shown on its line; `null` while unknown. */
const props = defineProps<{ project: Project; address: string | null }>();

const server = computed(() => {
  const p = props.project;
  const title = p.status === "busy" ? phase(p) : t(`status.${p.status}`);
  // The command the dev server runs, as the core runs it (npm, pnpm, yarn or bun).
  const sub = [p.runCommand];
  // The port, unless the address under it shows it already.
  if (p.status === "running" && p.port && !props.address?.includes(`:${p.port}`)) sub.push(`:${p.port}`);
  if (p.status === "running" && p.startedAt) sub.push(uptime(p.startedAt, now.value));
  return { title, sub: p.issue && p.status !== "running" ? p.issue.text : sub.filter(Boolean).join(" · ") };
});

/** The toggle's labels, all of them in its box: it keeps the widest one's width. */
const toggleLabels = computed(() => [
  { id: "start", text: t("common.start"), icon: "play" as const },
  { id: "stop", text: t("common.stop"), icon: "stop" as const },
]);

function toggleServer(): void {
  void api.act(props.project.path, props.project.status === "running" ? "stop" : "start");
}

/** The address was just copied: its button says so, for a moment. */
const copied = ref(false);
let copiedTimer = 0;

function copyAddress(url: string): void {
  void api.copyText(url);
  copied.value = true;
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = false), 1200);
}

onBeforeUnmount(() => window.clearTimeout(copiedTimer));
</script>

<template>
  <section class="block" :aria-label="t('common.devServer')">
    <div class="section-label">{{ t("common.devServer") }}</div>
    <div class="server-text">
      <span class="server-title"><span :class="['state-dot', project.status]"></span>{{ server.title }}</span>
      <span :class="['server-sub', { bad: project.status === 'crashed' }]"><Marquee :text="server.sub" /></span>
    </div>
    <!-- The address is the way to the browser, as a link's row is; the server's own buttons close the line. -->
    <div class="address-row">
      <button type="button" class="address" :title="t('common.openInBrowser')" @click="api.openBrowser(project.path)">
        <Icon name="external" :size="13" />
        <span :class="['address-text', { named: !address }]">{{ address ? bareUrl(address) : t("common.openInBrowser") }}</span>
      </button>
      <button
        v-if="address"
        type="button"
        class="icon small"
        :aria-label="t('link.copyAddress')"
        :title="t(copied ? 'link.copied' : 'link.copyAddress')"
        @click="copyAddress(address)"
      >
        <Icon :name="copied ? 'check' : 'copy'" :size="13" />
      </button>
      <button v-if="project.status === 'running'" type="button" class="icon small" :aria-label="t('common.restart')" :title="t('common.restart')" @click="api.act(project.path, 'restart')">
        <Icon name="restart" :size="14" />
      </button>
      <!-- Start, its spinner and Stop in one width: the button doesn't move under the pointer. -->
      <button v-if="project.status === 'busy'" type="button" class="toggle" :aria-label="server.title" disabled>
        <SwapLabel :labels="toggleLabels" :show="null" />
      </button>
      <button v-else-if="project.status === 'running'" type="button" class="toggle stop" @click="toggleServer">
        <SwapLabel :labels="toggleLabels" show="stop" />
      </button>
      <button v-else type="button" class="toggle start" @click="toggleServer">
        <SwapLabel :labels="toggleLabels" show="start" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
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
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--run) 15%, transparent);
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

/* Out to the section's edges by the address's padding: its icon stands under the dot. */
.address-row {
  margin: 0 -8px;
  display: flex;
  align-items: center;
  gap: 2px;
}

.address {
  height: 30px;
  flex-grow: 1;
  min-width: 0;
  padding: 0 8px;
  display: flex;
  align-items: center;
  gap: 9px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-soft);
  text-align: left;
}

.address svg {
  flex-shrink: 0;
  color: var(--text-subtle);
}

.address:hover {
  background: var(--bg-row-hover);
  color: var(--text-strong);
}

.address:hover svg {
  color: var(--text-strong);
}

.address-text {
  font-family: var(--font-mono);
  font-size: 12.5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* No address known: the button says what it does instead. */
.address-text.named {
  font-family: inherit;
  font-size: 13px;
}

/* Start and Stop: a word with no box; Stop takes its colour only under the pointer. */
.toggle {
  height: 26px;
  flex-shrink: 0;
  padding: 0 9px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12.5px;
  color: var(--text-muted);
}

.toggle.stop:hover {
  background: var(--crash-bg);
  color: var(--crash-text);
}

.toggle.start {
  color: var(--run-text);
}

.toggle.start:hover {
  background: var(--run-bg);
}

.toggle:disabled {
  opacity: 0.6;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
