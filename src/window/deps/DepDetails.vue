<script setup lang="ts">
import { computed, ref, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import {
  anyInstalling,
  depLines,
  depTopics,
  installFailure,
  installJob,
  isScanning,
  migrate,
  migrateJob,
  scan,
  type DepLine,
  type DepTopicLines,
  type ScanView,
} from "../../lib/deps";
import { ago } from "../../lib/format";
import { t, type Key } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { DepPackage, DepReport, Project } from "../../lib/types";
import DepMigrateDialog from "./DepMigrateDialog.vue";
import DepScanDialog from "./DepScanDialog.vue";
import DepTopicLine from "./DepTopicLine.vue";

/**
 * The selected project, beside the list: its name, framework and last check, then a card a
 * topic (packages, database, runtime, updates), each with its own mark and its lines. Under a
 * line, what it is about: the packages its install would change (and a failed install's last
 * word), the pending migrations. A migrate asks first, in a dialog that shows the command and
 * where it would go (DepMigrateDialog). The scan's one button is under the managers it would
 * scan, beside what it needs; a scan's counts open what they count (DepScanDialog). `hidden`:
 * the ecosystems the language filter turned off. The page keys it by the project, so what is
 * asked here (a confirm, a failure's line) goes with the selection.
 */
const props = defineProps<{ project: Project; report: DepReport; hidden: string[]; now: number }>();
const emit = defineEmits<{ openProject: []; openOutput: [job: string] }>();

/** A list shows this many names; past it, "and N more". */
const LIST_MAX = 5;

const MARK_LABELS: Record<DepTopicLines["mark"], Key> = {
  busy: "deps.status.installing",
  bad: "deps.status.failed",
  warn: "deps.status.attention",
  ok: "deps.status.ok",
};

const lines = computed(() => depLines(props.report, props.hidden, props.now));
const topics = computed(() => depTopics(lines.value));
const installing = computed(() => anyInstalling(props.report));

const framework = computed(() => {
  const found = props.report.framework;
  return found ? t("deps.framework", { name: found.name, version: found.version }) : "";
});
const checked = computed(() => (props.report.checkedAt ? t("deps.checked", { ago: ago(props.report.checkedAt, props.now) }) : ""));

/* ---------- packages ---------- */

type Install = Extract<DepLine, { kind: "install" }>;

function shownPackages(line: Install): DepPackage[] {
  return line.packages.slice(0, LIST_MAX);
}

function morePackages(line: Install): number {
  return Math.max(0, line.differing - shownPackages(line).length);
}

/** What the install does to it: `13.32.0 → 13.34.0`, `missing · 2.1.0`, `5.0.1 · not locked`. */
function change(pkg: DepPackage): string {
  if (pkg.installed === null) return t("deps.pkg.missing", { version: pkg.locked ?? "" });
  if (pkg.locked === null) return t("deps.pkg.extra", { version: pkg.installed });
  return `${pkg.installed} → ${pkg.locked}`;
}

/** The last line the failed install wrote (its output, read through the core), under its line. */
const failureLines = ref<Record<string, string>>({});

async function readFailure(ecosystem: string): Promise<void> {
  try {
    const output = await api.output(props.project.path, installJob(ecosystem));
    const last = [...output].reverse().find((line) => line.trim() && !line.startsWith("$ ") && !line.startsWith("[pitwall]"));
    failureLines.value = { ...failureLines.value, [ecosystem]: last?.trim() ?? "" };
  } catch {
    // No output to quote; "Show output" says where to look.
  }
}

const failures = computed(() =>
  lines.value.flatMap((line) => {
    if (line.kind !== "install" || !line.failed) return [];
    const failure = installFailure(props.report, line.ecosystem);
    return failure ? [{ ecosystem: line.ecosystem, at: failure.at, error: failure.error }] : [];
  }),
);

// A failure shown (again): its output's last line, read once per failure.
watch(
  () => failures.value.map((f) => `${f.ecosystem}:${f.at}`).join(","),
  () => {
    for (const failure of failures.value) if (!failure.error) void readFailure(failure.ecosystem);
  },
  { immediate: true },
);

/** What the core said, else the output's last line. */
function failureLine(ecosystem: string): string {
  return failures.value.find((failure) => failure.ecosystem === ecosystem)?.error ?? failureLines.value[ecosystem] ?? "";
}

/* ---------- migrations ---------- */

type Migration = Extract<DepLine, { kind: "migration" }>;

/** The tool whose migrate is being confirmed, in a dialog; and its line, which the dialog shows. */
const confirming = ref<string | null>(null);
const asked = computed(() => lines.value.find((line): line is Migration => line.kind === "migration" && line.tool === confirming.value) ?? null);

/** Confirmed: the dialog closes itself, and the line shows the migrate running. */
function runMigrate(): void {
  if (confirming.value) void migrate(props.project.path, confirming.value);
}

// An install starting meanwhile takes the question back: migrate waits for it.
watch(installing, (on) => {
  if (on) confirming.value = null;
});

// Nothing pending any more (the database read again): no question left to answer.
watch(
  () => props.report.migrations.find((migrations) => migrations.tool === confirming.value)?.pending.length ?? 0,
  (count) => {
    if (!count) confirming.value = null;
  },
);

/* ---------- updates ---------- */

/** The managers a scan can look at: one button scans them all. */
const scannable = computed(() => lines.value.flatMap((line) => (line.kind === "scan" && line.scannable ? [line] : [])));
const scanning = computed(() => scannable.value.some((line) => isScanning(props.report, line.ecosystem)));
const scanFailed = computed(() => scannable.value.some((line) => line.error));

function scanAll(): void {
  void scan(
    props.project.path,
    scannable.value.map((line) => line.ecosystem),
  );
}

/** The scan whose count was pressed: its manager's lists show in a dialog, that count's first. */
const scanShown = ref<{ ecosystem: string; view: ScanView } | null>(null);
</script>

<template>
  <section class="dep-detail" :aria-label="project.name">
    <header class="dep-detail-head">
      <div class="dep-detail-heading">
        <span class="dep-detail-title">
          <span class="dep-detail-name" :title="project.name">{{ project.name }}</span>
          <span v-if="framework" class="dep-detail-meta" :title="framework">{{ framework }}</span>
        </span>
        <span class="dep-detail-sub" :title="project.path">{{ checked || project.path }}</span>
      </div>
      <button type="button" class="dep-btn" @click="emit('openProject')">{{ t("sessions.openProject") }}</button>
    </header>

    <div class="dep-detail-body">
      <div v-if="topics.length" class="dep-topics">
        <div v-for="topic in topics" :key="topic.topic" class="dep-topic">
          <span class="dep-topic-mark">
            <span :class="['dep-mark', topic.mark]" role="img" :aria-label="t(MARK_LABELS[topic.mark])"></span>
          </span>
          <span class="dep-topic-title">{{ topic.label }}</span>
          <div class="dep-topic-lines">
            <template v-for="line in topic.lines" :key="line.id">
              <DepTopicLine
                :line="line"
                :project="project"
                :report="report"
                @migrate="confirming = $event"
                @open-output="emit('openOutput', $event)"
                @details="(ecosystem: string, view: ScanView) => (scanShown = { ecosystem, view })"
              />

              <!-- A failed install's last word and the way to the rest, then what the install
                   would change. -->
              <div v-if="line.kind === 'install' && (line.packages.length || line.failed)" class="dep-under">
                <span v-if="line.failed && failureLine(line.ecosystem)" class="dep-failure-line selectable" role="alert">{{ failureLine(line.ecosystem) }}</span>
                <button v-if="line.failed" type="button" class="dep-btn" @click="emit('openOutput', installJob(line.ecosystem))">{{ t("deps.showOutput") }}</button>
                <div v-if="line.packages.length" class="dep-packages" :title="t('deps.pkg.title')">
                  <template v-for="pkg in shownPackages(line)" :key="pkg.name">
                    <span class="dep-package-name" :title="pkg.name">{{ pkg.name }}</span>
                    <span class="dep-package-change">{{ change(pkg) }}</span>
                  </template>
                </div>
                <span v-if="morePackages(line)" class="dep-more">{{ t("deps.pkg.more", { count: morePackages(line) }) }}</span>
              </div>

              <!-- The pending migrations, and what the last migrate left to say. -->
              <div v-else-if="line.kind === 'migration' && (line.pending.length || line.failure || line.ran)" class="dep-under">
                <span v-if="line.readError" class="dep-text">{{ line.readError }}</span>
                <div v-if="line.pending.length" class="dep-names">
                  <span v-for="name in line.pending.slice(0, LIST_MAX)" :key="name" :title="name">{{ name }}</span>
                </div>
                <span v-if="line.pending.length > LIST_MAX" class="dep-more">{{ t("deps.more", { count: line.pending.length - LIST_MAX }) }}</span>
                <span v-if="line.failure" class="dep-failure-line selectable">{{ line.failure }}</span>
                <!-- A migrate that ran this session: its output has a tab. -->
                <button v-if="line.ran" type="button" class="dep-btn" @click="emit('openOutput', migrateJob(line.tool))">{{ t("deps.showOutput") }}</button>
              </div>
            </template>

            <!-- The scan of every manager above: what it needs, then its button, like a line's. -->
            <div v-if="topic.topic === 'updates' && scannable.length" class="dep-scan">
              <span class="dep-hint" :title="t('deps.scanHint')">{{ t("deps.scanHint") }}</span>
              <button type="button" class="dep-btn" :disabled="scanning" :aria-busy="scanning" @click="scanAll">
                <Spinner v-if="scanning" :size="11" />
                <span class="dep-btn-text">{{ t(scanFailed ? "deps.retry" : "deps.scanButton") }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- Nothing Pitwall knows in this folder: no topic to show. -->
      <div v-else class="dep-untracked" role="note">
        <span class="dep-untracked-title">{{ t("deps.untracked.title") }}</span>
        <span class="dep-text">{{ t("deps.untracked.body") }}</span>
      </div>
    </div>

    <DepMigrateDialog v-if="asked" :project="project" :line="asked" :installing="installing" @run="runMigrate" @close="confirming = null" />
    <DepScanDialog v-if="scanShown" :project="project" :report="report" :ecosystem="scanShown.ecosystem" :view="scanShown.view" :now="now" @close="scanShown = null" />
  </section>
</template>

<style scoped src="./parts.css"></style>

<style scoped>
/* Beside the list, in the room it leaves; its own width decides how a topic's row is laid out. */
.dep-detail {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  container: dep-detail / inline-size;
}

.dep-detail-head {
  flex-shrink: 0;
  max-width: 808px;
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  padding: 16px 24px 14px;
}

.dep-detail-heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

/* The name, then its framework on the same line: the framework gives way first. */
.dep-detail-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.dep-detail-name,
.dep-detail-meta,
.dep-detail-sub {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dep-detail-name {
  flex: 0 1 auto;
  font-size: 15px;
  font-weight: 600;
}

.dep-detail-meta {
  flex: 0 9999 auto;
  font-size: 12px;
  color: var(--text-subtle);
}

.dep-detail-sub {
  font-size: 12px;
  color: var(--text-faint);
}

/* Only the topics scroll, never the window. */
.dep-detail-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  padding: 0 24px 18px;
}

/* A card a topic. No wider than its lines read well (the head above it neither): a button
   stays near what it acts on. */
.dep-topics {
  --dep-tool: 84px;
  max-width: 760px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

/* The topic's mark, its name, its lines. */
.dep-topic {
  display: grid;
  grid-template-columns: 16px 116px minmax(0, 1fr);
  gap: 12px;
  align-items: start;
  padding: 12px 16px;
  border: 1px solid var(--line);
  border-radius: var(--radius-card);
  background: var(--bg-panel);
}

/* As tall as a line, so the mark and the name sit on the first line's middle. */
.dep-topic-mark {
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.dep-topic-title {
  min-width: 0;
  font-weight: 500;
  line-height: 26px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dep-topic-lines {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

/* In a narrow pane the topic's name has a line to itself, its lines the pane's width. */
@container dep-detail (max-width: 560px) {
  .dep-detail-head {
    padding: 14px 16px 12px;
  }

  .dep-detail-body {
    padding: 0 16px 14px;
  }

  .dep-topics {
    --dep-tool: 72px;
    gap: 8px;
  }

  .dep-topic {
    grid-template-columns: 16px minmax(0, 1fr);
    row-gap: 2px;
    padding: 10px 12px;
  }

  .dep-topic-lines {
    grid-column: 2;
  }
}

/* Under a line, from where it starts to say things: past its tool. */
.dep-under {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  min-width: 0;
  padding: 2px 0 6px calc(var(--dep-tool) + 10px);
}

.dep-text {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-muted);
}

/* Under the scans, from where they start to say things; its button where the lines' are. */
.dep-scan {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  padding: 2px 0 0 calc(var(--dep-tool) + 10px);
}

.dep-hint {
  flex: 1 1 0;
  min-width: 0;
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* A name, then what the install does to it, as wide as the longest of each. */
.dep-packages {
  display: grid;
  grid-template-columns: minmax(0, max-content) max-content;
  gap: 2px 18px;
  max-width: 100%;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 16px;
}

.dep-package-name {
  min-width: 0;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dep-package-change {
  color: var(--text-subtle);
  white-space: nowrap;
}

.dep-more {
  font-size: 12px;
  color: var(--text-subtle);
}

.dep-names {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-width: 100%;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 16px;
  color: var(--text-muted);
}

.dep-names span {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* A real failure: the only red in the details. */
.dep-failure-line {
  max-width: 100%;
  padding: 7px 10px;
  border: 1px solid #1f2228;
  border-radius: 8px;
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.6;
  color: var(--crash-text);
  overflow-wrap: anywhere;
  cursor: text;
}

.dep-untracked {
  max-width: 760px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 14px 16px;
  border: 1px solid var(--line);
  border-radius: var(--radius-card);
  background: var(--bg-panel);
}

.dep-untracked-title {
  font-weight: 500;
}
</style>
