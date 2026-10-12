<script setup lang="ts">
import { computed, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";
import { byNewest, changedShort, recent, useSwitch } from "../../lib/setup";
import { api, now, snapshot } from "../../lib/store";
import type { Candidate, Discovery } from "../../lib/types";

/** The first step: the projects folder looked through, its finds ticked and added to the list.
 * `added`: the ticked ones are listed now. */
const emit = defineEmits<{ added: [] }>();

/** What the core found; null until it answers, and while another folder is looked through. */
const discovery = ref<Discovery | null>(null);
const scanning = ref(false);
const scanError = ref("");
/** The paths ticked. */
const ticked = ref(new Set<string>());
/** The folder the ticks belong to: another one starts from its own (`undefined`: none yet). */
let tickedFor: string | null | undefined;
/** Scans asked for so far: an answer to an older one is dropped. */
let asked = 0;

const listedPaths = computed(() => new Set((snapshot.value?.projects ?? []).map((project) => project.path)));

/** Already in Pitwall: as the folder was read, or added since (from the sidebar, a dropped folder). */
function isListed(candidate: Candidate): boolean {
  return candidate.listed || listedPaths.value.has(candidate.path);
}

/** A folder read for the first time comes with what was worked on lately ticked; read again
 * (after an add), it keeps the ticks it had. */
function take(found: Discovery): void {
  const fresh = found.dir !== tickedFor;
  const open = found.candidates.filter((candidate) => !isListed(candidate));
  ticked.value = new Set(open.filter((candidate) => (fresh ? recent(candidate, now.value) : ticked.value.has(candidate.path))).map((candidate) => candidate.path));
  tickedFor = found.dir;
  discovery.value = found;
}

async function scan(): Promise<void> {
  const mine = ++asked;
  scanning.value = true;
  scanError.value = "";
  try {
    const found = await api.discoverProjects();
    if (mine !== asked) return;
    take(found);
  } catch (reason) {
    if (mine !== asked) return;
    scanError.value = String(reason);
  } finally {
    if (mine === asked) scanning.value = false;
  }
}

// The folder dialog answers through the snapshot: another folder is looked through from scratch.
watch(
  () => snapshot.value?.settings.projectsDir,
  () => {
    discovery.value = null;
    void scan();
  },
  { immediate: true },
);

/** The folder as far as it is known: what was looked through, else what Settings holds. */
const dir = computed(() => discovery.value?.dir ?? snapshot.value?.settings.projectsDir ?? null);

const state = computed<"loading" | "failed" | "none" | "empty" | "list" | null>(() => {
  if (scanError.value) return "failed";
  if (!discovery.value) return scanning.value ? "loading" : null;
  if (discovery.value.dir === null) return "none";
  return discovery.value.candidates.length ? "list" : "empty";
});

/** `62 folders · 23 have a dev script`; the second part only when there is one. */
const counts = computed(() => {
  const found = discovery.value;
  if (!found || found.dir === null) return "";
  const parts = [t("setup.projects.folders", { count: found.folders })];
  if (found.candidates.length) parts.push(t("setup.projects.withScript", { count: found.candidates.length }));
  return parts.join(" · ");
});

/** The finds, newest first. `stale`: the first one left unticked for its age, which says why. */
const rows = computed(() => {
  let said = false;
  return [...(discovery.value?.candidates ?? [])].sort(byNewest).map((candidate) => {
    const listed = isListed(candidate);
    const old = !listed && candidate.changedAt !== null && !recent(candidate, now.value);
    const stale = old && !said;
    if (stale) said = true;
    return { candidate, listed, stale, on: listed || ticked.value.has(candidate.path), changed: changedShort(candidate.changedAt, now.value) };
  });
});

/** The ticked ones that aren't listed yet: what the button adds. */
const selected = computed(() => rows.value.filter((row) => !row.listed && row.on).map((row) => row.candidate.path));

/** Every shape that button's label takes as boxes are ticked, from none to all of them (a zero
 * stands for any digit: they are one width there), so the button is as wide as the widest. */
const addWidths = computed(() => {
  const open = (discovery.value?.candidates ?? []).filter((candidate) => !isListed(candidate)).length;
  return [...new Set(Array.from({ length: open + 1 }, (_, count) => t("setup.projects.add", { count }).replace(/\d/g, "0")))];
});

function tick(path: string, on: boolean): void {
  const next = new Set(ticked.value);
  if (on) next.add(path);
  else next.delete(path);
  ticked.value = next;
}

/** ↑/↓ move along the boxes; Space ticks, as on any checkbox. */
function onListKey(event: KeyboardEvent): void {
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  const boxes = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLInputElement>("input:not(:disabled)")];
  const at = boxes.indexOf(event.target as HTMLInputElement);
  if (at < 0) return;
  event.preventDefault();
  boxes[Math.min(boxes.length - 1, Math.max(0, at + (event.key === "ArrowDown" ? 1 : -1)))]?.focus();
}

