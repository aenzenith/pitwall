<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { accountName, accountsOf } from "../../lib/accounts";
import { agentName, agentsOn } from "../../lib/agents";
import { useBackdropClose } from "../../lib/dialog";
import { useReturnFocus } from "../../lib/dialogFocus";
import { language, t } from "../../lib/i18n";
import { isWindows } from "../../lib/platform";
import { blankLap, hasAgent, hasScript, KIND_HINTS, KIND_LABELS, KINDS, lapAgent, lapOf, refusalText, type Lap, type LapKind, type LapPlace, type LapView } from "../../lib/laps";
import { CODEX_MODE_LABELS, CODEX_MODELS, CODEX_MODES, MODE_LABELS, MODEL_LABELS, MODELS, MODES } from "../../lib/start";
import { api, snapshot } from "../../lib/store";
import type { Agent, ClaudeMode, ClaudeModel, CodexMode, CodexModel } from "../../lib/types";
import LapAdvanced from "./LapAdvanced.vue";
import LapDeleteDialog from "./LapDeleteDialog.vue";
import WhenField from "./WhenField.vue";

/**
 * Yeni iş and Düzenle (TN4): nearly the whole window, a new job's too (the user's word,
 * 2026-11-06: no small first step). Left the basics (Ad, Tür, Nerede, Ajan, Betik, Prompt, Ne
 * zaman with its next moments; a kind's hint on its button), right Gelişmiş. Kaydet saves it (the core checks it whole and
 * its refusal shows here), Şimdi dene saves it and runs it once now, İşi sil asks once; Vazgeç,
 * Esc or a click outside leaves it as it was.
 */
const props = defineProps<{ lap: LapView | null; place: LapPlace | null; elsewhere: boolean }>();
const emit = defineEmits<{ saved: [id: string]; deleted: [id: string]; close: [] }>();

const dialog = ref<HTMLDialogElement | null>(null);
const nameInput = ref<HTMLInputElement | null>(null);
const backdrop = useBackdropClose(dialog);
useReturnFocus();

const projects = computed(() => [...(snapshot.value?.projects ?? [])].sort((a, b) => a.name.localeCompare(b.name, language.value, { sensitivity: "base" })));

function firstPlace(): LapPlace {
  if (props.place) return props.place;
  const first = projects.value[0];
  return first ? { kind: "project", path: first.path } : { kind: "folder", path: "" };
}

const draft = reactive<Lap>(props.lap ? lapOf(props.lap) : blankLap(firstPlace()));

onMounted(() => {
  dialog.value?.showModal();
  if (!props.lap) nameInput.value?.focus();
  else {
    if (!dialog.value?.hasAttribute("tabindex")) dialog.value?.setAttribute("tabindex", "-1");
    dialog.value?.focus({ preventScroll: true });
  }
});

function pickKind(kind: LapKind): void {
  draft.kind = kind;
  if (hasScript(kind) && !draft.script) draft.script = defaultScript();
}

/* ---------- Nerede ---------- */

/** The folder of a job in one, as shown (`~/…`). */
const folderShown = ref(props.lap?.place.kind === "folder" ? props.lap.folder : "");

const project = computed(() => (draft.place.kind === "project" ? (snapshot.value?.projects.find((p) => p.path === draft.place.path) ?? null) : null));

const placeValue = computed(() => (draft.place.kind === "project" ? `p:${draft.place.path}` : draft.place.path ? `f:${draft.place.path}` : ""));

/** Whether Git's gate can hold: a project's, or the job's own folder as the core last found it. */
const git = computed<boolean | null>(() => {
  if (draft.place.kind === "project") return project.value ? project.value.git !== null : null;
  return props.lap && props.lap.place.kind === "folder" && props.lap.place.path === draft.place.path ? props.lap.git : null;
});

const picking = ref(false);

async function setPlace(event: Event): Promise<void> {
  const select = event.target as HTMLSelectElement;
  const value = select.value;
  if (value === "pick") {
    select.value = placeValue.value;
    if (picking.value) return;
    picking.value = true;
    error.value = "";
    try {
      const picked = await api.lapPickFolder();
      if (picked) {
        movePlace({ kind: "folder", path: picked.path });
        folderShown.value = picked.shown;
      }
    } catch (reason) {
      error.value = refusalText(reason);
    } finally {
      picking.value = false;
    }
    return;
  }
  if (value.startsWith("p:")) movePlace({ kind: "project", path: value.slice(2) });
}

/** Another place: a saved command of the last project is written out as its line. */
function movePlace(to: LapPlace): void {
  const script = draft.script;
  if (script?.kind === "command" && (to.kind !== "project" || to.path !== draft.place.path)) {
    const line = project.value?.commands.find((c) => c.id === script.id)?.command ?? "";
    draft.script = { kind: "text", text: line };
  }
  draft.place = to;
}

