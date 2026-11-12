<script setup lang="ts">
import { computed, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { clock } from "../../lib/day";
import { agoDays } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, now, snapshot } from "../../lib/store";
import type { AgentDetail, AgentInfo } from "../../lib/types";
import AccountsSection from "./AccountsSection.vue";
import AgentHead from "./AgentHead.vue";
import RadioSection from "./RadioSection.vue";

/**
 * Codex's part of Settings › Agents. Not on this computer: where it was looked for, and that it
 * turns on by itself once it is (unless the user turned it off). Off by the user's word: what that means, and the way to take out
 * what Pitwall put into it (its hook, the radio's entry), which stays. On: the hook in its three
 * states (not installed; installed and waiting for the user's approval in Codex's `/hooks`, which
 * Pitwall never gives itself; at work), the team radio and the accounts. Everything here goes to
 * the core by its own command, never with Settings' form.
 */
const props = defineProps<{ info: AgentInfo; detail: AgentDetail | null; startAdd?: boolean }>();

const hook = computed(() => props.info.hook);
/** Where its hooks file is, as the user reads it. */
/** When the hook's latest event came (the core's, to the minute), for "The last event came…". */
const heard = computed(() => snapshot.value?.codexHeard ?? null);
const hooksFile = computed(() => `${props.detail?.home ?? "~/.codex"}/hooks.json`);
const radioOn = computed(() => !!snapshot.value?.settings.agents?.codex?.radio);
/** Where its fuel shows: on until the user unticks it. */
const fuelInBar = computed(() => snapshot.value?.settings.agents?.codex?.fuelInBar !== false);
const fuelInPopover = computed(() => snapshot.value?.settings.agents?.codex?.fuelInPopover !== false);
/** Turned off by the user: not found, it doesn't turn on by itself. */
const saidOff = computed(() => snapshot.value?.settings.agents?.codex?.on === false);
/** Off, the hook's state isn't read again: once "Remove hook" did its work there, it is gone. */
const hookTakenOut = ref(false);
watch(
  () => props.info.on,
  (on) => {
    if (on) hookTakenOut.value = false;
  },
);

const busy = ref<"hook" | "radio" | "check" | null>(null);
const error = ref("");
/** What the last "Check" found while Codex doesn't run every line yet; gone with a new check. */
const notYet = ref<{ trusted: number; total: number } | null>(null);
watch(hook, () => (notYet.value = null));

async function run(what: "hook" | "radio" | "check", act: () => Promise<unknown>): Promise<void> {
  if (busy.value) return;
  busy.value = what;
  error.value = "";
  try {
    await act();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = null;
  }
}

const installHook = () => run("hook", () => api.installCodexHook());
const removeHook = () => run("hook", () => api.uninstallCodexHook());
const removeOffHook = () =>
  run("hook", async () => {
    await api.uninstallCodexHook();
    hookTakenOut.value = true;
  });
const removeRadio = () => run("radio", () => api.codexRadioSet(false));
/** A box of its fuel ticked or not: at once, by its own command. */
function setFuel(where: { bar?: boolean; popover?: boolean }): void {
  error.value = "";
  api.agentSetFuel("codex", where).catch((reason) => (error.value = String(reason)));
}
/** Asks Codex itself whether it approved Pitwall's lines: once it did, the card turns On. */
const checkHook = () =>
  run("check", async () => {
    notYet.value = null;
    const said = await api.checkCodexHook();
    if (said.trusted < said.total) notYet.value = said;
  });
</script>

