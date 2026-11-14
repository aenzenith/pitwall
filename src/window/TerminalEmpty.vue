<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import AgentMark from "../components/AgentMark.vue";
import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { agentName, agentOn } from "../lib/agents";
import { startChat } from "../lib/chat/open";
import { t, type Key } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import { markOpened } from "../lib/sessionsMeta";
import { SHELL_LABELS } from "../lib/shells";
import { CODEX_MODE_LABELS, CODEX_MODELS, CODEX_MODES, codexModelLabel, MODE_LABELS, MODELS, MODES, modelLabel, projectChoice, projectCodexChoice } from "../lib/start";
import { api } from "../lib/store";
import { draftOf, EMPTY_KINDS, emptyPick, keepDraft, kindOn, lastSeen, pickEmpty, type EmptyKind, type EmptyStart } from "../lib/terminalEmpty";
import type { AgentStartChoice, CodexChoice, PastSession, Project, Shell, ShellChoice, StartChoice } from "../lib/types";

/**
 * The terminal panel with nothing open (window/TerminalPanel), and an empty pane (TerminalPanes):
 * pick what opens (a shell, a chat, Claude's or Codex's tab), type its first job and press Return.
 * Blank, Return opens it empty. The pick is remembered (lib/terminalEmpty); the text stays in the
 * page's memory until it opens. A chip picks the agent's model and mode for this one start, the
 * project's own staying as they are. The panel's box (not a pane's) also offers the picked agent's
 * last session of the project, read as the terminals' history reads it, to go on with. Too little
 * room (a small pane, a narrow card): the four as buttons, opening empty.
 */
const props = defineProps<{
  project: Project;
  /** The box's key for what is typed in it: the project's path, an empty pane's with its key. */
  draftKey: string;
  /** The panel is opening something already. */
  opening: boolean;
  /** An empty pane's: no last session, and its head only while the pane has room for it. */
  pane?: boolean;
  /** The shells installed, on Windows (the shell's chip offers them, an administrator's too); none elsewhere. */
  shells?: Shell[];
  /** Opens what was asked; true once a tab opened. */
  launch: (start: EmptyStart) => Promise<boolean>;
}>();

const root = ref<HTMLElement | null>(null);
const field = ref<HTMLTextAreaElement | null>(null);

const kinds = computed(() => EMPTY_KINDS.filter(kindOn));
const picked = computed(() => emptyPick.value);

/** A kind's name: a shell's and a chat's in the app's words, an agent's its brand. */
function label(kind: EmptyKind): string {
  if (kind === "shell") return t("terminal.empty.shell");
  if (kind === "chat") return t("terminal.empty.chat");
  return agentName(kind);
}

const PLACEHOLDERS: Record<EmptyKind, Key> = {
  shell: "terminal.empty.placeholder.shell",
  chat: "terminal.empty.placeholder.chat",
  claude: "terminal.empty.placeholder",
  codex: "terminal.empty.placeholder.codex",
};

/* ---------- what is typed ---------- */

const draft = ref(draftOf(props.draftKey));
watch(
  () => props.draftKey,
  (key) => {
    draft.value = draftOf(key);
    void nextTick(fit);
  },
);
watch(draft, (text) => {
  keepDraft(props.draftKey, text);
  fit();
});

/** The field grows with its text, up to a few lines, then scrolls. */
function fit(): void {
  const el = field.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
}

/* ---------- the start a chip picks, for this one start ---------- */

/** Picked on a chip, by project and agent (and on Windows the shell); gone once it started, or
 * the project's own came back. */
type Chosen = { claude: StartChoice | null; codex: CodexChoice | null; shell: ShellChoice | null };
const NONE_CHOSEN: Chosen = { claude: null, codex: null, shell: null };
const chosen = ref<Chosen>({ ...NONE_CHOSEN });
watch(
  () => props.project.path,
  () => (chosen.value = { ...NONE_CHOSEN }),
);

