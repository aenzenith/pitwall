<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { agentInfo } from "../../lib/agents";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";

/**
 * The step for Codex, there only when Codex was found on this computer: that it is on beside
 * Claude, its hook (installed by Pitwall, approved by the user in Codex's `/hooks`), and the way
 * to turn Codex off for good, or back on. Its radio and its accounts are Settings › Agents'.
 */

const info = computed(() => agentInfo("codex"));
const on = computed(() => !!info.value?.on);
const hook = computed(() => info.value?.hook ?? "missing");
const outdated = computed(() => hook.value !== "missing" && !!info.value?.hookOutdated);
const busy = ref<"hook" | "switch" | null>(null);
const error = ref("");

async function run(what: "hook" | "switch", act: () => Promise<unknown>): Promise<void> {
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

/** Installing again upgrades an older one. */
const install = () => run("hook", () => api.installCodexHook());
const turn = (next: boolean) => run("switch", () => api.agentSetOn("codex", next));

const state = computed(() => {
  if (!on.value) return t("settings.off");
  if (outdated.value) return t("setup.hook.outdated");
  if (hook.value === "waiting") return t("setup.hook.waiting.codex");
  return t(hook.value === "working" ? "setup.hook.installed" : "setup.hook.missing");
});
</script>

<template>
  <section class="pane">
    <div class="head">
      <span class="head-text">
        <span class="title">{{ t("setup.hook.title.codex") }}</span>
        <span class="sub">
          <span :class="['mark', { ok: on && hook === 'working' && !outdated, warn: on && (hook === 'waiting' || outdated) }]">
            <Icon v-if="on && hook === 'working' && !outdated" name="check" :size="13" />
            <i v-else></i>
          </span>
          <span class="sub-text">{{ state }}</span>
        </span>
      </span>
    </div>

    <div class="block">
      <ul class="facts">
        <li>{{ t(on ? "setup.codex.found" : "setup.codex.off") }}</li>
        <li>
          <Rich :text="t('setup.hook.adds.codex')">
            <template #file><code>~/.codex/hooks.json</code></template>
            <template #command><code>/hooks</code></template>
          </Rich>
        </li>
        <li>{{ t("setup.hook.reports.codex") }}</li>
      </ul>
    </div>

    <div class="foot">
      <p v-if="error" class="foot-text error" role="alert">{{ error }}</p>
      <button type="button" class="quiet" :disabled="!!busy" :aria-busy="busy === 'switch'" @click="turn(!on)">
        <span :class="{ hidden: busy === 'switch' }">{{ t(on ? "setup.codex.turnOff" : "setup.codex.turnOn") }}</span>
        <Spinner v-if="busy === 'switch'" class="over" :size="11" />
      </button>
      <button v-if="on && (hook === 'missing' || outdated)" type="button" class="control primary" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="install">
        <span :class="{ hidden: busy === 'hook' }">{{ t(outdated ? "settings.updateHook" : "settings.agents.hook.install") }}</span>
        <Spinner v-if="busy === 'hook'" class="over" :size="12" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>
