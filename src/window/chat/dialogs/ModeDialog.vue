<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { useReturnFocus } from "../../../lib/dialogFocus";
import { t, type Key } from "../../../lib/i18n";
import { modelOf } from "../../../lib/chat/commands";
import { peekChat } from "../../../lib/chat/store";
import { MODE_ICONS } from "../../../lib/chat/ui";
import type { ChatPermissionMode } from "../../../lib/chat/wire";
import ChatIcon from "../ChatIcon.vue";
import NotAnswered from "./NotAnswered.vue";
import { send, useChipMenu, useListNav, type DialogProps } from "./shared";

/**
 * The permission mode (B5), as a menu over the composer's mode chip, as the model's opens (the
 * user's word, 15 Nov: no dialog): the ones a chat can be in, each with a sentence of what it
 * does, the current one checked; a pick switches to it (`set_permission_mode`) and closes. Auto
 * is always listed, greyed with why when the chat's model is known not to have it (before a turn
 * names the model, the CLI's default one stands for it). Never bypass (O4): it isn't offered, and
 * the core refuses it.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();
// The host passes every dialog's listeners; a menu teleported to the page takes none of them.
defineOptions({ inheritAttrs: false });

type Mode = { id: ChatPermissionMode; name: Key; note: Key };
const ALL: Mode[] = [
  { id: "default", name: "chatDialog.mode.default", note: "chatDialog.mode.defaultNote" },
  { id: "acceptEdits", name: "chatDialog.mode.acceptEdits", note: "chatDialog.mode.acceptEditsNote" },
  { id: "plan", name: "chatDialog.mode.plan", note: "chatDialog.mode.planNote" },
  { id: "auto", name: "chatDialog.mode.auto", note: "chatDialog.mode.autoNote" },
  { id: "dontAsk", name: "chatDialog.mode.dontAsk", note: "chatDialog.mode.dontAskNote" },
];

const menu = ref<HTMLElement | null>(null);
const listEl = ref<HTMLElement | null>(null);
useReturnFocus();

const WIDTH = 380;
function close(): void {
  emit("close");
}
const place = useChipMenu({
  anchor: () => document.querySelector<HTMLElement>(`[data-chat-mode-chip="${props.id}"]`),
  width: WIDTH,
  menu,
  focus: () => listEl.value,
  close,
});

const modes = ALL;

/** The chat's model as it runs now (a switch made here, else the last turn's, else the CLI's
 * default): auto is off only where its row says so. */
const autoOff = computed(() => modelOf(props.info, peekChat(props.id)?.model ?? props.info?.model ?? null)?.supportsAutoMode === false);
const off = (mode: Mode): boolean => mode.id === "auto" && autoOff.value;

/** `manual` is the CLI's other name for the default. */
const current = computed(() => {
  const mode = props.info?.permissionMode;
  return mode === "manual" ? "default" : (mode ?? null);
});

const busy = ref<ChatPermissionMode | null>(null);
const error = ref<string | null>(null);

async function pick(mode: Mode): Promise<void> {
  if (busy.value || off(mode)) return;
  if (mode.id === current.value) return close();
  busy.value = mode.id;
  error.value = null;
  error.value = await send(props.id, { kind: "setMode", mode: mode.id });
  busy.value = null;
  if (!error.value) close();
}

const nav = useListNav(
  () => modes.length,
  (index) => void pick(modes[index]),
  listEl,
);

// The keyboard starts on the mode `/permissions <mode>`… named, else on the current one.
const wanted = props.arg?.trim().toLowerCase();
const start = modes.findIndex((mode) => (wanted ? mode.id.toLowerCase() === wanted : mode.id === current.value));
nav.active.value = Math.max(0, start);

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    return close();
  }
  if (nav.onKey(event)) {
    event.preventDefault();
    event.stopPropagation();
  }
}

const rowId = (index: number) => `chat-mode-${props.id}-${index}`;
</script>

<template>
  <Teleport to="body">
    <div ref="menu" class="chip-menu" tabindex="-1" :style="{ width: `${WIDTH}px`, ...place }" role="dialog" :aria-label="t('chatDialog.mode.title')" @keydown="onKey">
    <div class="cap">{{ t("chatDialog.mode.title") }}</div>
    <div ref="listEl" class="scroll list" role="listbox" tabindex="0" :aria-label="t('chatDialog.mode.title')" :aria-activedescendant="rowId(nav.active.value)">
      <div
        v-for="(mode, i) in modes"
        :id="rowId(i)"
        :key="mode.id"
        :data-row="i"
        role="option"
        :aria-selected="i === nav.active.value"
        :aria-checked="mode.id === current"
        :aria-disabled="off(mode) || undefined"
        :class="['row', 'tall', { active: i === nav.active.value, off: off(mode) }]"
        @click="
          nav.active.value = i;
          pick(mode);
        "
      >
        <span class="mark lead"><ChatIcon :name="MODE_ICONS[mode.id]" :size="14" /></span>
        <span class="text">
          <span class="name">{{ t(mode.name) }}</span>
          <span class="desc">{{ t(off(mode) ? "chatDialog.mode.autoOff" : mode.note) }}</span>
        </span>
        <span class="mark end">
          <Spinner v-if="busy === mode.id" :size="12" />
          <Icon v-else-if="mode.id === current" name="check" :size="13" />
        </span>
      </div>
    </div>

    <NotAnswered v-if="error" :id="id" :state="state" :error="error" @done="close" />
    </div>
  </Teleport>
</template>

<style scoped src="./dialog.css"></style>
<style scoped src="./menu.css"></style>

<style scoped>
.row.tall {
  align-items: flex-start;
  min-height: 0;
  padding: 6px 10px;
}

.row .name {
  font-weight: 500;
}

.row .desc {
  font-size: 11.5px;
}

/* Each mode's icon, the composer's chip's own, level with its name. */
.row.tall .mark.lead {
  margin-top: 2px;
  color: var(--text-nav);
}

/* The check at the right stands level with the whole row. */
.row.tall .mark.end {
  align-self: center;
}
</style>
