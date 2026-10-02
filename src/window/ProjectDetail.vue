<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";

import { t } from "../lib/i18n";
import { useOutputStream } from "../lib/outputStream";
import { COLLAPSED_HEIGHT, outputCollapsed, outputHeight, outputRequest, terminalCollapsed, terminalHeight, useResizer } from "../lib/panel";
import { forgetProjectSettings, projectSettings, updateProjectSettings } from "../lib/projectSettings";
import { api } from "../lib/store";
import type { CustomCommand, LinkSuggestion, Project } from "../lib/types";
import CommandDialog from "./CommandDialog.vue";
import ClaudeCard from "./detail/ClaudeCard.vue";
import CommandsBlock from "./detail/CommandsBlock.vue";
import DetailHeader from "./detail/DetailHeader.vue";
import LinksBlock from "./detail/LinksBlock.vue";
import OutputPanel from "./detail/OutputPanel.vue";
import ProjectSettingsPane from "./detail/ProjectSettingsPane.vue";
import ServerBlock from "./detail/ServerBlock.vue";

const props = defineProps<{ project: Project }>();

const stage = ref<HTMLElement | null>(null);
const settingsOpen = ref(false);
/** Output tab: `null` is the dev server, otherwise a command id. */
const tab = ref<string | null>(null);
/** A command waiting for "Run?" to be confirmed. Kept here, so it outlasts a visit to the settings. */
const confirming = ref<string | null>(null);
/** Where "Open in browser" goes. */
const address = ref<string | null>(null);
/** Addresses the project names itself (git remote, .env, package.json); read once per project. */
const found = ref<LinkSuggestion[]>([]);
/** The add/edit dialog: open with the command being edited, or `null` for a new one. */
const dialog = ref<{ command: CustomCommand | null; running: boolean } | null>(null);

const commands = computed(() => props.project.commands ?? []);
/** The project's own settings, with a change not in the snapshot yet on top (lib/projectSettings). */
const settings = computed(() => projectSettings(props.project));

/* ---------- output ---------- */

// Kept here rather than in the panel: its lines outlast a visit to the settings.
const { lines, load: loadOutput } = useOutputStream(() => props.project.path, tab);

function showTab(id: string | null): void {
  tab.value = id;
  void loadOutput();
}

// The switcher asked for a command's output: that tab, the panel open. After the project
// switch has reset the tab, hence the tick.
watch(
  () => [outputRequest.value, props.project.path] as const,
  async ([request, path]) => {
    if (!request || request.path !== path) return;
    await nextTick();
    settingsOpen.value = false;
    outputCollapsed.value = false;
    showTab(request.job);
    outputRequest.value = null;
  },
  { immediate: true },
);

// The output keeps the height you gave it, whatever the sections above do. It can grow over
// them, up to the whole area under the header; the sections scroll on their own.
const resizer = useResizer(
  outputHeight,
  () => stage.value?.clientHeight ?? 0,
  () => (terminalCollapsed.value ? null : terminalHeight.value),
);
const outputDragging = resizer.dragging;

/** Collapsed, only the tab row shows; the chosen height comes back on expanding. */
const shownOutput = computed(() => (outputCollapsed.value ? COLLAPSED_HEIGHT : outputHeight.value));

/* ---------- dev server ---------- */

/** The newest `loadAddress`: an older answer, or one for another project, is dropped. */
let addressLoad = 0;

async function loadAddress(): Promise<void> {
  const load = ++addressLoad;
  const path = props.project.path;
  const resolved = await api.resolveAddress(path);
  if (load === addressLoad && path === props.project.path) address.value = resolved;
}

/* ---------- links ---------- */

async function loadSuggestions(path: string): Promise<void> {
  const list = await api.linkSuggestions(path);
  if (path === props.project.path) found.value = list;
}

watch(
  () => props.project.path,
  (path) => {
    found.value = [];
    void loadSuggestions(path);
  },
  { immediate: true },
);

/* ---------- commands ---------- */

