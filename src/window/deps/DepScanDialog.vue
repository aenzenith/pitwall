<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { advisoryLabel, bumpText, canScan, fixText, isScanning, managerName, scan, scanErrorText, severityText, type ScanView } from "../../lib/deps";
import { useBackdropClose } from "../../lib/dialog";
import { ago } from "../../lib/format";
import { t, type Key } from "../../lib/i18n";
import { useGrow } from "../../lib/slide";
import { api } from "../../lib/store";
import { tabKey } from "../../lib/tabs";
import type { DepAdvisory, DepReport, DepScanDetails, Project } from "../../lib/types";

/**
 * What a scan's counts are of, for one manager of a project: its outdated packages and the
 * advisories against its packages, a table each; the one whose count was pressed (`view`) shows
 * first. The lists are the last scan's, asked from the core when the dialog opens and again
 * when a scan ends. Nothing here changes the project: an advisory's link opens in the browser,
 * and "Scan again" runs the read-only scan.
 */
const props = defineProps<{ project: Project; report: DepReport; ecosystem: string; view: ScanView; now: number }>();
const emit = defineEmits<{ close: [] }>();

const VIEWS: Array<{ id: ScanView; label: Key }> = [
  { id: "updates", label: "deps.topic.updates" },
  { id: "vulns", label: "deps.scan.tab.vulns" },
];

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);

const shown = ref<ScanView>(props.view);
// The two lists are rarely as long: the dialog grows or shrinks to the other one's height.
useGrow(dialog, shown);

const check = computed(() => props.report.checks.find((found) => found.ecosystem === props.ecosystem));
const manager = computed(() => managerName(check.value?.manager ?? props.ecosystem));
const last = computed(() => props.report.scans[props.ecosystem] ?? null);
const scanning = computed(() => isScanning(props.report, props.ecosystem));
const scannable = computed(() => !!check.value && canScan(check.value));

const subtitle = computed(() => {
  const parts = [props.project.name, manager.value];
  if (last.value) parts.push(t("deps.scan.last", { ago: ago(last.value.at, props.now) }));
  return parts;
});

/** The page's counts, beside each view's name; none where the scan didn't get one. */
const counts = computed<Record<ScanView, number | null>>(() => ({ updates: last.value?.outdated ?? null, vulns: last.value?.vulnerable ?? null }));

/* ---------- the lists ---------- */

/** `undefined`: not answered yet; `null`: the scan kept none (made before they were kept). */
const details = ref<DepScanDetails | null | undefined>(undefined);

async function load(): Promise<void> {
  try {
    details.value = await api.depsScanDetails(props.project.path, props.ecosystem);
  } catch {
    details.value = null;
  }
}

// showModal focuses the first control (the shown view's tab); the lists open with nothing focused.
onMounted(() => {
  dialog.value?.showModal();
  (document.activeElement as HTMLElement | null)?.blur();
  void load();
});

// A scan that ended, from here or from the page: its lists are new.
watch(
  () => last.value?.running ?? false,
  (running) => {
    if (!running) void load();
  },
);

const updates = computed(() => details.value?.outdated ?? null);

/** The advisories, each with whether it is its package's first: the package is named once. */
const advisories = computed(() => {
  const list = details.value?.advisories ?? null;
  return list && list.map((advisory, at) => ({ advisory, first: at === 0 || list[at - 1].package !== advisory.package }));
});

/** How many the scan counted past the ones it kept (the core keeps 300 of each). */
const moreUpdates = computed(() => Math.max(0, (counts.value.updates ?? 0) - (updates.value?.length ?? 0)));

/** The newest the constraint allows, where the manager told one that isn't installed yet. */
function wanted(installed: string | null, found: string | null): string {
  return found && found !== installed ? found : "—";
}

/** `high` and `critical` are the red dot, `moderate` the yellow one, the rest a ring. */
function dot(severity: DepAdvisory["severity"]): string {
  if (severity === "critical" || severity === "high") return "bad";
  return severity === "moderate" ? "warn" : "ok";
}

