<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Spinner from "../components/Spinner.vue";
import { clock, dayName } from "../lib/day";
import { useBackdropClose } from "../lib/dialog";
import { openUnfocused, useReturnFocus } from "../lib/dialogFocus";
import { t } from "../lib/i18n";
import { dayShort, shiftDay } from "../lib/period";
import { api, snapshot, today } from "../lib/store";
import type { ResumeOffer, ResumeProject } from "../lib/types";

/**
 * What was open when Pitwall last quit, asked about at the next start: Restore puts it back (with
 * the box ticked, from now on without asking), Start empty drops the offer. Esc or a click outside
 * only closes the dialog: the offer stays, and the popover's banner still shows it.
 */
const props = defineProps<{ offer: ResumeOffer }>();
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const restoreButton = ref<HTMLButtonElement | null>(null);

// The offer as the dialog opened: the snapshot's goes once it is answered.
const offer = props.offer;

const always = ref(false);
/** The answer under way. */
const busy = ref<"restore" | "dismiss" | null>(null);
const error = ref("");

onMounted(() => {
  // Putting it back is the answer Return gives.
  openUnfocused(dialog.value, () => restoreButton.value);
});

/** When Pitwall quit: today and yesterday by name, an older day by its date. */
const quit = computed(() => {
  const day = dayName(offer.quitAt);
  const time = clock(offer.quitAt);
  if (day === today.value) return t("resume.quitToday", { time });
  if (day === shiftDay(today.value, -1)) return t("resume.quitYesterday", { time });
  return t("resume.quitOn", { date: dayShort(day), time });
});

/** What a project had: `server · 2 tabs · queue:work`. */
function had(project: ResumeProject): string {
  const parts: string[] = [];
  if (project.server) parts.push(t("resume.server"));
  if (project.terminals) parts.push(t("resume.tabs", { count: project.terminals }));
  return [...parts, ...project.commands].join(" · ");
}

// Answered, here or from the popover's banner: nothing is left to ask.
watch(
  () => snapshot.value?.resume ?? null,
  (left) => {
    if (!left) dialog.value?.close();
  },
);

/** Done once the offer has left the snapshot: the watch above closes the dialog then. */
async function restore(): Promise<void> {
  if (busy.value) return;
  busy.value = "restore";
  error.value = "";
  try {
    await api.resumeRestore(always.value);
    if (!snapshot.value?.resume) dialog.value?.close();
  } catch (reason) {
    error.value = String(reason);
    busy.value = null;
  }
}

async function dismiss(): Promise<void> {
  if (busy.value) return;
  busy.value = "dismiss";
  error.value = "";
  try {
    await api.resumeDismiss();
    dialog.value?.close();
  } catch (reason) {
    error.value = String(reason);
    busy.value = null;
  }
}
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="t('resume.title')" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="body">
      <div class="head">
        <div class="title">{{ t("resume.title") }}</div>
        <div class="sub">{{ quit }}</div>
      </div>

      <!-- Many projects scroll in here: the dialog never outgrows the window. -->
      <ul class="list">
        <li v-for="project in offer.projects" :key="project.path" class="row">
          <span class="ring" aria-hidden="true"></span>
          <span class="name" :title="project.name">{{ project.name }}</span>
          <span class="had" :title="had(project)">{{ had(project) }}</span>
        </li>
      </ul>

      <label class="check">
        <input v-model="always" type="checkbox" :disabled="!!busy" />
        <span>{{ t("resume.always") }}</span>
      </label>

      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <div class="actions">
        <button type="button" class="control" :disabled="!!busy" :aria-busy="busy === 'dismiss'" @click="dismiss">
          <!-- The label keeps its room under the spinner: the button never changes width. -->
          <span :class="{ hidden: busy === 'dismiss' }">{{ t("resume.dismiss") }}</span>
          <Spinner v-if="busy === 'dismiss'" class="over" :size="12" />
        </button>
        <button ref="restoreButton" type="button" class="control primary" :disabled="!!busy" :aria-busy="busy === 'restore'" @click="restore">
          <span :class="{ hidden: busy === 'restore' }">{{ t("resume.restore") }}</span>
          <Spinner v-if="busy === 'restore'" class="over" :size="12" />
        </button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the command dialog. */
.dialog {
  width: 380px;
  max-width: calc(100vw - 32px);
  max-height: calc(100vh - 32px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
  overflow: hidden;
}

.dialog[open] {
  display: flex;
  flex-direction: column;
  animation: pop 0.16s ease-out;
}

.body {
  flex: 1 1 auto;
  min-height: 0;
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.head,
.check,
.error,
.actions {
  flex-shrink: 0;
}

.head {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

.sub {
  font-size: 12px;
  color: var(--text-subtle);
}

/* Out to the dialog's edges, so the scrollbar runs along its side. */
.list {
  list-style: none;
  flex: 0 1 auto;
  min-height: 30px;
  margin: 0 -20px;
  padding: 0 20px;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.row {
  flex-shrink: 0;
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 10px;
}

/* Not running now: the list's idle ring. */
.ring {
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  margin: 1px;
  box-sizing: border-box;
  border: 1.5px solid var(--idle-ring);
  border-radius: 50%;
}

.name,
.had {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.name {
  flex: 1 1 auto;
  font-weight: 500;
}

/* Gives way first; the name keeps a third of the row at least. */
.had {
  flex: 0 1 auto;
  max-width: 66%;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.check {
  display: flex;
  align-items: center;
  gap: 10px;
}

.check input {
  width: 16px;
  height: 16px;
  margin: 0;
  flex-shrink: 0;
  accent-color: var(--run);
}

.error {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

.control {
  position: relative;
  flex-shrink: 0;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
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

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .dialog[open] {
    animation: none;
  }
}
</style>
