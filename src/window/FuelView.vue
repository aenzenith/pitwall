<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUpdated, ref, watch, type ComponentPublicInstance } from "vue";

import AgentMark from "../components/AgentMark.vue";
import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import Rich from "../components/Rich.vue";
import Spinner from "../components/Spinner.vue";
import { accountInUse, accountName, severalAccounts } from "../lib/accounts";
import { agentName, multiAgent } from "../lib/agents";
import { codexFuel, fuelAgents, fuelPicked as picked, fuelShown as agent } from "../lib/codexFuel";
import { duration } from "../lib/day";
import { ago } from "../lib/format";
import { clockText, cluster, errorKeys, isStale } from "../lib/fuel";
import { language, t } from "../lib/i18n";
import { openInPanel } from "../lib/panel";
import { dragRegion } from "../lib/platform";
import { stillMotion } from "../lib/slide";
import { api, fuel, now, visible } from "../lib/store";
import type { Agent, UsageWindow } from "../lib/types";
import AccountNote from "./fuel/AccountNote.vue";
import AccountPanel from "./fuel/AccountPanel.vue";
import FuelBackdrop from "./fuel/FuelBackdrop.vue";
import FuelFooter from "./fuel/FuelFooter.vue";
import FuelGauge from "./fuel/FuelGauge.vue";
import FuelNotice from "./fuel/FuelNotice.vue";
import SessionPace from "./fuel/SessionPace.vue";
import TripComputer from "./fuel/TripComputer.vue";

/** `terminalProject`: the project the terminal panel shows, where "Open Claude with <account>"
 * opens its tab; null when there is none. */
const props = defineProps<{ terminalProject: string | null }>();

/** The gear at the bar's end opens Settings on its Claude tab; the account panel's "Manage
 * accounts" at its accounts (`part`), its "Add account" with the add dialog open. With another
 * agent picked, `agent` says whose (none: Claude's, as before). */
const emit = defineEmits<{ "open-settings": [part?: "accounts" | "addAccount", agent?: Agent] }>();

/** The least the refresh button spins, so a click the core answers at once still shows. */
const MIN_SPIN_MS = 500;

/* ---------- the agent picked ---------- */

/** More than one agent with a part on the page (`fuelAgents`): the picker at the page's top, each
 * with its accounts' chip. With Claude alone the page is as it was. `agent`: the one shown. */
const multi = computed(() => multiAgent.value && fuelAgents.value.length > 1);
const isCodex = computed(() => agent.value === "codex");

/** The picked agent's part of the payload: its limits, today's tokens. */
const part = computed(() => (isCodex.value ? codexFuel(fuel.value) : fuel.value));

const limits = computed(() => part.value?.limits ?? null);
const usage = computed(() => limits.value?.usage ?? null);
const status = computed(() => limits.value?.status ?? "loading");
const today = computed(() => part.value?.today ?? null);

/** Codex's limits come from its own logs: none yet until it makes a request (signed in, nothing
 * written down). */
const unread = computed(() => isCodex.value && status.value === "ready" && !usage.value?.windows.length);
/** Unread with no tokens today either: the message alone, no empty boxes under it. */
const unreadOnly = computed(() => unread.value && !today.value?.tokens.total);

function pick(each: Agent): void {
  picked.value = each;
}

function openSettings(where?: "accounts" | "addAccount"): void {
  if (isCodex.value) emit("open-settings", where, "codex");
  else emit("open-settings", where);
}

/** A reading the core couldn't renew: shown dimmed, under a notice that says why. */
const stale = computed(() => !!usage.value && !!limits.value && isStale(limits.value));

/** The limits as they stand now (one whose reset has passed since the reading has refilled) and
 * which of them get a dial: beside the week the most used model's week, else Extra Usage when it
 * is on with a cap. Worked out in lib/fuel, where the popover's strip reads the same three. */
const gauges = computed(() => cluster(usage.value, now.value, t, language.value));
const windows = computed(() => gauges.value.windows);
const session = computed(() => gauges.value.session);
/** The big dial's limit: the session, or the week on a reading without one. The backdrop's glow,
 * the trip computer and the pace strip follow it. */
