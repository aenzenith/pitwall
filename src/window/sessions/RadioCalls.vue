<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { clock } from "../../lib/day";
import { percentText } from "../../lib/fuel";
import { agentKey } from "../../lib/format";
import { language, t } from "../../lib/i18n";
import { api, snapshot } from "../../lib/store";
import type { RadioCall } from "../../lib/types";

/**
 * What a session asked Pitwall over the team radio today, the newest first: when, which tool,
 * and what came of it. Asked of the core when the session's details show and again whenever a
 * call comes in (the snapshot counts them; the calls themselves are no part of it). Nothing is
 * drawn for a session that made none.
 */
const props = defineProps<{ session: string }>();

/** The calls as last answered, and whose they are. */
const answered = ref<{ session: string; calls: RadioCall[] }>({ session: "", calls: [] });
const calls = computed(() => (answered.value.session === props.session ? answered.value.calls : []));

let asking = 0;

async function load(): Promise<void> {
  const mine = ++asking;
  const session = props.session;
  try {
    const list = await api.radioCalls(session);
    if (mine === asking) answered.value = { session, calls: list ?? [] };
  } catch {
    // Kept as they were.
  }
}

watch([() => props.session, () => snapshot.value?.radioCalls], load, { immediate: true });

function result(call: RadioCall): string {
  const came = call.result;
  switch (came.kind) {
    case "status": {
      // The line the app shows for the server, when it has one.
      if (came.issue) return came.issue;
      const status = t(`status.${came.status}`);
      return came.port ? `${status} · :${came.port}` : status;
    }
    case "lines":
      return t("radio.result.lines", { count: came.count });
    case "card":
      return t("radio.result.card", { title: came.title, column: t("board.column.queued") });
    case "noted":
      return t("board.column.review");
    case "ran":
      return came.name;
    // [seam A board results]
    case "listed":
      // One card read is named by its title; a list, by how many it named.
      return came.title === null ? t("radio.result.listed", { count: came.count }) : t("radio.result.read", { title: came.title });
    case "edited":
      return t("radio.result.edited", { title: came.title });
    case "moved":
      return t("radio.result.card", { title: came.title, column: t(`board.column.${came.column}`) });
    case "deleted":
      return t("radio.result.deleted", { title: came.title });
    case "taken":
      return t("radio.result.card", { title: came.title, column: t(agentKey("board.column.claude", came.agent)) });
    // [seam B lap results]
    case "lapList":
      return t("radio.result.laps", { count: came.count });
    case "lapRead":
    case "lapDeleted":
      return t("radio.result.lap", { name: came.name });
    case "lapAdded":
    case "lapEdited":
      // Held: an agent's script waits, paused, for the user.
      return t(came.held ? "radio.result.lapPaused" : "radio.result.lap", { name: came.name });
    case "lapPaused":
      return t(came.paused ? "radio.result.lapPaused" : "radio.result.lapResumed", { name: came.name });
    // [seam C server and team results]
    case "stopped":
      return came.running ? came.name : t("radio.result.notRunning", { name: came.name });
    case "team":
      return t("radio.result.sessions", { count: came.count });
    case "fuel": {
      // What is left of each, in the catalog's own words for a limit and its share left.
      const left = (name: string, used: number | null): string[] => (used === null ? [] : [t("fuel.gauge.label", { name, value: percentText(Math.max(0, 100 - used), language.value) })]);
      const parts = [...left(t("fuel.name.sessionShort"), came.session), ...left(t("fuel.name.weekShort"), came.week)];
      return parts.length ? parts.join(" · ") : t("radio.result.noFuel");
    }
    // [seam D tab results]
    case "tabs":
      return t("radio.result.tabs", { count: came.count });
    case "tabOpened":
      return t("radio.result.tabOpened", { name: came.name });
    case "tabsClosed":
      return t("radio.result.tabsClosed", { count: came.count });
    case "tabsSent":
      return t("radio.result.tabsSent", { count: came.count });
    case "tabWoken":
      return t("radio.result.tabWoken", { name: came.name });
    case "tabRead":
    case "tabRenamed":
      return t("radio.result.read", { title: came.name });
    case "tabPinned":
      return t(came.pinned ? "radio.result.tabPinned" : "radio.result.tabUnpinned", { name: came.name });
    case "tabAwake":
      return t(came.awake ? "radio.result.tabAwake" : "radio.result.tabMaySleep", { name: came.name });
    case "refused":
      return t("radio.result.refused");
  }
}

const rows = computed(() => calls.value.map((call, at) => ({ key: `${call.at}-${at}`, time: clock(call.at), tool: call.tool, result: result(call) })));
</script>

<template>
  <section v-if="rows.length" class="calls" :aria-label="t('radio.calls')">
    <div class="calls-head">
      <span class="section-label">{{ t("radio.calls") }}</span>
      <span class="count">{{ t("radio.callsToday", { count: rows.length }) }}</span>
    </div>
    <ul>
      <li v-for="row in rows" :key="row.key">
        <span class="time">{{ row.time }}</span>
        <span class="tool">{{ row.tool }}</span>
        <span class="result" :title="row.result">{{ row.result }}</span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
/* A section of the details, as its others are (detail/detail.css: `.block`). */
.calls {
  flex-shrink: 0;
  padding: 14px 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  border-bottom: 1px solid var(--line);
}

.calls-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}

.count {
  flex-grow: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* A long day's calls scroll in their own room: six rows and the top of the next. */
ul {
  max-height: 182px;
  margin: 0;
  padding: 0;
  list-style: none;
  overflow-y: auto;
  overscroll-behavior: contain;
}

li {
  display: flex;
  align-items: baseline;
  gap: 12px;
  padding: 6px 0;
  border-top: 1px solid var(--line);
  font-size: 12px;
  line-height: 16px;
}

li:first-child {
  padding-top: 0;
  border-top: 0;
}

.time,
.tool {
  flex-shrink: 0;
  font-family: var(--font-mono);
}

.time {
  color: var(--text-subtle);
}

.tool {
  width: 104px;
}

.result {
  flex-grow: 1;
  min-width: 0;
  color: var(--text-soft);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
