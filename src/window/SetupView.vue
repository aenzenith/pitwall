<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import RoadSky from "../components/RoadSky.vue";
import Spinner from "../components/Spinner.vue";
import { shortcutLabel } from "../lib/format";
import { t, type Key } from "../lib/i18n";
import { dragRegion, isMac } from "../lib/platform";
import { api, snapshot, visible } from "../lib/store";
import type { ExtensionStatus } from "../lib/types";
import StepAlerts from "./setup/StepAlerts.vue";
import StepExtension from "./setup/StepExtension.vue";
import StepHook from "./setup/StepHook.vue";
import StepProjects from "./setup/StepProjects.vue";
import StepRadio from "./setup/StepRadio.vue";
import StepStartup from "./setup/StepStartup.vue";

/**
 * The first run: over the whole window, the road of the site's download section running behind
 * it. A welcome first, from Settings too, then the six steps one at a time on a card, Back and
 * Next at its foot and Finish on the last. Next never waits for a step: each can be left as it
 * is. `fade`: it fades in (asked for again from Settings; the first run's is there at once).
 * `covered`: Settings stands over it. `done`: the steps are over, finished or put off;
 * `openPhone`: Settings at the phone's part is asked for.
 */
const props = defineProps<{ fade: boolean; covered: boolean }>();
const emit = defineEmits<{ done: []; openPhone: [] }>();

type Step = "projects" | "hook" | "radio" | "extension" | "startup" | "alerts";

const steps: Array<{ id: Step; title: Key; hint: Key }> = [
  { id: "projects", title: "setup.step.projects", hint: "setup.step.projectsHint" },
  { id: "hook", title: "setup.step.hook", hint: "setup.step.hookHint" },
  { id: "radio", title: "setup.step.radio", hint: "setup.step.radioHint" },
  { id: "extension", title: "setup.step.extension", hint: "setup.step.extensionHint" },
  { id: "startup", title: "setup.step.startup", hint: "setup.step.startupHint" },
  // The pitwall command is macOS's alone so far: elsewhere its line says nothing of it.
  { id: "alerts", title: "setup.step.alerts", hint: isMac ? "setup.step.alertsHint" : "setup.step.alertsHintNoCli" },
];
const order = steps.map((item) => item.id);

/* ---------- Pitwall for VS Code ---------- */

/** Whether the chosen editor has the extension: its step's, and its sector's colour. Read again
 * when the editor changes and when the window comes back (after installing it there). */
const extension = ref<ExtensionStatus | null>(null);
const checking = ref(false);
const checkFailed = ref(false);
/** Readings asked for so far: an answer to an older one is dropped. */
let checks = 0;

const editor = computed(() => snapshot.value?.settings.editor ?? "vscode");

async function checkExtension(): Promise<void> {
  const mine = ++checks;
  checking.value = true;
  try {
    const status = await api.extensionStatus(editor.value);
    if (mine !== checks) return;
    extension.value = status;
    checkFailed.value = !status;
  } catch {
    if (mine !== checks) return;
    extension.value = null;
    checkFailed.value = true;
  } finally {
    if (mine === checks) checking.value = false;
  }
}

watch(editor, () => void checkExtension(), { immediate: true });
window.addEventListener("focus", checkExtension);
onBeforeUnmount(() => window.removeEventListener("focus", checkExtension));

/* ---------- the pages ---------- */

/** A step is done when its condition holds. The last two have none: they are always done, so
 * they show green whether anything in them was touched or not. */
const done = computed<Record<Step, boolean>>(() => ({
  projects: !!snapshot.value?.projects.length,
  hook: !!snapshot.value?.claudeHook && !snapshot.value.claudeHookOutdated,
  radio: !!snapshot.value?.settings.radio?.on,
  extension: !!extension.value?.version,
  startup: true,
  alerts: true,
}));

const page = ref<"welcome" | Step>("welcome");

const at = computed(() => (page.value === "welcome" ? -1 : order.indexOf(page.value)));
const current = computed(() => steps[at.value] ?? null);
const last = computed(() => at.value === order.length - 1);

function hint(item: (typeof steps)[number]): string {
  return t(item.hint, { keys: shortcutLabel(snapshot.value?.settings.shortcutKeys ?? "") });
}

const heading = ref<HTMLElement | null>(null);

/**
 * To another page. On a step the keyboard goes to its heading (whose page it is now), since what
 * it was on may be gone; on the welcome it is nowhere, and Return starts all the same.
 */
function go(to: "welcome" | Step): void {
  page.value = to;
  if (to !== "welcome") void nextTick(() => heading.value?.focus({ preventScroll: true }));
}

