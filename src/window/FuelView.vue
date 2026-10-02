<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUpdated, ref, watch, type ComponentPublicInstance } from "vue";

import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import Rich from "../components/Rich.vue";
import Spinner from "../components/Spinner.vue";
import { duration } from "../lib/day";
import { ago } from "../lib/format";
import { clockText, current, dayClockText, isStale, left, moneyText, sideWindow, windowName } from "../lib/fuel";
import { language, t, type Key } from "../lib/i18n";
import { dragRegion } from "../lib/platform";
import { api, fuel, now, visible } from "../lib/store";
import type { UsageError, UsageWindow } from "../lib/types";
import FuelBackdrop from "./fuel/FuelBackdrop.vue";
import FuelFooter from "./fuel/FuelFooter.vue";
import FuelGauge from "./fuel/FuelGauge.vue";
import FuelNotice from "./fuel/FuelNotice.vue";
import SessionPace from "./fuel/SessionPace.vue";
import TripComputer from "./fuel/TripComputer.vue";

/** The least the refresh button spins, so a click the core answers at once still shows. */
const MIN_SPIN_MS = 500;

const limits = computed(() => fuel.value?.limits ?? null);
const usage = computed(() => limits.value?.usage ?? null);
const status = computed(() => limits.value?.status ?? "loading");
const today = computed(() => fuel.value?.today ?? null);

/** A reading the core couldn't renew: shown dimmed, under a notice that says why. */
const stale = computed(() => !!usage.value && !!limits.value && isStale(limits.value));

/** The limits as they stand now: one whose reset has passed since the reading has refilled. */
const windows = computed(() => (usage.value?.windows ?? []).map((window) => current(window, now.value)));
const session = computed(() => windows.value.find((window) => window.id === "five_hour") ?? null);
const week = computed(() => windows.value.find((window) => window.id === "seven_day") ?? null);
/** Beside the week: the most used model's week, else Extra Usage when it is on with a cap. */
const side = computed(() => sideWindow(windows.value));
const extra = computed(() => usage.value?.extra ?? null);
const extraOnGauge = computed(() => !side.value && !!extra.value?.enabled && (extra.value.monthlyLimit ?? 0) > 0);
/** The limits without a dial, listed under the cluster on request; one never used isn't worth a line. */
const rest = computed(() => windows.value.filter((window) => window.used > 0 && ![session.value, week.value, side.value].includes(window)));

/** `name` goes in the pod, under the figure; `caption` under the pod: when it refills, or what is
 * left; `until`: how long until it refills, after the caption (the session's dial only). */
type Dial = { key: string; size: "big" | "side"; left: number; name: string; caption: string; until: string | null };

function refills(window: UsageWindow, weekday: boolean): string {
  if (window.resetsAt === null) return t("fuel.notStarted");
  const when = weekday ? dayClockText(window.resetsAt, language.value) : clockText(window.resetsAt, language.value);
  return t("fuel.refills", { when });
}

/** How long until it refills, in hours and minutes. */
function until(window: UsageWindow): string | null {
  return window.resetsAt !== null && window.resetsAt > now.value ? duration(window.resetsAt - now.value) : null;
}

/** The cluster's dials, left to right: the week, the session (big), the side one. */
const dials = computed<Dial[]>(() => {
  const list: Dial[] = [];
  if (week.value) {
    const name = t("fuel.name.weekShort");
    list.push({ key: "week", size: "side", left: left(week.value.used), name, caption: refills(week.value, true), until: null });
  }
  if (session.value) {
    const name = t("fuel.session");
    list.push({ key: "session", size: "big", left: left(session.value.used), name, caption: refills(session.value, false), until: until(session.value) });
  }
  if (side.value) {
    const name = windowName(side.value, t);
    list.push({ key: "side", size: "side", left: left(side.value.used), name, caption: refills(side.value, true), until: null });
  } else if (extraOnGauge.value && extra.value) {
    const cap = extra.value.monthlyLimit ?? 0;
    const used = extra.value.used ?? (extra.value.usedCredits / cap) * 100;
    const money = moneyText(Math.max(0, cap - extra.value.usedCredits), extra.value.currency, language.value);
    const name = t("fuel.extra");
    list.push({ key: "extra", size: "side", left: left(used), name, caption: t("fuel.left", { value: money }), until: null });
  }
  return list;
});

