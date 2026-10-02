<script lang="ts">
import { ref } from "vue";

import type { Filter } from "../lib/sessions";

/** The filter tab and "listed projects only": kept while the app runs, never stored. */
const filter = ref<Filter>("all");
const listedOnly = ref(false);
</script>

<script setup lang="ts">
import { computed, onMounted, watch } from "vue";

import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { useNativeMenu } from "../lib/nativeMenu";
import { dragRegion } from "../lib/platform";
import { bringKeys, FILTERS, keepOrder, layout, matches, resumeCommand, sectionOf, type Layout } from "../lib/sessions";
import { api, loadSessions, now, sessions } from "../lib/store";
import { tabKey } from "../lib/tabs";
import SessionDetail from "./sessions/SessionDetail.vue";
import SessionList from "./sessions/SessionList.vue";
import SessionNotice from "./sessions/SessionNotice.vue";

/** Every Claude Code session today, live. `claudeProject`: where "Open a Claude terminal" would
 * open one (the project list's selection), if anywhere. */
defineProps<{ claudeProject: string | null }>();
const emit = defineEmits<{ openProject: [path: string]; openSettings: []; openClaude: [] }>();

/** The least an action's spinner shows, so one the core answers at once still shows. */
const MIN_SPIN_MS = 400;

const view = computed(() => sessions.value);
const all = computed(() => view.value?.sessions ?? []);
const byId = computed(() => new Map(all.value.map((row) => [row.id, row])));

/* ---------- what the list shows ---------- */

const query = ref("");
const q = computed(() => query.value.trim().toLowerCase());
/** The sessions the search and "listed projects only" leave; the tabs pick from these. */
const pool = computed(() => all.value.filter((row) => (!listedOnly.value || row.path !== null) && matches(row, q.value)));

const counts = computed(() => {
  const found: Record<Filter, number> = { all: pool.value.length, waiting: 0, working: 0, ended: 0 };
  for (const row of pool.value) {
    const section = sectionOf(row);
    if (section === "waiting" || section === "working" || section === "ended") found[section]++;
  }
  return found;
});

const current = computed(() => FILTERS.find((f) => f.id === filter.value) ?? FILTERS[0]);
const fresh = computed(() => layout(pool.value, current.value.sections));

/**
 * While the pointer or the keyboard is on the list, rows keep their places (lib/sessions:
 * keepOrder): a session that changes state stays put until the list is let go.
 */
const holding = ref(false);
const frozen = ref<Layout | null>(null);
const shown = computed<Layout>(() => (holding.value && frozen.value ? keepOrder(frozen.value, fresh.value, new Set(pool.value.map((row) => row.id))) : fresh.value));

function hold(on: boolean): void {
  holding.value = on;
  frozen.value = on ? (frozen.value ?? fresh.value) : null;
}

// A new filter or search is a new list: its order is taken as it stands.
watch([filter, q, listedOnly], () => {
  if (holding.value) frozen.value = fresh.value;
});

/* ---------- selection ---------- */

const selectedId = ref<string | null>(null);
const shownIds = computed(() => shown.value.flatMap((group) => group.ids));

watch(
  shownIds,
  (ids) => {
    if (!selectedId.value || !ids.includes(selectedId.value)) selectedId.value = ids[0] ?? null;
  },
  { immediate: true },
);

const selected = computed(() => (selectedId.value ? (byId.value.get(selectedId.value) ?? null) : null));

/** The selected session's project's other sessions waiting on you: marking it seen clears them too. */
const others = computed(() => {
  const row = selected.value;
  if (!row) return 0;
  const where = row.path ?? row.folder;
  return all.value.filter((other) => other.id !== row.id && other.phase === "waiting" && (other.path ?? other.folder) === where).length;
});

/** ↓ in the search box goes on to the selected row. */
function focusSelected(): void {
  document.querySelector<HTMLElement>('.sessions [data-path][tabindex="0"]')?.focus();
}

/** The tabs are one Tab stop: ←/→ (Home, End) pick and focus the next. */
function onTabKey(event: KeyboardEvent, at: number): void {
  const next = tabKey(event, FILTERS.length, at);
  if (next !== null) filter.value = FILTERS[next].id;
}

/* ---------- actions ---------- */

/** The action on its way (its button spins), and why adding a folder failed. */
const pending = ref<{ id: string; action: "bringUp" | "addProject" } | null>(null);
const addError = ref<{ id: string; text: string } | null>(null);

async function run(id: string, action: "bringUp" | "addProject", work: () => Promise<unknown>): Promise<void> {
  if (pending.value) return;
  pending.value = { id, action };
  const started = Date.now();
  try {
    await work();
    if (action === "addProject") addError.value = null;
  } catch (error) {
    if (action === "addProject") addError.value = { id, text: String(error) };
  }
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  pending.value = null;
}

