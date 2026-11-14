<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t, type Key } from "../../../lib/i18n";
import { useReturnFocus } from "../../../lib/dialogFocus";
import { noteControl, peekChat } from "../../../lib/chat/store";
import type { ChatEffort, ChatModelInfo } from "../../../lib/chat/wire";
import NotAnswered from "./NotAnswered.vue";
import { modelRow, send, useAsk, useChipMenu, useListNav, type DialogProps } from "./shared";

/**
 * The model (B5), as a menu over the composer's model chip (the user's word, 14 Nov: as the VS Code
 * extension draws it, no OK button, a click outside or Esc closes it): the models the CLI lists
 * (its initialize reply, then `list_models`), the one the chat runs checked at the row's end; a
 * click or Return switches to it (`set_model` with its value). Under them what that model takes:
 * its effort on a slider of its levels (`apply_flag_settings {effortLevel}`), and thinking off or
 * on with its default budget where the model can do without (`set_max_thinking_tokens`), and
 * Claude Code's Ultracode at the foot (`apply_flag_settings {ultracode}`). No fast mode (the
 * user's word).
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();
// The host passes every dialog's listeners; a menu teleported to the page takes none of them.
defineOptions({ inheritAttrs: false });

type Settings = { applied?: { model?: string | null; effort?: string | null; ultracode?: unknown }; effective?: { alwaysThinkingEnabled?: unknown; ultracode?: unknown } };
type Listed = { models?: ChatModelInfo[] };
/** A row as the CLI may send it: one it shows but the account can't pick (`disabled`). */
type Row = ChatModelInfo & { disabled?: boolean };

const menu = ref<HTMLElement | null>(null);
const listEl = ref<HTMLElement | null>(null);
useReturnFocus();

const listed = useAsk<Listed>(() => props.id);
const settings = useAsk<Settings>(() => props.id);

/* ---------- the menu: over the model chip (`data-chat-model-chip` in Composer) ---------- */

const WIDTH = 420;
function close(): void {
  emit("close");
}
const place = useChipMenu({
  anchor: () => document.querySelector<HTMLElement>(`[data-chat-model-chip="${props.id}"]`),
  width: WIDTH,
  menu,
  focus: () => listEl.value,
  close,
});

onMounted(() => {
  void listed.load({ kind: "listModels" });
  void settings.load({ kind: "settings" });
});

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

const models = computed<Row[]>(() => {
  const fresh = listed.value.value?.models;
  return Array.isArray(fresh) && fresh.length ? fresh : (props.info?.models ?? []);
});

/** Picked here; until then the one the CLI says it runs. */
const chosen = ref<string | null>(null);
const current = computed(() => chosen.value ?? modelRow(models.value, settings.value.value?.applied?.model ?? props.info?.model)?.value ?? null);
const currentRow = computed(() => models.value.find((row) => row.value === current.value) ?? null);

/** The thing under way: `model:<value>`, `effort:<level>`, `thinking` or `ultracode`. */
const busy = ref<string | null>(null);
const actionError = ref<string | null>(null);

async function pick(row: Row): Promise<void> {
  if (busy.value || row.disabled || row.value === current.value) return;
  busy.value = `model:${row.value}`;
  actionError.value = null;
  const error = await send(props.id, { kind: "setModel", model: row.value });
  busy.value = null;
  if (error) actionError.value = error;
  else chosen.value = row.value;
}

const nav = useListNav(
  () => models.value.length,
  (index) => void pick(models.value[index]),
  listEl,
);

// The keyboard starts on the model `/model <name>` named, else on the one the chat runs.
const startRow = computed(() => {
  const wanted = props.arg?.trim().toLowerCase();
  const named = wanted ? models.value.findIndex((row) => [row.value, row.displayName, row.resolvedModel].some((name) => name?.toLowerCase() === wanted)) : -1;
  return named >= 0 ? named : models.value.findIndex((row) => row.value === current.value);
});
// Once, when the list and the current model are known.
let placed = false;
watch(
  () => models.value.length > 0 && (!settings.busy.value || !!props.arg),
  (ready) => {
    if (!ready || placed) return;
    placed = true;
    nav.move(Math.max(0, startRow.value));
  },
  { immediate: true },
);

/* ---------- effort ---------- */

const EFFORTS: Record<ChatEffort, Key> = {
  auto: "chatDialog.effort.auto",
  low: "chatDialog.effort.low",
  medium: "chatDialog.effort.medium",
  high: "chatDialog.effort.high",
  xhigh: "chatDialog.effort.xhigh",
  max: "chatDialog.effort.max",
};

