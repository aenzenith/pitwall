<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { t, type Key } from "../../lib/i18n";
import { isMac } from "../../lib/platform";
import { useEditor } from "../../lib/setup";
import { api } from "../../lib/store";
import type { CliState, Editor, NotifyAccess, PhoneChannel, PhoneView } from "../../lib/types";

/**
 * The last step: whether the system lets Pitwall post notifications (with a test), the
 * `pitwall` command, the editor projects open in (only where more than one is installed) and
 * the phone's channel, each a row; and the button that ends the first run. What each row shows
 * is asked of the core when the step shows, again when the window comes back to the front and
 * when Settings over it closes (`covered`): nothing of it is kept.
 * `finishing`: the view is on its way out; `finish`: the button was pressed; `openPhone`: Settings
 * at the phone's part is asked for.
 */
const props = defineProps<{ shown: boolean; covered: boolean; finishing: boolean }>();
const emit = defineEmits<{ finish: []; openPhone: [] }>();

/* ---------- notifications ---------- */

const ACCESS: Record<Exclude<NotifyAccess, "unknown">, Key> = {
  allowed: "setup.alerts.allowed",
  off: "setup.alerts.off",
  notAsked: "setup.alerts.notAsked",
  borrowed: "setup.alerts.borrowed",
};

/** What the system allows; null until it said. */
const access = ref<NotifyAccess | null>(null);
const testing = ref(false);
const opening = ref(false);
const notifyError = ref("");
/** Readings asked for so far: an answer to an older one is dropped. */
let reads = 0;

async function readAccess(): Promise<void> {
  // While a test runs its answer is the one that counts: the window comes back to the front from
  // macOS's question before the system has it.
  if (testing.value) return;
  const mine = ++reads;
  try {
    const next = await api.notificationAccess();
    if (mine === reads) access.value = next;
  } catch {
    if (mine === reads) access.value = "unknown";
  }
}

/** One test banner: macOS asks first when it never had an answer, and the button spins until it has one. */
async function test(): Promise<void> {
  if (testing.value) return;
  testing.value = true;
  notifyError.value = "";
  const mine = ++reads;
  try {
    const next = await api.notificationTest();
    if (mine === reads) access.value = next;
  } catch (reason) {
    notifyError.value = String(reason);
  } finally {
    testing.value = false;
  }
}

async function openSystemSettings(): Promise<void> {
  if (opening.value) return;
  opening.value = true;
  notifyError.value = "";
  try {
    await api.notificationSettings();
  } catch (reason) {
    notifyError.value = String(reason);
  } finally {
    opening.value = false;
  }
}

/* ---------- the pitwall command, as Settings › General's part installs it ---------- */

const cli = ref<CliState | null>(null);
const installing = ref(false);
const cliError = ref("");
/** Counted up as an install begins and ends: an answer asked for before then is not taken. */
let turns = 0;

/** On macOS only so far: nothing of it is drawn elsewhere. */
const cliShown = computed(() => isMac && cli.value?.status !== "unsupported");
/** Where it is, in two: its folders, which give way when it is long, and its end, which stays. */
const cliPath = computed(() => {
  const path = cli.value?.path ?? "";
  const cut = path.lastIndexOf("/", path.lastIndexOf("/") - 1);
  return cut > 0 ? { head: path.slice(0, cut), tail: path.slice(cut) } : { head: "", tail: path };
});

async function readCli(): Promise<void> {
  const mine = turns;
  try {
    const state = await api.cliState();
    if (mine === turns) cli.value = state;
  } catch {
    // Kept as it was.
  }
}

async function install(): Promise<void> {
  if (installing.value) return;
  installing.value = true;
  cliError.value = "";
  turns++;
  try {
    cli.value = await api.cliInstall();
  } catch (reason) {
    cliError.value = String(reason);
  } finally {
    turns++;
    installing.value = false;
  }
  // How it stands now, after a refusal.
  if (cliError.value) void readCli();
}

