<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Rich from "../components/Rich.vue";
import Spinner from "../components/Spinner.vue";
import { ago } from "../lib/format";
import { clockText, current, dayClockText, isStale, left, moneyText, sideWindow, windowName } from "../lib/fuel";
import { language, t, type Key } from "../lib/i18n";
import { api, fuel, now, visible } from "../lib/store";
import type { UsageError, UsageWindow } from "../lib/types";
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
/** The limits without a dial, listed under the cluster on request. */
const rest = computed(() => windows.value.filter((window) => ![session.value, week.value, side.value].includes(window)));

type Dial = { key: string; size: "big" | "side"; left: number; name: string; caption: string };

function refills(window: UsageWindow, weekday: boolean): string {
  if (window.resetsAt === null) return t("fuel.notStarted");
  const when = weekday ? dayClockText(window.resetsAt, language.value) : clockText(window.resetsAt, language.value);
  return t("fuel.refills", { when });
}

/** The cluster's dials, left to right: the week, the session (big), the side one. */
const dials = computed<Dial[]>(() => {
  const list: Dial[] = [];
  if (week.value) {
    const name = t("fuel.name.weekShort");
    list.push({ key: "week", size: "side", left: left(week.value.used), name, caption: `${name} · ${refills(week.value, true)}` });
  }
  if (session.value) {
    const name = t("fuel.session");
    list.push({ key: "session", size: "big", left: left(session.value.used), name, caption: `${name} · ${refills(session.value, false)}` });
  }
  if (side.value) {
    const name = windowName(side.value, t);
    list.push({ key: "side", size: "side", left: left(side.value.used), name, caption: `${name} · ${refills(side.value, true)}` });
  } else if (extraOnGauge.value && extra.value) {
    const cap = extra.value.monthlyLimit ?? 0;
    const used = extra.value.used ?? (extra.value.usedCredits / cap) * 100;
    const money = moneyText(Math.max(0, cap - extra.value.usedCredits), extra.value.currency, language.value);
    const name = t("fuel.extra");
    list.push({ key: "extra", size: "side", left: left(used), name, caption: `${name} · ${t("fuel.left", { value: money })}` });
  }
  return list;
});

/** Nothing to show yet: the core hasn't answered, or its first reading is on its way. */
const firstLoad = computed(() => !usage.value && (!fuel.value || status.value === "loading"));

const subtitle = computed(() => [t("fuel.subtitle"), limits.value?.plan].filter(Boolean).join(" · "));
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
</script>

<template>
  <div class="fuel">
    <!-- Its heading and free space drag the window; the refresh button stays clickable. -->
    <header class="bar" data-tauri-drag-region="deep">
      <div class="heading">
        <span class="title">{{ t("fuel.title") }}</span>
        <span class="subtitle">{{ subtitle }}</span>
      </div>
      <span v-if="readAgo" class="ago">{{ readAgo }}</span>
      <button type="button" class="refresh" :aria-label="t('fuel.refresh')" :title="t('fuel.refresh')" :aria-busy="spinning" @click="refresh">
        <Spinner v-if="spinning" />
        <Icon v-else name="restart" :size="13" />
      </button>
    </header>

    <div class="scroll">
      <div class="content">
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

        <!-- The instrument cluster: the dials on one baseline, the trip computer under them. -->
        <section v-if="windows.length || today" :class="['cluster', { stale }]" :aria-label="t('fuel.cluster')">
          <div v-if="dials.length" class="dials">
            <FuelGauge v-for="dial in dials" :key="dial.key" :size="dial.size" :left="dial.left" :name="dial.name" :caption="dial.caption" :stale="stale" />
          </div>
          <TripComputer :session="session" :read-at="limits?.fetchedAt ?? now" :today="today" :stale="stale" />
        </section>

        <SessionPace v-if="session" :window="session" :read-at="limits?.fetchedAt ?? now" :now="now" :stale="stale" />

        <FuelFooter
          v-if="windows.length || today"
          :rest="rest"
          :extra="windows.length ? extra : null"
          :extra-on-gauge="extraOnGauge"
          :today="today"
          :limits="windows.length > 0"
          :stale="stale"
        />
      </div>
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

/* The same height and line as the projects toolbar, so switching never moves them. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
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

.ago {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.refresh {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text-muted);
}

.refresh:hover {
  background: #2c3039;
  color: var(--text-strong);
}

/* Only the page body scrolls, never the window. */
.scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  container-type: inline-size;
}

.content {
  max-width: 1100px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
}

/* Darker than the cards, rounder: the dashboard the dials sit in. */
.cluster {
  display: flex;
  flex-direction: column;
  border-radius: calc(var(--radius-panel) + 4px);
  background: var(--bg-input);
  border: 1px solid var(--line);
  overflow: hidden;
}

/* One baseline; the dials give way together in a narrow window. */
.dials {
  display: flex;
  align-items: flex-end;
  justify-content: center;
  gap: clamp(16px, 4cqw, 36px);
  padding: 26px 24px 18px;
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

<!-- The cards' common look, for the parts in ./fuel. -->
<style>
.fuel .card {
  padding: 20px 22px;
  border-radius: var(--radius-panel);
  background: var(--bg-sidebar);
  border: 1px solid var(--line);
}

.fuel .card-title {
  font-size: 13px;
  font-weight: 600;
}
</style>
