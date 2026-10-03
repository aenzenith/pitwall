<script lang="ts">
import { ref } from "vue";

type Tab = "all" | "install" | "mismatch" | "migration" | "ok";

/** The tab, the languages turned off and the selected project: kept while the app runs, never stored. */
const tab = ref<Tab>("all");
const hiddenEcosystems = ref<string[]>([]);
const selectedPath = ref<string | null>(null);
</script>

<script setup lang="ts">
import { computed, onMounted, watch } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { ago } from "../lib/format";
import { searchProjects } from "../lib/fuzzy";
import { t, type Key } from "../lib/i18n";
import {
  anyInstalling,
  ATTENTION,
  attentionRank,
  canInstall,
  canScan,
  depState,
  ecosystemName,
  install,
  installAll,
  installingAll,
  isInstalling,
  languages,
  pendingMigrations,
  scan,
  searchTerms,
  shownChecks,
  type DepState,
} from "../lib/deps";
import { useNativeMenu } from "../lib/nativeMenu";
import { dragRegion } from "../lib/platform";
import { rowKeys } from "../lib/rows";
import { api, deps, loadDeps, now, snapshot, visible } from "../lib/store";
import { tabKey } from "../lib/tabs";
import type { DepReport, Project } from "../lib/types";
import DepDetails from "./deps/DepDetails.vue";
import DepRow from "./deps/DepRow.vue";

/** Every listed project's dependencies, whatever its languages, against their lock files: the
 * projects in a list, the selected one's topics beside it. Installs and migrations run from
 * here, their output in the project's output panel. */
const emit = defineEmits<{ openProject: [path: string]; openOutput: [path: string, job: string] }>();

/** The least the re-check button spins, so a check the core answers at once still shows. */
const MIN_SPIN_MS = 500;

const TABS: Array<{ id: Tab; label: Key }> = [
  { id: "all", label: "deps.tab.all" },
  { id: "install", label: "deps.tab.install" },
  { id: "mismatch", label: "deps.tab.mismatch" },
  { id: "migration", label: "deps.tab.migration" },
  { id: "ok", label: "deps.tab.ok" },
];

const projects = computed(() => snapshot.value?.projects ?? []);
const byPath = computed(() => new Map((deps.value ?? []).map((report) => [report.path, report])));

/** The listed projects that have a report, in the list's order. */
const listed = computed(() =>
  projects.value.flatMap((project) => {
    const report = byPath.value.get(project.path);
    return report ? [{ project, report }] : [];
  }),
);

/** Some listed project has a report: else the page says what it would show. */
const reported = computed(() => listed.value.length > 0);

/* ---------- languages ---------- */

/** The languages among the listed projects, the most used first: the filter menu's lines. */
const present = computed(() => languages(listed.value.map((item) => item.report)));

/** The languages turned off, among those present; never every one of them. */
const hidden = computed(() => {
  const ids = present.value.map((language) => language.id);
  const off = hiddenEcosystems.value.filter((id) => ids.includes(id));
  return off.length < ids.length ? off : [];
});

/** The menu button says what is shown: every language, the one left, or how many. */
const languageLabel = computed(() => {
  if (!hidden.value.length) return t("deps.allLanguages");
  const on = present.value.filter((language) => !hidden.value.includes(language.id));
  return on.length === 1 ? ecosystemName(on[0].id) : t("deps.languages", { count: on.length });
});

/** A language shown or hidden; the last one shown stays. */
function toggleLanguage(id: string): void {
  const off = hidden.value;
  if (off.includes(id)) hiddenEcosystems.value = off.filter((other) => other !== id);
  else if (off.length < present.value.length - 1) hiddenEcosystems.value = [...off, id];
}

const menu = useNativeMenu();

/** The native menu under the button: every language, then a check item a language with how many
 * projects use it. */
