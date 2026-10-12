<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";

/** The second step: Claude Code's hook, there or not, what it does, and the button that adds it. */

const installed = computed(() => !!snapshot.value?.claudeHook);
/** Installed, but from before the hooks that report Claude's state at once. */
const outdated = computed(() => installed.value && !!snapshot.value?.claudeHookOutdated);
const busy = ref(false);
const error = ref("");

/** Installing again upgrades an older one. */
async function install(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await api.installClaudeHook();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="pane">
    <div class="head">
      <span class="head-text">
        <span class="title">{{ t("setup.hook.title") }}</span>
        <span class="sub">
          <span :class="['mark', { ok: installed && !outdated, warn: outdated }]">
            <Icon v-if="installed && !outdated" name="check" :size="13" />
            <i v-else></i>
          </span>
          <span class="sub-text">{{ t(outdated ? "setup.hook.outdated" : installed ? "setup.hook.installed" : "setup.hook.missing") }}</span>
        </span>
      </span>
    </div>

    <div class="block">
      <ul class="facts">
        <li>
          <Rich :text="t('setup.hook.adds')">
            <template #settings><code>~/.claude/settings.json</code></template>
          </Rich>
        </li>
        <li>{{ t("setup.hook.reports") }}</li>
      </ul>
      <p v-if="outdated" class="note">{{ t("settings.hookOutdated") }}</p>
    </div>

    <div v-if="!installed || outdated || error" class="foot">
      <p v-if="error" class="foot-text error" role="alert">{{ error }}</p>
      <button v-if="!installed || outdated" type="button" class="control primary" :disabled="busy" :aria-busy="busy" @click="install">
        <span :class="{ hidden: busy }">{{ t(outdated ? "settings.updateHook" : "settings.addHook") }}</span>
        <Spinner v-if="busy" class="over" :size="12" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>
