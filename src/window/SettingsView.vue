<script setup lang="ts">
import { getVersion } from "@tauri-apps/api/app";
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import ClaudeMark from "../components/ClaudeMark.vue";
import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Rich from "../components/Rich.vue";
import { useBackdropClose } from "../lib/dialog";
import { editorName, shortcutLabel } from "../lib/format";
import { LANGUAGES, languageName, t, type Key } from "../lib/i18n";
import { keys, shortcutModifiers } from "../lib/platform";
import { api, snapshot } from "../lib/store";
import { tabKey } from "../lib/tabs";
import type { ExtensionStatus, Settings } from "../lib/types";

const emit = defineEmits<{ close: [] }>();

type Tab = "general" | "servers" | "editor" | "claude" | "sounds" | "about";

/** Each tab with its icon; Claude's is Claude's mark, in the same colour as the others. */
const tabs: Array<{ id: Tab; label: Key; icon: "settings" | "server" | "editor" | "claude" | "speaker" }> = [
  { id: "general", label: "settings.general", icon: "settings" },
  { id: "servers", label: "settings.servers", icon: "server" },
  { id: "editor", label: "settings.editor", icon: "editor" },
  { id: "claude", label: "settings.claude", icon: "claude" },
  { id: "sounds", label: "settings.sounds", icon: "speaker" },
];

/** Pitwall's sounds (src-tauri/sounds). */
const soundOptions: Array<{ id: string; label: Key }> = [
  { id: "boxbox", label: "sound.boxbox" },
  { id: "limiter", label: "sound.limiter" },
  { id: "purple", label: "sound.purple" },
  { id: "lightsout", label: "sound.lightsout" },
  { id: "yellowflag", label: "sound.yellowflag" },
  { id: "wheelgun", label: "sound.wheelgun" },
  { id: "pitboard", label: "sound.pitboard" },
  { id: "radio", label: "sound.radio" },
  { id: "lights", label: "sound.lights" },
  { id: "chime", label: "sound.chime" },
];

/** What can make a sound; Claude's come with its notifications. */
const soundEvents: Array<{ id: keyof Settings["sounds"]; label: Key; claude: boolean }> = [
  { id: "claudeFinished", label: "sound.event.claudeFinished", claude: true },
  { id: "claudeAsking", label: "sound.event.claudeAsking", claude: true },
  { id: "serverCrashed", label: "sound.event.serverCrashed", claude: false },
  { id: "serverReady", label: "sound.event.serverReady", claude: false },
  { id: "commandDone", label: "sound.event.commandDone", claude: false },
];

function soundName(id: string): string {
  return t(soundOptions.find((sound) => sound.id === id)?.label ?? "sound.none");
}

const dialog = ref<HTMLDialogElement | null>(null);
const backdrop = useBackdropClose(dialog);
const tab = ref<Tab>("general");
/** The rail top to bottom: the sections, then About. */
const railOrder: Tab[] = [...tabs.map((section) => section.id), "about"];

/** ↑/↓ (or ←/→), Home and End move along the rail. */
function onRailKey(event: KeyboardEvent, id: Tab): void {
  const next = tabKey(event, railOrder.length, railOrder.indexOf(id));
  if (next !== null) tab.value = railOrder[next];
}

const version = ref("");
const form = reactive<Settings>({
  language: "system",
  script: "dev",
  packageManager: "auto",
  editor: "vscode",
  openUrlOnStart: false,
  notify: true,
  fuelAlert: true,
  sounds: { claudeFinished: "boxbox", claudeAsking: "limiter", serverCrashed: "yellowflag", serverReady: "", commandDone: "" },
  launchAtLogin: false,
  shortcut: true,
  shortcutKeys: "Ctrl+Alt+KeyP",
  projects: {},
  order: [],
});
const error = ref("");
const hookBusy = ref(false);

/* ---------- shortcut recorder ---------- */

const recording = ref(false);
/** What the keyboard is sending right now, shown live while recording. */
const held = ref("");
const shortcutError = ref("");

function modifiers(event: KeyboardEvent): string[] {
  const mods: string[] = [];
  if (event.ctrlKey) mods.push("Ctrl");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");
  if (event.metaKey) mods.push("Super");
  return mods;
}

