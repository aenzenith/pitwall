<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { clock, duration } from "../../lib/day";
import { useBackdropClose } from "../../lib/dialog";
import { openUnfocused, useReturnFocus } from "../../lib/dialogFocus";
import { shellRequest } from "../../lib/flag";
import { language, t } from "../../lib/i18n";
import { cardRequest } from "../../lib/panel";
import { dayMonth } from "../../lib/period";
import { rowKeys } from "../../lib/rows";
import { api, now, snapshot, today } from "../../lib/store";
import type { Flag, FlagScheduled } from "../../lib/types";

/**
 * The chequered flag's dialog: what is still open, a part each with its count, only the parts
 * that have something. What it lists is the core's answer (`flag_state`), asked for again whenever
 * the state or the board changes, so a commit made meanwhile drops its row. `quit`: a quit waits
 * on it, and its foot answers that quit; `day`: asked for from the Today page, it only closes.
 *
 * A row's one thing to do is its button's; a press anywhere on the row does it too, but for "Stop
 * all". Where that takes the user into Pitwall's own window (a terminal tab, the board, a session
 * that runs in one of Pitwall's terminals) the dialog closes; an editor or another app coming up
 * leaves it open.
 */
const props = defineProps<{ mode: "day" | "quit"; first: Flag }>();
const emit = defineEmits<{ close: [] }>();

/** A row that stays waits at least this long under its spinner, so a quick answer doesn't flicker. */
const MIN_SPIN_MS = 400;
/** The state settles for this long before the core is asked again. */
const SETTLE_MS = 120;
/** A stop that took hold nowhere gives its button back after this long; one that shows as
 * stopping, once the core's own limit for it has passed. */
const STOP_IDLE_MS = 3_000;
const STOP_LIMIT_MS = 35_000;

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const cancelButton = ref<HTMLButtonElement | null>(null);
const closeButton = ref<HTMLButtonElement | null>(null);

const flag = ref<Flag>(props.first);

/* ---------- following the state ---------- */

let asked = 0;
let settle: number | undefined;

async function reload(): Promise<void> {
  const ask = ++asked;
  try {
    const answer = await api.flagState();
    if (ask === asked) flag.value = answer.open;
  } catch {
    // Kept as it was; the next change asks again.
  }
}

function soon(): void {
  window.clearTimeout(settle);
  settle = window.setTimeout(() => void reload(), SETTLE_MS);
}

// Git, Claude, servers and commands all show in the snapshot; the board has its own event.
watch(snapshot, soon);

/* ---------- the parts ---------- */

type Row =
  | { key: string; part: "commit" | "push"; path: string; name: string; branch: string; count: string; label: string }
  | { key: string; part: "claude"; path: string; name: string; session: string; status: string; tool: { before: string; name: string; after: string } | null; label: string }
  | { key: string; part: "review" | "scheduled"; path: string; name: string; count: string; label: string }
  | { key: string; part: "running"; name: string; sub: string };

type Part = { id: Row["part"]; label: string; count: string; rows: Row[] };

/** Stands in for a tool's name while its sentence is put together (no name ever holds it). */
const TOOL = "\uE000";
/** What is left in sight of a tool's name at the least, in characters (the style's `6ch`): a name
 * no longer than that is never cut, and stays in its sentence. */
const TOOL_LEAST = 6;

/** How long, at least "< 1 min". */
function lasting(since: number): string {
  return duration(Math.max(now.value - since, 1));
}

