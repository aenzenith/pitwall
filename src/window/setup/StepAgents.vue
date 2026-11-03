<script setup lang="ts">
import { computed, ref, watch } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { agents } from "../../lib/agents";
import { agentsTab } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";
import type { Agent, AgentDetail, AgentInfo } from "../../lib/types";

/**
 * The second step: every agent Pitwall knows of, a row each, as the core lists them, with its
 * hook and its team radio in two columns. An agent that is on gets the buttons that put each in;
 * one the user turned off, or one not on this computer, a faded row that says so. "Set up all"
 * does every one's missing part in the rows' order. Whether Codex runs its hook is Codex's to
 * say (the user approves it in `/hooks`): its cell waits for that, and "Check" asks Codex. Turning
 * the radio off, what an agent may do over it and its accounts are Settings' (Claude's tab or
 * Agents). `shown`: the step is open, and the agents' versions are asked for then.
 */
const props = defineProps<{ shown: boolean }>();

/** What Pitwall does for each agent, each by its own command. */
const ACT: Record<Agent, { hook: () => Promise<unknown>; radio: () => Promise<unknown>; radioOn: () => boolean }> = {
  claude: {
    hook: () => api.installClaudeHook(),
    radio: () => api.radioSet(true),
    radioOn: () => !!snapshot.value?.settings.radio?.on,
  },
  codex: {
    hook: () => api.installCodexHook(),
    radio: () => api.codexRadioSet(true),
    radioOn: () => !!snapshot.value?.settings.agents?.codex?.radio,
  },
};

/** Each agent's CLI version, as Settings › Agents asks for it: once, when the step first shows. */
const details = ref<AgentDetail[]>([]);
let asked = false;
watch(
  () => props.shown,
  (shown) => {
    if (!shown || asked) return;
    asked = true;
    void api.agentDetails().then(
      (listed) => (details.value = listed ?? []),
      () => undefined,
    );
  },
  { immediate: true },
);

function version(info: AgentInfo): string {
  return info.on ? (details.value.find((detail) => detail.id === info.id)?.version ?? "") : "";
}

/** The user turned it off: then it never turns on by itself, found or not. */
function saidOff(info: AgentInfo): boolean {
  return snapshot.value?.settings.agents?.[info.id]?.on === false;
}

/** What is still to do for an agent that is on: its hook (missing or from an older Pitwall) and its
 * radio. */
function hookToDo(info: AgentInfo): boolean {
  return info.caps.hook && (info.hook === "missing" || info.hookOutdated);
}

function radioToDo(info: AgentInfo): boolean {
  return info.caps.radio && !ACT[info.id].radioOn();
}

const todo = computed(() => agents.value.some((info) => info.on && (hookToDo(info) || radioToDo(info))));

/* ---------- the buttons ---------- */

/** The part being put in, while the core is at it; `all`: "Set up all" is going through them. */
const busy = ref<{ agent: Agent; part: "hook" | "radio" | "check" } | null>(null);
const all = ref(false);
/** What went wrong, under the agent's row. */
const errors = ref<Partial<Record<Agent, string>>>({});
/** What the last "Check" found while Codex doesn't run every line yet; gone once the hook moves. */
const notYet = ref<Partial<Record<Agent, { trusted: number; total: number }>>>({});
watch(
  () => agents.value.map((info) => `${info.id}:${info.hook}`).join(),
  () => (notYet.value = {}),
);

/** One part of one agent. False when it went wrong, the reason under its row. */
async function act(info: AgentInfo, part: "hook" | "radio" | "check"): Promise<boolean> {
  busy.value = { agent: info.id, part };
  errors.value = { ...errors.value, [info.id]: undefined };
  try {
    if (part === "hook") await ACT[info.id].hook();
    // Without the agent's CLI to add it with, the core says so ("Codex wasn't found").
    else if (part === "radio") await ACT[info.id].radio();
    else {
      notYet.value = { ...notYet.value, [info.id]: undefined };
      const said = await api.checkCodexHook();
      if (said.trusted < said.total) notYet.value = { ...notYet.value, [info.id]: said };
    }
    return true;
  } catch (reason) {
    errors.value = { ...errors.value, [info.id]: String(reason) };
    return false;
  } finally {
    busy.value = null;
  }
}