/** Where it runs: its tab or terminal comes up; an ended one opens again in the editor. */
function bringUp(id: string): void {
  const row = byId.value.get(id);
  if (row) void run(id, "bringUp", () => api.revealClaude(row.path ?? row.folder, row.id));
}

/** Seen is kept per project: every session of the project stops waiting on you. */
function markSeen(id: string): void {
  const row = byId.value.get(id);
  if (row?.phase === "waiting") void api.markSeen(row.path ?? row.folder).catch(() => undefined);
}

function openProject(id: string): void {
  const path = byId.value.get(id)?.path;
  if (path) emit("openProject", path);
}

function addProject(id: string): void {
  const row = byId.value.get(id);
  if (row) void run(id, "addProject", () => api.addProject(row.folder));
}

function copyResume(id: string): void {
  const row = byId.value.get(id);
  if (row) void api.copyText(resumeCommand(row));
}

const menu = useNativeMenu();

/** Right-click on a session: the native menu with what the row can do. */
function rowMenu(event: MouseEvent, id: string): void {
  const row = byId.value.get(id);
  if (!row) return;
  void menu.popup(event, [
    { text: t(bringKeys(row).label), action: () => bringUp(id) },
    { text: t("sessions.menu.markSeen", { project: row.project }), enabled: row.phase === "waiting", action: () => markSeen(id) },
    { text: t("sessions.copyResume"), action: () => copyResume(id) },
    "separator",
    row.path ? { text: t("sessions.openProject"), action: () => openProject(id) } : { text: t("window.addProject"), action: () => addProject(id) },
  ]);
}

/* ---------- text ---------- */

const subtitle = computed(() => t(view.value && !view.value.hook ? "sessions.subtitleLogs" : "sessions.subtitle"));

const noneText = computed(() => {
  if (q.value) return t("sessions.none.search");
  switch (filter.value) {
    case "waiting":
      return t("sessions.none.waiting");
    case "working":
      return t("sessions.none.working");
    case "ended":
      return t("sessions.none.ended");
    default:
      return t("sessions.none.listed");
  }
});

/** "Listed projects only" hides the sessions outside them: how many. */
const hiddenText = computed(() => {
  const count = listedOnly.value ? all.value.filter((row) => row.path === null).length : 0;
  return count ? t("sessions.hiddenCount", { count }) : "";
});

/* ---------- life ---------- */

// The core pushes every change (`sessions`) once asked, while the window is on screen; on
// opening, a fresh look (back on screen, the window asks: WindowApp).
onMounted(() => void loadSessions());
</script>

<template>
  <div class="sessions">
    <main class="column">
      <!-- Its heading and free space drag the window; the search box stays clickable. -->
      <header class="bar" :data-tauri-drag-region="dragRegion">
        <div class="heading">
          <span class="title">{{ t("sessions.title") }}</span>
          <span class="subtitle">{{ subtitle }}</span>
        </div>
        <label class="sr-only" for="session-search">{{ t("sessions.search") }}</label>
        <input id="session-search" v-model="query" type="search" :placeholder="t('sessions.search')" @keydown.down.prevent="focusSelected" />
      </header>

      <!-- The list's filters, level with the details' header line (56 + 44 = 100px). -->
      <div class="filters">
        <div role="tablist" class="tabs" :aria-label="t('sessions.filters')">
          <button
            v-for="(f, i) in FILTERS"
            :key="f.id"
            type="button"
            role="tab"
            :aria-selected="filter === f.id"
            :tabindex="filter === f.id ? 0 : -1"
            :class="{ on: filter === f.id }"
            :title="t(f.label)"
            @click="filter = f.id"
            @keydown="onTabKey($event, i)"
          >
            <span v-if="f.id !== 'all'" :class="['tab-mark', f.id]" aria-hidden="true"></span>
            <span :class="['tab-label', { markable: f.id !== 'all' }]">{{ t(f.label) }}</span>
            <span v-if="view" :class="['tab-count', { hot: f.id === 'waiting' && counts.waiting > 0 }]">{{ counts[f.id] }}</span>
          </button>
        </div>
        <label class="check" :title="t('sessions.listedOnlyTitle')">
          <input v-model="listedOnly" type="checkbox" role="switch" />
          <span class="check-text">{{ t("sessions.listedOnly") }}</span>
        </label>
      </div>

      <!-- Only the list scrolls, never the window. -->
      <div class="scroll">
        <div class="content">
          <SessionNotice v-if="view && !view.hook" :tag="t('sessions.noHook.tag')" :title="t('sessions.noHook.title')">
            {{ t("sessions.noHook.body") }}
            <template #action>
              <button type="button" class="control" @click="emit('openSettings')">{{ t("settings.addHook") }}</button>
            </template>
          </SessionNotice>

          <div v-if="!view" class="loading" role="status">
            <Spinner />
            <span>{{ t("sessions.loading") }}</span>
          </div>

          <SessionNotice v-else-if="!all.length" :tag="t('sessions.empty.tag')" :title="t('sessions.empty.title')">
            {{ t("sessions.empty.body") }}
            <template v-if="claudeProject" #action>
              <button type="button" class="control" :title="t('sessions.empty.openClaudeTitle', { project: claudeProject })" @click="emit('openClaude')">
                {{ t("sessions.empty.openClaude") }}
              </button>
            </template>
          </SessionNotice>

          <template v-else>
            <SessionList
              v-if="shown.length"
              :layout="shown"
              :rows="byId"
              :selected="selectedId"
              :now="now"
              :label="t(current.label)"
              @select="selectedId = $event"
              @bring-up="bringUp"
              @mark-seen="markSeen"
              @open-project="openProject"
              @copy="copyResume"
              @menu="rowMenu"
              @hold="hold"
            />
            <p v-else class="none">{{ noneText }}</p>
          </template>

          <p v-if="hiddenText" class="hidden-note">{{ hiddenText }}</p>
        </div>
      </div>
    </main>

    <SessionDetail
      v-if="selected"
      :row="selected"
      :now="now"
      :others="others"
      :error="addError?.id === selected.id ? addError.text : ''"
      @mark-seen="markSeen(selected.id)"
    />
    <!-- Nothing to show: the panel stays blank but keeps its place, so the layout never jumps. -->
    <section v-else class="detail-empty" :aria-label="t('sessions.details')"></section>
  </div>
