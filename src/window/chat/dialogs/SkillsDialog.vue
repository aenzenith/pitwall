<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Highlight from "../../../components/Highlight.vue";
import Spinner from "../../../components/Spinner.vue";
import { fuzzy } from "../../../lib/fuzzy";
import { t, type Key } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { count, projectName, send, useAsk, useListNav, type DialogProps } from "./shared";

/**
 * Skills (B9): the skills of the session as Claude Code's `/skills` menu lists them
 * (`get_skills_dialog`): each with its description, where it comes from, its state and what it
 * costs the context; Reload reads them again (`reload_skills`). One that runs as a command is
 * put into the composer on a click, as the help's commands are. Where the CLI lists no rows, the
 * skills its last turn named (`system/init`) stand in, with what the command list says of them.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: []; insert: [text: string] }>();

type Skill = { name?: string; display_name?: string; description?: string; source?: string; tokens?: number; state?: string; advertised?: boolean };
type Dialog = { skills?: Skill[] };
type Row = { key: string; name: string; description: string; source: string; tokens: number | null; state: string; command: boolean };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);
const skills = useAsk<Dialog>(() => props.id);
onMounted(() => void skills.load({ kind: "skills" }));

const SOURCES: Record<string, Key> = {
  user: "chatDialog.skills.source.user",
  project: "chatDialog.skills.source.project",
  plugin: "chatDialog.skills.source.plugin",
  "built-in": "chatDialog.skills.source.builtin",
  bundled: "chatDialog.skills.source.builtin",
  managed: "chatDialog.skills.source.managed",
  policy: "chatDialog.skills.source.managed",
  mcp: "chatDialog.skills.source.mcp",
};
const STATES: Record<string, Key> = {
  "name-only": "chatDialog.skills.state.nameOnly",
  "user-invocable-only": "chatDialog.skills.state.userOnly",
  off: "chatDialog.skills.state.off",
};

const rows = computed<Row[]>(() => {
  const listed = (skills.value.value?.skills ?? []).filter((skill) => typeof skill.name === "string");
  if (listed.length) {
    return listed.map((skill, i) => ({
      key: `${skill.name}-${i}`,
      name: skill.display_name || skill.name!,
      description: skill.description ?? "",
      source: skill.source ? (SOURCES[skill.source] ? t(SOURCES[skill.source]) : skill.source) : "",
      tokens: typeof skill.tokens === "number" ? skill.tokens : null,
      state: skill.state && STATES[skill.state] ? t(STATES[skill.state]) : "",
      command: skill.advertised !== false && skill.state !== "off",
    }));
  }
  const commands = props.info?.commands ?? [];
  return (props.info?.skills ?? []).map((name, i) => ({
    key: `${name}-${i}`,
    name,
    description: commands.find((command) => command.name === name)?.description ?? "",
    source: "",
    tokens: null,
    state: "",
    command: commands.some((command) => command.name === name),
  }));
});

const query = ref(props.arg?.trim() ?? "");
const shown = computed(() => {
  const q = query.value.trim();
  if (!q) return rows.value.map((row) => ({ row, hits: null as Set<number> | null }));
  return rows.value
    .map((row) => ({ row, match: fuzzy(row.name, q), inText: row.description.toLowerCase().includes(q.toLowerCase()) }))
    .filter((item) => item.match || item.inText)
    .sort((a, b) => (b.match?.score ?? -100) - (a.match?.score ?? -100))
    .map((item) => ({ row: item.row, hits: item.match ? new Set(item.match.hits) : null }));
});

function use(row: Row): void {
  if (!row.command) return;
  emit("insert", `/${row.name} `);
  frame.value?.close();
}

const nav = useListNav(
  () => shown.value.length,
  (i) => use(shown.value[i].row),
  listEl,
);
watch(query, () => nav.move(0));

const reloading = ref(false);
const reloadError = ref<string | null>(null);
async function reload(): Promise<void> {
  if (reloading.value) return;
  reloading.value = true;
  reloadError.value = await send(props.id, { kind: "reloadSkills" });
  if (!reloadError.value) await skills.load({ kind: "skills" });
  reloading.value = false;
}

const rowId = (i: number) => `chat-skill-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.skills.title')" :sub="projectName(path)" :width="520" @close="emit('close')" @key="nav.onKey">
    <input
      v-model="query"
      class="search"
      type="search"
      data-focus
      spellcheck="false"
      autocomplete="off"
      :placeholder="t('chatDialog.skills.search')"
      :aria-label="t('chatDialog.skills.search')"
      role="combobox"
      aria-autocomplete="list"
      :aria-expanded="true"
      :aria-controls="`chat-skills-${id}`"
      :aria-activedescendant="shown.length ? rowId(nav.active.value) : undefined"
    />

    <div v-if="skills.busy.value && !skills.value.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="skills.error.value && !rows.length" :id="id" :state="state" :error="skills.error.value" @done="frame?.close()" />
    <p v-else-if="!rows.length" class="state">{{ t("chatDialog.skills.empty") }}</p>
    <p v-else-if="!shown.length" class="state">{{ t("chatDialog.noMatch", { query: query.trim() }) }}</p>

    <div v-else :id="`chat-skills-${id}`" ref="listEl" class="scroll list" role="listbox" :aria-label="t('chatDialog.skills.title')">
      <div
        v-for="(item, i) in shown"
        :id="rowId(i)"
        :key="item.row.key"
        :data-row="i"
        role="option"
        :aria-selected="i === nav.active.value"
        :class="['row', 'tall', { active: i === nav.active.value, still: !item.row.command }]"
        @click="
          nav.active.value = i;
          use(item.row);
        "
      >
        <span class="text">
          <span class="line">
            <span class="name mono"
              ><Highlight v-if="item.hits" :text="item.row.name" :hits="item.hits" /><template v-else>{{ item.row.name }}</template></span
            >
            <span v-if="item.row.state" class="tag">{{ item.row.state }}</span>
          </span>
          <span v-if="item.row.description" class="desc two" :title="item.row.description">{{ item.row.description }}</span>
        </span>
        <span class="side">
          <span v-if="item.row.source" class="tag">{{ item.row.source }}</span>
          <span v-if="item.row.tokens !== null" class="meta">{{ t("chatDialog.tokens", { count: item.row.tokens, tokens: count(item.row.tokens) }) }}</span>
        </span>
      </div>
    </div>

    <NotAnswered v-if="reloadError" :id="id" :state="state" :error="reloadError" @done="frame?.close()" />

    <template #foot>
      <span class="grow note">{{ t("chatDialog.skills.note") }}</span>
      <button type="button" class="control" :disabled="reloading" :aria-busy="reloading" @click="reload">
        <span :class="{ hidden: reloading }">{{ t("chatDialog.skills.reload") }}</span>
        <Spinner v-if="reloading" class="over" :size="12" />
      </button>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list {
  max-height: 380px;
}

.row.tall {
  align-items: flex-start;
  padding-top: 6px;
  padding-bottom: 6px;
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.name {
  font-size: 12.5px;
}

.side {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 3px;
}

.grow.note {
  font-size: 11.5px;
}
</style>
