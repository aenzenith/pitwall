<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { editorName } from "../../lib/format";
import { t, type Key } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { ExtensionStatus } from "../../lib/types";

/**
 * The third step: Pitwall for VS Code in the chosen editor, there or not, what it adds, and the
 * way to its page. The reading is the view's (its rail shows it too): `status` null until the
 * core answers, `failed` when it didn't; `recheck` asks again.
 */
const props = defineProps<{ editor: string; status: ExtensionStatus | null; checking: boolean; failed: boolean }>();
const emit = defineEmits<{ recheck: [] }>();

const why: Key[] = ["settings.extWhyServers", "settings.extWhySeen", "settings.extWhyReveal", "settings.extWhyProjects"];

/** `VS Code · Installed · 1.4.0`, `Cursor · Not installed`. */
const line = computed(() => {
  if (!props.status) return props.failed ? t("setup.ext.failed") : t("setup.ext.checking");
  const state = props.status.version ? t("settings.extInstalled", { version: props.status.version }) : t("settings.extMissing");
  return `${editorName(props.editor)} · ${state}`;
});

const opening = ref(false);
const error = ref("");

/** The extension's page in the editor's marketplace, or its .vsix where the editor has none. */
async function openPage(): Promise<void> {
  if (opening.value) return;
  opening.value = true;
  error.value = "";
  try {
    await api.openExtensionPage(props.editor);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    opening.value = false;
  }
}
</script>

<template>
  <section class="pane">
    <div class="head">
      <span class="head-text">
        <span class="title">Pitwall for VS Code</span>
        <span class="sub" role="status">
          <Spinner v-if="!status && checking" :size="11" />
          <span v-else :class="['mark', { ok: status?.version }]">
            <Icon v-if="status?.version" name="check" :size="13" />
            <i v-else></i>
          </span>
          <span class="sub-text">{{ line }}</span>
        </span>
      </span>
    </div>

    <div class="block">
      <p class="text">{{ t("settings.extIntro") }}</p>
      <ul class="facts">
        <li v-for="key in why" :key="key">{{ t(key) }}</li>
      </ul>
      <p v-if="status && !status.version && !status.marketplace" class="note">{{ t("settings.extVsixHint", { editor: editorName(editor) }) }}</p>
    </div>

    <div class="foot">
      <!-- After installing it in the editor: the window coming back reads it again by itself. -->
      <button type="button" class="quiet" :disabled="checking" :aria-busy="checking" @click="emit('recheck')">
        <span :class="{ hidden: checking }">{{ t("setup.ext.recheck") }}</span>
        <Spinner v-if="checking" class="over" :size="11" />
      </button>
      <p v-if="error" class="foot-text error" role="alert">{{ error }}</p>
      <button v-if="status && !status.version" type="button" class="control primary" :disabled="opening" :aria-busy="opening" @click="openPage">
        <span :class="['label', { hidden: opening }]">
          {{ t(status.marketplace ? "settings.extOpenPage" : "settings.extDownload") }}
          <Icon name="external" :size="13" />
        </span>
        <Spinner v-if="opening" class="over" :size="12" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>

<style scoped>
/* The text button sits on the text's left edge, not its own padding's. */
.foot .quiet {
  margin-left: -8px;
}

.label {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}
</style>