function startRecording(): void {
  shortcutError.value = "";
  held.value = "";
  recording.value = true;
  // The current shortcut must not fire while its replacement is typed.
  void api.suspendShortcut();
  window.addEventListener("keydown", onRecordKey, true);
  window.addEventListener("keyup", onRecordKeyUp, true);
}

function stopRecording(): void {
  recording.value = false;
  held.value = "";
  window.removeEventListener("keydown", onRecordKey, true);
  window.removeEventListener("keyup", onRecordKeyUp, true);
}

function cancelRecording(): void {
  stopRecording();
  void api.resumeShortcut();
}

function onRecordKeyUp(event: KeyboardEvent): void {
  held.value = shortcutLabel(modifiers(event).join("+"));
}

async function onRecordKey(event: KeyboardEvent): Promise<void> {
  event.preventDefault();
  event.stopPropagation();
  const mods = modifiers(event);

  if (event.key === "Escape" && !mods.length) {
    cancelRecording();
    return;
  }

  // Only modifiers so far: show them and wait for the key.
  if (/^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/.test(event.code)) {
    held.value = shortcutLabel(mods.join("+"));
    return;
  }

  const keys = [...mods, event.code].join("+");
  held.value = shortcutLabel(keys);

  if (!mods.some((m) => m !== "Shift")) {
    shortcutError.value = t("settings.shortcutNeedsModifier", shortcutModifiers());
    return;
  }

  stopRecording();
  try {
    await api.setShortcut(keys);
    shortcutError.value = "";
  } catch (reason) {
    shortcutError.value = String(reason);
  }
}

// showModal focuses the first control; settings open with nothing focused.
onMounted(() => {
  dialog.value?.showModal();
  (document.activeElement as HTMLElement | null)?.blur();
  void getVersion().then((v) => (version.value = v));
});

onBeforeUnmount(() => {
  if (recording.value) cancelRecording();
});

/** Esc while recording a shortcut cancels the recording, not the dialog. */
function onCancel(event: Event): void {
  if (recording.value) event.preventDefault();
}
const hookError = ref("");
/** Installed, but from before the hooks that report Claude's state at once. */
const hookOutdated = computed(() => !!snapshot.value?.claudeHook && !!snapshot.value.claudeHookOutdated);

async function setHook(on: boolean): Promise<void> {
  hookBusy.value = true;
  hookError.value = "";
  try {
    await (on ? api.installClaudeHook() : api.uninstallClaudeHook());
  } catch (reason) {
    hookError.value = String(reason);
  } finally {
    hookBusy.value = false;
  }
}

/* ---------- form ← snapshot ---------- */

/** Each field as last taken from the snapshot (serialized): a field that differs from it holds
 * an edit not saved, or not back in a snapshot, yet. */
const taken: Partial<Record<keyof Settings, string>> = {};
let lastSeen = "";
/** Saves not answered yet. */
let saving = 0;

/** The text field being typed in (`data-setting`): a snapshot never writes over it. */
function focusedField(): string | undefined {
  return (document.activeElement as HTMLElement | null)?.dataset?.setting;
}

function clone<T>(value: T): T {
  return value === undefined ? value : (JSON.parse(JSON.stringify(value)) as T);
}

/**
 * Takes the snapshot's settings into the form, field by field, leaving the field being typed in
 * and any edit on its way to the core. `revert`: a save failed, so edits go back to what is
 * stored too.
 */
function takeSettings(settings: Settings, revert = false): void {
  const focused = focusedField();
  const keys = new Set([...Object.keys(form), ...Object.keys(settings)]) as Set<keyof Settings>;
  const fields = form as Record<keyof Settings, unknown>;

  for (const key of keys) {
    if (key === focused) continue;
    const theirs = JSON.stringify(settings[key]);
    const mine = JSON.stringify(fields[key]);
    if (theirs === mine) {
      taken[key] = theirs;
      continue;
    }
    // While a save is on its way, a snapshot may still be from before it.
    const edited = saving > 0 || (taken[key] !== undefined && mine !== taken[key]);
    if (edited && !revert) continue;
    fields[key] = clone(settings[key]);
    taken[key] = theirs;
  }
}

// Every `state` event brings a whole new snapshot; only a real change in the settings counts.
watch(
  () => snapshot.value?.settings,
  (settings) => {
    if (!settings) return;
    const seen = JSON.stringify(settings);
    if (seen === lastSeen) return;
    lastSeen = seen;
    takeSettings(settings);
  },
  { immediate: true },
);

