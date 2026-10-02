<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { useBackdropClose } from "../lib/dialog";
import { useReturnFocus } from "../lib/dialogFocus";
import { bareUrl } from "../lib/format";
import { t } from "../lib/i18n";
import type { LinkSuggestion, ProjectLink } from "../lib/types";

/** `suggestions`: addresses the project names itself, offered while adding. */
const props = defineProps<{ link: ProjectLink | null; projectName: string; suggestions: LinkSuggestion[] }>();
const emit = defineEmits<{ save: [link: ProjectLink]; remove: []; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
const nameInput = ref<HTMLInputElement | null>(null);
const draft = ref<ProjectLink>(props.link ? { ...props.link } : { name: "", url: "" });
const error = ref("");
const title = computed(() => t(props.link ? "link.editTitle" : "link.newTitle"));

useReturnFocus();

onMounted(() => {
  dialog.value?.showModal();
  nameInput.value?.focus();
});

/** The only kinds of address the core opens. */
const OPENABLE = ["http", "https", "mailto"];

/** The scheme an address starts with (`https`, `mailto`, `ftp`…), or `null`: in `localhost:5173`
 * the part before the colon is a host, with its port after it. */
function schemeOf(url: string): string | null {
  const scheme = /^([a-z][a-z0-9+.-]*):\/\//i.exec(url) ?? /^([a-z][a-z0-9+.-]*):(?!\d+(?:[/?#]|$))/i.exec(url);
  return scheme ? scheme[1].toLowerCase() : null;
}

/** `staging.app.dev` is saved as `https://staging.app.dev`; `localhost:5173` and IP addresses,
 * where dev servers listen, as `http://…`. */
function withScheme(url: string): string {
  if (schemeOf(url)) return url;
  const host = url.split(/[/?#]/)[0].replace(/^.*@/, "").replace(/:\d+$/, "").toLowerCase();
  const local = host === "localhost" || host.endsWith(".localhost") || /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.startsWith("[");
  return `${local ? "http" : "https"}://${url}`;
}

function submit(): void {
  const name = draft.value.name.trim();
  const url = draft.value.url.trim();

  if (!name || !url) {
    error.value = t("link.missing");
    return;
  }

  const scheme = schemeOf(url);
  if (scheme && !OPENABLE.includes(scheme)) {
    error.value = t("link.badScheme");
    return;
  }

  emit("save", { name, url: withScheme(url) });
}

/** Esc, Cancel and a click outside all end here, through the dialog's own close event. */
function cancel(): void {
  dialog.value?.close();
}
</script>

<template>
  <dialog ref="dialog" class="dialog" :aria-label="title" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <form class="body" @submit.prevent="submit">
      <div class="head">
        <div class="title">{{ title }}</div>
        <div class="project">{{ projectName }}</div>
      </div>
      <div class="fields">
        <label class="field">{{ t("link.name") }} <input ref="nameInput" v-model="draft.name" :placeholder="t('link.namePlaceholder')" spellcheck="false" /></label>
        <label class="field">{{ t("link.address") }} <input v-model="draft.url" class="mono" placeholder="https://staging.example.com" spellcheck="false" /></label>
      </div>
      <p v-if="error" class="error" role="alert">{{ error }}</p>

      <!-- Found in the project itself (git remote, .env, package.json): one click adds it. -->
      <div v-if="!link && suggestions.length" class="found">
        <div class="caps">{{ t("link.found") }}</div>
        <div v-for="s in suggestions" :key="s.url" class="found-row">
          <span class="found-name">{{ s.name }}</span>
          <span class="found-url" :title="s.url">{{ bareUrl(s.url) }}</span>
          <span class="found-source">{{ s.source }}</span>
          <button type="button" class="control small" @click="emit('save', { name: s.name, url: s.url })">{{ t("link.use") }}</button>
        </div>
      </div>

      <div class="actions">
        <button v-if="link" type="button" class="control danger" @click="emit('remove')">{{ t("common.delete") }}</button>
        <span class="grow"></span>
        <button type="button" class="control" @click="cancel">{{ t("common.cancel") }}</button>
        <button type="submit" class="control primary">{{ t(link ? "common.save" : "link.add") }}</button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
.dialog {
  width: 520px;
  max-width: calc(100vw - 32px);
  padding: 0;
  border: 1px solid #2f343d;
  border-radius: 12px;
  background: #1b1e24;
  color: var(--text);
  font-size: 13px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
}

.dialog[open] {
  animation: pop 0.16s ease-out;
}

.dialog::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

.body {
  padding: 18px 20px 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.head {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

.project {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.fields {
  display: grid;
  grid-template-columns: 150px minmax(0, 1fr);
  gap: 10px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-muted);
}

.field input {
  height: 30px;
  min-width: 0;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.field input.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.error {
  margin: 0;
  font-size: 12px;
  color: var(--crash-text);
}

.found {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.caps {
  margin-bottom: 4px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
}

:lang(zh) .caps,
:lang(ja) .caps {
  letter-spacing: 0;
}

.found-row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 36px;
  padding: 0 4px 0 10px;
  border-radius: var(--radius-control);
  background: var(--bg-panel);
}

.found-name {
  width: 110px;
  flex-shrink: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.found-url {
  flex-grow: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.found-source {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: #6c727c;
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 2px;
}

.grow {
  flex-grow: 1;
}

.control {
  height: 30px;
  padding: 0 12px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover {
  background: #2c3039;
}

.control.small {
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
}

.control.primary {
  background: #2a3a30;
  border-color: #3d5446;
  color: #cdebd8;
}

.control.primary:hover {
  background: #33473a;
}

.control.danger {
  color: var(--crash-text);
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}
</style>