const parts = computed<Part[]>(() => {
  const open = flag.value;
  const list: Part[] = [];

  if (open.commit.length) {
    list.push({
      id: "commit",
      label: t("flag.commit"),
      count: t("flag.projects", { count: open.commit.length }),
      rows: open.commit.map((row) => ({ key: `commit\u001f${row.path}`, part: "commit", path: row.path, name: row.name, branch: row.branch, count: t("flag.changes", { count: row.count }), label: t("flag.openEditor", { name: row.name }) })),
    });
  }
  if (open.push.length) {
    list.push({
      id: "push",
      label: t("flag.push"),
      count: t("flag.projects", { count: open.push.length }),
      rows: open.push.map((row) => ({ key: `push\u001f${row.path}`, part: "push", path: row.path, name: row.name, branch: row.branch, count: t("flag.commits", { count: row.count }), label: t("flag.openTerminal", { name: row.name }) })),
    });
  }
  if (open.claude.length) {
    list.push({
      id: "claude",
      label: t("flag.claude"),
      count: t("flag.sessions", { count: open.claude.length }),
      rows: open.claude.map((row) => {
        const wait = lasting(row.since);
        const status =
          row.kind === "permission"
            ? row.tool
              ? t("sessions.status.permissionTool", { tool: row.tool, duration: wait })
              : t("flag.permission", { duration: wait })
            : t(row.kind === "asking" ? "flag.asking" : "flag.finished", { duration: wait });
        // A permission's tool apart from the words around it: a long name gives way, they stay.
        const [before, after] = row.kind === "permission" && row.tool && row.tool.length > TOOL_LEAST ? t("sessions.status.permissionTool", { tool: TOOL, duration: wait }).split(TOOL) : [];
        const tool = row.tool && after !== undefined ? { before, name: row.tool, after } : null;
        return { key: `claude\u001f${row.session}`, part: "claude", path: row.path, name: row.name, session: row.session, status, tool, label: t("flag.bringUp", { name: row.name }) };
      }),
    });
  }
  if (open.review.length) {
    list.push({
      id: "review",
      label: t("flag.review"),
      count: t("flag.cards", { count: open.review.reduce((sum, row) => sum + row.cards, 0) }),
      rows: open.review.map((row) => ({ key: `review\u001f${row.path}`, part: "review", path: row.path, name: row.name, count: t("flag.cards", { count: row.cards }), label: t("flag.openBoard", { name: row.name }) })),
    });
  }
  // Cards told to wait for their moment: the core gives them only while Pitwall runs. How many
  // each project has, and the earliest of their moments as a time; none while one of them waits
  // its turn in the refill's line, which has no moment.
  if (open.scheduled.length) {
    const count = (row: FlagScheduled): string => (row.at === null ? t("flag.cards", { count: row.cards }) : `${t("flag.cards", { count: row.cards })} · ${clock(row.at)}`);
    list.push({
      id: "scheduled",
      label: t("flag.scheduled"),
      count: t("flag.cards", { count: open.scheduled.reduce((sum, row) => sum + row.cards, 0) }),
      rows: open.scheduled.map((row) => ({ key: `scheduled\u001f${row.path}`, part: "scheduled", path: row.path, name: row.name, count: count(row), label: t("flag.openBoard", { name: row.name }) })),
    });
  }

  const { servers, commands } = open.running;
  if (servers.length || commands.length) {
    const counts = [servers.length ? t("flag.servers", { count: servers.length }) : "", commands.length ? t("flag.commands", { count: commands.length }) : ""];
    // The first project by name, how many more run, and the commands by theirs.
    const sub = [servers.length > 1 ? t("flag.more", { count: servers.length - 1 }) : "", ...new Set(commands.map((command) => command.name))];
    list.push({
      id: "running",
      label: t("flag.running"),
      count: counts.filter(Boolean).join(" · "),
      rows: [{ key: "running", part: "running", name: servers[0]?.name ?? commands[0]?.project ?? "", sub: sub.filter(Boolean).join(" · ") }],
    });
  }

  return list;
});

const rows = computed(() => parts.value.flatMap((part) => part.rows));

/** Wide once it has had rows, and from then on: what was open going while it is up (a commit
 * made, everything stopped) leaves the dialog the width it came up with. */
const wide = ref(parts.value.length > 0);
watch(parts, (list) => {
  if (list.length) wide.value = true;
});

/** `Tue 13 Oct · 19:42`: the day it is and the time, as the clock has them. */
const when = computed(() => {
  const weekday = new Intl.DateTimeFormat(language.value, { weekday: "short" }).format(now.value);
  return `${weekday} ${dayMonth(today.value)} · ${clock(now.value)}`;
});

/* ---------- the rows' keyboard ---------- */

