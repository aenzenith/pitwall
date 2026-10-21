<script setup lang="ts">
// The mini tower's page (the `tower` window): a small panel over every other window, for a
// glance at which projects run, where Claude works and where it waits on you. A row per project
// (`list`) or one row of three-letter codes (`strip`), as `settings.tower.form` says.
//
// It draws from the snapshot every window gets, and from `fuel` for the session's share. While
// the tower is off its window is hidden and this page draws nothing. The window is as large as
// the panel: each time the panel's size changes the core is told (`tower_fit`), which sizes the
// window, keeps it on a screen and brings it up.
//
// The window never has the keyboard, and WebKit lights nothing under the pointer in such a
// window: no `:hover`, no pointer events until a button goes down. So what is pointed at is kept
// here (`pointed`), fed by the core on macOS (`tower-pointer`, src-tauri/src/tower.rs) and by the
// page's own pointer events elsewhere, and the rows are lit by class.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, reactive, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Spinner from "../components/Spinner.vue";
import StatusIcon from "../components/StatusIcon.vue";
import { isStale, left, LOW_LEFT, percentText, sessionWindow } from "../lib/fuel";
import { language, t } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import { openFromOutside } from "../lib/open";
import { api, connectFuel, fuel, now, snapshot } from "../lib/store";
import { towerRows, towerSession, towerSettings, towerState } from "../lib/tower";
import { codes } from "../lib/track";
import type { Project } from "../lib/types";

const settings = computed(() => towerSettings(snapshot.value?.settings.tower));
const on = computed(() => settings.value.on);
const strip = computed(() => settings.value.form === "strip");

const projects = computed(() => snapshot.value?.projects ?? []);
/** The projects left out are shown after all, until the tower is hidden again: in the list, whose
 * foot row folds them away again. The strip has no such row and keeps to what Settings says. */
const unfolded = ref(false);
const rows = computed(() => towerRows(projects.value, settings.value.hideStopped));
const listed = computed(() => (unfolded.value && !strip.value ? [...rows.value.shown, ...rows.value.idle] : rows.value.shown));
/** Each project's three letters, the Track page's: told from every project there is, so a code
 * stays the same whichever are shown. */
const letters = computed(() => codes(projects.value));

/** The counts as the menu bar writes them: the servers that run, the Claude sessions at work. */
const running = computed(() => snapshot.value?.running ?? 0);
const working = computed(() => projects.value.reduce((sum, project) => sum + project.claudeSessions.filter((session) => session.phase === "working").length, 0));

/** The session's fuel left, when there is a reading of it. */
const share = computed(() => {
  const session = sessionWindow(fuel.value, now.value);
  if (!session || !fuel.value) return null;
  const rest = left(session.used);
  return { text: percentText(rest, language.value), low: rest <= LOW_LEFT, stale: isStale(fuel.value.limits) };
});

/* ---------- what is pointed at ---------- */

/** The row (a project's path) and the button (`data-act`) under the pointer. */
const pointed = reactive<{ path: string | null; act: string | null }>({ path: null, act: null });
/** Where the pointer was last told to be: looked up again once the page has drawn, as a row
 * that lights brings its buttons under a pointer that hasn't moved. */
let at: { x: number; y: number } | null = null;

function look(): void {
  const under = at ? document.elementFromPoint(at.x, at.y) : null;
  pointed.path = under?.closest<HTMLElement>("[data-path]")?.dataset.path ?? null;
  pointed.act = under?.closest<HTMLElement>("[data-act]")?.dataset.act ?? null;
}

function pointAt(x: number, y: number): void {
  at = { x, y };
  look();
}

function pointAway(): void {
  at = null;
  look();
}

const lit = (project: Project): boolean => pointed.path === project.path;
const litAct = (act: string, project?: Project): boolean => pointed.act === act && (!project || lit(project));

/** The row that was lit before the press under way began. */
let litBefore: string | null = null;
/** The row whose buttons the press under way may work: the one that was lit, its buttons in
 * sight, before it began. */
let armed: string | null = null;

/** A press lights the row it lands on, as pointing at it does. */
function press(event: PointerEvent): void {
  litBefore = pointed.path;
  pointAt(event.clientX, event.clientY);
}

