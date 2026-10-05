<script lang="ts">
import { ref } from "vue";

import type { Filter } from "../lib/sessions";

/** The filter tab and the project picked (null: every project, as the app opens on): kept while
 * the app runs, never stored. */
const filter = ref<Filter>("all");
const scope = ref<string | null>(null);
</script>

<script setup lang="ts">
import { computed, onMounted, watch } from "vue";

import PeriodPicker from "../components/PeriodPicker.vue";
import ProjectPicker from "../components/ProjectPicker.vue";
import Spinner from "../components/Spinner.vue";
import { t, type Key } from "../lib/i18n";
import { useNativeMenu } from "../lib/nativeMenu";
import { dragRegion } from "../lib/platform";
import { messageState, useLastMessage } from "../lib/lastMessage";
import { ALL, dayLong, type Period } from "../lib/period";
import { bringKeys, FILTERS, keepOrder, layout, matches, othersWaiting, resumeCommand, sectionOf, sessionWaits, type Layout } from "../lib/sessions";
import { api, loadSessions, now, sessions, snapshot, today, visible } from "../lib/store";
import { tabKey } from "../lib/tabs";
import type { SessionRow, SessionsView } from "../lib/types";
import SessionDetail from "./sessions/SessionDetail.vue";
import SessionList from "./sessions/SessionList.vue";
import SessionNotice from "./sessions/SessionNotice.vue";

/** Every Claude Code session today, live; or, looking back, those of an earlier day or of every
 * day there is something kept of. `claudeProject`: where "Open a Claude terminal" would open one
 * (the project list's selection), if anywhere. */
defineProps<{ claudeProject: string | null }>();
const emit = defineEmits<{ openProject: [path: string]; openSettings: []; openClaude: [] }>();

/** The least an action's spinner shows, so one the core answers at once still shows. */
const MIN_SPIN_MS = 400;

/* ---------- which days ---------- */

/** The days shown: today (null), an earlier day, or every day. Today again each time the page
 * opens. */
const period = ref<Period>(null);

/** The shown days' sessions as the core last told them (`claude_history`), each an ended row;
 * null until it has. The last answer stays while the next is on its way. */
const history = ref<SessionsView | null>(null);
let asked = 0;

async function loadHistory(): Promise<void> {
  const wanted = period.value;
  if (wanted === null) return;
  const ask = ++asked;
  let found: SessionsView;
  try {
    found = await api.claudeHistory(wanted);
  } catch {
    // A core that can't tell them shows none.
    found = { now: Date.now(), hook: true, sessions: [], unlisted: 0 };
  }
  if (ask === asked) history.value = found;
}

watch(period, () => void loadHistory());
// Past midnight the earlier days are one more.
watch(today, () => void loadHistory());

// Back on screen, a fresh look: a day's sessions grow while Claude Code writes its logs.
watch(visible, (on) => {
  if (on) void loadHistory();
});

/** A session's time in the days before today and its time today, added up. The hours they were made
 * of are left out, as several days don't share a day's axis. */
function summed(before: SessionRow["today"], day: SessionRow["today"]): SessionRow["today"] {
  if (!before && !day) return null;
  return { work: (before?.work ?? 0) + (day?.work ?? 0), wait: (before?.wait ?? 0) + (day?.wait ?? 0), turns: (before?.turns ?? 0) + (day?.turns ?? 0), spans: [] };
}

/**
 * What the page shows. Today: the live sessions. Looking back: the shown days' sessions, each as
 * it stands now when it is still among the live ones (its hours stay the shown day's). With every
 * day's, a session's time is the earlier days' as the core summed them and today's from its live
 * row, which goes on counting; the live ones the core has nothing kept of yet come too. Tokens are
 * today's alone, so they are left out looking back.
 */
const view = computed<SessionsView | null>(() => {
  const live = sessions.value;
  if (period.value === null) return live;
  const past = history.value;
  if (!past) return null;

  const every = period.value === ALL;
  const current = new Map((live?.sessions ?? []).map((row) => [row.id, row]));
  const rows = past.sessions.map((row) => {
    const open = current.get(row.id);
    return open ? { ...open, today: every ? summed(row.today, open.today) : row.today, spend: null } : row;
  });
  if (every) {
    const known = new Set(rows.map((row) => row.id));
    rows.push(...(live?.sessions ?? []).filter((row) => !known.has(row.id)).map((row) => ({ ...row, today: summed(null, row.today), spend: null })));
  }
  return { now: past.now, hook: live?.hook ?? past.hook, sessions: rows, unlisted: rows.filter((row) => row.path === null).length };
});
const all = computed(() => view.value?.sessions ?? []);
const byId = computed(() => new Map(all.value.map((row) => [row.id, row])));