function pickLanguages(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const off = hidden.value;
  void menu.popup({ clientX: box.left, clientY: box.bottom + 4 }, [
    { text: t("deps.allLanguages"), checked: !off.length, action: () => (hiddenEcosystems.value = []) },
    "separator",
    ...present.value.map(({ id, count }) => ({
      text: t("deps.languageCount", { name: ecosystemName(id), count }),
      checked: !off.includes(id),
      action: () => toggleLanguage(id),
    })),
  ]);
}

/* ---------- rows ---------- */

/** A project and its report over the shown languages. `rank`: why it needs you, most pressing
 * first (lib/deps: ATTENTION); `pending`: its pending migrations. */
type Entry = { project: Project; report: DepReport; attention: boolean; rank: number; state: DepState; pending: number };

/** With a language turned off, only the projects with a manager in one still on. */
const entries = computed<Entry[]>(() => {
  const off = hidden.value;
  return listed.value.flatMap(({ project, report }) => {
    if (off.length && !shownChecks(report, off).length) return [];
    const rank = attentionRank(report, off);
    return [{ project, report, rank, attention: rank !== ATTENTION.none, state: depState(report, off), pending: pendingMigrations(report) }];
  });
});

const query = ref("");
const q = computed(() => query.value.trim().toLowerCase());
/** The entries the search leaves: by the project (name, branch, path), or by one of its
 * languages, package managers, framework or migration tool. The tabs pick from these. */
const pool = computed(() => {
  if (!q.value) return entries.value;
  const found = new Set(searchProjects(entries.value.map((entry) => entry.project), q.value).map((match) => match.project.path));
  return entries.value.filter((entry) => found.has(entry.project.path) || searchTerms(entry.report).some((term) => term.includes(q.value)));
});

/** "To install" and "Version mismatch" are the project's state (an install comes first);
 * "Migration": migrations pending in its database, whatever else; "Compatible": nothing to do
 * at all. */
function inTab(entry: Entry, id: Tab): boolean {
  switch (id) {
    case "all":
      return true;
    case "ok":
      return !entry.attention;
    case "migration":
      return entry.pending > 0;
    default:
      return entry.state === id;
  }
}

const counts = computed(() => {
  const found: Record<Tab, number> = { all: 0, install: 0, mismatch: 0, migration: 0, ok: 0 };
  for (const entry of pool.value) for (const { id } of TABS) if (inTab(entry, id)) found[id]++;
  return found;
});

/** The rows, attention first (to install, then a runtime that doesn't fit, pending migrations,
 * a missing tool; the list's order within each); under "All", in two groups with their headings. */
const groups = computed(() => {
  const shown = pool.value.filter((entry) => inTab(entry, tab.value));
  const attention = shown.filter((entry) => entry.attention).sort((a, b) => a.rank - b.rank);
  const rest = shown.filter((entry) => !entry.attention);
  const headed = tab.value === "all";
  return [
    { id: "attention", label: headed ? t("deps.group.attention", { count: attention.length }) : "", entries: attention },
    { id: "ok", label: headed ? t("deps.group.ok", { count: rest.length }) : "", entries: rest },
  ].filter((group) => group.entries.length);
});

const rows = computed(() => groups.value.flatMap((group) => group.entries));
/** The project whose topics show beside the list: the one picked, else (never picked, or the
 * tabs and the search left it out) the first. It is the list's one Tab stop too. */
const selected = computed(() => rows.value.find((entry) => entry.project.path === selectedPath.value) ?? rows.value[0] ?? null);

/* ---------- header ---------- */

/** `9 projects · 7 languages · lock files read 1 min ago`. */
const subtitle = computed(() => {
  const reports = entries.value.map((entry) => entry.report);
  const parts = [t("deps.projects", { count: reports.length })];
  const shown = present.value.length - hidden.value.length;
  if (shown) parts.push(t("deps.languages", { count: shown }));
  const read = Math.max(0, ...reports.map((report) => report.checkedAt));
  if (read) parts.push(t("deps.read", { ago: ago(read, now.value) }));
  return parts.join(" · ");
});