const claudeStart = computed<StartChoice>(() => chosen.value.claude ?? projectChoice(props.project));
const codexStart = computed<CodexChoice>(() => chosen.value.codex ?? projectCodexChoice(props.project));
/** A new tab's shell when none was picked: the Command Prompt, as "+" has it checked. */
const shellStart = computed<ShellChoice>(() => chosen.value.shell ?? { shell: "cmd", elevated: false });
/** The shell's chip shows only where there are shells to pick (Windows). */
const shellChip = computed(() => (props.shells?.length ?? 0) > 0);

const chipText = computed(() => {
  if (picked.value === "shell") return t(SHELL_LABELS[shellStart.value.shell]) + (shellStart.value.elevated ? ` · ${t("terminal.asAdmin")}` : "");
  if (picked.value === "codex") return `${codexModelLabel(codexStart.value.model)} · ${t(CODEX_MODE_LABELS[codexStart.value.mode])}`;
  return `${modelLabel(claudeStart.value.model)} · ${t(MODE_LABELS[claudeStart.value.mode])}`;
});

const menu = useNativeMenu();

function chipMenu(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const at = { clientX: box.left, clientY: box.bottom + 4 };
  let entries: MenuEntry[];
  if (picked.value === "shell") {
    const now = shellStart.value;
    const item = (shell: Shell, elevated: boolean): MenuEntry => ({
      text: t(SHELL_LABELS[shell]),
      checked: now.shell === shell && now.elevated === elevated,
      action: () => void (chosen.value = { ...chosen.value, shell: { shell, elevated } }),
    });
    const shells = props.shells ?? [];
    entries = [...shells.map((shell) => item(shell, false)), "separator", { text: t("terminal.asAdmin"), enabled: false }, ...shells.map((shell) => item(shell, true))];
  } else if (picked.value === "codex") {
    const now = codexStart.value;
    const set = (next: Partial<CodexChoice>): void => void (chosen.value = { ...chosen.value, codex: { ...now, ...next } });
    entries = [
      { text: t("start.model"), enabled: false },
      ...[null, ...CODEX_MODELS].map((model) => ({ text: codexModelLabel(model), checked: now.model === model, action: () => set({ model }) })),
      "separator",
      { text: t("start.mode"), enabled: false },
      ...CODEX_MODES.map((mode) => ({ text: t(CODEX_MODE_LABELS[mode]), checked: now.mode === mode, action: () => set({ mode }) })),
    ];
  } else {
    const now = claudeStart.value;
    const set = (next: Partial<StartChoice>): void => void (chosen.value = { ...chosen.value, claude: { ...now, ...next } });
    entries = [
      { text: t("start.model"), enabled: false },
      ...[null, ...MODELS].map((model) => ({ text: modelLabel(model), checked: now.model === model, action: () => set({ model }) })),
      "separator",
      { text: t("start.mode"), enabled: false },
      ...MODES.map((mode) => ({ text: t(MODE_LABELS[mode]), checked: now.mode === mode, action: () => set({ mode }) })),
    ];
  }
  void menu.popup(at, entries).then(() => field.value?.focus());
}

/* ---------- opening ---------- */

const busy = ref(false);
/** The last session going on (`resume`): nothing else opens meanwhile. */
const resuming = ref(false);
const blocked = computed(() => busy.value || resuming.value || props.opening);

/** A click picks; a double click (on the picker, `@dblclick`) opens that kind empty at once. */
function pick(kind: EmptyKind): void {
  pickEmpty(kind);
  field.value?.focus();
}

/** ←/→ on the picker move the pick, as a radio group's arrows do. */
function pickerKeys(event: KeyboardEvent): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  event.preventDefault();
  const list = kinds.value;
  const at = list.indexOf(picked.value);
  const next = list[(at + (event.key === "ArrowRight" ? 1 : list.length - 1)) % list.length];
  if (!next) return;
  pickEmpty(next);
  void nextTick(() => root.value?.querySelector<HTMLElement>(".opt.on")?.focus());
}

