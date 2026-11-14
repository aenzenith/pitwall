<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Spinner from "../../../components/Spinner.vue";
import { t, type Key } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { projectName, reveal, shownPath, sourceName as source, useAsk, type DialogProps } from "./shared";

/**
 * Permission rules (B9): the session's live rules as `/permissions` lists them
 * (`list_permission_rules`), by what they do (allowed, asked, denied), each with where it comes
 * from, and the folders added to the session. Read only: rules change in Claude Code's settings
 * files, or by "don't ask again" on a card.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Rule = {
  behavior?: string;
  source?: string;
  rule?: string;
  description?: { prefix?: string; emphasis?: string; suffix?: string } | null;
  notInEffect?: boolean;
};
type Rules = {
  state?: {
    rules?: Rule[];
    workspaceDirectories?: { path?: string; source?: string }[];
    managedOnly?: boolean;
    errors?: unknown[];
  };
};

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const rules = useAsk<Rules>(() => props.id);
onMounted(() => void rules.load({ kind: "permissionRules" }));

const GROUPS: { behavior: string; label: Key }[] = [
  { behavior: "allow", label: "chatDialog.permissions.allow" },
  { behavior: "ask", label: "chatDialog.permissions.ask" },
  { behavior: "deny", label: "chatDialog.permissions.deny" },
];

const listing = computed(() => rules.value.value?.state ?? null);
const groups = computed(() =>
  GROUPS.map((group) => ({ ...group, rules: (listing.value?.rules ?? []).filter((rule) => rule.behavior === group.behavior && typeof rule.rule === "string") })).filter((group) => group.rules.length),
);
const folders = computed(() => (listing.value?.workspaceDirectories ?? []).filter((folder) => typeof folder.path === "string"));
const errors = computed(() =>
  (listing.value?.errors ?? [])
    .map((error) => (typeof error === "string" ? error : error && typeof error === "object" && "message" in error ? String((error as { message: unknown }).message) : ""))
    .filter(Boolean),
);
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.permissions.title')" :sub="projectName(path)" :width="520" @close="emit('close')">
    <div v-if="rules.busy.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="rules.error.value" :id="id" :state="state" :error="rules.error.value" @done="frame?.close()" />

    <template v-else-if="listing">
      <p v-if="listing.managedOnly" class="note">{{ t("chatDialog.permissions.managedOnly") }}</p>
      <p v-for="(error, i) in errors" :key="`e${i}`" class="error">{{ error }}</p>

      <div class="scroll">
        <p v-if="!groups.length" class="state">{{ t("chatDialog.permissions.empty") }}</p>
        <template v-for="group in groups" :key="group.behavior">
          <div class="group">
            <span class="section-label">{{ t(group.label) }}</span>
            <span class="count">{{ group.rules.length }}</span>
          </div>
          <div v-for="(rule, i) in group.rules" :key="`${group.behavior}-${i}`" :class="['row', 'still', 'rule', { dim: rule.notInEffect }]">
            <span class="text">
              <span class="name mono selectable" :title="reveal(rule.rule!)">{{ reveal(rule.rule!) }}</span>
              <span v-if="rule.description" class="desc">
                {{ rule.description.prefix ?? "" }}<b v-if="rule.description.emphasis">{{ reveal(rule.description.emphasis) }}</b
                >{{ rule.description.suffix ?? "" }}
              </span>
            </span>
            <span v-if="rule.notInEffect" class="tag">{{ t("chatDialog.permissions.notInEffect") }}</span>
            <span class="tag">{{ source(rule.source) }}</span>
          </div>
        </template>

        <template v-if="folders.length">
          <div class="group">
            <span class="section-label">{{ t("chatDialog.permissions.folders") }}</span>
            <span class="count">{{ folders.length }}</span>
          </div>
          <div v-for="(folder, i) in folders" :key="`d${i}`" class="row still rule">
            <span class="text"
              ><span class="name mono selectable" :title="folder.path">{{ shownPath(path, folder.path!) }}</span></span
            >
            <span class="tag">{{ source(folder.source) }}</span>
          </div>
        </template>
      </div>

      <p class="note">{{ t("chatDialog.permissions.note") }}</p>
    </template>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.rule .name {
  font-size: 12px;
  font-weight: 400;
}

.rule.dim {
  opacity: 0.6;
}

.rule b {
  font-weight: 600;
  color: var(--text-soft);
}
</style>