const lead = computed(() => gauges.value.lead);
const extra = computed(() => gauges.value.extra);
const extraOnGauge = computed(() => gauges.value.extraOnGauge);
/** The limits without a dial, listed under the cluster on request; one never used isn't worth a line. */
const rest = computed(() => windows.value.filter((window) => window.used > 0 && ![session.value, gauges.value.week, gauges.value.side].includes(window)));

/** How long until it refills, in hours and minutes. */
function until(window: UsageWindow): string | null {
  return window.resetsAt !== null && window.resetsAt > now.value ? duration(window.resetsAt - now.value) : null;
}

/** The cluster's dials, left to right: the week, the session (big), the side one; without a
 * session the week is the big one. `name` goes in the pod, under the figure; `caption` under the
 * pod; `until`: how long until it refills, after the caption (the big dial only). `place`: the big
 * one's, or left or right of it. A dial is known by its place, not its limit: a reading with other
 * limits turns the dial standing there to its own value, and only a place filled or emptied
 * brings a dial in or takes one out. Another agent's dials are a page of their own (`.page`). */
const dials = computed(() => {
  const big = gauges.value.dials.findIndex((dial) => dial.size === "big");
  return gauges.value.dials.map((dial, index) => ({
    ...dial,
    place: index === big ? "big" : index < big ? "left" : "right",
    until: dial.size === "big" && dial.window ? until(dial.window) : null,
  }));
});

/** Nothing to show yet: the core hasn't answered, or its first reading is on its way. */
const firstLoad = computed(() => !usage.value && (!fuel.value || status.value === "loading"));

const readAgo = computed(() => (limits.value?.fetchedAt ? ago(limits.value.fetchedAt, now.value) : ""));

/** What the notice above the values says, if anything; `account`: the account in use whose
 * sign-in expired, with more than one account (its "Open Claude with" button). */
type Notice = { kind: "signedOut" | "expired" | "rateLimited" | "error" | "noLimits"; tag: string; title: string; body: string; account?: { id: string; name: string } };

const notice = computed<Notice | null>(() => {
  const state = limits.value;
  if (!state) return null;
  // Codex's: signed out, or nothing to show yet (`unread`, drawn on its own); nothing else is asked.
  if (isCodex.value) {
    return state.status === "signedOut" ? { kind: "signedOut", tag: t("fuel.state.signedOut.tag"), title: t("fuel.state.signedOut.title.codex"), body: t("fuel.state.signedOut.body.codex") } : null;
  }
  switch (state.status) {
    case "signedOut":
      return { kind: "signedOut", tag: t("fuel.state.signedOut.tag"), title: t("fuel.state.signedOut.title"), body: t("fuel.state.signedOut.body") };
    case "expired": {
      // With more than one account, which one's: renewed by opening Claude with it.
      const account = severalAccounts.value ? accountInUse.value : null;
      if (account) {
        const name = accountName(account);
        return { kind: "expired", tag: t("fuel.state.expired.tag"), title: t("accounts.expired.title", { name }), body: t("accounts.expired.body"), account: { id: account.id, name } };
      }
      return { kind: "expired", tag: t("fuel.state.expired.tag"), title: t("fuel.state.expired.title"), body: t("fuel.state.expired.body") };
    }
    case "rateLimited":
      return {
        kind: "rateLimited",
        tag: t("fuel.state.rateLimited.tag"),
        title: t("fuel.state.rateLimited.title"),
        body: state.retryAt ? t("fuel.state.rateLimited.body", { time: clockText(state.retryAt, language.value) }) : t("fuel.state.rateLimited.later"),
      };
    case "error": {
      const [title, body] = errorKeys(state.error);
      return { kind: "error", tag: t("fuel.state.error.tag"), title: t(title), body: t(body) };
    }
    case "ready":
      // An API-key account: no plan limits at all.
      return usage.value && !usage.value.windows.length
        ? { kind: "noLimits", tag: t("fuel.state.noLimits.tag"), title: t("fuel.state.noLimits.title"), body: t("fuel.state.noLimits.body") }
        : null;
    default:
      return null;
  }
});

/* ---------- refresh ---------- */

const asking = ref(false);
const spinning = computed(() => asking.value || (status.value === "loading" && !!fuel.value));

function ask(force: boolean): Promise<unknown> {
  // Before the core knows the command, this fails: the page stays as it is.
  return api.refreshFuel(force).catch(() => undefined);
}

async function refresh(): Promise<void> {
  if (asking.value) return;
  asking.value = true;
  const started = Date.now();
  await ask(true);
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
  asking.value = false;
}

