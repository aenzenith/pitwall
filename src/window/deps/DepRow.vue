<script setup lang="ts">
import { computed } from "vue";

import Spinner from "../../components/Spinner.vue";
import { installFailure, isInstalling, rowSummary, shownChecks } from "../../lib/deps";
import { t } from "../../lib/i18n";
import type { DepReport, Project } from "../../lib/types";

/**
 * One project in the list: its mark, its name, and under it what is to do in a few words (with
 * nothing to do, its last scan). A click selects it: its topics show beside the list
 * (DepDetails). `hidden`: the ecosystems the language filter turned off; `focusable`: the
 * list's one Tab stop.
 */
const props = defineProps<{
  project: Project;
  report: DepReport;
  attention: boolean;
  hidden: string[];
  selected: boolean;
  focusable: boolean;
  now: number;
}>();
const emit = defineEmits<{ select: []; menu: [event: MouseEvent] }>();

const checks = computed(() => shownChecks(props.report, props.hidden));

const installingNow = computed(() => checks.value.find((check) => isInstalling(props.report, check.ecosystem))?.ecosystem ?? props.report.installing);
const failed = computed(() => checks.value.some((check) => installFailure(props.report, check.ecosystem)));

/** The mark: spins while installing, red when an install failed, yellow when you are needed. */
const mark = computed(() => {
  if (installingNow.value) return { kind: "busy", label: t("deps.status.installing") };
  if (failed.value) return { kind: "bad", label: t("deps.status.failed") };
  if (props.attention) return { kind: "warn", label: t("deps.status.attention") };
  return { kind: "ok", label: t("deps.status.ok") };
});

const summary = computed(() => rowSummary(props.report, props.hidden, props.now));
</script>

<template>
  <div
    role="option"
    :data-path="project.path"
    :tabindex="focusable ? 0 : -1"
    :aria-selected="selected"
    :class="['dep-row', { selected }]"
    @click="emit('select')"
    @contextmenu="emit('menu', $event)"
  >
    <span class="dep-mark-cell">
      <span :class="['dep-mark', mark.kind]" role="img" :aria-label="mark.label"></span>
    </span>
    <span class="dep-row-text">
      <span class="dep-name" :title="project.name">{{ project.name }}</span>
      <span :class="['dep-summary', summary.tone]" :aria-busy="summary.spin" :title="summary.title || undefined">
        <Spinner v-if="summary.spin" :size="10" />
        <span class="dep-summary-text">{{ summary.text }}</span>
      </span>
    </span>
  </div>
</template>

<style scoped src="./parts.css"></style>

<style scoped>
/* No colour of its own: hover and the selected row are a subtle ground. The whole row is the
   button. */
.dep-row {
  flex-shrink: 0;
  display: grid;
  grid-template-columns: 16px minmax(0, 1fr);
  gap: 12px;
  align-items: center;
  height: 48px;
  padding: 0 12px;
  border-radius: 8px;
  cursor: pointer;
}

.dep-row:hover:not(.selected) {
  background: #1b1e24;
}

.dep-row.selected {
  background: var(--bg-selected);
  box-shadow: inset 0 0 0 1px #2c313a;
}

/* The keyboard's row: the ring inside, so neighbours and the list's edge never cut it. */
.dep-row:focus-visible {
  outline-offset: -2px;
}

.dep-mark-cell {
  display: flex;
  justify-content: center;
}

.dep-row-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.dep-name,
.dep-summary-text {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dep-name {
  font-weight: 500;
}

/* What is to do reads plain; a last scan with nothing to do, and "not watched", fainter. */
.dep-summary {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-faint);
}

.dep-summary.plain {
  color: #c7ccd3;
}

.dep-summary.dim {
  color: var(--text-subtle);
}

.dep-summary :deep(.spinner) {
  flex-shrink: 0;
  color: var(--text-muted);
}
</style>
