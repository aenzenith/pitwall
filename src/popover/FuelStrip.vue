<script setup lang="ts">
import { emitTo } from "@tauri-apps/api/event";
import { computed } from "vue";

import Icon from "../components/Icon.vue";
import { clockText, cluster, errorKeys, isStale, LOW_LEFT, percentText, type ClusterDial } from "../lib/fuel";
import { language, t } from "../lib/i18n";
import type { Page } from "../lib/pages";
import { api, fuel, now, snapshot } from "../lib/store";
import type { UsageState } from "../lib/types";
import FuelDial from "./FuelDial.vue";

// Claude's limits as the popover's foot, one short strip under a hairline: the session
// first (its dial, its share left large, when it refills), the Fuel page's other two dials (the
// week, the most used model's week) as lines beside it. The whole strip is one button that opens
// that page. Without a reading to draw, one line that says why; nothing at all while the setting
// is off, before the first reading, and for an account without plan limits.

/** A dial as the strip writes it: `share` is what is left, as the language writes a percent. */
type Drawn = { key: ClusterDial["key"]; left: number; name: string; caption: string; share: string; low: boolean };

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
  return { key: dial.key, left: dial.left, name, caption: dial.caption, share: percentText(dial.left, language.value), low: dial.left <= LOW_LEFT };
}

const shown = computed<Shown | null>(() => {
  const limits = fuel.value?.limits;
  if (!limits || !snapshot.value?.settings.fuelInPopover) return null;
  if (limits.usage) {
    const { windows, dials } = cluster(limits.usage, now.value, t, language.value);
    // An API-key account has no plan limits to draw; Extra usage alone is no cluster.
    if (!windows.length || !dials.length) return null;
    // The session leads; on a plan without one, the first dial there is.
    const lead = dials.find((dial) => dial.key === "session") ?? dials[0];
    const stale = isStale(limits);
    return { kind: "dials", lead: drawn(lead), rest: dials.filter((dial) => dial !== lead).map(drawn), stale, why: stale ? tag(limits) : null };
  }
  const text = title(limits);
  return text ? { kind: "line", text } : null;
});

/** Pitwall's window, on the Fuel page: the way the switcher's `/` opens a page. */
function openFuel(): void {
  const page: Page = "fuel";
  void emitTo("main", "reveal-page", { page });
  void api.openWindow();
}
</script>

<template>
  <!-- The foot itself: with nothing to show there is none, and the panel ends at its list. -->
  <footer v-if="shown" class="fuelstrip">
    <button
      type="button"
      :class="['fuelstrip-open', shown.kind]"
      :aria-label="t('popover.fuel.open')"
      :title="t('popover.fuel.open')"
      aria-describedby="fuelstrip-says"
      @click="openFuel"
    >
      <span v-if="shown.kind === 'dials'" id="fuelstrip-says" :class="['fuelstrip-body', { stale: shown.stale }]">
        <span class="fuelstrip-row">
          <FuelDial variant="big" :left="shown.lead.left" :name="shown.lead.name" :stale="shown.stale" />
          <span class="fuelstrip-lead">
            <span :class="['fuelstrip-share', { low: shown.lead.low }]" aria-hidden="true">{{ shown.lead.share }}</span>
            <span class="fuelstrip-note">{{ shown.lead.name }} · {{ shown.lead.caption }}</span>
          </span>
          <template v-if="shown.rest.length">
            <span class="fuelstrip-rule" aria-hidden="true"></span>
            <span class="fuelstrip-rest">
              <!-- A line has no room for its refill: it is the line's tip. -->
              <span v-for="dial in shown.rest" :key="dial.key" class="fuelstrip-line" :title="`${dial.name} · ${dial.caption}`">
                <FuelDial variant="mini" :left="dial.left" :name="dial.name" :stale="shown.stale" />
                <span class="fuelstrip-name" aria-hidden="true">{{ dial.name }}</span>
                <span :class="['fuelstrip-part', { low: dial.low }]" aria-hidden="true">{{ dial.share }}</span>
              </span>
            </span>
          </template>
        </span>
        <span v-if="shown.why" class="fuelstrip-why">{{ shown.why }}</span>
      </span>
      <template v-else>
        <span id="fuelstrip-says" class="fuelstrip-text">{{ shown.text }}</span>
        <span class="fuelstrip-go">{{ t("fuel.nav") }} <Icon name="chevron" :size="12" /></span>
      </template>
    </button>
  </footer>
</template>

<style scoped>
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
  color: #c3c8cf;
}
</style>
