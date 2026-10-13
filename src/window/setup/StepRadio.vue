<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Rich from "../../components/Rich.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";

/**
 * The third step: the team radio (Pitwall among Claude Code's MCP servers), on or off, what
 * turning it on does, and the button that does it. Turning it off again, and what Claude may do
 * over it, are Settings › Claude's.
 */

const on = computed(() => !!snapshot.value?.settings.radio?.on);
/** Being turned on: Claude Code's CLI is at work. */
const busy = ref(false);
const error = ref("");

async function turnOn(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await api.radioSet(true);
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
        <span class="title">
          {{ t("radio.title") }}
          <span class="kind">{{ t("radio.kind") }}</span>
        </span>
        <span class="sub">
          <span :class="['mark', { ok: on }]">
            <Icon v-if="on" name="check" :size="13" />
            <i v-else></i>
          </span>
          <span class="sub-text">{{ t(on ? "radio.on" : "radio.off") }}</span>
        </span>
      </span>
    </div>

    <div class="block">
      <ul class="facts">
        <li>
          <Rich :text="t('setup.radio.adds')">
            <template #command><code>claude mcp add</code></template>
          </Rich>
        </li>
        <li>{{ t("setup.radio.acts") }}</li>
      </ul>
    </div>

    <div v-if="!on || error" class="foot">
      <p v-if="error" class="foot-text error" role="alert">{{ error }}</p>
      <button v-if="!on" type="button" class="control primary" :disabled="busy" :aria-busy="busy" @click="turnOn">
        <span :class="{ hidden: busy }">{{ t("radio.turnOn") }}</span>
        <Spinner v-if="busy" class="over" :size="12" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>

<style scoped>
/* What the radio is to Claude Code, beside its name. */
.kind {
  margin-left: 6px;
  font-size: 12px;
  font-weight: 400;
  color: var(--text-subtle);
}
</style>