function back(): void {
  go(at.value > 0 ? order[at.value - 1] : "welcome");
}

function next(): void {
  if (page.value === "welcome") go(order[0]);
  else if (last.value) void leave("finish");
  else go(order[at.value + 1]);
}

/* ---------- the keyboard ---------- */

/** What takes these keys itself: a field, a list to pick from, a switch. */
const TAKES_KEYS = "input, textarea, select, [contenteditable]:not([contenteditable='false'])";

/**
 * ←/→ move between pages and Return goes on, unless the key belongs to what has the keyboard
 * (Return on a button presses it; arrows in a field move its caret). Escape puts the run off, as
 * Later does. A dialog over the window (Settings) keeps its keys.
 */
function onKey(event: KeyboardEvent): void {
  if (event.defaultPrevented || props.covered || leaving.value || event.metaKey || event.ctrlKey || event.altKey) return;
  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest("dialog") || document.querySelector("dialog[open]")) return;
  if (event.key === "Escape") {
    event.preventDefault();
    void leave("later");
    return;
  }
  if (target?.closest(TAKES_KEYS)) return;
  if (event.key === "ArrowRight" && !event.shiftKey && !last.value) {
    event.preventDefault();
    next();
  } else if (event.key === "ArrowLeft" && !event.shiftKey && page.value !== "welcome") {
    event.preventDefault();
    back();
  } else if (event.key === "Enter" && !target?.closest("button, a, [role='button'], [role='switch'], label")) {
    event.preventDefault();
    next();
  }
}

onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));

/* ---------- leaving ---------- */

/** Which button is ending the run, while the core is told. */
const leaving = ref<"later" | "finish" | null>(null);

/** "Later" and "Finish" both end the first run: the steps don't come up by themselves again. */
async function leave(by: "later" | "finish"): Promise<void> {
  if (leaving.value) return;
  leaving.value = by;
  try {
    await api.setOnboarded();
  } catch {
    // A core that doesn't take it keeps nobody here: the steps come up again at the next start.
  }
  leaving.value = null;
  emit("done");
}

/** Drawn only while it can be seen: not with the window hidden, nor under Settings. */
const roadRunning = computed(() => visible.value && !props.covered);
</script>

