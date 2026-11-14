<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";

import Highlight from "../../../components/Highlight.vue";
import Spinner from "../../../components/Spinner.vue";
import { fuzzy } from "../../../lib/fuzzy";
import { t, type Key } from "../../../lib/i18n";
import { keys } from "../../../lib/platform";
import { snapshot } from "../../../lib/store";
import type { ChatCommand } from "../../../lib/chat/wire";
import DialogFrame from "./DialogFrame.vue";
import { projectName, useListNav, type DialogProps } from "./shared";

/**
 * Help: every command the CLI lists for the session (`initialize.commands`, kept fresh by
 * `commands_changed`), searchable; a click or Return puts `/name ` into the composer. Beside them,
 * the composer's keys. Claude Code's terminal `/help` isn't asked: it answers nothing headless.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: []; insert: [text: string] }>();

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);
const searchEl = ref<HTMLInputElement | null>(null);

const tab = ref<"commands" | "keys">("commands");
const query = ref((props.arg ?? "").trim().replace(/^\//, ""));

const commands = computed<ChatCommand[]>(() => (props.info?.commands ?? []).filter((command) => typeof command.name === "string" && command.name));

const shown = computed(() => {
  const q = query.value.trim().replace(/^\//, "");
  if (!q) return commands.value.map((command) => ({ command, hits: null as Set<number> | null }));
  const lower = q.toLowerCase();
  return commands.value
    .map((command) => {
      const match = fuzzy(command.name, q) ?? null;
      const alias = (command.aliases ?? []).some((name) => fuzzy(name, q));
      const inText = (command.description ?? "").toLowerCase().includes(lower);
      return { command, match, keep: !!match || alias || inText };
    })
    .filter((item) => item.keep)
    .sort((a, b) => (b.match?.score ?? -100) - (a.match?.score ?? -100))
    .map((item) => ({ command: item.command, hits: item.match ? new Set(item.match.hits) : null }));
});

function use(command: ChatCommand): void {
  emit("insert", `/${command.name} `);
  frame.value?.close();
}

const nav = useListNav(
  () => (tab.value === "commands" ? shown.value.length : 0),
  (i) => use(shown.value[i].command),
  listEl,
);
watch(query, () => nav.move(0));
watch(tab, (now) => {
  if (now === "commands") void nextTick(() => searchEl.value?.focus());
});

/** The composer's keys, as this platform writes them; Return's as Settings › Claude › Sohbet sets it. */
const sendsWithMod = computed(() => snapshot.value?.settings.chat?.sendKey === "mod-enter");
const KEYS = computed<{ keys: string; say: Key }[]>(() => [
  sendsWithMod.value ? { keys: keys("mod+Enter"), say: "chatDialog.help.key.send" } : { keys: keys("Enter"), say: "chatDialog.help.key.send" },
  sendsWithMod.value ? { keys: keys("Enter"), say: "chatDialog.help.key.newLine" } : { keys: keys("Shift+Enter"), say: "chatDialog.help.key.newLine" },
  { keys: keys("Alt+Enter"), say: "chatDialog.help.key.now" },
  { keys: keys("Escape"), say: "chatDialog.help.key.stop" },
  { keys: `${keys("Escape")} ${keys("Escape")}`, say: "chatDialog.help.key.rewind" },
  { keys: `${keys("ArrowUp")} ${keys("ArrowDown")}`, say: "chatDialog.help.key.history" },
  { keys: keys("Shift+Tab"), say: "chatDialog.help.key.mode" },
  { keys: keys("Tab"), say: "chatDialog.help.key.suggestion" },
  { keys: "/", say: "chatDialog.help.key.slash" },
  { keys: "@", say: "chatDialog.help.key.mention" },
  { keys: "!", say: "chatDialog.help.key.shell" },
  { keys: keys("Ctrl+KeyS"), say: "chatDialog.help.key.stash" },
]);

