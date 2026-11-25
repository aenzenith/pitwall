<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import AgentMark from "../components/AgentMark.vue";
import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { agentName, agentOn } from "../lib/agents";
import type { Shot } from "../lib/cardImages";
import { draftOf as composerDraft, readImage, takenFiles, type Draft } from "../lib/chat/composer";
import { draftId, draftInfo, draftModels, draftTake, registerDraft, type DraftStart } from "../lib/chat/draft";
import { chatSettings, startChat } from "../lib/chat/open";
import { contextFor } from "../lib/chat/store";
import { MODE_ICONS, type ChatDialogOpen, type ComposerApi, type TerminalRef } from "../lib/chat/ui";
import type { ChatControl, ChatOutgoing } from "../lib/chat/wire";
import { t, type Key } from "../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../lib/nativeMenu";
import { isWindows } from "../lib/platform";
import { markOpened } from "../lib/sessionsMeta";
import { SHELL_LABELS } from "../lib/shells";
import { CODEX_MODE_LABELS, CODEX_MODELS, CODEX_MODES, codexModelLabel, MODE_LABELS, MODELS, MODES, modelLabel, projectChoice, projectCodexChoice } from "../lib/start";
import { api, snapshot } from "../lib/store";
import { draftOf, EMPTY_KINDS, emptyPick, imagesOf, keepDraft, keepImages, kindOn, lastSeen, pickEmpty, type BoxImage, type EmptyKind, type EmptyStart } from "../lib/terminalEmpty";
import type { AgentStartChoice, ChatStartChoice, CodexChoice, PastSession, Project, Shell, ShellChoice, StartChoice } from "../lib/types";
import ImageLightbox from "./board/ImageLightbox.vue";
import ChatIcon from "./chat/ChatIcon.vue";
import Composer from "./chat/Composer.vue";
import ModeDialog from "./chat/dialogs/ModeDialog.vue";
import ModelDialog from "./chat/dialogs/ModelDialog.vue";

/**
 * The terminal panel with nothing open (window/TerminalPanel), and an empty pane (TerminalPanes):
 * pick what opens (a shell, a chat, Claude's or Codex's tab), type its first job and press Return.
 * Blank, Return opens it empty. The pick is remembered (lib/terminalEmpty); the text stays in the
 * page's memory until it opens.
 *
 * A chat's box is the chat's own composer (Composer.vue), every one of its ways as in a chat (the
 * user's word, 25 Nov): a draft chat of its own (lib/chat/draft) whose chips' menus set how the
 * chat starts, and whose first message, attachments and all, is the chat's. The others' box has
 * the composer's look; Claude's and Codex's take pictures pasted or dropped into it, which the core
 * writes for the agent as it starts. Its chips pick the agent's model and mode for this one start,
 * the project's own staying as they are. The panel's box (not a pane's) also offers the picked
 * agent's last session of the project, read as the terminals' history reads it, to go on with.
 * Too little room (a small pane, a narrow card): the four as buttons, opening empty.
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

const emit = defineEmits<{
  /** Settings › Claude › Sohbet (the chat's `/config`). */
  settings: [];
}>();

const root = ref<HTMLElement | null>(null);
const field = ref<HTMLTextAreaElement | null>(null);

const kinds = computed(() => EMPTY_KINDS.filter(kindOn));
const picked = computed(() => emptyPick.value);
/** Claude's or Codex's tab: the box takes pictures (not on Windows, where their tab takes no first job). */
const agentBox = computed(() => picked.value === "claude" || picked.value === "codex");
const takesImages = computed(() => agentBox.value && !isWindows);

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
const images = ref<BoxImage[]>(imagesOf(props.draftKey));
watch(
  () => props.draftKey,
  (key) => {
    draft.value = draftOf(key);
    images.value = imagesOf(key);
    void nextTick(fit);
  },
);
watch(draft, (text) => {
  keepDraft(props.draftKey, text);
  fit();
});
watch(images, (list) => keepImages(props.draftKey, list));

/** The field grows with its text, up to a few lines, then scrolls. */
function fit(): void {
  const el = field.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
}

