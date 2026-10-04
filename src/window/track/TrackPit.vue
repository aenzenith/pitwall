<script setup lang="ts">
import Icon from "../../components/Icon.vue";
import StatusIcon from "../../components/StatusIcon.vue";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { Project } from "../../lib/types";

/** The pit, under the circuit: the stopped projects, each a press away from the track. */
defineProps<{
  /** The stopped projects, in the tower's order. */
  projects: Project[];
  codes: Map<string, string>;
  selected: string | null;
  /** Something starts or stops: "all" waits its turn. */
  busy: boolean;
  /** Any project runs. */
  running: boolean;
}>();
const emit = defineEmits<{ select: [path: string] }>();
</script>

<template>
  <section class="pit" :aria-label="t('track.pit')">
    <span class="pit-head">
      <span class="section-label">{{ t("track.pit") }}</span>
      <span class="pit-count">{{ projects.length }}</span>
    </span>

    <!-- Only the boxes scroll, sideways, when there are more than the strip holds. -->
    <div class="pit-boxes">
      <div v-for="project in projects" :key="project.path" :class="['box', { on: project.path === selected }]">
        <button type="button" class="box-name" @click="emit('select', project.path)">
          <StatusIcon :project="project" ring="#15171c" />
          <span class="box-text">{{ project.name }}</span>
          <span class="box-code">{{ codes.get(project.path) }}</span>
        </button>
        <button type="button" class="box-start" :aria-label="t('common.startName', { name: project.name })" :title="t('common.start')" @click="api.act(project.path, 'start')">
          <Icon name="play" :size="12" />
        </button>
      </div>
    </div>

    <button v-if="projects.length" type="button" class="all" :disabled="busy" @click="api.startAll()"><Icon name="play" :size="12" /> {{ t("common.startAll") }}</button>
    <button v-if="running" type="button" class="all" :disabled="busy" @click="api.stopAll()"><Icon name="stop" :size="11" /> {{ t("common.stopAll") }}</button>
  </section>
</template>

<style scoped>
.pit {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 50px;
  padding: 0 8px 0 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(19, 21, 26, 0.9);
}

.pit-head {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.pit-count {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.pit-boxes {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  overflow-x: auto;
  overscroll-behavior: contain;
  scrollbar-width: none;
}

.pit-boxes::-webkit-scrollbar {
  display: none;
}

.box {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  height: 34px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--bg-sidebar);
}

.box.on {
  border-color: #5b626d;
}

.box-name {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 100%;
  padding: 0 4px 0 10px;
  border: 0;
  border-radius: 8px 0 0 8px;
  background: transparent;
}

.box-text {
  max-width: 160px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
}

.box-code {
  padding: 1px 5px;
  border-radius: 5px;
  background: var(--bg-control);
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
}

.box-start {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 100%;
  padding: 0;
  border: 0;
  border-radius: 0 8px 8px 0;
  background: transparent;
  color: #b4bac3;
}

.box-start:hover,
.all:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-strong);
}

.all {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: #b4bac3;
  font-size: 12px;
}

.all:disabled {
  opacity: 0.5;
}
</style>