const effortSet = ref<string | null>(null);
const effort = computed(() => effortSet.value ?? settings.value.value?.applied?.effort ?? null);
// The chip names the effort beside the model (frames never tell of it): what the CLI says, once.
watch(
  () => settings.value.value?.applied?.effort,
  (said) => {
    if (typeof said === "string" && said in EFFORTS && !peekChat(props.id)?.effort) noteControl(props.id, { kind: "setEffort", level: said as ChatEffort });
  },
);
/** The slider's stops: the model's own levels (Auto, the CLI's choice, is where none is set). */
const levels = computed<ChatEffort[]>(() => {
  const row = currentRow.value;
  if (!row?.supportsEffort) return [];
  return (row.supportedEffortLevels ?? []).filter((level): level is ChatEffort => level in EFFORTS && level !== "auto");
});
const stop = computed(() => levels.value.indexOf(effort.value as ChatEffort));
const effortName = computed(() => t(EFFORTS[(effort.value && effort.value in EFFORTS ? effort.value : "auto") as ChatEffort]));

/** ←/→ on the slider: a level down or up. */
function onSliderKey(event: KeyboardEvent): void {
  const step = event.key === "ArrowRight" || event.key === "ArrowUp" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowDown" ? -1 : 0;
  if (!step) return;
  event.preventDefault();
  event.stopPropagation();
  const next = levels.value[Math.max(0, Math.min(levels.value.length - 1, (stop.value < 0 ? (step > 0 ? -1 : levels.value.length) : stop.value) + step))];
  if (next) void setEffort(next);
}

async function setEffort(level: ChatEffort): Promise<void> {
  if (busy.value || level === effort.value) return;
  busy.value = `effort:${level}`;
  actionError.value = null;
  const error = await send(props.id, { kind: "setEffort", level });
  busy.value = null;
  if (error) actionError.value = error;
  else {
    effortSet.value = level;
    noteControl(props.id, { kind: "setEffort", level });
  }
}

/* ---------- thinking ---------- */

const thinkingSet = ref<boolean | null>(null);
const thinking = computed(() => {
  if (thinkingSet.value !== null) return thinkingSet.value;
  const said = settings.value.value?.effective?.alwaysThinkingEnabled;
  return typeof said === "boolean" ? said : true;
});

async function setThinking(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  const on = box.checked;
  box.checked = thinking.value;
  if (busy.value) return;
  busy.value = "thinking";
  actionError.value = null;
  // On is the model's own default budget; off is none.
  const error = await send(props.id, { kind: "setThinking", tokens: on ? null : 0 });
  busy.value = null;
  if (error) actionError.value = error;
  else thinkingSet.value = on;
}

/* ---------- Ultracode ---------- */

/** Claude Code's Ultracode (the user's word, 14 Nov: at the menu's foot): Claude may run a
 * workflow of agents for a task. Whether it is on is what the CLI's settings say, until switched
 * here. */
const ultracodeSet = ref<boolean | null>(null);
const ultracode = computed(() => {
  if (ultracodeSet.value !== null) return ultracodeSet.value;
  const said = settings.value.value?.applied;
  return (said?.ultracode ?? settings.value.value?.effective?.ultracode) === true || said?.effort === "ultracode";
});

async function setUltracode(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  const on = box.checked;
  box.checked = ultracode.value;
  if (busy.value) return;
  busy.value = "ultracode";
  actionError.value = null;
  const error = await send(props.id, { kind: "setUltracode", on });
  busy.value = null;
  if (error) actionError.value = error;
  else ultracodeSet.value = on;
}

const rowId = (index: number) => `chat-model-${props.id}-${index}`;
</script>

