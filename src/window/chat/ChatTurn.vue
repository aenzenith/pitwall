<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, reactive, ref, watch } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import type { Shot } from "../../lib/cardImages";
import { skillSegments } from "../../lib/chat/composer";
import type { ChangeEntry, ChatEntry, CommandEntry, ResultEntry, TaskEntry, ToolEntry, UserEntry } from "../../lib/chat/store";
import { compactNumber, exitMeta, QUIET_TOOLS, VERBS } from "../../lib/chat/tools";
import { CHAT_ROW } from "../../lib/chat/ui";
import { language, t } from "../../lib/i18n";
import { isMac } from "../../lib/platform";
import ChatIcon from "./ChatIcon.vue";
import ChatMarkdown from "./ChatMarkdown.vue";
import CommandRow from "./CommandRow.vue";
import ToolRow from "./ToolRow.vue";

/**
 * One turn of the conversation: the user's message (right, in its box, with its actions on
 * hover), then what Claude did and said, in order: its words as Markdown, a tool call per line
 * (several lookups in a row folded into one), thinking folded, errors, notices, and the turn's
 * time at its end. In Focus view only the user's words and Claude's answers show, the steps
 * between them folded into one row.
 */
export type Turn = { key: string; user: UserEntry | null; items: ChatEntry[]; last: boolean };

const props = defineProps<{
  turn: Turn;
  project: string;
  commands: Map<string, CommandEntry>;
  changes: Map<string, ChangeEntry>;
  tasks: Map<string, TaskEntry>;
  runs: Map<string, CommandEntry>;
  /** Tool calls a card waits for. */
  waiting: Set<string>;
  /** The skills the CLI lists (`skillNames`). */
  skills: Set<string>;
  expanded: Set<string>;
  bookmarked: boolean;
  focus: boolean;
  timestamps: boolean;
  /** The turn is still under way. */
  running: boolean;
  /** Fork and bookmark need the session; edit needs a uuid. */
  canFork: boolean;
}>();

const emit = defineEmits<{
  toggle: [key: string];
  long: [title: string, text: string];
  copy: [text: string];
  edit: [user: UserEntry, text: string];
  rewind: [user: UserEntry];
  fork: [user: UserEntry];
  bookmark: [user: UserEntry];
  resend: [text: string];
  /** A picture shown large in the chat's lightbox (ChatView), all the chat's pictures with it. */
  picture: [key: string];
}>();

/** The chat's id and clock, for a shell row's command opened under it (ChatView gives them). */
const row = inject(CHAT_ROW, null);

/** The message's images; a click shows one large in the chat's lightbox (ChatView), which steps
 * through every picture of the chat, those Claude read too. Its key there: `<message key>:<index>`. */
const pictures = computed<Shot[]>(() => (props.turn.user?.images ?? []).map((image, i) => ({ n: i + 1, src: `data:${image.mediaType};base64,${image.data}` })));
const shotKey = (n: number) => `${props.turn.user?.key}:${n - 1}`;
/** Each picture's size in pixels, read as its thumbnail loads. */
const shotSizes = reactive<Record<number, string>>({});
function sized(n: number, event: Event): void {
  const img = event.target as HTMLImageElement;
  if (img.naturalWidth) shotSizes[n] = `${img.naturalWidth}×${img.naturalHeight}`;
}

/** The message's copy was just pressed: its icon is a green tick for a moment. */
const copied = ref(false);
let copiedFor: ReturnType<typeof setTimeout> | undefined;
function copyOut(text: string): void {
  emit("copy", text);
  copied.value = true;
  clearTimeout(copiedFor);
  copiedFor = setTimeout(() => (copied.value = false), 1500);
}

type Item =
  | { kind: "entry"; key: string; entry: ChatEntry; /** One of a Focus view fold's steps, drawn under its line while it is open. */ inSteps?: boolean }
  | { kind: "group"; key: string; tools: ToolEntry[] }
  | { kind: "steps"; key: string; entries: ChatEntry[] };

/** Several lookups in a row fold into one row. */
const GROUP_FROM = 3;

/* Which thinking has any text, as one string: it is looked at again with each word that streams
   in, and the rows are worked out again only when it changes, not at every word. */
const thoughtKeys = computed(() =>
  props.turn.items
    .filter((entry) => entry.kind === "thinking" && /\S/.test(entry.text))
    .map((entry) => entry.key)
    .join("\n"),
);
const thought = computed(() => new Set(thoughtKeys.value.split("\n")));

