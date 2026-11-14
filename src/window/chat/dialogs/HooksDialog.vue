<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { projectName, reveal, sourceName, useAsk, useListNav, type DialogProps } from "./shared";

/**
 * Hooks (B9): the hooks the session runs, as `/hooks` lists them (`get_hooks_listing`), by event,
 * each with its matcher, where it is set and, opened, what it runs. Read only: hooks change in
 * Claude Code's settings files.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Hook = {
  event?: string;
  matcher?: string;
  source?: string;
  sourceLabel?: string;
  pluginName?: string;
  type?: string;
  displayText?: string;
  commandText?: string;
  contentLabel?: string;
  condition?: string;
  timeout?: number;
  runsOnce?: boolean;
  runsInBackground?: boolean;
  disabled?: boolean;
};
type Listing = {
  events?: { name?: string; summary?: string }[];
  hooks?: Hook[];
  eventCatalog?: { name?: string; summary?: string }[];
  policy?: { disabledByPolicy?: boolean; managedOnly?: boolean; allDisabled?: boolean; pluginOnly?: boolean };
};

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);
const listing = useAsk<Listing>(() => props.id);
onMounted(() => void listing.load({ kind: "hooks" }));

/** The events with hooks, in the menu's order, each with its hooks and the CLI's summary. */
const groups = computed(() => {
  const value = listing.value.value;
  const hooks = (value?.hooks ?? []).filter((hook) => typeof hook.event === "string");
  const order = (value?.events ?? []).map((event) => event.name).filter((name): name is string => !!name);
  for (const hook of hooks) if (!order.includes(hook.event!)) order.push(hook.event!);
  const summary = (name: string) => value?.events?.find((event) => event.name === name)?.summary ?? value?.eventCatalog?.find((event) => event.name === name)?.summary ?? "";
  return order.map((name) => ({ name, summary: summary(name), hooks: hooks.filter((hook) => hook.event === name) })).filter((group) => group.hooks.length);
});

/** Every hook in the order shown, for the keys. */
const flat = computed(() => groups.value.flatMap((group) => group.hooks));
const index = (hook: Hook) => flat.value.indexOf(hook);

/** The hook whose details show, by its place in `flat`. */
const opened = ref<number | null>(null);
const toggle = (hook: Hook) => (opened.value = opened.value === index(hook) ? null : index(hook));

const nav = useListNav(
  () => flat.value.length,
  (i) => toggle(flat.value[i]),
  listEl,
);

const policy = computed(() => {
  const value = listing.value.value?.policy;
  if (value?.disabledByPolicy) return t("chatDialog.hooks.policyOff");
  if (value?.allDisabled) return t("chatDialog.hooks.allOff");
  if (value?.managedOnly) return t("chatDialog.hooks.managedOnly");
  return "";
});

function where(hook: Hook): string {
  const from = hook.source === "pluginHook" && hook.pluginName ? hook.pluginName : sourceName(hook.source, hook.sourceLabel);
  return [from, hook.type].filter(Boolean).join(" · ");
}

const rowId = (i: number) => `chat-hook-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.hooks.title')" :sub="projectName(path)" :width="520" @close="emit('close')" @key="nav.onKey">
    <div v-if="listing.busy.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="listing.error.value" :id="id" :state="state" :error="listing.error.value" @done="frame?.close()" />

    <template v-else-if="listing.value.value">
      <p v-if="policy" class="note strong">{{ policy }}</p>
      <p v-if="!groups.length" class="state">{{ t("chatDialog.hooks.empty") }}</p>

      <div v-else ref="listEl" class="scroll list" role="listbox" tabindex="0" data-focus :aria-label="t('chatDialog.hooks.title')" :aria-activedescendant="rowId(nav.active.value)">
        <template v-for="group in groups" :key="group.name">
          <div class="group">
            <span class="event mono">{{ group.name }}</span>
            <span v-if="group.summary" class="summary">{{ group.summary }}</span>
          </div>
          <template v-for="hook in group.hooks" :key="index(hook)">
            <div
              :id="rowId(index(hook))"
              :data-row="index(hook)"
              role="option"
              :aria-selected="index(hook) === nav.active.value"
              :aria-expanded="opened === index(hook)"
              :class="['row', { active: index(hook) === nav.active.value, dim: hook.disabled }]"
              @click="
                nav.active.value = index(hook);
                toggle(hook);
              "
            >
              <span v-if="hook.matcher" class="tag mono" :title="reveal(hook.matcher)">{{ reveal(hook.matcher) }}</span>
              <span class="text">
                <span class="name">{{ reveal(hook.displayText || hook.commandText || "") }}</span>
                <span class="desc one">{{ where(hook) }}</span>
              </span>
              <span v-if="hook.disabled" class="tag">{{ t("chatDialog.hooks.wontRun") }}</span>
              <Icon name="chevron-down" :size="13" :class="['chevron', { open: opened === index(hook) }]" />
            </div>
            <div v-if="opened === index(hook)" class="details">
              <span class="label">{{ hook.contentLabel || t("chatDialog.hooks.runs") }}</span>
              <pre class="block selectable">{{ reveal(hook.commandText ?? "") }}</pre>
              <span v-if="hook.condition" class="facts">{{ t("chatDialog.hooks.condition", { condition: reveal(hook.condition) }) }}</span>
              <div class="facts">
                <span v-if="typeof hook.timeout === 'number'">{{ t("chatDialog.hooks.timeout", { count: hook.timeout }) }}</span>
                <span v-if="hook.runsInBackground">{{ t("chatDialog.hooks.background") }}</span>
                <span v-if="hook.runsOnce">{{ t("chatDialog.hooks.once") }}</span>
              </div>
            </div>
          </template>
        </template>
      </div>

      <p class="note">{{ t("chatDialog.hooks.note") }}</p>
    </template>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list {
  max-height: 380px;
}

.note.strong {
  color: var(--warn-text);
}

.group {
  align-items: baseline;
}

.event {
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-strong);
}

.summary {
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row.dim .text {
  opacity: 0.6;
}

.row .name {
  font-weight: 400;
}

.chevron {
  flex-shrink: 0;
  color: var(--text-faint);
  transition: transform 0.15s;
}

.chevron.open {
  transform: rotate(180deg);
}

.details {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 2px 0 6px 10px;
}

.label {
  font-size: 11.5px;
  color: var(--text-subtle);
}

.facts {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 11.5px;
  color: var(--text-subtle);
}

@media (prefers-reduced-motion: reduce) {
  .chevron {
    transition: none;
  }
}
</style>