onMounted(() => void ask(true));

/** "Open Claude with <account>": a Claude tab with it, in the project the terminal panel shows. */
function openClaude(account: string): void {
  if (props.terminalProject) openInPanel(props.terminalProject, true, account);
}

// Back on screen: a fresh look, as far as the core's pace allows.
watch(visible, (on) => {
  if (on) void ask(false);
});

/* ---------- shade ---------- */

const scroller = ref<HTMLElement | null>(null);
const trip = ref<ComponentPublicInstance | null>(null);
const pace = ref<ComponentPublicInstance | null>(null);
/** How far the shade over the backdrop reaches up: from the bottom to the trip computer's top, in px. */
const shade = ref(0);

function measureShade(): void {
  const ground = scroller.value?.parentElement?.getBoundingClientRect();
  const boxes = (trip.value?.$el as HTMLElement | undefined)?.getBoundingClientRect();
  shade.value = ground && boxes ? Math.max(0, ground.bottom - boxes.top) : 0;
}

// The boxes move when the window, the footer, the boxes or the card under them change size, and
// with whatever the page shows next.
const shadeObserver = new ResizeObserver(measureShade);
watch(scroller, (element, before) => {
  if (before) shadeObserver.unobserve(before);
  if (element) shadeObserver.observe(element);
});
for (const part of [trip, pace]) {
  watch(part, (component, before) => {
    if (before?.$el) shadeObserver.unobserve(before.$el as HTMLElement);
    if (component?.$el) shadeObserver.observe(component.$el as HTMLElement);
    measureShade();
  });
}
onUpdated(measureShade);

onBeforeUnmount(() => shadeObserver.disconnect());

/* ---------- a switch of agent ---------- */

/** How long what the page says of the agent picked takes to fade in, and how. */
const SWAP = { duration: 260, easing: "ease-out" };
/** How long the pace strip takes to open or close, and how: the dials above glide to make room
 * for it, or into the room it leaves. */
const FOLD = { duration: 320, easing: "cubic-bezier(0.2, 0, 0, 1)" };

const root = ref<HTMLElement | null>(null);

/** Where the agent picked stands in the picker: its dials as far along the ground, a page's width
 * for each agent before it (`data-pan`, FuelBackdrop). */
const agentIndex = computed(() => Math.max(0, fuelAgents.value.indexOf(agent.value)));

/** How stiff the spring is that carries the view from one agent's dials to another's, per second:
 * off at once, settled in under half a second, never past its mark. */
const SPRING = 16;

const backdrop = ref<InstanceType<typeof FuelBackdrop> | null>(null);
const pages = ref<HTMLElement | null>(null);

/** Where along the strip of the agents' dials the view stands, in pages (0: the first agent's),
 * and how fast it goes there, in pages a second. Each page stands at its own place on the strip
 * (`data-pan`), the ground under them all: a switch made while one slides goes on from where the
 * view is, at the speed it has. */
let view = agentIndex.value;
let speed = 0;
/** The frame the slide asked for, and when it last moved, in ms. */
let sliding = 0;
let movedAt = 0;
/** The pages going out, with what lets them go once the view has left them. */
const leaving = new Map<HTMLElement, () => void>();

/** Every page where the view has it. */
function place(): void {
  const box = pages.value;
  if (!box) return;
  for (const page of box.querySelectorAll<HTMLElement>(":scope > .page")) {
    page.style.transform = `translate3d(${(Number(page.dataset.pan) - view) * box.offsetWidth}px, 0, 0)`;
  }
}

/** One frame of the slide: the view a step on, the pages placed, and the ground drawn under them
 * in the same frame (FuelBackdrop's `step`), so the two never part. */
function slide(at: number): void {
  const seconds = Math.min(0.05, Math.max(0, (at - movedAt) / 1000));
  movedAt = at;
  // A critically damped spring, worked out exactly for the step, however long it was.
  const off = view - agentIndex.value;
  const lean = speed + SPRING * off;
  const decay = Math.exp(-SPRING * seconds);
  view = agentIndex.value + (off + lean * seconds) * decay;
  speed = (speed - SPRING * lean * seconds) * decay;
  // Within a pixel or so and all but still: the rest of the way is no more than a jump nobody sees.
  const done = Math.abs(view - agentIndex.value) < 0.0008 && Math.abs(speed) < 0.02;
  if (done) view = agentIndex.value;
  place();
  backdrop.value?.step(at);
  if (done) arrive();
  else sliding = requestAnimationFrame(slide);
}