/* ---------- the editor ---------- */

/** The editors Settings offers, by the names it gives them. */
const EDITORS: Array<{ id: Editor; name: string }> = [
  { id: "vscode", name: "VS Code" },
  { id: "vscode-insiders", name: "VS Code Insiders" },
  { id: "cursor", name: "Cursor" },
  { id: "windsurf", name: "Windsurf" },
];

const installed = ref<Editor[]>([]);
const { editor, error: editorError, set: setEditor } = useEditor();
/** The installed ones, and the one chosen should it not be among them: the select says what is set. */
const editorChoices = computed(() => EDITORS.filter((choice) => installed.value.includes(choice.id) || choice.id === editor.value));

async function readEditors(): Promise<void> {
  try {
    installed.value = await api.installedEditors();
  } catch {
    // Kept as it was.
  }
}

/* ---------- the phone ---------- */

const CHANNELS: Record<PhoneChannel, string> = { ntfy: "ntfy", telegram: "Telegram", webhook: "Webhook" };

/** The phone's channel as the core holds it; null until it answered. */
const phone = ref<PhoneView | null>(null);
const phoneLine = computed(() => {
  if (!phone.value) return "";
  return phone.value.on ? t("setup.alerts.phoneOn", { channel: CHANNELS[phone.value.channel] }) : t("setup.alerts.phoneOff");
});

async function readPhone(): Promise<void> {
  try {
    phone.value = await api.phoneConfig();
  } catch {
    // Kept as it was.
  }
}

/* ---------- read when shown, and when it comes back ---------- */

function read(): void {
  void readAccess();
  if (isMac) void readCli();
  void readEditors();
  void readPhone();
}

watch(
  () => props.shown,
  (shown) => {
    if (shown) read();
  },
  { immediate: true },
);

// Settings over it closed (the phone's "Set up…"): what was set there shows.
watch(
  () => props.covered,
  (covered) => {
    if (!covered && props.shown) read();
  },
);

// Back from System Settings, or from wherever else something changed.
function onFocus(): void {
  if (props.shown) read();
}

window.addEventListener("focus", onFocus);
onBeforeUnmount(() => window.removeEventListener("focus", onFocus));
</script>