<template>
  <div class="codex" @change.stop>
    <AgentHead agent="codex" :detail="detail" :faded="!info.found" />

    <!-- Not on this computer. -->
    <template v-if="!info.found">
      <p class="text">{{ saidOff ? t("settings.agents.missingTextOff.codex") : t("settings.agents.missingText.codex") }}</p>
      <p class="text">
        <Rich :text="t('settings.agents.lookedIn.codex')">
          <template #variable><code>$CODEX_HOME</code></template>
          <template #folder><code>~/.codex</code></template>
        </Rich>
      </p>
    </template>

    <!-- Off by the user's word: nothing of it is read; what Pitwall put into it stays. -->
    <template v-else-if="!info.on">
      <div class="hook-text">
        <span class="hook-title">
          {{ t("settings.agents.offTitle.codex") }}
          <span class="badge">{{ t("settings.off") }}</span>
        </span>
        <small>{{ t("settings.agents.offText.codex") }}</small>
      </div>
      <div v-if="!hookTakenOut || radioOn" class="buttons">
        <button v-if="!hookTakenOut" type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="removeOffHook">
          <span :class="{ hidden: busy === 'hook' }">{{ t("settings.removeHook") }}</span>
          <Spinner v-if="busy === 'hook'" class="over" :size="12" />
        </button>
        <button v-if="radioOn" type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'radio'" @click="removeRadio">
          <span :class="{ hidden: busy === 'radio' }">{{ t("settings.agents.removeRadio") }}</span>
          <Spinner v-if="busy === 'radio'" class="over" :size="12" />
        </button>
      </div>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </template>

    <template v-else>
      <!-- Its fuel, as Claude's part has it (the same rows, the same gaps); no interval: nothing is
           asked for it. -->
      <div class="fuel section">
        <label class="box">
          <input type="checkbox" :checked="fuelInBar" @change="setFuel({ bar: ($event.target as HTMLInputElement).checked })" />
          <span>{{ t("settings.fuelInBar") }}</span>
        </label>
        <label class="box">
          <input type="checkbox" :checked="fuelInPopover" @change="setFuel({ popover: ($event.target as HTMLInputElement).checked })" />
          <span>{{ t("settings.fuelInPopover") }}</span>
        </label>
        <!-- Where Claude's has its interval: a title and what to know, with nothing to set. -->
        <div class="hook">
          <div class="hook-text">
            <span>{{ t("settings.agents.fuelTitle") }}</span>
            <small>{{ t("settings.agents.fuelText.codex") }}</small>
          </div>
        </div>
      </div>

      <!-- The hook: Pitwall installs it, the user approves it in Codex. Not installed and at work,
           a row as the radio's: its name and how it stands, what to know of it, and at the row's end
           the one thing to do; waiting for the approval, the steps under its name. -->
      <div v-if="hook !== 'waiting'" class="hook section">
        <div class="hook-text">
          <span class="hook-title wraps">
            {{ t("settings.hookTitle") }}
            <span v-if="hook === 'missing'" class="badge">{{ t("settings.agents.hook.missing") }}</span>
            <span v-else class="badge on">{{ t("settings.on") }}</span>
          </span>
          <template v-if="hook === 'missing'">
            <small>{{ t("settings.agents.hook.missingText.codex") }}</small>
            <small>{{ t("settings.agents.hook.trustOnce.codex") }}</small>
          </template>
          <template v-else>
            <small>
              {{ t("settings.agents.hook.workingText.codex") }}
              <span v-if="heard" :title="clock(heard)">{{ t("settings.agents.hook.lastEvent", { time: agoDays(heard, now) }) }}</span>
            </small>
            <small v-if="info.hookOutdated">{{ t("settings.agents.hook.outdated.codex") }}</small>
          </template>
        </div>
        <button v-if="hook === 'missing'" type="button" class="control primary" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="installHook">
          <span :class="{ hidden: busy === 'hook' }">{{ t("settings.agents.hook.install") }}</span>
          <Spinner v-if="busy === 'hook'" class="over" :size="12" />
        </button>
        <button v-else-if="info.hookOutdated" type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="installHook">
          <span :class="{ hidden: busy === 'hook' }">{{ t("settings.updateHook") }}</span>
          <Spinner v-if="busy === 'hook'" class="over" :size="12" />
        </button>
        <button v-else type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="removeHook">
          <span :class="{ hidden: busy === 'hook' }">{{ t("settings.removeHook") }}</span>
          <Spinner v-if="busy === 'hook'" class="over" :size="12" />
        </button>
      </div>
      <div v-else class="hook-card section">
        <span class="hook-title wraps">
          {{ t("settings.hookTitle") }}
          <span class="badge waits">{{ t("settings.agents.hook.waiting.codex") }}</span>
        </span>
        <small>{{ t("settings.agents.hook.waitingText.codex") }}</small>
        <ol class="steps">
          <li class="done">
            <span class="step-mark"><Icon name="check" :size="11" /></span>
            <span>
              <Rich :text="t('settings.agents.hook.step1')">
                <template #file><code>{{ hooksFile }}</code></template>
              </Rich>
            </span>
          </li>
          <li class="now">
            <span class="step-mark">2</span>
            <span>
              <Rich :text="t('settings.agents.hook.step2.codex')">
                <template #command><code class="command">/hooks</code></template>
              </Rich>
            </span>
          </li>
          <li>
            <span class="step-mark">3</span>
            <span>{{ t("settings.agents.hook.step3") }}</span>
          </li>
        </ol>
        <small v-if="notYet" class="not-yet" role="status">{{ t("settings.agents.hook.checkNot", { trusted: notYet.trusted, total: notYet.total }) }}</small>
        <div class="hook-card-foot">
          <small>
            {{ t("settings.agents.hook.afterApproval") }}&nbsp;
            <button type="button" class="check" :disabled="!!busy" :aria-busy="busy === 'check'" @click="checkHook">
              <span :class="{ hidden: busy === 'check' }">{{ t("settings.agents.hook.check") }}</span>
              <Spinner v-if="busy === 'check'" class="over" :size="11" />
            </button>
          </small>
          <button type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="removeHook">
            <span :class="{ hidden: busy === 'hook' }">{{ t("settings.removeHook") }}</span>
            <Spinner v-if="busy === 'hook'" class="over" :size="12" />
          </button>
        </div>
      </div>
      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <RadioSection v-if="info.caps.radio" agent="codex" stacked />
      <!-- Codex's accounts: CX5's section, given the agent. -->
      <AccountsSection v-if="info.caps.accounts" agent="codex" :start-add="startAdd" />
    </template>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.codex {
  display: flex;
  flex-direction: column;
  gap: 16px;
  font-size: 13px;
}