/** Thinking with no text is drawn as nothing (O8). */
function shown(entry: ChatEntry): boolean {
  if (entry.kind === "thinking") return thought.value.has(entry.key);
  if (entry.kind === "result") return !!entry.duration || entry.error;
  return true;
}

const items = computed<Item[]>(() => {
  const list = props.turn.items.filter(shown);
  if (props.focus) {
    // Focus view: the answers alone, the steps between them in one fold each. Opened, the steps
    // follow its line, each drawn as the full view draws it (a `!` run, a command's output, an
    // interrupt, a cleared conversation, not the tools alone).
    const out: Item[] = [];
    let steps: ChatEntry[] = [];
    const flush = () => {
      if (steps.length) {
        const key = `steps-${steps[0].key}`;
        out.push({ kind: "steps", key, entries: steps });
        if (props.expanded.has(key)) for (const entry of steps) out.push({ kind: "entry", key: entry.key, entry, inSteps: true });
      }
      steps = [];
    };
    for (const entry of list) {
      if (entry.kind === "text" || entry.kind === "error") {
        flush();
        out.push({ kind: "entry", key: entry.key, entry });
      } else if (entry.kind !== "result" && !(entry.kind === "shell" && !props.runs.get(entry.run))) steps.push(entry);
    }
    flush();
    return out;
  }
  const out: Item[] = [];
  let run: ToolEntry[] = [];
  const flush = () => {
    if (run.length >= GROUP_FROM) out.push({ kind: "group", key: `group-${run[0].key}`, tools: run });
    else for (const tool of run) out.push({ kind: "entry", key: tool.key, entry: tool });
    run = [];
  };
  for (const entry of list) {
    if (entry.kind === "tool" && QUIET_TOOLS.has(entry.name) && !props.waiting.has(entry.id)) run.push(entry);
    else {
      flush();
      out.push({ kind: "entry", key: entry.key, entry });
    }
  }
  flush();
  return out;
});