/** Nothing to show yet: the core hasn't answered, or its first reading is on its way. */
const firstLoad = computed(() => !usage.value && (!fuel.value || status.value === "loading"));

const readAgo = computed(() => (limits.value?.fetchedAt ? ago(limits.value.fetchedAt, now.value) : ""));

const ERRORS: Record<UsageError | "unknown", [Key, Key]> = {
  network: ["fuel.error.network", "fuel.error.networkHint"],
  timeout: ["fuel.error.timeout", "fuel.error.timeoutHint"],
  forbidden: ["fuel.error.forbidden", "fuel.error.forbiddenHint"],
  server: ["fuel.error.server", "fuel.error.serverHint"],
  http: ["fuel.error.http", "fuel.error.httpHint"],
  badResponse: ["fuel.error.badResponse", "fuel.error.badResponseHint"],
  credentials: ["fuel.error.credentials", "fuel.error.credentialsHint"],
  keychain: ["fuel.error.keychain", "fuel.error.keychainHint"],
  unknown: ["fuel.error.unknown", "fuel.error.unknownHint"],
};

/** What the notice above the values says, if anything. */
const notice = computed<{ kind: "signedOut" | "expired" | "rateLimited" | "error" | "noLimits"; tag: string; title: string; body: string } | null>(() => {
  const state = limits.value;
  if (!state) return null;
  switch (state.status) {
    case "signedOut":
      return { kind: "signedOut", tag: t("fuel.state.signedOut.tag"), title: t("fuel.state.signedOut.title"), body: t("fuel.state.signedOut.body") };
    case "expired":
      return { kind: "expired", tag: t("fuel.state.expired.tag"), title: t("fuel.state.expired.title"), body: t("fuel.state.expired.body") };
    case "rateLimited":
      return {
        kind: "rateLimited",
        tag: t("fuel.state.rateLimited.tag"),
        title: t("fuel.state.rateLimited.title"),
        body: state.retryAt ? t("fuel.state.rateLimited.body", { time: clockText(state.retryAt, language.value) }) : t("fuel.state.rateLimited.later"),
      };
    case "error": {
      const [title, body] = ERRORS[state.error ?? "unknown"] ?? ERRORS.unknown;
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
</script>

<template>
  <div :class="['fuel', { stale }]">
    <!-- Its heading and free space drag the window; the refresh button stays clickable. -->
    <header class="bar" :data-tauri-drag-region="dragRegion">
      <div class="heading">{{ t("fuel.title") }}</div>
      <span v-if="readAgo" class="ago">{{ readAgo }}</span>
      <button type="button" class="refresh" :aria-label="t('fuel.refresh')" :title="t('fuel.refresh')" :aria-busy="spinning" @click="refresh">
        <Spinner v-if="spinning" />
        <Icon v-else name="restart" :size="16" />
      </button>
    </header>

    <!-- Between the bars, the page's body on its own ground: the smoke, lit round the dials. -->
    <div class="stage">
      <FuelBackdrop :window="session" :read-at="limits?.fetchedAt ?? now" :stale="stale" />
      <!-- Black rising from the bottom, gone by the trip computer's top. -->
      <div v-if="shade" class="shade" :style="{ height: `${shade}px` }" aria-hidden="true"></div>

      <div ref="scroller" class="scroll" @scroll.passive="measureShade">
        <div class="content">
          <!-- The Claude plan, top left on the page's ground, behind Claude's mark. -->
          <div v-if="limits?.plan" class="plan"><ClaudeLogo :size="32" />{{ t("fuel.plan", { plan: limits.plan }) }}</div>

          <FuelNotice v-if="notice" :tag="notice.tag" :title="notice.title">
            <Rich v-if="notice.kind === 'signedOut'" :text="notice.body">
              <template #claude><code>claude</code></template>
              <template #login><code>/login</code></template>
            </Rich>
            <template v-else>{{ notice.body }}</template>
            <span v-if="stale" class="dimmed">{{ t("fuel.state.dimmed") }}</span>
          </FuelNotice>

          <div v-if="firstLoad" class="loading" role="status">
            <Spinner />
            <span>{{ t("fuel.loading") }}</span>
          </div>

          <!-- The instrument cluster: the dials on one line, the trip computer's boxes under them. -->
          <section v-if="windows.length || today" :class="['cluster', { stale }]" :aria-label="t('fuel.cluster')">
            <div v-if="dials.length" class="dials">
              <FuelGauge v-for="dial in dials" :key="dial.key" :size="dial.size" :left="dial.left" :name="dial.name" :caption="dial.caption" :until="dial.until" :stale="stale" />
            </div>
            <TripComputer ref="trip" :session="session" :read-at="limits?.fetchedAt ?? now" :today="today" :stale="stale" />
          </section>

          <SessionPace v-if="session" ref="pace" :window="session" :read-at="limits?.fetchedAt ?? now" :now="now" :stale="stale" />
        </div>
      </div>

      <!-- Held at the bottom, on the same ground: it never scrolls away. -->
      <FuelFooter
        v-if="windows.length || today"
        class="footer"
        :rest="rest"
        :extra="windows.length ? extra : null"
        :extra-on-gauge="extraOnGauge"
        :today="today"
        :limits="windows.length > 0"
        :stale="stale"
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

/* The same height and line as the projects toolbar, so switching never moves them; the refresh
   button where a project's settings button sits. */
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
.refresh {
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

.refresh:hover {
  background: #262a33;
  color: var(--text-strong);
}

/* The body between the bars: the backdrop under it, the page over it. */
.stage {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-input);
}

/* Over the backdrop, under the page. */
.shade {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  pointer-events: none;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.75), rgba(0, 0, 0, 0.45) 45%, transparent);
}

/* Only the page body scrolls, never the window. Its size, width and height, sizes the dials. */
.scroll {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  container-type: size;
}

/* The whole window: the cluster takes whatever height the rest leaves. */
.content {
  min-height: 100cqh;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
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
  --dial: clamp(170px, min(40cqw - 60px, 100cqh - 418px), 600px);
  margin-block: auto;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: clamp(16px, 4cqw, 56px);
  padding: 28px 24px 22px;
}

/* The page's own margin below it, as on its sides. */
.footer {
  position: relative;
  flex-shrink: 0;
  padding: 0 20px 20px;
}

/* On a line of its own: no space to guess between sentences in Chinese and Japanese. */
.dimmed {
  display: block;
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
  The cards' common look, for the parts in ./fuel: the dials' pods made square. Smoked glass over
  the backdrop, lit faintly from below, a bezel catching the light at two corners; labels in
  capitals, as on the dials.
-->
<style>
/* A length, worked out where it is set: the dials read it as the big one's size, not their own. */
@property --dial {
  syntax: "<length>";
  inherits: true;
  initial-value: 300px;
}

.fuel {
  --glow: var(--claude);
}

.fuel.stale {
  --glow: var(--idle-ring);
}

.fuel .glass,
.fuel .card {
  position: relative;
  border-radius: calc(var(--radius-panel) + 4px);
  background: linear-gradient(180deg, rgba(30, 32, 38, 0.72), rgba(8, 9, 11, 0.88));
  -webkit-backdrop-filter: blur(10px);
  backdrop-filter: blur(10px);
  /* No shadow cast below: the ground round the cards stays one ground. */
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    inset 0 -14px 28px -16px color-mix(in srgb, var(--glow) 32%, transparent),
    0 0 0 1px rgba(255, 255, 255, 0.05);
}

.fuel .glass::before,
.fuel .card::before {
  content: "";
  position: absolute;
  inset: 0;
  padding: 1.5px;
  border-radius: inherit;
  background: linear-gradient(
    115deg,
    rgba(255, 255, 255, 0.3),
    rgba(255, 255, 255, 0.03) 25%,
    rgba(255, 255, 255, 0.16) 55%,
    rgba(255, 255, 255, 0.03) 80%,
    rgba(255, 255, 255, 0.26)
  );
  -webkit-mask:
    linear-gradient(#000 0 0) content-box,
    linear-gradient(#000 0 0);
  -webkit-mask-composite: xor;
  mask:
    linear-gradient(#000 0 0) content-box exclude,
    linear-gradient(#000 0 0);
  pointer-events: none;
}

.fuel .card {
  padding: 20px 22px;
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
