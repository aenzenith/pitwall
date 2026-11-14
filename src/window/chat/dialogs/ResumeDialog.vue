<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Highlight from "../../../components/Highlight.vue";
import Spinner from "../../../components/Spinner.vue";
import { fuzzy } from "../../../lib/fuzzy";
import { t } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import type { PastSession } from "../../../lib/types";
import DialogFrame from "./DialogFrame.vue";
import { errorText, projectName, useListNav, when, type DialogProps } from "./shared";

/**
 * Resume: the project's earlier Claude sessions, those `claude --resume` finds from its folder
 * (`claude_past`: each one's name and when it was last written), the latest first and searchable
 * by name; a pick opens that session in a chat (the `resume` event), this one left out.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: []; resume: [session: string] }>();

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);

const list = ref<PastSession[] | null>(null);
const failed = ref<string | null>(null);
const query = ref((props.arg ?? "").trim());

onMounted(async () => {
  try {
    const read = await api.claudePast(props.path);
    list.value = read.filter((past) => past.agent === "claude" && past.id !== props.session).sort((a, b) => (b.at ?? 0) - (a.at ?? 0));
  } catch (reason) {
    failed.value = errorText(reason);
    list.value = [];
  }
});

type Item = { session: PastSession; name: string; hits: Set<number> | null };
const items = computed<Item[]>(() => {
  const q = query.value.trim();
  const out: Item[] = [];
  for (const session of list.value ?? []) {
    const name = session.title?.trim() || t("chatDialog.resume.untitled");
    if (!q) {
      out.push({ session, name, hits: null });
      continue;
    }
    const match = fuzzy(name, q);
    if (match) out.push({ session, name, hits: new Set(match.hits) });
  }
  return out;
});

function pick(item: Item): void {
  emit("resume", item.session.id);
  frame.value?.close();
}

const nav = useListNav(
  () => items.value.length,
  (i) => pick(items.value[i]),
  listEl,
);
watch(query, () => nav.move(0));

const rowId = (i: number) => `chat-resume-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.resume.title')" :sub="projectName(path)" :width="500" @close="emit('close')" @key="nav.onKey">
    <input
      v-model="query"
      class="search"
      type="search"
      data-focus
      spellcheck="false"
      autocomplete="off"
      :placeholder="t('chatDialog.resume.search')"
      :aria-label="t('chatDialog.resume.search')"
      role="combobox"
      aria-autocomplete="list"
      :aria-expanded="true"
      :aria-controls="`chat-resume-list-${id}`"
      :aria-activedescendant="items.length ? rowId(nav.active.value) : undefined"
    />

    <div v-if="list === null" class="state"><Spinner :size="14" /></div>
    <p v-else-if="failed" class="error" role="alert">
      {{ t("chatDialog.resume.failed") }} <span class="mono">{{ failed }}</span>
    </p>
    <p v-else-if="!list.length" class="state">{{ t("chatDialog.resume.empty") }}</p>
    <p v-else-if="!items.length" class="state">{{ t("chatDialog.noMatch", { query: query.trim() }) }}</p>
    <div v-else :id="`chat-resume-list-${id}`" ref="listEl" class="scroll list" role="listbox" :aria-label="t('chatDialog.resume.title')">
      <div
        v-for="(item, i) in items"
        :id="rowId(i)"
        :key="item.session.id"
        :data-row="i"
        role="option"
        :aria-selected="i === nav.active.value"
        :class="['row', { active: i === nav.active.value }]"
        @click="
          nav.active.value = i;
          pick(item);
        "
      >
        <span class="text">
          <span class="name" :class="{ untitled: !item.session.title?.trim() }" :title="item.name">
            <Highlight v-if="item.hits" :text="item.name" :hits="item.hits" />
            <template v-else>{{ item.name }}</template>
          </span>
        </span>
        <span v-if="item.session.at !== null" class="meta">{{ when(item.session.at) }}</span>
      </div>
    </div>

    <p class="note">{{ t("chatDialog.resume.note") }}</p>

    <template #foot>
      <button type="button" class="control" @click="frame?.close()">{{ t("common.cancel") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list {
  max-height: 360px;
}

.name {
  font-weight: 400;
}

.name.untitled {
  color: var(--text-subtle);
}
</style>