/** "Okudu 3 · Aradı 2": what a fold holds, by the verbs its rows use (a tool with none by its name). */
function groupLine(tools: ToolEntry[]): string {
  const counts = new Map<string, number>();
  for (const tool of tools) {
    const verb = VERBS[tool.name];
    const label = verb ? t(verb) : tool.name;
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return [...counts.entries()].map(([label, count]) => `${label} ${count}`).join(" · ");
}

const groupRunning = (tools: ToolEntry[]) => tools.some((tool) => !tool.result && tool.ended === null && !tool.history);

/* ---------- the user's message ---------- */

const LONG_LINES = 14;
const LONG_CHARS = 1600;
const userLong = computed(() => {
  const text = props.turn.user?.text ?? "";
  return text.length > LONG_CHARS || text.split("\n").length > LONG_LINES;
});
/** A slash command sent as typed: drawn as a command, not as words. */
const userCommand = computed(() => {
  const text = props.turn.user?.text ?? "";
  return /^\/[\w:.-]+(\s|$)/.test(text) && !text.includes("\n") && text.length < 200;
});
const userShell = computed(() => (props.turn.user?.text ?? "").startsWith("!"));
/** Its words, the skills it names apart (the user's word, 16 Nov): those the CLI lists and those
 * Claude took up in this turn (a chat whose list hasn't come yet). */
const userParts = computed(() => {
  const text = props.turn.user?.text ?? "";
  if (!text.includes("/") || userShell.value) return [{ text, kind: "plain" as const }];
  let skills = props.skills;
  for (const entry of props.turn.items) {
    if (entry.kind !== "tool" || entry.name !== "Skill" || typeof entry.input.skill !== "string") continue;
    if (skills === props.skills) skills = new Set(props.skills);
    skills.add(entry.input.skill);
  }
  return skillSegments(text, skills);
});

/* The message stays at the conversation's top while its turn is read, until the next one's
   pushes it away, as in the VS Code extension (the user's word, 15 Nov): sticky within its turn,
   the whole width, its pictures small chips at its top as the composer shows them. A long one
   keeps its start there (the user's word, 16 Nov: its last lines, cut through a line at the top,
   read as broken): as the turn scrolls under it, it is cut where its end would be, its words
   fading out there, down to `STICK_MOST`; that much stays until the turn's end comes up to it, the
   rest (`tail`) given back by a margin below it and a spacer after it. */
const STICK_MOST = 132;
/** The turn's gap, which the spacer after a long message brings with it. */
const TURN_GAP = 8;
const userRow = ref<HTMLElement | null>(null);
const userHeight = ref(0);
const tail = computed(() => {
  const over = userHeight.value - STICK_MOST;
  return over > TURN_GAP ? over : 0;
});
let userWatch: ResizeObserver | null = null;
watch(userRow, (row) => {
  userWatch?.disconnect();
  if (!row) return;
  userWatch ??= new ResizeObserver(() => {
    if (!userRow.value) return;
    userHeight.value = userRow.value.offsetHeight;
    cut();
  });
  userWatch.observe(row);
});

/** Where a long message is cut: written on the row itself, as the conversation scrolls, with no
 * render of the turn. The row is its turn's first, so its own place is the turn's top. */
let scroller: HTMLElement | null = null;
let cutAt = -1;
function cut() {
  const row = userRow.value;
  if (!row) return;
  const height = userHeight.value;
  const show = tail.value ? Math.round(Math.min(height, Math.max(STICK_MOST, row.parentElement!.getBoundingClientRect().top + height - row.getBoundingClientRect().top))) : height;
  if (show === cutAt) return;
  cutAt = show;
  row.style.setProperty("--show", `${show}px`);
  if (show < height) row.dataset.cut = "";
  else delete row.dataset.cut;
}
watch([userRow, tail], ([row, long]) => {
  scroller?.removeEventListener("scroll", cut);
  scroller = long && row ? row.closest<HTMLElement>(".scroll") : null;
  scroller?.addEventListener("scroll", cut, { passive: true });
  cutAt = -1;
  cut();
});
onBeforeUnmount(() => {
  userWatch?.disconnect();
  scroller?.removeEventListener("scroll", cut);
});

const editing = ref<string | null>(null);
const editor = ref<HTMLTextAreaElement | null>(null);

function startEdit(): void {
  editing.value = props.turn.user?.text ?? "";
  void nextTick(() => {
    editor.value?.focus();
    editor.value?.setSelectionRange(editor.value.value.length, editor.value.value.length);
  });
}

function sendEdit(): void {
  const user = props.turn.user;
  const text = editing.value?.trim();
  if (!user || !text) return;
  editing.value = null;
  emit("edit", user, text);
}

function onEditKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    editing.value = null;
  } else if (event.key === "Enter" && (isMac ? event.metaKey : event.ctrlKey)) {
    event.preventDefault();
    sendEdit();
  }
}

/* ---------- times ---------- */

function clock(at: number | null): string {
  if (!at) return "";
  return new Intl.DateTimeFormat(language.value, { hour: "2-digit", minute: "2-digit" }).format(at);
}

function duration(ms: number): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  if (seconds < 60) return t("chat.time.seconds", { count: seconds });
  return t("chat.time.minutes", { minutes: Math.floor(seconds / 60), seconds: seconds % 60 });
}

function resultLine(entry: ResultEntry): string {
  const parts: string[] = [];
  if (entry.duration) parts.push(duration(entry.duration));
  if (entry.cost) parts.push(new Intl.NumberFormat(language.value, { style: "currency", currency: "USD", maximumFractionDigits: entry.cost < 0.1 ? 3 : 2 }).format(entry.cost));
  if (props.timestamps && entry.time) parts.push(clock(entry.time));
  return parts.join(" · ");
}

function thinkingLine(entry: Extract<ChatEntry, { kind: "thinking" }>): string {
  if (entry.started && entry.ended && entry.ended > entry.started) return t("chat.thinking.for", { time: duration(entry.ended - entry.started) });
  return t("chat.thinking.done");
}

function errorText(entry: Extract<ChatEntry, { kind: "error" }>): string {
  const known: Record<string, Parameters<typeof t>[0]> = {
    rate_limit: "chat.error.rateLimit",
    overloaded: "chat.error.overloaded",
    authentication_failed: "chat.error.auth",
    billing_error: "chat.error.billing",
    max_output_tokens: "chat.error.maxOutput",
    prompt_too_long: "chat.error.tooLong",
    server_error: "chat.error.server",
  };
  const key = entry.code ? known[entry.code] : undefined;
  return key ? t(key) : t("chat.error.generic");
}