function startOf(kind: EmptyKind): AgentStartChoice | null {
  if (kind === "codex") return chosen.value.codex ?? projectCodexChoice(props.project);
  if (kind === "claude" || kind === "chat") return chosen.value.claude;
  return null;
}

async function open(kind: EmptyKind, text: string): Promise<void> {
  if (blocked.value || !kindOn(kind)) return;
  busy.value = true;
  // The field empties as it is sent, in the page's memory too (the box is gone once a tab opens,
  // and its watch with it); it gets the text back only when nothing opened.
  const key = props.draftKey;
  if (text) {
    draft.value = "";
    keepDraft(key, "");
  }
  try {
    const opened = await props.launch({ kind, text: text.trim() ? text : "", start: startOf(kind), shell: kind === "shell" ? chosen.value.shell : null });
    if (!opened) {
      // Its text comes back, before what was typed meanwhile.
      if (text) {
        const typed = draftOf(key);
        const back = typed ? `${text}\n${typed}` : text;
        keepDraft(key, back);
        if (props.draftKey === key) draft.value = back;
      }
      return;
    }
    chosen.value = { ...NONE_CHOSEN };
  } finally {
    busy.value = false;
  }
}

function submit(): void {
  void open(picked.value, draft.value);
}

/** Return opens; ⇧Return a new line; a key that composes (an input method) is its own. */
function fieldKeys(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  submit();
}

/* ---------- the last session, to go on with ---------- */

const last = ref<PastSession | null>(null);
/** The history is still being read and nothing of it was seen before: a spinner in the row. */
const lastLoading = ref(false);

/** Its agent's: Claude's for a chat or Claude's tab, Codex's; none for a shell. */
const lastAgent = computed(() => (picked.value === "codex" ? "codex" : picked.value === "shell" ? null : "claude"));

let asked = 0;
async function loadLast(): Promise<void> {
  const ask = ++asked;
  const agent = lastAgent.value;
  if (props.pane || !agent || !agentOn(agent)) {
    last.value = null;
    lastLoading.value = false;
    return;
  }
  // Reading the history takes a moment (Codex's most): what was found last shows meanwhile.
  const key = `${agent}\n${props.project.path}`;
  const seen = lastSeen.get(key);
  last.value = seen ?? null;
  lastLoading.value = seen === undefined;
  try {
    const list = agent === "codex" ? await api.codexPast(props.project.path) : await api.claudePast(props.project.path);
    if (ask !== asked) return;
    const found = list.reduce<PastSession | null>((best, each) => ((each.at ?? 0) > (best?.at ?? -1) ? each : best), null);
    lastSeen.set(key, found);
    last.value = found;
  } catch {
    if (ask === asked && seen === undefined) last.value = null;
  } finally {
    if (ask === asked) lastLoading.value = false;
  }
}

// Each source on its own: the project comes as a new object with every snapshot, its path the same.
watch([() => props.project.path, lastAgent, () => props.pane], loadLast, { immediate: true });

async function resume(): Promise<void> {
  const session = last.value;
  if (!session || blocked.value) return;
  resuming.value = true;
  try {
    if (session.agent === "codex") await api.resumeCodex(props.project.path, session.id);
    else if (picked.value === "chat") await startChat(props.project.path, { session: session.id });
    else await api.resumeClaude(props.project.path, session.id);
    void markOpened(session.id);
  } catch {
    // The terminals' history says what went wrong with a session; here it simply doesn't open.
  } finally {
    resuming.value = false;
  }
}

/* ---------- room ---------- */

/** Too little room for the box: the four as buttons. */
const tight = ref(false);
/** A pane too short for the project's name and the line under it: the picker and box alone. */
const bare = ref(false);
const PANE_HEAD = 250;
let watcher: ResizeObserver | null = null;

onMounted(() => {
  fit();
  if (!root.value) return;
  watcher = new ResizeObserver(([entry]) => {
    const { width, height } = entry.contentRect;
    tight.value = width < 380 || height < (props.pane ? 210 : 250);
    bare.value = !!props.pane && height < PANE_HEAD;
  });
  watcher.observe(root.value);
});

