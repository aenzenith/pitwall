<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { filterCommands, isKnownCommand, menuCommands, modelOf, needsArgs, nextMode, parseCommand, routeCommand, skillNames, type MenuCommand, type Route } from "../../lib/chat/commands";
import {
  commandAt,
  contextKey,
  draftOf,
  forgetDraft,
  isBlank,
  isLongPaste,
  lineCount,
  liveMentions,
  mentionAt,
  mentionBefore,
  mentionFiles,
  mentionText,
  messageText,
  newKey,
  outgoing,
  readImage,
  readPdf,
  relativePath,
  scrub,
  segments,
  suggestionHit,
  takenFiles,
  TEXT_MAX,
  type FileHit,
} from "../../lib/chat/composer";
import { MODE_ICONS, type ComposerApi, type ComposerEmits, type ComposerProps, type TerminalRef } from "../../lib/chat/ui";
import type { Shot } from "../../lib/cardImages";
import { sizeText } from "../../lib/explorer";
import { modelName, percentText } from "../../lib/fuel";
import { fuzzy } from "../../lib/fuzzy";
import { language, t, type Key } from "../../lib/i18n";
import { isMac } from "../../lib/platform";
import { api } from "../../lib/store";
import ImageLightbox from "../board/ImageLightbox.vue";
import AttachMenu from "./AttachMenu.vue";
import ChatIcon from "./ChatIcon.vue";
import MentionMenu, { type MentionItem } from "./MentionMenu.vue";
import SlashMenu from "./SlashMenu.vue";

/**
 * A chat's prompt box (docs/SOHBET.md, The composer; TS3): the text, the chips under it (model,
 * permission mode, effort, the context used), the attachments, the queue over it, Send that turns
 * into Stop while a turn runs.
 *
 * The text is a plain text area, so WebKit's own text keys, undo, input methods and spell check
 * all work; what it holds is drawn again under it (the field's own text is clear) with the
 * mentions as chips, a known command at its start, `ultrathink` and `ultracode` coloured, and a
 * command's argument hint as ghost text. Nothing of a draft leaves the page's memory
 * (lib/chat/composer: `draftOf`).
 */
const props = defineProps<ComposerProps>();
const emit = defineEmits<ComposerEmits>();

const draft = computed(() => draftOf(props.id));

/** The images waiting in the box, as a card's are shown large (board/ImageLightbox), numbered in
 * their order; the one shown, by its number. */
const attachedImages = computed(() => draft.value.attachments.filter((each) => each.kind === "image"));
const shots = computed<Shot[]>(() => attachedImages.value.map((each, i) => ({ n: i + 1, src: `data:${each.mediaType};base64,${each.data}` })));
const shotShown = ref<number | null>(null);
const refsIn = ref<HTMLElement | null>(null);
function shotThumb(n: number): HTMLElement | null {
  return refsIn.value?.querySelector<HTMLElement>(`[data-n="${n}"] img`) ?? null;
}
/** Each waiting image's size as it goes (`210×81`), read off its thumbnail once drawn. */
const imageSizes = reactive<Record<string, string>>({});
function sized(key: string, event: Event): void {
  const image = event.target as HTMLImageElement;
  imageSizes[key] = `${image.naturalWidth}×${image.naturalHeight}`;
}
/** A PDF's size from its base64. */
const pdfSize = (data: string): string => sizeText(Math.floor((data.length * 3) / 4));
const field = ref<HTMLTextAreaElement | null>(null);
const mirror = ref<HTMLElement | null>(null);
const box = ref<HTMLElement | null>(null);
const clip = ref<HTMLButtonElement | null>(null);
const attachMenu = ref<InstanceType<typeof AttachMenu> | null>(null);

const running = computed(() => props.state === "running" || props.state === "requires_action");
const shell = computed(() => draft.value.text.startsWith("!"));
const model = computed(() => modelOf(props.info, props.model));
const commands = computed(() => menuCommands(props.info));

/* ---------- the field ---------- */

/** Where the caret is; the menus follow it. */
const caret = ref(0);
/** A prompt recalled with ↑: no menu opens over it until it is edited (V25). */
const recalled = ref(false);
/** Esc closed the menu: it stays closed until the next keystroke that edits. */
const menuOff = ref(false);
/** ↑/↓ walking the prompts sent from this tab: where, and the draft it started from. */
let walk: { at: number; saved: string } | null = null;
let lastEsc = 0;

function syncFromField(): void {
  const el = field.value;
  if (!el) return;
  draft.value.text = el.value;
  caret.value = el.selectionStart;
  if (draft.value.mentions.length) draft.value.mentions = liveMentions(el.value, draft.value.mentions);
  fit();
}

function onInput(): void {
  recalled.value = false;
  menuOff.value = false;
  walk = null;
  syncFromField();
}

function onCaret(): void {
  if (field.value) caret.value = field.value.selectionStart;
}

/** The field grows with its text up to its cap (CSS), then scrolls; the drawing follows. */
/**
 * The field as tall as its text, between its least and most. It grows and shrinks smoothly (the
 * user's word, 14 Nov: "mesaj paneli de animasyonla büyüsün"), from where it stands, so a key
 * pressed while it moves carries on from there; `still`, it takes its height at once (a chat
 * opened, the composer shown).
 */
function fit(still = false): void {
  void nextTick(() => {
    const el = field.value;
    if (!el) return;
    const style = getComputedStyle(el);
    const least = parseFloat(style.minHeight) || 0;
    const most = parseFloat(style.maxHeight) || Infinity;
    const from = el.offsetHeight;
    el.style.transition = "none";
    el.style.height = "auto";
    const to = Math.min(most, Math.max(least, el.scrollHeight));
    el.style.height = `${still ? to : from}px`;
    void el.offsetHeight;
    el.style.transition = "";
    if (!still && to !== from) el.style.height = `${to}px`;
    if (mirror.value) mirror.value.scrollTop = el.scrollTop;
  });
}

function onScroll(): void {
  if (mirror.value && field.value) mirror.value.scrollTop = field.value.scrollTop;
}

/** Puts `text` in place of `start…end`, as typing would (the field's undo keeps it). */
function replaceRange(start: number, end: number, text: string): void {
  const el = field.value;
  if (!el) {
    const now = draft.value.text;
    draft.value.text = now.slice(0, start) + text + now.slice(end);
    return;
  }
  el.focus();
  el.setSelectionRange(start, end);
  if (!document.execCommand("insertText", false, text)) el.setRangeText(text, start, end, "end");
  syncFromField();
}