/** Opens the dialog on the stored command (not its live view), or empty for a new one. */
function editCommand(id: string | null): void {
  const command = id ? storedCommands().find((c) => c.id === id) : undefined;
  if (id && !command) return;
  dialog.value = { command: command ?? null, running: commands.value.some((c) => c.id === id && c.status === "running") };
}

function storedCommands(): CustomCommand[] {
  return (settings.value.commands ?? []).map((c) => ({ ...c }));
}

/** Commands are saved on their own, so half-typed dev server fields aren't saved with them; every
 * change goes through the one writer, on top of the newest settings (lib/projectSettings). */
function changeCommands(change: (list: CustomCommand[]) => CustomCommand[]): void {
  void updateProjectSettings(props.project.path, (s) => ({ ...s, commands: change(s.commands ?? []) }));
}

function saveCommand(command: CustomCommand): void {
  if (!command.id) {
    const added = { ...command, id: `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}` };
    changeCommands((list) => [...list, added]);
  } else {
    changeCommands((list) => list.map((c) => (c.id === command.id ? command : c)));
  }
  dialog.value = null;
}

/** A running command is stopped too; once deleted it would have no row left to stop it from. */
function removeCommand(id: string): void {
  if (commands.value.some((c) => c.id === id && c.status === "running")) void api.stopCommand(props.project.path, id);
  changeCommands((list) => list.filter((c) => c.id !== id));
  if (tab.value === id) showTab(null);
}

/* ---------- project settings ---------- */

/** The gear swaps the panel body between the project's details and its settings. */
function toggleSettings(): void {
  settingsOpen.value = !settingsOpen.value;
}

/* ---------- lifecycle ---------- */

watch(
  () => props.project.path,
  (_, left) => {
    if (left) forgetProjectSettings(left);
    settingsOpen.value = false;
    dialog.value = null;
    confirming.value = null;
    tab.value = null;
    void loadOutput();
    void loadAddress();
  },
  { immediate: true },
);

watch(() => [props.project.status, props.project.url], () => void loadAddress());
</script>

<template>
  <section class="detail" :aria-label="t('window.projectDetails')">
    <DetailHeader :project="project" :settings-open="settingsOpen" @toggle="toggleSettings" />

    <ProjectSettingsPane v-if="settingsOpen" :project="project" @edit="editCommand" @remove="removeCommand" @close="settingsOpen = false" />

    <!-- The sections scroll on their own; the output sits over their lower part. -->
    <div v-else ref="stage" :class="['stage', { animated: !outputDragging }]">
      <div class="sections" :style="{ bottom: `${shownOutput}px` }">
        <ServerBlock :project="project" :address="address" />
        <ClaudeCard :project="project" />
        <LinksBlock :project="project" :found="found" />
        <CommandsBlock v-model:confirming="confirming" :project="project" :tab="tab" @show="showTab" @edit="editCommand" @remove="removeCommand" />
      </div>

      <OutputPanel :project="project" :tab="tab" :lines="lines" :height="shownOutput" :resizer="resizer" @show="showTab" />
    </div>

    <CommandDialog
      v-if="dialog"
      :command="dialog.command"
      :running="dialog.running"
      :project-name="project.name"
      @save="saveCommand"
      @close="dialog = null"
    />
  </section>
</template>

<style scoped>
.detail {
  width: 380px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
  min-height: 0;
  overflow: hidden;
}

/* Everything under the header. The sections scroll in the part above the output, so their
   scrollbar ends where the output begins; 24px stays free under their end. */
.stage {
  flex: 1 1 auto;
  min-height: 0;
  position: relative;
}

.sections {
  position: absolute;
  inset: 0;
  padding-bottom: 24px;
  overflow-y: auto;
}

/* Collapsing and expanding slide with the output (detail/OutputPanel); a drag follows the pointer. */
.stage.animated .sections {
  transition: bottom 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .stage.animated .sections {
    transition: none;
  }
}
</style>