onBeforeUnmount(() => watcher?.disconnect());

defineExpose({ focus: () => (tight.value ? root.value?.querySelector<HTMLElement>(".mini")?.focus() : field.value?.focus()) });
</script>

<template>
  <div ref="root" :class="['te', { pane, tight }]">
    <template v-if="tight">
      <div class="minis">
        <button v-for="kind in kinds" :key="kind" type="button" class="mini" :disabled="blocked" :title="label(kind)" :aria-label="label(kind)" @click="open(kind, '')">
          <Icon v-if="kind === 'shell'" name="terminal" :size="14" />
          <Icon v-else-if="kind === 'chat'" name="chat" :size="14" />
          <AgentMark v-else :agent="kind" :size="13" />
        </button>
      </div>
    </template>
    <template v-else>
      <div v-if="!bare" class="head">
        <span class="name">{{ project.name }}</span>
        <span class="lead">{{ t("terminal.empty.lead") }}</span>
      </div>
      <div class="picker" role="radiogroup" :aria-label="t('terminal.empty.kind')" @keydown="pickerKeys">
        <button
          v-for="kind in kinds"
          :key="kind"
          type="button"
          role="radio"
          :aria-checked="kind === picked"
          :tabindex="kind === picked ? 0 : -1"
          :class="['opt', { on: kind === picked }]"
          @click="pick(kind)"
          @dblclick="open(kind, '')"
        >
          <Icon v-if="kind === 'shell'" name="terminal" :size="13" />
          <Icon v-else-if="kind === 'chat'" name="chat" :size="13" />
          <AgentMark v-else :agent="kind" :size="12" />
          {{ label(kind) }}
        </button>
      </div>
      <div :class="['box', { mono: picked === 'shell' }]" @pointerdown.self="field?.focus()">
        <textarea
          ref="field"
          v-model="draft"
          class="input"
          rows="2"
          spellcheck="false"
          :placeholder="t(PLACEHOLDERS[picked])"
          :aria-label="t(PLACEHOLDERS[picked])"
          @keydown="fieldKeys"
        ></textarea>
        <div class="row">
          <button
            v-if="picked !== 'shell' || shellChip"
            type="button"
            class="chip"
            :title="picked === 'shell' ? t('terminal.empty.shellTitle') : t('terminal.empty.startTitle')"
            @click="chipMenu"
          >
            <Icon v-if="picked === 'shell' && shellStart.elevated" name="shield" :size="11" class="warn" />
            <span class="lbl">{{ chipText }}</span>
            <Icon name="chevron-down" :size="12" />
          </button>
          <span class="grow"></span>
          <button
            type="button"
            class="send"
            :disabled="blocked"
            :title="draft.trim() ? t('terminal.empty.start') : t('terminal.empty.startEmpty')"
            :aria-label="draft.trim() ? t('terminal.empty.start') : t('terminal.empty.startEmpty')"
            @click="submit"
          >
            <Spinner v-if="busy" :size="12" />
            <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M5.5 11.5 12 5l6.5 6.5" /></svg>
          </button>
        </div>
      </div>
      <!-- Always there, empty for a shell or no session, so the box doesn't move as the kind changes. -->
      <div v-if="!pane" class="last" :aria-hidden="!last && !lastLoading">
        <template v-if="last">
          <AgentMark :agent="last.agent" :size="12" />
          <span class="tag">{{ t("terminal.empty.last") }}</span>
          <span class="title">{{ last.title ?? t("terminal.historyUntitled") }}</span>
          <button type="button" class="resume" :disabled="blocked" @click="resume">
            <Spinner v-if="resuming" :size="10" />
            {{ t("terminal.empty.resume") }}
            <Icon v-if="!resuming" name="chevron" :size="11" />
          </button>
        </template>
        <template v-else-if="lastLoading && lastAgent">
          <AgentMark :agent="lastAgent" :size="12" />
          <span class="tag">{{ t("terminal.empty.last") }}</span>
          <Spinner :size="10" />
        </template>
      </div>
    </template>
  </div>
