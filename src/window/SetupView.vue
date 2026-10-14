<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { shortcutLabel } from "../lib/format";
import { t, type Key } from "../lib/i18n";
import { dragRegion, isMac, isWindows } from "../lib/platform";
import { api, snapshot } from "../lib/store";
import { tabKey } from "../lib/tabs";
import type { ExtensionStatus } from "../lib/types";
import StepAlerts from "./setup/StepAlerts.vue";
import StepExtension from "./setup/StepExtension.vue";
import StepHook from "./setup/StepHook.vue";
import StepProjects from "./setup/StepProjects.vue";
import StepRadio from "./setup/StepRadio.vue";
import StepStartup from "./setup/StepStartup.vue";

/**
 * The first run: six steps beside the sidebar, where the project list and the details are
 * otherwise. A checklist, not a wizard: any step opens at any time, and one whose condition
 * holds shows a check. `covered`: Settings stands over it. `done`: the steps are over, finished or
 * put off; `openPhone`: Settings at the phone's part is asked for.
 */
defineProps<{ covered: boolean }>();
const emit = defineEmits<{ done: []; openPhone: [] }>();

type Step = "projects" | "hook" | "radio" | "extension" | "startup" | "alerts";

/** The team radio is not on Windows yet: nothing of it is shown there. */
const hasRadio = !isWindows;

const every: Array<{ id: Step; title: Key; hint: Key }> = [
  { id: "projects", title: "setup.step.projects", hint: "setup.step.projectsHint" },
  { id: "hook", title: "setup.step.hook", hint: "setup.step.hookHint" },
  { id: "radio", title: "setup.step.radio", hint: "setup.step.radioHint" },
  { id: "extension", title: "setup.step.extension", hint: "setup.step.extensionHint" },
  { id: "startup", title: "setup.step.startup", hint: "setup.step.startupHint" },
  // The pitwall command is macOS's alone so far: elsewhere its line says nothing of it.
  { id: "alerts", title: "setup.step.alerts", hint: isMac ? "setup.step.alertsHint" : "setup.step.alertsHintNoCli" },
];
const steps = every.filter((item) => item.id !== "radio" || hasRadio);
const order = steps.map((item) => item.id);

/* ---------- Pitwall for VS Code ---------- */

/** Whether the chosen editor has the extension: the third step's, and its check in the rail. Read
 * again when the editor changes and when the window comes back (after installing it there). */
const extension = ref<ExtensionStatus | null>(null);
const checking = ref(false);
const checkFailed = ref(false);
/** Readings asked for so far: an answer to an older one is dropped. */
let checks = 0;

const editor = computed(() => snapshot.value?.settings.editor ?? "vscode");

async function checkExtension(): Promise<void> {
  const mine = ++checks;
  checking.value = true;
  try {
    const status = await api.extensionStatus(editor.value);
    if (mine !== checks) return;
    extension.value = status;
    checkFailed.value = !status;
    // Opened on this step only because the answer wasn't in yet: on to the next one.
    if (!chosen && step.value === "extension" && status?.version) step.value = "startup";
  } catch {
    if (mine !== checks) return;
    extension.value = null;
    checkFailed.value = true;
  } finally {
    if (mine === checks) checking.value = false;
  }
}

watch(editor, () => void checkExtension(), { immediate: true });
window.addEventListener("focus", checkExtension);
onBeforeUnmount(() => window.removeEventListener("focus", checkExtension));

/* ---------- the steps ---------- */

/** A step is done when its condition holds. The last two never are: they have none, and the
 * last one's button ends the run. */
const done = computed<Record<Step, boolean>>(() => ({
  projects: !!snapshot.value?.projects.length,
  hook: !!snapshot.value?.claudeHook && !snapshot.value.claudeHookOutdated,
  radio: !!snapshot.value?.settings.radio?.on,
  extension: !!extension.value?.version,
  startup: false,
  alerts: false,
}));

/** It opens on the first step still to do. */
const step = ref<Step>(order.find((id) => !done.value[id]) ?? "alerts");
/** A step was opened by hand, or by a step's own button: the view no longer picks one itself. */
let chosen = false;

function open(id: Step): void {
  chosen = true;
  step.value = id;
}

function hint(item: (typeof steps)[number]): string {
  return t(item.hint, { keys: shortcutLabel(snapshot.value?.settings.shortcutKeys ?? "") });
}

/** ↑/↓ (or ←/→), Home and End move along the rail. */
function onRailKey(event: KeyboardEvent, id: Step): void {
  const next = tabKey(event, order.length, order.indexOf(id));
  if (next !== null) open(order[next]);
}

const stage = ref<HTMLElement | null>(null);

/**
 * A step's own button finished it: the next one still to do opens. The keyboard goes with it
 * to that step's place in the rail, since the pane it was in is gone.
 */
function advance(from: Step): void {
  const later = order.slice(order.indexOf(from) + 1);
  const next = later.find((id) => !done.value[id]) ?? later[0];
  if (!next) return;
  const inside = !!stage.value?.contains(document.activeElement);
  open(next);
  if (inside) void nextTick(() => document.getElementById(`setup-tab-${next}`)?.focus());
}

/* ---------- leaving ---------- */

/** Which button is ending the run, while the core is told. */
const leaving = ref<"later" | "finish" | null>(null);

