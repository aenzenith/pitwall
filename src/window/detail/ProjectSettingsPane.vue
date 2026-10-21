<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";
import { projectSettings, updateProjectSettings } from "../../lib/projectSettings";
import { MODE_LABELS, MODEL_LABELS, MODELS, MODES, projectChoice } from "../../lib/start";
import { snapshot } from "../../lib/store";
import { codes } from "../../lib/track";
import type { ClaudeMode, ClaudeModel, Project } from "../../lib/types";

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

/** How a Claude session started in this project begins unless something else is picked for it:
 * a board card given, "New Claude". Kept as it is picked, as the commands are. */
const claude = computed(() => projectChoice(props.project));

/** The choice that is Claude Code's own is left out of the settings: no model, the normal mode. */
function setModel(value: string): void {
  const model = MODELS.find((known) => known === value);
  void updateProjectSettings(props.project.path, (s) => ({ ...s, claudeModel: model satisfies ClaudeModel | undefined }));
}

function setMode(value: string): void {
  const mode = MODES.find((known) => known === value && known !== "normal");
  void updateProjectSettings(props.project.path, (s) => ({ ...s, claudeMode: mode satisfies ClaudeMode | undefined }));
}

/** The three letters the Track page and the mini tower write for the project when none is set:
 * told from its name, beside the others' (lib/track). */
const nameCode = computed(() => {
  const projects = (snapshot.value?.projects ?? []).map((project) => (project.path === props.project.path ? { ...project, settings: { ...project.settings, code: undefined } } : project));
  return codes(projects).get(props.project.path) ?? "";
});

/** The code as typed: letters and digits in capitals, three at most, as the core keeps it. */
const code = ref(settings.value.code ?? "");
/** The project that has the typed code set already. */
const codeTakenBy = ref<string | null>(null);

/** Not while an input method composes (kana, pinyin): what it holds is put right once it ends. */
function typeCode(event: Event): void {
  if ((event as InputEvent).isComposing) return;
  const input = event.target as HTMLInputElement;
  code.value = [...input.value.toLocaleUpperCase("en").replace(/[^\p{L}\p{N}]/gu, "")].slice(0, 3).join("");
  input.value = code.value;
  codeTakenBy.value = null;
}

/** Kept as it is left, as the picks above are; an empty field goes back to the name's letters. */
function saveCode(): void {
  const value = code.value || undefined;
  if (value === settings.value.code) return;
  const other = value ? snapshot.value?.projects.find((project) => project.path !== props.project.path && project.settings.code === value) : undefined;
  if (other) {
    codeTakenBy.value = other.name;
    return;
  }
  void updateProjectSettings(props.project.path, (s) => ({ ...s, code: value }));
}

/** What a double click (or Return) on the project's row opens: its Files window unless the
 * editor is picked; only the editor is written down. */
function setDoubleClick(value: string): void {
  void updateProjectSettings(props.project.path, (s) => ({ ...s, doubleClick: value === "editor" ? "editor" : undefined }));
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

    <section class="group" :aria-label="t('settings.claude')">
      <div class="section-label">{{ t("settings.claude") }}</div>
      <div class="picks">
        <label class="pick">
          <span class="pick-name">{{ t("start.model") }}</span>
          <select :value="claude.model ?? ''" @change="setModel(($event.target as HTMLSelectElement).value)">
            <option value="">{{ t("start.model.auto") }}</option>
            <option v-for="model in MODELS" :key="model" :value="model">{{ t(MODEL_LABELS[model]) }}</option>
          </select>
        </label>
        <label class="pick">
          <span class="pick-name">{{ t("start.mode") }}</span>
          <select :value="claude.mode" @change="setMode(($event.target as HTMLSelectElement).value)">
            <option v-for="mode in MODES" :key="mode" :value="mode">{{ t(MODE_LABELS[mode]) }}</option>
          </select>
        </label>
      </div>
    </section>

    <section class="group" :aria-label="t('track.nav')">
      <div class="section-label">{{ t("track.nav") }}</div>
      <div class="picks">
        <label class="pick">
          <span class="pick-name">{{ t("detail.code") }}</span>
          <input
            class="code"
            :value="code"
            :placeholder="nameCode"
            maxlength="3"
            spellcheck="false"
            autocomplete="off"
            :aria-invalid="codeTakenBy !== null"
            @input="typeCode"
            @compositionend="typeCode"
            @change="saveCode"
          />
        </label>
      </div>
      <p v-if="codeTakenBy" class="error" role="alert">{{ t("detail.codeTaken", { name: codeTakenBy }) }}</p>
    </section>

    <section class="group" :aria-label="t('detail.listSection')">
      <div class="section-label">{{ t("detail.listSection") }}</div>
      <div class="picks">
        <label class="pick">
          <span class="pick-name">{{ t("detail.doubleClick") }}</span>
          <select :value="settings.doubleClick ?? 'files'" @change="setDoubleClick(($event.target as HTMLSelectElement).value)">
            <option value="files">{{ t("detail.doubleClick.files") }}</option>
            <option value="editor">{{ t("detail.doubleClick.editor") }}</option>
          </select>
        </label>
      </div>
    </section>

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

      <p class="hint">{{ t("detail.commandsHint") }}</p>
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
  /* The line between two parts runs the pane's whole width, past its padding: the one line the
     pane draws, so it reads as where a part ends. */
  margin: 0 -20px;
  padding: 0 20px 18px;
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

/* A line each: what it is, and its choice at the line's end; no rule between them. */
.picks {
  display: flex;
  flex-direction: column;
}

.pick {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 36px;
  font-size: 13px;
}

.pick-name {
  flex: 1 1 auto;
  min-width: 0;
}

.pick select {
  flex-shrink: 0;
  max-width: 70%;
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text);
  font: inherit;
  font-size: 12px;
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
  background: var(--bg-tile);
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
  background: var(--bg-tag);
  color: var(--text-muted);
}

.tag.run {
  background: var(--run-bg);
  color: var(--run);
}

.tag.ask {
  background: var(--btn-claude-bg);
  color: var(--claude-soft);
}

.control.add {
  align-self: flex-start;
}

/* Three capitals, as the tower writes them. */
.pick input.code {
  flex-shrink: 0;
  width: 64px;
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-align: center;
}

.pick input.code::placeholder {
  color: var(--text-subtle);
  font-weight: 400;
}

.error {
  margin: 0;
  font-size: 12px;
  color: var(--crash-text);
}
</style>