</template>

<style scoped>
.te {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 18px;
  padding: 24px;
  overflow: hidden;
  color: var(--text-muted);
}

.te.pane {
  position: static;
  flex: 1 1 auto;
  gap: 12px;
  padding: 14px;
}

.head {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  max-width: 100%;
}

.name {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 20px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-strong);
}

.lead {
  max-width: 100%;
  font-size: 13px;
  text-align: center;
  color: var(--text-muted);
}

.te.pane .name {
  font-size: 17px;
}

.te.pane .lead {
  font-size: 12.5px;
}

/* ---- what opens ---- */

.picker {
  display: inline-flex;
  flex-shrink: 0;
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--bg-seg);
}

.opt {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  padding: 0 13px;
  border: none;
  border-radius: 7px;
  background: none;
  font: inherit;
  font-size: 13px;
  color: var(--text-nav);
  white-space: nowrap;
}

.opt:hover:not(.on) {
  color: var(--text);
}

.opt.on {
  background: var(--bg-seg-on);
  color: var(--text-strong);
}

/* ---- the box ---- */

.box {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(640px, 100%);
  padding: 12px 12px 10px 14px;
  border: 1px solid var(--line-bold);
  border-radius: 14px;
  background: var(--bg-panel);
  cursor: text;
}

.box:focus-within {
  border-color: var(--line-lit);
}

.input {
  box-sizing: border-box;
  width: 100%;
  min-height: 44px;
  margin: 0;
  padding: 2px 2px 2px 3px;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  font-family: var(--font);
  font-size: 14px;
  /* In pixels, so a shell's smaller mono lines leave the box as tall as the others'. */
  line-height: 22px;
  color: var(--text);
}

.box.mono .input {
  font-family: var(--font-mono);
  font-size: 13px;
}

.input::placeholder {
  color: var(--text-faint);
}

.row {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: default;
}

.grow {
  flex: 1;
}

/* An administrator's shell picked: the shield in the warning's colour, as "+" shows it. */
.chip .warn {
  flex-shrink: 0;
  color: var(--warn);
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  height: 26px;
  padding: 0 9px 0 11px;
  border: none;
  border-radius: 13px;
  background: var(--bg-control-hover);
  font: inherit;
  font-size: 12.5px;
  color: var(--text);
  white-space: nowrap;
}

.chip:hover {
  background: color-mix(in srgb, var(--bg-control-hover) 86%, var(--text));
}

.chip .lbl {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.chip :deep(svg) {
  flex-shrink: 0;
  color: var(--text-subtle);
}

.send {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: var(--text-strong);
  color: var(--bg-app);
}

.send:hover:not(:disabled) {
  background: var(--text-lit);
}

.send:disabled {
  background: var(--bg-control-hover);
  color: var(--text-faint);
  cursor: default;
}

/* ---- the last session ---- */

.last {
  display: flex;
  align-items: center;
  gap: 7px;
  width: min(640px, 100%);
  min-width: 0;
  height: 22px;
  flex-shrink: 0;
  padding: 0 4px;
  font-size: 12.5px;
  color: var(--text-muted);
}

.last .tag {
  flex-shrink: 0;
  white-space: nowrap;
}

.last .title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text);
}

.resume {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  gap: 4px;
  height: 22px;
  padding: 0 6px;
  border: none;
  border-radius: 5px;
  background: none;
  font: inherit;
  color: var(--text-nav);
  white-space: nowrap;
}

.resume:hover:not(:disabled) {
  background: var(--bg-control-hover);
  color: var(--text);
}

.resume:disabled {
  opacity: 0.5;
  cursor: default;
}

/* ---- too little room ---- */

.minis {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 6px;
}

.mini {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 32px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text-soft);
}

.mini:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.mini:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
