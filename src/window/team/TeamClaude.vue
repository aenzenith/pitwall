<script setup lang="ts">
import ClaudeDot from "../../components/ClaudeDot.vue";
import type { ClaudeNote } from "../../lib/teams";

/**
 * What Claude is at in a group's projects, after the group's count: a team folded to its head
 * still says that Claude waits on you in it, or is at work. Each note is parted from what comes
 * before it by the count's own small dot ("2 projects · 1 running · Claude finished"), and
 * carries Claude's dot as a row does: filled while it waits, the breathing ring while it works.
 * The dot before the first note is the count's to draw, at its own end (team/TeamHead,
 * TeamDetail): cut away with the count in a narrow window, it never stands alone.
 */
defineProps<{ notes: ClaudeNote[] }>();
</script>

<template>
  <span v-if="notes.length" class="team-claude">
    <template v-for="(note, at) in notes" :key="note.text">
      <span v-if="at" class="sep" aria-hidden="true">·</span>
      <span :class="['note', note.phase]"><ClaudeDot :live="{ phase: note.phase, text: '' }" /><span class="note-text">{{ note.text }}</span></span>
    </template>
  </span>
</template>

<style scoped>
/* It gives way after the count beside it has and before the name does: in the narrowest window
   a note's words are cut short and its dot stays. Its room from the count is its own, so it
   keeps it from a count that is cut short too. */
.team-claude {
  flex: 0 20 auto;
  min-width: 13px;
  margin-left: 0.25em;
  display: inline-flex;
  align-items: center;
  overflow: hidden;
  white-space: nowrap;
  font-size: 12px;
}

/* The count's own " · ": a space either side, in its colour. */
.sep {
  flex-shrink: 0;
  margin: 0 0.25em;
  color: var(--text-subtle);
}

.note {
  flex: 0 1 auto;
  min-width: 7px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
}

.note-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.note.waiting {
  color: var(--claude-text);
}

.note.working {
  color: var(--text-muted);
}
</style>