/** Every install Pitwall can run in the shown languages, any tab or search: packages differing,
 * the manager on this machine, a command to run. Never a migrate. */
const needed = computed(() =>
  entries.value.flatMap(({ project, report }) =>
    shownChecks(report, hidden.value)
      .filter((check) => canInstall(check) && !isInstalling(report, check.ecosystem))
      .map((check) => ({ path: project.path, name: project.name, ecosystem: check.ecosystem, command: check.installCommand ?? "" })),
  ),
);

/** What the button would run, a line a project: `okul-api: uv sync, npm install`. */
const neededTitle = computed(() => {
  const lines = new Map<string, { name: string; commands: string[] }>();
  for (const item of needed.value) {
    const line = lines.get(item.path) ?? { name: item.name, commands: [] };
    line.commands.push(item.command);
    lines.set(item.path, line);
  }
  return [...lines.values()].map((line) => `${line.name}: ${line.commands.join(", ")}`).join("\n");
});

function installNeeded(): void {
  void installAll(needed.value.map(({ path, ecosystem }) => ({ path, ecosystem })));
}

const checking = ref(false);

async function recheck(): Promise<void> {
  if (checking.value) return;
  checking.value = true;
  const started = Date.now();
  await api.depsCheck(null).catch(() => undefined);
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  checking.value = false;
}

/* ---------- keys and menus ---------- */

function select(path: string): void {
  selectedPath.value = path;
}

/** The tabs are one Tab stop: ←/→ (Home, End) pick and focus the next. */
function onTabKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, TABS.length, at);
  if (next !== null) tab.value = TABS[next].id;
}

/** The list is one Tab stop: ↑/↓ (Home, End) select the next project; its buttons are beside
 * the list, a Tab away. */
function onListKey(event: KeyboardEvent): void {
  rowKeys(event, { move: select, enter: select, space: select });
}

/** ↓ in the search box goes on to the list. */
function focusList(): void {
  document.querySelector<HTMLElement>('.deps-rows [data-path][tabindex="0"]')?.focus();
}

/** Right-click on a project in the list: the native menu with what can be done for it, each
 * install by its own command. */
function rowMenu(event: MouseEvent, entry: Entry): void {
  const { project, report } = entry;
  const checks = shownChecks(report, hidden.value);
  const scannable = checks.filter(canScan).map((check) => check.ecosystem);
  void menu.popup(event, [
    ...checks.filter(canInstall).map((check) => ({ text: check.installCommand ?? "", enabled: !anyInstalling(report), action: () => void install(project.path, check.ecosystem) })),
    { text: t("deps.scanButton"), enabled: scannable.length > 0, action: () => void scan(project.path, scannable) },
    { text: t("deps.recheck"), action: () => void api.depsCheck(project.path).catch(() => undefined) },
    "separator",
    { text: t("sessions.openProject"), action: () => emit("openProject", project.path) },
    { text: t("common.openInEditor"), action: () => void api.openEditor(project.path) },
  ]);
}

/* ---------- text ---------- */

const noneText = computed(() => {
  if (q.value) return t("deps.none.search");
  switch (tab.value) {
    case "install":
      return t("deps.none.install");
    case "mismatch":
      return t("deps.none.mismatch");
    case "migration":
      return t("deps.none.migration");
    case "ok":
      return t("deps.none.ok");
    default:
      // Nothing under "All" without a search: no project uses a language still shown.
      return t("deps.none.language");
  }
});

/* ---------- life ---------- */

// The core pushes every change (`deps`); on opening, and back on screen, a fresh look.
onMounted(() => void loadDeps());
watch(visible, (on) => {
  if (on) void loadDeps();
});
</script>

