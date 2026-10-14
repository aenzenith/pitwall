<script setup lang="ts">
import { computed } from "vue";

import { t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";

/**
 * Settings › General › Quitting: whether a quit from Pitwall's own menus shows what is still open
 * first (window/FlagDialog). Its own: the switch goes to the core by its own command, never with
 * the form (`@change.stop` around it), and the dialog's box writes the same setting.
 */
const atQuit = computed(() => snapshot.value?.settings.flag?.atQuit ?? true);

/** A core that refuses it leaves the switch as the setting is. */
function set(event: Event): void {
  const box = event.target as HTMLInputElement;
  void api.flagSet(box.checked).catch(() => (box.checked = atQuit.value));
}
</script>

<template>
  <div class="section part flag">
    <div id="flag-quit" class="section-label">{{ t("flag.settings.section") }}</div>
    <!-- The whole row is the switch's. -->
    <div class="rows" role="group" aria-labelledby="flag-quit">
      <label class="row">
        <span class="name">{{ t("flag.settings.atQuit") }}</span>
        <input type="checkbox" role="switch" :checked="atQuit" @change="set" />
      </label>
    </div>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.flag {
  font-size: 13px;
}

/* With more parts after it in the tab: as far from the next as the tab's own are from each other. */
.flag:not(:last-child) {
  margin-bottom: 16px;
}
</style>
