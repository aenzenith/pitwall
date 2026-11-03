<script setup lang="ts">
import { emitTo } from "@tauri-apps/api/event";
import { computed } from "vue";

import AgentMark from "../components/AgentMark.vue";
import Icon from "../components/Icon.vue";
import { accountInUse, accountName, severalAccounts } from "../lib/accounts";
import { agentName, agentOn } from "../lib/agents";
import { codexFuel } from "../lib/codexFuel";
import { clockText, cluster, dayClockText, errorKeys, isStale, LOW_LEFT, percentText, type ClusterDial } from "../lib/fuel";
import { language, t } from "../lib/i18n";
import type { Page } from "../lib/pages";
import { api, fuel, now, snapshot } from "../lib/store";
import type { Agent, UsageState } from "../lib/types";
import FuelDial from "./FuelDial.vue";

// Claude's limits as the popover's foot, one short strip under a hairline: the session
// first (its dial, its share left large, when it refills), the Fuel page's other two dials (the
// week, the most used model's week) as lines beside it. The whole strip is one button that opens
// that page. Without a reading to draw, one line that says why; nothing at all while the setting
// is off, before the first reading, and for an account without plan limits. With Codex's dials
// to show too (Settings › Agents › Codex), the strip is two halves side by side, Claude's and
// Codex's, each its agent's mark, its lead dial and share and when that refills; each half opens
// the page on its agent. Codex's alone (Claude's box off) is the whole strip as Claude's is, its
// mark beside its share.

/** A dial as the strip writes it: `share` is what is left, as the language writes a percent;
 * `when` the moment it refills alone (a week's with its weekday), none before it started. */
type Drawn = { key: ClusterDial["key"]; left: number; name: string; caption: string; when: string | null; share: string; low: boolean };

type Shown =
  /** The dials: `lead` large at the left, `rest` as lines; `why`: the line under them when the
   * reading is one the core couldn't renew. */
  | { kind: "dials"; lead: Drawn; rest: Drawn[]; stale: boolean; why: string | null }
  /** No reading: why, in a line. */
  | { kind: "line"; text: string };

/** An old reading's state in a word or two, with the time the core asks again when it was told one. */
function tag(limits: UsageState): string {
  switch (limits.status) {
    case "signedOut":
      return t("fuel.state.signedOut.tag");
    case "expired":
      return t("fuel.state.expired.tag");
    case "rateLimited": {
      const name = t("fuel.state.rateLimited.tag");
      return limits.retryAt ? `${name} · ${t("popover.fuel.retryAt", { time: clockText(limits.retryAt, language.value) })}` : name;
    }
    default:
      return t("fuel.state.error.tag");
  }
}

/** Why there is no reading, as the Fuel page's notice heads it; null while one is on its way. */
function title(limits: UsageState): string | null {
  switch (limits.status) {
    case "signedOut":
      return t("fuel.state.signedOut.title");
    case "expired":
      return t("fuel.state.expired.title");
    case "rateLimited":
      return t("fuel.state.rateLimited.title");
    case "error":
      return t(errorKeys(limits.error)[0]);
    default:
      return null;
  }
}

function drawn(dial: ClusterDial): Drawn {
  // The page's name for the session ("5-hour session") is too long for a line here.
  const name = dial.key === "session" ? t("fuel.name.sessionShort") : dial.name;
  const at = dial.window?.resetsAt ?? null;
  const when = at === null ? null : dial.key === "session" ? clockText(at, language.value) : dayClockText(at, language.value);
  return { key: dial.key, left: dial.left, name, caption: dial.caption, when, share: percentText(dial.left, language.value), low: dial.left <= LOW_LEFT };
}

const shown = computed<Shown | null>(() => {
  const limits = fuel.value?.limits;
  if (!limits || !snapshot.value?.settings.fuelInPopover) return null;
  if (limits.usage) {
    const { windows, dials } = cluster(limits.usage, now.value, t, language.value);
    // An API-key account has no plan limits to draw; Extra usage alone is no cluster.
    if (!windows.length || !dials.length) return null;
    // The big dial leads (the session, or the week on a reading without one); else the first.
    const lead = dials.find((dial) => dial.size === "big") ?? dials[0];
    const stale = isStale(limits);
    return { kind: "dials", lead: drawn(lead), rest: dials.filter((dial) => dial !== lead).map(drawn), stale, why: stale ? tag(limits) : null };
  }
  const text = title(limits);
  return text ? { kind: "line", text } : null;
});

/** One agent's half of the strip while Codex's dials show: its lead dial alone. `note`: the
 * dial's name and when it refills, the time alone (half a strip has no room for more), `tip` the
 * whole of it; `empty`: no reading to draw, `note` saying why. */
type Half = { agent: Agent; left: number; name: string; share: string; note: string; tip: string; low: boolean; stale: boolean; empty: boolean };