/** What the advisory says: its title; for a package with none of its own, what it comes with. */
function advisoryText(advisory: DepAdvisory): string {
  return advisory.title ?? (advisory.via ? t("deps.scan.via", { names: advisory.via }) : "—");
}

/** Why a view has no table: the scan failed there, or (the advisories') the manager has no
 * audit of its own. */
const missing = computed(() => {
  if (last.value?.error) return scanErrorText(last.value.error);
  return shown.value === "vulns" ? t("deps.scan.noAudit", { name: manager.value }) : scanErrorText(null);
});

/* ---------- keys and actions ---------- */

/** The views are one Tab stop: ←/→ (Home, End) pick and focus the next. */
function onViewKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, VIEWS.length, at);
  if (next !== null) shown.value = VIEWS[next].id;
}

function rescan(): void {
  void scan(props.project.path, [props.ecosystem]);
}
</script>

<template>
  <dialog ref="dialog" class="scan-dialog" :aria-label="t('deps.scan.details')" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="sd-body">
      <header class="sd-head">
        <div class="sd-heading">
          <span class="sd-title">{{ t("deps.scan.details") }}</span>
          <span class="sd-sub" :title="subtitle.join(' · ')">
            <template v-for="(part, at) in subtitle" :key="at">
              <span v-if="at" aria-hidden="true"> · </span>
              <span :class="{ mono: at === 1 }">{{ part }}</span>
            </template>
          </span>
        </div>
        <div role="tablist" class="sd-views" :aria-label="t('deps.filters')">
          <button
            v-for="(item, at) in VIEWS"
            :key="item.id"
            type="button"
            role="tab"
            :aria-selected="shown === item.id"
            :tabindex="shown === item.id ? 0 : -1"
            :class="{ on: shown === item.id }"
            @click="shown = item.id"
            @keydown="onViewKey($event, at)"
          >
            {{ t(item.label) }}
            <span v-if="counts[item.id] !== null" class="sd-count">{{ counts[item.id] }}</span>
          </button>
        </div>
      </header>

      <div v-if="details === undefined" class="sd-note" role="status">
        <Spinner :size="12" />
      </div>

      <!-- A scan from before its lists were kept: only its counts are left. -->
      <div v-else-if="details === null" class="sd-note" role="note">
        <span class="sd-note-title">{{ t("deps.scan.noDetails.title") }}</span>
        <span class="sd-note-text">{{ t("deps.scan.noDetails.body") }}</span>
      </div>

      <template v-else-if="shown === 'updates'">
        <div v-if="updates === null" class="sd-note" role="note">
          <span class="sd-note-text">{{ missing }}</span>
        </div>
        <div v-else-if="!updates.length" class="sd-note" role="note">
          <span class="sd-note-text">{{ t("deps.scan.empty.updates") }}</span>
        </div>
        <div v-else role="tabpanel" :class="['sd-table', { stale: scanning }]" :aria-label="t('deps.topic.updates')">
          <div class="sd-row sd-updates sd-heads">
            <span>{{ t("deps.scan.col.package") }}</span>
            <span>{{ t("deps.scan.col.installed") }}</span>
            <span>{{ t("deps.scan.col.wanted") }}</span>
            <span>{{ t("deps.scan.col.latest") }}</span>
            <span>{{ t("deps.scan.col.kind") }}</span>
          </div>
          <div class="sd-rows">
            <div v-for="pkg in updates" :key="pkg.name" class="sd-row sd-updates">
              <span class="sd-cell mono" :title="pkg.name">{{ pkg.name }}</span>
              <span class="sd-cell mono dim" :title="pkg.installed ?? undefined">{{ pkg.installed ?? "—" }}</span>
              <span class="sd-cell mono dim" :title="pkg.wanted ?? undefined">{{ wanted(pkg.installed, pkg.wanted) }}</span>
              <span class="sd-cell mono" :title="pkg.latest ?? undefined">{{ pkg.latest ?? "—" }}</span>
              <span class="sd-cell subtle">{{ bumpText(pkg.installed, pkg.latest) }}</span>
            </div>
            <span v-if="moreUpdates" class="sd-more">{{ t("deps.more", { count: moreUpdates }) }}</span>
          </div>
        </div>
      </template>

      <template v-else>
        <div v-if="advisories === null" class="sd-note" role="note">
          <span class="sd-note-text">{{ missing }}</span>
        </div>
        <div v-else-if="!advisories.length" class="sd-note" role="note">
          <span class="sd-note-text">{{ t("deps.scan.empty.vulns") }}</span>
        </div>
        <div v-else role="tabpanel" :class="['sd-table', { stale: scanning }]" :aria-label="t('deps.scan.tab.vulns')">
          <div class="sd-row sd-vulns sd-heads">
            <span>{{ t("deps.scan.col.severity") }}</span>
            <span>{{ t("deps.scan.col.package") }}</span>
            <span>{{ t("deps.scan.col.advisory") }}</span>
            <span>{{ t("deps.scan.col.fix") }}</span>
          </div>
          <div class="sd-rows">
            <div v-for="({ advisory, first }, at) in advisories" :key="at" :class="['sd-row', 'sd-vulns', 'sd-advisory', { first }]">
              <span class="sd-severity">
                <span class="sd-dot"><span :class="['dep-mark', dot(advisory.severity)]" aria-hidden="true"></span></span>
                {{ severityText(advisory.severity) }}
              </span>
              <span class="sd-package">
                <template v-if="first">
                  <span class="sd-cell mono" :title="advisory.package">{{ advisory.package }}</span>
                  <span v-if="advisory.installed || advisory.direct === false" class="sd-cell sd-small">
                    <span v-if="advisory.installed" class="mono">{{ advisory.installed }}</span>
                    <span v-if="advisory.installed && advisory.direct === false" aria-hidden="true"> · </span>
                    <span v-if="advisory.direct === false">{{ t("deps.scan.indirect") }}</span>
                  </span>
                </template>
              </span>
              <span class="sd-about">
                <span class="sd-about-text selectable">{{ advisoryText(advisory) }}</span>
                <span v-if="advisory.affected || advisory.url" class="sd-small sd-about-more">
                  <span v-if="advisory.affected" class="mono selectable">{{ advisory.affected }}</span>
                  <button v-if="advisory.url" type="button" class="sd-link" :title="advisory.url" @click="api.openUrl(advisory.url)">
                    <span class="mono">{{ advisoryLabel(advisory.url) }}</span>
                    <Icon name="external" :size="11" />
                  </button>
                </span>
              </span>
              <span :class="['sd-cell', { mono: !!advisory.fix }]" :title="fixText(advisory)">{{ fixText(advisory) }}</span>
            </div>
          </div>
        </div>
      </template>

      <footer class="sd-foot">
        <template v-if="scannable">
          <button type="button" class="sd-control" :disabled="scanning" :aria-busy="scanning" @click="rescan">
            <Spinner v-if="scanning" :size="11" />
            {{ scanning ? t("deps.scan.running") : t("deps.scan.again") }}
          </button>
          <span class="sd-hint">{{ t("deps.scanHint") }}</span>
        </template>
        <span class="sd-grow"></span>
        <button type="button" class="sd-control" @click="dialog?.close()">{{ t("common.close") }}</button>
      </footer>
    </div>
  </dialog>
