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
  <!-- A switch, not a button: what it does, and the switch itself. -->
  <button
    type="button"
    role="switch"
    :class="['alert', { on }]"
    :aria-checked="on"
    :title="t('settings.fuelAlert')"
    :disabled="!snapshot"
    @click="toggle"
  >
    <span class="label">{{ t("fuel.alertChip", { value: percentText(90, language) }) }}</span>
    <span class="switch" aria-hidden="true"><i></i></span>
  </button>
</template>

<style scoped>
/* No box of its own: it sits in the footer's line like the facts beside it. */
.alert {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  height: 28px;
  padding: 0 4px 0 8px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
  transition: background 0.15s ease, color 0.15s ease;
}

.alert:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text);
}

.alert:disabled {
  opacity: 0.5;
}

.alert.on {
  color: var(--text);
}

/* A Mac switch: the knob slides right and the track fills with Claude's colour when on. */
.switch {
  position: relative;
  width: 28px;
  height: 16px;
  flex-shrink: 0;
  border-radius: 8px;
  background: var(--line-strong);
  box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.35);
  transition: background 0.18s ease;
}

.switch i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--text-strong);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.45);
  transition: transform 0.18s ease;
}

.on .switch {
  background: var(--claude);
}

.on .switch i {
  transform: translateX(12px);
}

@media (prefers-reduced-motion: reduce) {
  .alert,
  .switch,
  .switch i {
    transition: none;
  }
}
</style>