</template>

<style scoped>
.sessions {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  overflow: clip;
}

.column {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

/* As tall as the projects toolbar, so switching never moves the title; no line under it: the
   filters below belong to the same head. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 20px;
}

.heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.subtitle {
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

input[type="search"] {
  width: 220px;
  min-width: 110px;
  flex-shrink: 1;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

input[type="search"]::placeholder {
  color: var(--text-faint);
}

/* With the bar above, as tall as the details' head (100px), so their lines meet. */
.filters {
  height: 44px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  /* Room below, so the tabs sit closer to the title than to the list. */
  padding: 0 20px 8px;
  border-bottom: 1px solid #1e2127;
  container-type: inline-size;
}

.tabs {
  display: flex;
  min-width: 0;
  gap: 4px;
}

/* In a narrow list a tab's name gives way; its count never does. */
.tabs button {
  height: 28px;
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 12px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  font-size: 13px;
  color: var(--text-subtle);
}

.tabs button:hover:not(.on) {
  background: #1b1e24;
  color: var(--text-muted);
}

.tabs button.on {
  background: #23272e;
  color: var(--text-strong);
}

.tab-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* In a narrow list the tabs but "All" show the rows' marks in place of their names (still their
   tooltips, and read out): a filled dot waits, a ring works, a grey ring ended. */
.tab-mark {
  display: none;
  width: 8px;
  height: 8px;
  flex-shrink: 0;
  box-sizing: border-box;
  border-radius: 50%;
}

.tab-mark.waiting {
  background: var(--claude);
}

.tab-mark.working {
  border: 1.5px solid var(--claude);
}

.tab-mark.ended {
  width: 7px;
  height: 7px;
  border: 1.5px solid var(--idle-ring);
}

@container (max-width: 420px) {
  .tab-mark {
    display: inline-block;
  }

  .tab-label.markable {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

.tab-count {
  flex-shrink: 0;
  min-width: 20px;
  height: 18px;
  box-sizing: border-box;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 9px;
  background: #1c1f25;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  color: var(--text-faint);
}

.tabs button.on .tab-count {
  background: #2c3039;
  color: var(--text-muted);
}

.tab-count.hot,
.tabs button.on .tab-count.hot {
  background: #3a2718;
  color: var(--claude-text);
  font-weight: 600;
}

.check {
  flex-shrink: 0;
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

/* A small switch, like the app's other on/off chips, not a form checkbox. */
.check input {
  appearance: none;
  -webkit-appearance: none;
  position: relative;
  flex-shrink: 0;
  width: 26px;
  height: 14px;
  margin: 0;
  border-radius: 7px;
  background: #3a3f48;
  transition: background 0.15s;
}

.check input::before {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--bg-app);
  transition: transform 0.15s;
}

.check input:checked {
  background: var(--text-muted);
}

.check input:checked::before {
  transform: translateX(12px);
}

/* A narrow list keeps the box; its name stays as its tooltip and for screen readers. */
@container (max-width: 520px) {
  .check-text {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}

.scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
}

.content {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px;
}

.control {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover {
  background: #2c3039;
}

.loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 48px 0;
  color: var(--text-muted);
}

.none {
  margin: 0;
  padding: 40px 12px;
  text-align: center;
  color: var(--text-muted);
}

.hidden-note {
  margin: 0;
  padding: 6px 12px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.detail-empty {
  width: 380px;
  flex-shrink: 0;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
}
</style>