/** The whole text replaced (a recalled prompt, the suggestion taken, `setText`), caret at its end. */
function setFieldText(text: string): void {
  const el = field.value;
  draft.value.text = text;
  if (el) {
    el.value = text;
    el.setSelectionRange(text.length, text.length);
    caret.value = text.length;
  }
  draft.value.mentions = liveMentions(text, draft.value.mentions);
  fit();
}

/** `text` at the caret, or at the end when the field isn't the keyboard's. */
function insertText(text: string): void {
  const el = field.value;
  const focused = el && document.activeElement === el;
  const at = focused ? el.selectionStart : draft.value.text.length;
  const end = focused ? el.selectionEnd : at;
  replaceRange(at, end, text);
}

/** A command (a dialog's "/compact ", a picked skill) goes at the draft's start, where a command
 * is one, in the place of one already there; the caret after it, for its arguments. */
function insertCommand(text: string): void {
  const head = /^\/\S*\s?/.exec(draft.value.text);
  replaceRange(0, head ? head[0].length : 0, text);
}

/** A mention at the caret, with a space before it where it would touch a word. */
function insertMention(token: string): void {
  const el = field.value;
  const focused = el && document.activeElement === el;
  const at = focused ? el.selectionStart : draft.value.text.length;
  const before = draft.value.text.slice(0, at);
  const lead = before && !/\s$/.test(before) ? " " : "";
  if (!draft.value.mentions.includes(token)) draft.value.mentions.push(token);
  replaceRange(at, focused ? el.selectionEnd : at, `${lead}@${token} `);
}

/* ---------- what is drawn under the field ---------- */

const skills = computed(() => skillNames(props.info));
const drawn = computed(() => segments(draft.value.text, draft.value.mentions, (name) => isKnownCommand(name, props.info), skills.value));

/** A command typed with its space and nothing after: its arguments' hint, as ghost text. */
const argHint = computed(() => {
  const match = /^\/(\S+) $/.exec(draft.value.text);
  if (!match || caret.value !== draft.value.text.length) return "";
  return commands.value.find((command) => command.name === match[1] || command.aliases.includes(match[1]))?.hint ?? "";
});

/** The suggested next prompt (V26): shown in the empty field, taken with Tab, never sent by Enter. */
const ghost = computed(() => (props.settings.suggestions && props.suggestion && !draft.value.text && !draft.value.attachments.length && !running.value ? props.suggestion : null));

const placeholder = computed(() => ghost.value ?? t(running.value ? "chatInput.placeholder.queue" : "chatInput.placeholder"));

/* ---------- the `/` menu ---------- */

const slash = computed(() => {
  if (menuOff.value || recalled.value || shell.value || props.disabled) return null;
  return commandAt(draft.value.text, caret.value);
});
const slashItems = computed<MenuCommand[]>(() => (slash.value ? filterCommands(commands.value, slash.value.query) : []));
const slashActive = ref(0);
watch(
  () => slash.value?.query,
  () => (slashActive.value = 0),
);

/** A command picked: Tab completes it; Enter runs it at once unless its arguments are needed,
 * and mid-text it only completes (C4). */
function pickCommand(index: number, how: "enter" | "tab" | "click"): void {
  const at = slash.value;
  const command = slashItems.value[index];
  if (!at || !command) return;
  const end = caret.value;
  replaceRange(at.start, end, `/${command.name} `);
  if (!at.head || how === "tab" || needsArgs(command)) return;
  if (draft.value.text.trim() === `/${command.name}`) submit("next");
}

/* ---------- the `@` menu ---------- */

const mention = computed(() => {
  if (menuOff.value || recalled.value || shell.value || props.disabled) return null;
  return mentionAt(draft.value.text, caret.value);
});
const mentionItems = ref<MentionItem[]>([]);
const mentionActive = ref(0);
const mentionLoading = ref(false);
let mentionRun = 0;
let mentionTimer: ReturnType<typeof setTimeout> | undefined;

watch(
  () => (mention.value ? mention.value.query : null),
  (query) => {
    clearTimeout(mentionTimer);
    if (query === null) {
      mentionItems.value = [];
      return;
    }
    mentionActive.value = 0;
    mentionTimer = setTimeout(() => void searchMentions(query), 90);
  },
);

/** The Files window's names of a project, read once a minute at most: where Claude Code gives no
 * suggestions of its own. Page memory only. */
const projectNames = new Map<string, { at: number; hits: Promise<FileHit[]> }>();

function namesOf(path: string): Promise<FileHit[]> {
  const known = projectNames.get(path);
  if (known && Date.now() - known.at < 60_000) return known.hits;
  const hits = api
    .explorerFiles(path)
    .then((found) => (found ? [...found.dirs.map((dir) => ({ path: `${dir}/`, dir: true })), ...found.files.map((file) => ({ path: file, dir: false }))] : []))
    .catch(() => [] as FileHit[]);
  projectNames.set(path, { at: Date.now(), hits });
  return hits;
}

/** Claude Code's own suggestions for `query` (`file_suggestions`), or none when it doesn't answer. */
async function cliSuggestions(query: string): Promise<FileHit[]> {
  try {
    const answer = await Promise.race([api.chatControl<{ suggestions?: unknown[] }>(props.id, { kind: "fileSuggestions", query }), new Promise<null>((resolve) => setTimeout(() => resolve(null), 1500))]);
    const list = answer && Array.isArray(answer.suggestions) ? answer.suggestions : [];
    return list.map(suggestionHit).filter((hit): hit is FileHit => !!hit);
  } catch {
    return [];
  }
}

async function searchMentions(query: string): Promise<void> {
  const run = ++mentionRun;
  const bare = query.split("#")[0];
  const agents: MentionItem[] = (props.info?.agents ?? [])
    .filter((agent) => !bare || !!fuzzy(`agent-${agent.name}`, bare) || !!fuzzy(agent.name, bare))
    .slice(0, 6)
    .map((agent) => ({ kind: "agent", key: `agent-${agent.name}`, label: agent.name, detail: (agent.description ?? "").split("\n")[0] }));
  mentionItems.value = agents;
  mentionLoading.value = true;
  const [cli, names] = await Promise.all([cliSuggestions(bare), namesOf(props.path)]);
  if (run !== mentionRun) return;
  mentionLoading.value = false;
  // Paths relative to the project before they are matched, so the project's own folder names
  // never match what was typed.
  const relative = (hit: FileHit): FileHit => ({ path: hit.dir ? `${relativePath(hit.path.replace(/\/$/, ""), props.path)}/` : relativePath(hit.path, props.path), dir: hit.dir });
  const files: MentionItem[] = mentionFiles(cli.map(relative), names.map(relative), bare).map((hit) => ({ kind: hit.dir ? "dir" : "file", key: hit.path, label: hit.path, detail: "" }));
  mentionItems.value = [...agents, ...files];
}

