<script setup lang="ts">
import { computed, ref } from "vue";

import Spinner from "../../../components/Spinner.vue";
import { t } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import type { ChatState } from "../../../lib/chat/wire";
import { errorText } from "./shared";

/**
 * What a dialog shows when Claude Code didn't answer it (a control it doesn't know in this
 * version, or refused): said in plain words, the CLI's own reason under them, and the one way on
 * that works: the same session in Claude Code's terminal, where the screen is its own (CONTRACT
 * "Additions"). A chat that isn't running is asked nothing, and offered nothing.
 */
const props = defineProps<{ id: number; state: ChatState; error: string }>();
/** `done`: the session went to the terminal; the dialog has nothing more to show. */
const emit = defineEmits<{ done: [] }>();

const exited = computed(() => props.state === "exited");
const busy = ref(false);
const failed = ref<string | null>(null);

async function toTerminal(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  failed.value = null;
  try {
    await api.chatToTerminal(props.id);
    emit("done");
  } catch (reason) {
    failed.value = errorText(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="unanswered" role="alert">
    <p class="say">{{ exited ? t("chatDialog.unanswered.exited") : t("chatDialog.unanswered.text") }}</p>
    <p v-if="error" class="why">{{ error }}</p>
    <template v-if="!exited">
      <p class="hint">{{ t("chatDialog.unanswered.hint") }}</p>
      <div>
        <button type="button" class="control" :disabled="busy" :aria-busy="busy" @click="toTerminal">
          <span :class="{ hidden: busy }">{{ t("chatDialog.unanswered.terminal") }}</span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
      <p v-if="failed" class="why">{{ failed }}</p>
    </template>
  </div>
</template>

<style scoped>
.unanswered {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px 14px;
  border-radius: 8px;
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-soft);
}

.say {
  margin: 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-strong);
}

.why {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--text-subtle);
  overflow-wrap: anywhere;
}

.hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.control {
  position: relative;
  height: 26px;
  margin-top: 2px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 12px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control:disabled {
  opacity: 0.5;
}

.hidden {
  visibility: hidden;
}

.over {
  position: absolute;
  inset: 0;
  margin: auto;
}
</style>