/** Leaving a text field: what changed in the snapshot while it was being typed in comes in now. */
function onFieldBlur(): void {
  if (snapshot.value) takeSettings(snapshot.value.settings);
}

/* ---------- Pitwall for VS Code ---------- */

/** Whether the chosen editor has the extension; read again when the editor changes and when the
 * window comes back (after installing it there). */
const extension = ref<ExtensionStatus | null>(null);
const extensionWhy: Key[] = ["settings.extWhyServers", "settings.extWhySeen", "settings.extWhyReveal", "settings.extWhyProjects"];

async function checkExtension(): Promise<void> {
  const editor = form.editor;
  const status = await api.extensionStatus(editor);
  if (editor === form.editor) extension.value = status;
}

watch(() => form.editor, () => void checkExtension(), { immediate: true });
window.addEventListener("focus", checkExtension);
onBeforeUnmount(() => window.removeEventListener("focus", checkExtension));

async function save(): Promise<void> {
  error.value = "";
  // As the core will store it, so the snapshot that comes back matches the form.
  form.script = form.script.trim() || "dev";
  let failed = false;
  saving += 1;
  try {
    await api.setSettings({ ...form });
  } catch (reason) {
    error.value = String(reason);
    failed = true;
  } finally {
    saving -= 1;
  }
  // A failed save stored nothing: the form shows what is stored again.
  if (snapshot.value && !saving) takeSettings(snapshot.value.settings, failed);
}
</script>

