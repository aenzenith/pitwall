<script setup lang="ts">
import { computed, ref } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { agents } from "../../lib/agents";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";

/**
 * The found notice, once, on the projects' page under its head, in the page's flow (board AJ2): an
 * agent found on this Mac (its home folder, looked for at a start and whenever the window comes back: WindowApp) is on, and
 * the user is told so here, with the way to turn it off and to install its hook. Closing it, or
 * either button, is the user told: it doesn't come back (`agent_told`). An agent the user turned
 * off is never found on again. `hidden`: the first run is up, which tells of it itself.
 */
defineProps<{ hidden?: boolean }>();

/** The agent to tell of: found, on because it was, the user not told yet. */
const found = computed(() => agents.value.find((info) => info.undecided) ?? null);
const busy = ref<"off" | "hook" | null>(null);
const error = ref("");

async function act(what: "off" | "hook"): Promise<void> {
  const info = found.value;
  if (!info || busy.value) return;
  busy.value = what;
  error.value = "";
  try {
    if (what === "off") {
      await api.agentSetOn(info.id, false);
    } else {
      await api.installCodexHook();
      await api.agentTold(info.id);
    }
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = null;
  }
}

function close(): void {
  if (found.value) void api.agentTold(found.value.id).catch(() => undefined);
}
</script>

<template>
  <div v-if="found && found.id === 'codex' && !hidden" class="found" role="status">
    <AgentMark :agent="found.id" :size="20" />
    <div class="text">
      <span class="title">{{ t("agents.found.title.codex") }}</span>
      <span class="sub">{{ t(found.hook === "missing" ? "agents.found.text.codex" : "agents.found.textHooked.codex") }}</span>
      <span v-if="error" class="error" role="alert">{{ error }}</span>
    </div>
    <div class="acts">
      <button type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'off'" @click="act('off')">
        <span :class="{ hidden: busy === 'off' }">{{ t("agents.found.off.codex") }}</span>
        <Spinner v-if="busy === 'off'" class="over" :size="12" />
      </button>
      <button v-if="found.hook === 'missing'" type="button" class="control primary" :disabled="!!busy" :aria-busy="busy === 'hook'" @click="act('hook')">
        <span :class="{ hidden: busy === 'hook' }">{{ t("settings.agents.hook.install") }}</span>
        <Spinner v-if="busy === 'hook'" class="over" :size="12" />
      </button>
    </div>
    <button type="button" class="close" :aria-label="t('common.close')" :title="t('common.close')" @click="close">
      <Icon name="close" :size="14" />
    </button>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
/* Under the page's head, in its flow: the list starts under it until it is closed. */
.found {
  flex-shrink: 0;
  margin: 12px 20px 4px;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  padding: 11px 12px 11px 14px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: var(--bg-card);
  font-size: 13px;
  container-type: inline-size;
}

.text {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.acts {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 12px;
}

/* A narrow window: the buttons go under the words (one under the other when they must), the
   words keep their room and the close button stays at the first line's end. */
@container (max-width: 584px) {
  .acts {
    order: 2;
    flex-basis: 100%;
    padding-left: 32px;
  }

  .close {
    order: 1;
  }
}

.title {
  font-weight: 500;
  color: var(--text-strong);
}

.sub {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.control {
  height: 28px;
  padding: 0 12px;
  font-size: 12.5px;
}

.close {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.close:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.error {
  font-size: 12px;
}

/* The one thing to do, as the dialogs' (SettingsView: control primary). */
.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}
</style>