<template>
  <section class="setup night" :class="{ fade, steps: page !== 'welcome' }" role="dialog" aria-modal="true" :aria-label="t('setup.title')">
    <div class="sky" aria-hidden="true">
      <RoadSky intro :running="roadRunning" />
    </div>
    <!-- The top edge drags the window, as a title bar would; the traffic lights sit in it. -->
    <!-- Over the whole road on the welcome, so its lines read clearly; gone on a step. -->
    <div class="shade" aria-hidden="true"></div>
    <div class="bar" :data-tauri-drag-region="dragRegion"></div>

    <!-- The welcome: the glyph, two lines rising, what Pitwall is in one, and the two ways on. -->
    <div v-if="page === 'welcome'" class="welcome">
      <PitwallGlyph class="glyph" :size="84" lamps="var(--run)" />
      <h1 class="title">
        <span v-for="(line, index) in t('setup.welcome.title').split('\n')" :key="index" class="line">
          <span :style="{ animationDelay: `${0.25 + index * 0.11}s` }">{{ line }}</span>
        </span>
      </h1>
      <p class="lead">{{ t("setup.welcome.lead") }}</p>
      <div class="ways">
        <button type="button" class="go" :disabled="!!leaving" @click="go(order[0])">
          {{ t("setup.start") }}
          <Icon name="arrow-right" :size="15" />
        </button>
        <button type="button" class="later" :disabled="!!leaving" :aria-busy="leaving === 'later'" @click="leave('later')">
          <span :class="{ hidden: leaving === 'later' }">{{ t("setup.later") }}</span>
          <Spinner v-if="leaving === 'later'" class="over" :size="12" />
        </button>
      </div>
    </div>

    <!-- A step: a card over the road, which runs on dimmed. -->
    <div v-else class="card">
      <div class="top">
        <!-- Each step's sector: the open one lit, a done one (and those with nothing to do) green. -->
        <div class="sectors" aria-hidden="true">
          <span v-for="(item, index) in steps" :key="item.id" :class="{ on: index === at, done: index !== at && done[item.id] }"></span>
        </div>
        <span class="count">{{ t("setup.stepOf", { n: at + 1, total: steps.length }) }}</span>
      </div>

      <div v-if="current" class="intro">
        <h2 :id="`setup-title-${current.id}`" ref="heading" tabindex="-1">{{ t(current.title) }}</h2>
        <p>{{ hint(current) }}</p>
      </div>

      <!-- Every pane stays alive under its step: what a step read or ticked is there on the way back. -->
      <div class="stage">
        <StepProjects v-show="page === 'projects'" id="setup-pane-projects" role="group" aria-labelledby="setup-title-projects" @added="go(order[1])" />
        <StepHook v-show="page === 'hook'" id="setup-pane-hook" role="group" aria-labelledby="setup-title-hook" />
        <StepRadio v-show="page === 'radio'" id="setup-pane-radio" role="group" aria-labelledby="setup-title-radio" />
        <StepExtension
          v-show="page === 'extension'"
          id="setup-pane-extension"
          role="group"
          aria-labelledby="setup-title-extension"
          :editor="editor"
          :status="extension"
          :checking="checking"
          :failed="checkFailed"
          @recheck="checkExtension"
        />
        <StepStartup v-show="page === 'startup'" id="setup-pane-startup" role="group" aria-labelledby="setup-title-startup" />
        <StepAlerts v-show="page === 'alerts'" id="setup-pane-alerts" role="group" aria-labelledby="setup-title-alerts" :shown="page === 'alerts'" :covered="covered" @open-phone="emit('openPhone')" />
      </div>

      <div class="foot">
        <button type="button" class="later" :disabled="!!leaving" :aria-busy="leaving === 'later'" @click="leave('later')">
          <span :class="{ hidden: leaving === 'later' }">{{ t("setup.later") }}</span>
          <Spinner v-if="leaving === 'later'" class="over" :size="11" />
        </button>
        <span class="moves">
          <button type="button" class="move" :disabled="!!leaving" @click="back">
            <Icon name="back" :size="13" />
            {{ t("setup.back") }}
          </button>
          <!-- Next, green once the step is done (quiet while it isn't, and it still goes on), and on
               the last step Finish, white: one button as wide as the wider of the two, so it never
               changes size between them. -->
          <button
            type="button"
            :class="['move', last ? 'go' : { primary: current && done[current.id] }]"
            :disabled="!!leaving"
            :aria-busy="leaving === 'finish'"
            @click="next"
          >
            <span class="faces">
              <span :class="['face', { hidden: last }]">
                {{ t("setup.next") }}
                <Icon name="arrow-right" :size="13" />
              </span>
              <span :class="['face', 'finish', { hidden: !last || leaving === 'finish' }]">
                {{ t("setup.finish") }}
                <Icon name="check" :size="13" />
              </span>
            </span>
            <Spinner v-if="leaving === 'finish'" class="over" :size="12" />
          </button>
        </span>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Over the whole window, sidebar and all (WindowApp: under Windows' own title bar, not over it). */
.setup {
  position: absolute;
  inset: 0;
  z-index: 44;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: clip;
  background: radial-gradient(ellipse 75% 65% at 50% 55%, var(--road-sky) 0%, var(--bg-log) 100%);
}

/* Asked for again from Settings, it comes up over the window gently; the first run's is just there. */
.setup.fade {
  animation: setup-in 0.4s ease both;
}

@keyframes setup-in {
  from {
    opacity: 0;
  }
}

/* The road, faded out at the top and bottom edges as on the site; dimmed behind a step's card. */
.sky {
  position: absolute;
  inset: 0;
  -webkit-mask-image: linear-gradient(to bottom, transparent 0%, #000 10%, #000 72%, transparent 100%);
  mask-image: linear-gradient(to bottom, transparent 0%, #000 10%, #000 72%, transparent 100%);
  transition: opacity 0.6s ease;
}

.steps .sky {
  opacity: 0.5;
}

/* One even dark over the whole road, the same everywhere. */
.shade {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: color-mix(in srgb, var(--bg-log) 70%, transparent);
  transition: opacity 0.6s ease;
}

.steps .shade {
  opacity: 0;
}

.bar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 40px;
}

/* ---------- the welcome ---------- */

.welcome {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 24px;
  padding: max(64px, 15vh) 32px 32px;
  text-align: center;
  pointer-events: none;
}

.welcome > * {
  pointer-events: auto;
}

.glyph {
  flex-shrink: 0;
  color: var(--text-soft);
  animation: setup-fade 0.9s ease-out both 0.1s;
}

.title {
  margin: 0;
  font-size: clamp(44px, 6.2vw, 64px);
  font-weight: 600;
  line-height: 0.98;
  letter-spacing: -0.045em;
  color: var(--text-strong);
}

/* Each line rises out of its own slot. */
.title .line {
  display: block;
  overflow: hidden;
  padding-bottom: 0.08em;
}

.title .line > span {
  display: block;
  animation: setup-rise 1.1s cubic-bezier(0.16, 1, 0.3, 1) both;
}

.lead {
  max-width: 540px;
  margin: 0;
  font-size: 17px;
  line-height: 1.55;
  color: var(--text-soft);
  text-wrap: balance;
  animation: setup-fade 0.9s ease-out both 0.45s;
}

/* Start and Later side by side, as wide as each other. */
.ways {
  display: inline-grid;
  grid-auto-flow: column;
  grid-auto-columns: 1fr;
  gap: 10px;
  margin-top: 4px;
  animation: setup-fade 0.9s ease-out both 0.55s;
}

.ways button {
  position: relative;
  height: 40px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 0 20px;
  border-radius: 9px;
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
}

/* Later: no ground of its own, only its edge. */
.ways .later {
  border: 1px solid var(--line-bold);
  background: transparent;
  color: var(--text-soft);
}

.ways .later:hover:not(:disabled) {
  border-color: var(--text-subtle);
  color: var(--text-strong);
}

/* The site's white button: Start here, Finish on the last step. */
.go {
  border: 0;
  background: var(--btn-go-bg);
  color: var(--btn-go-text);
}

.go:hover:not(:disabled) {
  background: var(--btn-go-hover);
}

.go:focus-visible {
  outline-offset: 3px;
}

@keyframes setup-rise {
  from {
    transform: translateY(105%);
  }
}

@keyframes setup-fade {
  from {
    opacity: 0;
    transform: translateY(14px);
  }
}

/* ---------- a step ---------- */

/* As tall in every step, so Next stays where it was; a small window takes from it and the step's
   block scrolls (setup/setup.css). */
.card {
  position: relative;
  width: min(720px, calc(100% - 64px));
  height: min(620px, calc(100% - 96px));
  margin-top: 16px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-dialog);
  border-radius: 14px;
  background: var(--bg-road-card);
  -webkit-backdrop-filter: blur(18px);
  backdrop-filter: blur(18px);
  box-shadow: var(--shadow-dialog);
  overflow: clip;
  animation: setup-card 0.35s cubic-bezier(0.16, 1, 0.3, 1) both;
}

@keyframes setup-card {
  from {
    opacity: 0;
    transform: translateY(10px) scale(0.985);
  }
}

.top {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px 26px 0;
}

.sectors {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: 1fr;
  gap: 4px;
}

.sectors span {
  height: 3px;
  border-radius: 2px;
  background: var(--line-strong);
  transition: background-color 0.3s ease;
}

.sectors .done {
  background: var(--run);
}

.sectors .on {
  background: var(--text);
}

.count {
  font-family: var(--font-mono);
  font-size: 11px;
  letter-spacing: 0.04em;
  color: var(--text-subtle);
}

.intro {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 10px 26px 0;
}

.intro h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-strong);
}

