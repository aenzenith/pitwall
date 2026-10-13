<script setup lang="ts">
import { computed, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { eventOn, notifications, quiet, quietLine } from "../lib/quiet";
import { api, now } from "../lib/store";

/**
 * Under the popover's head while it is quiet (`snapshot.quiet`): until when, what still comes
 * through, and the way out. It goes with the quiet. One line or two, each on one line: the
 * popover's window is fitted to its content.
 *
 * `leave`: the keyboard was on "Turn off", which goes with the banner, and the popover finds it
 * a place.
 */
const emit = defineEmits<{ leave: [] }>();

const line = computed(() => (quiet.value ? quietLine(quiet.value, now.value) : ""));
/** Questions and permissions still come through: said only while Claude's notifications are on. */
const through = computed(() => notifications.value.questionsThrough && eventOn("claude"));

/** Asked to end: it lasts until the snapshot says it is quiet no more. */
const busy = ref(false);

watch(quiet, (still) => {
  if (!still) busy.value = false;
});

async function end(event: MouseEvent): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  // Pressed from the keyboard, the button turns busy (disabled) and would drop the focus.
  if (event.detail === 0) emit("leave");
  try {
    await api.quietEnd();
  } catch {
    busy.value = false;
  }
}
</script>

<template>
  <!-- Padding, not a margin: the popover adds up its parts' heights to size its window. -->
  <section v-if="quiet" class="quiet" :aria-label="line">
    <div class="card">
      <Icon name="moon" :size="15" class="mark" />
      <span class="text">
        <span class="line" :title="line">{{ line }}</span>
        <span v-if="through" class="sub" :title="t('quiet.through')">{{ t("quiet.through") }}</span>
      </span>
      <button type="button" class="off" :disabled="busy" :aria-busy="busy" @click="end">
        <!-- The label keeps its room under the spinner: the button never changes width. -->
        <span :class="{ hidden: busy }">{{ t("quiet.off") }}</span>
        <Spinner v-if="busy" class="over" :size="11" />
      </button>
    </div>
  </section>
</template>

<style scoped>
.quiet {
  flex-shrink: 0;
  padding: 0 8px 8px;
}

.card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 8px 9px 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-card);
  background: var(--bg-control);
}

.mark {
  flex-shrink: 0;
  color: var(--text-soft);
}

/* Gives way to the button beside it. */
.text {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.line,
.sub {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.line {
  font-weight: 500;
}

.sub {
  font-size: 12px;
  color: var(--text-muted);
}

.off {
  position: relative;
  flex-shrink: 0;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-icon-hover);
  font-size: 12px;
}

.off:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.off:disabled {
  opacity: 0.6;
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
