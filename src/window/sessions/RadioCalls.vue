<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { clock } from "../../lib/day";
import { t } from "../../lib/i18n";
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