/** The row that is the list's Tab stop: the first, until the arrows or a press pick another. */
const picked = ref<string | null>(null);
const current = computed(() => (rows.value.some((row) => row.key === picked.value) ? picked.value : (rows.value[0]?.key ?? null)));

/** A row that goes while the keyboard is on it (its commit made, its session brought up) hands
 * the keyboard to the list's Tab stop, or to the dialog once nothing is left: never to the page
 * behind. */
const list = ref<HTMLElement | null>(null);
let inList = false;
watch(
  rows,
  () => {
    inList = !!list.value?.contains(document.activeElement);
  },
  { flush: "pre" },
);
watch(
  rows,
  () => {
    if (!inList || list.value?.contains(document.activeElement)) return;
    (list.value?.querySelector<HTMLElement>('[data-path][tabindex="0"]') ?? dialog.value)?.focus({ preventScroll: true });
  },
  { flush: "post" },
);

function onKey(event: KeyboardEvent): void {
  const act = (key: string): void => {
    const row = rows.value.find((entry) => entry.key === key);
    // "Stop all" is its button's alone, for the keyboard as for the pointer: ←/→ reach it.
    if (row && row.part !== "running") void run(row);
  };
  rowKeys(event, { move: (key) => (picked.value = key), enter: act, space: act });
}

/* ---------- what a row does ---------- */

/** The row whose action is under way. */
const busy = ref<string | null>(null);
/** When "Stop all" was pressed; 0 when it wasn't, or once everything is down. */
const stopAsked = ref(0);
let stopLimit: number | undefined;

/** Runs a row's action under its spinner, for `least` ms at least; false when the core refused. */
async function spun(key: string, work: () => Promise<unknown>, least = 0): Promise<boolean> {
  if (busy.value) return false;
  busy.value = key;
  const started = Date.now();
  let done = true;
  try {
    await work();
  } catch {
    done = false;
  }
  const rest = least - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  busy.value = null;
  return done;
}

async function run(row: Row): Promise<void> {
  if (quitting.value) return;
  switch (row.part) {
    // Another app comes up: the dialog stays.
    case "commit":
      void api.openEditor(row.path).catch(() => undefined);
      return;
    // A new tab of the project's terminals, in this window: the list shows the project, its
    // panel opens the tab (it takes the request), and the dialog gives way at once, so the
    // keyboard can go into the new shell.
    case "push":
      shellRequest.value = { path: row.path, at: Date.now() };
      if (await spun(row.key, () => api.flagTerminal(row.path))) close();
      else shellRequest.value = null;
      return;
    // Where it runs: in one of Pitwall's terminals the window shows it (`reveal-terminal`,
    // which closes the dialog below); in an editor or a terminal app the dialog stays.
    case "claude":
      await spun(row.key, () => api.revealClaude(row.path, row.session), MIN_SPIN_MS);
      return;
    // The Board page, on that project's board (it takes the request as it takes a card's).
    // Asked for once the row's action is under way: a press that isn't taken (another row's
    // spinner turns) or that the core refuses leaves no request behind for the page's next opening.
    case "review":
    case "scheduled": {
      const taken = await spun(row.key, () => {
        cardRequest.value = { id: "", path: row.path };
        return api.flagBoard();
      });
      if (taken) close();
      else if (cardRequest.value?.id === "" && cardRequest.value.path === row.path) cardRequest.value = null;
      return;
    }
    case "running":
      return stopAll();
  }
}

/** Every server and every running command: the button spins until they are really down, and
 * the part goes with them. */
async function stopAll(): Promise<void> {
  if (stopAsked.value || quitting.value) return;
  stopAsked.value = Date.now();
  window.clearTimeout(stopLimit);
  stopLimit = window.setTimeout(() => (stopAsked.value = 0), STOP_LIMIT_MS);
  try {
    await api.flagStopAll();
  } catch {
    stopAsked.value = 0;
  }
}

watch(flag, (open) => {
  if (!stopAsked.value) return;
  const left = open.running.servers.length + open.running.commands.length;
  // Down; or nothing of it is on its way down (a stop that took hold nowhere).
  if (!left || (!open.running.stopping && Date.now() - stopAsked.value > STOP_IDLE_MS)) stopAsked.value = 0;
});