<template>
  <main class="deps">
    <!-- Its heading and free space drag the window; the search box and buttons stay clickable. -->
    <header class="deps-bar" :data-tauri-drag-region="dragRegion">
      <div class="deps-heading">
        <span class="deps-title">{{ t("deps.title") }}</span>
        <span class="deps-subtitle">{{ deps ? subtitle : "" }}</span>
      </div>
      <label class="sr-only" for="deps-search">{{ t("common.searchProjects") }}</label>
      <input id="deps-search" v-model="query" type="search" :placeholder="t('common.searchProjects')" @keydown.down.prevent="focusList" />
      <button type="button" class="deps-control" :title="t('deps.recheckTitle')" :aria-busy="checking" :disabled="!deps" @click="recheck">
        <Spinner v-if="checking" />
        {{ t("deps.recheck") }}
      </button>
      <button type="button" class="deps-control" :title="neededTitle" :aria-busy="installingAll" :disabled="!needed.length || installingAll" @click="installNeeded">
        <Spinner v-if="installingAll" />
        {{ needed.length ? t("deps.installAllCount", { count: needed.length }) : t("deps.installAll") }}
      </button>
    </header>

    <!-- The tabs and the language menu. -->
    <div class="deps-filters">
      <div role="tablist" class="deps-tabs" :aria-label="t('deps.filters')">
        <button
          v-for="(item, i) in TABS"
          :key="item.id"
          type="button"
          role="tab"
          :aria-selected="tab === item.id"
          :tabindex="tab === item.id ? 0 : -1"
          :class="{ on: tab === item.id }"
          :title="t(item.label)"
          @click="tab = item.id"
          @keydown="onTabKey($event, i)"
        >
          <span class="deps-tab-label">{{ t(item.label) }}</span>
          <span v-if="deps" class="deps-tab-count">{{ counts[item.id] }}</span>
        </button>
      </div>
      <!-- Which languages are shown: a native menu of check items, so only with more than one. -->
      <button
        v-if="present.length > 1"
        type="button"
        class="deps-languages"
        aria-haspopup="menu"
        :aria-label="t('deps.languageFilter', { name: languageLabel })"
        :title="t('deps.languageFilter', { name: languageLabel })"
        @click="pickLanguages"
      >
        <span class="deps-languages-text">{{ languageLabel }}</span>
        <Icon name="chevron-down" :size="12" />
      </button>
    </div>

    <span id="deps-keys" class="sr-only">{{ t("deps.rowKeys") }}</span>

    <div v-if="!deps" class="deps-loading" role="status">
      <Spinner />
      <span>{{ t("deps.loading") }}</span>
    </div>

    <section v-else-if="!reported" class="deps-empty" role="note">
      <p class="deps-empty-title">{{ t("deps.empty.title") }}</p>
      <p class="deps-empty-body">{{ t("deps.empty.body") }}</p>
    </section>

    <p v-else-if="!selected" class="deps-none">{{ noneText }}</p>

    <div v-else class="deps-split">
      <!-- Each half scrolls on its own, never the window. -->
      <div class="deps-rows" role="listbox" :aria-label="t('deps.title')" aria-describedby="deps-keys" @keydown="onListKey">
        <template v-for="group in groups" :key="group.id">
          <div v-if="group.label" class="deps-group section-label" aria-hidden="true">{{ group.label }}</div>
          <DepRow
            v-for="entry in group.entries"
            :key="entry.project.path"
            :project="entry.project"
            :report="entry.report"
            :attention="entry.attention"
            :hidden="hidden"
            :selected="selected.project.path === entry.project.path"
            :focusable="selected.project.path === entry.project.path"
            :now="now"
            @select="select(entry.project.path)"
            @menu="rowMenu($event, entry)"
          />
        </template>
      </div>
      <DepDetails
        :key="selected.project.path"
        :project="selected.project"
        :report="selected.report"
        :hidden="hidden"
        :now="now"
        @open-project="emit('openProject', selected.project.path)"
        @open-output="(job: string) => emit('openOutput', selected!.project.path, job)"
      />
    </div>
  </main>
