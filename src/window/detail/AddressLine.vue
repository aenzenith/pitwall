<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { bareUrl, phase } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { Project } from "../../lib/types";
import SwapLabel from "../team/SwapLabel.vue";

/**
 * The dev server's address line, in the project's details and on the Track page's card: the
 * address is the way to the browser, as a link's row is (`null`: unknown, the button says what it
 * does instead); the server's own buttons close the line. `compact`: the card's smaller sizes.
 */
const props = defineProps<{ project: Project; address: string | null; compact?: boolean }>();

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
  <div :class="['address-row', { compact }]">
    <button type="button" class="address" :title="t('common.openInBrowser')" @click="api.openBrowser(project.path)">
      <Icon name="external" :size="compact ? 12 : 13" />
      <span :class="['address-text', { named: !address }]">{{ address ? bareUrl(address) : t("common.openInBrowser") }}</span>
    </button>
    <button
      v-if="address"
      type="button"
      class="tool"
      :aria-label="t('link.copyAddress')"
      :title="t(copied ? 'link.copied' : 'link.copyAddress')"
      @click="copyAddress(address)"
    >
      <Icon :name="copied ? 'check' : 'copy'" :size="13" />
    </button>
    <button v-if="project.status === 'running'" type="button" class="tool" :aria-label="t('common.restart')" :title="t('common.restart')" @click="api.act(project.path, 'restart')">
      <Icon name="restart" :size="compact ? 13 : 14" />
    </button>
    <!-- Start, its spinner and Stop in one width: the button doesn't move under the pointer. -->
    <button v-if="project.status === 'busy'" type="button" class="toggle" :aria-label="phase(project)" disabled>
      <SwapLabel :labels="toggleLabels" :show="null" />
    </button>
    <button v-else-if="project.status === 'running'" type="button" class="toggle stop" @click="toggleServer">
      <SwapLabel :labels="toggleLabels" show="stop" />
    </button>
    <button v-else type="button" class="toggle start" @click="toggleServer">
      <SwapLabel :labels="toggleLabels" show="start" />
    </button>
  </div>
</template>

<style scoped>
/* Out to its block's edges by the address's padding: its icon stands under the state's dot. */
.address-row {
  margin: 0 -8px;
  display: flex;
  align-items: center;
  gap: 2px;
}

.address-row.compact {
  margin: 0 -6px;
  gap: 1px;
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

.compact .address {
  height: 28px;
  padding: 0 6px;
  gap: 8px;
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

.compact .address-text {
  font-size: 12px;
}

/* No address known: the button says what it does instead. */
.address-text.named {
  font-family: inherit;
  font-size: 13px;
}

.compact .address-text.named {
  font-size: 12px;
}

/* Copy and restart: an icon with no box. */
.tool {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
}

.compact .tool {
  width: 24px;
  height: 24px;
}

.tool:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
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

.compact .toggle {
  height: 24px;
  padding: 0 8px;
  font-size: 12px;
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
</style>