async function one(info: AgentInfo, part: "hook" | "radio" | "check"): Promise<void> {
  if (busy.value) return;
  await act(info, part);
}

/** Every agent that is on, in the rows' order: its hook, then its radio. Stops at the first that
 * goes wrong. */
async function setUpAll(): Promise<void> {
  if (busy.value) return;
  all.value = true;
  try {
    for (const info of agents.value) {
      if (!info.on) continue;
      if (hookToDo(info) && !(await act(info, "hook"))) return;
      if (radioToDo(info) && !(await act(info, "radio"))) return;
    }
  } finally {
    all.value = false;
  }
}

const working = (agent: Agent, part: "hook" | "radio" | "check") => busy.value?.agent === agent && busy.value.part === part;

const switchError = ref<Partial<Record<Agent, string>>>({});

async function turn(info: AgentInfo, event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  switchError.value = { ...switchError.value, [info.id]: undefined };
  try {
    await api.agentSetOn(info.id, box.checked);
  } catch (reason) {
    // Not taken: the switch shows what is stored again.
    box.checked = info.on;
    switchError.value = { ...switchError.value, [info.id]: String(reason) };
  }
}
</script>

<template>
  <section class="pane">
    <div class="sheet">
      <div class="row head" aria-hidden="true">
        <span></span>
        <span class="col">
          {{ t("setup.agents.hook") }}
          <small>{{ t("setup.agents.hookText") }}</small>
        </span>
        <span class="col">
          {{ t("radio.title") }}
          <small>{{ t("setup.agents.radioText") }}</small>
        </span>
      </div>

      <div v-for="info in agents" :key="info.id" :class="['agent', { faded: !info.on }]">
        <div class="row">
          <span class="who">
            <AgentMark :agent="info.id" :size="18" />
            <span class="name">
              <span class="agent-name">{{ info.name }}</span>
              <span class="version">{{ version(info) }}</span>
            </span>
            <!-- One not on this computer has no switch, unless it is on (Claude). -->
            <input
              v-if="info.on || info.found"
              type="checkbox"
              role="switch"
              :checked="info.on"
              :disabled="info.id === 'claude'"
              :aria-label="info.name"
              @change="turn(info, $event)"
            />
          </span>

          <!-- Off by the user's word, or not on this computer: what that means, across both cells. -->
          <span v-if="!info.on" class="cell wide">
            <span class="small">{{ t(saidOff(info) ? "setup.agents.off" : "setup.agents.notFound") }}</span>
          </span>

          <template v-else>
            <span class="cell">
              <template v-if="!info.caps.hook"></template>
              <span v-else-if="info.hook === 'waiting'" class="stack">
                <span class="state"><span class="mark warn"><i></i></span>{{ t("setup.agents.waiting") }}</span>
                <span class="small">
                  <template v-if="notYet[info.id]">{{ t("setup.agents.checkNot", notYet[info.id]!) }}</template>
                  <Rich v-else :text="t('setup.agents.approve.codex')">
                    <template #command><code>/hooks</code></template>
                  </Rich>
                  ·
                  <button type="button" class="link" :disabled="!!busy" :aria-busy="working(info.id, 'check')" @click="one(info, 'check')">
                    <span :class="{ hidden: working(info.id, 'check') }">{{ t("settings.agents.hook.check") }}</span>
                    <Spinner v-if="working(info.id, 'check')" class="over" :size="10" />
                  </button>
                </span>
              </span>
              <span v-else-if="info.hookOutdated" class="state"><span class="mark warn"><i></i></span>{{ t("setup.hook.outdated") }}</span>
              <span v-else-if="info.hook === 'missing'" class="state"><span class="mark"><i></i></span>{{ t("setup.hook.missing") }}</span>
              <span v-else class="state"><span class="mark ok"><Icon name="check" :size="13" /></span>{{ t("setup.hook.installed") }}</span>
              <button
                v-if="info.caps.hook && (info.hook === 'missing' || info.hookOutdated)"
                type="button"
                class="control"
                :disabled="!!busy"
                :aria-busy="working(info.id, 'hook')"
                @click="one(info, 'hook')"
              >
                <span :class="{ hidden: working(info.id, 'hook') }">{{ t(info.hookOutdated ? "setup.agents.update" : "setup.agents.install") }}</span>
                <Spinner v-if="working(info.id, 'hook')" class="over" :size="12" />
              </button>
            </span>

            <span class="cell">
              <template v-if="!info.caps.radio"></template>
              <span v-else-if="ACT[info.id].radioOn()" class="state"><span class="mark ok"><Icon name="check" :size="13" /></span>{{ t("radio.on") }}</span>
              <template v-else>
                <span class="state"><span class="mark"><i></i></span>{{ t("radio.off") }}</span>
                <button type="button" class="control" :disabled="!!busy" :aria-busy="working(info.id, 'radio')" @click="one(info, 'radio')">
                  <span :class="{ hidden: working(info.id, 'radio') }">{{ t("setup.agents.turnOn") }}</span>
                  <Spinner v-if="working(info.id, 'radio')" class="over" :size="12" />
                </button>
              </template>
            </span>
          </template>
        </div>
        <p v-if="errors[info.id] || switchError[info.id]" class="error" role="alert">{{ errors[info.id] || switchError[info.id] }}</p>
      </div>

      <ul class="facts">
        <li>{{ t("setup.agents.hookFact") }}</li>
        <li>{{ t("setup.agents.radioFact") }}</li>
      </ul>
    </div>

    <div class="foot">
      <p class="foot-text note">{{ t(agentsTab ? "setup.agents.tools.agents" : "setup.agents.tools") }}</p>
      <button v-if="todo || all" type="button" class="control primary" :disabled="!!busy" :aria-busy="all" @click="setUpAll">
        <span :class="{ hidden: all }">{{ t("setup.agents.all") }}</span>
        <Spinner v-if="all" class="over" :size="12" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>

