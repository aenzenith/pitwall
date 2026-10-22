<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { useAddress } from "../lib/address";
import { t } from "../lib/i18n";
import { chooseTab, chosenTab, panelPath, showOutput } from "../lib/panel";
import { forgetProjectSettings, projectSettings, updateProjectSettings } from "../lib/projectSettings";
import { api } from "../lib/store";
import type { CustomCommand, LinkSuggestion, Project } from "../lib/types";
import CommandDialog from "./CommandDialog.vue";
import ClaudeCard from "./detail/ClaudeCard.vue";
import CommandsBlock from "./detail/CommandsBlock.vue";
import DetailHeader from "./detail/DetailHeader.vue";
import LinksBlock from "./detail/LinksBlock.vue";
import ProjectSettingsPane from "./detail/ProjectSettingsPane.vue";
import ServerBlock from "./detail/ServerBlock.vue";

const props = defineProps<{ project: Project }>();

const settingsOpen = ref(false);
/** The command whose output the terminal panel shows (lib/panel), its row lit; null for none. */
const tab = computed(() => {
  const chosen = chosenTab.value[props.project.path];
  return panelPath.value === props.project.path && chosen?.kind === "output" ? chosen.job : null;
});
/** A command waiting for "Run?" to be confirmed. Kept here, so it outlasts a visit to the settings. */
const confirming = ref<string | null>(null);
/** Where "Open in browser" goes. */
const address = useAddress(() => props.project);
/** Addresses the project names itself (git remote, .env, package.json); read once per project. */
const found = ref<LinkSuggestion[]>([]);
/** The add/edit dialog: open with the command being edited, or `null` for a new one. */
const dialog = ref<{ command: CustomCommand | null; running: boolean } | null>(null);

const commands = computed(() => props.project.commands ?? []);
/** The project's own settings, with a change not in the snapshot yet on top (lib/projectSettings). */
const settings = computed(() => projectSettings(props.project));

/* ---------- output ---------- */

/** A command's output (its row's button, a run): in the terminal panel, open. */
function showTab(id: string): void {
  showOutput(props.project.path, id);
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
  if (tab.value === id) chooseTab(props.project.path, { kind: "terminal", id: -1 });
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
  },
  { immediate: true },
);
</script>

<template>
  <section class="detail" :aria-label="t('window.projectDetails')">
    <DetailHeader :project="project" :settings-open="settingsOpen" @toggle="toggleSettings" />

    <ProjectSettingsPane v-if="settingsOpen" :project="project" @edit="editCommand" @remove="removeCommand" @close="settingsOpen = false" />

    <!-- The sections scroll on their own; the outputs are the terminal panel's (TerminalPanel). -->
    <div v-else class="stage">
      <div class="sections">
        <ServerBlock :project="project" :address="address" />
        <ClaudeCard :project="project" />
        <LinksBlock :project="project" :found="found" />
        <CommandsBlock v-model:confirming="confirming" :project="project" :tab="tab" @show="showTab" @edit="editCommand" @remove="removeCommand" />
      </div>
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

/* Everything under the header; the sections scroll in it, 24px free under their end. */
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
  /* Room under the end for the terminal panel lying over the page (WindowApp: `--panel-over`). */
  padding-bottom: calc(24px + var(--panel-over, 0px));
  scroll-padding-bottom: var(--panel-over, 0px);
}
/* Its scrollbar ends where the panel begins: the list keeps its size, only the bar's track is
   shorter (WindowApp: `--panel-over`). */
.sections::-webkit-scrollbar-track {
  margin-bottom: var(--panel-over, 0px);
}


</style>