/* ---------- what the list shows ---------- */

const projects = computed(() => snapshot.value?.projects ?? []);

// A project taken off the list leaves every project's sessions.
watch(
  projects,
  (list) => {
    if (snapshot.value && scope.value && !list.some((p) => p.path === scope.value)) scope.value = null;
  },
  { immediate: true },
);

/** What Claude waits on you with in each project (a finished turn not seen yet, a question, a
 * permission prompt): the picker marks those projects with Claude's dot. Picking one marks nothing
 * seen: that stays the row's to do. */
const waits = computed(() => sessionWaits(all.value));

const query = ref("");
const q = computed(() => query.value.trim().toLowerCase());
/** The sessions the project picked and the search leave, the listed projects' alone (a folder
 * outside them has none here); the tabs pick from these. */
const pool = computed(() => all.value.filter((row) => (scope.value ? row.path === scope.value : row.path !== null) && matches(row, q.value)));

const counts = computed(() => {
  const found: Record<Filter, number> = { all: pool.value.length, waiting: 0, working: 0, ended: 0 };
  for (const row of pool.value) {
    const section = sectionOf(row);
    if (section === "waiting" || section === "working" || section === "ended") found[section]++;
  }
  return found;
});

const current = computed(() => FILTERS.find((f) => f.id === filter.value) ?? FILTERS[0]);
/** A tab's name; looking back, the ended ones didn't end today. */
const filterLabel = (f: (typeof FILTERS)[number]): string => t(f.id === "ended" && period.value !== null ? "sessions.filter.endedPast" : f.label);
const fresh = computed(() => layout(pool.value, current.value.sections));

/**
 * While the pointer or the keyboard is on the list, rows keep their places (lib/sessions:
 * keepOrder): a session that changes state stays put until the list is let go. So it is while the
 * keyboard is in the selected session's terminal: answered there, it stays selected even when it
 * leaves the filter.
 */
const holding = ref(false);
const frozen = ref<Layout | null>(null);
const shown = computed<Layout>(() => (holding.value && frozen.value ? keepOrder(frozen.value, fresh.value, new Set(pool.value.map((row) => row.id))) : fresh.value));

const held = { list: false, terminal: false };

function hold(by: keyof typeof held, on: boolean): void {
  held[by] = on;
  holding.value = held.list || held.terminal;
  frozen.value = holding.value ? (frozen.value ?? fresh.value) : null;
}

// A new filter, project or search is a new list: its order is taken as it stands.
watch([filter, q, scope], () => {
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
const others = computed(() => (selected.value ? othersWaiting(selected.value, all.value) : 0));

/** What Claude last said in the selected session: its details show it, beside its terminal when it
 * runs in one of Pitwall's. */
const lastMessage = useLastMessage(
  () => selected.value?.id ?? null,
  () => messageState(selected.value, now.value),
);

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

/** The session on its way up: nothing else is brought up meanwhile. */
const pending = ref<string | null>(null);

/** Where it runs: its tab or terminal comes up; an ended one opens again in the editor. */
async function bringUp(id: string): Promise<void> {
  const row = byId.value.get(id);
  if (!row || pending.value) return;
  pending.value = id;
  const started = Date.now();
  await api.revealClaude(row.path ?? row.folder, row.id).catch(() => undefined);
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  pending.value = null;
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
    { text: t(bringKeys(row).label), action: () => void bringUp(id) },
    { text: t("sessions.menu.markSeen", { project: row.project }), enabled: row.phase === "waiting", action: () => markSeen(id) },
    { text: t("sessions.copyResume"), action: () => copyResume(id) },
    "separator",
    { text: t("sessions.openProject"), action: () => openProject(id) },
  ]);
}

/* ---------- text ---------- */

/** The project picked, while it is listed. */
const project = computed(() => (scope.value ? (projects.value.find((p) => p.path === scope.value) ?? null) : null));