/* ---------- Ajan ---------- */

const agent = computed<Agent>(() => lapAgent(draft));

/** The agents on, and the job's own when it is off. */
const agentChoices = computed<Agent[]>(() => (agentsOn.value.includes(agent.value) ? agentsOn.value : [...agentsOn.value, agent.value]));

function setAgent(to: Agent): void {
  if (to === agent.value) return;
  const start = { ...draft.start };
  if (to === "claude") delete start.agent;
  else start.agent = to;
  draft.start = start;
  draft.account = null;
}

const modelValue = computed(() => (agent.value === "codex" ? (draft.start.codex?.model ?? "") : (draft.start.model ?? "")));
// A job that runs in the background reads only (Claude with its reading tools alone, Codex in its
// read-only sandbox), as it always does on Windows: a mode would change nothing there.
const background = computed(() => draft.where === "background" || isWindows);
const modeValue = computed(() => (agent.value === "codex" ? (draft.start.codex?.mode ?? "normal") : (draft.start.mode ?? "normal")));

function setModel(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  const start = { ...draft.start };
  if (agent.value === "codex") {
    const codex = { ...start.codex };
    if (value) codex.model = value as CodexModel;
    else delete codex.model;
    start.codex = codex;
  } else if (value) start.model = value as ClaudeModel;
  else delete start.model;
  draft.start = start;
}

function setMode(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  const start = { ...draft.start };
  if (agent.value === "codex") {
    const codex = { ...start.codex };
    if (value !== "normal") codex.mode = value as CodexMode;
    else delete codex.mode;
    start.codex = codex;
  } else if (value !== "normal") start.mode = value as ClaudeMode;
  else delete start.mode;
  draft.start = start;
}

const models = computed<Array<{ id: string; label: string }>>(() =>
  agent.value === "codex"
    ? [{ id: "", label: t("start.codexModel.auto") }, ...CODEX_MODELS.map((id) => ({ id, label: id }))]
    : [{ id: "", label: t("start.model.auto") }, ...MODELS.map((id) => ({ id, label: t(MODEL_LABELS[id]) }))],
);

const modes = computed<Array<{ id: string; label: string }>>(() =>
  agent.value === "codex" ? CODEX_MODES.map((id) => ({ id, label: t(CODEX_MODE_LABELS[id]) })) : MODES.map((id) => ({ id, label: t(MODE_LABELS[id]) })),
);

/** The agent's accounts, while it has more than the main one: none picked is the one in use. */
const accounts = computed(() => accountsOf(agent.value));

/* ---------- Betik ---------- */

/** A project's saved commands a job may run: never one marked "ask first". */
const commands = computed(() => (project.value?.commands ?? []).filter((command) => !command.confirm));

function defaultScript(): Lap["script"] {
  const first = draft.place.kind === "project" ? commands.value[0] : undefined;
  return first ? { kind: "command", id: first.id } : { kind: "text", text: "" };
}

const scriptValue = computed(() => (draft.script?.kind === "command" ? `c:${draft.script.id}` : "text"));

/** The job names a command that isn't offered (gone, or marked "ask first" since). */
const scriptMissing = computed(() => draft.script?.kind === "command" && !commands.value.some((c) => draft.script?.kind === "command" && c.id === draft.script.id));

const commandLine = computed(() => {
  const script = draft.script;
  if (script?.kind !== "command") return "";
  return commands.value.find((c) => c.id === script.id)?.command ?? "";
});

function setScript(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  if (value === "text") draft.script = { kind: "text", text: commandLine.value };
  else if (value.startsWith("c:")) draft.script = { kind: "command", id: value.slice(2) };
}

const scriptText = computed({
  get: () => (draft.script?.kind === "text" ? draft.script.text : ""),
  set: (text: string) => (draft.script = { kind: "text", text }),
});

/* ---------- saving ---------- */

const busy = ref<"save" | "try" | null>(null);
const error = ref("");
const deleting = ref(false);

/** The job as it goes to the core: what its kind has no use for left out. */
function built(): Lap {
  const lap: Lap = JSON.parse(JSON.stringify(draft));
  lap.name = lap.name.trim();
  if (!hasAgent(lap.kind)) {
    lap.start = {};
    lap.prompt = "";
    lap.fuel = null;
    lap.carry = false;
    lap.account = null;
  }
  if (!hasScript(lap.kind)) lap.script = null;
  else if (!lap.script) lap.script = { kind: "text", text: "" };
  if (lap.kind !== "scriptAgent") lap.gate.script = "none";
  if (lap.file !== null && !lap.file.trim()) lap.file = null;
  return lap;
}

