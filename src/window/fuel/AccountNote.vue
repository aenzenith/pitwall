<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";

import Spinner from "../../components/Spinner.vue";
import { accountByIdOf, accountName, accountNote, dismissNote, syncAccount } from "../../lib/accounts";
import { t } from "../../lib/i18n";
import type { Agent, AgentSyncReport, AgentSyncServer } from "../../lib/types";
import SyncDetails from "./SyncDetails.vue";

/**
 * The note at the top right of the Fuel page, across from the plan, in one line on the page's own
 * ground: for a moment after a switch ("New Claude sessions open with Personal. 2 MCP servers
 * synced."), and while a sync goes on; one that failed ("Try again") or a switch to an account
 * whose servers differ ("Details", "Sync") stays until it is acted on, the next switch, or the page
 * is left (lib/accounts). It takes no room: nothing on the page moves as it comes and goes.
 */

function named(report: AgentSyncReport | null, state: AgentSyncServer["state"]): string[] {
  return (report?.servers ?? []).filter((server) => server.state === state).map((server) => server.name);
}

const shown = computed(() => {
  const note = accountNote.value;
  if (!note || note.state === "switching") return null;
  const agent: Agent = note.agent ?? "claude";
  const account = accountByIdOf(agent, note.id);
  if (!account) return null;
  const name = accountName(account);
  const first = note.switched ? t(agent === "codex" ? "accounts.note.switched.codex" : "accounts.note.switched", { name }) : null;
  let rest: string | null = null;
  switch (note.state) {
    case "syncing":
      rest = t("accounts.note.syncing");
      break;
    case "done": {
      const synced = named(note.report, "synced").length;
      // Codex's: servers its CLI can't add are named, never said to match.
      const cantCopy = named(note.report, "cantCopy");
      const left = cantCopy.length ? t("accounts.note.cantCopy", { names: cantCopy.join(", ") }) : null;
      rest = [synced ? t("accounts.note.synced", { count: synced }) : first || left ? null : t("accounts.note.same"), left].filter(Boolean).join(" ") || null;
      break;
    }
    case "failed": {
      const failed = named(note.report, "failed");
      rest = failed.length ? t("accounts.note.failed", { names: failed.join(", ") }) : (note.error ?? t("accounts.note.failedAll"));
      break;
    }
    case "differs":
      rest = t("accounts.note.differs", { name, count: named(note.report, "missing").length });
      break;
  }
  if (!first && !rest) return null;
  return { id: note.id, agent, name, state: note.state, text: [first, rest].filter(Boolean).join(" "), report: note.report };
});

/** The details of a differing account's servers. */
const details = ref<{ id: string; agent: Agent; name: string; report: AgentSyncReport | null } | null>(null);

function sync(id: string, agent: Agent): void {
  void syncAccount(id, agent);
}

// Seen here: what waits on the user goes with the page.
onBeforeUnmount(dismissNote);
</script>

<template>
  <Transition name="note">
    <div v-if="shown" :class="['note', shown.state]" role="status">
      <Spinner v-if="shown.state === 'syncing'" :size="12" />
      <span class="text">{{ shown.text }}</span>
      <template v-if="shown.state === 'differs'">
        <button type="button" class="link" @click="details = { id: shown.id, agent: shown.agent, name: shown.name, report: shown.report }">{{ t("accounts.note.details") }}</button>
        <button type="button" class="link" @click="sync(shown.id, shown.agent)">{{ t("accounts.note.sync") }}</button>
      </template>
      <button v-else-if="shown.state === 'failed'" type="button" class="link" @click="sync(shown.id, shown.agent)">{{ t("accounts.retry") }}</button>
    </div>
  </Transition>
  <SyncDetails v-if="details" :id="details.id" :agent="details.agent" :name="details.name" :report="details.report" @close="details = null" />
</template>

<style scoped>
/* Across from the plan at the top of the page (FuelView's `.content`), level with it: plain words
   on the page's ground, held over the page so that nothing moves as it comes and goes. */
.note {
  position: absolute;
  top: 20px;
  right: 20px;
  max-width: calc(100% - 280px);
  height: 32px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-muted);
  white-space: nowrap;
}

/* It comes and goes softly, never all at once. */
.note-enter-active,
.note-leave-active {
  transition: opacity 0.25s ease;
}

.note-enter-from,
.note-leave-to {
  opacity: 0;
}

.note.failed .text {
  color: var(--crash-text);
}

.note > :deep(.spinner) {
  flex-shrink: 0;
  color: var(--text-muted);
}

.text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link {
  flex-shrink: 0;
  height: 26px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-soft);
  font-size: 13px;
  white-space: nowrap;
}

.link:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}
</style>
