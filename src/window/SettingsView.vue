<script setup lang="ts">
import { getVersion } from "@tauri-apps/api/app";
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import ClaudeLogo from "../components/ClaudeLogo.vue";
import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Rich from "../components/Rich.vue";
import Spinner from "../components/Spinner.vue";
import ThemeSwitch from "../components/ThemeSwitch.vue";
import { useBackdropClose } from "../lib/dialog";
import { ago, editorName, megabytes, shortcutLabel, updateError } from "../lib/format";
import { LANGUAGES, languageName, t, type Key } from "../lib/i18n";
import { OPEN_LABELS, OPEN_TARGETS, type OpenFrom } from "../lib/open";
import { keys, shortcutModifiers } from "../lib/platform";
import { recordingEnded, recordingStarts } from "../lib/shortcutRecording";
import { api, now, snapshot, update } from "../lib/store";
import { tabKey } from "../lib/tabs";
import type { ExtensionStatus, OpenTarget, Settings, Theme } from "../lib/types";
import BackupSection from "./backup/BackupSection.vue";
import NotificationsPage from "./settings/NotificationsPage.vue";
import AccountsSection from "./settings/AccountsSection.vue";
import CliSection from "./settings/CliSection.vue";
import FlagSection from "./settings/FlagSection.vue";
import RadioSection from "./settings/RadioSection.vue";
import TowerSection from "./settings/TowerSection.vue";
import MissingProjects from "./backup/MissingProjects.vue";
import TrackOptions from "./track/TrackOptions.vue";

type Tab = "general" | "projects" | "servers" | "editor" | "claude" | "track" | "notifications" | "about";

/** `startTab`: the tab it opens on (the Sessions page's "Add hook" opens Claude's, the Track
 * page's settings button Track's); `startPart`: a part of it scrolled to the top (the first run's
 * phone row asks for the phone's, the Fuel page's account panel for the accounts', with the add
 * dialog open for `addAccount`). `openSetup`: the first run's steps are asked for again;
 * `openWhatsNew`: this version's notes. */
const props = defineProps<{ startTab?: Tab; startPart?: "phone" | "accounts" | "addAccount" }>();
const emit = defineEmits<{ close: []; openUpdate: []; openSetup: []; openWhatsNew: [] }>();