async function save(run: boolean): Promise<void> {
  if (busy.value) return;
  if (!draft.name.trim()) {
    error.value = t("laps.edit.needName");
    void nextTick(() => nameInput.value?.focus());
    return;
  }
  busy.value = run ? "try" : "save";
  error.value = "";
  try {
    const saved = await api.lapSave(built());
    // Saved: a run that couldn't start leaves the dialog open, and its next Kaydet is this job's.
    draft.id = saved.id;
    if (run) await api.lapRunNow(saved.id);
    emit("saved", saved.id);
    dialog.value?.close();
  } catch (reason) {
    error.value = refusalText(reason);
  } finally {
    busy.value = null;
  }
}

function onDeleted(id: string): void {
  emit("deleted", id);
  dialog.value?.close();
}

const title = computed(() => (props.lap ? t("laps.edit.title") : t("laps.edit.new")));
</script>

<template>
  <dialog
    ref="dialog"
    class="dialog edit"
    :aria-label="title"
    @close="emit('close')"
    @pointerdown="backdrop.down"
    @click="backdrop.click"
  >
    <div class="dialog-head">
      <span class="dialog-title">{{ title }}</span>
      <span v-if="lap" class="subtitle">{{ lap.name }}</span>
      <span class="grow"></span>
      <button type="button" class="icon-button" :aria-label="t('common.close')" :title="t('common.close')" @click="dialog?.close()">
        <Icon name="close" :size="14" />
      </button>
    </div>

    <!-- The whole job. -->
    <div class="columns">
      <div class="basics">
        <label class="field">
          <span class="label">{{ t("laps.edit.name") }}</span>
          <input ref="nameInput" v-model="draft.name" class="input" maxlength="200" spellcheck="false" :placeholder="t('laps.edit.namePlaceholder')" />
        </label>

        <div class="field">
          <span class="label">{{ t("laps.fact.kind") }}</span>
          <div class="seg" role="radiogroup" :aria-label="t('laps.fact.kind')">
            <button
              v-for="kind in KINDS"
              :key="kind"
              type="button"
              role="radio"
              :aria-checked="draft.kind === kind"
              :class="{ on: draft.kind === kind }"
              :title="t(KIND_HINTS[kind])"
              @click="pickKind(kind)"
            >
              <Icon :name="kind === 'agent' ? 'zap' : 'terminal'" :size="12" />{{ t(KIND_LABELS[kind]) }}
            </button>
          </div>
        </div>

        <label class="field">
          <span class="label">{{ t("laps.fact.where") }}</span>
          <span class="place-row">
            <select class="select grow" :value="placeValue" :disabled="picking" @change="setPlace">
              <option v-if="!placeValue" value="" disabled>{{ t("laps.edit.place.none") }}</option>
              <option v-for="one in projects" :key="one.path" :value="`p:${one.path}`">{{ one.name }}</option>
              <option v-if="draft.place.kind === 'folder' && draft.place.path" :value="`f:${draft.place.path}`">{{ folderShown || draft.place.path }}</option>
              <option value="pick">{{ t("laps.edit.place.pick") }}</option>
            </select>
            <Spinner v-if="picking" :size="12" />
          </span>
          <span class="hint">{{ t(draft.place.kind === "project" ? "laps.edit.place.project" : "laps.edit.place.folder") }}</span>
        </label>

        <div v-if="hasAgent(draft.kind)" class="field">
          <span class="label">{{ t("laps.edit.agent") }}</span>
          <div class="agent-row">
            <div v-if="agentChoices.length > 1" class="seg" role="radiogroup" :aria-label="t('laps.edit.agent')">
              <button v-for="one in agentChoices" :key="one" type="button" role="radio" :aria-checked="agent === one" :class="{ on: agent === one }" @click="setAgent(one)">
                <AgentMark :agent="one" :size="11" />{{ agentName(one) }}
              </button>
            </div>
            <span v-else class="agent-alone"><AgentMark :agent="agent" :size="12" />{{ agentName(agent) }}</span>
            <select class="select" :value="modelValue" :aria-label="t('laps.edit.model')" @change="setModel">
              <option v-for="one in models" :key="one.id" :value="one.id">{{ one.label }}</option>
            </select>
            <select v-if="!background" class="select" :value="modeValue" :aria-label="t('laps.edit.mode')" @change="setMode">
              <option v-for="one in modes" :key="one.id" :value="one.id">{{ one.label }}</option>
            </select>
            <select v-if="accounts.length > 1" v-model="draft.account" class="select" :aria-label="t('laps.edit.account')">
              <option :value="null">{{ t("laps.edit.accountInUse") }}</option>
              <option v-for="one in accounts" :key="one.id" :value="one.id">{{ accountName(one) }}</option>
            </select>
          </div>
        </div>

        <div v-if="hasScript(draft.kind)" class="field">
          <span class="label">{{ t(draft.kind === "scriptAgent" ? "laps.edit.scriptFirst" : "laps.edit.script") }}</span>
          <div v-if="draft.place.kind === 'project'" class="script-row">
            <select class="select command-select" :value="scriptValue" :aria-label="t('laps.edit.script')" @change="setScript">
              <option v-if="scriptMissing && draft.script?.kind === 'command'" :value="`c:${draft.script.id}`" disabled>{{ t("laps.edit.script.missing") }}</option>
              <option v-for="one in commands" :key="one.id" :value="`c:${one.id}`">{{ t("laps.edit.script.saved", { name: one.name }) }}</option>
              <option value="text">{{ t("laps.edit.script.write") }}</option>
            </select>
            <span v-if="draft.script?.kind === 'command' && commandLine" class="input mono command-line" :title="commandLine">{{ commandLine }}</span>
          </div>
          <textarea v-if="draft.place.kind !== 'project' || draft.script?.kind !== 'command'" v-model="scriptText" class="textarea mono" rows="3" spellcheck="false" :aria-label="t('laps.edit.script')"></textarea>
          <span class="hint">
            {{ t(draft.kind === "scriptAgent" ? "laps.edit.script.hint" : "laps.edit.script.hint.alone") }}
            <template v-if="draft.place.kind === 'project'">{{ t("laps.edit.script.askFirst") }}</template>
          </span>
        </div>

        <label v-if="hasAgent(draft.kind)" class="field">
          <span class="label">{{ t("laps.edit.prompt") }}</span>
          <textarea v-model="draft.prompt" class="textarea" rows="3" :placeholder="t('laps.edit.promptPlaceholder')"></textarea>
        </label>

        <div class="field">
          <span class="label">{{ t("laps.fact.when") }}</span>
          <WhenField v-model:when="draft.when" v-model:weekdays-off="draft.weekdaysOff" :kind="draft.kind" :agent="agent" :account="draft.account" />
        </div>
      </div>

      <LapAdvanced :lap="draft" :agent="agent" :git="git" />
    </div>

    <div class="dialog-foot">
      <button v-if="lap" type="button" class="control small danger" :disabled="busy !== null" @click="deleting = true">{{ t("laps.delete") }}</button>
      <p v-if="error" class="error grow foot-error" role="alert" :title="error">{{ error }}</p>
      <span v-else class="grow"></span>
      <button type="button" class="control small" :disabled="busy !== null || elsewhere" :aria-busy="busy === 'try'" @click="save(true)">
        <span :class="['label-row', { hidden: busy === 'try' }]"><Icon name="play" :size="11" />{{ t("laps.edit.tryNow") }}</span>
        <Spinner v-if="busy === 'try'" class="over" :size="12" />
      </button>
      <button type="button" class="control small" @click="dialog?.close()">{{ t("common.cancel") }}</button>
      <button type="button" class="control small primary" :disabled="busy !== null" :aria-busy="busy === 'save'" @click="save(false)">
        <span :class="{ hidden: busy === 'save' }">{{ t("common.save") }}</span>
        <Spinner v-if="busy === 'save'" class="over" :size="12" />
      </button>
    </div>

    <LapDeleteDialog
      v-if="deleting && lap"
      :id="lap.id"
      :name="lap.name"
      :running="lap.running !== null"
      @deleted="onDeleted"
      @close="deleting = false"
    />
  </dialog>
</template>

<style scoped src="./laps.css"></style>

<style scoped>
.edit {
  width: min(1180px, calc(100vw - 48px));
  height: min(724px, calc(100vh - 48px));
}

.subtitle {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  color: var(--text-faint);
}

.columns {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  gap: 26px;
  padding: 16px 20px;
  overflow: hidden;
}

.basics {
  /* It scrolls, so it cuts what is drawn outside it: a field's focus ring (the system's, about
     3 px out) at its edges. Room for the ring inside it, taken back outside, leaves the fields
     where they were. */
  flex: 0 1 528px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow-y: auto;
  margin: -4px;
  padding: 4px;
}

.place-row,
.agent-row,
.script-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.agent-row {
  flex-wrap: wrap;
}

.agent-alone {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.command-select {
  flex: 0 1 230px;
}

.command-line {
  flex: 1 1 auto;
  display: flex;
  align-items: center;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-soft);
  font-size: 11.5px;
}

.textarea.mono {
  font-size: 12px;
}

.foot-error {
  padding-left: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.label-row {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

@media (max-width: 860px) {
  .columns {
    flex-direction: column;
    overflow-y: auto;
  }

  .basics {
    flex: none;
    overflow: visible;
  }
}
</style>