function pickMention(index: number): void {
  const at = mention.value;
  const item = mentionItems.value[index];
  if (!at || !item) return;
  const range = /#\d+(?:-\d+)?$/.exec(at.query)?.[0] ?? "";
  const token = item.kind === "agent" ? item.key : item.kind === "file" ? `${item.label}${range}` : item.label;
  if (!draft.value.mentions.includes(token)) draft.value.mentions.push(token);
  replaceRange(at.start, caret.value, `@${token} `);
}

/* ---------- placing the menus over the box ---------- */

const place = ref<Record<string, string>>({});

function placeMenus(): void {
  const rect = box.value?.getBoundingClientRect();
  if (!rect) return;
  const width = Math.min(rect.width, 600);
  place.value = {
    left: `${Math.max(8, rect.left)}px`,
    bottom: `${window.innerHeight - rect.top + 6}px`,
    width: `${width}px`,
    maxHeight: `${Math.max(120, Math.min(340, rect.top - 16))}px`,
  };
}

const menuOpen = computed(() => (slash.value ? "slash" : mention.value ? "mention" : null));
watch(menuOpen, (open) => open && placeMenus());

const menuId = computed(() => `composer-${props.id}-${menuOpen.value ?? "menu"}`);
const activeOption = computed(() => {
  if (menuOpen.value === "slash" && slashItems.value.length) return `${menuId.value}-${slashActive.value}`;
  if (menuOpen.value === "mention" && mentionItems.value.length) return `${menuId.value}-${mentionActive.value}`;
  return undefined;
});

function closeMenus(): void {
  menuOff.value = true;
}

/* ---------- attachments ---------- */

const reading = ref(0);
const opened = ref<string | null>(null);
const IMAGES_MAX = 20;

async function addFile(file: File): Promise<void> {
  if (file.type === "application/pdf") {
    reading.value++;
    try {
      const data = await readPdf(file);
      draft.value.attachments.push({ kind: "pdf", key: newKey(), data, name: file.name || "document.pdf" });
    } catch {
      say(t("chatInput.notice.pdfTooLarge"));
    } finally {
      reading.value--;
    }
    return;
  }
  if (draft.value.attachments.filter((each) => each.kind === "image").length + reading.value >= IMAGES_MAX) {
    say(t("chatInput.notice.imageTooMany", { count: IMAGES_MAX }));
    return;
  }
  reading.value++;
  try {
    const image = await readImage(file);
    draft.value.attachments.push({ kind: "image", key: newKey(), mediaType: image.mediaType, data: image.data, name: file.name || "image", scaled: image.scaled });
  } catch {
    say(t("chatInput.notice.imageFailed"));
  } finally {
    reading.value--;
  }
}

function remove(key: string): void {
  draft.value.attachments = draft.value.attachments.filter((each) => each.key !== key);
  delete imageSizes[key];
  if (opened.value === key) opened.value = null;
}

/** The pasted block unfolded under the chips, read only (V43). */
const openedText = computed(() => {
  const each = draft.value.attachments.find((one) => one.key === opened.value);
  return each && each.kind === "pasted" ? each.text : null;
});

const pastedLength = computed(() => draft.value.attachments.reduce((sum, each) => sum + (each.kind === "pasted" ? each.text.length + 2 : 0), 0));

function onPaste(event: ClipboardEvent): void {
  const files = takenFiles(event.clipboardData);
  if (files.length) {
    event.preventDefault();
    for (const file of files) void addFile(file);
    return;
  }
  const raw = event.clipboardData?.getData("text/plain") ?? "";
  if (!raw) return;
  const { text, removed } = scrub(raw);
  if (removed) say(t("chatInput.notice.scrubbed", { count: removed }));
  if (draft.value.text.length + pastedLength.value + text.length > TEXT_MAX) {
    event.preventDefault();
    say(t("chatInput.notice.pasteTooLong", { max: TEXT_MAX.toLocaleString(language.value) }));
    return;
  }
  if (!shell.value && isLongPaste(text)) {
    event.preventDefault();
    draft.value.attachments.push({ kind: "pasted", key: newKey(), text, lines: lineCount(text), name: null });
    return;
  }
  if (removed) {
    event.preventDefault();
    insertText(text);
  }
}

const dropping = ref(false);

function onDragOver(event: DragEvent): void {
  if (!event.dataTransfer?.types.includes("Files")) return;
  event.preventDefault();
  dropping.value = true;
}

function onDrop(event: DragEvent): void {
  dropping.value = false;
  const files = takenFiles(event.dataTransfer);
  if (!files.length) return;
  event.preventDefault();
  for (const file of files) void addFile(file);
}

async function addTerminal(terminal: TerminalRef): Promise<void> {
  reading.value++;
  try {
    const text = scrub(await api.chatTerminalText(terminal.id)).text.trimEnd();
    if (!text) return say(t("chatInput.notice.terminalEmpty"));
    if (draft.value.text.length + pastedLength.value + text.length > TEXT_MAX) return say(t("chatInput.notice.pasteTooLong", { max: TEXT_MAX.toLocaleString(language.value) }));
    draft.value.attachments.push({ kind: "pasted", key: newKey(), text, lines: lineCount(text), name: terminal.name });
  } catch {
    say(t("chatInput.notice.terminalFailed"));
  } finally {
    reading.value--;
  }
  field.value?.focus();
}

function attachMention(): void {
  const el = field.value;
  if (!el) return;
  el.focus();
  const at = el.selectionStart;
  const before = draft.value.text.slice(0, at);
  replaceRange(at, el.selectionEnd, before && !/\s$/.test(before) ? " @" : "@");
}

function attachImage(): void {
  say(t("chatInput.notice.image"));
  field.value?.focus();
}


/* ---------- the Files window's file (V48) ---------- */

const contextToken = computed(() => (props.context ? contextKey({ ...props.context, path: relativePath(props.context.path, props.path) }) : null));
const contextOn = computed(() => !!contextToken.value && draft.value.contextOff !== contextToken.value);
/** The Files window's file as its chip names it: its name, and its lines fainter beside it. */
const contextFile = computed(() => {
  if (!props.context) return { name: "", lines: "" };
  const name = props.context.path.slice(props.context.path.lastIndexOf("/") + 1);
  const { startLine, endLine } = props.context;
  if (!startLine) return { name, lines: "" };
  return { name, lines: endLine && endLine !== startLine ? `${startLine}–${endLine}` : `${startLine}` };
});

/* ---------- notices ---------- */

const notice = ref<string | null>(null);
let noticeTimer: ReturnType<typeof setTimeout> | undefined;

function say(text: string): void {
  notice.value = text;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => (notice.value = null), 7000);
}

/* ---------- sending ---------- */