/** Each tab with its icon; Claude's is Claude's mark, in the same colour as the others. */
const tabs: Array<{ id: Tab; label: Key; icon: "settings" | "grid" | "server" | "editor" | "claude" | "flag" | "bell" }> = [
  { id: "general", label: "settings.general", icon: "settings" },
  { id: "projects", label: "common.projects", icon: "grid" },
  { id: "servers", label: "settings.servers", icon: "server" },
  { id: "editor", label: "settings.editor", icon: "editor" },
  { id: "claude", label: "settings.claude", icon: "claude" },
  { id: "track", label: "track.nav", icon: "flag" },
  { id: "notifications", label: "settings.notifications", icon: "bell" },
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
const tab = ref<Tab>(props.startTab ?? "general");
// Asked for another tab while it is open (the popover's "Quiet hours…"): that one comes up.
watch(
  () => props.startTab,
  (asked) => {
    if (asked) tab.value = asked;
  },
);
/** The rail top to bottom: the sections, then About. */
const railOrder: Tab[] = [...tabs.map((section) => section.id), "about"];

/** ↑/↓ (or ←/→), Home and End move along the rail. */
function onRailKey(event: KeyboardEvent, id: Tab): void {
  const next = tabKey(event, railOrder.length, railOrder.indexOf(id));
  if (next !== null) tab.value = railOrder[next];
}

/** The tab's room: a long tab (General) scrolls in it, and each tab opens at its top. */
const body = ref<HTMLFormElement | null>(null);
watch(tab, () => {
  if (body.value) body.value.scrollTop = 0;
});

const version = ref("");
const form = reactive<Settings>({
  language: "system",
  script: "dev",
  packageManager: "auto",
  editor: "vscode",
  openUrlOnStart: false,
  notify: true,
  fuelAlert: true,
  fuelInBar: true,
  fuelInPopover: true,
  barProjects: "working",
  terminalProjects: "open",
  fuelEvery: 15,
  serversInBar: true,
  claudesInBar: true,
  sounds: { claudeFinished: "boxbox", claudeAsking: "limiter", serverCrashed: "yellowflag", serverReady: "", commandDone: "" },
  launchAtLogin: false,
  shortcut: true,
  shortcutKeys: "Ctrl+Alt+KeyP",
  autoUpdate: true,
  projects: {},
  order: [],
  theme: "system",
  resume: "ask",
  onboarded: false,
  suggestProjects: false,
  open: { list: "terminal", pages: "terminal", outside: "terminal" },
});
const error = ref("");
const hookBusy = ref(false);

/** How often the core may ask Anthropic for the limits, in minutes: the paces it takes. */
const FUEL_EVERY = [1, 5, 15];

/** What was open at the last quit, at the next start: asked for, put back, or left. */
const RESUME: Array<{ id: Settings["resume"]; label: Key }> = [
  { id: "ask", label: "settings.resume.ask" },
  { id: "always", label: "settings.resume.always" },
  { id: "never", label: "settings.resume.never" },
];

/** Which projects the menu bar's window lists: every one, or the favourites and those at work. */
const BAR_PROJECTS: Array<{ id: string; label: Key }> = [
  { id: "all", label: "settings.barAll" },
  { id: "working", label: "settings.barWorking" },
];

/** Which projects the terminal panel's picker lists: every one, or the favourites and those open
 * or at work, as the list's "Favourites and open". */
const TERMINAL_PROJECTS: Array<{ id: string; label: Key }> = [
  { id: "all", label: "settings.barAll" },
  { id: "open", label: "window.scopeOpen" },
];

/** Where a project opens from each place unless it chose its own (lib/open); `hint` says what
 * opens it there. */
const OPEN_FROM: Array<{ from: OpenFrom; label: Key; hint: Key }> = [
  { from: "list", label: "open.from.list", hint: "open.listHint" },
  { from: "pages", label: "open.from.pages", hint: "open.pagesHint" },
  { from: "outside", label: "open.from.outside", hint: "open.outsideHint" },
];

/** The form's choice for `from`, read as lib/open reads it. */
function openChoice(from: OpenFrom): OpenTarget {
  return OPEN_TARGETS[from].find((target) => target === form.open?.[from]) ?? OPEN_TARGETS[from][0];
}

/** A pick goes into the form; its `change` then saves it. */
function pickOpen(from: OpenFrom, value: string): void {
  form.open = { ...form.open, [from]: OPEN_TARGETS[from].find((target) => target === value) };
}

/** The moved folders' list (backup/MissingProjects): an import may bring in projects whose folder
 * isn't on this machine. */
const missing = ref<InstanceType<typeof MissingProjects> | null>(null);

/** This version has notes to show: asked of the core when About opens, and the way to them is
 * there only then. */
const hasNotes = ref(false);
watch(
  tab,
  (shown) => {
    if (shown !== "about") return;
    void api.whatsNew().then(
      (news) => (hasNotes.value = !!news?.notes?.length),
      () => (hasNotes.value = false),
    );
  },
  { immediate: true },
);

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
  // One recording at a time: the mini tower's (settings/TowerSection), if under way, ends here.
  recordingStarts(stopRecording);
  shortcutError.value = "";
  held.value = "";
  recording.value = true;
  // The current shortcut must not fire while its replacement is typed.
  void api.suspendShortcut();
  window.addEventListener("keydown", onRecordKey, true);
  window.addEventListener("keyup", onRecordKeyUp, true);
}

