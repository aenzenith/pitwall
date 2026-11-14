<script setup lang="ts">
// Settings › Terminal › Sohbet (Settings › Claude's until 2026-11-15). Its own: each change goes
// to the core by its own command
// (`set_chat_settings`, a patch of the fields changed), never with the form (`@change.stop`).
//
// Every value is the snapshot's (`settings.chat` over CHAT_DEFAULTS, lib/chat/open), shown at once
// as clicked until the core's answer comes back; one the core refuses goes back to what is stored.
// "Claude açılınca" (`open`) is in Settings › Terminal since 2026-11-14.
import { computed, ref, watch } from "vue";

import { chatSettings } from "../../lib/chat/open";
import type { ChatSettings } from "../../lib/chat/wire";
import { t, type Key } from "../../lib/i18n";
import { keys } from "../../lib/platform";
import { api, snapshot } from "../../lib/store";

type Switch = Exclude<keyof ChatSettings, "open" | "sendKey" | "mode">;

/** The switches, each with what it does when the name alone doesn't say it. */
const SWITCHES: Array<{ id: Switch; name: Key; hint?: Key }> = [
  { id: "suggestions", name: "chatSurface.settings.suggestions", hint: "chatSurface.settings.suggestionsHint" },
  { id: "scrollOnSend", name: "chatSurface.settings.scrollOnSend" },
  { id: "timestamps", name: "chatSurface.settings.timestamps", hint: "chatSurface.settings.timestampsHint" },
  { id: "focusView", name: "chatSurface.settings.focusView", hint: "chatSurface.settings.focusViewHint" },
  { id: "continueInterrupted", name: "chatSurface.settings.continueInterrupted", hint: "chatSurface.settings.continueInterruptedHint" },
  { id: "attachFile", name: "chatSurface.settings.attachFile", hint: "chatSurface.settings.attachFileHint" },
  { id: "attachSelection", name: "chatSurface.settings.attachSelection", hint: "chatSurface.settings.attachSelectionHint" },
];

/** The mode a new chat starts in, named as the chat's mode dialog names it. */
const MODES: Array<{ id: ChatSettings["mode"]; name: Key }> = [
  { id: "default", name: "chatDialog.mode.default" },
  { id: "auto", name: "chatDialog.mode.auto" },
];

/** What was clicked and not yet back in a snapshot. */
const picked = ref<Partial<ChatSettings>>({});
const error = ref("");

// A new snapshot of them holds what was sent (or what the core kept instead).
watch(
  () => JSON.stringify(snapshot.value?.settings.chat ?? null),
  () => (picked.value = {}),
);

const shown = computed<ChatSettings>(() => ({ ...chatSettings.value, ...picked.value }));

async function set(patch: Partial<ChatSettings>): Promise<void> {
  error.value = "";
  picked.value = { ...picked.value, ...patch };
  try {
    await api.setChatSettings(patch);
  } catch (reason) {
    const back = { ...picked.value };
    for (const key of Object.keys(patch) as Array<keyof ChatSettings>) delete back[key];
    picked.value = back;
    error.value = String(reason);
  }
}

function flip(id: Switch, event: Event): void {
  void set({ [id]: (event.target as HTMLInputElement).checked });
}

/** A click on a segment: saved here, never twice for the one already picked. */
function pick(value: ChatSettings["sendKey"]): void {
  if (shown.value.sendKey !== value) void set({ sendKey: value });
}

function pickMode(value: ChatSettings["mode"]): void {
  if (shown.value.mode !== value) void set({ mode: value });
}

/** Return or ⌘Return, as this keyboard writes them. */
const SEND_KEYS: Array<{ id: ChatSettings["sendKey"]; label: string }> = [
  { id: "enter", label: keys("Enter") },
  { id: "mod-enter", label: keys("mod+Enter") },
];
const sendHint = computed(() =>
  shown.value.sendKey === "mod-enter"
    ? t("chatSurface.settings.sendKeyMod", { send: keys("mod+Enter"), line: keys("Enter") })
    : t("chatSurface.settings.sendKeyEnter", { send: keys("Enter"), line: keys("shift+Enter") }),
);

/** The selection goes along with the file only. */
const disabled = (id: Switch): boolean => id === "attachSelection" && !shown.value.attachFile;
</script>

<template>
  <div class="section part chat-part" @change.stop>
    <div id="chat-part" class="section-label">{{ t("chatSurface.settings.title") }}</div>
    <div class="rows" role="group" aria-labelledby="chat-part">
      <div class="row boxed">
        <span class="name">
          <span id="chat-send">{{ t("chatSurface.settings.sendKey") }}</span>
          <small>{{ sendHint }}</small>
        </span>
        <span class="seg keys" role="group" aria-labelledby="chat-send">
          <button v-for="choice in SEND_KEYS" :key="choice.id" type="button" :aria-pressed="shown.sendKey === choice.id" @click="pick(choice.id)">{{ choice.label }}</button>
        </span>
      </div>
      <div class="row boxed">
        <span class="name">
          <span id="chat-mode">{{ t("chatSurface.settings.mode") }}</span>
          <small>{{ t("chatSurface.settings.modeHint") }}</small>
        </span>
        <span class="seg" role="group" aria-labelledby="chat-mode">
          <button v-for="choice in MODES" :key="choice.id" type="button" :aria-pressed="shown.mode === choice.id" @click="pickMode(choice.id)">{{ t(choice.name) }}</button>
        </span>
      </div>
      <!-- The whole row is the switch's. -->
      <label v-for="each in SWITCHES" :key="each.id" :class="['row', { off: disabled(each.id) }]">
        <span class="name">
          <span>{{ t(each.name) }}</span>
          <small v-if="each.hint">{{ t(each.hint) }}</small>
        </span>
        <input type="checkbox" role="switch" :checked="shown[each.id]" :disabled="disabled(each.id)" @change="flip(each.id, $event)" />
      </label>
    </div>
    <small class="later">{{ t("chatSurface.settings.later") }}</small>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.chat-part {
  font-size: 13px;
}

/* A name and, under it, what to know of it. */
.name {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.name small {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-subtle);
}

/* A line that ends in a boxed control is as tall as the lines that end in a switch. */
.row.boxed {
  padding-top: 6px;
  padding-bottom: 6px;
}

/* The keys, in the font they are written in elsewhere (Settings' shortcuts). */
.seg.keys button {
  min-width: 44px;
  font-family: var(--font-mono);
  font-size: 12.5px;
}

:root:not([data-platform="mac"]) .seg.keys button {
  font-family: var(--font);
  font-size: 12px;
}

.row.off .name {
  color: var(--text-subtle);
}

.later {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}
</style>