/** A press on a row not lit yet brings its buttons under the pointer: they are not what was
 * pressed. Nor in a double click's second press, which comes after the first lit the row. */
function arm(event: MouseEvent): void {
  if (event.detail <= 1) armed = litBefore;
}

/** A row's button works for a press that began with it in sight, and when pressed by name (a
 * screen reader's press comes with no pointer: `detail` 0). */
function may(project: Project, event: MouseEvent): boolean {
  return event.detail === 0 || armed === project.path;
}

/** A double click on a row's buttons is theirs when they were in sight for it; on a row it only
 * just lit it is the row's, and opens the editor. */
function actsDouble(project: Project, event: MouseEvent): void {
  if (armed === project.path) event.stopPropagation();
}

/* ---------- what a row does ---------- */

/** Claude's session comes up where it runs: the one that waits on you, else the one at work. */
function bringUp(project: Project): void {
  const session = towerSession(project);
  void (session ? api.revealClaude(project.path, session) : api.openClaude(project.path));
}

function toggle(project: Project): void {
  void api.act(project.path, project.status === "running" ? "stop" : "start");
}

/** What each listed project says at its right end. */
const states = computed(() => new Map(listed.value.map((project) => [project.path, towerState(project, now.value)])));

/** A project read out: its name, and what the tower says of it. */
function said(project: Project): string {
  return `${project.name}: ${states.value.get(project.path)?.text ?? ""}`;
}

/* ---------- the window's own options ---------- */

const menu = useNativeMenu();

/** The tower's own options, as a menu: under the button that asked, or where the pointer is. */
function options(event: MouseEvent): void {
  const button = (event.currentTarget as HTMLElement | null)?.closest("button")?.getBoundingClientRect();
  const { form, hideStopped } = settings.value;
  const entries: MenuEntry[] = [
    { text: t("tower.form.list"), checked: form === "list", action: () => void api.towerForm("list") },
    { text: t("tower.form.strip"), checked: form === "strip", action: () => void api.towerForm("strip") },
    "separator",
    { text: t("tower.hideStopped"), checked: hideStopped, action: () => void api.towerOption("hideStopped", !hideStopped) },
    "separator",
    { text: t("tower.hide"), action: () => void api.towerShow(false) },
  ];
  void menu.popup(button ? { clientX: button.left, clientY: button.bottom + 4 } : event, entries);
}

/** The window goes where its head is dragged. */
function drag(event: MouseEvent): void {
  if (event.button === 0) void getCurrentWindow().startDragging();
}

/* ---------- the window is as large as the panel ---------- */

const panel = ref<HTMLElement | null>(null);
/** The list scrolls inside itself before the panel outgrows its screen. */
const listRoom = computed(() => `${Math.max(120, window.screen.availHeight - 120)}px`);
/** The size the core was told last. */
let told = "";

/** Kept between the strip and its screen's edges, as the core keeps it (`MARGIN`, tower.rs). */
const EDGE = 16;
/** How many of the strip's projects are drawn; the rest are counted at its end (`+3 more`). All of
 * them until the strip is measured wider than its screen (`trim`). */
const drawn = ref(Infinity);
const chips = computed(() => listed.value.slice(0, drawn.value));
const more = computed(() => listed.value.length - chips.value.length);
/** The room that count was measured for. */
let measuredFor = 0;

/** The widest the strip may be on the screen it stands on. */
function stripRoom(): number {
  return Math.max(160, window.screen.availWidth - 2 * EDGE);
}

// Other projects, or the strip just chosen: all of them again, then as many as there is room for.
watch(
  () => [strip.value, ...listed.value.map((project) => project.path)].join("\n"),
  () => (drawn.value = Infinity),
);

/** Leaves out the strip's projects its screen has no room for, the last ones first (they are the
 * ones that matter least: `towerOrder`). True when their number changed: the page is drawn
 * again, and measured again. */
function trim(): boolean {
  const list = strip.value ? panel.value?.querySelector<HTMLElement>(".chips") : null;
  if (!list) return false;
  const room = stripRoom();
  if (room !== measuredFor) {
    // Another screen: all of them again first.
    measuredFor = room;
    if (drawn.value !== Infinity) {
      drawn.value = Infinity;
      return true;
    }
  }
  if (list.scrollWidth <= list.clientWidth) return false;
  const edge = list.getBoundingClientRect().right;
  const whole = [...list.children].filter((chip) => chip.getBoundingClientRect().right <= edge + 0.5).length;
  // Fewer each time, so it ends: the count that comes in at the end takes room of its own.
  drawn.value = Math.max(0, Math.min(whole, chips.value.length - 1));
  return true;
}