/* ---------- pictures, for Claude and Codex ---------- */

/** As many as a board card holds: the core takes no more. */
const IMAGES_MOST = 12;
const reading = ref(0);
const notice = ref<string | null>(null);
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
let imageKeys = 0;

function say(text: string): void {
  notice.value = text;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => (notice.value = null), 7000);
}

async function addImage(file: File): Promise<void> {
  if (!file.type.startsWith("image/")) return;
  if (images.value.length + reading.value >= IMAGES_MOST) {
    say(t("chatInput.notice.imageTooMany", { count: IMAGES_MOST }));
    return;
  }
  const key = props.draftKey;
  reading.value++;
  try {
    const image = await readImage(file);
    const added: BoxImage = { key: `i${++imageKeys}`, mediaType: image.mediaType, data: image.data, name: file.name || "image", scaled: image.scaled };
    // The box may show another project's by now: the picture goes to the one it was pasted into.
    if (props.draftKey === key) images.value = [...images.value, added];
    else keepImages(key, [...imagesOf(key), added]);
  } catch {
    say(t("chatInput.notice.imageFailed"));
  } finally {
    reading.value--;
  }
}

function removeImage(key: string): void {
  images.value = images.value.filter((each) => each.key !== key);
  delete imageSizes[key];
}

function onPaste(event: ClipboardEvent): void {
  if (!takesImages.value) return;
  const files = takenFiles(event.clipboardData).filter((file) => file.type.startsWith("image/"));
  if (!files.length) return;
  event.preventDefault();
  for (const file of files) void addImage(file);
}

const dropping = ref(false);

function onDragOver(event: DragEvent): void {
  if (!takesImages.value || !event.dataTransfer?.types.includes("Files")) return;
  event.preventDefault();
  dropping.value = true;
}

function onDrop(event: DragEvent): void {
  dropping.value = false;
  if (!takesImages.value) return;
  const files = takenFiles(event.dataTransfer).filter((file) => file.type.startsWith("image/"));
  if (!files.length) return;
  event.preventDefault();
  for (const file of files) void addImage(file);
  field.value?.focus();
}

/** Each picture's size as it goes (`210×81`), read off its thumbnail once drawn. */
const imageSizes = reactive<Record<string, string>>({});
function sized(key: string, event: Event): void {
  const image = event.target as HTMLImageElement;
  imageSizes[key] = `${image.naturalWidth}×${image.naturalHeight}`;
}

