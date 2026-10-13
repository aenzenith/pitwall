<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import { useBackdropClose } from "../lib/dialog";
import { openUnfocused, useReturnFocus } from "../lib/dialogFocus";
import { t } from "../lib/i18n";
import { dayShort } from "../lib/period";
import { api, update } from "../lib/store";
import type { ReleaseNote, WhatsNew } from "../lib/types";

/**
 * What is new in the version that runs: the notes written for it, which the core hands over in
 * the app's language. It comes up once they are here; a version with none shows nothing and is
 * over at once. However it ends (OK, Esc, a click outside) the notes count as shown.
 */
const emit = defineEmits<{ close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();
const okButton = ref<HTMLButtonElement | null>(null);

/** Null until the core answers; `failed`: it didn't. */
const news = ref<WhatsNew | null>(null);
const failed = ref(false);

/** The notes count as shown, and the dialog is over: Esc, OK and a click outside end here too,
 * through the dialog's own close event. */
function done(): void {
  // A core that doesn't know the command yet: the notes come up again next time.
  void api.whatsNewSeen().catch(() => undefined);
  emit("close");
}

onMounted(async () => {
  try {
    const answer = await api.whatsNew();
    // Nothing written for this build: nothing to show.
    if (!answer?.notes?.length) return done();
    news.value = answer;
  } catch {
    failed.value = true;
  }
  // Opened with its notes in place: nothing moves once it is up.
  await nextTick();
  openUnfocused(dialog.value, () => okButton.value);
});

/** The version that runs; the update's own state knows it when the notes couldn't be read. */
const version = computed(() => news.value?.current ?? update.value?.current ?? "");

/** A release's date (`2026-10-12`) as a day; anything else as it is written. */
function day(date: string | null | undefined): string {
  if (!date) return "";
  return /^\d{4}-\d{2}-\d{2}$/.test(date) ? dayShort(date) : date;
}

/** Across from the title: the day this version came out, when it has one. */
const released = computed(() => day(news.value?.date));

/** What is new first, then the fixes under their label; what is new gets one only beside fixes. */
const groups = computed<Array<{ label: string; notes: ReleaseNote[] }>>(() => {
  const notes = news.value?.notes ?? [];
  const fixes = notes.filter((note) => note.kind === "fix");
  return [
    { label: fixes.length ? t("whatsNew.features") : "", notes: notes.filter((note) => note.kind === "new") },
    { label: t("whatsNew.fixes"), notes: fixes },
  ].filter((group) => group.notes.length);
});

/** The notes are longer than their room: hairlines then mark where they are cut. */
const list = ref<HTMLElement | null>(null);
const cut = ref(false);
const measure = new ResizeObserver(() => {
  const el = list.value;
  cut.value = !!el && el.scrollHeight > el.clientHeight + 1;
});
watch(list, (el, before) => {
  if (before) measure.unobserve(before);
  if (el) measure.observe(el);
});
onBeforeUnmount(() => measure.disconnect());

function openNotes(): void {
  void api.openLink("releases").catch(() => undefined);
}
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="t('whatsNew.label', { version })" @close="done" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="body">
      <div class="head">
        <div class="title">Pitwall {{ version }}</div>
        <div v-if="released" class="released">{{ released }}</div>
      </div>

      <!-- Long notes scroll in here: the dialog never outgrows the window. -->
      <div v-if="groups.length" ref="list" :class="['notes', { cut }]" role="region" tabindex="0" :aria-label="t('update.notes')">
        <template v-for="(group, at) in groups" :key="at">
          <div v-if="group.label" class="section-label">{{ group.label }}</div>
          <ul class="group">
            <li v-for="(note, index) in group.notes" :key="index" class="note">
              <Icon name="check" :size="14" class="mark" />
              <span class="note-text">
                <span class="note-title">{{ note.title }}</span>
                <span v-if="note.body" class="note-body">{{ note.body }}</span>
              </span>
            </li>
          </ul>
        </template>
      </div>
      <p v-else class="quiet">{{ t("whatsNew.failed") }}</p>

      <div class="actions">
        <button type="button" class="control" @click="openNotes">{{ t("whatsNew.all") }}</button>
        <button ref="okButton" type="button" class="control primary" @click="dialog?.close()">{{ t("whatsNew.ok") }}</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
/* The same look as the command dialog. As tall as its notes, up to the window: past that the
   notes scroll, never the dialog. */
.dialog {
  width: 400px;
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

/* The title and, across from it, the day: one line, on one baseline. */
.head {
  flex-shrink: 0;
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

.released {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

/* Out to the dialog's edges, so the scrollbar runs along its side. */
.notes {
  flex: 1 1 auto;
  min-height: 0;
  margin: 0 -20px;
  padding: 0 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* The scrollbar takes half of the right padding: the notes keep the dialog's right edge. */
.notes.cut {
  padding: 12px 10px 12px 20px;
  border-top: 1px solid var(--line-strong);
  border-bottom: 1px solid var(--line-strong);
}

.notes:focus-visible {
  outline-offset: -2px;
}

.notes > * {
  flex-shrink: 0;
}

/* A label keeps a little more room above it than the notes do between themselves. */
.notes > .section-label:not(:first-child) {
  margin-top: 4px;
}

.group {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.note {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

/* On the title's line; a quiet mark, green being a running server's. */
.mark {
  flex-shrink: 0;
  margin-top: 2px;
  color: var(--text-muted);
}

.note-text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.note-title {
  font-weight: 500;
}

.note-body {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-subtle);
}

.note-title,
.note-body {
  overflow-wrap: anywhere;
}

.quiet {
  flex-shrink: 0;
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.actions {
  flex-shrink: 0;
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

.control {
  flex-shrink: 0;
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover {
  background: var(--bg-control-hover);
}

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover {
  background: var(--btn-primary-hover);
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
