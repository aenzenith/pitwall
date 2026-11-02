<script setup lang="ts">
import { computed, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { clock } from "../../lib/day";
import { t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";
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
/** When the hook's latest event came (the core's, to the minute), for "Last event". */
const heard = computed(() => snapshot.value?.codexHeard ?? null);
const hooksFile = computed(() => `${props.detail?.home ?? "~/.codex"}/hooks.json`);
const radioOn = computed(() => !!snapshot.value?.settings.agents?.codex?.radio);
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

const busy = ref<"hook" | "radio" | null>(null);
const error = ref("");

async function run(what: "hook" | "radio", act: () => Promise<unknown>): Promise<void> {
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
      <!-- The hook: Pitwall installs it, the user approves it in Codex. -->
      <div :class="['card', { waits: hook === 'waiting' }]">
        <div class="card-head">
          <span class="hook-title">{{ t("settings.hookTitle") }}</span>
          <span v-if="hook === 'missing'" class="badge">{{ t("settings.agents.hook.missing") }}</span>
          <span v-else-if="hook === 'waiting'" class="badge waits">{{ t("settings.agents.hook.waiting.codex") }}</span>
          <span v-else class="badge on">{{ t("settings.on") }}</span>
        </div>

        <template v-if="hook === 'missing'">
          <small>{{ t("settings.agents.hook.missingText.codex") }}</small>
          <div class="card-foot">
            <small>{{ t("settings.agents.hook.trustOnce.codex") }}</small>
            <button type="button" class="control primary" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="installHook">
              <span :class="{ hidden: busy === 'hook' }">{{ t("settings.agents.hook.install") }}</span>
              <Spinner v-if="busy === 'hook'" class="over" :size="12" />
            </button>
          </div>
        </template>

        <template v-else-if="hook === 'waiting'">
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
          <div class="card-foot">
            <small>{{ t("settings.agents.hook.afterApproval") }}</small>
            <button type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="removeHook">
              <span :class="{ hidden: busy === 'hook' }">{{ t("settings.removeHook") }}</span>
              <Spinner v-if="busy === 'hook'" class="over" :size="12" />
            </button>
          </div>
        </template>

        <template v-else>
          <small>
            {{ t("settings.agents.hook.workingText.codex") }}
            <template v-if="heard">{{ t("settings.agents.hook.lastEvent", { time: clock(heard) }) }}</template>
          </small>
          <small v-if="info.hookOutdated">{{ t("settings.agents.hook.outdated.codex") }}</small>
          <div class="card-foot end">
            <button v-if="info.hookOutdated" type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="installHook">
              <span :class="{ hidden: busy === 'hook' }">{{ t("settings.updateHook") }}</span>
              <Spinner v-if="busy === 'hook'" class="over" :size="12" />
            </button>
            <button v-else type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="removeHook">
              <span :class="{ hidden: busy === 'hook' }">{{ t("settings.removeHook") }}</span>
              <Spinner v-if="busy === 'hook'" class="over" :size="12" />
            </button>
          </div>
        </template>
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

.buttons {
  display: flex;
  gap: 8px;
}

/* The hook's card: its colour a wait's while it waits for the user's approval in Codex. */
.card {
  display: flex;
  flex-direction: column;
  gap: 9px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--bg-card, transparent);
}

.card.waits {
  border-color: var(--claude-line);
}

.card small {
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-subtle);
}

.card-head,
.card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.card-foot.end {
  justify-content: flex-end;
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