const rowId = (i: number) => `chat-help-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.help.title')" :sub="projectName(path)" :width="540" @close="emit('close')" @key="nav.onKey">
    <template #tools>
      <div class="seg" role="tablist" :aria-label="t('chatDialog.help.title')">
        <button type="button" role="tab" :aria-selected="tab === 'commands'" :aria-pressed="tab === 'commands'" @click="tab = 'commands'">{{ t("chatDialog.help.commands") }}</button>
        <button type="button" role="tab" :aria-selected="tab === 'keys'" :aria-pressed="tab === 'keys'" @click="tab = 'keys'">{{ t("chatDialog.help.keys") }}</button>
      </div>
    </template>

    <template v-if="tab === 'commands'">
      <input
        ref="searchEl"
        v-model="query"
        class="search"
        type="search"
        data-focus
        spellcheck="false"
        autocomplete="off"
        :placeholder="t('chatDialog.help.search')"
        :aria-label="t('chatDialog.help.search')"
        role="combobox"
        aria-autocomplete="list"
        :aria-expanded="true"
        :aria-controls="`chat-help-list-${id}`"
        :aria-activedescendant="shown.length ? rowId(nav.active.value) : undefined"
      />

      <div v-if="!info" class="state"><Spinner :size="14" /></div>
      <p v-else-if="!shown.length" class="state">{{ query.trim() ? t("chatDialog.noMatch", { query: query.trim() }) : t("chatDialog.help.empty") }}</p>
      <div v-else :id="`chat-help-list-${id}`" ref="listEl" class="scroll list" role="listbox" :aria-label="t('chatDialog.help.commands')">
        <div
          v-for="(item, i) in shown"
          :id="rowId(i)"
          :key="`${item.command.name}-${i}`"
          :data-row="i"
          role="option"
          :aria-selected="i === nav.active.value"
          :class="['row', { active: i === nav.active.value }]"
          @click="
            nav.active.value = i;
            use(item.command);
          "
        >
          <span class="command mono">
            /<Highlight v-if="item.hits" :text="item.command.name" :hits="item.hits" /><template v-else>{{ item.command.name }}</template>
            <span v-if="item.command.argumentHint" class="hint">{{ item.command.argumentHint }}</span>
          </span>
          <span class="desc one" :title="item.command.description">{{ item.command.description }}</span>
        </div>
      </div>
      <p class="note">{{ t("chatDialog.help.note") }}</p>
    </template>

    <div v-else class="scroll">
      <dl class="keys">
        <div v-for="row in KEYS" :key="row.say" class="key-row">
          <dt>
            <kbd>{{ row.keys }}</kbd>
          </dt>
          <dd>{{ t(row.say) }}</dd>
        </div>
      </dl>
    </div>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list {
  max-height: 360px;
}

/* A command and its description on one line, their letters on one baseline. */
.list .row {
  align-items: baseline;
  padding-top: 7px;
}

.command {
  flex-shrink: 0;
  max-width: 55%;
  font-size: 12.5px;
  color: var(--text-strong);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.hint {
  margin-left: 6px;
  color: var(--text-faint);
}

.desc.one {
  flex: 1 1 auto;
}

.keys {
  margin: 0;
  display: flex;
  flex-direction: column;
}

.key-row {
  display: grid;
  grid-template-columns: 110px 1fr;
  align-items: baseline;
  gap: 12px;
  padding: 6px 0;
  border-bottom: 1px solid var(--line-soft);
}

.key-row:last-child {
  border-bottom: 0;
}

dt {
  margin: 0;
}

dd {
  margin: 0;
  font-size: 12.5px;
  line-height: 1.45;
  color: var(--text-soft);
}

kbd {
  padding: 1px 6px;
  border-radius: 5px;
  background: var(--bg-key);
  box-shadow:
    inset 0 -1px 0 var(--line-strong),
    0 0 0 1px var(--line-soft);
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text);
  white-space: nowrap;
}
</style>