/** The view at its mark: the pages going out let go, the one left back on the page's own layer. */
function arrive(): void {
  cancelAnimationFrame(sliding);
  sliding = 0;
  view = agentIndex.value;
  speed = 0;
  for (const done of leaving.values()) done();
  leaving.clear();
  for (const page of pages.value?.querySelectorAll<HTMLElement>(":scope > .page") ?? []) {
    page.style.transform = "";
    page.style.willChange = "";
  }
  backdrop.value?.lead(false);
}

// Another agent picked: the view slides to its dials, the ground along.
watch(agentIndex, () => {
  if (stillMotion()) return arrive();
  if (sliding) return;
  movedAt = performance.now();
  backdrop.value?.lead(true);
  sliding = requestAnimationFrame(slide);
});

/** A page coming in: on its own layer, where the view has it (off to its side, as the slide starts). */
function pageIn(element: Element, done: () => void): void {
  const page = element as HTMLElement;
  if (sliding) {
    page.style.willChange = "transform";
    place();
  }
  done();
}

/** A page going out: over the place it had, until the view has left it. */
function pageOut(element: Element, done: () => void): void {
  const page = element as HTMLElement;
  if (!sliding) return done();
  page.classList.add("out");
  page.style.willChange = "transform";
  leaving.set(page, done);
}

onBeforeUnmount(() => cancelAnimationFrame(sliding));

// Another agent picked: its dials slide in and what stays where it is (its account, the trip
// computer, the pace strip, the foot) fades in where the other's stood (every `[data-swap]`), so
// nothing changes at a stroke.
watch(
  agent,
  () => {
    if (stillMotion()) return;
    for (const element of root.value?.querySelectorAll("[data-swap]") ?? []) element.animate([{ opacity: 0 }, { opacity: 1 }], SWAP);
  },
  { flush: "post" },
);

/** The pace strip comes (`opening`) or goes: it grows from nothing to its height, or shrinks back
 * to nothing, rather than pushing the dials up or letting them fall at once. */
function fold(element: Element, opening: boolean, done: () => void): void {
  const box = element as HTMLElement;
  if (stillMotion()) return done();
  const style = getComputedStyle(box);
  const open = { height: `${box.offsetHeight}px`, paddingTop: style.paddingTop, paddingBottom: style.paddingBottom, borderTopWidth: style.borderTopWidth, opacity: 1 };
  const shut = { height: "0px", paddingTop: "0px", paddingBottom: "0px", borderTopWidth: "0px", opacity: 0 };
  box.style.overflow = "hidden";
  const run = box.animate(opening ? [shut, open] : [open, shut], FOLD);
  // The shade's top follows the trip computer's all the way there.
  let frame = requestAnimationFrame(function follow() {
    measureShade();
    frame = requestAnimationFrame(follow);
  });
  const end = (): void => {
    cancelAnimationFrame(frame);
    box.style.overflow = "";
    measureShade();
    done();
  };
  run.onfinish = end;
  run.oncancel = end;
}

const paceIn = (element: Element, done: () => void): void => fold(element, true, done);
const paceOut = (element: Element, done: () => void): void => fold(element, false, done);
</script>

