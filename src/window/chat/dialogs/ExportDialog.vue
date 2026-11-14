<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { errorText, shownPath, useAsk, type DialogProps } from "./shared";

/**
 * Export: the conversation as the plain text Claude Code's `/export` writes
 * (`export_conversation`), shown, then saved where the user picks in the system's save dialog
 * (`chat_export`; the core writes it there and nowhere else) or copied. While the save dialog is
 * up nothing closes this one. The text lives in this dialog only, gone when it closes.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Export = { text?: string; default_filename?: string };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const exported = useAsk<Export>(() => props.id);
onMounted(() => void exported.load({ kind: "export" }));

const text = computed(() => exported.value.value?.text ?? "");
const name = computed(() => exported.value.value?.default_filename || "conversation.txt");
const lines = computed(() => (text.value ? text.value.replace(/^\n+/, "").split("\n").length : 0));

const saving = ref(false);
const saved = ref<string | null>(null);
const failed = ref<string | null>(null);

async function save(): Promise<void> {
  if (saving.value || !text.value) return;
  saving.value = true;
  failed.value = null;
  try {
    // Null: the save dialog was cancelled, and this one stays for another try.
    saved.value = await api.chatExport(props.id, text.value, name.value);
  } catch (reason) {
    failed.value = errorText(reason);
  } finally {
    saving.value = false;
  }
}

const copied = ref(false);
let copiedTimer = 0;
async function copy(): Promise<void> {
  if (!text.value) return;
  failed.value = null;
  try {
    await api.copyText(text.value);
    copied.value = true;
    window.clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => (copied.value = false), 1600);
  } catch (reason) {
    failed.value = errorText(reason);
  }
}
onBeforeUnmount(() => window.clearTimeout(copiedTimer));
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.export.title')" :sub="name" :width="560" :hold="saving" @close="emit('close')">
    <div v-if="exported.busy.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="exported.error.value" :id="id" :state="state" :error="exported.error.value" @done="frame?.close()" />
    <p v-else-if="!text.trim()" class="state">{{ t("chatDialog.export.empty") }}</p>

    <template v-else-if="saved === null">
      <pre class="block preview selectable" tabindex="0" :aria-label="t('chatDialog.export.preview')">{{ text.replace(/^\n+/, "") }}</pre>
      <span class="meta">{{ t("chatDialog.export.lines", { count: lines }) }}</span>
    </template>

    <p v-else class="saved" role="status">
      <span class="saved-mark"><Icon name="check" :size="12" /></span>
      <span class="saved-text">{{ t("chatDialog.export.saved", { file: shownPath(path, saved) }) }}</span>
    </p>

    <p v-if="failed" class="error" role="alert">{{ failed }}</p>

    <template #foot>
      <template v-if="saved === null">
        <button type="button" class="control" :disabled="!text.trim() || saving" @click="frame?.close()">{{ t("common.cancel") }}</button>
        <button type="button" class="control" :disabled="!text.trim() || saving" @click="copy">
          <!-- Both words hold their room: the button keeps its width when it says it copied. -->
          <span class="stack">
            <span :class="{ hidden: copied }">{{ t("chatDialog.export.copy") }}</span>
            <span :class="{ hidden: !copied }" aria-live="polite">{{ t("chatDialog.export.copied") }}</span>
          </span>
        </button>
        <button type="button" class="control primary" :disabled="!text.trim() || saving" :aria-busy="saving" @click="save">
          <span :class="{ hidden: saving }">{{ t("chatDialog.export.save") }}</span>
          <Spinner v-if="saving" class="over" :size="12" />
        </button>
      </template>
      <button v-else type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.preview {
  flex: 0 1 auto;
  min-height: 80px;
  max-height: 320px;
  overflow: auto;
  outline: none;
}

.meta {
  align-self: flex-end;
}

.stack {
  display: inline-grid;
}

.stack > * {
  grid-area: 1 / 1;
  text-align: center;
}

.saved {
  margin: 0;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  font-size: 12.5px;
  line-height: 1.5;
}

.saved-mark {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--bg-icon-hover);
  color: var(--run-text);
}

.saved-text {
  min-width: 0;
  overflow-wrap: anywhere;
}
</style>
