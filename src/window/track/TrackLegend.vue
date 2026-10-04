<script setup lang="ts">
import { t, type Key } from "../../lib/i18n";

/** What the lights mean, along the bottom of the Track page. */
const LEGEND: Array<{ mark: string; text: Key }> = [
  { mark: "server", text: "track.legend.server" },
  { mark: "claude", text: "track.legend.claude" },
  { mark: "waiting", text: "track.legend.waiting" },
  { mark: "crashed", text: "track.legend.crashed" },
];
</script>

<template>
  <ul class="tleg" :aria-label="t('track.legend')">
    <li v-for="item in LEGEND" :key="item.mark"><span :class="['mark', item.mark]" aria-hidden="true"></span>{{ t(item.text) }}</li>
  </ul>
</template>

<style scoped>
/* One line where the window has the room; in a narrow one it wraps, and stays centred. */
.tleg {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 6px 22px;
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
}

.tleg li {
  display: flex;
  align-items: center;
  gap: 8px;
  white-space: nowrap;
}

.mark {
  flex-shrink: 0;
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

/* Moving lights are drawn as a short trail; the ones that stand, as a dot. */
.mark.server,
.mark.claude {
  width: 14px;
  height: 4px;
  border-radius: 2px;
}

.mark.server {
  background: linear-gradient(90deg, transparent, var(--run));
}

.mark.claude {
  background: linear-gradient(90deg, transparent, var(--claude));
}

.mark.waiting {
  background: var(--claude);
}

.mark.crashed {
  background: var(--crash);
}
</style>
