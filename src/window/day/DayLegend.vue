<script setup lang="ts">
import { t } from "../../lib/i18n";
</script>

<template>
  <div class="legend">
    <!-- What the timeline's marks mean; each key is drawn as its block is (DayLane). -->
    <span><i class="key work"></i>{{ t("day.legend.work") }}</span>
    <span><i class="key wait"></i>{{ t("day.legend.wait") }}</span>
    <span><i class="key server"></i>{{ t("day.legend.server") }}</span>
    <span><i class="key commit"></i>{{ t("day.legend.commit") }}</span>
    <span><i class="key crash" aria-hidden="true">✕</i>{{ t("day.legend.crash") }}</span>
  </div>
</template>

<style scoped>
.legend {
  flex-shrink: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
  padding-top: 12px;
  border-top: 1px solid var(--line-soft);
  font-size: 12px;
  color: var(--text-subtle);
}

.legend span {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.key {
  display: inline-block;
  box-sizing: border-box;
}

.key.work {
  width: 20px;
  height: 12px;
  background: var(--work);
}

.key.wait {
  width: 20px;
  height: 12px;
  border: 1px dashed color-mix(in srgb, var(--claude) 85%, transparent);
  background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--claude) 32%, transparent) 0 3px, transparent 3px 7px);
}

.key.server {
  width: 20px;
  height: 4px;
  background: var(--run);
  box-shadow: 0 0 6px var(--run-glow);
}

.key.commit {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-strong);
  box-shadow:
    0 0 0 2px var(--bg-app),
    0 0 0 3px var(--line-heavy);
}

.key.crash {
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--crash-bg);
  color: var(--crash-text);
  font-size: 9px;
  font-style: normal;
  font-weight: 700;
}
</style>