function emptyHalf(agent: Agent, note: string): Half {
  return { agent, left: 0, name: note, share: "—", note, tip: note, low: false, stale: true, empty: true };
}

/** An agent's strip as its half: its lead dial alone. */
function half(agent: Agent, strip: Shown): Half {
  if (strip.kind === "line") return emptyHalf(agent, strip.text);
  const lead = strip.lead;
  const tip = `${lead.name} · ${lead.caption}`;
  return { agent, left: lead.left, name: lead.name, share: lead.share, note: `${lead.name} · ${lead.when ?? lead.caption}`, tip, low: lead.low, stale: strip.stale, empty: false };
}

/** Codex's strip, as Claude's: while Codex is on and its box is ticked. Its limits are its own
 * last words (never stale); signed in with none written yet, it says so; signed out, there is none. */
const codexShown = computed<Shown | null>(() => {
  if (!agentOn("codex") || snapshot.value?.settings.agents?.codex?.fuelInPopover === false) return null;
  const limits = codexFuel(fuel.value)?.limits;
  if (!limits || limits.status === "signedOut") return null;
  const { dials } = cluster(limits.usage, now.value, t, language.value);
  const lead = dials.find((dial) => dial.size === "big") ?? dials[0];
  if (!lead) return { kind: "line", text: t("popover.fuel.unread") };
  return { kind: "dials", lead: drawn(lead), rest: dials.filter((dial) => dial !== lead).map(drawn), stale: false, why: null };
});

/** The halves, Claude's first, while both have something to say. */
const halves = computed<Half[] | null>(() => {
  const [claude, codex] = [shown.value, codexShown.value];
  return claude && codex ? [half("claude", claude), half("codex", codex)] : null;
});

/** With more than one Claude account, the one in use: named beside its share (or before why
 * there is none), since the strip is its limits. */
const account = computed(() => (severalAccounts.value && accountInUse.value ? accountName(accountInUse.value) : null));

/** One agent's strip, the whole width: Claude's as it always was, else Codex's, `marked` with its
 * mark to say whose. */
const single = computed(() => {
  if (halves.value) return null;
  if (shown.value) return { agent: "claude" as Agent, strip: shown.value, account: account.value, marked: false };
  if (codexShown.value) return { agent: "codex" as Agent, strip: codexShown.value, account: null, marked: true };
  return null;
});

/** Pitwall's window, on the Fuel page (on `agent`'s part, from a half): the way the switcher's `/`
 * opens a page. */
function openFuel(agent?: Agent): void {
  const page: Page = "fuel";
  void emitTo("main", "reveal-page", agent ? { page, agent } : { page });
  void api.openWindow();
}
</script>

<template>
  <!-- Claude's and Codex's halves, side by side: each its own button. -->
  <footer v-if="halves" class="fuelstrip">
    <span class="fuelstrip-halves">
      <template v-for="(half, at) in halves" :key="half.agent">
        <span v-if="at > 0" class="fuelstrip-rule" aria-hidden="true"></span>
        <button
          type="button"
          :class="['fuelstrip-open', 'half', { stale: half.stale }]"
          :aria-label="t('popover.fuel.open')"
          :title="account && half.agent === 'claude' ? `${account} · ${half.tip}` : half.tip"
          :aria-describedby="`fuelstrip-${half.agent}`"
          @click="openFuel(half.agent)"
        >
          <FuelDial variant="big" :left="half.left" :name="half.name" :stale="half.stale" />
          <span :id="`fuelstrip-${half.agent}`" class="fuelstrip-lead">
            <span class="fuelstrip-top">
              <AgentMark :agent="half.agent" :size="12" />
              <span class="sr-only">{{ agentName(half.agent) }}</span>
              <span :class="['fuelstrip-share', { low: half.low, empty: half.empty }]" :aria-hidden="half.empty">{{ half.share }}</span>
            </span>
            <span class="fuelstrip-note">{{ half.note }}</span>
          </span>
        </button>
      </template>
    </span>
  </footer>
  <!-- The foot itself: with nothing to show there is none, and the panel ends at its list. -->
  <footer v-else-if="single" class="fuelstrip">
    <button
      type="button"
      :class="['fuelstrip-open', single.strip.kind]"
      :aria-label="t('popover.fuel.open')"
      :title="t('popover.fuel.open')"
      aria-describedby="fuelstrip-says"
      @click="openFuel(single.marked ? single.agent : undefined)"
    >
      <span v-if="single.strip.kind === 'dials'" id="fuelstrip-says" :class="['fuelstrip-body', { stale: single.strip.stale }]">
        <span class="fuelstrip-row">
          <FuelDial variant="big" :left="single.strip.lead.left" :name="single.strip.lead.name" :stale="single.strip.stale" />
          <span class="fuelstrip-lead">
            <span :class="['fuelstrip-top', { marked: single.marked }]">
              <template v-if="single.marked">
                <AgentMark :agent="single.agent" :size="12" />
                <span class="sr-only">{{ agentName(single.agent) }}</span>
              </template>
              <span :class="['fuelstrip-share', { low: single.strip.lead.low }]" aria-hidden="true">{{ single.strip.lead.share }}</span>
              <span v-if="single.account" class="fuelstrip-account">{{ single.account }}</span>
            </span>
            <span class="fuelstrip-note">{{ single.strip.lead.name }} · {{ single.strip.lead.caption }}</span>
          </span>
          <template v-if="single.strip.rest.length">
            <span class="fuelstrip-rule" aria-hidden="true"></span>
            <span class="fuelstrip-rest">
              <!-- A line has no room for its refill: it is the line's tip. -->
              <span v-for="dial in single.strip.rest" :key="dial.key" class="fuelstrip-line" :title="`${dial.name} · ${dial.caption}`">
                <FuelDial variant="mini" :left="dial.left" :name="dial.name" :stale="single.strip.stale" />
                <span class="fuelstrip-name" aria-hidden="true">{{ dial.name }}</span>
                <span :class="['fuelstrip-part', { low: dial.low }]" aria-hidden="true">{{ dial.share }}</span>
              </span>
            </span>
          </template>
        </span>
        <span v-if="single.strip.why" class="fuelstrip-why">{{ single.strip.why }}</span>
      </span>
      <template v-else>
        <AgentMark v-if="single.marked" :agent="single.agent" :size="12" />
        <span id="fuelstrip-says" class="fuelstrip-text">{{ single.account ? `${single.account} · ${single.strip.text}` : single.strip.text }}</span>
        <span class="fuelstrip-go">{{ t("fuel.nav") }} <Icon name="chevron" :size="12" /></span>
      </template>
    </button>
  </footer>
