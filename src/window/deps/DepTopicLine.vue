<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { anyInstalling, install, installJob, type DepLine, type ScanView } from "../../lib/deps";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { DepReport, Project } from "../../lib/types";

/**
 * One line of a topic, the same columns whatever it is about: its tool, what it says with a
 * note beside it (under it, where the two don't fit), and at the line's end the one button that
 * acts on it. A migrate is only asked for here (`migrate`): the page confirms it in a dialog.
 * A scan's counts are buttons of their own: each asks for what it counts (`details`).
 */
const props = defineProps<{ line: DepLine; project: Project; report: DepReport }>();
const emit = defineEmits<{ migrate: [tool: string]; openOutput: [job: string]; details: [ecosystem: string, view: ScanView] }>();

/** The least a re-check's spinner shows, so one the core answers at once still shows. */
const MIN_SPIN_MS = 500;

/** One install at a time in a project: while one runs, the other lines' buttons wait. */
const installing = computed(() => anyInstalling(props.report));

/** The project's lock files and installed packages read again: what a manager put on this
 * machine meanwhile waits for. */
const rechecking = ref(false);

async function recheck(): Promise<void> {
  if (rechecking.value) return;
  rechecking.value = true;
  const started = Date.now();
  await api.depsCheck(props.project.path).catch(() => undefined);
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  rechecking.value = false;
}
</script>

<template>
  <div class="dep-line">
    <span :class="['dep-line-name', { mono: line.mono }]" :title="line.name">{{ line.name }}</span>
    <span class="dep-line-body">
      <span :class="['dep-line-state', line.tone]" :aria-busy="line.spin" :title="line.spinTitle || undefined">
        <Spinner v-if="line.spin" :size="10" />
        <Icon v-else-if="line.kind === 'scan' && line.offline" name="wifi-off" :size="12" />
        <!-- What a scan found, piece by piece: a count is pressed for what it counts. -->
        <span v-if="line.kind === 'scan' && line.parts.length" class="dep-line-parts">
          <template v-for="(part, at) in line.parts" :key="at">
            <span v-if="at" aria-hidden="true"> · </span>
            <button v-if="part.open" type="button" class="dep-count" :title="t('deps.scan.showDetails')" @click="emit('details', line.ecosystem, part.open)">{{ part.text }}</button>
            <span v-else>{{ part.text }}</span>
          </template>
        </span>
        <span v-else class="dep-line-text" :title="line.state">{{ line.state }}</span>
      </span>
      <span v-if="line.note" :class="['dep-line-note', { mono: line.kind === 'migration' }]" :title="line.note">{{ line.note }}</span>
    </span>
    <span class="dep-line-action">
      <template v-if="line.kind === 'install'">
        <button
          v-if="line.installing"
          type="button"
          class="dep-btn"
          :aria-label="t('deps.showOutputName', { name: project.name })"
          @click="emit('openOutput', installJob(line.ecosystem))"
        >
          <span class="dep-btn-text">{{ t("deps.showOutput") }}</span>
        </button>
        <!-- The command as it runs; Pitwall can't run one it doesn't know. -->
        <button
          v-else-if="line.command !== null"
          type="button"
          class="dep-btn"
          :disabled="installing"
          :title="installing ? t('deps.migrateAfterInstall') : line.command"
          :aria-label="t('deps.installName', { command: line.command, name: project.name })"
          @click="install(project.path, line.ecosystem)"
        >
          <span class="dep-btn-text">{{ line.command }}</span>
        </button>
      </template>
      <!-- Asks first, in a dialog: the command, and where it would go. -->
      <button
        v-else-if="line.kind === 'migration' && line.pending.length"
        type="button"
        class="dep-btn"
        :disabled="installing || line.migrating || line.blocked"
        :aria-busy="line.migrating"
        :title="installing ? t('deps.migrateAfterInstall') : line.command"
        @click="emit('migrate', line.tool)"
      >
        <span class="dep-btn-text">{{ t("deps.migrate") }}</span>
      </button>
      <button v-else-if="line.kind === 'missing'" type="button" class="dep-btn" :disabled="rechecking" :aria-busy="rechecking" @click="recheck">
        <Spinner v-if="rechecking" :size="11" />
        <span class="dep-btn-text">{{ t("deps.recheck") }}</span>
      </button>
    </span>
  </div>
</template>

<style scoped src="./parts.css"></style>

<style scoped>
/* The tool (DepDetails: --dep-tool), what it says, and the line's button at its end: every
   button of a project in one column, on the line it acts on. All three start on the line's
   first row, as tall as a button. */
.dep-line {
  display: grid;
  grid-template-columns: var(--dep-tool) minmax(0, 1fr) auto;
  gap: 10px;
  align-items: start;
}

.dep-line-name,
.dep-line-text,
.dep-line-note {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dep-line-name {
  font-size: 12px;
  line-height: 26px;
  color: #c7ccd3;
}

.dep-line-name.mono,
.dep-line-note.mono {
  font-family: var(--font-mono);
}

/* What it says, then the note beside it; a note that doesn't fit there whole goes under it. */
.dep-line-body {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  column-gap: 8px;
  min-width: 0;
}

.dep-line-state {
  flex: 0 1 auto;
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  min-height: 26px;
  font-size: 12px;
  color: var(--text-muted);
}

.dep-line-state.todo {
  font-weight: 600;
  color: var(--text);
}

/* A failed install: the page's only red text. */
.dep-line-state.bad {
  font-weight: 600;
  color: var(--crash-text);
}

.dep-line-state.plain {
  color: #c7ccd3;
}

.dep-line-state.faint {
  color: var(--text-faint);
}

.dep-line-state :deep(svg),
.dep-line-state :deep(.spinner) {
  flex-shrink: 0;
  color: var(--text-muted);
}

/* What a scan found, piece by piece; never cut, so a count's ground shows whole. */
.dep-line-parts {
  min-width: 0;
  white-space: nowrap;
}

/* A count that opens what it counts: text still, marked as a thing to press. */
.dep-count {
  margin: 0 -4px;
  padding: 2px 4px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font-size: 12px;
  text-decoration: underline dotted #5d636d;
  text-underline-offset: 3px;
}

.dep-count:hover {
  background: #262a31;
  color: var(--text-strong);
  text-decoration-color: var(--text-subtle);
}

.dep-line-note {
  flex: 0 1 auto;
  font-size: 11px;
  line-height: 18px;
  color: var(--text-faint);
}

.dep-line-action {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  min-width: 0;
}
</style>
