<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t, type Key } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { errorText, projectName, shownPath, useAsk, useListNav, when, within, type DialogProps } from "./shared";

/**
 * Memory (B9): the instruction files and memories Claude Code loads for the session, as its
 * `/memory` dialog lists them (`get_memory_dialog`). A file inside the project opens in the editor
 * (`open_reference`, which refuses anything outside a listed project); every other one is named
 * only. Nothing of any of them is read here.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type File = { kind?: string; path?: string; label?: string; description?: string; exists?: boolean };
type Folder = { kind?: string; path?: string; label?: string; description?: string };
type Memory = { name?: string; path?: string; description?: string | null; type?: string | null; modified_ms?: number };
type Dialog = { files?: File[]; folders?: Folder[]; memories?: Memory[]; auto_memory?: { enabled?: boolean; status?: string } };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);
const memory = useAsk<Dialog>(() => props.id);
onMounted(() => void memory.load({ kind: "memory" }));

const KINDS: Record<string, Key> = {
  user: "chatDialog.memory.user",
  project: "chatDialog.memory.project",
  "project-alt": "chatDialog.memory.projectAlt",
  local: "chatDialog.memory.local",
  auto: "chatDialog.memory.autoFolder",
  team: "chatDialog.memory.teamFolder",
  agent: "chatDialog.memory.agentFolder",
};

type Item = { key: string; label: string; path: string; shown: string; note: string; folder: boolean; missing: boolean; inside: string | null };

const items = computed<Item[]>(() => {
  const value = memory.value.value;
  if (!value) return [];
  const files = (value.files ?? [])
    .filter((file) => typeof file.path === "string")
    .map((file, i) => ({
      key: `f${i}`,
      label: file.kind && KINDS[file.kind] ? t(KINDS[file.kind]) : (file.label ?? file.path!),
      path: file.path!,
      shown: shownPath(props.path, file.path!),
      note: file.exists === false ? t("chatDialog.memory.missing") : "",
      folder: false,
      missing: file.exists === false,
      inside: file.exists === false ? null : within(props.path, file.path!),
    }));
  const folders = (value.folders ?? [])
    .filter((folder) => typeof folder.path === "string")
    .map((folder, i) => ({
      key: `d${i}`,
      label: folder.kind && KINDS[folder.kind] ? t(KINDS[folder.kind]) : (folder.label ?? folder.path!),
      path: folder.path!,
      shown: shownPath(props.path, folder.path!),
      note: "",
      folder: true,
      missing: false,
      inside: null,
    }));
  return [...files, ...folders];
});

const memories = computed(() => (memory.value.value?.memories ?? []).filter((item) => typeof item.name === "string"));

const autoMemory = computed(() => {
  const auto = memory.value.value?.auto_memory;
  if (!auto || typeof auto.enabled !== "boolean") return "";
  return auto.enabled ? t("chatDialog.memory.autoOn") : t("chatDialog.memory.autoOff");
});

const opening = ref<string | null>(null);
const failed = ref<string | null>(null);

async function open(item: Item): Promise<void> {
  if (!item.inside || opening.value) return;
  opening.value = item.key;
  failed.value = null;
  try {
    const found = await api.openReference(props.path, item.inside, 1, null, true);
    if (!found) failed.value = t("chatDialog.memory.notFound", { name: item.shown });
  } catch (reason) {
    failed.value = errorText(reason);
  } finally {
    opening.value = null;
  }
}

const nav = useListNav(
  () => items.value.length,
  (i) => void open(items.value[i]),
  listEl,
);

const rowId = (i: number) => `chat-memory-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.memory.title')" :sub="projectName(path)" :width="500" @close="emit('close')" @key="nav.onKey">
    <div v-if="memory.busy.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="memory.error.value" :id="id" :state="state" :error="memory.error.value" @done="frame?.close()" />

    <template v-else-if="memory.value.value">
      <div class="scroll">
        <div ref="listEl" class="list" role="listbox" tabindex="0" data-focus :aria-label="t('chatDialog.memory.title')" :aria-activedescendant="items.length ? rowId(nav.active.value) : undefined">
          <div
            v-for="(item, i) in items"
            :id="rowId(i)"
            :key="item.key"
            :data-row="i"
            role="option"
            :aria-selected="i === nav.active.value"
            :aria-disabled="!item.inside || undefined"
            :class="['row', { active: i === nav.active.value, still: !item.inside }]"
            :title="item.path"
            @click="
              nav.active.value = i;
              open(item);
            "
          >
            <span class="mark"><Icon :name="item.folder ? 'folder' : 'file'" :size="13" /></span>
            <span class="text">
              <span class="name">{{ item.label }}</span>
              <span class="desc one mono">{{ item.shown }}</span>
            </span>
            <span v-if="item.note" class="tag">{{ item.note }}</span>
            <span v-if="item.inside" class="open">
              <Spinner v-if="opening === item.key" :size="11" />
              <template v-else>
                <Icon name="editor" :size="12" />
                {{ t("chatDialog.memory.open") }}
              </template>
            </span>
          </div>
        </div>

        <template v-if="memories.length">
          <div class="group">
            <span class="section-label">{{ t("chatDialog.memory.memories") }}</span>
            <span class="count">{{ memories.length }}</span>
          </div>
          <div v-for="(item, i) in memories" :key="`m${i}`" class="row still" :title="item.path">
            <span class="text">
              <span class="name mono">{{ item.name }}</span>
              <span v-if="item.description" class="desc one">{{ item.description }}</span>
            </span>
            <span v-if="item.modified_ms" class="meta">{{ when(item.modified_ms) }}</span>
          </div>
        </template>
      </div>

      <p v-if="failed" class="error" role="alert">{{ failed }}</p>
      <p v-if="autoMemory" class="note">{{ autoMemory }}</p>
      <p class="note">{{ t("chatDialog.memory.note") }}</p>
    </template>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.mark {
  color: var(--text-subtle);
}

.open {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11.5px;
  color: var(--text-subtle);
  opacity: 0;
}

.row:hover .open,
.row.active .open {
  opacity: 1;
}
</style>