.text {
  margin: 0;
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--text-soft);
}

/* Off: its words as tall as they are, the buttons right under them. */
.codex > .hook-text {
  flex-grow: 0;
}

/* Its words as a hook's (settings.css's `.hook small`, which doesn't reach them here). */
.codex > .hook-text small {
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-subtle);
}

/* Its fuel's boxes, as Claude's (SettingsView's `.check`, its fieldset's gap), and under them
   where its limits come from. */
.fuel {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.box {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
}

.box input {
  width: 16px;
  height: 16px;
  accent-color: var(--run);
}


/* The hook is settings.css's row (`.hook`) as Claude's is, and while it waits for the user's
   approval in Codex its card (`.hook-card`), its badge in a wait's colour. The row's button keeps
   its label on one line; the words beside it give way. */
.hook > .control {
  white-space: nowrap;
}

.badge.waits {
  border: 1px solid var(--claude-line);
  background: var(--claude-bg);
  color: var(--claude-text);
}

.steps {
  display: flex;
  flex-direction: column;
  gap: 7px;
  margin: 0;
  padding: 2px 0;
  list-style: none;
  font-size: 12.5px;
}

.steps li {
  display: flex;
  align-items: center;
  gap: 9px;
}

.step-mark {
  width: 18px;
  height: 18px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--line-strong);
  border-radius: 50%;
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-subtle);
}

.done .step-mark {
  border-color: transparent;
  background: var(--run-bg);
  color: var(--run);
}

.now .step-mark {
  border-color: var(--claude-text);
  color: var(--claude-text);
}

.command {
  color: var(--claude-text);
}

/* "Check" after the line that the card turns On by itself (two spaces from it, one of them
   unbreakable: a wrap never starts its line indented): plain, underlined, no ground of its own,
   in that line's words' size. */
.check {
  position: relative;
  padding: 0;
  border: 0;
  background: transparent;
  font: inherit;
  color: var(--text-muted);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: default;
}

.check:hover:not(:disabled) {
  color: var(--text);
}

.check:disabled {
  opacity: 0.6;
}

.check .hidden {
  visibility: hidden;
}

.check .over {
  position: absolute;
  inset: 0;
  margin: auto;
}

.hook-card small.not-yet {
  color: var(--claude-text);
}

code {
  font-family: var(--font-mono);
  font-size: 12px;
}

/* The one thing to do, as the dialogs' (SettingsView: control primary). */
.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}
</style>