/** The folder dialog is the core's; the choice comes back as `settings.projectsDir`. */
function pick(): void {
  void api.pickProjectsDir().catch(() => undefined);
}

const adding = ref(false);
const addError = ref("");

async function add(): Promise<void> {
  const paths = selected.value;
  if (!paths.length || adding.value) return;
  adding.value = true;
  addError.value = "";
  try {
    await api.addProjects(paths);
  } catch (reason) {
    addError.value = String(reason);
    adding.value = false;
    return;
  }
  // They are listed now: the folder is read again, so their rows say so.
  await scan();
  adding.value = false;
  emit("added");
}

const { on: suggestOn, error: suggestError, set: setSuggest } = useSwitch("suggestProjects");
</script>

<template>
  <section class="pane">
    <div class="head">
      <span class="head-text">
        <span class="title">{{ t("settings.projectsFolder") }}</span>
        <span v-if="dir" class="sub where">
          <span class="place">
            <Icon name="folder" :size="13" />
            <span class="path" :title="dir">{{ dir }}</span>
          </span>
          <span v-if="counts" class="counts">{{ counts }}</span>
        </span>
        <span v-else class="sub">
          <span class="sub-text">{{ t("settings.notSet") }}</span>
        </span>
      </span>
      <button type="button" :class="['control', { primary: !dir }]" @click="pick">
        <span class="label">
          <span>{{ t(dir ? "setup.projects.change" : "setup.projects.choose") }}</span>
          <span class="width" aria-hidden="true">{{ t(dir ? "setup.projects.choose" : "setup.projects.change") }}</span>
        </span>
      </button>
    </div>

    <div v-if="state === 'loading'" class="block">
      <p class="waiting" role="status"><Spinner :size="12" /> {{ t("setup.projects.scanning") }}</p>
    </div>

    <template v-else-if="state === 'failed'">
      <div class="block">
        <p class="text">{{ t("setup.projects.failed") }}</p>
        <p class="error">{{ scanError }}</p>
      </div>
      <div class="foot">
        <button type="button" class="control" :disabled="scanning" :aria-busy="scanning" @click="scan">
          <span :class="{ hidden: scanning }">{{ t("setup.retry") }}</span>
          <Spinner v-if="scanning" class="over" :size="12" />
        </button>
      </div>
    </template>

    <div v-else-if="state === 'none'" class="block">
      <p class="text">{{ t("setup.projects.intro") }}</p>
    </div>

    <div v-else-if="state === 'empty'" class="block">
      <p class="text">{{ t("setup.projects.empty") }}</p>
      <p class="note">{{ t("setup.projects.intro") }}</p>
    </div>

    <!-- A row is one click target: the label around its box. -->
    <div v-else-if="state === 'list'" class="list" role="group" :aria-label="t('setup.projects.found')">
      <div class="cols" aria-hidden="true">
        <span></span>
        <span>{{ t("window.col.project") }}</span>
        <span>{{ t("setup.projects.kind") }}</span>
        <span class="right">{{ t("setup.projects.changed") }}</span>
      </div>
      <div class="rows" @keydown="onListKey">
        <label v-for="row in rows" :key="row.candidate.path" :class="['row', { off: !row.on, listed: row.listed }]">
          <input type="checkbox" :checked="row.on" :disabled="row.listed || adding" @change="tick(row.candidate.path, ($event.target as HTMLInputElement).checked)" />
          <span class="name">
            <span class="name-text">{{ row.candidate.name }}</span>
            <span v-if="row.listed" class="tag">{{ t("setup.projects.listed") }}</span>
            <span v-else-if="row.stale" class="tag">{{ t("setup.projects.stale") }}</span>
          </span>
          <span class="kind">{{ row.candidate.kind }}</span>
          <span class="changed">{{ row.changed }}</span>
        </label>
      </div>
    </div>

    <div v-if="state === 'list' || state === 'empty'" class="foot">
      <label class="opt foot-text">
        <input type="checkbox" role="switch" :checked="suggestOn" @change="setSuggest(($event.target as HTMLInputElement).checked)" />
        <span class="opt-text">
          <span>{{ t("setup.projects.suggest") }}</span>
        </span>
      </label>
      <button v-if="state === 'list'" type="button" class="control primary" :disabled="!selected.length || adding" :aria-busy="adding" @click="add">
        <span :class="['label', 'counted', { hidden: adding }]">
          <span>{{ t("setup.projects.add", { count: selected.length }) }}</span>
          <span v-for="text in addWidths" :key="text" class="width" aria-hidden="true">{{ text }}</span>
        </span>
        <Spinner v-if="adding" class="over" :size="12" />
      </button>
    </div>
    <p v-if="addError || suggestError" class="error" role="alert">{{ addError || suggestError }}</p>
  </section>