/* ---------- the foot ---------- */

const atQuit = computed(() => snapshot.value?.settings.flag?.atQuit ?? true);

/** The box writes the setting at once, as Settings › General's switch does. */
function setAtQuit(event: Event): void {
  const box = event.target as HTMLInputElement;
  void api.flagSet(box.checked).catch(() => (box.checked = atQuit.value));
}

/** "Quit anyway": the app goes away under the dialog, so only a failure ever comes back. Until
 * then the dialog has had its answer: its rows and buttons stand still, and nothing closes it. */
const quitting = ref(false);

function close(): void {
  if (!quitting.value) dialog.value?.close();
}

/** Esc, by the key and by the dialog's own event: a quit on its way isn't given up. */
function keep(event: Event): void {
  if (quitting.value) event.preventDefault();
}

/** Closed: a held quit is given up. Never from under one that goes on: should the dialog close
 * all the same, it comes back, and nothing is told. */
function closed(): void {
  if (quitting.value) dialog.value?.showModal();
  else emit("close");
}

async function quit(): Promise<void> {
  if (quitting.value) return;
  quitting.value = true;
  try {
    await api.flagQuit();
  } catch {
    quitting.value = false;
  }
}

/* ---------- life ---------- */

/** The window goes somewhere of its own (a terminal, a project, a page): the dialog would cover it. */
const REVEALS = ["reveal-terminal", "reveal-project", "reveal-output", "reveal-page", "reveal-fuel"];
const unlisten: UnlistenFn[] = [];
let gone = false;

function shown(): void {
  if (props.mode === "quit") void api.flagShown().catch(() => undefined);
}

// A quit held while the dialog was up for the day takes it over.
watch(() => props.mode, shown);

/** The core hid the window (its close button, ⌘W): a held quit is given up rather than answered
 * out of sight. By the core's own word, not the page's: a window still on its way up for the
 * dialog isn't visible yet either. */
function onVisibility(visible: boolean): void {
  if (!visible && props.mode === "quit") close();
}

onMounted(async () => {
  // The safe answer is the one Return gives: Cancel on a held quit, Close otherwise.
  openUnfocused(dialog.value, () => cancelButton.value ?? closeButton.value);
  shown();

  const win = getCurrentWindow();
  const stops = await Promise.all([
    win.listen("board", soon),
    win.listen<{ visible: boolean }>("visibility", (event) => onVisibility(event.payload.visible)),
    ...REVEALS.map((name) => listen(name, close)),
  ]);
  if (gone) stops.forEach((stop) => stop());
  else unlisten.push(...stops);
});

onBeforeUnmount(() => {
  gone = true;
  window.clearTimeout(settle);
  window.clearTimeout(stopLimit);
  unlisten.forEach((stop) => stop());
});
</script>