async function fit(): Promise<void> {
  await nextTick();
  // The strip first leaves out what its screen has no room for; its size is told once it has.
  if (trim()) return;
  const box = panel.value?.getBoundingClientRect();
  if (!box || box.width < 1 || box.height < 1) return;
  const [width, height] = [Math.ceil(box.width), Math.ceil(box.height)];
  if (told === `${width}x${height}`) return;
  told = `${width}x${height}`;
  try {
    await api.towerFit(width, height);
  } catch {
    // Not taken: told again with the next drawing.
    told = "";
  }
}

// Hidden, the page draws nothing and keeps nothing: it comes back folded, and says its size anew
// (that is what brings the window up).
watch(on, () => {
  told = "";
  unfolded.value = false;
  pointAway();
});

let unlistenFuel: (() => void) | null = null;
let unlistenPointer: (() => void) | null = null;
let gone = false;

/** The bundled fonts come in after the first drawing, and the strip is as wide as its letters. */
function onFonts(): void {
  drawn.value = Infinity;
  void fit();
}

onMounted(async () => {
  void fit();
  document.fonts?.addEventListener("loadingdone", onFonts);
  const [fuelOff, pointerOff] = await Promise.all([
    connectFuel(),
    getCurrentWindow().listen<{ x: number; y: number } | null>("tower-pointer", (event) => (event.payload ? pointAt(event.payload.x, event.payload.y) : pointAway())),
  ]);
  if (gone) return void (fuelOff(), pointerOff());
  [unlistenFuel, unlistenPointer] = [fuelOff, pointerOff];
});

onUpdated(() => {
  look();
  void fit();
});

onBeforeUnmount(() => {
  gone = true;
  document.fonts?.removeEventListener("loadingdone", onFonts);
  unlistenFuel?.();
  unlistenPointer?.();
});
</script>