<template>
  <Teleport to="body">
    <div ref="menu" class="chip-menu" tabindex="-1" :style="{ width: `${WIDTH}px`, ...place }" role="dialog" :aria-label="t('chatDialog.model.title')" @keydown="onKey">
      <div class="cap">{{ t("chatDialog.model.pick") }}</div>

      <div v-if="!models.length" class="state">
        <NotAnswered v-if="listed.error.value && !listed.busy.value" :id="id" :state="state" :error="listed.error.value" @done="close" />
        <Spinner v-else :size="14" />
      </div>

      <div v-else ref="listEl" class="scroll list" role="listbox" tabindex="0" :aria-label="t('chatDialog.model.title')" :aria-activedescendant="rowId(nav.active.value)">
        <div
          v-for="(row, i) in models"
          :id="rowId(i)"
          :key="row.value"
          :data-row="i"
          role="option"
          :aria-selected="i === nav.active.value"
          :aria-checked="row.value === current"
          :aria-disabled="row.disabled || undefined"
          :class="['row', { active: i === nav.active.value, off: row.disabled }]"
          @mouseenter="nav.active.value = i"
          @click="pick(row)"
        >
          <span class="text">
            <span class="name">{{ row.displayName || row.value }}</span>
            <span v-if="row.description" class="desc one" :title="row.description">{{ row.description }}</span>
          </span>
          <span class="mark end">
            <Spinner v-if="busy === `model:${row.value}`" :size="12" />
            <Icon v-else-if="row.value === current" name="check" :size="13" />
          </span>
        </div>
      </div>

      <div v-if="models.length" class="options">
        <div v-if="currentRow && levels.length" class="option">
          <span id="chat-effort-label" class="option-name">
            {{ t("chatDialog.model.effort") }} <span class="option-value">({{ effortName }})</span>
          </span>
          <Spinner v-if="busy?.startsWith('effort:')" :size="11" />
          <div
            class="slider"
            role="slider"
            tabindex="0"
            aria-labelledby="chat-effort-label"
            :aria-valuemin="1"
            :aria-valuemax="levels.length"
            :aria-valuenow="stop + 1 || undefined"
            :aria-valuetext="effortName"
            @keydown="onSliderKey"
          >
            <span class="fill" :style="{ width: stop < 0 ? '0' : `calc(${((stop + 0.5) / levels.length) * 100}% + 7px)` }"></span>
            <button
              v-for="(level, i) in levels"
              :key="level"
              type="button"
              tabindex="-1"
              :class="['stop', { on: i === stop, past: i < stop }]"
              :title="t(EFFORTS[level])"
              :aria-label="t(EFFORTS[level])"
              :disabled="!!busy"
              @click="setEffort(level)"
            ></button>
          </div>
        </div>

        <label v-if="currentRow?.supportsThinkingOff" class="option">
          <span class="option-name">{{ t("chatDialog.model.thinking") }}</span>
          <Spinner v-if="busy === 'thinking'" :size="11" />
          <input type="checkbox" role="switch" :checked="thinking" :disabled="!!busy" @change="setThinking" />
        </label>

        <label class="option" :title="t('chatDialog.model.ultracodeNote')">
          <span class="option-name">{{ t("chatDialog.model.ultracode") }}</span>
          <Spinner v-if="busy === 'ultracode'" :size="11" />
          <input type="checkbox" role="switch" :checked="ultracode" :disabled="!!busy" @change="setUltracode" />
        </label>
      </div>

      <NotAnswered v-if="actionError" :id="id" :state="state" :error="actionError" @done="actionError = null" />
    </div>
  </Teleport>
</template>

<style scoped src="./dialog.css"></style>
<style scoped src="./menu.css"></style>

<style scoped>
.row {
  min-height: 0;
  padding: 5px 10px;
}

.row .name {
  font-weight: 500;
}

.row .desc {
  font-size: 11.5px;
}

.options {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  margin-top: 4px;
  padding-top: 4px;
  border-top: 1px solid var(--line-soft);
}

.option {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
  padding: 0 10px;
}

.option-name {
  flex: 1;
  min-width: 0;
  font-weight: 500;
}

.option-value {
  font-weight: 400;
  color: var(--text-subtle);
}

/* The effort's stops on a track, filled up to the one set, its knob there. */
.slider {
  position: relative;
  display: flex;
  align-items: center;
  flex-shrink: 0;
  height: 18px;
  padding: 0 2px;
  border-radius: 9px;
  background: var(--bg-control-hover);
  outline: none;
}

.slider:focus-visible {
  box-shadow: 0 0 0 2px var(--focus-ring);
}

.fill {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  max-width: 100%;
  border-radius: 9px;
  background: var(--line-lit);
  transition: width 0.15s ease;
}

.stop {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 18px;
  padding: 0;
  border: 0;
  background: none;
}

.stop::after {
  content: "";
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: var(--text-faint);
  transition:
    width 0.15s ease,
    height 0.15s ease,
    background-color 0.15s ease;
}

.stop.past::after {
  background: var(--text-muted);
}

.stop.on::after {
  width: 14px;
  height: 14px;
  background: var(--text-lit);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);
}

.stop:hover:not(.on)::after {
  background: var(--text);
}

@media (prefers-reduced-motion: reduce) {
  .fill,
  .stop::after {
    transition: none;
  }
}
</style>