<template>
  <dialog ref="dialog" class="settings" :aria-label="t('common.settings')" @cancel="onCancel" @close="emit('close')" @pointerdown="backdrop.down" @click="backdrop.click">
    <div class="head">
      <div class="heading">{{ t("common.settings") }}</div>
      <button type="button" class="close" :aria-label="t('settings.close')" :title="t('common.close')" @click="dialog?.close()">
        <Icon name="close" :size="16" />
      </button>
    </div>

    <div class="frame">
      <nav class="rail" role="tablist" :aria-label="t('settings.sections')" aria-orientation="vertical">
        <button
          v-for="section in tabs"
          :key="section.id"
          type="button"
          role="tab"
          :aria-selected="tab === section.id"
          :tabindex="tab === section.id ? 0 : -1"
          :class="{ on: tab === section.id }"
          @click="tab = section.id"
          @keydown="onRailKey($event, section.id)"
        >
          <span class="tab-icon">
            <ClaudeMark v-if="section.icon === 'claude'" :size="14" color="currentColor" />
            <Icon v-else :name="section.icon" :size="15" />
          </span>
          {{ t(section.label) }}
        </button>
        <div class="grow"></div>
        <button
          type="button"
          role="tab"
          :aria-selected="tab === 'about'"
          :tabindex="tab === 'about' ? 0 : -1"
          :class="{ on: tab === 'about' }"
          @click="tab = 'about'"
          @keydown="onRailKey($event, 'about')"
        >
          <span class="tab-icon"><Icon name="info" :size="15" /></span>
          {{ t("settings.about") }}
        </button>
      </nav>

      <form class="body" @submit.prevent="save" @change="save">
        <fieldset v-show="tab === 'servers'" :aria-label="t('settings.servers')">
          <label class="field">
            <span>{{ t("common.script") }}</span>
            <input v-model="form.script" data-setting="script" spellcheck="false" @blur="onFieldBlur" />
            <small>
              <Rich :text="t('settings.scriptHint')">
                <template #build><code>build</code></template>
              </Rich>
            </small>
          </label>
          <label class="field">
            <span>{{ t("settings.packageManager") }}</span>
            <select v-model="form.packageManager">
              <option value="auto">{{ t("settings.packageManagerAuto") }}</option>
              <option value="npm">npm</option>
              <option value="pnpm">pnpm</option>
              <option value="yarn">yarn</option>
              <option value="bun">bun</option>
            </select>
          </label>
          <label class="check">
            <input v-model="form.openUrlOnStart" type="checkbox" />
            <span>{{ t("settings.openUrlOnStart") }}</span>
          </label>
        </fieldset>

        <fieldset v-show="tab === 'claude'" :aria-label="t('settings.claude')">
          <label class="check">
            <input v-model="form.notify" type="checkbox" />
            <span>{{ t("settings.notify") }}</span>
          </label>
          <!-- The Fuel page's chip turns the same one on and off. -->
          <label class="check">
            <input v-model="form.fuelAlert" type="checkbox" />
            <span>{{ t("settings.fuelAlert") }}</span>
          </label>
          <div class="hook">
            <div class="hook-text">
              <span class="hook-title">
                {{ t("settings.hookTitle") }}
                <span :class="['badge', { on: snapshot?.claudeHook }]">{{ t(snapshot?.claudeHook ? "settings.on" : "settings.off") }}</span>
              </span>
              <small>
                <Rich :text="t('settings.hookText')">
                  <template #hook><code>Notification</code></template>
                  <template #settings><code>~/.claude/settings.json</code></template>
                  <template #events><code>~/.pitwall/claude-events</code></template>
                  <template #backup><code>settings.json.pitwall-backup</code></template>
                </Rich>
              </small>
              <small v-if="hookOutdated">{{ t("settings.hookOutdated") }}</small>
            </div>
            <!-- An older hook: installing again upgrades it. -->
            <button v-if="hookOutdated" type="button" class="control hook-button" :disabled="hookBusy" @click="setHook(true)">
              {{ t("settings.updateHook") }}
            </button>
            <button v-else type="button" class="control hook-button" :disabled="hookBusy" @click="setHook(!snapshot?.claudeHook)">
              {{ t(snapshot?.claudeHook ? "settings.removeHook" : "settings.addHook") }}
            </button>
          </div>
          <p v-if="hookError" class="error">{{ hookError }}</p>
        </fieldset>

        <!-- Picking a sound plays it; the button plays it again. -->
        <fieldset v-show="tab === 'sounds'" :aria-label="t('settings.sounds')">
          <div v-for="event in soundEvents" :key="event.id" class="sound-row">
            <label :for="`sound-${event.id}`">{{ t(event.label) }}</label>
            <select
              :id="`sound-${event.id}`"
              v-model="form.sounds[event.id]"
              :disabled="event.claude && !form.notify"
              @change="api.playSound(form.sounds[event.id])"
            >
              <option value="">{{ t("sound.none") }}</option>
              <option v-for="sound in soundOptions" :key="sound.id" :value="sound.id">{{ t(sound.label) }}</option>
            </select>
            <button
              type="button"
              class="play"
              :disabled="!form.sounds[event.id] || (event.claude && !form.notify)"
              :aria-label="t('settings.playSound', { name: soundName(form.sounds[event.id]) })"
              :title="t('settings.playSound', { name: soundName(form.sounds[event.id]) })"
              @click="api.playSound(form.sounds[event.id])"
            >
              <Icon name="play" :size="12" />
            </button>
          </div>
          <small v-if="!form.notify" class="hint">{{ t("settings.soundsClaudeOff") }}</small>
        </fieldset>

        <fieldset v-show="tab === 'editor'" :aria-label="t('settings.editor')">
          <label class="field">
            <span>{{ t("settings.openProjectsIn") }}</span>
            <select v-model="form.editor">
              <option value="vscode">VS Code</option>
              <option value="vscode-insiders">VS Code Insiders</option>
              <option value="cursor">Cursor</option>
              <option value="windsurf">Windsurf</option>
            </select>
          </label>

          <!-- Pitwall for VS Code: there or not in the chosen editor, how to get it, and why. -->
          <div class="hook">
            <div class="hook-text">
              <span class="hook-title">
                Pitwall for VS Code
                <span :class="['badge', { on: extension?.version }]">
                  {{ extension?.version ? t("settings.extInstalled", { version: extension.version }) : t("settings.extMissing") }}
                </span>
              </span>
              <small>{{ t("settings.extIntro") }}</small>
              <ul class="why">
                <li v-for="key in extensionWhy" :key="key">{{ t(key) }}</li>
              </ul>
              <small v-if="extension && !extension.version && !extension.marketplace">{{ t("settings.extVsixHint", { editor: editorName(form.editor) }) }}</small>
            </div>
            <button v-if="extension && !extension.version" type="button" class="control hook-button" @click="api.openExtensionPage(form.editor)">
              {{ t(extension.marketplace ? "settings.extOpenPage" : "settings.extDownload") }}
            </button>
          </div>
        </fieldset>

        <fieldset v-show="tab === 'general'" :aria-label="t('settings.general')">
          <label class="field">
            <span>{{ t("settings.language") }}</span>
            <select v-model="form.language">
              <option value="system">{{ t("settings.languageSystem", { language: languageName(snapshot?.systemLanguage ?? "en") }) }}</option>
              <option v-for="option in LANGUAGES" :key="option.code" :value="option.code">{{ option.name }}</option>
            </select>
          </label>
          <label class="check">
            <input v-model="form.launchAtLogin" type="checkbox" />
            <span>{{ t("settings.launchAtLogin") }}</span>
          </label>
          <label class="check">
            <input v-model="form.shortcut" type="checkbox" />
            <span>{{ t("settings.shortcutToggle") }}</span>
          </label>
          <div class="shortcut">
            <span class="shortcut-label">{{ t("settings.shortcut") }}</span>
            <button
              type="button"
              :class="['recorder', { live: recording }]"
              :aria-label="t(recording ? 'settings.shortcutRecording' : 'settings.shortcutChange')"
              @click="recording ? cancelRecording() : startRecording()"
            >
              <template v-if="recording">{{ held || t("settings.shortcutPress") }}</template>
              <template v-else>{{ shortcutLabel(form.shortcutKeys) }}</template>
            </button>
            <small v-if="recording">{{ t("settings.shortcutHint", { ...shortcutModifiers(), esc: keys("Escape") }) }}</small>
          </div>
          <p v-if="shortcutError" class="error">{{ shortcutError }}</p>

          <div class="field">
            <span>{{ t("settings.projectsFolder") }}</span>
            <div class="folder">
              <span :class="['folder-path', { unset: !snapshot?.settings.projectsDir }]" :title="snapshot?.settings.projectsDir">
                {{ snapshot?.settings.projectsDir ?? t("settings.notSet") }}
              </span>
              <button type="button" class="control" @click="api.pickProjectsDir()">{{ t("settings.choose") }}</button>
              <button v-if="snapshot?.settings.projectsDir" type="button" class="control" @click="api.clearProjectsDir()">
                {{ t("settings.clear") }}
              </button>
            </div>
            <small>{{ t("settings.projectsFolderHint") }}</small>
          </div>
        </fieldset>

        <section v-show="tab === 'about'" class="about" :aria-label="t('settings.about')">
          <div class="app">
            <PitwallGlyph :size="40" lamps="var(--run)" />
            <div class="app-text">
              <span class="app-name">Pitwall</span>
              <small v-if="version">{{ t("about.version", { version }) }}</small>
            </div>
          </div>
          <p class="about-text">{{ t("about.blurb") }}</p>

          <div class="about-block">
            <div class="about-block-text">
              <span>{{ t("about.madeBy") }}</span>
              <small>{{ t("about.bio") }}</small>
            </div>
            <button type="button" class="control about-button" @click="api.openLink('site')">
              aenzenith.com
              <Icon name="external" :size="13" />
            </button>
          </div>

          <div class="about-block">
            <div class="about-block-text">
              <span>{{ t("about.support") }}</span>
              <small>{{ t("about.supportText") }}</small>
            </div>
            <button type="button" class="control about-button coffee" @click="api.openLink('coffee')">
              <Icon name="coffee" :size="15" />
              {{ t("about.coffee") }}
            </button>
          </div>
        </section>

        <p v-if="error" class="error">{{ error }}</p>
      </form>
    </div>
  </dialog>
