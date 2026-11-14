<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Spinner from "../../../components/Spinner.vue";
import { t } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { projectName, useAsk, type DialogProps } from "./shared";

/**
 * Status (B9): the sections of Claude Code's `/status` screen as it words them (`get_status`:
 * version, session, model, servers, settings). The account's address and organisation are left
 * out here too, whatever the core let through (the plan's V65: no email).
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Row = { label?: string; value?: string };
type Status = { sections?: { title?: string; rows?: Row[] }[] };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const status = useAsk<Status>(() => props.id);
onMounted(() => void status.load({ kind: "status" }));

const PRIVATE_LABEL = /^(e-?mail|email address|organi[sz]ation|org|account email)$/i;
const EMAIL = /[^\s@]+@[^\s@]+\.[^\s@]+/;

const sections = computed(() =>
  (status.value.value?.sections ?? [])
    .map((section, i) => ({
      key: `${section.title ?? ""}-${i}`,
      title: section.title ?? "",
      rows: (section.rows ?? []).filter((row) => typeof row.value === "string" && !PRIVATE_LABEL.test(row.label?.trim() ?? "") && !EMAIL.test(row.value)),
    }))
    .filter((section) => section.rows.length),
);
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.status.title')" :sub="projectName(path)" :width="500" @close="emit('close')">
    <div v-if="status.busy.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="status.error.value" :id="id" :state="state" :error="status.error.value" @done="frame?.close()" />
    <p v-else-if="!sections.length" class="note">{{ t("chatDialog.status.empty") }}</p>

    <div v-else class="scroll">
      <section v-for="section in sections" :key="section.key" class="section">
        <div v-if="section.title" class="group">
          <span class="section-label">{{ section.title }}</span>
        </div>
        <dl class="rows">
          <div v-for="(row, i) in section.rows" :key="i" :class="['pair', { whole: !row.label }]">
            <dt v-if="row.label">{{ row.label }}</dt>
            <dd class="selectable">{{ row.value }}</dd>
          </div>
        </dl>
      </section>
    </div>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.section + .section {
  margin-top: 6px;
}

.group {
  padding-left: 0;
  padding-right: 0;
}

.rows {
  margin: 0;
  display: flex;
  flex-direction: column;
}

.pair {
  display: grid;
  grid-template-columns: minmax(110px, 38%) 1fr;
  gap: 12px;
  padding: 5px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12.5px;
}

.pair:last-child {
  border-bottom: 0;
}

.pair.whole {
  grid-template-columns: 1fr;
}

dt {
  color: var(--text-subtle);
}

dd {
  margin: 0;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text);
  overflow-wrap: anywhere;
}
</style>