const commandOf = (tool: ToolEntry) => (tool.name === "Bash" ? props.commands.get(tool.id) : undefined);
const changeOf = (tool: ToolEntry) => props.changes.get(tool.id);
const taskOf = (tool: ToolEntry) => props.tasks.get(tool.id);
</script>

<template>
  <section class="turn">
    <!-- The user's message. -->
    <div v-if="turn.user" ref="userRow" :class="['user', { command: userCommand || userShell, editing: editing !== null }]" :style="tail ? { marginBottom: `-${tail}px` } : undefined">
      <div v-if="editing === null" class="bubble-wrap">
        <div class="actions" data-nofind>
          <button type="button" :class="['act', { done: copied }]" :title="t(copied ? 'chat.copied' : 'chat.action.copy')" :aria-label="t('chat.action.copy')" @click="copyOut(turn.user.text)">
            <Icon v-if="copied" name="check" :size="13" />
            <ChatIcon v-else name="copy" :size="13" />
          </button>
          <button v-if="turn.user.uuid && canFork" type="button" class="act" :title="t('chat.action.edit')" :aria-label="t('chat.action.edit')" @click="startEdit">
            <ChatIcon name="edit" :size="13" />
          </button>
          <button v-if="turn.user.uuid" type="button" class="act" :title="t('chat.action.rewind')" :aria-label="t('chat.action.rewind')" @click="emit('rewind', turn.user)">
            <ChatIcon name="rewind" :size="13" />
          </button>
          <button v-if="turn.user.uuid && canFork" type="button" class="act" :title="t('chat.action.fork')" :aria-label="t('chat.action.fork')" @click="emit('fork', turn.user)">
            <ChatIcon name="fork" :size="13" />
          </button>
          <button
            v-if="turn.user.uuid && canFork"
            type="button"
            :class="['act', { on: bookmarked }]"
            :aria-pressed="bookmarked"
            :title="t(bookmarked ? 'chat.action.unbookmark' : 'chat.action.bookmark')"
            :aria-label="t(bookmarked ? 'chat.action.unbookmark' : 'chat.action.bookmark')"
            @click="emit('bookmark', turn.user)"
          >
            <ChatIcon :name="bookmarked ? 'bookmark-on' : 'bookmark'" :size="13" />
          </button>
        </div>
        <div class="bubble">
          <span v-if="bookmarked" class="mark" :title="t('chat.bookmarked')"><ChatIcon name="bookmark-on" :size="11" /></span>
          <!-- Its pictures as the composer showed them (the user's word, 15 Nov): small chips over a line. -->
          <div v-if="pictures.length" :class="['shots', { alone: !turn.user.text }]">
            <button v-for="picture in pictures" :key="picture.n" type="button" class="shot" :data-shot="shotKey(picture.n)" @click="emit('picture', shotKey(picture.n))">
              <img class="sthumb" :src="picture.src ?? ''" alt="" draggable="false" @load="sized(picture.n, $event)" />
              <span class="sname">{{ t("board.image", { n: picture.n }) }}</span>
              <span v-if="shotSizes[picture.n]" class="smeta">{{ shotSizes[picture.n] }}</span>
            </button>
          </div>
          <div v-if="turn.user.attached" class="attached" data-nofind>{{ t("chat.user.attached", { count: turn.user.attached }) }}</div>
          <div :class="['text', 'selectable', { clamp: userLong }]" data-find><template v-for="(part, i) in userParts" :key="i"><span v-if="part.kind === 'skill'" class="skill">{{ part.text }}</span><template v-else>{{ part.text }}</template></template></div>
          <button v-if="userLong" type="button" class="link" data-nofind @click="emit('long', t('chat.user.title'), turn.user.text)">{{ t("chat.showAll") }}</button>
        </div>
        <div v-if="timestamps && turn.user.time" class="when">{{ clock(turn.user.time) }}</div>
      </div>
      <div v-else class="edit-box">
        <textarea ref="editor" v-model="editing" class="selectable" rows="3" :aria-label="t('chat.action.edit')" @keydown="onEditKey"></textarea>
        <div class="edit-foot">
          <span class="hint">{{ t("chat.edit.hint", { keys: isMac ? "⌘↩" : "Ctrl+Enter" }) }}</span>
          <span class="grow"></span>
          <button type="button" class="control" @click="editing = null">{{ t("common.cancel") }}</button>
          <button type="button" class="control primary" :disabled="!editing?.trim()" @click="sendEdit">{{ t("chat.edit.send") }}</button>
        </div>
      </div>
    </div>
    <div v-if="tail" class="user-tail" :style="{ height: `${tail - TURN_GAP}px` }" aria-hidden="true"></div>

    <template v-for="item in items" :key="item.key">
      <!-- Lookups folded together. -->
      <div v-if="item.kind === 'group'" class="group">
        <button type="button" class="group-line" :aria-expanded="expanded.has(item.key)" @click="emit('toggle', item.key)">
          <span :class="['icon', { breath: groupRunning(item.tools) }]"><ChatIcon name="search" :size="14" /></span>
          <span class="verb">{{ t("chat.tool.group", { count: item.tools.length }) }}</span>
          <span class="arg">{{ groupLine(item.tools) }}</span>
          <span class="grow"></span>
          <span :class="['chev', { down: expanded.has(item.key) }]"><Icon name="chevron" :size="12" /></span>
        </button>
        <Collapse :show="expanded.has(item.key)">
          <div class="group-body">
            <ToolRow
              v-for="tool in item.tools"
              :key="tool.key"
              :tool="tool"
              :project="project"
              :open="expanded.has(tool.key)"
              :waiting="waiting.has(tool.id)"
              @toggle="emit('toggle', tool.key)"
              @long="(title, text) => emit('long', title, text)"
              @picture="(key) => emit('picture', key)"
            />
          </div>
        </Collapse>
      </div>
      <!-- Focus view: the steps between two answers, in one fold; opened, they follow its line. -->
      <div v-else-if="item.kind === 'steps'" class="group steps">
        <button type="button" class="group-line" :aria-expanded="expanded.has(item.key)" @click="emit('toggle', item.key)">
          <span class="icon"><ChatIcon name="list" :size="14" /></span>
          <span class="verb">{{ t("chat.focus.steps", { count: item.entries.length }) }}</span>
          <span class="grow"></span>
          <span :class="['chev', { down: expanded.has(item.key) }]"><Icon name="chevron" :size="12" /></span>
        </button>
      </div>
      <div v-else :class="['entry', { 'in-steps': item.inSteps }]">
        <ChatMarkdown v-if="item.entry.kind === 'text'" class="answer selectable" :entry="item.entry" :project="project" />
        <ToolRow
          v-else-if="item.entry.kind === 'tool'"
          :tool="item.entry"
          :project="project"
          :change="changeOf(item.entry)"
          :command="commandOf(item.entry)"
          :task="taskOf(item.entry)"
          :open="expanded.has(item.entry.key)"
          :waiting="waiting.has(item.entry.id)"
          @toggle="emit('toggle', item.entry.key)"
          @long="(title, text) => emit('long', title, text)"
          @picture="(key) => emit('picture', key)"
        />
        <div v-else-if="item.entry.kind === 'thinking'" class="thinking">
          <button type="button" class="think-line" :aria-expanded="expanded.has(item.entry.key)" @click="emit('toggle', item.entry.key)">
            <ChatIcon name="brain" :size="13" />
            <span>{{ item.entry.streaming && !item.entry.ended ? t("chat.thinking.now") : thinkingLine(item.entry) }}</span>
            <span :class="['chev', { down: expanded.has(item.entry.key) }]"><Icon name="chevron" :size="11" /></span>
          </button>
          <Collapse :show="expanded.has(item.entry.key)">
            <div class="think-text selectable" data-find>{{ item.entry.text }}</div>
          </Collapse>
        </div>
        <div v-else-if="item.entry.kind === 'error'" class="error-card" role="alert">
          <div class="error-head">
            <Icon name="info" :size="13" />
            <span>{{ errorText(item.entry) }}</span>
          </div>
          <div v-if="item.entry.text" class="error-text selectable" data-find>{{ item.entry.text }}</div>
          <div class="error-actions" data-nofind>
            <button v-if="turn.user && turn.last" type="button" class="control" @click="emit('resend', turn.user.text)">{{ t("chat.error.retry") }}</button>
            <button v-if="item.entry.text" type="button" class="control" @click="emit('copy', item.entry.text)">{{ t("chat.action.copy") }}</button>
          </div>
        </div>
        <div v-else-if="item.entry.kind === 'compact'" class="compact">
          <ChatIcon name="compact" :size="13" />
          <span>{{ item.entry.trigger === "auto" ? t("chat.compact.auto") : t("chat.compact.manual") }}</span>
          <span v-if="item.entry.pre" class="faint">
            {{ item.entry.post ? t("chat.compact.tokens", { pre: compactNumber(item.entry.pre), post: compactNumber(item.entry.post) }) : t("chat.compact.from", { pre: compactNumber(item.entry.pre) }) }}
          </span>
        </div>
        <div v-else-if="item.entry.kind === 'reset'" class="divider" role="separator">
          <span>{{ item.entry.trigger === "clear" ? t("chat.reset.clear") : t("chat.reset.new") }}</span>
        </div>
        <div v-else-if="item.entry.kind === 'notice'" :class="['notice', item.entry.level]">
          <pre v-if="item.entry.level === 'output'" class="selectable" data-find>{{ item.entry.text }}</pre>
          <template v-else>
            <Icon name="info" :size="13" />
            <span class="selectable" data-find>{{ item.entry.text }}</span>
          </template>
        </div>
        <div v-else-if="item.entry.kind === 'shell' && runs.get(item.entry.run)" class="group">
          <button type="button" class="group-line" :aria-expanded="expanded.has(item.entry.key)" @click="emit('toggle', item.entry.key)">
            <span :class="['icon', { breath: runs.get(item.entry.run)!.state === 'running' }]"><ChatIcon name="terminal" :size="14" /></span>
            <span class="verb">{{ t("chat.shell.ran") }}</span>
            <span class="arg mono" data-find>{{ runs.get(item.entry.run)!.command }}</span>
            <span class="grow"></span>
            <span :class="['meta', { error: runs.get(item.entry.run)!.state === 'failed' }]">{{ exitMeta(runs.get(item.entry.run)!) }}</span>
            <span :class="['chev', { down: expanded.has(item.entry.key) }]"><Icon name="chevron" :size="12" /></span>
          </button>
          <Collapse :show="!!row && expanded.has(item.entry.key)">
            <CommandRow
              v-if="row"
              :id="row.id.value"
              bare
              :path="project"
              :command="runs.get(item.entry.run)!"
              :open="true"
              :now="row.now.value"
              :pending="null"
              @long="(title, text) => emit('long', title, text)"
            />
          </Collapse>
        </div>
        <div v-else-if="item.entry.kind === 'interrupt'" class="interrupt">{{ t("chat.interrupted") }}</div>
        <div v-else-if="item.entry.kind === 'result' && !focus" class="turn-end">
          <span>{{ resultLine(item.entry) }}</span>
        </div>
      </div>
    </template>
  </section>
</template>

<style scoped>
.turn {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* ---- the user's message ---- */

/* Sticky over its turn, on the conversation's own ground: what scrolls under it is hidden, up to
   the room's top (its shadow fills the 8 px above, inside the gap between turns) and fading out
   under it (inside the turn's gap). Under the search bar. */
.user {
  position: sticky;
  top: 8px;
  z-index: 2;
  display: flex;
  padding: 6px 0 4px;
  background: var(--bg-log);
  box-shadow: 0 -8px 0 var(--bg-log);
}

.user::after {
  content: "";
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  height: 8px;
  background: linear-gradient(var(--bg-log), transparent);
  pointer-events: none;
}

/* A long one cut where its end would be (`cut`, `--show` from the row's top): what lies under the
   rest shows and takes the clicks, the ground above and the fade under it kept. Its ground ends at
   the cut, so the fade lies over what scrolls under it, dark to clear, with no hard edge (the
   user's word, 16 Nov: "transparanlık çalışmıyor"). */
.user[data-cut] {
  clip-path: inset(-8px 0 calc(100% - var(--show) - 16px) 0);
  background: linear-gradient(var(--bg-log), var(--bg-log)) 0 0 / 100% var(--show) no-repeat;
}

.user[data-cut]::after {
  top: var(--show);
  height: 16px;
}

/* The bubble stays a whole bubble down to the cut, its edge and round corners drawn there, and its
   words are wiped out inside it, into its own ground, never into the dark under it (the user's
   word, 16 Nov: "tam mesaj balonu gibi gözüksün, içerik içerden silinsin"). Drawn over, never
   resized: the row keeps its height, which the cut is measured from. Its height there: the cut
   less the row's padding (6 above, 4 below). */
.user[data-cut] .bubble {
  clip-path: inset(-8px 0 calc(100% - var(--show) + 10px) 0 round 8px);
}

/* Slowly, from the bottom up (the user's word, 16 Nov); over the words even where they lie in a
   layer of their own (a clamped message's mask makes one). */
.user[data-cut] .bubble::before {
  content: "";
  position: absolute;
  top: calc(var(--show) - 10px - 2px - 72px);
  right: 0;
  left: 0;
  z-index: 1;
  height: 72px;
  background: linear-gradient(transparent, var(--bg-user) 88%);
  pointer-events: none;
}

.user[data-cut] .bubble::after {
  content: "";
  position: absolute;
  top: -1px;
  right: -1px;
  left: -1px;
  box-sizing: border-box;
  z-index: 1;
  height: calc(var(--show) - 10px);
  border: 1px solid var(--line-user);
  border-radius: 8px;
  pointer-events: none;
}

.bubble-wrap {
  position: relative;
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
}

.bubble {
  position: relative;
  padding: 9px 13px;
  border: 1px solid var(--line-user);
  border-radius: 8px;
  background: var(--bg-user);
  font-size: 13.5px;
  line-height: 1.55;
  color: var(--text-strong);
}

.user.command .bubble .text {
  font-family: var(--font-mono);
  font-size: 12.5px;
}

/* A skill the message names: apart from its words, as the composer draws it, its ground and ring
   outside the letters. */
.text .skill {
  border-radius: 4px;
  background: var(--bg-skill);
  box-shadow: 0 0 0 2px var(--bg-skill);
  color: var(--claude-text);
}

.text {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.text.clamp {
  max-height: calc(1.55em * 12);
  overflow: hidden;
  -webkit-mask-image: linear-gradient(to bottom, #000 70%, transparent);
  mask-image: linear-gradient(to bottom, #000 70%, transparent);
}

.mark {
  position: absolute;
  top: -6px;
  right: 10px;
  color: var(--claude);
}

/* The pictures, as the composer's chips: a 14 px thumbnail, the name, the size fainter; a line
   under them parts them from the words, edge to edge. */
.shots {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: -2px -13px 8px;
  padding: 0 13px 8px;
  box-shadow: inset 0 -1px var(--line-user);
}

.shots.alone {
  margin-bottom: -2px;
  padding-bottom: 0;
  box-shadow: none;
}

.shot {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 260px;
  height: 22px;
  box-sizing: border-box;
  padding: 0 8px 0 4px;
  border: 1px solid var(--line-user);
  border-radius: 6px;
  background: var(--bg-control);
  font: inherit;
  font-size: 12px;
  line-height: 1;
  color: var(--text);
}

.sthumb {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border-radius: 3px;
  object-fit: cover;
}

.sname {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--text-strong);
}

.smeta {
  flex-shrink: 0;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  color: var(--text-subtle);
}

.attached {
  margin-bottom: 4px;
  font-size: 11.5px;
  color: var(--text-subtle);
}

.when {
  align-self: flex-end;
  margin-top: 3px;
  font-size: 11px;
  color: var(--text-faint);
}

/* Inside the bubble, at its top right corner with a little room (the user's word, 16 Nov). */
.actions {
  position: absolute;
  top: 5px;
  right: 5px;
  z-index: 1;
  display: flex;
  gap: 1px;
  padding: 2px;
  border: 1px solid var(--line-dialog);
  border-radius: 7px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.12s;
}

/* Shown while the pointer is over the message, or the keyboard is in them: a click leaves focus
   on its button, and that alone doesn't keep them up. Apart, so that a WebView without :has keeps
   the hover. */
.bubble-wrap:hover .actions {
  opacity: 1;
  pointer-events: auto;
}

.actions:has(:focus-visible) {
  opacity: 1;
  pointer-events: auto;
}

.act {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: none;
  color: var(--text-muted);
}

.act:hover {
  background: var(--bg-control-hover);
  color: var(--text);
}

.act.on {
  color: var(--claude);
}

/* Just copied: a green tick for a moment. */
.act.done,
.act.done:hover {
  color: var(--run);
}

.edit-box {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-input);
}

.edit-box textarea {
  display: block;
  box-sizing: border-box;
  width: 100%;
  min-height: 70px;
  padding: 10px 12px;
  border: 0;
  background: none;
  color: var(--text-strong);
  font: inherit;
  font-size: 13.5px;
  line-height: 1.55;
  resize: vertical;
  outline: none;
}

.edit-foot {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px 8px 12px;
}

.hint {
  font-size: 11.5px;
  color: var(--text-faint);
}

.grow {
  flex: 1;
}

/* ---- Claude's side ---- */

.answer {
  padding: 2px 2px;
}

.group {
  border: 1px solid var(--line-inset);
  border-radius: 8px;
  background: var(--bg-panel);
}

.group-line {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-width: 0;
  height: 32px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  text-align: left;
}

.group-line:hover {
  background: var(--bg-row-hover);
}

.group-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 0 8px 8px;
}

/* An entry is the turn's own row; a Focus view fold's step hangs under the fold's line, on a rule
   that runs through the steps. */
.entry {
  display: contents;
}

.entry.in-steps {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-left: 14px;
  padding-left: 10px;
  border-left: 2px solid var(--line-soft);
}

.entry.in-steps + .entry.in-steps {
  margin-top: -8px;
  padding-top: 8px;
}

.icon {
  display: inline-flex;
  width: 14px;
  justify-content: center;
  flex-shrink: 0;
  color: var(--text-muted);
}

/* At work, its icon breathes, as a tool's row's (ToolRow). */
.icon.breath {
  color: var(--text-soft);
  animation: breath 1.6s ease-in-out infinite;
}

@keyframes breath {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.3;
  }
}

@media (prefers-reduced-motion: reduce) {
  .icon.breath {
    animation: none;
    opacity: 0.6;
  }
}

.verb {
  flex-shrink: 0;
  font-weight: 600;
  color: var(--text-strong);
}

.arg {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-soft);
  font-size: 12px;
}