.intro h2:focus {
  outline: none;
}

.intro p {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-muted);
}

/* The open step. Its panes ask how wide it is (setup/setup.css). */
.stage {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  container: setup-step / inline-size;
}

.stage :deep(.pane) {
  padding: 16px 26px 20px;
}

.foot {
  flex-shrink: 0;
  height: 60px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 16px 0 14px;
  border-top: 1px solid var(--line);
}

.moves {
  margin-left: auto;
  display: inline-flex;
  gap: 8px;
}

/* The window's own button (setup/setup.css: control), a little taller here. */
.move {
  position: relative;
  height: 32px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 0 14px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
  color: var(--text);
  white-space: nowrap;
}

.move:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.move.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.move.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.move.go {
  border-color: transparent;
  background: var(--btn-go-bg);
  color: var(--btn-go-text);
}

/* Next's face and Finish's in one cell, the one not shown kept unseen for its width. Finish is
   always set bold, so its width is the same whichever shows. */
.faces {
  display: inline-grid;
}

.face {
  grid-area: 1 / 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
}

.face.finish {
  font-weight: 600;
}

.move.go:hover:not(:disabled) {
  background: var(--btn-go-hover);
}

/* Putting it off is always there, and quiet: a text button, no box until pointed at. */
.foot .later {
  position: relative;
  height: 28px;
  padding: 0 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 13px;
  color: var(--text-muted);
}

.foot .later:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.hidden {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}

/* Reduced motion: nothing rises or fades in, and the road stands still (RoadSky). */
@media (prefers-reduced-motion: reduce) {
  .setup.fade,
  .glyph,
  .title .line > span,
  .lead,
  .ways,
  .card {
    animation: none;
  }

  .sky,
  .shade,
  .sectors span {
    transition: none;
  }
}
</style>