<template>
  <dialog ref="dialog" :class="['dialog', { bare: !wide }]" :aria-label="t('flag.title')" @close="closed" @cancel="keep" @keydown.esc="keep" @pointerdown="backdrop.down" @click="quitting || backdrop.click($event)">
    <div class="head">
      <Icon name="flag" :size="18" class="glyph" />
      <span class="title">{{ t("flag.title") }}</span>
      <span v-if="parts.length" class="when">{{ when }}</span>
    </div>

    <!-- Many rows scroll in here: the dialog never outgrows the window. -->
    <template v-if="parts.length">
      <span id="flag-keys" class="sr-only">{{ t("flag.keys") }}</span>
      <div class="parts" role="grid" :aria-label="t('flag.title')" aria-describedby="flag-keys" :inert="quitting">
        <div ref="list" class="rows" role="rowgroup" @keydown="onKey">
          <template v-for="part in parts" :key="part.id">
            <div class="part-head" role="row">
              <span role="columnheader" class="section-label">{{ part.label }}</span>
              <span role="gridcell" class="part-count">{{ part.count }}</span>
            </div>
            <div
              v-for="row in part.rows"
              :key="row.key"
              role="row"
              :data-path="row.key"
              :tabindex="row.key === current ? 0 : -1"
              :class="['row', row.part]"
              @focus="picked = row.key"
              @click="row.part !== 'running' && run(row)"
            >
              <!-- What the part is about, in one box for every part: the names stand in a line. -->
              <span class="mark" aria-hidden="true">
                <svg v-if="row.part === 'commit'" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <circle cx="7" cy="6" r="2" />
                  <circle cx="7" cy="18" r="2" />
                  <circle cx="17" cy="8" r="2" />
                  <path d="M7 8v8M17 10c0 4.5-10 2-10 6" />
                </svg>
                <svg v-else-if="row.part === 'push'" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M12 19V5M6 11l6-6 6 6" />
                </svg>
                <Icon v-else-if="row.part === 'review'" name="board" :size="13" />
                <Icon v-else-if="row.part === 'scheduled'" name="clock" :size="13" />
                <span v-else :class="['dot', row.part]"></span>
              </span>
              <span class="name" role="gridcell" :title="row.name">{{ row.name }}</span>

              <template v-if="row.part === 'commit' || row.part === 'push'">
                <span class="sub" role="gridcell" :title="row.branch">{{ row.branch }}</span>
                <span class="grow"></span>
                <span class="count" role="gridcell">{{ row.count }}</span>
                <button type="button" class="icon" tabindex="-1" :aria-label="row.label" :title="row.label" :disabled="busy === row.key" :aria-busy="busy === row.key" @click.stop="run(row)">
                  <Spinner v-if="busy === row.key" :size="12" />
                  <Icon v-else :name="row.part === 'commit' ? 'editor' : 'terminal'" :size="13" />
                </button>
              </template>

              <template v-else-if="row.part === 'claude'">
                <!-- All of it is the cell's text and its title; a long tool's name is cut short in sight. -->
                <span :class="['status', { split: row.tool }]" role="gridcell" :title="row.status">
                  <template v-if="row.tool">
                    <span>{{ row.tool.before }}</span>
                    <span class="tool">{{ row.tool.name }}</span>
                    <span>{{ row.tool.after }}</span>
                  </template>
                  <template v-else>{{ row.status }}</template>
                </span>
                <button type="button" class="act" tabindex="-1" :aria-label="row.label" :disabled="busy === row.key" :aria-busy="busy === row.key" @click.stop="run(row)">
                  <!-- The label keeps its room under the spinner: the button never changes width. -->
                  <span :class="{ hidden: busy === row.key }">{{ t("sessions.action.bringUp") }}</span>
                  <Spinner v-if="busy === row.key" class="over" :size="12" />
                </button>
              </template>

              <template v-else-if="row.part === 'review' || row.part === 'scheduled'">
                <span class="grow"></span>
                <span class="count" role="gridcell">{{ row.count }}</span>
                <button type="button" class="icon" tabindex="-1" :aria-label="row.label" :title="row.label" :disabled="busy === row.key" :aria-busy="busy === row.key" @click.stop="run(row)">
                  <Spinner v-if="busy === row.key" :size="12" />
                  <Icon v-else name="chevron" :size="13" />
                </button>
              </template>

              <template v-else-if="row.part === 'running'">
                <span v-if="row.sub" class="sub" role="gridcell" :title="row.sub">{{ row.sub }}</span>
                <span class="grow"></span>
                <button type="button" class="act" tabindex="-1" :disabled="stopAsked > 0" :aria-busy="stopAsked > 0" @click.stop="stopAll">
                  <span :class="{ hidden: stopAsked > 0 }">{{ t("common.stopAll") }}</span>
                  <Spinner v-if="stopAsked > 0" class="over" :size="12" />
                </button>
              </template>
            </div>
          </template>
        </div>
      </div>
    </template>
    <p v-else class="none">{{ t("flag.none") }}</p>

    <div class="foot">
      <template v-if="mode === 'quit'">
        <label class="check">
          <input type="checkbox" :checked="atQuit" :disabled="quitting" @change="setAtQuit" />
          <span>{{ t("flag.atQuit") }}</span>
        </label>
        <span class="grow"></span>
        <button ref="cancelButton" type="button" class="control" :disabled="quitting" @click="dialog?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="control primary" :disabled="quitting" :aria-busy="quitting" @click="quit">
          <span :class="{ hidden: quitting }">{{ t("flag.quitAnyway") }}</span>
          <Spinner v-if="quitting" class="over" :size="12" />
        </button>
      </template>
      <template v-else>
        <span class="grow"></span>
        <button ref="closeButton" type="button" class="control primary" @click="dialog?.close()">{{ t("common.close") }}</button>
      </template>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the command dialog, wider for its rows. As tall as what is open, up to the
   window: past that the parts scroll, never the dialog. */
