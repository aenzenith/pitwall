<script setup lang="ts">
import { computed } from "vue";

import Marquee from "../../components/Marquee.vue";
import { serverLines } from "../../lib/address";
import { t } from "../../lib/i18n";
import { now } from "../../lib/store";
import type { Project } from "../../lib/types";
import AddressLine from "./AddressLine.vue";

/** `address`: where "Open in browser" goes, shown on its line; `null` while unknown. */
const props = defineProps<{ project: Project; address: string | null }>();

const server = computed(() => serverLines(props.project, props.address, now.value));
</script>

<template>
  <section class="block" :aria-label="t('common.devServer')">
    <div class="section-label">{{ t("common.devServer") }}</div>
    <div class="server-text">
      <span class="server-title"><span :class="['state-dot', project.status]"></span>{{ server.title }}</span>
      <span :class="['server-sub', { bad: project.status === 'crashed' }]"><Marquee :text="server.sub" /></span>
    </div>
    <AddressLine :project="project" :address="address" />
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

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