<template>
  <div ref="root" :class="['fuel', { stale }]">
    <!-- Its heading and free space drag the window; the refresh button stays clickable. -->
    <header class="bar" :data-tauri-drag-region="dragRegion">
      <div class="heading">{{ t("fuel.title") }}</div>
      <!-- With more than one account: the one in use, and the panel of them all. With more than one
           agent it goes beside the agent picker. -->
      <AccountPanel v-if="severalAccounts && !multi" @add="emit('open-settings', 'addAccount')" @manage="emit('open-settings', 'accounts')" />
      <span v-if="readAgo" class="ago">{{ readAgo }}</span>
      <button type="button" class="refresh" :aria-label="t('fuel.refresh')" :title="t('fuel.refresh')" :aria-busy="spinning" @click="refresh">
        <Spinner v-if="spinning" />
        <Icon v-else name="restart" :size="16" />
      </button>
      <button type="button" class="options" :aria-label="t(isCodex ? 'fuel.options.codex' : 'fuel.options')" :title="t(isCodex ? 'fuel.options.codex' : 'fuel.options')" @click="openSettings()">
        <Icon name="settings" :size="16" />
      </button>
    </header>

    <!-- Between the bars, the page's body on its own ground: the smoke, lit round the dials. An
         instrument panel, drawn for both themes: smoked glass over embers in the dark one, frosted
         glass over a warm haze in the light one (the cluster's own roles in styles/tokens.css). -->
    <div class="stage">
      <FuelBackdrop ref="backdrop" :window="lead" :read-at="limits?.fetchedAt ?? now" :stale="stale" />
      <!-- The page's lower part on a ground of its own, from the bottom up to the trip computer's
           top: the trip computer, the session's pace and the foot stand on it, parted by lines. -->
      <div v-if="shade" class="shade" :style="{ height: `${shade}px` }" aria-hidden="true"></div>

      <div ref="scroller" class="scroll" @scroll.passive="measureShade">
        <div class="content">
          <!-- The Claude plan, top left on the page's ground, behind Claude's mark. -->
          <div v-if="!multi && limits?.plan" class="plan"><ClaudeLogo :size="32" />{{ t("fuel.plan", { plan: limits.plan }) }}</div>
          <!-- With more than one agent: each by its mark and name, the picked one bright; beside
               them the picked one's account in use with its plan, opening its accounts. -->
          <div v-else-if="multi" class="agents">
            <span class="picker" role="group" :aria-label="t('fuel.agents')">
              <button v-for="each in fuelAgents" :key="each" type="button" :class="['pick', { on: each === agent }]" :aria-pressed="each === agent" @click="pick(each)">
                <span class="pick-mark"><AgentMark :agent="each" :size="18" /></span>{{ agentName(each) }}
              </button>
            </span>
            <span class="rule-v" aria-hidden="true"></span>
            <AccountPanel :key="agent" :agent="agent" plan data-swap @add="openSettings('addAccount')" @manage="openSettings('accounts')" />
          </div>
          <!-- Across from it, for a moment after a switch: what new sessions open with, and how its
               MCP servers went. Held over the page: nothing moves as it comes and goes. -->
          <AccountNote />

          <!-- What is drawn of the agent picked above the trip computer, as a page of its own: at a
               switch the other agent's slides in whole from its side while this one slides out,
               the ground with them (`data-pan`: where along it the page stands). -->
          <div ref="pages" class="pages">
            <Transition :css="false" @enter="pageIn" @leave="pageOut">
              <div :key="agent" class="page" :data-pan="agentIndex">
                <FuelNotice v-if="notice" :tag="notice.tag" :title="notice.title">
                  <Rich v-if="notice.kind === 'signedOut'" :text="notice.body">
                    <template #claude><code>claude</code></template>
                    <template #codex><code>codex</code></template>
                    <template #login><code>/login</code></template>
                  </Rich>
                  <template v-else>{{ notice.body }}</template>
                  <span v-if="stale" class="dimmed">{{ t("fuel.state.dimmed") }}</span>
                  <template v-if="notice.account && terminalProject" #actions>
                    <button type="button" class="control" @click="openClaude(notice.account.id)">{{ t("accounts.openClaude", { name: notice.account.name }) }}</button>
                  </template>
                </FuelNotice>

                <div v-if="firstLoad" class="loading" role="status">
                  <Spinner />
                  <span>{{ t(isCodex ? "fuel.loading.codex" : "fuel.loading") }}</span>
                </div>

                <!-- Codex signed in, its limits not written down by it yet: they come with its next request. -->
                <div v-if="unread" class="unread" role="status">
                  <Icon name="fuel" :size="26" />
                  <span class="unread-title">{{ t("fuel.unread.title.codex") }}</span>
                  <span class="unread-body">{{ t("fuel.unread.body.codex") }}</span>
                </div>

                <!-- The instrument cluster: the dials on one line. -->
                <section v-if="(windows.length || today) && !unreadOnly" :class="['cluster', { stale }]" :aria-label="t('fuel.cluster')">
                  <!-- Each dial by its place: one coming grows out of it, one going shrinks into it. -->
                  <TransitionGroup v-if="dials.length" tag="div" name="dial" class="dials">
                    <FuelGauge
                      v-for="dial in dials"
                      :key="dial.place"
                      :size="dial.size"
                      :left="dial.left"
                      :name="dial.name"
                      :caption="dial.caption"
                      :until="dial.until"
                      :stale="stale"
                      :ring="`${agent}-${dial.place}`"
                    />
                  </TransitionGroup>
                </section>
              </div>
            </Transition>
          </div>

          <!-- The page's lower part, as wide as the page on its own ground (`.shade`): the trip
               computer and the leading window's pace, parted by a line, the foot under them. They stay
               where they are at a switch, their words fading in. -->
          <div v-if="(windows.length || today) && !unreadOnly" class="lower">
            <TripComputer ref="trip" :window="lead" :read-at="limits?.fetchedAt ?? now" :today="today" :stale="stale" />
            <Transition :css="false" @enter="paceIn" @leave="paceOut">
              <SessionPace v-if="lead" ref="pace" :window="lead" :read-at="limits?.fetchedAt ?? now" :now="now" :stale="stale" :agent="isCodex ? agent : undefined" />
            </Transition>
          </div>
        </div>
      </div>

      <!-- Held at the bottom, on the same ground: it never scrolls away. The backdrop reads where
           it stands (`data-foot`). -->
      <FuelFooter
        v-if="(windows.length || today) && !unreadOnly"
        class="footer"
        data-foot
        :rest="rest"
        :extra="windows.length ? extra : null"
        :extra-on-gauge="extraOnGauge"
        :today="today"
        :limits="windows.length > 0"
        :stale="stale"
        :last-read="isCodex && limits?.fetchedAt ? { at: limits.fetchedAt, now } : null"
      />
    </div>
  </div>
</template>

<style scoped>
.fuel {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: clip;
}

/* The same height and line as the projects toolbar, so switching never moves them; the gear where
   a project's settings button sits, as on the Track page. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 16px 0 20px;
  border-bottom: 1px solid var(--line);
}

/* The page's name alone, as the projects toolbar shows its own. */
.heading {
  flex-grow: 1;
  min-width: 0;
  font-size: 15px;
  font-weight: 600;
}

.ago {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

/* Shaped like a project's settings button (detail/detail.css `.icon`): no box until hovered. */
.refresh,
.options {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.refresh:hover,
.options:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* The body between the bars: the backdrop under it, the page over it. */
.stage {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-detail);
}

/* Over the backdrop, under the page. */
.shade {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  pointer-events: none;
  /* Frosted glass over the backdrop, as wide as the page: no corners, an edge along its top only. */
  border-top: 1px solid var(--line);
  background: color-mix(in srgb, var(--bg-tile) 72%, transparent);
  -webkit-backdrop-filter: blur(10px);
  backdrop-filter: blur(10px);
}

/* Only the page body scrolls, never the window. Its size, width and height, sizes the dials. */
.scroll {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  container-type: size;
  /* No room made for the terminal panel: it lies over the page, and the boxes stay where they are
     as it grows (WindowApp: `--panel-over`). */
}
/* Its scrollbar ends where the panel begins: the list keeps its size, only the bar's track is
   shorter (WindowApp: `--panel-over`). */
.scroll::-webkit-scrollbar-track {
  margin-bottom: var(--panel-over, 0px);
}


/* The whole window: the cluster takes whatever height the rest leaves, its lower part down to the
   foot. */
.content {
  position: relative;
  min-height: 100cqh;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px 20px 0;
}

/* The agents, then the picked one's account: one line, top left, where the plan is with one. */
.agents {
  align-self: flex-start;
  display: inline-flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
  max-width: 100%;
}

.picker {
  display: inline-flex;
  align-items: center;
  gap: 14px;
}

.pick {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-faint);
  white-space: nowrap;
  transition: color 0.2s ease;
}

.pick:hover {
  color: var(--text-muted);
}

.pick.on {
  color: var(--text-strong);
}

/* The others' marks faint with their names; the picked one's in its own colours. */
.pick-mark {
  display: inline-flex;
  opacity: 0.4;
  transition: opacity 0.2s ease;
}

.pick.on .pick-mark {
  opacity: 1;
}

.rule-v {
  width: 1px;
  height: 18px;
  flex-shrink: 0;
  background: var(--line-strong);
}

/* Codex's limits not written down yet: in the middle of the page, as the dials would be. */
.unread {
  margin-block: auto;
  align-self: center;
  max-width: 380px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 48px 0;
  text-align: center;
  color: var(--text-muted);
}

.unread-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
}

