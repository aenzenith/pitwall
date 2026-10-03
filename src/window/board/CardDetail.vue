<script setup lang="ts">
import Markdown from "../../components/Markdown.vue";
import { COLUMN_LABELS, type CardStatus } from "../../lib/board";
import { t } from "../../lib/i18n";
import { dragRegion } from "../../lib/platform";
import type { Card, TerminalView } from "../../lib/types";
import SessionTerminal from "../sessions/SessionTerminal.vue";

/**
 * The board's details for a card with no session in the list (not given yet, or its session only
 * starting): what the card says, and its Pitwall terminal once it has one. A card with a session
 * shows that session's details instead (sessions/SessionDetail). `project`: its project's name;
 * `lastMessage`: what Claude last said in its session (Markdown), for a card whose work is over
 * and whose session has left the list. `hold`: the keyboard is in its terminal.
 */
defineProps<{ card: Card; project: string; status: CardStatus | null; terminal: TerminalView | null; lastMessage: string }>();
const emit = defineEmits<{ hold: [on: boolean] }>();
</script>

<template>
  <section class="detail" :aria-label="card.title">
    <!-- The name and lines under it drag the window. -->
    <header class="head" :data-tauri-drag-region="dragRegion">
      <div class="name" :title="card.title">{{ card.title }}</div>
      <div class="where">
        <span class="project">{{ project }}</span>
        <span class="dot" aria-hidden="true">·</span>
        <span class="column">{{ t(COLUMN_LABELS[card.column]) }}</span>
      </div>
      <div class="path" :title="card.path">{{ card.path }}</div>
    </header>

    <div class="body">
      <section v-if="status" class="block" :aria-label="t('window.col.claude')">
        <div class="section-label">{{ t("window.col.claude") }}</div>
        <span :class="['state', status.tone]">{{ status.text }}</span>
      </section>

      <section v-if="card.note" class="block" :aria-label="t('board.dialog.note')">
        <div class="section-label">{{ t("board.dialog.note") }}</div>
        <p class="card-note">{{ card.note }}</p>
      </section>

      <!-- In the terminal's place once its work is over: what Claude last said. -->
      <SessionTerminal v-if="terminal" :terminal="terminal" @hold="emit('hold', $event)" />
      <section v-else-if="lastMessage" class="block" :aria-label="t('sessions.lastMessage')">
        <div class="section-label">{{ t("sessions.lastMessage") }}</div>
        <Markdown :text="lastMessage" />
        <span class="said-note">{{ t("sessions.privacy") }}</span>
      </section>
    </div>
  </section>
</template>

<style scoped src="../detail/detail.css"></style>
<style scoped src="../sessions/panel.css"></style>
<style scoped>
.column {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Its session's state, in the card's own colours: Claude's while it waits on you. */
.state {
  font-size: 13px;
  color: var(--text-subtle);
}

.state.hot {
  color: var(--claude-text);
}

.state.live {
  color: var(--text-muted);
}

.said-note {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}
</style>