const hasContent = computed(() => !!draft.value.text.trim() || draft.value.attachments.length > 0);

function remember(typed: string): void {
  const history = draft.value.history;
  if (!typed.trim() || history[history.length - 1] === typed) return;
  history.push(typed);
  if (history.length > 200) history.shift();
}

/** The text is gone (sent or done); a stashed draft comes back (C22). */
function clearAfterSend(keepAttachments: boolean): void {
  const kept = draft.value;
  setFieldText("");
  kept.mentions = [];
  if (!keepAttachments) kept.attachments = [];
  opened.value = null;
  walk = null;
  if (kept.stash) {
    const stash = kept.stash;
    kept.stash = null;
    kept.mentions = stash.mentions;
    kept.attachments = [...kept.attachments, ...stash.attachments];
    setFieldText(stash.text);
    say(t("chatInput.notice.unstashed"));
  }
}

function runRoute(route: Exclude<Route, { kind: "send" }>, typed: string): void {
  switch (route.kind) {
    case "dialog":
      emit("dialog", route.open);
      break;
    case "control":
      // What follows the command (`/plan <task>`) goes once the control is through, never before.
      emit("control", route.control, route.then ? outgoing(route.then, [], "next") : undefined);
      break;
    case "settings":
      emit("settings");
      break;
    case "terminal":
      emit("terminal", route.command);
      break;
    case "pickDir":
      void api
        .chatPickDir(props.id)
        .then((dir) => dir && say(t("chatInput.notice.dirAdded", { dir })))
        .catch(() => say(t("chatInput.notice.dirFailed")));
      break;
    case "refused":
      say(t(route.key));
      return;
  }
  remember(typed);
  clearAfterSend(true);
}

function submit(priority: "next" | "now"): void {
  if (props.disabled) return;
  const kept = draft.value;
  const typed = kept.text;
  if (shell.value) {
    const line = typed.slice(1).trim();
    if (!line) return;
    remember(typed);
    emit("shell", line);
    clearAfterSend(true);
    return;
  }
  // Nothing typed or attached (an empty field, the suggestion's ghost alone): nothing goes, not
  // even the Files window's file on its own.
  if (!typed.trim() && !kept.attachments.length) return;
  // Invisible characters at send time: nothing goes, the cleaned text comes back (V28).
  const cleaned = scrub(typed);
  if (cleaned.removed) {
    setFieldText(cleaned.text);
    say(t("chatInput.notice.scrubbedSend", { count: cleaned.removed }));
    return;
  }
  const parsed = parseCommand(typed);
  if (parsed) {
    const route = routeCommand(parsed.name, parsed.rest, { info: props.info, mode: props.mode, model: props.model, effort: props.effort });
    if (route.kind !== "send") return runRoute(route, typed);
  }
  // A command's line (a skill's, a plugin's: sent as typed) takes no file along: its arguments are its own.
  const text = messageText(typed, kept.attachments, contextOn.value && !parsed ? contextToken.value : null);
  if (!text && !kept.attachments.some((each) => each.kind === "image" || each.kind === "pdf")) return;
  if (text.length > TEXT_MAX) {
    say(t("chatInput.notice.tooLong", { max: TEXT_MAX.toLocaleString(language.value) }));
    return;
  }
  emit("send", outgoing(text, kept.attachments, running.value ? priority : "next"));
  remember(typed);
  clearAfterSend(false);
}

function onSendButton(): void {
  if (running.value && !hasContent.value) emit("interrupt");
  else submit("next");
  field.value?.focus();
}

/* ---------- keys ---------- */

function moveIn(delta: number): void {
  if (menuOpen.value === "slash") {
    const count = slashItems.value.length;
    if (count) slashActive.value = (slashActive.value + delta + count) % count;
  } else if (menuOpen.value === "mention") {
    const count = mentionItems.value.length;
    if (count) mentionActive.value = (mentionActive.value + delta + count) % count;
  }
}

function pickActive(how: "enter" | "tab"): boolean {
  if (menuOpen.value === "slash" && slashItems.value.length) {
    pickCommand(slashActive.value, how);
    return true;
  }
  if (menuOpen.value === "mention" && mentionItems.value.length) {
    pickMention(mentionActive.value);
    return true;
  }
  return false;
}

/** ↑ on the first line (or an empty field) walks back through this tab's prompts. */
function recallUp(el: HTMLTextAreaElement): boolean {
  const history = draft.value.history;
  if (!history.length || el.selectionStart !== el.selectionEnd) return false;
  const before = el.value.slice(0, el.selectionStart);
  if (!(el.value === "" || el.selectionStart === 0 || (walk && !before.includes("\n")))) return false;
  walk ??= { at: history.length, saved: el.value };
  if (walk.at === 0) return true;
  walk.at--;
  recalled.value = true;
  setFieldText(history[walk.at]);
  return true;
}

function recallDown(el: HTMLTextAreaElement): boolean {
  if (!walk || el.value.slice(el.selectionEnd).includes("\n")) return false;
  const history = draft.value.history;
  walk.at++;
  recalled.value = true;
  if (walk.at >= history.length) {
    setFieldText(walk.saved);
    walk = null;
  } else setFieldText(history[walk.at]);
  return true;
}

function cycleMode(): void {
  emit("control", { kind: "setMode", mode: nextMode(props.mode, model.value) });
}

/** Ctrl+S: the draft put aside, or the one put aside back. */
function stashToggle(): void {
  const kept = draft.value;
  if (hasContent.value) {
    kept.stash = { text: kept.text, mentions: [...kept.mentions], attachments: [...kept.attachments] };
    kept.attachments = [];
    kept.mentions = [];
    setFieldText("");
    say(t("chatInput.notice.stashed"));
  } else if (kept.stash) {
    const stash = kept.stash;
    kept.stash = null;
    kept.mentions = stash.mentions;
    kept.attachments = stash.attachments;
    setFieldText(stash.text);
  }
}