<template>
  <div
    v-if="on"
    ref="panel"
    :class="['tower', strip ? 'strip' : 'list']"
    :style="strip ? { maxWidth: `${stripRoom()}px` } : undefined"
    role="group"
    :aria-label="t('tower.title')"
    @pointermove="pointAt($event.clientX, $event.clientY)"
    @pointerdown="press"
    @mousedown.capture="arm"
    @pointerleave="pointAway"
    @contextmenu.prevent="options"
  >
    <!-- One row: the glyph, each project's mark and letters, the fuel left. Dragged by anything but
         a project. -->
    <template v-if="strip">
      <span class="grip" @mousedown="drag"><PitwallGlyph :size="15" lamps="var(--run)" /></span>
      <ul class="chips" :aria-label="t('common.projects')" @mousedown.self="drag">
        <li
          v-for="project in chips"
          :key="project.path"
          :data-path="project.path"
          :class="['chip', { lit: lit(project) }]"
          :aria-label="said(project)"
          :title="said(project)"
          @dblclick="openFromOutside(project.path)"
        >
          <StatusIcon :project="project" :ring="lit(project) ? 'var(--bg-hover)' : 'var(--bg-panel)'" />
          <span class="code" aria-hidden="true">{{ letters.get(project.path) }}</span>
        </li>
      </ul>
      <!-- The projects its screen has no room for, counted. -->
      <span v-if="more" class="rest" role="img" :aria-label="t('flag.more', { count: more })" @mousedown="drag">{{ t("suggest.more", { count: more }) }}</span>
      <template v-if="share">
        <span class="rule" aria-hidden="true" @mousedown="drag"></span>
        <span :class="['share', { low: share.low, stale: share.stale }]" role="img" :aria-label="t('tower.fuel', { share: share.text })" @mousedown="drag">{{ share.text }}</span>
      </template>
    </template>

    <template v-else>
      <!-- The head: the window is dragged by it. -->
      <header class="head" @mousedown="drag">
        <span class="glyph"><PitwallGlyph :size="15" lamps="var(--run)" /></span>
        <span v-if="running" class="count run" role="img" :aria-label="t('popover.running', { count: running })">{{ running }}</span>
        <span v-if="working" class="count claude" role="img" :aria-label="t('core.tray.working', { count: working })">{{ working }}</span>
        <span class="grow"></span>
        <span v-if="share" :class="['share', { low: share.low, stale: share.stale }]" role="img" :aria-label="t('tower.fuel', { share: share.text })">{{ share.text }}</span>
        <button
          type="button"
          :class="['more', { lit: litAct('options') }]"
          data-act="options"
          aria-haspopup="menu"
          :aria-label="t('tower.options')"
          :title="t('tower.options')"
          @mousedown.stop
          @click="options"
        >
          <Icon name="more" :size="13" />
        </button>
      </header>

      <ul v-if="listed.length" class="rows" :style="{ maxHeight: listRoom }" :aria-label="t('common.projects')">
        <li
          v-for="project in listed"
          :key="project.path"
          :data-path="project.path"
          :class="['row', { lit: lit(project) }]"
          @dblclick="openFromOutside(project.path)"
        >
          <StatusIcon :project="project" :ring="lit(project) ? 'var(--bg-hover)' : 'var(--bg-panel)'" />
          <span class="name">{{ project.name }}</span>
          <!-- Cut short where a tool's name is long (an MCP tool's); whole for whoever reads it. -->
          <span :class="['state', states.get(project.path)?.tone]" :title="states.get(project.path)?.text">{{ states.get(project.path)?.text }}</span>
          <!-- In the state's place while the row is pointed at; there for a screen reader always. -->
          <span class="acts" @dblclick="actsDouble(project, $event)">
            <button
              v-if="project.claude || project.claudeWorking"
              type="button"
              :class="['act', { lit: litAct('claude', project) }]"
              data-act="claude"
              :aria-label="t('tower.bringUp', { name: project.name })"
              :title="t('sessions.action.bringUp')"
              @click.stop="may(project, $event) && bringUp(project)"
            >
              <Icon name="sparkles" :size="13" />
            </button>
            <button
              v-if="project.status === 'running'"
              type="button"
              :class="['act', { lit: litAct('browser', project) }]"
              data-act="browser"
              :aria-label="t('popover.openInBrowserName', { name: project.name })"
              :title="t('common.openInBrowser')"
              @click.stop="may(project, $event) && api.openBrowser(project.path)"
            >
              <Icon name="external" :size="13" />
            </button>
            <button
              type="button"
              :class="['act', { lit: litAct('toggle', project) }]"
              data-act="toggle"
              :disabled="project.status === 'busy'"
              :aria-label="t(project.status === 'running' ? 'common.stopName' : 'common.startName', { name: project.name })"
              :title="t(project.status === 'running' ? 'common.stop' : 'common.start')"
              @click.stop="may(project, $event) && toggle(project)"
            >
              <Spinner v-if="project.status === 'busy'" :size="12" />
              <Icon v-else :name="project.status === 'running' ? 'stop' : 'play'" :size="13" />
            </button>
          </span>
        </li>
      </ul>
      <p v-else-if="!rows.idle.length" class="none">{{ t("popover.noProjects") }}</p>

      <!-- How many are left out; a click shows them until the tower is hidden again. -->
      <button
        v-if="rows.idle.length"
        type="button"
        :class="['foot', { lit: litAct('foot') }]"
        data-act="foot"
        :aria-expanded="unfolded"
        @click="unfolded = !unfolded"
      >
        <span class="foot-text">{{ t("tower.stopped", { count: rows.idle.length }) }}</span>
        <span :class="['foot-mark', { up: unfolded }]"><Icon name="chevron-down" :size="12" /></span>
      </button>
    </template>
  </div>
</template>

<style>
/* The tower's window is see-through; only its panel paints, and nothing of the page scrolls. */
html[data-surface="tower"],
html[data-surface="tower"] body {
  background: transparent;
  overflow: hidden;
}

/* Linux: a borderless window may not be composited, so the page paints the panel's ground edge to
   edge there and the panel's corners go square, as the popover's do. */
html[data-platform="linux"][data-surface="tower"] body {
  background: var(--bg-panel);
}
</style>