<style scoped>
/* The table and what to know of it: it scrolls on its own if a small window can't hold it. */
.sheet {
  flex: 0 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}

/* A row per agent: the agent, its hook, its radio. */
.row {
  display: grid;
  grid-template-columns: 186px minmax(0, 1fr) minmax(0, 1fr);
  column-gap: 18px;
  align-items: center;
}

.head {
  padding-bottom: 8px;
  border-bottom: 1px solid var(--line);
}

.col {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
}

.col small {
  font-size: 11.5px;
  font-weight: 400;
  letter-spacing: 0;
  text-transform: none;
  color: var(--text-faint);
}

.agent {
  border-bottom: 1px solid var(--line);
}

.agent .row {
  min-height: 58px;
  padding: 8px 0;
}

.who {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
}

.name {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.agent-name {
  font-size: 13.5px;
  font-weight: 500;
  color: var(--text-strong);
}

.version {
  min-height: 1lh;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Off, or not on this computer: the mark and the name faded, the words as they are. */
.faded :is(.agent-name, .version, svg) {
  opacity: 0.55;
}

.who input:focus-visible {
  outline-offset: 2px;
}

/* How it stands at the left, the one thing to do at the right. */
.cell {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.cell.wide {
  grid-column: span 2;
}

.cell .control {
  height: 28px;
  padding: 0 11px;
  font-size: 12.5px;
}

.state {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.stack {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.small {
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--text-subtle);
}

.small code {
  font-size: 11px;
}

/* Check: a word that does something, underlined, beside what to do in Codex. */
.link {
  position: relative;
  padding: 0;
  border: 0;
  background: none;
  font-size: inherit;
  color: var(--text-soft);
  text-decoration: underline;
  text-decoration-color: var(--line-dotted);
  text-underline-offset: 2px;
}

.link:hover:not(:disabled) {
  color: var(--text-strong);
}

.link .hidden {
  visibility: hidden;
}

.link .over {
  position: absolute;
  inset: 0;
  margin: auto;
}

.error {
  padding: 0 0 10px 28px;
}

.facts {
  padding-top: 12px;
  font-size: 12px;
  color: var(--text-subtle);
}

/* A narrow card: the agent's column gives way first. */
@container setup-step (max-width: 560px) {
  .row {
    grid-template-columns: 132px minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 12px;
  }
}
</style>