.arg.mono {
  font-family: var(--font-mono);
}

.meta {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-subtle);
}

.meta.error {
  color: var(--crash-text);
}

.chev {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-faint);
  transition: transform 0.15s;
}

.chev.down {
  transform: rotate(90deg);
}

.thinking {
  font-size: 12.5px;
}

.think-line {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 2px 4px;
  border: 0;
  border-radius: 5px;
  background: none;
  color: var(--text-subtle);
  font: inherit;
}

.think-line:hover {
  color: var(--text);
}

.think-text {
  margin: 4px 0 2px 6px;
  padding-left: 12px;
  border-left: 2px solid var(--line-soft);
  white-space: pre-wrap;
  color: var(--text-soft);
  font-style: italic;
  line-height: 1.55;
}

.error-card {
  padding: 10px 12px;
  border: 1px solid var(--crash-line);
  border-radius: 8px;
  background: var(--bg-panel);
}

.error-head {
  display: flex;
  align-items: center;
  gap: 7px;
  font-weight: 600;
  color: var(--crash-text);
}

.error-text {
  margin-top: 6px;
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text-soft);
  max-height: 200px;
  overflow: auto;
}

.error-actions {
  display: flex;
  gap: 6px;
  margin-top: 8px;
}

.compact {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 12px;
  border: 1px dashed var(--line-strong);
  border-radius: 8px;
  font-size: 12.5px;
  color: var(--text-soft);
}

.faint {
  color: var(--text-faint);
}

.divider {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 8px 0;
  font-size: 11.5px;
  color: var(--text-faint);
}

.divider::before,
.divider::after {
  content: "";
  flex: 1;
  height: 1px;
  background: var(--line-soft);
}

.notice {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  font-size: 12.5px;
  color: var(--text-soft);
}

.notice.warning {
  color: var(--warn-text);
}

.notice pre {
  flex: 1;
  margin: 0;
  max-height: 360px;
  overflow: auto;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
}

.interrupt {
  font-size: 12px;
  color: var(--text-subtle);
}

.turn-end {
  display: flex;
  justify-content: flex-start;
  font-size: 11px;
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}

.link {
  align-self: flex-start;
  margin-top: 4px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-link);
  font: inherit;
  font-size: 12px;
}

.link:hover {
  text-decoration: underline;
}

.control {
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text-soft);
  font: inherit;
  font-size: 12px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control.primary {
  border-color: var(--btn-primary-line);
  background: var(--btn-primary-bg);
  color: var(--btn-primary-text);
}

.control.primary:hover:not(:disabled) {
  background: var(--btn-primary-hover);
}

.control:disabled {
  opacity: 0.5;
}
</style>