/** Shown large as the chat's are (board/ImageLightbox), by its number. */
const shots = computed<Shot[]>(() => images.value.map((each, i) => ({ n: i + 1, src: `data:${each.mediaType};base64,${each.data}` })));
const shotShown = ref<number | null>(null);
const refsIn = ref<HTMLElement | null>(null);
function shotThumb(n: number): HTMLElement | null {
  return refsIn.value?.querySelector<HTMLElement>(`[data-n="${n}"] img`) ?? null;
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

const shellText = computed(() => t(SHELL_LABELS[shellStart.value.shell]) + (shellStart.value.elevated ? ` · ${t("terminal.asAdmin")}` : ""));
const modelText = computed(() => (picked.value === "codex" ? codexModelLabel(codexStart.value.model) : modelLabel(claudeStart.value.model)));
const modeText = computed(() => (picked.value === "codex" ? t(CODEX_MODE_LABELS[codexStart.value.mode]) : t(MODE_LABELS[claudeStart.value.mode])));
/** The mode's icon, as the chat's chip draws its own. */
const modeIcon = computed(() => {
  if (picked.value === "codex") return codexStart.value.mode === "readOnly" ? "ban" : MODE_ICONS.default;
  const mode = claudeStart.value.mode;
  return mode === "plan" ? MODE_ICONS.plan : mode === "acceptEdits" ? MODE_ICONS.acceptEdits : MODE_ICONS.default;
});

const menu = useNativeMenu();

function under(event: MouseEvent): { clientX: number; clientY: number } {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  return { clientX: box.left, clientY: box.bottom + 4 };
}

function shellMenu(event: MouseEvent): void {
  const now = shellStart.value;
  const item = (shell: Shell, elevated: boolean): MenuEntry => ({
    text: t(SHELL_LABELS[shell]),
    checked: now.shell === shell && now.elevated === elevated,
    action: () => void (chosen.value = { ...chosen.value, shell: { shell, elevated } }),
  });
  const shells = props.shells ?? [];
  const entries: MenuEntry[] = [...shells.map((shell) => item(shell, false)), "separator", { text: t("terminal.asAdmin"), enabled: false }, ...shells.map((shell) => item(shell, true))];
  void menu.popup(under(event), entries).then(() => field.value?.focus());
}

function modelMenu(event: MouseEvent): void {
  let entries: MenuEntry[];
  if (picked.value === "codex") {
    const now = codexStart.value;
    const set = (model: CodexChoice["model"]): void => void (chosen.value = { ...chosen.value, codex: { ...now, model } });
    entries = [{ text: t("start.model"), enabled: false }, ...[null, ...CODEX_MODELS].map((model) => ({ text: codexModelLabel(model), checked: now.model === model, action: () => set(model) }))];
  } else {
    const now = claudeStart.value;
    const set = (model: StartChoice["model"]): void => void (chosen.value = { ...chosen.value, claude: { ...now, model } });
    entries = [{ text: t("start.model"), enabled: false }, ...[null, ...MODELS].map((model) => ({ text: modelLabel(model), checked: now.model === model, action: () => set(model) }))];
  }
  void menu.popup(under(event), entries).then(() => field.value?.focus());
}

function modeMenu(event: MouseEvent): void {
  let entries: MenuEntry[];
  if (picked.value === "codex") {
    const now = codexStart.value;
    const set = (mode: CodexChoice["mode"]): void => void (chosen.value = { ...chosen.value, codex: { ...now, mode } });
    entries = [{ text: t("start.mode"), enabled: false }, ...CODEX_MODES.map((mode) => ({ text: t(CODEX_MODE_LABELS[mode]), checked: now.mode === mode, action: () => set(mode) }))];
  } else {
    const now = claudeStart.value;
    const set = (mode: StartChoice["mode"]): void => void (chosen.value = { ...chosen.value, claude: { ...now, mode } });
    entries = [{ text: t("start.mode"), enabled: false }, ...MODES.map((mode) => ({ text: t(MODE_LABELS[mode]), checked: now.mode === mode, action: () => set(mode) }))];
  }
  void menu.popup(under(event), entries).then(() => field.value?.focus());
}

/* ---------- the chat's box: the chat's composer ---------- */

const chatId = computed(() => draftId(props.draftKey));
const composer = ref<(InstanceType<typeof Composer> & ComposerApi) | null>(null);

/** What the chat's chips picked, by box; the rest is how the project's chat starts: its model, and
 * its mode where it names one, else the chat settings' (as the core starts one, `with_mode`). */
const chatPicks = reactive(new Map<number, Partial<DraftStart>>());
const chatDefault = computed<DraftStart>(() => {
  const own = projectChoice(props.project);
  const mode = own.mode === "plan" ? "plan" : own.mode === "acceptEdits" ? "acceptEdits" : chatSettings.value.mode;
  return { model: own.model, mode, effort: null };
});
const chatStart = computed<DraftStart>(() => ({ ...chatDefault.value, ...chatPicks.get(chatId.value) }));
const chatInfo = computed(() => draftInfo(props.project.path, chatStart.value));
/** The project's other tabs, which the composer offers as context (as a chat's own does). */
const chatTerminals = computed<TerminalRef[]>(() => {
  const project = snapshot.value?.projects.find((each) => each.path === props.project.path);
  return (project?.terminals ?? []).filter((view) => view.kind !== "chat").map((view) => ({ id: view.id, name: view.name }));
});
const chatContext = computed(() => contextFor(props.project.path));

watch(
  chatId,
  (id, before) => {
    if (before !== undefined) registerDraft(before, null);
    registerDraft(id, {
      start: () => chatStart.value,
      models: () => draftModels(props.project.path),
      set: (next) => void chatPicks.set(id, { ...chatPicks.get(id), ...next }),
    });
  },
  { immediate: true },
);

/** The model's or the mode's menu over its chip, as a chat's. */
const chatMenu = ref<ChatDialogOpen | null>(null);
function chatDialog(open: ChatDialogOpen): void {
  if (open.kind !== "model" && open.kind !== "mode") return;
  chatMenu.value = !open.arg && chatMenu.value?.kind === open.kind ? null : open;
}

function chatControl(control: ChatControl, then?: ChatOutgoing): void {
  draftTake(chatId.value, control);
  if (then) void openChatWith(then, null);
}

/** The start the chat is asked for: none while nothing was picked (the core starts it as shown). */
function chatChoice(): ChatStartChoice | null {
  const picks = chatPicks.get(chatId.value);
  if (!picks || !Object.keys(picks).length) return null;
  const start = chatStart.value;
  return { model: start.model, mode: start.mode as ChatStartChoice["mode"], effort: start.effort as ChatStartChoice["effort"] };
}

/** Opens the chat with its first message, or with a line it runs once ready. Called as the
 * composer sends, before it empties its draft: what it held comes back when nothing opened. */
async function openChatWith(message: ChatOutgoing | null, later: string | null): Promise<void> {
  if (blocked.value) return;
  const id = chatId.value;
  const held = heldOf(id);
  busy.value = true;
  try {
    const opened = await props.launch({
      kind: "chat",
      text: "",
      start: null,
      shell: null,
      chat: { message, later, start: chatChoice(), exact: chatPicks.get(id)?.mode !== undefined },
    });
    if (opened) {
      chatPicks.delete(id);
      return;
    }
    const now = composerDraft(id);
    if (!now.text && !now.attachments.length) {
      now.text = held.text;
      now.mentions = held.mentions;
      now.attachments = held.attachments;
    }
  } finally {
    busy.value = false;
  }
}

/** What the composer's draft holds now, copied. */
function heldOf(id: number): Pick<Draft, "text" | "mentions" | "attachments"> {
  const held = composerDraft(id);
  return { text: held.text, mentions: [...held.mentions], attachments: [...held.attachments] };
}

/* ---------- opening ---------- */

const busy = ref(false);
/** The last session going on (`resume`): nothing else opens meanwhile. */
const resuming = ref(false);
const blocked = computed(() => busy.value || resuming.value || props.opening);

/** A click picks; a double click (on the picker, `@dblclick`) opens that kind empty at once. */
function pick(kind: EmptyKind): void {
  pickEmpty(kind);
  void nextTick(focusBox);
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
  if (kind === "claude") return chosen.value.claude;
  return null;
}

async function open(kind: EmptyKind, text: string): Promise<void> {
  if (blocked.value || !kindOn(kind)) return;
  // A chat opened empty from the picker or its button: as the chat's box would open it.
  if (kind === "chat") return openChatWith(null, null);
  busy.value = true;
  // The field empties as it is sent, in the page's memory too (the box is gone once a tab opens,
  // and its watch with it); it gets the text back only when nothing opened. So do the pictures.
  const key = props.draftKey;
  const pictures = kind === "claude" || kind === "codex" ? imagesOf(key) : [];
  if (text) {
    draft.value = "";
    keepDraft(key, "");
  }
  if (pictures.length) {
    images.value = [];
    keepImages(key, []);
  }
  try {
    const opened = await props.launch({
      kind,
      text: text.trim() ? text : "",
      start: startOf(kind),
      shell: kind === "shell" ? chosen.value.shell : null,
      images: pictures.map((each) => `data:${each.mediaType};base64,${each.data}`),
    });
    if (!opened) {
      // Its text comes back, before what was typed meanwhile; its pictures before those pasted since.
      if (text) {
        const typed = draftOf(key);
        const back = typed ? `${text}\n${typed}` : text;
        keepDraft(key, back);
        if (props.draftKey === key) draft.value = back;
      }
      if (pictures.length) {
        const back = [...pictures, ...imagesOf(key)];
        keepImages(key, back);
        if (props.draftKey === key) images.value = back;
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

onBeforeUnmount(() => {
  watcher?.disconnect();
  clearTimeout(noticeTimer);
  registerDraft(chatId.value, null);
});

function focusBox(): void {
  if (tight.value) root.value?.querySelector<HTMLElement>(".mini")?.focus();
  else if (picked.value === "chat") composer.value?.focus();
  else field.value?.focus();
}

defineExpose({ focus: focusBox });
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

      <!-- A chat: the chat's own composer, a chat to be. -->
      <div v-if="picked === 'chat'" class="chatbox">
        <Composer
          :id="chatId"
          ref="composer"
          :path="project.path"
          :info="chatInfo"
          state="idle"
          :settings="chatSettings"
          :suggestion="null"
          :queued="[]"
          :context="chatContext"
          :terminals="chatTerminals"
          :model="chatStart.model"
          :mode="chatStart.mode"
          :effort="chatStart.effort"
          :context-share="null"
          :disabled="blocked"
          :placeholder="t('terminal.empty.placeholder.chat')"
          @send="(message) => openChatWith(message, null)"
          @shell="(line) => openChatWith(null, `!${line}`)"
          @later="(line) => openChatWith(null, line)"
          @dialog="chatDialog"
          @control="chatControl"
          @settings="emit('settings')"
        />
        <ModelDialog v-if="chatMenu?.kind === 'model'" :id="chatId" :path="project.path" :session="null" :info="chatInfo" state="idle" :arg="chatMenu.arg" @close="chatMenu = null" />
        <ModeDialog v-else-if="chatMenu?.kind === 'mode'" :id="chatId" :path="project.path" :session="null" :info="chatInfo" state="idle" :arg="chatMenu.arg" @close="chatMenu = null" />
      </div>

      <!-- A shell, Claude's or Codex's tab: a box of the composer's look. -->
      <div v-else :class="['box', { mono: picked === 'shell', dropping }]" @pointerdown.self="field?.focus()" @dragover="onDragOver" @dragleave="dropping = false" @drop="onDrop">
        <div v-if="takesImages && (images.length || reading)" ref="refsIn" class="refs">
          <span v-for="(each, i) in images" :key="each.key" class="chip" :data-n="i + 1" :title="each.scaled ? t('chatInput.image.scaled') : each.name">
            <button type="button" class="cbody" :aria-label="each.name" @click="shotShown = i + 1">
              <img class="cthumb" :src="`data:${each.mediaType};base64,${each.data}`" alt="" draggable="false" @load="sized(each.key, $event)" />
              <span class="cname">{{ each.name }}</span>
              <span v-if="imageSizes[each.key]" class="cmeta">{{ imageSizes[each.key] }}</span>
            </button>
            <button class="cx" type="button" :aria-label="t('chatInput.remove')" :title="t('chatInput.remove')" @click="removeImage(each.key)"><Icon name="close" :size="10" /></button>
          </span>
          <span v-if="reading" class="chip faint">
            <Spinner :size="11" />
            <span class="cname">{{ t("chatInput.reading") }}</span>
          </span>
        </div>
        <ImageLightbox v-if="shotShown !== null" :images="shots" :start="shotShown" :thumb="shotThumb" @close="shotShown = null" />
        <textarea
          ref="field"
          v-model="draft"
          class="input"
          rows="2"
          spellcheck="false"
          :placeholder="t(PLACEHOLDERS[picked])"
          :aria-label="t(PLACEHOLDERS[picked])"
          @keydown="fieldKeys"
          @paste="onPaste"
        ></textarea>
        <div v-if="notice" class="notice" role="status">
          <Icon name="info" :size="12" />
          <span>{{ notice }}</span>
        </div>
        <div class="row">
          <button v-if="picked === 'shell' && shellChip" type="button" class="chip-btn" :title="t('terminal.empty.shellTitle')" @click="shellMenu">
            <Icon v-if="shellStart.elevated" name="shield" :size="12" class="lead warn" />
            <ChatIcon v-else name="terminal" :size="14" class="lead" />
            <span class="lbl">{{ shellText }}</span>
          </button>
          <template v-else-if="agentBox">
            <button type="button" class="chip-btn" :title="t('terminal.empty.modelTitle')" @click="modelMenu">
              <ChatIcon name="brain" :size="14" class="lead" />
              <span class="lbl">{{ modelText }}</span>
            </button>
            <button type="button" class="chip-btn" :title="t('terminal.empty.modeTitle')" @click="modeMenu">
              <ChatIcon :name="modeIcon" :size="14" class="lead" />
              <span class="lbl">{{ modeText }}</span>
            </button>
          </template>
          <span class="grow"></span>
          <button
            type="button"
            class="send"
            :disabled="blocked"
            :title="draft.trim() || images.length ? t('terminal.empty.start') : t('terminal.empty.startEmpty')"
            :aria-label="draft.trim() || images.length ? t('terminal.empty.start') : t('terminal.empty.startEmpty')"
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

/* ---- a chat's box: the chat's composer, as wide as the others ---- */

.chatbox {
  width: min(640px, 100%);
  color: var(--text);
}

/* ---- the others' box: the composer's look (chat/Composer.vue) ---- */

.box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: min(640px, 100%);
  box-sizing: border-box;
  padding: 10px 10px 8px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: var(--bg-input);
  cursor: text;
}

.box:focus-within {
  border-color: var(--line-lit);
}

.box.dropping {
  border-color: var(--claude-line);
  background: var(--claude-bg);
}

.input {
  box-sizing: border-box;
  display: block;
  width: 100%;
  min-height: 44px;
  margin: 0;
  padding: 2px 2px 2px 3px;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  font-family: var(--font);
  font-size: 13.5px;
  /* In pixels, so a shell's smaller mono lines leave the box as tall as the others'. */
  line-height: 20px;
  color: var(--text-strong);
}

.box.mono .input {
  font-family: var(--font-mono);
  font-size: 12.5px;
}

.input::placeholder {
  color: var(--text-faint);
}

/* The pictures over the text, a line under them, as the chat's attachments stand. */
.refs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin: -2px -10px 0;
  padding: 0 10px 8px;
  box-shadow: inset 0 -1px var(--line);
  cursor: default;
}

.chip {
  position: relative;
  display: inline-flex;
  align-items: center;
  max-width: 260px;
  height: 22px;
  box-sizing: border-box;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--bg-control);
  font-size: 12px;
  color: var(--text);
}

.chip.faint {
  gap: 5px;
  padding: 0 8px 0 6px;
  border-style: dashed;
  background: transparent;
  color: var(--text-muted);
}

.chip.faint .cname {
  font-weight: 400;
  color: inherit;
}

.cbody {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  height: 100%;
  padding: 0 8px 0 4px;
  border: none;
  background: transparent;
  font: inherit;
  color: inherit;
}

.cthumb {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border-radius: 3px;
  object-fit: cover;
}

.cname {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--text-strong);
}

.cmeta {
  flex-shrink: 0;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  color: var(--text-subtle);
}

.cx {
  position: absolute;
  top: 50%;
  right: 3px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  padding: 0;
  border: none;
  border-radius: 4px;
  background: var(--bg-control);
  box-shadow: -6px 0 6px var(--bg-control);
  color: var(--text-muted);
  opacity: 0;
  transform: translateY(-50%);
}

.chip:hover .cx,
.cx:focus-visible {
  opacity: 1;
}

.cx:hover {
  color: var(--text-strong);
}

.notice {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-muted);
  cursor: default;
}

.row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  cursor: default;
}

.grow {
  flex: 1;
}

.chip-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  height: 26px;
  box-sizing: border-box;
  padding: 0 11px 0 9px;
  border: 1px solid transparent;
  border-radius: 13px;
  background: var(--bg-control-hover);
  font: inherit;
  font-size: 12.5px;
  color: var(--text);
  white-space: nowrap;
}

.chip-btn:hover {
  background: color-mix(in srgb, var(--bg-control-hover) 86%, var(--text));
}

.chip-btn .lead {
  flex-shrink: 0;
  color: var(--text-nav);
}

/* An administrator's shell picked: the shield in the warning's colour, as "+" shows it. */
.chip-btn .lead.warn {
  color: var(--warn);
}

.chip-btn .lbl {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.send {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
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