const subtitle = computed(() => {
  const where = project.value?.name ?? t("sessions.everyProject");
  if (period.value !== null) return t("sessions.subtitleOn", { scope: where, when: period.value === ALL ? t("period.all") : dayLong(period.value) });
  return t(view.value && !view.value.hook ? "sessions.subtitleLogs" : "sessions.subtitle", { scope: where });
});

/** No session at all in the days shown. */
const emptyTitle = computed<Key>(() => (period.value === null ? "sessions.empty.title" : period.value === ALL ? "sessions.empty.titleAll" : "sessions.empty.titlePast"));

const noneText = computed(() => {
  if (q.value) return t("sessions.none.search");
  switch (filter.value) {
    case "waiting":
      return t("sessions.none.waiting");
    case "working":
      return t("sessions.none.working");
    case "ended":
      return t(period.value === null ? "sessions.none.ended" : "sessions.none.endedPast");
    default:
      if (project.value) return t(period.value === null ? "sessions.none.project" : "sessions.none.projectPast", { project: project.value.name });
      return t(period.value === null ? "sessions.none.listed" : "sessions.none.listedPast");
  }
});

/** The sessions in folders outside the listed projects are left out: how many, under every
 * project's (a project picked says whose are shown). */
const hiddenText = computed(() => {
  const count = scope.value ? 0 : all.value.filter((row) => row.path === null).length;
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
      <!-- Its heading and free space drag the window; the days and the search box stay clickable. -->
      <header class="bar" :data-tauri-drag-region="dragRegion">
        <div class="heading">
          <span class="title">{{ t("sessions.title") }}</span>
          <span class="subtitle">{{ subtitle }}</span>
        </div>
        <PeriodPicker v-model="period" />
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
            :title="filterLabel(f)"
            @click="filter = f.id"
            @keydown="onTabKey($event, i)"
          >
            <span v-if="f.id !== 'all'" :class="['tab-mark', f.id]" aria-hidden="true"></span>
            <span :class="['tab-label', { markable: f.id !== 'all' }]">{{ filterLabel(f) }}</span>
            <span v-if="view" :class="['tab-count', { hot: f.id === 'waiting' && counts.waiting > 0 }]">{{ counts[f.id] }}</span>
          </button>
        </div>
        <ProjectPicker class="picker" :projects="projects" :scope="scope" :waits="waits" label="sessions.pickerLabel" @pick="scope = $event" />
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

          <SessionNotice v-else-if="!all.length" :tag="t('sessions.empty.tag')" :title="t(emptyTitle)">
            {{ t(period === null ? "sessions.empty.body" : "sessions.empty.bodyPast") }}
            <template v-if="claudeProject && period === null" #action>
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
              :label="filterLabel(current)"
              :plain="period !== null"
              @select="selectedId = $event"
              @bring-up="bringUp"
              @mark-seen="markSeen"
              @open-project="openProject"
              @copy="copyResume"
              @menu="rowMenu"
              @hold="hold('list', $event)"
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
      :last-message="lastMessage"
      :period="period"
      @mark-seen="markSeen(selected.id)"
      @hold="hold('terminal', $event)"
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
  container-type: inline-size;
}

/* Without room for the days' tabs beside the search, one button holds them (PeriodPicker). */
@container (max-width: 760px) {
  .bar :deep(.period-full) {
    display: none;
  }

  .bar :deep(.period-compact) {
    display: inline-flex;
  }
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

/* In a list without room for them beside the project picker, the tabs but "All" show the rows'
   marks in place of their names (still their tooltips, and read out): a filled dot waits, a ring
   works, a grey ring ended. */
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

@container (max-width: 500px) {
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

/* The project picker, at the row's end: a long project name gives way before a tab's does. */
.filters > .picker {
  margin-left: auto;
  flex-shrink: 1000;
}

/* The window at its narrowest: the picker is its icon alone (its name stays as its tooltip, and
   read out), so the tabs keep their room. */
@container (max-width: 350px) {
  .filters > .picker {
    min-width: 0;
    flex-shrink: 0;
  }

  .filters :deep(.ppick-text),
  .filters :deep(.ppick-more) {
    display: none;
  }

  .filters :deep(.ppick-every) {
    display: block;
  }

  .filters :deep(.ppick-button) {
    padding: 0 8px;
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