function onKey(event: KeyboardEvent): void {
  const el = field.value;
  if (!el || event.isComposing || event.keyCode === 229) return;
  const plain = !event.shiftKey && !event.altKey && !event.metaKey && !event.ctrlKey;

  if (menuOpen.value) {
    if (plain && event.key === "ArrowDown") return stop(event, () => moveIn(1));
    if (plain && event.key === "ArrowUp") return stop(event, () => moveIn(-1));
    if (plain && (event.key === "Tab" || event.key === "Enter") && pickActive(event.key === "Tab" ? "tab" : "enter")) return stop(event);
    if (event.key === "Escape") return stop(event, closeMenus);
  }

  switch (event.key) {
    case "Tab":
      if (event.shiftKey && !event.altKey && !event.metaKey && !event.ctrlKey) return stop(event, cycleMode);
      if (plain && ghost.value) return stop(event, () => setFieldText(ghost.value ?? ""));
      return;
    case "Enter": {
      const mod = isMac ? event.metaKey : event.ctrlKey;
      // ⌥Enter: while a turn runs, the message goes now (C31); otherwise a new line.
      if (event.altKey && !event.metaKey && !event.ctrlKey && !event.shiftKey) return stop(event, () => (running.value ? submit("now") : insertText("\n")));
      if (props.settings.sendKey === "mod-enter") {
        if (mod && !event.shiftKey && !event.altKey) return stop(event, () => submit("next"));
        return;
      }
      if (!event.shiftKey && !event.altKey && (plain || mod)) return stop(event, () => submit("next"));
      return;
    }
    case "Escape":
      if (running.value) return stop(event, () => emit("interrupt"));
      if (!hasContent.value) {
        const now = Date.now();
        // Esc twice on an empty composer: the rewind picker, as in the CLI (V181).
        if (now - lastEsc < 700) {
          lastEsc = 0;
          emit("dialog", { kind: "rewind" });
        } else lastEsc = now;
        return stop(event);
      }
      return;
    case "ArrowUp":
      if (plain && recallUp(el)) event.preventDefault();
      return;
    case "ArrowDown":
      if (plain && recallDown(el)) event.preventDefault();
      return;
    case "Backspace": {
      if (!plain || el.selectionStart !== el.selectionEnd) return;
      // A mention goes whole: selected, then the key deletes it as typing would.
      const start = mentionBefore(el.value, el.selectionStart, draft.value.mentions);
      if (start !== null) el.setSelectionRange(start, el.selectionStart);
      return;
    }
  }
  if ((event.key === "s" || event.key === "S") && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey) stop(event, stashToggle);
}

function stop(event: KeyboardEvent, then?: () => void): void {
  event.preventDefault();
  event.stopPropagation();
  then?.();
}

/* ---------- the chips ---------- */

const modelLabel = computed(() => {
  const found = model.value;
  if (props.model) {
    if (found && found.value !== "default" && found.displayName) return found.displayName;
    if (found?.resolvedModel) return modelName(found.resolvedModel);
    return modelName(props.model);
  }
  // Before a turn or the CLI's settings name one, its default: the model that resolves to. Only
  // before the CLI has told of its models at all does the chip say "Model".
  return found?.resolvedModel ? modelName(found.resolvedModel) : t("chatInput.model.unknown");
});

const MODES: Record<string, Key> = {
  default: "chatInput.mode.default",
  acceptEdits: "chatInput.mode.acceptEdits",
  plan: "chatInput.mode.plan",
  auto: "chatInput.mode.auto",
  dontAsk: "chatInput.mode.dontAsk",
};
const modeLabel = computed(() => t(MODES[props.mode ?? "default"] ?? "chatInput.mode.default"));
const modeIcon = computed(() => MODE_ICONS[(props.mode ?? "default") as keyof typeof MODE_ICONS] ?? MODE_ICONS.default);

const EFFORTS: Record<string, Key> = {
  low: "chatInput.effort.low",
  medium: "chatInput.effort.medium",
  high: "chatInput.effort.high",
  xhigh: "chatInput.effort.xhigh",
  max: "chatInput.effort.max",
  auto: "chatInput.effort.auto",
};
/** The effort beside the model in its pill, once the chat knows it and it is a level of the
 * model's (Auto, the CLI's own choice, goes unsaid). */
const effortText = computed(() => {
  const level = props.effort;
  if (!level || level === "auto" || !EFFORTS[level] || model.value?.supportsEffort === false) return "";
  return t(EFFORTS[level]);
});

const contextText = computed(() => (props.contextShare === null ? "" : percentText(Math.round(props.contextShare * 100), language.value)));
const ring = computed(() => {
  const share = Math.min(1, Math.max(0, props.contextShare ?? 0));
  const length = 2 * Math.PI * 5;
  return { dash: `${share * length} ${length}`, tone: share >= 0.9 ? "hot" : share >= 0.75 ? "warm" : "" };
});

/* ---------- the bar over the message ---------- */

// What goes with the message stands in a bar over it that opens upward and shuts down onto its
// line, its height following what it holds (a chip added on a new row, a pasted block unfolded),
// its chips coming, going and moving to their places (the user's word, 14 Nov: "smooth olsun").
// A chat opened with chips already there shows them at once.
// "Reading" shows only once a file takes a while: a quick one would come and go at once.
const readingShown = ref(false);
let readingTimer: ReturnType<typeof setTimeout> | undefined;
watch(reading, (count) => {
  clearTimeout(readingTimer);
  if (count > 0) readingTimer = setTimeout(() => (readingShown.value = true), 250);
  else readingShown.value = false;
});
const hasRefs = computed(() => draft.value.attachments.length > 0 || contextOn.value || readingShown.value || !!draft.value.stash);
const refsHeight = ref(0);
const refsStill = ref(true);
let refsWatch: ResizeObserver | null = null;

function measureRefs(): void {
  refsHeight.value = hasRefs.value && refsIn.value ? refsIn.value.offsetHeight : 0;
}

/** A frame without the bar's transitions: its height set where it stands. */
function settleRefs(): void {
  refsStill.value = true;
  measureRefs();
  requestAnimationFrame(() => requestAnimationFrame(() => (refsStill.value = false)));
}

watch(hasRefs, () => void nextTick(measureRefs));

/** A chip going leaves the row where it stood, so the others move over its place. */
function pinChip(el: Element): void {
  const chip = el as HTMLElement;
  chip.style.left = `${chip.offsetLeft}px`;
  chip.style.top = `${chip.offsetTop}px`;
}

/* ---------- the chips' room ---------- */

// Narrow, the model's and the mode's chips are their icons alone and the context its share (the
// user's word, 14 Nov: "dar iken o ikonlar gözüksün"). The row goes narrow once its chips at full
// width no longer fit it, and wide again once it is as wide as they took; what they say changing
// weighs them again, in the same frame.
const chipsRow = ref<HTMLElement | null>(null);
const narrow = ref(false);
let wideNeed = 0;
let chipsWatch: ResizeObserver | null = null;

function weigh() {
  const row = chipsRow.value;
  if (!row) return;
  if (!narrow.value) {
    if (row.scrollWidth > row.clientWidth + 1) {
      wideNeed = row.scrollWidth;
      narrow.value = true;
    }
  } else if (row.clientWidth >= wideNeed) {
    narrow.value = false;
    void nextTick(weigh);
  }
}

watch([modelLabel, effortText, modeLabel, contextText, shell, language], () => {
  narrow.value = false;
  void nextTick(weigh);
});

