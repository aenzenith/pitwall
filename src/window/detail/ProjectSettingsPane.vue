<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import { t } from "../../lib/i18n";
import { projectSettings, updateProjectSettings } from "../../lib/projectSettings";
import type { Project } from "../../lib/types";

const props = defineProps<{ project: Project }>();
/** `edit`: the command dialog (`null` for a new command); `close`: saved, back to the details. */
const emit = defineEmits<{ edit: [id: string | null]; remove: [id: string]; close: [] }>();

/** The project's own settings, with a change not in the snapshot yet on top (lib/projectSettings). */
const settings = computed(() => projectSettings(props.project));

// Dev server fields, as the settings were when the pane opened.
const script = ref(settings.value.script ?? "");
const port = ref(settings.value.port ? String(settings.value.port) : "");
const url = ref(settings.value.url ?? "");

/** Only the dev server fields: commands are saved on their own, as they change. */
function saveSettings(): void {
  const parsed = Number(port.value);
  const server = {
    script: script.value.trim() || undefined,
    port: Number.isInteger(parsed) && parsed > 0 && parsed < 65536 ? parsed : undefined,
    url: url.value.trim() || undefined,
  };
  void updateProjectSettings(props.project.path, (s) => ({ ...s, ...server }));
  emit("close");
}
</script>

<template>
  <div class="settings-pane">
    <!-- Project settings, grouped: each per-project setting has its own fieldset. -->
    <form class="group" :aria-label="t('detail.serverSettings')" @submit.prevent="saveSettings">
      <div class="section-label">{{ t("common.devServer") }}</div>
      <label class="field">{{ t("common.script") }} <input v-model="script" class="mono" :placeholder="project.script" spellcheck="false" /></label>
      <label class="field">{{ t("detail.port") }} <input v-model="port" class="mono" inputmode="numeric" :placeholder="t('detail.portPlaceholder')" /></label>
      <label class="field">{{ t("detail.address") }} <input v-model="url" class="mono" :placeholder="t('detail.addressPlaceholder')" spellcheck="false" /></label>
      <p class="hint">{{ t("detail.settingsHint") }}</p>
      <div class="form-actions">
        <button type="submit" class="control primary">{{ t("common.save") }}</button>
      </div>
    </form>

    <section class="group" :aria-label="t('common.commands')">
      <div class="section-label">{{ t("common.commands") }}</div>

      <div v-for="c in settings.commands ?? []" :key="c.id" class="cmd-item">
        <div class="cmd-text">
          <span class="cmd-name">{{ c.name }}</span>
          <span class="cmd-line">{{ c.command }}</span>
        </div>
        <span v-if="c.keepRunning" class="tag run">{{ t("detail.tagKeepsRunning") }}</span>
        <span v-if="c.confirm" class="tag ask">{{ t("detail.tagAsksFirst") }}</span>
        <span v-if="c.withServer" class="tag">{{ t("detail.tagWithServer") }}</span>
        <button type="button" class="icon small" :aria-label="t('common.editName', { name: c.name })" :title="t('common.edit')" @click="emit('edit', c.id)">
          <Icon name="pencil" :size="13" />
        </button>
        <button type="button" class="icon small" :aria-label="t('common.deleteName', { name: c.name })" :title="t('common.delete')" @click="emit('remove', c.id)">×</button>
      </div>

      <button type="button" class="control add" @click="emit('edit', null)">{{ t("detail.addCommand") }}</button>

      <p class="hint">
        <Rich :text="t('detail.commandsHint')">
          <template #build><code>npm run build</code></template>
          <template #vite><code>vite build</code></template>
        </Rich>
      </p>
    </section>
  </div>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
.settings-pane {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 16px 20px 20px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.group {
  margin: 0;
  padding: 0 0 18px;
  border: 0;
  border-bottom: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group:last-child {
  border-bottom: 0;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-muted);
}

.field input {
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.field input.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.hint code {
  font-family: var(--font-mono);
}

.form-actions {
  display: flex;
  gap: 8px;
}

.cmd-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 6px 8px 10px;
  border-radius: 8px;
  background: #181b20;
  border: 1px solid var(--line);
}

.cmd-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
}

.tag {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: 9px;
  font-size: 11px;
  white-space: nowrap;
  background: #23262d;
  color: var(--text-muted);
}

.tag.run {
  background: #1d2a22;
  color: var(--run);
}

.tag.ask {
  background: #2a1f17;
  color: #f3c29b;
}

.control.add {
  align-self: flex-start;
}
</style>