/** "Later" and "Done" both end the first run: the steps don't come up by themselves again. */
async function leave(by: "later" | "finish"): Promise<void> {
  if (leaving.value) return;
  leaving.value = by;
  try {
    await api.setOnboarded();
  } catch {
    // A core that doesn't take it keeps nobody here: the steps come up again at the next start.
  }
  leaving.value = null;
  emit("done");
}
</script>

<template>
  <section class="setup" :aria-label="t('setup.title')">
    <!-- Its heading and free space drag the window; the button stays clickable. -->
    <header class="bar" :data-tauri-drag-region="dragRegion">
      <div class="heading">{{ t("setup.title") }}</div>
      <button type="button" class="later" :disabled="!!leaving" :aria-busy="leaving === 'later'" @click="leave('later')">
        <span :class="{ hidden: leaving === 'later' }">{{ t("setup.later") }}</span>
        <Spinner v-if="leaving === 'later'" class="over" :size="11" />
      </button>
    </header>

    <div class="frame">
      <nav class="rail" role="tablist" :aria-label="t('setup.steps')" aria-orientation="vertical">
        <button
          v-for="(item, index) in steps"
          :id="`setup-tab-${item.id}`"
          :key="item.id"
          type="button"
          role="tab"
          :aria-selected="step === item.id"
          :aria-controls="`setup-pane-${item.id}`"
          :tabindex="step === item.id ? 0 : -1"
          :class="{ on: step === item.id, done: done[item.id] }"
          @click="open(item.id)"
          @keydown="onRailKey($event, item.id)"
        >
          <span class="num">
            <Icon v-if="done[item.id]" name="check" :size="12" />
            <template v-else>{{ index + 1 }}</template>
          </span>
          <span class="step-text">
            <span class="step-title">
              {{ t(item.title) }}
              <span v-if="done[item.id]" class="sr-only">({{ t("setup.stepDone") }})</span>
            </span>
            <span class="step-hint">{{ hint(item) }}</span>
          </span>
        </button>
      </nav>

      <!-- Every pane stays alive under its tab: what a step read or ticked is there on the way back. -->
      <div ref="stage" class="stage">
        <StepProjects v-show="step === 'projects'" id="setup-pane-projects" role="tabpanel" aria-labelledby="setup-tab-projects" @added="advance('projects')" />
        <StepHook v-show="step === 'hook'" id="setup-pane-hook" role="tabpanel" aria-labelledby="setup-tab-hook" />
        <StepRadio v-if="hasRadio" v-show="step === 'radio'" id="setup-pane-radio" role="tabpanel" aria-labelledby="setup-tab-radio" />
        <StepExtension
          v-show="step === 'extension'"
          id="setup-pane-extension"
          role="tabpanel"
          aria-labelledby="setup-tab-extension"
          :editor="editor"
          :status="extension"
          :checking="checking"
          :failed="checkFailed"
          @recheck="checkExtension"
        />
        <StepStartup v-show="step === 'startup'" id="setup-pane-startup" role="tabpanel" aria-labelledby="setup-tab-startup" @next="advance('startup')" />
        <StepAlerts
          v-show="step === 'alerts'"
          id="setup-pane-alerts"
          role="tabpanel"
          aria-labelledby="setup-tab-alerts"
          :shown="step === 'alerts'"
          :covered="covered"
          :finishing="leaving === 'finish'"
          @finish="leave('finish')"
          @open-phone="emit('openPhone')"
        />
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Takes the project list's and the details' place, right of the sidebar. */
.setup {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

/* The same height and line as the projects toolbar, so switching never moves them. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.heading {
  flex-grow: 1;
  min-width: 0;
  font-size: 15px;
  font-weight: 600;
}

/* Putting it off is always there, and quiet: a text button, no box until pointed at. */
.later {
  position: relative;
  flex-shrink: 0;
  height: 24px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.later:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.later .hidden {
  visibility: hidden;
}

.later .over {
  position: absolute;
  inset: 0;
  margin: auto;
}

.frame {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  container-type: inline-size;
}

/* The steps: a list of rows, each a number (a check once done), a name and a line under it. */
.rail {
  width: 300px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 16px 12px;
  border-right: 1px solid var(--line);
  overflow-y: auto;
}

/* A row of text, not a push button: its lines wrap. */
.rail button {
  flex-shrink: 0;
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px;
  border: 0;
  border-radius: var(--radius-card);
  background: transparent;
  text-align: left;
  white-space: normal;
}

.rail button:hover {
  background: var(--bg-row-hover);
}

/* The open step, as the project list's selected row. */
.rail button.on {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

.num {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1.5px solid var(--idle-ring);
  border-radius: 50%;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.on .num {
  border-color: var(--text-soft);
  color: var(--text);
}

/* Done: the check in green, on the ground Settings' "on" badge has. */
.done .num {
  border: 0;
  background: var(--run-bg);
  color: var(--run);
}

.step-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.step-title {
  font-weight: 500;
  color: var(--text-nav);
}

.on .step-title,
.done .step-title {
  color: var(--text);
}

.step-hint {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-subtle);
}

/* The open step. Its panes ask how wide it is (setup/setup.css). */
.stage {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  display: flex;
  container: setup-step / inline-size;
}

/* A narrow window: the rail gives its room to the step. */
@container (max-width: 820px) {
  .rail {
    width: 232px;
  }
}
</style>
