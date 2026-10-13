<script lang="ts">
import { ref } from "vue";

import type { RadioState, RadioTool } from "../../lib/types";

/** The tools there are, as the core lists them: they wait for nothing, so they are asked for
 * once, when the section is first made, and kept while the app runs. */
const tools = ref<RadioTool[]>([]);
/** What the core said when last asked: kept while the app runs, so the section opens as it was. */
const asked = ref<RadioState | null>(null);
/** The ask under way: one at a time, however often the section comes into view. */
let flying: Promise<void> | null = null;
/** Counted up as a turn on or off begins and ends: an answer asked for before then is not taken. */
let turns = 0;
</script>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from "vue";

import Spinner from "../../components/Spinner.vue";
import { t, type Key } from "../../lib/i18n";
import { isWindows } from "../../lib/platform";
import { api, snapshot } from "../../lib/store";

/**
 * Settings › Claude's team radio: on or off (Pitwall among Claude Code's MCP servers), and what
 * Claude may do over it, a switch per tool. Everything here is the radio's own: each change
 * goes to the core by its own command, never with the form (`@change.stop`).
 *
 * On or off and each tool's switch are the snapshot's, and the tools there are the core's own
 * list (`radio_tools`): all there at once, so the rows stand with the section. What only Claude
 * Code can tell (its CLI found or not, the sessions connected) is the core's answer
 * (`radio_state`), which only the badge and the line under the head wait for. It is asked each
 * time the section comes into view and after a change, never on a timer; the core answers from
 * what Claude Code said in the last half minute.
 */

/** What each tool does, by its name. Which tools there are is the core's to say. */
const TEXT: Record<string, Key> = {
  server_status: "radio.tool.server_status",
  server_output: "radio.tool.server_output",
  server_restart: "radio.tool.server_restart",
  run_command: "radio.tool.run_command",
  board_add: "radio.tool.board_add",
  board_note: "radio.tool.board_note",
};

const radio = computed(() => snapshot.value?.settings.radio);
const on = computed(() => !!radio.value?.on);
/** A tool's switch as it is stored. */
const allowed = (name: string): boolean => !!radio.value?.tools[name];
/** Being turned on or off: Claude Code's CLI is at work. */
const busy = ref(false);
const error = ref("");

// Asked for as the section is made, not when it comes into view: the rows are there by then.
if (!tools.value.length) {
  void api.radioTools().then(
    (listed) => listed && (tools.value = listed),
    () => undefined,
  );
}

/** Not on Windows yet: nothing of it is shown there. */
const hidden = computed(() => isWindows || asked.value?.status === "unsupported");
const badge = computed(() => {
  if (!on.value) return t("radio.off");
  return asked.value?.status === "on" ? t("radio.onSessions", { count: asked.value.sessions }) : t("radio.on");
});

function ask(): Promise<void> {
  flying ??= (async () => {
    const mine = turns;
    try {
      const state = await api.radioState();
      // A turn on or off meanwhile knows better.
      if (mine === turns) asked.value = state;
    } catch {
      // Kept as it was: the snapshot still says on or off.
    }
  })().finally(() => (flying = null));
  return flying;
}

async function turn(next: boolean): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  turns++;
  try {
    asked.value = await api.radioSet(next);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    turns++;
    busy.value = false;
  }
  if (!error.value) return;
  // How it stands now is asked, once an ask from before the turn is out of the way.
  await flying;
  void ask();
}

async function setTool(name: string, event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  try {
    await api.radioTool(name, box.checked);
  } catch {
    // Not taken: the switch shows what is stored again.
    box.checked = allowed(name);
  }
}

/* ---------- asked when it comes into view ---------- */

const root = ref<HTMLElement | null>(null);
let watcher: IntersectionObserver | null = null;

onMounted(() => {
  watcher = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return;
    // Looked at anew: what went wrong the last time has been read.
    error.value = "";
    void ask();
  });
  if (root.value) watcher.observe(root.value);
});

onBeforeUnmount(() => watcher?.disconnect());
</script>

<template>
  <!-- A part of the Claude tab, its head a row as the hook's above it. -->
  <div v-if="!hidden" ref="root" class="section radio" @change.stop>
    <div class="hook">
      <div class="hook-text">
        <span class="hook-title">
          {{ t("radio.title") }}
          <span class="kind">{{ t("radio.kind") }}</span>
          <span :class="['badge', { on }]">{{ badge }}</span>
        </span>
        <small>{{ t("radio.hint") }}</small>
      </div>
      <button type="button" class="control" :disabled="busy" :aria-busy="busy" @click="turn(!on)">
        <!-- Both labels keep their room, one over the other: the button is one width on or off,
             and under its spinner. -->
        <span :class="['labels', { hidden: busy }]">
          <span :class="{ away: on }" :aria-hidden="on">{{ t("radio.turnOn") }}</span>
          <span :class="{ away: !on }" :aria-hidden="!on">{{ t("radio.turnOff") }}</span>
        </span>
        <Spinner v-if="busy" class="over" :size="12" />
      </button>
    </div>
    <!-- One line at most: what just went wrong, else what stands in the way. -->
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-else-if="asked?.missing" class="note">{{ t("radio.missing") }}</p>

    <div v-if="tools.length" class="part" role="group" aria-labelledby="radio-tools">
      <div class="tools-head">
        <span id="radio-tools">{{ t("radio.tools") }}</span>
        <small>{{ t("radio.scope") }}</small>
      </div>
      <!-- The whole row is the switch's. -->
      <div class="rows">
        <label v-for="tool in tools" :key="tool.name" :class="['row', { off: !allowed(tool.name) }]">
          <span class="tool-name">{{ tool.name }}</span>
          <span class="tool-text">{{ TEXT[tool.name] ? t(TEXT[tool.name]) : "" }}</span>
          <span :class="['badge', { acts: tool.acts }]">{{ t(tool.acts ? "radio.acts" : "radio.reads") }}</span>
          <input type="checkbox" role="switch" :checked="allowed(tool.name)" @change="setTool(tool.name, $event)" />
        </label>
      </div>
    </div>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.radio {
  display: flex;
  flex-direction: column;
  gap: 16px;
  font-size: 13px;
}

/* What the radio is to Claude Code, beside its name. */
.kind {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.labels {
  display: grid;
}

.labels > span {
  grid-area: 1 / 1;
}

.away {
  visibility: hidden;
}

.note {
  margin: 0;
  font-size: 13px;
  overflow-wrap: anywhere;
}

/* What the list is and, at the line's end, how far it reaches. */
.tools-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}

.tools-head small {
  flex-grow: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  text-align: right;
}

.tool-name {
  width: 124px;
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
}

.tool-text {
  flex-grow: 1;
  min-width: 0;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-soft);
}

.row.off .tool-name,
.row.off .tool-text {
  color: var(--text-subtle);
}

/* Reads or acts. The second is Claude doing something: its colour. */
.badge.acts {
  background: var(--btn-claude-bg);
  color: var(--claude-soft);
}
</style>