<style scoped>
/* At the page's top left, as large as its content: the window takes this size (`fit`). */
.tower {
  position: absolute;
  top: 0;
  left: 0;
  box-sizing: border-box;
  background: var(--bg-panel);
  /* The window casts no shadow: this line is the panel's edge on whatever it stands over. */
  border: 1px solid var(--line-panel);
}

.tower.list {
  width: 248px;
  display: flex;
  flex-direction: column;
  padding: 4px;
  border-radius: var(--radius-panel);
}

:root[data-platform="linux"] .tower {
  border-radius: 0;
}

/* ---------- the head ---------- */

.head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 28px;
  padding: 0 4px 0 8px;
}

.glyph,
.grip {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-soft);
}

.count,
.share,
.state,
.code {
  flex-shrink: 0;
  font-family: var(--font-mono);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.count {
  font-size: 12px;
  font-weight: 500;
}

.count.run {
  color: var(--run);
}

.count.claude {
  color: var(--claude);
}

.grow {
  flex-grow: 1;
}

.share {
  font-size: 11.5px;
  color: var(--text-subtle);
}

.share.low {
  color: var(--crash-text);
}

/* An older reading: no state colour, as the menu bar's. */
.share.stale {
  color: var(--text-faint);
}

.more,
.act {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.more {
  width: 22px;
  height: 22px;
}

.act {
  width: 24px;
  height: 24px;
}

.more.lit,
.act.lit:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.act:disabled {
  opacity: 0.6;
}

/* ---------- a row per project ---------- */

.rows,
.chips {
  list-style: none;
  margin: 0;
  padding: 0;
}

.rows {
  display: flex;
  flex-direction: column;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.row {
  position: relative;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-control);
}

.row.lit {
  padding-right: 4px;
  background: var(--bg-hover);
}

/* The name gives way first, down to what still tells the project; then the state does. */
.name {
  flex: 1 1000 auto;
  min-width: 56px;
  font-size: 12.5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.state {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 11.5px;
  color: var(--text-subtle);
}

.state.claude {
  color: var(--claude-text);
}

.state.crash {
  color: var(--crash-text);
}

.state.faint {
  color: var(--text-faint);
}

/* The row's buttons take the state's place once the row is pointed at. Until then they are out
   of the row's flow and of the pointer's reach, unseen but there to be read and pressed by name. */
.acts {
  position: absolute;
  right: 4px;
  display: inline-flex;
  align-items: center;
  opacity: 0;
  pointer-events: none;
}

.row.lit .acts {
  position: static;
  flex-shrink: 0;
  opacity: 1;
  pointer-events: auto;
}

.row.lit .state {
  display: none;
}

.none {
  margin: 0;
  padding: 6px 10px 8px;
  font-size: 12px;
  color: var(--text-faint);
}

/* ---------- how many stand still ---------- */

.foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 26px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
  text-align: left;
}

.foot.lit {
  background: var(--bg-hover);
  color: var(--text);
}

.foot-text {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.foot-mark {
  display: inline-flex;
  flex-shrink: 0;
}

.foot-mark.up {
  transform: rotate(180deg);
}

/* ---------- one row of codes ---------- */

.tower.strip {
  width: max-content;
  display: flex;
  align-items: center;
  gap: 12px;
  height: 32px;
  padding: 0 12px 0 10px;
  border-radius: 16px;
}

/* Each project's ground reaches 4 px past its mark and letters, so the row's parts stand 12 px
   apart whichever is lit. */
.chips {
  display: flex;
  align-items: center;
  align-self: stretch;
  gap: 4px;
  margin: 0 -4px;
  /* On a screen too narrow for them all the last ones give way, never the fuel left (`trim`). */
  min-width: 0;
  overflow: hidden;
}

.chip {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  padding: 0 4px;
  border-radius: 6px;
}

.chip.lit {
  background: var(--bg-hover);
}

.code {
  font-size: 11.5px;
  color: var(--text-soft);
}

.rest {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  color: var(--text-subtle);
}

.rule {
  flex-shrink: 0;
  width: 1px;
  height: 14px;
  background: var(--line-strong);
}

/* The row is dragged by its glyph, its count and its share, each as tall as the row. */
.grip,
.rest,
.tower.strip .share {
  align-self: stretch;
  display: inline-flex;
  align-items: center;
}
</style>
