<script setup lang="ts">
import { computed } from "vue";

import ClaudeSpark from "../../components/ClaudeSpark.vue";
import { ago } from "../../lib/format";
import { t } from "../../lib/i18n";
import { api, now } from "../../lib/store";
import type { Project } from "../../lib/types";

const props = defineProps<{ project: Project }>();

const claudeTitle = computed(() => {
  const turn = props.project.claude;
  if (!turn) return t(props.project.claudeWorking ? "detail.claudeWorking" : "detail.noSession");
  return t(({ finished: "detail.claudeFinished", asking: "detail.claudeAsking", permission: "detail.claudePermission" } as const)[turn.kind]);
});
</script>

<template>
  <section class="block" :aria-label="t('window.col.claude')">
    <div class="section-label">Claude</div>
    <div :class="['card', { hot: project.claude }]">
      <ClaudeSpark :state="project.claude ? 'waiting' : project.claudeWorking ? 'working' : 'quiet'" />
      <div class="card-text">
        <span class="card-title">{{ claudeTitle }}</span>
        <span class="card-sub">
          {{
            project.claude
              ? t("detail.notLookedAt", { ago: ago(project.claude.at, now) })
              : t(project.claudeWorking ? "detail.workingHint" : "detail.sessionsHint")
          }}
        </span>
      </div>
      <button v-if="project.claude" type="button" class="seen" @click="api.markSeen(project.path)">{{ t("detail.markSeen") }}</button>
    </div>
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
/* Bare, with no box in any state. */
.card {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* Waiting: still no box; a solid spark that pings (components/ClaudeSpark), the title in Claude's
   colour and a filled button carry it instead. */
.card.hot .card-title {
  font-weight: 600;
  color: var(--claude-text);
}

.card.hot .card-sub {
  color: #c9a88c;
}

.card-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.card-title {
  font-size: 13px;
  font-weight: 500;
}

.card-sub {
  font-size: 12px;
  color: var(--text-muted);
}

.seen {
  flex-shrink: 0;
  height: 28px;
  padding: 0 12px;
  border: 0;
  border-radius: var(--radius-control);
  background: var(--claude);
  color: #1a110a;
  font-size: 12px;
  font-weight: 600;
}

.seen:hover {
  background: var(--claude-text);
}
</style>