</template>

<style scoped>
.deps {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

/* As tall as the projects toolbar, so switching never moves the title; no line under it: the
   filters below belong to the same head. */
.deps-bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
}

.deps-heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.deps-title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.deps-subtitle {
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* In a narrow window the search box gives way first, then the subtitle; the buttons never do. */
input[type="search"] {
  width: 220px;
  min-width: 96px;
  flex-shrink: 3;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  text-overflow: ellipsis;
}

input[type="search"]::placeholder {
  color: var(--text-faint);
}

.deps-control {
  flex-shrink: 0;
  height: 30px;
  padding: 0 12px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.deps-control:hover:not(:disabled) {
  background: #2c3039;
}

.deps-control:disabled {
  opacity: 0.4;
}

.deps-control[aria-busy="true"]:disabled {
  opacity: 0.7;
}

.deps-control :deep(.spinner) {
  color: var(--text-muted);
}

.deps-filters {
  height: 44px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 20px 8px;
  border-bottom: 1px solid #1e2127;
  container: deps-filters / inline-size;
}

/* A column a tab, each as wide as its name where there is room. Where there isn't, the room is
   shared out equally and a short name is full first: only the longest names give way. */
.deps-tabs {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: minmax(0, max-content);
  min-width: 0;
  gap: 4px;
}

/* In a narrow window a tab's name gives way (its tooltip keeps it); its count never does. */
.deps-tabs button {
  height: 28px;
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 12px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 13px;
  color: var(--text-subtle);
}

.deps-tabs button:hover:not(.on) {
  background: #1b1e24;
  color: var(--text-muted);
}

.deps-tabs button.on {
  background: #23272e;
  color: var(--text-strong);
}

.deps-tab-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.deps-tab-count {
  flex-shrink: 0;
  min-width: 20px;
  height: 18px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 9px;
  background: var(--bg-control);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  color: var(--text-faint);
}

.deps-tabs button.on .deps-tab-count {
  background: #2c3039;
  color: var(--text-muted);
}

/* The language menu's button, at the right; its label gives way before it pushes the tabs out. */
.deps-languages {
  flex-shrink: 0;
  max-width: 40%;
  margin-left: auto;
  height: 28px;
  padding: 0 8px 0 10px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
}

.deps-languages:hover {
  background: #2c3039;
}

.deps-languages-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.deps-languages :deep(svg) {
  flex-shrink: 0;
  color: var(--text-subtle);
}

/* Down to the window's least width the tabs sit closer, so their names stay whole longer. */
@container deps-filters (max-width: 760px) {
  .deps-tabs {
    gap: 2px;
  }

  .deps-tabs button {
    gap: 6px;
    padding: 0 6px 0 8px;
  }

  .deps-languages {
    padding: 0 6px 0 8px;
  }
}

.deps-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 48px 0;
  color: var(--text-muted);
}

.deps-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 48px 24px;
  text-align: center;
}

.deps-empty p {
  margin: 0;
}

.deps-empty-title {
  font-weight: 600;
}

.deps-empty-body {
  max-width: 420px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-muted);
}

.deps-split {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  container: deps-split / inline-size;
}

/* The projects, in a column of their own; beside it the selected one's details take the rest. */
.deps-rows {
  width: 312px;
  flex-shrink: 0;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 10px 10px;
  border-right: 1px solid #1e2127;
}

/* In a narrow window (down to its least width) the list gives some of its room to the details. */
@container deps-split (max-width: 860px) {
  .deps-rows {
    width: 248px;
  }
}

/* Like the rows, a heading keeps its height when the list is taller than the window. */
.deps-group {
  flex-shrink: 0;
  padding: 12px 12px 6px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.deps-none {
  margin: 0;
  padding: 40px 12px;
  text-align: center;
  color: var(--text-muted);
}
</style>
