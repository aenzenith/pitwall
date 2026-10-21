<script setup lang="ts">
import Icon from "../../components/Icon.vue";
import { filesShown, toggleFiles } from "../../lib/explorer";
import { editorName, gitLine } from "../../lib/format";
import { t } from "../../lib/i18n";
import { dragRegion, keys } from "../../lib/platform";
import { api, snapshot } from "../../lib/store";
import type { Project } from "../../lib/types";

defineProps<{ project: Project; settingsOpen: boolean }>();
/** The gear (or, in settings, the back button in its place) swaps details and settings. */
const emit = defineEmits<{ toggle: [] }>();
</script>

<template>
  <header v-if="settingsOpen" class="head bar" :data-tauri-drag-region="dragRegion">
    <!-- Settings mode: only the name and a back button; everything else waits for the way back.
         Settings: same height as the list's toolbar, so both bottom lines run as one. -->
    <div class="name">{{ t("detail.settingsHeading") }}</div>
    <!-- Same spot as the settings button, so it toggles in place. -->
    <button type="button" class="icon" :aria-label="t('detail.backToDetails')" :title="t('common.back')" @click="emit('toggle')">
      <Icon name="back" :size="16" />
    </button>
  </header>

  <header v-else class="head" :data-tauri-drag-region="dragRegion">
    <!-- Name, path and branch drag the window; the settings button stays clickable. -->
    <div class="title-row">
      <div class="name">{{ project.name }}</div>
      <!-- The project in the editor Settings › Editor names: it opens the project, not its server. -->
      <button
        type="button"
        class="icon"
        :aria-label="t('detail.openInEditorName', { editor: editorName(snapshot?.settings.editor) })"
        :title="t('detail.openInEditorName', { editor: editorName(snapshot?.settings.editor) })"
        @click="api.openEditor(project.path)"
      >
        <Icon name="editor" :size="16" />
      </button>
      <!-- The project's files in their floating window (files/FilesWindow); pressed while it shows them. -->
      <button
        type="button"
        :class="['icon', { on: filesShown(project.path) }]"
        :aria-pressed="filesShown(project.path)"
        data-files-toggle
        :aria-label="t('files.title')"
        :title="t('files.toggle', { keys: keys('mod+Shift+E') })"
        @click="toggleFiles(project.path)"
      >
        <Icon name="folder" :size="16" />
      </button>
      <button type="button" class="icon" :aria-label="t('detail.projectSettings')" :title="t('detail.projectSettings')" @click="emit('toggle')">
        <Icon name="settings" :size="16" />
      </button>
    </div>
    <div class="path" :title="project.path">{{ project.path }}</div>
    <div v-if="project.git" class="git">{{ gitLine(project.git) }}</div>
  </header>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
/* 94px: toolbar (56) + table header (38), so its line meets the table header's line.
   The 13px top puts the gear's centre at 28px, where the settings header's back button sits. */
.head {
  height: 94px;
  flex-shrink: 0;
  padding: 13px 16px 0 20px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  border-bottom: 1px solid var(--line);
}

/* Settings header matches the list toolbar (56px with its line), so the two bottom lines meet. */
.head.bar {
  height: 56px;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  padding: 0 16px 0 20px;
  border-bottom: 1px solid var(--line);
}

.title-row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.icon.on {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

.name {
  font-size: 17px;
  font-weight: 600;
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.head.bar .name {
  font-size: 15px;
}

.path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.git {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-soft);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
