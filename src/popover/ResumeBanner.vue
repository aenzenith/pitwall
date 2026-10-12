<script setup lang="ts">
import { computed, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { api, snapshot } from "../lib/store";

/**
 * Under the popover's head while what was open at the last quit is still to be answered
 * (`snapshot.resume`): how many projects, up to three of them by name, Restore and Start empty.
 * It goes with the offer. A call that fails says so on the names' line, so the row keeps its
 * height (the popover's window is fitted to its content).
 */
const offer = computed(() => snapshot.value?.resume ?? null);

/** Up to three by name; the rest as a count. */
const names = computed(() => {
  const all = (offer.value?.projects ?? []).map((project) => project.name);
  const shown = all.slice(0, 3).join(", ");
  return all.length > 3 ? t("resume.more", { names: shown, count: all.length - 3 }) : shown;
});

/** The answer under way: it lasts until the offer has left the snapshot. */
const busy = ref<"restore" | "dismiss" | null>(null);
const error = ref("");

watch(offer, (left) => {
  if (left) return;
  busy.value = null;
  error.value = "";
});

async function answer(what: "restore" | "dismiss"): Promise<void> {
  if (busy.value) return;
  busy.value = what;
  error.value = "";
  try {
    await (what === "restore" ? api.resumeRestore(false) : api.resumeDismiss());
  } catch (reason) {
    error.value = String(reason);
    busy.value = null;
  }
}
</script>

<template>
  <!-- Padding, not a margin: the popover adds up its parts' heights to size its window. -->
  <section v-if="offer" class="resume" :aria-label="t('resume.title')">
    <div class="card">
      <Icon name="clock" :size="15" class="mark" />
      <span class="text">
        <span class="line" :title="t('resume.banner', { count: offer.projects.length })">{{ t("resume.banner", { count: offer.projects.length }) }}</span>
        <span v-if="error" class="sub bad" role="alert" :title="error">{{ error }}</span>
        <span v-else class="sub" :title="names">{{ names }}</span>
      </span>
      <span class="actions">
        <button type="button" class="restore" :disabled="!!busy" :aria-busy="busy === 'restore'" @click="answer('restore')">
          <!-- The label keeps its room under the spinner: the button never changes width. -->
          <span :class="{ hidden: busy === 'restore' }">{{ t("resume.restore") }}</span>
          <Spinner v-if="busy === 'restore'" class="over" :size="11" />
        </button>
        <button type="button" class="dismiss" :disabled="!!busy" :aria-busy="busy === 'dismiss'" :aria-label="t('resume.dismiss')" :title="t('resume.dismiss')" @click="answer('dismiss')">
          <Spinner v-if="busy === 'dismiss'" :size="11" />
          <Icon v-else name="close" :size="14" />
        </button>
      </span>
    </div>
  </section>
</template>

<style scoped>
.resume {
  flex-shrink: 0;
  padding: 0 8px 8px;
}

.card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 8px 9px 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-card);
  background: var(--bg-control);
}

.mark {
  flex-shrink: 0;
  color: var(--text-soft);
}

/* Gives way to the buttons beside it. */
.text {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.line,
.sub {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.line {
  font-weight: 500;
}

.sub {
  font-size: 12px;
  color: var(--text-muted);
}

.sub.bad {
  color: var(--crash-text);
}

.actions {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.restore {
  position: relative;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-icon-hover);
  font-size: 12px;
}

.restore:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.dismiss {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.dismiss:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.restore:disabled,
.dismiss:disabled {
  opacity: 0.6;
}

.hidden {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}
</style>