</template>

<style scoped src="./setup.css"></style>

<style scoped>
/* A label and the labels it could be share one cell: the button is as wide as the widest
   (components/PeriodPicker), whatever it says. */
.label {
  display: inline-grid;
  justify-items: center;
}

.label > span {
  grid-area: 1 / 1;
}

.label .width {
  visibility: hidden;
}

/* A count in it: every digit as wide as another. */
.counted {
  font-variant-numeric: tabular-nums;
}

/* The folder and what it holds: a long path gives way, and in a narrow window the counts drop
   under it. The path alone is in the mono face, as paths are. */
.where {
  flex-wrap: wrap;
  gap: 2px 12px;
}

.place {
  min-width: 0;
  max-width: 100%;
  display: flex;
  align-items: center;
  gap: 6px;
  font-family: var(--font-mono);
}

.place svg {
  flex-shrink: 0;
}

.path,
.counts {
  min-width: 0;
  max-width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.waiting {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-muted);
}

/* The finds between two hairlines; only its rows scroll. */
.list {
  flex: 0 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 6px 0;
  border-top: 1px solid var(--line);
  border-bottom: 1px solid var(--line);
}

.cols,
.row {
  display: grid;
  grid-template-columns: 16px minmax(0, 1fr) 130px 108px;
  gap: 12px;
  align-items: center;
  padding: 0 10px;
}

/* Its right end keeps the room the rows' scrollbar takes. */
.cols {
  flex-shrink: 0;
  height: 28px;
  padding-right: 20px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
}

:lang(zh) .cols,
:lang(ja) .cols {
  letter-spacing: 0;
}

.cols span {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.right {
  text-align: right;
}

/* Always a scrollbar's room (it draws nothing while all rows fit), so the columns stand under
   their heads either way. */
.rows {
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: scroll;
}

.row {
  height: 36px;
  border-radius: 8px;
}

.row:hover:not(.listed) {
  background: var(--bg-row-hover);
}

/* The keyboard's row: the ring round the whole row, inside it so the list's edge never cuts it. */
.row:has(input:focus-visible) {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.row input {
  width: 16px;
  height: 16px;
  margin: 0;
  accent-color: var(--run);
  outline: none;
}

/* The name and a quiet word after it. One line high and wrapping: a word that doesn't fit whole
   beside the name drops to a second line, out of sight, rather than show as a stump. */
.name {
  min-width: 0;
  height: 20px;
  display: flex;
  flex-wrap: wrap;
  column-gap: 8px;
  overflow: hidden;
  line-height: 20px;
}

.name-text {
  min-width: 0;
  max-width: 100%;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row.off .name-text {
  color: var(--text-nav);
}

.row.listed .name-text {
  color: var(--text-muted);
}

.tag {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.kind {
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.changed {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  text-align: right;
  white-space: nowrap;
}

/* A narrow window: the kind's column gives room to the names. */
@container setup-step (max-width: 560px) {
  .cols,
  .row {
    grid-template-columns: 16px minmax(0, 1fr) 104px 108px;
  }
}
</style>