</template>

<style scoped>
/* Two agents' halves: as wide as each other, a hairline between. */
.fuelstrip-halves {
  display: flex;
  align-items: stretch;
  gap: 4px;
}

.fuelstrip-open.half {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
}

.half .fuelstrip-top,
.fuelstrip-top.marked {
  align-items: center;
  gap: 6px;
}

.fuelstrip-share.empty {
  color: var(--text-muted);
}

.fuelstrip-halves .fuelstrip-rule {
  margin: 8px 0;
}

.fuelstrip {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 6px 8px;
  border-top: 1px solid var(--line);
  background: var(--bg-footer);
}

/* No box of its own: it stands on the foot's ground, its dial starting where the rows' icons
   start. Pointed at, it lights as a row does. */
.fuelstrip-open {
  padding: 6px 8px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  text-align: left;
}

.fuelstrip-open:hover {
  background: var(--bg-hover);
}

.fuelstrip-body {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.fuelstrip-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* The session's share and what it is: this column gives way, the lines beside it keep their width. */
.fuelstrip-lead {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.fuelstrip-share {
  font-size: 18px;
  font-weight: 600;
  line-height: 1.1;
  letter-spacing: -0.01em;
  color: var(--claude-text);
  font-variant-numeric: tabular-nums;
}

/* The share and, with more than one account, whose it is. */
.fuelstrip-top {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.fuelstrip-top .fuelstrip-share {
  flex-shrink: 0;
}

.fuelstrip-account {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-soft);
}

.stale .fuelstrip-account {
  color: var(--text-muted);
}

.fuelstrip-note {
  font-size: 11px;
  line-height: 1.2;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-variant-numeric: tabular-nums;
}

.fuelstrip-rule {
  width: 1px;
  align-self: stretch;
  flex-shrink: 0;
  background: var(--line);
}

/* As wide as its longest line, within bounds: a long name (Extra usage) is cut, never the share. */
.fuelstrip-rest {
  flex: 0 0 auto;
  min-width: 96px;
  max-width: 128px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.fuelstrip-line {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.fuelstrip-name {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 11px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fuelstrip-part {
  flex-shrink: 0;
  margin-left: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--claude-text);
  font-variant-numeric: tabular-nums;
}

.fuelstrip-share.low,
.fuelstrip-part.low {
  color: var(--crash-text);
}

/* An older reading: no state colour, as its dials. */
.stale .fuelstrip-share,
.stale .fuelstrip-part {
  color: var(--text-muted);
}

.stale .fuelstrip-note,
.stale .fuelstrip-name {
  color: var(--text-faint);
}

/* Why the dials are grey, in the middle under them. */
.fuelstrip-why {
  font-size: 11px;
  text-align: center;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* No reading: what is wrong at the left, where a click leads at the right. */
.fuelstrip-open.line {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-muted);
}

.fuelstrip-text {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.fuelstrip-go {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-soft);
}
</style>
