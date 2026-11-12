<script setup lang="ts">
import { emitTo } from "@tauri-apps/api/event";
import { computed } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { api, update } from "../lib/store";

/**
 * Over the popover's foot while a new version waits for the user: where nothing installs it at a
 * quit (Windows, a system package, a place the user can't write) or this install can't put it in
 * its place (the core's `UpdateView::waits_for_user`). Its button opens Pitwall's window with the
 * update's dialog, which asks before the restart.
 */
const line = computed(() => {
  const state = update.value;
  if (!state) return null;
  const version = state.version ?? "";
  if (state.status === "available") return t("update.dialog.available", { version });
  if (state.onQuit) return null;
  if (state.status === "ready") return t("update.dialog.ready", { version });
  if (state.status === "installing") return t("about.update.installing", { version });
  return null;
});
const installing = computed(() => update.value?.status === "installing");

/** Pitwall's window with the update's dialog: the way the fuel strip opens its page. */
function open(): void {
  void emitTo("main", "reveal-page", { page: "update" });
  void api.openWindow();
}
</script>

<template>
  <section v-if="line" class="updatestrip" :aria-label="line">
    <Icon name="download" :size="14" class="updatestrip-mark" />
    <span class="updatestrip-line" :title="line">{{ line }}</span>
    <button type="button" class="updatestrip-open" :disabled="installing" :aria-busy="installing" @click="open">
      <!-- The label keeps its room under the spinner: the button never changes width. -->
      <span :class="{ 'updatestrip-hidden': installing }">{{ t("update.button") }}</span>
      <Spinner v-if="installing" class="updatestrip-over" :size="11" />
    </button>
  </section>
</template>

<style scoped>
.updatestrip {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 8px 7px 16px;
  border-top: 1px solid var(--line);
  background: var(--bg-footer);
}

.updatestrip-mark {
  flex-shrink: 0;
  color: var(--text-soft);
}

/* Gives way to the button beside it. */
.updatestrip-line {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 12px;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.updatestrip-open {
  position: relative;
  flex-shrink: 0;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
}

.updatestrip-open:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.updatestrip-open:disabled {
  opacity: 0.6;
}

.updatestrip-hidden {
  visibility: hidden;
}

.updatestrip-over {
  position: absolute;
  inset: 0;
  margin: auto;
}
</style>