/** The model's chip, narrow: its name and effort said on hover. */
const modelTitle = computed(() => (effortText.value ? `${modelLabel.value} · ${effortText.value}` : modelLabel.value));

/* ---------- the composer's own ways in ---------- */

const exposed: ComposerApi = {
  focus: () => field.value?.focus(),
  insert: (text: string) => (text.startsWith("/") ? insertCommand(text) : insertText(text)),
  attach: (path: string, startLine?: number, endLine?: number) => insertMention(mentionText(relativePath(path, props.path), startLine, endLine)),
  setText: (text: string) => {
    setFieldText(text);
    field.value?.focus();
  },
};
defineExpose(exposed);

watch(
  () => props.id,
  () => {
    walk = null;
    void nextTick(settleRefs);
    void nextTick(() => {
      if (field.value) field.value.value = draft.value.text;
      fit(true);
    });
  },
);

// The draft changed from outside the field (a message the process never took, handed back).
watch(
  () => draft.value.text,
  (text) => {
    if (!field.value || field.value.value === text) return;
    field.value.value = text;
    fit();
  },
);

onMounted(() => {
  if (field.value) field.value.value = draft.value.text;
  fit(true);
  window.addEventListener("resize", placeMenus);
  if (chipsRow.value) {
    chipsWatch = new ResizeObserver(weigh);
    chipsWatch.observe(chipsRow.value);
  }
  if (refsIn.value) {
    refsWatch = new ResizeObserver(measureRefs);
    refsWatch.observe(refsIn.value);
  }
  settleRefs();
});

onBeforeUnmount(() => {
  clearTimeout(noticeTimer);
  clearTimeout(readingTimer);
  clearTimeout(mentionTimer);
  window.removeEventListener("resize", placeMenus);
  chipsWatch?.disconnect();
  refsWatch?.disconnect();
  // An empty draft with nothing kept needn't outlive its view.
  if (isBlank(draft.value)) forgetDraft(props.id);
});
</script>