</template>

<style scoped>
/* A dialog over the window, like the command dialog. */
/* Fixed height, so switching tabs never resizes it. */
.settings {
  width: 760px;
  height: 540px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 48px);
  padding: 0;
  border: 1px solid #2f343d;
  border-radius: 12px;
  background: #1b1e24;
  color: var(--text);
  font-size: 13px;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
}

.settings[open] {
  display: flex;
  flex-direction: column;
  animation: pop 0.16s ease-out;
}

.settings::backdrop {
  background: rgba(8, 9, 11, 0.55);
}

.head {
  height: 52px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  padding: 0 12px 0 24px;
  border-bottom: 1px solid var(--line);
}

.heading {
  flex-grow: 1;
  font-size: 15px;
  font-weight: 600;
}

.close {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.close:hover {
  background: #262a33;
  color: var(--text-strong);
}

.frame {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
}

.rail {
  width: 180px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 10px;
  border-right: 1px solid var(--line);
}

.rail button {
  height: 32px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  font-size: 13px;
  color: #b4bac3;
  text-align: left;
}

.rail button:hover {
  background: #23272e;
}

.rail button.on {
  background: #2a2f37;
  color: var(--text-strong);
}

.rail .grow {
  flex-grow: 1;
}

/* One width for every icon, so the labels line up. */
.tab-icon {
  width: 16px;
  display: inline-flex;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-subtle);
}

