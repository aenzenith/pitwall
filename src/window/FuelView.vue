<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUpdated, ref, watch, type ComponentPublicInstance } from "vue";

import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import Rich from "../components/Rich.vue";
import Spinner from "../components/Spinner.vue";
import { duration } from "../lib/day";
import { ago } from "../lib/format";
import { clockText, cluster, errorKeys, isStale } from "../lib/fuel";
import { language, t } from "../lib/i18n";
import { dragRegion } from "../lib/platform";
import { api, fuel, now, visible } from "../lib/store";
import type { UsageWindow } from "../lib/types";
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

/** The limits as they stand now (one whose reset has passed since the reading has refilled) and
 * which of them get a dial: beside the week the most used model's week, else Extra Usage when it
 * is on with a cap. Worked out in lib/fuel, where the popover's strip reads the same three. */
const gauges = computed(() => cluster(usage.value, now.value, t, language.value));
const windows = computed(() => gauges.value.windows);
const session = computed(() => gauges.value.session);
const extra = computed(() => gauges.value.extra);
const extraOnGauge = computed(() => gauges.value.extraOnGauge);
/** The limits without a dial, listed under the cluster on request; one never used isn't worth a line. */
const rest = computed(() => windows.value.filter((window) => window.used > 0 && ![session.value, gauges.value.week, gauges.value.side].includes(window)));

/** How long until it refills, in hours and minutes. */
function until(window: UsageWindow): string | null {
  return window.resetsAt !== null && window.resetsAt > now.value ? duration(window.resetsAt - now.value) : null;
}

/** The cluster's dials, left to right: the week, the session (big), the side one. `name` goes in
 * the pod, under the figure; `caption` under the pod; `until`: how long until it refills, after
 * the caption (the session's dial only). */
const dials = computed(() => gauges.value.dials.map((dial) => ({ ...dial, until: dial.key === "session" && dial.window ? until(dial.window) : null })));

/** Nothing to show yet: the core hasn't answered, or its first reading is on its way. */
const firstLoad = computed(() => !usage.value && (!fuel.value || status.value === "loading"));

const readAgo = computed(() => (limits.value?.fetchedAt ? ago(limits.value.fetchedAt, now.value) : ""));

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

    <!-- Between the bars, the page's body on its own ground: the smoke, lit round the dials. An
         instrument panel, drawn for both themes: smoked glass over embers in the dark one, frosted
         glass over a warm haze in the light one (the cluster's own roles in styles/tokens.css). -->
    <div class="stage">
      <FuelBackdrop :window="session" :read-at="limits?.fetchedAt ?? now" :stale="stale" />
      <!-- The ground's own colour rising from the bottom, gone by the trip computer's top. -->
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

      <!-- Held at the bottom, on the same ground: it never scrolls away. The backdrop reads where
           it stands (`data-foot`). -->
      <FuelFooter
        v-if="windows.length || today"
        class="footer"
        data-foot
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
  background: linear-gradient(
    to top,
    color-mix(in srgb, var(--bg-detail) 75%, transparent),
    color-mix(in srgb, var(--bg-detail) 45%, transparent) 45%,
    transparent
  );
}

/* Only the page body scrolls, never the window. Its size, width and height, sizes the dials. */
.scroll {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  container-type: size;
  /* Room under the end for the terminal panel lying over the page (WindowApp: `--panel-over`). */
  padding-bottom: var(--panel-over, 0px);
  scroll-padding-bottom: var(--panel-over, 0px);
}
/* Its scrollbar ends where the panel begins: the list keeps its size, only the bar's track is
   shorter (WindowApp: `--panel-over`). */
.scroll::-webkit-scrollbar-track {
  margin-bottom: var(--panel-over, 0px);
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
  The cards' common look, for the parts in ./fuel: the dials' pods made square. Glass over the
  backdrop (smoked in the dark theme, frosted in the light one), lit faintly from below, a bezel
  catching the light at two corners; labels in capitals, as on the dials.
-->
<style>
/* A length, worked out where it is set: the dials read it as the big one's size, not their own. */
@property --dial {
  syntax: "<length>";
  inherits: true;
  initial-value: 300px;
}

/* Set on the stage the cards stand in. */
.fuel .stage {
  --glow: var(--claude);
}

.fuel.stale .stage {
  --glow: var(--idle-ring);
}

.fuel .glass,
.fuel .card {
  position: relative;
  border-radius: calc(var(--radius-panel) + 4px);
  background: linear-gradient(180deg, var(--glass-top), var(--glass-bottom));
  -webkit-backdrop-filter: blur(10px);
  backdrop-filter: blur(10px);
  /* No shadow cast below: the ground round the cards stays one ground. */
  box-shadow:
    inset 0 1px 0 var(--glass-shine),
    inset 0 -14px 28px -16px color-mix(in srgb, var(--glow) var(--glass-glow), transparent),
    0 0 0 1px var(--glass-edge);
}

.fuel .glass::before,
.fuel .card::before {
  content: "";
  position: absolute;
  inset: 0;
  padding: 1.5px;
  border-radius: inherit;
  background: linear-gradient(115deg, var(--bezel-lit), var(--bezel-dim) 25%, var(--bezel-half) 55%, var(--bezel-dim) 80%, var(--bezel-far));
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