function stopRecording(): void {
  recordingEnded(stopRecording);
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

/** The parts a start can ask for, by their labels' ids. */
const PARTS = { phone: "phone-section", accounts: "accounts-section", addAccount: "accounts-section" } as const;

/** Scrolls the part whose label is `id` to the top of the tab, a little room above it, once it is
 * drawn (the phone's waits for the core's answer); not looked for after two seconds. */
function revealPart(id: string): void {
  const reveal = (): boolean => {
    const label = document.getElementById(id);
    if (label && body.value) body.value.scrollTop += label.getBoundingClientRect().top - body.value.getBoundingClientRect().top - 16;
    return !!label;
  };
  if (reveal() || !body.value) return;
  const watcher = new MutationObserver(() => {
    if (reveal()) watcher.disconnect();
  });
  watcher.observe(body.value, { childList: true, subtree: true });
  setTimeout(() => watcher.disconnect(), 2000);
}

// showModal focuses the first control; settings open with nothing focused.
onMounted(() => {
  dialog.value?.showModal();
  (document.activeElement as HTMLElement | null)?.blur();
  void getVersion().then((v) => (version.value = v));
  if (props.startPart) revealPart(PARTS[props.startPart]);
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

/* ---------- the app's own update (About) ---------- */

/** One quiet line under the version: where the update stands. Empty for a build that never asks. */
const updateLine = computed(() => {
  const state = update.value;
  if (!state) return "";
  const version = state.version ?? "";
  switch (state.status) {
    case "idle":
      if (state.offline) return t("about.update.offline");
      return state.checkedAt === null ? t("about.update.current") : t("about.update.checked", { time: ago(state.checkedAt, now.value) });
    case "checking":
      return t("about.update.checking");
    case "downloading":
      return state.total === null
        ? t("about.update.downloadingSome", { version, done: megabytes(state.downloaded) })
        : t("about.update.downloading", { version, done: megabytes(state.downloaded), total: megabytes(state.total) });
    case "ready":
      return t("about.update.ready", { version });
    case "installing":
      return t("about.update.installing", { version });
    case "available":
      return t("about.update.available", { version });
    case "failed":
      return updateError(state.error);
    default:
      return "";
  }
});

/** How far the download is, 0–100; null when it isn't downloading or the server gave no size. */
const downloadShare = computed(() => {
  const state = update.value;
  if (state?.status !== "downloading" || !state.total) return null;
  return Math.min(100, Math.max(0, (state.downloaded / state.total) * 100));
});

/** Asks for a new version now; the answer shows as the state changes. */
function checkUpdate(): void {
  void api.updateCheck().catch(() => undefined);
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

/** A click on a segment is no `change` for the form to save on: it saves here. */
function setFuelEvery(minutes: number): void {
  if (form.fuelEvery === minutes) return;
  form.fuelEvery = minutes;
  void save();
}

/** A value it doesn't know reads as `working`, as the menu bar's window reads it. */
const barProjects = computed(() => (form.barProjects === "all" ? "all" : "working"));

function setBarProjects(choice: string): void {
  if (barProjects.value === choice) return;
  form.barProjects = choice;
  void save();
}

/** A value it doesn't know reads as `open`, as the terminal panel reads it. */
const terminalProjects = computed(() => (form.terminalProjects === "all" ? "all" : "open"));

function setTerminalProjects(choice: string): void {
  if (terminalProjects.value === choice) return;
  form.terminalProjects = choice;
  void save();
}

function setResume(mode: Settings["resume"]): void {
  if (form.resume === mode) return;
  form.resume = mode;
  void save();
}

/** The theme switch's pick: saved here, whatever it is made of (its own `change`s are kept from
 * the form, which would save a second time). */
function setTheme(theme: Theme): void {
  if (form.theme === theme) return;
  form.theme = theme;
  void save();
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
            <ClaudeLogo v-if="section.icon === 'claude'" :size="14" color="currentColor" />
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

      <form ref="body" class="body" @submit.prevent="save" @change="save">
        <fieldset v-show="tab === 'projects'" :aria-label="t('common.projects')">
          <!-- Where projects come from: the tab's first part (Settings › General's until 2026-10-22). -->
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
          <!-- Nothing to watch without a projects folder. -->
          <label class="check">
            <input v-model="form.suggestProjects" type="checkbox" :disabled="!snapshot?.settings.projectsDir" />
            <span>{{ t("settings.suggestProjects") }}</span>
          </label>
          <div class="hook">
            <div class="hook-text">
              <span id="bar-projects">{{ t("settings.barProjects") }}</span>
              <small>{{ t("settings.barProjectsHint") }}</small>
            </div>
            <span class="seg" role="group" aria-labelledby="bar-projects">
              <button v-for="choice in BAR_PROJECTS" :key="choice.id" type="button" :aria-pressed="barProjects === choice.id" @click="setBarProjects(choice.id)">
                {{ t(choice.label) }}
              </button>
            </span>
          </div>
          <div class="hook">
            <div class="hook-text">
              <span id="terminal-projects">{{ t("settings.terminalProjects") }}</span>
              <small>{{ t("settings.terminalProjectsHint") }}</small>
            </div>
            <span class="seg" role="group" aria-labelledby="terminal-projects">
              <button v-for="choice in TERMINAL_PROJECTS" :key="choice.id" type="button" :aria-pressed="terminalProjects === choice.id" @click="setTerminalProjects(choice.id)">
                {{ t(choice.label) }}
              </button>
            </span>
          </div>

          <div class="section section-label">{{ t("open.title") }}</div>
          <label v-for="place in OPEN_FROM" :key="place.from" class="field">
            <span>{{ t(place.label) }}</span>
            <select :value="openChoice(place.from)" @change="pickOpen(place.from, ($event.target as HTMLSelectElement).value)">
              <option v-for="target in OPEN_TARGETS[place.from]" :key="target" :value="target">{{ t(OPEN_LABELS[target]) }}</option>
            </select>
            <small>{{ t(place.hint, { enter: keys("Enter") }) }}</small>
          </label>
        </fieldset>

        <fieldset v-show="tab === 'servers'" :aria-label="t('settings.servers')">
          <label class="field">
            <span>{{ t("common.script") }}</span>
            <input v-model="form.script" data-setting="script" spellcheck="false" @blur="onFieldBlur" />
            <small>{{ t("settings.scriptHint") }}</small>
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
          <label class="check">
            <input v-model="form.serversInBar" type="checkbox" />
            <span>{{ t("settings.serversInBar") }}</span>
          </label>
        </fieldset>

        <fieldset v-show="tab === 'claude'" :aria-label="t('settings.claude')">
          <label class="check">
            <input v-model="form.claudesInBar" type="checkbox" />
            <span>{{ t("settings.claudesInBar") }}</span>
          </label>
          <label class="check">
            <input v-model="form.fuelInBar" type="checkbox" />
            <span>{{ t("settings.fuelInBar") }}</span>
          </label>
          <label class="check">
            <input v-model="form.fuelInPopover" type="checkbox" />
            <span>{{ t("settings.fuelInPopover") }}</span>
          </label>
          <div class="hook">
            <div class="hook-text">
              <span id="fuel-every">{{ t("settings.fuelEvery") }}</span>
              <small>{{ t("settings.fuelEveryHint") }}</small>
            </div>
            <span class="seg" role="group" aria-labelledby="fuel-every">
              <button v-for="minutes in FUEL_EVERY" :key="minutes" type="button" :aria-pressed="form.fuelEvery === minutes" @click="setFuelEvery(minutes)">
                {{ t("settings.fuelEveryMinutes", { count: minutes }) }}
              </button>
            </span>
          </div>
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
            <button v-if="hookOutdated" type="button" class="control hook-button" :disabled="hookBusy" :aria-busy="hookBusy" @click="setHook(true)">
              <span :class="{ hidden: hookBusy }">{{ t("settings.updateHook") }}</span>
              <Spinner v-if="hookBusy" class="over" :size="12" />
            </button>
            <button v-else type="button" class="control hook-button" :disabled="hookBusy" :aria-busy="hookBusy" @click="setHook(!snapshot?.claudeHook)">
              <span :class="{ hidden: hookBusy }">{{ t(snapshot?.claudeHook ? "settings.removeHook" : "settings.addHook") }}</span>
              <Spinner v-if="hookBusy" class="over" :size="12" />
            </button>
          </div>
          <p v-if="hookError" class="error">{{ hookError }}</p>
          <RadioSection />
          <AccountsSection :start-add="startPart === 'addAccount'" />
        </fieldset>

        <!-- The Track page's own: each change goes to the core by its own command, never with the
             form (`@change.stop`). -->
        <fieldset v-show="tab === 'track'" :aria-label="t('track.nav')" @change.stop>
          <TrackOptions />
        </fieldset>

        <!-- Notifications' own: each change goes to the core by its own command (`@change.stop`).
             The sounds among them are the form's: picking one plays it and saves, the button
             plays it again. -->
        <fieldset v-show="tab === 'notifications'" :aria-label="t('settings.notifications')" @change.stop>
          <NotificationsPage>
            <template #sounds>
              <div class="rows" role="group" aria-labelledby="notif-sounds" @change="save">
                <div v-for="event in soundEvents" :key="event.id" class="row sound-row">
                  <label :for="`sound-${event.id}`" class="name">{{ t(event.label) }}</label>
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
              </div>
            </template>
          </NotificationsPage>
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

        <!-- Longer than the dialog: it scrolls in the tab's room, its later parts under their labels. -->
        <fieldset v-show="tab === 'general'" :aria-label="t('settings.general')">
          <div id="settings-appearance" class="section-label">{{ t("settings.appearance") }}</div>
          <!-- Its pick is saved by `setTheme`: a `change` of its own never reaches the form. -->
          <div class="theme" role="group" aria-labelledby="settings-appearance" @change.stop>
            <ThemeSwitch :model-value="form.theme" @update:model-value="setTheme" />
          </div>

          <label class="field section">
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

          <button type="button" class="control quiet" @click="emit('openSetup')">{{ t("settings.openSetup") }}</button>
          <div class="field">
            <label class="check">
              <input v-model="form.autoUpdate" type="checkbox" />
              <span>{{ t("settings.autoUpdate") }}</span>
            </label>
            <small>{{ t("settings.autoUpdateHint") }}</small>
          </div>
          <!-- On unless turned off: a settings file from before it holds no value. -->
          <label class="check">
            <input type="checkbox" :checked="form.panelFoldOutside !== false" @change="form.panelFoldOutside = ($event.target as HTMLInputElement).checked" />
            <span>{{ t("settings.panelFoldOutside") }}</span>
          </label>

          <div class="section section-label">{{ t("settings.startup") }}</div>
          <div class="hook">
            <div class="hook-text">
              <span id="resume-mode">{{ t("resume.title") }}</span>
              <small>{{ t("settings.resumeHint") }}</small>
            </div>
            <span class="seg" role="group" aria-labelledby="resume-mode">
              <button v-for="mode in RESUME" :key="mode.id" type="button" :aria-pressed="form.resume === mode.id" @click="setResume(mode.id)">
                {{ t(mode.label) }}
              </button>
            </span>
          </div>

          <!-- Each its own part, label and rows, saved by its own commands (`@change.stop`). -->
          <div @change.stop>
            <FlagSection />
            <TowerSection />
            <CliSection />
          </div>

          <!-- The backup's own: its dialogs' boxes are no settings for the form to save (`@change.stop`). -->
          <div class="section section-label">{{ t("backup.section") }}</div>
          <div @change.stop>
            <BackupSection @imported="missing?.load()" />
          </div>

          <!-- Drawn only while a project's folder is gone. -->
          <MissingProjects ref="missing" :active="tab === 'general'">
            <div class="section section-label">{{ t("missing.section") }}</div>
          </MissingProjects>
        </fieldset>

        <section v-show="tab === 'about'" class="about" :aria-label="t('settings.about')">
          <div class="app">
            <PitwallGlyph :size="40" lamps="var(--run)" />
            <div class="app-text">
              <span class="app-name">Pitwall</span>
              <span v-if="version" class="version-row">
                <small>{{ t("about.version", { version }) }}</small>
                <button v-if="hasNotes" type="button" class="link" @click="emit('openWhatsNew')">{{ t("about.whatsNew") }}</button>
              </span>
              <!-- Where the app's own update stands, and at the row's right the one thing to do about it. -->
              <small v-if="updateLine" class="update-line">{{ updateLine }}</small>
              <span
                v-if="downloadShare !== null"
                class="update-bar"
                role="progressbar"
                :aria-label="t('about.update.progress')"
                :aria-valuenow="Math.round(downloadShare)"
                aria-valuemin="0"
                aria-valuemax="100"
              >
                <span :style="{ width: `${downloadShare}%` }"></span>
              </span>
            </div>
            <button
              v-if="update?.status === 'idle' || update?.status === 'checking'"
              type="button"
              class="control about-button"
              :disabled="update.status === 'checking'"
              :aria-busy="update.status === 'checking'"
              @click="checkUpdate"
            >
              <!-- The label keeps its room under the spinner: the button never changes width. -->
              <span :class="{ hidden: update.status === 'checking' }">{{ t("about.update.check") }}</span>
              <Spinner v-if="update.status === 'checking'" class="over" :size="12" />
            </button>
            <button v-else-if="update?.status === 'ready'" type="button" class="control primary about-button" @click="emit('openUpdate')">{{ t("common.restart") }}</button>
            <button v-else-if="update?.status === 'available'" type="button" class="control about-button" @click="api.openLink('releases')">{{ t("update.openPage") }}</button>
            <button v-else-if="update?.status === 'failed'" type="button" class="control about-button" @click="checkUpdate">{{ t("about.update.retry") }}</button>
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

<style scoped src="./settings/settings.css"></style>
<style scoped>
/* A dialog over the window, like the command dialog. */
/* Fixed height, so switching tabs never resizes it. */
.settings {
  width: 760px;
  height: 540px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 48px);
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 12px;
  background: var(--bg-dialog);
  color: var(--text);
  font-size: 13px;
  box-shadow: var(--shadow-dialog);
}

.settings[open] {
  display: flex;
  flex-direction: column;
  animation: pop 0.16s ease-out;
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
  background: var(--bg-icon-hover);
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
  color: var(--text-nav);
  text-align: left;
}

.rail button:hover {
  background: var(--bg-rail-hover);
}

.rail button.on {
  background: var(--bg-rail-on);
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
  overscroll-behavior: contain;
}

@keyframes pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

/* At its own height: a tab longer than its room scrolls there, it is never squeezed. */
fieldset {
  flex-shrink: 0;
  min-width: 0;
  margin: 0;
  padding: 20px 0 0;
  border: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

/* A part's label sits closer to what it names than the rows do to each other. */
fieldset > .section-label {
  margin-bottom: -6px;
}

.theme {
  display: flex;
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

/* A box that can't be ticked as things stand. */
.check:has(input:disabled) {
  color: var(--text-faint);
}

.hint {
  font-size: 12px;
  color: var(--text-subtle);
  line-height: 1.5;
}

/* Event, its sound, and a button to hear it: a line of Notifications' list. */
.sound-row select {
  flex: none;
  width: 210px;
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
  background: var(--bg-control-hover);
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

/* The label never wraps; the text column gives way instead. */
.hook-button {
  flex-shrink: 0;
  white-space: nowrap;
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

.control.primary {
  background: var(--btn-primary-bg);
  border-color: var(--btn-primary-line);
  color: var(--btn-primary-text);
}

.control.primary:hover {
  background: var(--btn-primary-hover);
}

/* A way to somewhere else, not a setting: smaller, and no ground of its own until the pointer is
   on it. */
.control.quiet {
  align-self: flex-start;
  height: 28px;
  padding: 0 10px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.control.quiet:hover {
  background: var(--bg-icon-hover);
  color: var(--text);
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
  flex-grow: 1;
  min-width: 0;
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

/* The version, and beside it the way to its notes. */
.version-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.version-row small {
  flex-shrink: 0;
  white-space: nowrap;
}

/* A link in the row's flow. */
.link {
  min-width: 0;
  padding: 0;
  border: 0;
  border-radius: 3px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
  text-decoration: underline;
  text-decoration-color: var(--text-faint);
  text-underline-offset: 3px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link:hover {
  color: var(--text-strong);
  text-decoration-color: currentColor;
}

/* A long one wraps beside its button. */
.update-line {
  line-height: 1.5;
}

/* The download so far: a thin neutral bar, no state colour. */
.update-bar {
  display: block;
  height: 4px;
  margin-top: 6px;
  border-radius: 2px;
  background: var(--line);
  overflow: hidden;
}

.update-bar span {
  display: block;
  height: 100%;
  border-radius: 2px;
  background: var(--text-muted);
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

/* Its own colour: in the app's row the name's brighter white would reach it. */
.about-button {
  position: relative;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--text);
  white-space: nowrap;
}

.coffee :deep(svg) {
  color: var(--coffee);
}
</style>