.unread-body {
  font-size: 13px;
  line-height: 1.5;
}

.plan {
  align-self: flex-start;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  font-size: 16px;
  font-weight: 600;
  color: var(--text);
  white-space: nowrap;
}

/* No box of its own: the dials stand on the page's ground, every bit of height the rest leaves. */
.cluster {
  flex: 1 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

/*
 * The pods centred on one line, in the middle of the height above the trip computer; they give way
 * together in a narrow window. `--dial` is the big one's width (and height): as wide as three pods
 * side by side allow, as tall as the cards under them leave room for.
 */
.dials {
  --dial: clamp(170px, min(40cqw - 60px, 100cqh - 376px), 600px);
  --dial-gap: clamp(16px, 4cqw, 56px);
  margin-block: auto;
  /* The room between the pods is theirs, half on each side (below), so one coming or going brings
     its share or takes it along; the line stands out by a half on each end to keep the outer pods
     where a gap would have them. */
  margin-inline: calc(var(--dial-gap) / -2);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 28px 24px 22px;
}

.dials > .gauge {
  margin-inline: calc(var(--dial-gap) / 2);
}

/* A dial coming grows out of nothing at its place, one going shrinks into it: the others glide
   apart to make room, or together into the room it leaves. */
.dials > .gauge.dial-enter-active,
.dials > .gauge.dial-leave-active {
  transition:
    flex-basis 0.5s ease,
    margin 0.5s ease,
    opacity 0.35s ease;
}

.dials > .gauge.dial-enter-from,
.dials > .gauge.dial-leave-to {
  flex-basis: 0;
  margin-inline: 0;
  opacity: 0;
}

/* The agent picked's page, between the line that stays above and the trip computer below: as wide
   as the window, its words where the page's padding had them. */
.pages {
  position: relative;
  flex: 1 0 auto;
  display: flex;
  flex-direction: column;
  margin-inline: -20px;
}

.page {
  flex: 1 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding-inline: 20px;
}

/* A page going out at a switch lies over the place it had, the new one taking it; both are placed
   by the slide (`place`). */
.page.out {
  position: absolute;
  inset: 0;
}

@media (prefers-reduced-motion: reduce) {
  .pick,
  .pick-mark,
  .dials > .gauge.dial-enter-active,
  .dials > .gauge.dial-leave-active {
    transition: none;
  }
}

/* Below the scrolling part, its line and room its own (FuelFooter). */
.footer {
  position: relative;
  flex-shrink: 0;
}

/* From one edge of the page to the other. Its parts and the foot share one measure: their words
   stand as far from the page's edges as the trip computer's from the lines between its parts,
   each part has the same room above and below, a line between two of them. */
.lower {
  display: flex;
  flex-direction: column;
  margin-inline: -20px;
}

/* On a line of its own: no space to guess between sentences in Chinese and Japanese. */
.dimmed {
  display: block;
}

/* The notice's button (an expired account's "Open Claude with"). */
.control {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text);
  font-size: 13px;
  white-space: nowrap;
}

.control:hover {
  background: var(--bg-control-hover);
}

.loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 48px 0;
  color: var(--text-muted);
}
</style>

<!--
  What the parts in ./fuel share: a box (a notice) has the other pages' boxes' corners and edge (the
  Today page's tiles), its ground frosted glass over the backdrop; labels in capitals, as on the
  dials.
-->
<style>
/* A length, worked out where it is set: the dials read it as the big one's size, not their own. */
@property --dial {
  syntax: "<length>";
  inherits: true;
  initial-value: 300px;
}

/* The room between two dials, worked out where it is set as `--dial` is. */
@property --dial-gap {
  syntax: "<length>";
  inherits: true;
  initial-value: 32px;
}

.fuel .glass {
  position: relative;
  border: 1px solid var(--line);
  border-radius: var(--radius-card);
  background: color-mix(in srgb, var(--bg-tile) 72%, transparent);
  -webkit-backdrop-filter: blur(10px);
  backdrop-filter: blur(10px);
}

.fuel .caps {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

:lang(zh) .fuel .caps,
:lang(ja) .fuel .caps {
  letter-spacing: 0;
}

.fuel .card-title {
  color: var(--text);
}
</style>