<template>
  <div :class="['composer', { off: disabled }]">
    <div v-if="queued.length" class="queue" role="list" :aria-label="t('chatInput.queue')">
      <div v-for="each in queued" :key="each.uuid" class="qrow" role="listitem">
        <span class="qtag">{{ t(each.priority === "now" ? "chatInput.queue.now" : each.priority === "later" ? "chatInput.queue.later" : "chatInput.queue") }}</span>
        <span class="qtext">{{ each.text }}</span>
        <button class="qx" type="button" :title="t('chatInput.queue.cancel')" :aria-label="t('chatInput.queue.cancel')" @click="emit('cancelQueued', each.uuid)"><Icon name="close" :size="12" /></button>
      </div>
    </div>

    <div ref="box" :class="['box', { shell, dropping }]" @dragover="onDragOver" @dragleave="dropping = false" @drop="onDrop">
      <!-- What goes with the message stands over it, a line under it (the user's word, 14 Nov, from
           the VS Code extension's), the pasted block unfolded under its chip. -->
      <div :class="['refs', { shown: hasRefs, still: refsStill }]" :style="{ height: `${refsHeight}px` }">
        <div ref="refsIn" class="refs-in">
          <TransitionGroup tag="div" name="chipf" class="atts" @before-leave="pinChip">
            <!-- Each as a small chip (the user's word, 14 Nov, from the VS Code extension's): an image's
                 thumbnail or the kind's icon, its name, and what it holds fainter beside it; × on hover. -->
            <span v-if="draft.stash" key="stash" class="chip faint" :title="t('chatInput.stash.title')">
              <Icon name="pause" :size="11" class="cic" />
              <span class="cname">{{ t("chatInput.stash") }}</span>
            </span>
            <span v-if="contextOn" key="context" class="chip" :title="t('chatInput.contextFile.title')">
              <span class="cbody">
                <Icon name="file" :size="12" class="cic" />
                <span class="cname">{{ contextFile.name }}</span>
                <span v-if="contextFile.lines" class="cmeta">{{ contextFile.lines }}</span>
              </span>
              <button class="cx" type="button" :aria-label="t('chatInput.remove')" :title="t('chatInput.remove')" @click="draft.contextOff = contextToken"><Icon name="close" :size="10" /></button>
            </span>
            <template v-for="each in draft.attachments" :key="each.key">
              <span v-if="each.kind === 'image'" class="chip" :data-n="attachedImages.indexOf(each) + 1" :title="each.scaled ? t('chatInput.image.scaled') : each.name">
                <button type="button" class="cbody" :aria-label="each.name" @click="shotShown = attachedImages.indexOf(each) + 1">
                  <img class="cthumb" :src="`data:${each.mediaType};base64,${each.data}`" alt="" draggable="false" @load="sized(each.key, $event)" />
                  <span class="cname">{{ each.name }}</span>
                  <span v-if="imageSizes[each.key]" class="cmeta">{{ imageSizes[each.key] }}</span>
                </button>
                <button class="cx" type="button" :aria-label="t('chatInput.remove')" :title="t('chatInput.remove')" @click="remove(each.key)"><Icon name="close" :size="10" /></button>
              </span>
              <span v-else-if="each.kind === 'pdf'" class="chip" :title="each.name">
                <span class="cbody">
                  <Icon name="file" :size="12" class="cic" />
                  <span class="cname">{{ each.name }}</span>
                  <span class="cmeta">{{ pdfSize(each.data) }}</span>
                </span>
                <button class="cx" type="button" :aria-label="t('chatInput.remove')" :title="t('chatInput.remove')" @click="remove(each.key)"><Icon name="close" :size="10" /></button>
              </span>
              <span v-else :class="['chip', { on: opened === each.key }]">
                <button class="cbody" type="button" :aria-expanded="opened === each.key" @click="opened = opened === each.key ? null : each.key">
                  <Icon :name="each.name ? 'terminal' : 'copy'" :size="12" class="cic" />
                  <span class="cname">{{ each.name ?? t("chatInput.pasted") }}</span>
                  <span class="cmeta">{{ t("chatInput.pasted.lines", { count: each.lines }) }}</span>
                </button>
                <button class="cx" type="button" :aria-label="t('chatInput.remove')" :title="t('chatInput.remove')" @click="remove(each.key)"><Icon name="close" :size="10" /></button>
              </span>
            </template>
            <span v-if="readingShown" key="reading" class="chip faint">
              <Spinner :size="11" />
              <span class="cname">{{ t("chatInput.reading") }}</span>
            </span>
          </TransitionGroup>
          <pre v-if="openedText" class="pasted selectable">{{ openedText }}</pre>
        </div>
      </div>
      <ImageLightbox v-if="shotShown !== null" :images="shots" :start="shotShown" :thumb="shotThumb" @close="shotShown = null" />
      <div class="field">
        <div ref="mirror" :class="['mirror', { mono: shell }]" aria-hidden="true">
          <template v-for="(part, i) in drawn" :key="i">
            <span v-if="part.kind === 'plain'">{{ part.text }}</span>
            <span v-else :class="['mark', part.kind]">{{ part.text }}</span>
          </template>
          <span v-if="argHint" class="hint">{{ argHint }}</span>
          <span>&#8203;</span>
        </div>
        <textarea
          ref="field"
          :class="['input', { mono: shell, ghost: !!ghost }]"
          rows="1"
          :spellcheck="!shell"
          :placeholder="placeholder"
          :disabled="disabled"
          :aria-label="t('chatInput.field')"
          role="combobox"
          aria-autocomplete="list"
          :aria-expanded="!!menuOpen"
          :aria-controls="menuOpen ? menuId : undefined"
          :aria-activedescendant="activeOption"
          @input="onInput"
          @keydown="onKey"
          @keyup="onCaret"
          @click="onCaret"
          @select="onCaret"
          @focus="onCaret"
          @scroll="onScroll"
          @paste="onPaste"
        ></textarea>
      </div>


      <div v-if="notice" class="notice" role="status">
        <Icon name="info" :size="12" />
        <span>{{ notice }}</span>
      </div>

      <div ref="chipsRow" :class="['chips', { narrow }]">
        <button ref="clip" class="chip-btn icon" type="button" :disabled="disabled" :title="t('chatInput.attach')" :aria-label="t('chatInput.attach')" @click="attachMenu?.show(clip!)" @keydown.enter.prevent="attachMenu?.show(clip!, true)">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.4 11.1-9.2 9.2a6 6 0 0 1-8.5-8.5l9.2-9.2a4 4 0 0 1 5.7 5.7l-9.2 9.2a2 2 0 0 1-2.8-2.8l8.5-8.5" /></svg>
        </button>
        <!-- A line that starts with "!": a pill of the others' form, which only says so (the user's
             word, 16 Nov: "diğer butonlar ile aynı özellikte olsun"). -->
        <span
          v-if="shell"
          class="chip-btn shell-chip"
          :title="t('chatInput.shell.title')"
          :role="narrow ? 'img' : undefined"
          :aria-label="narrow ? t('chatInput.shell') : undefined"
        >
          <ChatIcon name="terminal" :size="14" class="lead" />
          <span v-if="!narrow" class="lbl">{{ t("chatInput.shell") }}</span>
        </span>
        <!-- The model and its effort in one pill, as the VS Code extension shows them (the user's
             word, 14 Nov); the menu over it sets both. -->
        <button
          class="chip-btn model-chip"
          type="button"
          :data-chat-model-chip="id"
          :disabled="disabled"
          :title="narrow ? modelTitle : t('chatInput.model.title')"
          :aria-label="narrow ? modelTitle : undefined"
          @click="emit('dialog', { kind: 'model' })"
        >
          <ChatIcon name="brain" :size="14" class="lead" />
          <template v-if="!narrow">
            <span class="lbl">{{ modelLabel }}</span>
            <span v-if="effortText" class="effort">{{ effortText }}</span>
          </template>
        </button>
        <!-- The mode, its menu over it as the model's (the user's word, 15 Nov: no arrow, no dialog). -->
        <button
          class="chip-btn mode-chip"
          type="button"
          :data-chat-mode-chip="id"
          :disabled="disabled"
          :title="narrow ? modeLabel : t('chatInput.mode.title')"
          :aria-label="narrow ? modeLabel : undefined"
          @click="emit('dialog', { kind: 'mode' })"
        >
          <ChatIcon :name="modeIcon" :size="14" class="lead" />
          <span v-if="!narrow" class="lbl">{{ modeLabel }}</span>
        </button>
        <span class="grow"></span>
        <button v-if="contextShare !== null" class="ctx" type="button" :disabled="disabled" :title="t('chatInput.context.title', { share: contextText })" @click="emit('dialog', { kind: 'context' })">
          <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
            <circle cx="7" cy="7" r="5" class="track" />
            <circle cx="7" cy="7" r="5" :class="['arc', ring.tone]" :stroke-dasharray="ring.dash" transform="rotate(-90 7 7)" />
          </svg>
          <span>{{ narrow ? contextText : t("chatInput.context", { share: contextText }) }}</span>
        </button>
        <button
          :class="['send', { stop: running && !hasContent }]"
          type="button"
          :disabled="disabled || (!running && !hasContent)"
          :title="running && !hasContent ? t('chatInput.stop') : t('chatInput.send')"
          :aria-label="running && !hasContent ? t('chatInput.stop') : t('chatInput.send')"
          @click="onSendButton"
        >
          <Icon v-if="running && !hasContent" name="stop" :size="11" />
          <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M5.5 11.5 12 5l6.5 6.5" /></svg>
        </button>
      </div>
    </div>

    <SlashMenu v-if="menuOpen === 'slash'" :id="menuId" :items="slashItems" :active="slashActive" :sectioned="!slash?.query" :place="place" @hover="slashActive = $event" @pick="pickCommand($event, 'click')" />
    <MentionMenu v-else-if="menuOpen === 'mention'" :id="menuId" :items="mentionItems" :active="mentionActive" :loading="mentionLoading" :place="place" @hover="mentionActive = $event" @pick="pickMention" />
    <AttachMenu ref="attachMenu" :terminals="terminals" @mention="attachMention" @image="attachImage" @terminal="addTerminal" />
  </div>
</template>

<style scoped>
.composer {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* ---- the queue ---- */

.queue {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 0 4px;
}

.qrow {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 26px;
  padding: 0 4px 0 10px;
  border: 1px dashed var(--line-strong);
  border-radius: var(--radius-control);
  font-size: 12.5px;
}

.qtag {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-faint);
}

.qtext {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-soft);
}

.qx {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-muted);
}

.qx:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* ---- the box ---- */

.box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 10px 8px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: var(--bg-input);
}

.box:focus-within {
  border-color: var(--line-lit);
}

.box.dropping {
  border-color: var(--claude-line);
  background: var(--claude-bg);
}

.box.shell {
  border-color: var(--warn-line);
}

.composer.off .box {
  opacity: 0.6;
}

/* The field and its drawing lie one on the other: same box, same text, same wrap. */
.field {
  position: relative;
}

.mirror,
.input {
  box-sizing: border-box;
  width: 100%;
  margin: 0;
  padding: 2px 2px 2px 3px;
  border: none;
  font-family: var(--font);
  font-size: 13.5px;
  line-height: 20px;
  letter-spacing: normal;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: normal;
  tab-size: 4;
  overflow-y: scroll;
  scrollbar-gutter: stable;
}

