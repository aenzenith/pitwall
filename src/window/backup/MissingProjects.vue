<script setup lang="ts">
import { nextTick, onBeforeUnmount, reactive, ref, watch } from "vue";

import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { MissingProject } from "../../lib/types";
import { counted } from "./counts";
import PathText from "./PathText.vue";

/**
 * Settings › General's moved folders: the projects whose folder is gone, each with what is still
 * kept for it. Show new location has the core ask for the folder and moves everything there;
 * Forget asks once in the row, then drops what is kept. Nothing is drawn while there are none.
 * `active`: the tab is open; they are asked for then, and again after each action.
 */
const props = defineProps<{ active: boolean }>();

const projects = ref<MissingProject[]>([]);
/** Asks so far: an answer to an earlier one is older than what shows. */
let asked = 0;

async function load(): Promise<void> {
  const mine = ++asked;
  try {
    const found = await api.missingProjects();
    if (mine === asked) projects.value = found ?? [];
  } catch {
    // Kept as it was: a core that doesn't answer shows no section.
  }
}

watch(() => props.active, (on) => on && void load(), { immediate: true });

// A folder may come back, or go, while the window is away.
function onFocus(): void {
  if (props.active) void load();
}
window.addEventListener("focus", onFocus);
onBeforeUnmount(() => window.removeEventListener("focus", onFocus));

defineExpose({ load });

/** By path: the action under way and the last failure. */
const busy = reactive<Record<string, "rehome" | "forget">>({});
const errors = reactive<Record<string, string>>({});
/** The project whose Forget is being asked about. */
const asking = ref<string | null>(null);

/** `3 commands, 2 links, 4 cards kept`; empty when nothing but its place in the list is. */
function kept(project: MissingProject): string {
  const what = counted([
    ["backup.count.commands", project.commands],
    ["backup.count.links", project.links],
    ["backup.count.cards", project.cards],
  ]);
  return what ? t("missing.kept", { what }) : "";
}

async function act(path: string, action: "rehome" | "forget"): Promise<void> {
  if (busy[path]) return;
  busy[path] = action;
  delete errors[path];
  try {
    // Rehome: null when the folder dialog was cancelled, and the row stays.
    await (action === "rehome" ? api.rehomeProject(path) : api.forgetProject(path));
    if (asking.value === path) asking.value = null;
  } catch (reason) {
    errors[path] = String(reason);
  } finally {
    delete busy[path];
  }
  await load();
}

/** The row's buttons change under the keyboard: it goes on to Cancel when asked, and back to
 * Forget when that is called off. */
async function ask(event: MouseEvent, path: string | null): Promise<void> {
  const row = (event.currentTarget as HTMLElement).closest(".row");
  asking.value = path;
  await nextTick();
  row?.querySelector<HTMLElement>("[data-focus]")?.focus();
}
</script>

<template>
  <section v-if="projects.length" class="missing" :aria-label="t('missing.section')">
    <slot />
    <div v-for="project in projects" :key="project.path" class="row">
      <span class="dot" aria-hidden="true"></span>
      <span class="text">
        <span class="line">
          <Rich :text="t('missing.notFound')">
            <template #path><PathText class="where" :path="project.path" :keep="1" /></template>
          </Rich>
        </span>
        <span v-if="errors[project.path]" class="bad" role="alert">{{ errors[project.path] }}</span>
        <span v-else-if="asking === project.path">{{ t("missing.forgetAsk") }}</span>
        <span v-else-if="kept(project)">{{ kept(project) }}</span>
      </span>

      <!-- Asked once, here in the row: keeping it is the safe answer, and the one the keyboard is on.
           Each button holds, unseen, the label of the one it trades places with (`width`): the
           two are as wide, and the row's buttons stay where they are. -->
      <template v-if="asking === project.path">
        <button type="button" class="btn danger" :disabled="!!busy[project.path]" :aria-busy="busy[project.path] === 'forget'" @click="act(project.path, 'forget')">
          <span :class="{ hidden: busy[project.path] === 'forget' }">{{ t("missing.forget") }}</span>
          <span class="width" aria-hidden="true">{{ t("missing.rehome") }}</span>
          <Spinner v-if="busy[project.path] === 'forget'" class="over" :size="11" />
        </button>
        <button type="button" class="btn" data-focus :disabled="!!busy[project.path]" @click="ask($event, null)">
          <span>{{ t("common.cancel") }}</span>
          <span class="width" aria-hidden="true">{{ t("missing.forget") }}</span>
        </button>
      </template>
      <template v-else>
        <button type="button" class="btn" :disabled="!!busy[project.path]" :aria-busy="busy[project.path] === 'rehome'" @click="act(project.path, 'rehome')">
          <!-- The label keeps its room under the spinner: the button never changes width. -->
          <span :class="{ hidden: busy[project.path] === 'rehome' }">{{ t("missing.rehome") }}</span>
          <span class="width" aria-hidden="true">{{ t("missing.forget") }}</span>
          <Spinner v-if="busy[project.path] === 'rehome'" class="over" :size="11" />
        </button>
        <button type="button" class="btn quiet" data-focus :disabled="!!busy[project.path]" @click="ask($event, project.path)">
          <span>{{ t("missing.forget") }}</span>
          <span class="width" aria-hidden="true">{{ t("common.cancel") }}</span>
        </button>
      </template>
    </div>
  </section>
</template>

<style scoped>
.missing {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* Needs you: the attention yellow, as a dot and its ground. */
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px;
  border: 1px solid var(--warn-line);
  border-radius: 8px;
  background: var(--warn-bg);
}

.dot {
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  margin: 1px;
  border-radius: 50%;
  background: var(--warn);
}

/* Gives way to the buttons beside it. */
.text {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12px;
  line-height: 1.4;
  color: var(--warn-text);
}

/* One line: the words stay, the path gives way. */
.line {
  display: flex;
  gap: 5px;
  white-space: nowrap;
}

.where {
  flex: 0 1000 auto;
}

.bad {
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

/* A label and the one it trades places with share one cell: the button is as wide as the wider. */
.btn {
  position: relative;
  flex-shrink: 0;
  display: inline-grid;
  align-items: center;
  justify-items: center;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
}

.btn:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.btn.quiet {
  border-color: transparent;
  background: transparent;
  color: var(--text-muted);
}

.btn.quiet:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.btn.danger {
  color: var(--crash-text);
}

.btn:disabled {
  opacity: 0.5;
}

.btn > span {
  grid-area: 1 / 1;
}

.hidden,
.width {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}
</style>
