<script setup lang="ts">
// Settings › Terminal › Sohbet (Settings › Claude's until 2026-11-15). Its own: each change goes
// to the core by its own command
// (`set_chat_settings`, a patch of the fields changed), never with the form (`@change.stop`).
//
// Every value is the snapshot's (`settings.chat` over CHAT_DEFAULTS, lib/chat/open), shown at once
// as clicked until the core's answer comes back; one the core refuses goes back to what is stored.
// "Claude açılınca" (`open`) is in Settings › Terminal since 2026-11-14.
import { computed, onMounted, ref, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import { chatSettings } from "../../lib/chat/open";
import { cancelDownload, downloadModel, megabytes, modelBytes, refreshVoice, useVoice, voice, type DownloadProblem, type VoiceModel } from "../../lib/chat/voice";
import { VOICE_DELAYS, voiceDelayOf, type ChatSettings } from "../../lib/chat/wire";
import { language, LANGUAGES, languageName, t, type Key } from "../../lib/i18n";
import { keys } from "../../lib/platform";
import { api, snapshot } from "../../lib/store";

type Switch = Exclude<keyof ChatSettings, "open" | "sendKey" | "mode" | "voiceModel" | "voiceSend" | "voiceDelay" | "voiceLanguage">;

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

/* Voice messages (Sesli mesaj; lib/chat/voice): the model, whether the text goes by itself, the
   language spoken, and the models on this machine. */

useVoice();
onMounted(() => void refreshVoice());

const VOICE_MODELS: Array<{ id: VoiceModel; name: Key }> = [
  { id: "small", name: "chatSurface.settings.voice.small" },
  { id: "large", name: "chatSurface.settings.voice.large" },
];
const DOWNLOAD_PROBLEMS: Record<DownloadProblem, Key> = {
  network: "chatInput.voice.download.network",
  http: "chatInput.voice.download.http",
  size: "chatInput.voice.download.size",
  hash: "chatInput.voice.download.size",
  write: "chatInput.voice.download.write",
};
/** whisper's code for each of the app's languages. */
const VOICE_LANGUAGES = LANGUAGES.map((each) => ({ id: each.code === "zh-Hans" ? "zh" : each.code, name: languageName(each.code) }));
const appLanguage = computed(() => t("chatSurface.settings.voice.languageApp", { language: languageName(language.value) }));
const size = (model: VoiceModel): string => megabytes(modelBytes(model), language.value);
const supported = computed(() => voice.status?.supported !== false);
/** The models on this machine, and the one picked even when it isn't. */
const voiceRows = computed(() =>
  VOICE_MODELS.filter((each) => each.id === shown.value.voiceModel || voice.status?.models.some((m) => m.model === each.id && m.ready)).map((each) => {
    const ready = !!voice.status?.models.find((m) => m.model === each.id)?.ready;
    const fetching = voice.download?.model === each.id ? voice.download : null;
    const problem = voice.downloadFailed?.model === each.id ? voice.downloadFailed.problem : null;
    return { ...each, ready, fetching, problem };
  }),
);
const deleting = ref<VoiceModel | null>(null);

function pickVoiceModel(value: VoiceModel): void {
  if (shown.value.voiceModel !== value) void set({ voiceModel: value });
}

/** The wait before a take's text goes: none, or so many seconds in the window's own numbers. */
const delayName = (ms: number): string =>
  ms ? t("chatSurface.settings.voice.seconds", { n: new Intl.NumberFormat(language.value, { maximumFractionDigits: 1 }).format(ms / 1000) }) : t("chatSurface.settings.voice.delayNow");
const shownDelay = computed(() => voiceDelayOf(shown.value.voiceDelay));

function pickVoiceDelay(value: number): void {
  if (shownDelay.value !== value) void set({ voiceDelay: value });
}

function pickVoiceLanguage(event: Event): void {
  void set({ voiceLanguage: (event.target as HTMLSelectElement).value });
}

async function deleteModel(model: VoiceModel): Promise<void> {
  deleting.value = model;
  error.value = "";
  try {
    await api.voiceDelete(model);
  } catch (reason) {
    error.value = String(reason);
  }
  await refreshVoice();
  deleting.value = null;
}
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

    <div id="voice-part" class="section-label voice-label">{{ t("chatSurface.settings.voice.title") }}</div>
    <p v-if="!supported" class="later">{{ t("chatSurface.settings.voice.unsupported") }}</p>
    <div v-else class="rows" role="group" aria-labelledby="voice-part">
      <div class="row boxed">
        <span class="name">
          <span id="voice-model">{{ t("chatSurface.settings.voice.model") }}</span>
          <small>{{ t("chatSurface.settings.voice.modelHint") }}</small>
        </span>
        <span class="seg" role="group" aria-labelledby="voice-model">
          <button v-for="choice in VOICE_MODELS" :key="choice.id" type="button" :aria-pressed="shown.voiceModel === choice.id" @click="pickVoiceModel(choice.id)">
            {{ t(choice.name) }} <span class="size">{{ size(choice.id) }}</span>
          </button>
        </span>
      </div>
      <label class="row">
        <span class="name">
          <span>{{ t("chatSurface.settings.voice.send") }}</span>
          <small>{{ t("chatSurface.settings.voice.sendHint") }}</small>
        </span>
        <input type="checkbox" role="switch" :checked="shown.voiceSend" @change="set({ voiceSend: ($event.target as HTMLInputElement).checked })" />
      </label>
      <!-- The wait goes with the switch: dimmed while the text stays in the box. -->
      <div :class="['row', 'boxed', { off: !shown.voiceSend }]">
        <span class="name">
          <span id="voice-delay">{{ t("chatSurface.settings.voice.delay") }}</span>
          <small>{{ t("chatSurface.settings.voice.delayHint") }}</small>
        </span>
        <span class="seg waits" role="group" aria-labelledby="voice-delay">
          <button v-for="ms in VOICE_DELAYS" :key="ms" type="button" :aria-pressed="shownDelay === ms" :disabled="!shown.voiceSend" @click="pickVoiceDelay(ms)">{{ delayName(ms) }}</button>
        </span>
      </div>
      <div class="row boxed">
        <span class="name">
          <span id="voice-language">{{ t("chatSurface.settings.voice.language") }}</span>
          <small>{{ t("chatSurface.settings.voice.languageHint") }}</small>
        </span>
        <select class="pick" aria-labelledby="voice-language" :value="shown.voiceLanguage" @change="pickVoiceLanguage">
          <option value="app">{{ appLanguage }}</option>
          <option v-for="each in VOICE_LANGUAGES" :key="each.id" :value="each.id">{{ each.name }}</option>
        </select>
      </div>
      <div class="row boxed">
        <span class="name">
          <span>{{ t("chatSurface.settings.voice.shortcut") }}</span>
          <small>{{ t("chatSurface.settings.voice.shortcutHint") }}</small>
        </span>
        <kbd class="keys-shown">{{ keys("mod+Shift+M") }}</kbd>
      </div>
      <div v-for="each in voiceRows" :key="each.id" class="row boxed">
        <span class="name">
          <span>{{ t(each.ready ? "chatSurface.settings.voice.downloaded" : each.fetching ? "chatSurface.settings.voice.downloading" : "chatSurface.settings.voice.notDownloaded") }}</span>
          <small v-if="each.fetching">{{ t("chatInput.voice.card.progress", { done: megabytes(each.fetching.done, language), total: megabytes(each.fetching.total, language) }) }}</small>
          <small v-else-if="each.problem" class="warn">{{ t(DOWNLOAD_PROBLEMS[each.problem]) }}</small>
          <small v-else-if="each.ready">{{ t("chatSurface.settings.voice.modelSize", { model: t(each.name), size: size(each.id) }) }}</small>
          <small v-else>{{ t("chatSurface.settings.voice.notDownloadedHint", { model: t(each.name) }) }}</small>
        </span>
        <button v-if="each.fetching" class="control" type="button" @click="cancelDownload()">{{ t("chatInput.voice.card.stop") }}</button>
        <button v-else-if="each.ready" class="control" type="button" :disabled="deleting === each.id" @click="deleteModel(each.id)">
          <span :class="{ hidden: deleting === each.id }">{{ t("chatSurface.settings.voice.delete") }}</span>
          <Spinner v-if="deleting === each.id" class="over" :size="12" />
        </button>
        <button v-else class="control" type="button" :disabled="!!voice.download" @click="downloadModel(each.id)">{{ t(each.problem ? "chatInput.voice.card.retry" : "chatInput.voice.card.download") }}</button>
      </div>
    </div>
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

.voice-label {
  margin-top: 10px;
}

p.later {
  margin: 0;
}

/* Five short waits side by side. */
.seg.waits button {
  padding: 0 8px;
}

.row.off .seg {
  opacity: 0.5;
}

/* A model's size, fainter beside its name. */
.seg .size {
  color: var(--text-subtle);
}

.name small.warn {
  color: var(--crash-text);
}

.pick {
  flex-shrink: 0;
  height: 30px;
  max-width: 240px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.keys-shown {
  flex-shrink: 0;
  padding: 3px 8px;
  border: 1px solid var(--line-strong);
  border-radius: 6px;
  background: var(--bg-seg);
  font-family: var(--font-mono);
  font-size: 12.5px;
  color: var(--text-strong);
}

:root:not([data-platform="mac"]) .keys-shown {
  font-family: var(--font);
  font-size: 12px;
}

.row .control {
  height: 28px;
  font-size: 12.5px;
}
</style>