.mirror.mono,
.input.mono {
  font-family: var(--font-mono);
  font-size: 12.5px;
}

.mirror {
  position: absolute;
  inset: 0;
  color: var(--text-strong);
  pointer-events: none;
}

/* Its scrollbar only keeps the same room as the field's; the field's own is the one seen. */
.mirror::-webkit-scrollbar-thumb,
.mirror::-webkit-scrollbar-track {
  background: transparent;
}

.input {
  position: relative;
  display: block;
  min-height: 44px;
  max-height: 240px;
  transition: height 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
  resize: none;
  outline: none;
  background: transparent;
  color: transparent;
  caret-color: var(--text-strong);
}

.input::placeholder {
  color: var(--text-faint);
}

.input.ghost::placeholder {
  font-style: italic;
  color: var(--text-muted);
}

.input::selection {
  background: var(--find-bg);
  color: transparent;
}

.input:disabled {
  cursor: default;
}

/* A chip that takes no room: its ground and ring are drawn outside the letters. */
.mark.mention {
  border-radius: 4px;
  background: var(--bg-tag);
  box-shadow: 0 0 0 2px var(--bg-tag);
  color: var(--text-link);
}

.mark.command {
  color: var(--claude-text);
}

/* A skill named anywhere in the message, as the sent message draws it (ChatTurn): its ground and
   ring outside the letters, as a mention's. */
.mark.skill {
  border-radius: 4px;
  background: var(--bg-skill);
  box-shadow: 0 0 0 2px var(--bg-skill);
  color: var(--claude-text);
}

.mark.ultra {
  background: linear-gradient(90deg, var(--data-1), var(--data-2), var(--data-3), var(--data-4), var(--data-5), var(--data-6));
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}

.hint {
  color: var(--text-faint);
}

/* ---- attachments ---- */

/* Over the message, edge to edge, a line under it. Its height is what it holds (`refsHeight`), so
   it opens upward and shuts down onto the line; its content stands at its bottom, so the box's top
   uncovers it as it rises. Shut, it takes no room and its margin gives back the box's gap. */
.refs {
  flex: none;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  overflow: hidden;
  margin: 0 -10px -8px;
  box-shadow: inset 0 -1px transparent;
  transition:
    height 0.24s cubic-bezier(0.2, 0.8, 0.2, 1),
    margin 0.24s cubic-bezier(0.2, 0.8, 0.2, 1),
    box-shadow 0.24s ease;
}

.refs.shown {
  margin: -6px -10px 0;
  box-shadow: inset 0 -1px var(--line);
}

.refs.still {
  transition: none;
}

.refs-in {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 2px 10px 8px;
}

.atts {
  position: relative;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

/* A chip comes in growing to its size and goes shrinking where it stood (`pinChip`); the others
   move to their new places. */
.chipf-enter-active,
.chipf-leave-active {
  transition:
    opacity 0.2s ease,
    transform 0.24s cubic-bezier(0.2, 0.8, 0.2, 1);
}

.chipf-enter-from,
.chipf-leave-to {
  opacity: 0;
  transform: scale(0.88);
}

.chip.chipf-leave-active {
  position: absolute;
}

.chipf-move {
  transition:
    opacity 0.2s ease,
    transform 0.24s cubic-bezier(0.2, 0.8, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .input,
  .refs,
  .chipf-enter-active,
  .chipf-leave-active,
  .chipf-move {
    transition: none;
  }
}

/* What goes with the message, each a small chip (the VS Code extension's, the user's word, 14 Nov):
   a thumbnail or an icon, its name, what it holds fainter; its × shows over its end on hover. */
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

/* The stash and the reading: a chip of words alone, dashed. */
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

.chip.on {
  border-color: var(--line-lit);
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

span.cbody,
.cbody:has(> .cic) {
  padding-left: 6px;
}

.cthumb {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border-radius: 3px;
  object-fit: cover;
}

.cic {
  flex-shrink: 0;
  color: var(--text-muted);
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

.pasted {
  max-height: 160px;
  margin: 0;
  padding: 8px 10px;
  overflow: auto;
  border: 1px solid var(--line);
  border-radius: var(--radius-control);
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 11.5px;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-log);
}

.notice {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-muted);
}

/* ---- the chips row ---- */

.chips {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

/* The composer's buttons share one form, round ends and no edge (the user's word, 14 Nov:
   "bütün butonlar aynı forma, full rounded"); one that holds only an icon is a circle. Hover
   leans the ground toward the text's colour, lighter in the dark and darker in the light. */
/* A chip never shrinks: the row goes narrow instead (`narrow`, weighed in the script). */
.chip-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  height: 26px;
  box-sizing: border-box;
  padding: 0 9px 0 11px;
  border: 1px solid transparent;
  border-radius: 13px;
  background: var(--bg-control-hover);
  font: inherit;
  font-size: 12.5px;
  color: var(--text);
  white-space: nowrap;
}

button.chip-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--bg-control-hover) 86%, var(--text));
}

.chip-btn:disabled {
  opacity: 0.5;
}

.chip-btn.icon {
  flex-shrink: 0;
  justify-content: center;
  width: 26px;
  padding: 0;
  color: var(--text-nav);
}

/* The model's pill (the effort fainter beside the name) and the mode's: no chevron, as the VS Code
   extension's. The shell's says only that the line runs in the shell. */
.chip-btn.model-chip,
.chip-btn.mode-chip,
.chip-btn.shell-chip {
  gap: 6px;
  padding: 0 11px 0 9px;
}

/* The chip's icon before its words: the model's brain, the mode's own. */
.chip-btn .lead {
  flex-shrink: 0;
  color: var(--text-nav);
}

/* Narrow, a chip holds its icon alone: a circle, as the clip's. */
.chips.narrow .chip-btn {
  justify-content: center;
  width: 26px;
  padding: 0;
}

.model-chip .effort {
  flex-shrink: 0;
  color: var(--text-subtle);
}

.chip-btn .lbl {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.grow {
  flex: 1;
}

.ctx {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  height: 26px;
  padding: 0 6px;
  border: none;
  border-radius: var(--radius-control);
  background: transparent;
  font: inherit;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

.ctx:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text);
}

.ctx .track {
  fill: none;
  stroke: var(--line-strong);
  stroke-width: 2;
}

.ctx .arc {
  fill: none;
  stroke: var(--text-soft);
  stroke-width: 2;
  stroke-linecap: round;
}

.ctx .arc.warm {
  stroke: var(--warn);
}

.ctx .arc.hot {
  stroke: var(--crash);
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
}

.send.stop {
  background: var(--bg-control-hover);
  color: var(--text-strong);
}

.send.stop:hover {
  background: var(--line-strong);
}
</style>