.dialog {
  width: 620px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
  overflow: hidden;
}

/* Nothing open as it comes up: one line and its button. */
.dialog.bare {
  width: 360px;
}

.dialog[open] {
  display: flex;
  flex-direction: column;
  animation: pop 0.16s ease-out;
}

.head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 18px 20px 14px;
}

.glyph {
  flex-shrink: 0;
  color: var(--text-soft);
}

.title {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 15px;
  font-weight: 600;
}

.when {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.parts {
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.rows {
  display: flex;
  flex-direction: column;
  padding-bottom: 8px;
}

/* A part: a hairline, its name and, across from it, how much of it there is. */
.part-head {
  flex-shrink: 0;
  height: 24px;
  box-sizing: content-box;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 20px 0;
  border-top: 1px solid var(--line);
}

.part-head:not(:first-child) {
  margin-top: 8px;
}

.part-head .section-label {
  flex: 1 1 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.part-count {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.row {
  flex-shrink: 0;
  min-height: 32px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 20px;
}

/* A press anywhere on it does what its button does; "Stop all" is its button's alone. */
.row:not(.running) {
  cursor: pointer;
}

.row:not(.running):hover {
  background: var(--bg-rail-hover);
}

/* Inside the row: the list's edge would cut a ring drawn around it. */
.row:focus-visible {
  outline-offset: -2px;
  border-radius: 6px;
}

.mark {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-subtle);
}

.dot {
  border-radius: 50%;
}

.dot.claude {
  width: 8px;
  height: 8px;
  background: var(--claude);
}

.dot.running {
  width: 9px;
  height: 9px;
  background: var(--run);
}

.name,
.sub {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The name keeps its room; what is said beside it gives way first. */
.name {
  flex: 0 1 auto;
  font-weight: 500;
}

.sub {
  flex: 0 10 auto;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.grow {
  flex: 1 1 0;
  min-width: 0;
}

.count,
.status {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: nowrap;
}

.count {
  color: var(--text-soft);
}

/* What Claude waits with has the room the project's name leaves, and stands at its right end.
   Its least is its words with the tool's name cut short: only then does the name give way. */
.status {
  flex: 1 1 0;
  color: var(--claude-text);
  text-align: right;
}

.status.split {
  display: grid;
  grid-template-columns: max-content minmax(6ch, max-content) max-content;
  justify-content: end;
}

/* The words around the tool's name keep their spaces. */
.status.split span {
  white-space: pre;
}

.status .tool {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row.claude .name {
  min-width: 64px;
}

.icon {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

/* Over the hovered row's own ground: a step further than that one. */
.icon:hover:not(:disabled),
.row:hover .icon:not(:disabled) {
  background: var(--bg-rail-on);
  color: var(--text);
}

.act {
  position: relative;
  flex-shrink: 0;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
}

.act:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.none {
  flex-shrink: 0;
  margin: 0;
  padding: 0 20px 16px;
  color: var(--text-soft);
}

.foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 20px;
  border-top: 1px solid var(--line);
  background: var(--bg-footer);
}

.check {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 9px;
  color: var(--text-soft);
}

.check input {
  width: 15px;
  height: 15px;
  margin: 0;
  flex-shrink: 0;
  accent-color: var(--run);
}

.check span {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.control {
  position: relative;
  flex-shrink: 0;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.control:disabled,
.act:disabled {
  opacity: 0.5;
}

/* A busy button keeps its look: its spinner says it is at work. */
.control[aria-busy="true"],
.act[aria-busy="true"] {
  opacity: 1;
}

.hidden {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .dialog[open] {
    animation: none;
  }
}
</style>