</template>

<style scoped src="./parts.css"></style>

<style scoped>
/* The same look as the app's other dialogs, wider: it holds tables. */
.scan-dialog {
  width: 680px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 64px);
  padding: 0;
  border: 1px solid #2f343d;
  border-radius: 12px;
  background: #1b1e24;
  color: var(--text);
  font-size: 13px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
  overflow: hidden;
}

.scan-dialog[open] {
  display: flex;
  animation: sd-pop 0.16s ease-out;
}

.scan-dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

@keyframes sd-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .scan-dialog[open] {
    animation: none;
  }
}

/* The head and the foot keep their place; the table between them scrolls. */
.sd-body {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px 20px 16px;
}

.sd-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.sd-heading {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sd-title {
  font-size: 15px;
  font-weight: 600;
}

.sd-sub {
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mono {
  font-family: var(--font-mono);
}

/* The two views: which list shows. */
.sd-views {
  flex-shrink: 0;
  display: flex;
  gap: 2px;
  padding: 3px;
  border: 1px solid #262a31;
  border-radius: 9px;
  background: var(--bg-detail);
}

.sd-views button {
  height: 26px;
  padding: 0 10px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.sd-views button:hover:not(.on) {
  color: var(--text-muted);
}

.sd-views button.on {
  background: var(--line-strong);
  color: var(--text-strong);
}

.sd-count {
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

/* In place of a table: why there is none. */
.sd-note {
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 12px;
  overflow: hidden;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-card);
  background: var(--bg-panel);
}

.sd-note-title {
  font-weight: 500;
}

.sd-note-text {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-muted);
}

.sd-note :deep(.spinner) {
  color: var(--text-muted);
}

.sd-table {
  flex: 0 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-card);
  background: var(--bg-panel);
  overflow: hidden;
}

/* A scan runs: the lists shown are the last one's. */
.sd-table.stale .sd-rows {
  opacity: 0.6;
}

.sd-rows {
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  padding: 4px 0;
}

/* The dialog on its way to this list's height (useGrow): squeezed, it shows no scrollbar. */
.scan-dialog[data-growing] .sd-rows {
  overflow-y: hidden;
}

.sd-row {
  flex-shrink: 0;
  display: grid;
  gap: 12px;
  padding: 0 12px;
}

/* The package, what is installed, what the constraint allows, the newest, how far it is. */
.sd-updates {
  grid-template-columns: minmax(0, 1fr) 84px 96px 84px 60px;
  align-items: center;
  min-height: 28px;
}

/* How severe, the package, what the advisory says, what fixes it. */
.sd-vulns {
  grid-template-columns: 96px 140px minmax(0, 1fr) 96px;
  align-items: start;
}

.sd-heads {
  min-height: 30px;
  align-items: center;
  border-bottom: 1px solid var(--line);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
}

.sd-heads span {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

:lang(zh) .sd-heads,
:lang(ja) .sd-heads {
  letter-spacing: 0;
}

.sd-cell {
  min-width: 0;
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sd-cell.dim {
  color: var(--text-muted);
}

.sd-cell.subtle {
  color: var(--text-subtle);
}

/* An advisory: a hairline over each package's first, its further ones close under it. */
.sd-advisory {
  padding-top: 4px;
  padding-bottom: 8px;
}

.sd-advisory.first {
  padding-top: 10px;
}

.sd-advisory.first:not(:first-child) {
  border-top: 1px solid var(--line);
}

.sd-severity {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  font-size: 12px;
  font-weight: 600;
  line-height: 18px;
}

/* As wide as the largest mark, so every severity's name starts at the same place. */
.sd-dot {
  width: 9px;
  flex-shrink: 0;
  display: inline-flex;
  justify-content: center;
}

.sd-package,
.sd-about {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.sd-small {
  font-size: 11px;
  line-height: 16px;
  color: var(--text-subtle);
}

.sd-about-text {
  font-size: 12px;
  line-height: 18px;
  color: #c7ccd3;
  overflow-wrap: anywhere;
  cursor: text;
}

.sd-about-more {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 10px;
  color: var(--text-faint);
}

/* The advisory's own page, in the browser. */
.sd-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 11px;
  color: var(--text-muted);
  text-decoration: underline dotted #5d636d;
  text-underline-offset: 3px;
}

.sd-link:hover {
  color: var(--text-strong);
}

.sd-more {
  padding: 4px 12px 2px;
  font-size: 12px;
  color: var(--text-subtle);
}

.sd-foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  /* At the dialog's bottom edge while its height is on its way (useGrow). */
  margin-top: auto;
  padding-top: 2px;
}

.sd-hint {
  min-width: 0;
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sd-grow {
  flex-grow: 1;
}

.sd-control {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.sd-control:hover:not(:disabled) {
  background: #2c3039;
}

.sd-control:disabled {
  opacity: 0.7;
}

.sd-control :deep(.spinner) {
  color: var(--text-muted);
}
</style>
