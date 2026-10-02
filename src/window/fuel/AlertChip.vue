<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { percentText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";

/** Settings' "notify at 90 %", on the page: shows it and turns it on or off. */

/** What was just picked, shown until the snapshot says the same. */
const pending = ref<boolean | null>(null);
const stored = computed(() => snapshot.value?.settings.fuelAlert ?? true);
const on = computed(() => pending.value ?? stored.value);

watch(stored, (value) => {
  if (value === pending.value) pending.value = null;
});

async function toggle(): Promise<void> {
  const settings = snapshot.value?.settings;
  if (!settings) return;
  const next = !on.value;
  pending.value = next;
  try {
    await api.setSettings({ ...settings, fuelAlert: next });
  } catch {
    pending.value = null;
  }
}
</script>

<template>
  <button type="button" class="chip" :aria-pressed="on" :title="t('settings.fuelAlert')" :disabled="!snapshot" @click="toggle">
    <span class="label">{{ t("fuel.alertChip", { value: percentText(90, language) }) }}</span>
    <span :class="['switch', { on }]" aria-hidden="true"><i></i></span>
  </button>
</template>

<style scoped>
.chip {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  height: 26px;
  padding: 0 8px 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: 6px;
  background: var(--bg-control);
  font-size: 12px;
  color: #c7ccd3;
  white-space: nowrap;
}

.chip:hover:not(:disabled) {
  background: #2c3039;
}

.chip:disabled {
  opacity: 0.5;
}

/* A small switch: the knob to the right when on. Shape, not colour, tells the two apart. */
.switch {
  position: relative;
  width: 22px;
  height: 12px;
  flex-shrink: 0;
  border-radius: 6px;
  background: var(--line-strong);
  transition: background 0.15s ease;
}

.switch i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-faint);
  transition: transform 0.15s ease, background 0.15s ease;
}

.switch.on {
  background: var(--text-muted);
}

.switch.on i {
  transform: translateX(10px);
  background: var(--bg-app);
}

@media (prefers-reduced-motion: reduce) {
  .switch,
  .switch i {
    transition: none;
  }
}
</style>