.rail button.on .tab-icon {
  color: var(--text-strong);
}

.body {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  padding: 4px 28px 20px;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

fieldset {
  margin: 0;
  padding: 20px 0 0;
  border: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}

.field small {
  font-size: 12px;
  color: var(--text-subtle);
  line-height: 1.5;
}

input:not([type="checkbox"]),
select {
  height: 32px;
  max-width: 320px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.check {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
}

.check input {
  width: 16px;
  height: 16px;
  accent-color: var(--run);
}

.hint {
  font-size: 12px;
  color: var(--text-subtle);
  line-height: 1.5;
}

/* Event, its sound, and a button to hear it. */
.sound-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 210px 30px;
  align-items: center;
  gap: 10px;
  font-size: 13px;
}

.sound-row label {
  min-width: 0;
}

.sound-row select:disabled {
  opacity: 0.5;
}

.play {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
}

.play:hover:not(:disabled) {
  background: #2c3039;
  color: var(--text-strong);
}

.play:disabled {
  opacity: 0.35;
}

kbd,
code {
  font-family: var(--font-mono);
  font-size: 12px;
}

.hook {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.hook-text {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-grow: 1;
  font-size: 13px;
}

.hook-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.why {
  margin: 0;
  padding-left: 16px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-muted);
}

.hook small {
  font-size: 12px;
  color: var(--text-subtle);
  line-height: 1.55;
}

/* The label never wraps; the text column gives way instead. */
.hook-button {
  flex-shrink: 0;
  white-space: nowrap;
}

.hook-text {
  min-width: 0;
}

.badge {
  flex-shrink: 0;
  white-space: nowrap;
  padding: 1px 7px;
  border-radius: 9px;
  font-size: 11px;
  font-weight: 600;
  background: #262a33;
  color: var(--text-muted);
}

.badge.on {
  background: #1d2a22;
  color: var(--run);
}

.control:disabled {
  opacity: 0.5;
}

.folder {
  display: flex;
  align-items: center;
  gap: 8px;
}

.folder-path {
  flex: 1 1 auto;
  min-width: 0;
  height: 32px;
  line-height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.folder-path.unset {
  font-family: var(--font);
  font-size: 13px;
  color: var(--text-subtle);
}

.shortcut {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 13px;
}

.shortcut-label {
  flex-shrink: 0;
  white-space: nowrap;
}

.shortcut small {
  font-size: 12px;
  color: var(--text-subtle);
}

.recorder {
  min-width: 120px;
  height: 32px;
  padding: 0 14px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  font-family: var(--font-mono);
  font-size: 14px;
  letter-spacing: 0.08em;
  color: var(--text-strong);
}

/* Windows and Linux spell the keys out (Ctrl+Alt+P): no tracking between their letters. */
:root:not([data-platform="mac"]) .recorder {
  letter-spacing: 0;
}

.recorder.live {
  border-color: var(--run);
  color: var(--run);
  font-family: var(--font);
  font-size: 12px;
  letter-spacing: 0;
}

.error {
  margin: 0;
  color: var(--crash-text);
  font-size: 13px;
}

.control {
  height: 32px;
  padding: 0 14px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover {
  background: #2c3039;
}

.about {
  padding-top: 24px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.app {
  display: flex;
  align-items: center;
  gap: 14px;
  color: var(--text-strong);
}

.app-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.app-name {
  font-size: 17px;
  font-weight: 600;
}

.app-text small {
  font-size: 12px;
  color: var(--text-subtle);
}

.about-text {
  margin: 0;
  font-size: 13px;
  line-height: 1.55;
  color: var(--text-muted);
}

.about-block {
  display: flex;
  align-items: center;
  gap: 16px;
  padding-top: 18px;
  border-top: 1px solid var(--line);
}

.about-block-text {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}

.about-block-text small {
  font-size: 12px;
  color: var(--text-subtle);
  line-height: 1.55;
}

.about-button {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  white-space: nowrap;
}

.coffee :deep(svg) {
  color: #ffdd00;
}
</style>