<template>
  <section class="pane">
    <div class="head">
      <span class="head-text">
        <span class="title">{{ t("setup.step.alerts") }}</span>
      </span>
    </div>

    <div class="block options">
      <!-- Notifications: what the system allows; no line where it isn't asked. -->
      <div class="option">
        <span class="opt-text">
          <span>{{ t("settings.notifications") }}</span>
          <span v-if="access !== 'unknown'" class="sub" role="status">
            <Spinner v-if="!access" :size="11" />
            <template v-else>
              <span :class="['mark', { ok: access === 'allowed', warn: access === 'off' }]">
                <Icon v-if="access === 'allowed'" name="check" :size="13" />
                <i v-else></i>
              </span>
              <span class="sub-text">{{ t(ACCESS[access]) }}</span>
            </template>
          </span>
        </span>
        <span class="actions">
          <button v-if="access === 'off' || access === 'borrowed'" type="button" class="control" :disabled="opening" :aria-busy="opening" @click="openSystemSettings">
            <span :class="{ hidden: opening }">{{ t("setup.alerts.systemSettings") }}</span>
            <Spinner v-if="opening" class="over" :size="12" />
          </button>
          <button type="button" class="control" :disabled="testing || access === 'off'" :aria-busy="testing" @click="test">
            <span :class="{ hidden: testing }">{{ t("phone.test") }}</span>
            <Spinner v-if="testing" class="over" :size="12" />
          </button>
        </span>
      </div>
      <p v-if="notifyError" class="error" role="alert">{{ notifyError }}</p>

      <!-- The pitwall command: where it is, or the button that installs it. -->
      <template v-if="cliShown">
        <div class="option">
          <span class="opt-text">
            <span>{{ t("cli.command") }}</span>
            <span class="sub" role="status">
              <Spinner v-if="!cli" :size="11" />
              <template v-else>
                <span :class="['mark', { ok: !!cli.path }]">
                  <Icon v-if="cli.path" name="check" :size="13" />
                  <i v-else></i>
                </span>
                <span v-if="cli.path" class="sub-text path" :title="cli.path"><span class="path-head">{{ cliPath.head }}</span><span class="path-tail">{{ cliPath.tail }}</span></span>
                <span v-else class="sub-text">{{ t("cli.missing") }}</span>
              </template>
            </span>
          </span>
          <button v-if="cli && !cli.path" type="button" class="control" :disabled="installing" :aria-busy="installing" @click="install">
            <span :class="{ hidden: installing }">{{ t("cli.install") }}</span>
            <Spinner v-if="installing" class="over" :size="12" />
          </button>
        </div>
        <p v-if="cliError" class="error" role="alert">{{ cliError }}</p>
      </template>

      <!-- The editor: only where there is a choice to make. -->
      <template v-if="installed.length > 1">
        <div class="option">
          <label for="setup-editor" class="opt-text">
            <span>{{ t("settings.openProjectsIn") }}</span>
          </label>
          <select id="setup-editor" :value="editor" @change="setEditor(($event.target as HTMLSelectElement).value)">
            <option v-for="choice in editorChoices" :key="choice.id" :value="choice.id">{{ choice.name }}</option>
          </select>
        </div>
        <p v-if="editorError" class="error" role="alert">{{ editorError }}</p>
      </template>

      <!-- The phone: on or off and by which channel; it is set up in Settings. -->
      <div class="option">
        <span class="opt-text">
          <span>{{ t("phone.section") }}</span>
          <span class="sub" role="status">
            <Spinner v-if="!phone" :size="11" />
            <template v-else>
              <span :class="['mark', { ok: phone.on }]">
                <Icon v-if="phone.on" name="check" :size="13" />
                <i v-else></i>
              </span>
              <span class="sub-text">{{ phoneLine }}</span>
            </template>
          </span>
        </span>
        <button type="button" class="control" @click="emit('openPhone')">{{ t("setup.alerts.phoneSetUp") }}</button>
      </div>
    </div>

    <div class="foot">
      <button type="button" class="control primary" :disabled="finishing" :aria-busy="finishing" @click="emit('finish')">
        <span :class="{ hidden: finishing }">{{ t("setup.finish") }}</span>
        <Spinner v-if="finishing" class="over" :size="12" />
      </button>
    </div>
  </section>
</template>

<style scoped src="./setup.css"></style>

<style scoped>
/* Rows under hairlines, as the step before has them: a name with where it stands under it, and
   at the row's end what can be done about it. In a narrow window that drops under the text,
   still at the right, as the foot's button does. */
.options {
  gap: 0;
  padding: 0;
}

.option {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px 16px;
  min-height: 34px;
  padding: 14px 0;
}

.option ~ .option {
  border-top: 1px solid var(--line);
}

.option > .opt-text {
  flex: 1 1 200px;
}

.option > .actions,
.option > .control,
.option > select {
  margin-left: auto;
}

.options .error {
  margin: -6px 0 12px;
}

/* Where a row stands may take two lines (a dev build's notifications say whose they are): the
   mark stays by the first. A path keeps to one line. */
.option .sub {
  align-items: flex-start;
}

.option .sub .mark {
  margin-top: 1px;
}

.option .sub-text:not(.path) {
  white-space: normal;
}

.actions {
  flex-shrink: 0;
  display: inline-flex;
  gap: 8px;
}

/* Where the command is, in the face paths are in: too long for the row, its folders give way
   and its end stays. */
.path {
  display: flex;
  font-family: var(--font-mono);
}

.path-head {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.path-tail {
  flex: none;
}

/* As Settings' selects, at the row buttons' height. */
select {
  flex-shrink: 0;
  height: 30px;
  max-width: 220px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
</style>
