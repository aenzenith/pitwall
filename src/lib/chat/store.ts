/**
 * A chat's conversation as the window holds it (docs/SOHBET.md › Window):
 * one reactive model per chat id, fed by the core's "chat" and "chat-exit" events (heard once, in
 * the main window) and by `chat_buffer` when a chat's view comes up or comes back. The CLI's frames
 * are turned into what the view draws: the turns, the Komutlar, Değişiklikler and Görevler of the
 * status line's card, the to-do list, the cards waiting on the user, the queue, the chips' values.
 *
 * Nothing of it is written anywhere, logged or put into a snapshot: it lives in this page's memory
 * while the tab is open, and goes with it.
 */
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, markRaw, reactive, ref, shallowReactive, watch } from "vue";

import { panelAway, revealTab } from "../panel";
import { api, snapshot } from "../store";
import type { TerminalView } from "../types";
import { draftOf, forgetDraft } from "./composer";
import { startChat } from "./open";
import { CHAT_ATTACH, CHAT_CONTEXT, type ChatAttach, type ChatContext, type ComposerApi, type PlanMode, type PlanNotes, type QueuedMessage } from "./ui";
import {
  CHAT_DEFAULTS,
  type AssistantFrame,
  type ChatBuffer,
  type ChatControl,
  type ChatEffort,
  type ChatEvent,
  type ChatExit,
  type ChatFrame,
  type ChatInfo,
  type ChatPending,
  type ChatSettings,
  type ChatState,
  type ContentBlock,
  type ControlRequestFrame,
  type ResultFrame,
  type StreamEventFrame,
  type UserFrame,
} from "./wire";

// ---------------------------------------------------------------- what the view draws

/** A user's message: sent from a window (`sent`), echoed by the CLI, or read from the session's log. */
export type UserEntry = {
  kind: "user";
  key: string;
  uuid: string | null;
  text: string;
  /** Pictures sent from this page's memory (`sent`), drawn small; none for the log's (omitted). */
  images: { mediaType: string; data: string }[];
  /** Pictures and documents named only (their data never comes back). */
  attached: number;
  time: number | null;
  history: boolean;
};
export type TextEntry = { kind: "text"; key: string; message: string; text: string; streaming: boolean; uuid: string | null; time: number | null; history: boolean };
export type ThinkingEntry = { kind: "thinking"; key: string; message: string; text: string; streaming: boolean; started: number | null; ended: number | null; history: boolean };
/** A picture a tool gave back (a Read of an image file): what Claude saw, kept in the chat's memory
 * alone, never written down (the user's word, 15 Nov: to see what was read). */
export type ToolPicture = { mediaType: string; data: string };
export type ToolResult = { text: string; error: boolean; data: unknown; images: number; pictures: ToolPicture[] };
export type ToolEntry = {
  kind: "tool";
  key: string;
  /** The `tool_use_id`. */
  id: string;
  name: string;
  input: Record<string, unknown>;
  /** The input's JSON as it streams in, before the block settles. */
  partial: string;
  streaming: boolean;
  result: ToolResult | null;
  started: number | null;
  ended: number | null;
  /** Seconds the CLI says the tool has run (`tool_progress`). */
  elapsed: number | null;
  denied: boolean;
  history: boolean;
};
export type ResultEntry = {
  kind: "result";
  key: string;
  duration: number | null;
  /** What this turn cost (the running total's step), when known. */
  cost: number | null;
  error: boolean;
  reason: string | null;
  time: number | null;
};
export type ErrorEntry = { kind: "error"; key: string; code: string | null; text: string };
export type CompactEntry = { kind: "compact"; key: string; trigger: string | null; pre: number | null; post: number | null };
export type ResetEntry = { kind: "reset"; key: string; trigger: string | null };
export type NoticeEntry = { kind: "notice"; key: string; level: "info" | "warning" | "output"; text: string };
export type ShellEntry = { kind: "shell"; key: string; run: string };
export type InterruptEntry = { kind: "interrupt"; key: string };

export type ChatEntry =
  | UserEntry
  | TextEntry
  | ThinkingEntry
  | ToolEntry
  | ResultEntry
  | ErrorEntry
  | CompactEntry
  | ResetEntry
  | NoticeEntry
  | ShellEntry
  | InterruptEntry;

/** A row of Komutlar: a Bash call of Claude's, or a `!` line the user ran. */
export type CommandEntry = {
  key: string;
  /** The Bash call's `tool_use_id`; null for a `!` run. */
  tool: string | null;
  /** The `!` run's id; null for a Bash call. */
  run: string | null;
  command: string;
  description: string | null;
  state: "running" | "done" | "failed" | "stopped";
  stdout: string;
  stderr: string;
  code: number | null;
  interrupted: boolean;
  /** The background task it became (`backgroundTaskId`), while it runs there. */
  background: string | null;
  started: number | null;
  ended: number | null;
  history: boolean;
};

/** One hunk of a change, as the CLI's `structuredPatch` gives it. */
export type Hunk = { oldStart: number; oldLines: number; newStart: number; newLines: number; lines: string[] };

/** A row of Değişiklikler: one Edit / MultiEdit / Write / NotebookEdit call. */
export type ChangeEntry = {
  key: string;
  tool: string;
  /** The file as the call named it (absolute). */
  file: string;
  kind: "edit" | "create" | "write" | "notebook";
  hunks: Hunk[];
  added: number;
  removed: number;
  state: "running" | "done" | "failed";
  /** The first line it changed, for "Editörde aç". */
  line: number;
  history: boolean;
};

/** A row of Görevler: a subagent, a background shell, a workflow or a monitor. */
export type TaskEntry = {
  key: string;
  /** The CLI's task id, once it is known. */
  task: string | null;
  /** The call that started it (the Agent/Task tool's, a Bash call's). */
  tool: string | null;
  kind: "agent" | "shell" | "other";
  description: string;
  agentType: string | null;
  state: "running" | "done" | "failed" | "stopped";
  started: number | null;
  ended: number | null;
  tokens: number | null;
  tools: number | null;
  lastTool: string | null;
  summary: string | null;
  background: boolean;
  history: boolean;
};

export type TodoItem = { content: string; state: "pending" | "in_progress" | "completed"; active: string | null };

export type RateLimit = { status: string; resetsAt: number | null; type: string | null; utilization: number | null };

/** The usage notice the user closed (`limitKey`): hidden in every chat until one of another status
 * or limit window comes. Page memory only. */
export const limitClosed = ref<string | null>(null);

export function limitKey(limit: RateLimit): string {
  return `${limit.status}|${limit.type ?? ""}|${limit.resetsAt ?? ""}`;
}
export type Retry = { attempt: number | null; max: number | null; delay: number | null; status: number | null; error: string | null; at: number };

/** A side question (`/btw`) and what came of it. */
export type SideQuestion = { n: number; question: string; answer: string | null; none: boolean; failed: string | null; busy: boolean };

/** The side questions' panel of a chat while it is open or folded away into the status line: its
 * questions and answers, and what is typed in its box. */
export type SideQuestions = { items: SideQuestion[]; draft: string; count: number };

/** The tabs of the card over the status line: Görevler, Değişiklikler, Komutlar. */
export type DetailTab = "tasks" | "changes" | "commands";

/** What the window keeps of how a chat's view was left (page memory only). */
export type ChatUi = {
  /** The card over the status line and the tab it shows, in this chat alone; shut at first (CK3,
   * the user's pick of 16 Nov: the right column gave way to it). */
  card: DetailTab | null;
  /** Rows opened in the card (their keys), and the one asked to come into sight. */
  open: Set<string>;
  /** Conversation rows opened (tool rows, groups, thinking). */
  expanded: Set<string>;
  /** Where the conversation was scrolled, and whether it followed the end. */
  scroll: number | null;
  stuck: boolean;
  todosFolded: boolean;
  /** Every thinking block opened (⌃O). */
  thinkingOpen: boolean;
  /** The card shows only what came since the user's last message (the user's word, 16 Nov). */
  recent: boolean;
  /** The side questions' panel, open or folded into the status line; null once closed (the user's
   * word, 16 Nov: it closes or folds away, and a folded one opens again from the line). */
  btw: SideQuestions | null;
  /** The plan waiting on the user (its request's id) while its screen is folded onto the bar over
   * the composer (PL5); null while it shows. */
  planFolded: string | null;
  /** The mode a plan starts in, as last picked in this chat (PlanStart); auto until picked (the
   * user's word, 25 Nov: no permission asked after every plan), accepting edits where the model
   * has no auto mode (`planModeOf`). */
  planMode: PlanMode;
  /** The comments and words written on the waiting plan, while its screen isn't shown. */
  planNotes: PlanNotes | null;
};

export type ChatModel = {
  id: number;
  /** The last frame's place the window has. */
  seq: number;
  loaded: boolean;
  state: ChatState;
  exit: ChatExit | null;
  info: ChatInfo | null;
  session: string | null;
  history: ChatEntry[];
  entries: ChatEntry[];
  commands: CommandEntry[];
  changes: ChangeEntry[];
  tasks: TaskEntry[];
  todos: TodoItem[] | null;
  pending: ChatPending[];
  queued: QueuedMessage[];
  suggestion: string | null;
  model: string | null;
  mode: string | null;
  effort: string | null;
  /** The context window and what the last main-thread request held of it. */
  contextWindow: number | null;
  contextUsed: number | null;
  rateLimit: RateLimit | null;
  retry: Retry | null;
  /** `system/status`: requesting, compacting, or nothing. */
  status: string | null;
  /** When the turn under way began, and when the last frame came (live ones only). */
  turnStarted: number | null;
  lastFrame: number;
  /** The process's running cost total, as the latest `result` says. */
  cost: number | null;
  bookmarks: Set<string>;
  ui: ChatUi;
};

// ---------------------------------------------------------------- the models

/** The page's own bookkeeping of a chat, never drawn. */
type Inner = {
  key: number;
  /** The tools whose result holds pictures, oldest first (`PICTURES_MOST`). */
  pictured: ToolEntry[];
  /** The message `stream_event`s are about now. */
  streaming: string | null;
  /** Streamed blocks by `${message}:${index}`. */
  blocks: Map<string, TextEntry | ThinkingEntry | ToolEntry>;
  tools: Map<string, ToolEntry>;
  commands: Map<string, CommandEntry>;
  changes: Map<string, ChangeEntry>;
  /** Tasks by task id and by the call that started them. */
  tasks: Map<string, TaskEntry>;
  taskTools: Map<string, TaskEntry>;
  /** The task id of each Bash call running in the foreground (every one gets a task), for when
   * it goes to the background. */
  bashTasks: Map<string, CommandEntry>;
  /** User messages drawn, by uuid. */
  users: Map<string, UserEntry>;
  /** The history frame is being read: entries go to `history`. */
  inHistory: boolean;
  /** The uuid of the last transcript entry seen (an assistant frame or a tool result), for forks. */
  chain: string | null;
  /** Each user message's uuid → the chain's uuid just before it. */
  before: Map<string, string | null>;
  /** Usage counted once per message id. */
  counted: Set<string>;
  fetching: Promise<void> | null;
  /** The ids TaskCreate gave, for TaskUpdate (the Task* to-do tools). */
  taskList: Map<string, TodoItem>;
  /** Pictures of queued messages, until they come out of the queue (page memory only). */
  pendingUsers: Map<string, { images: { mediaType: string; data: string }[]; documents: number }>;
};

const models = shallowReactive(new Map<number, ChatModel>());
const inners = new Map<number, Inner>();

function freshUi(): ChatUi {
  return {
    card: null,
    open: new Set(),
    expanded: new Set(),
    scroll: null,
    stuck: true,
    todosFolded: false,
    thinkingOpen: false,
    recent: false,
    btw: null,
    planFolded: null,
    planMode: "auto",
    planNotes: null,
  };
}

function freshInner(): Inner {
  return {
    key: 0,
    pictured: [],
    streaming: null,
    blocks: new Map(),
    tools: new Map(),
    commands: new Map(),
    changes: new Map(),
    tasks: new Map(),
    taskTools: new Map(),
    bashTasks: new Map(),
    users: new Map(),
    inHistory: false,
    chain: null,
    before: new Map(),
    counted: new Set(),
    fetching: null,
    taskList: new Map(),
    pendingUsers: new Map(),
  };
}

function emptyModel(id: number, ui: ChatUi = freshUi()): ChatModel {
  return reactive({
    id,
    seq: 0,
    loaded: false,
    state: "starting",
    exit: null,
    info: null,
    session: null,
    history: [],
    entries: [],
    commands: [],
    changes: [],
    tasks: [],
    todos: null,
    pending: [],
    queued: [],
    suggestion: null,
    model: null,
    mode: null,
    effort: null,
    contextWindow: null,
    contextUsed: null,
    rateLimit: null,
    retry: null,
    status: null,
    turnStarted: null,
    lastFrame: Date.now(),
    cost: null,
    bookmarks: new Set<string>(),
    ui,
  }) as ChatModel;
}

/** The chat `id`'s model, made empty the first time it is asked for. */
export function chatModel(id: number): ChatModel {
  let model = models.get(id);
  if (!model) {
    model = emptyModel(id);
    models.set(id, model);
    inners.set(id, freshInner());
  }
  return model;
}

/** A control the CLI took, from the composer or a dialog: the chips show what it set at once (the
 * frames tell of a model only at the next turn, of an effort never). */
export function noteControl(id: number, control: ChatControl): void {
  const model = models.get(id);
  if (!model) return;
  if (control.kind === "setModel") model.model = control.model;
  else if (control.kind === "setMode") model.mode = control.mode;
  else if (control.kind === "setEffort") model.effort = control.level;
}

/** The model, only when the chat has one already. */
export function peekChat(id: number): ChatModel | null {
  return models.get(id) ?? null;
}

/** Everything the window knows of the chat, read again from the core: on the view's first show
 * (from the start), and from the last frame it has when it comes back or a gap shows. */
export function loadChat(id: number, again = false): Promise<void> {
  const model = chatModel(id);
  const inner = inners.get(id)!;
  if (inner.fetching) return inner.fetching;
  if (model.loaded && !again) return Promise.resolve();
  // A chat read already goes on from its last frame, none yet (a new chat before its first
  // message) included: what the chips were told of it (a model picked, the effort) stays.
  const fromStart = !model.loaded;
  const since = fromStart ? 0 : model.seq;
  inner.fetching = api
    .chatBuffer(id, since)
    .then((buffer) => take(id, buffer, fromStart))
    .catch(() => undefined)
    .finally(() => {
      const now = inners.get(id);
      if (now) now.fetching = null;
    });
  return inner.fetching;
}

function take(id: number, buffer: ChatBuffer, fromStart: boolean): void {
  const model = models.get(id);
  if (!model) return;
  if (fromStart) {
    // Read from the start: everything drawn so far is drawn again from what the core holds (the
    // view's own choices and the bookmarks stay, and why the process ended, which no frame says).
    const { ui, bookmarks, ...fresh } = emptyModel(id);
    void ui;
    void bookmarks;
    Object.assign(model, fresh);
    model.exit = exits.get(id) ?? null;
    inners.set(id, freshInner());
  }
  const inner = inners.get(id)!;
  for (const frame of buffer.frames ?? []) apply(model, inner, frame, false);
  model.seq = Math.max(model.seq, buffer.seq ?? 0);
  model.loaded = true;
  if (buffer.state) setState(model, buffer.state);
  if (buffer.info) takeInfo(model, buffer.info);
  // What still waits on the user is the core's word, whatever the frames said.
  model.pending = (buffer.pending ?? []).filter((each) => isCard(each.request?.subtype));
  if (model.state === "exited") closeOut(model, inner);
  if (model.state !== "running" && model.state !== "requires_action") model.turnStarted = null;
}

/** A batch of frames as it came: in order after the last one the window has, else read again. */
function hear(event: ChatEvent): void {
  const model = models.get(event.id);
  if (!model) return;
  const inner = inners.get(event.id)!;
  if (!model.loaded || inner.fetching) {
    // The buffer on its way holds these too, or is asked again from its end once it came (a
    // first read that went out before these frames were written doesn't hold them). No read on
    // its way: the view's first one, when it comes up, reads everything.
    void inner.fetching?.then(() => (models.get(event.id) === model && model.seq < event.seq ? loadChat(event.id, true) : undefined));
    return;
  }
  const frames = event.frames ?? [];
  const first = event.seq - frames.length + 1;
  if (first > model.seq + 1) {
    void loadChat(event.id, true);
    return;
  }
  const skip = Math.max(0, model.seq + 1 - first);
  const now = Date.now();
  for (const frame of frames.slice(skip)) {
    model.lastFrame = now;
    apply(model, inner, frame, true);
  }
  model.seq = Math.max(model.seq, event.seq);
}

/** Why each chat's process ended last, kept for a chat whose view isn't up yet and across a read
 * from the start (page memory only). */
const exits = new Map<number, ChatExit>();

function ended(exit: ChatExit): void {
  exits.set(exit.id, exit);
  const model = models.get(exit.id);
  if (!model) return;
  const inner = inners.get(exit.id)!;
  model.exit = exit;
  setState(model, "exited");
  closeOut(model, inner);
  // A process started again runs at its own effort: it is asked again once its info comes.
  model.effort = null;
  effortAsked.delete(exit.id);
}

/** The process is gone: nothing waits on the user, nothing it held runs on, and what waited in its
 * queue will never run. */
function closeOut(model: ChatModel, inner: Inner): void {
  model.pending = [];
  model.queued = [];
  inner.pendingUsers.clear();
  settleAll(model, inner, "exited");
}

/** Forgets the chat: its tab closed. */
export function forgetChat(id: number): void {
  models.delete(id);
  inners.delete(id);
  exits.delete(id);
  composers.delete(id);
  drafts.delete(id);
  attaches.delete(id);
  forgetDraft(id);
}

// ---------------------------------------------------------------- frames

function nextKey(inner: Inner, kind: string): string {
  return `${kind}${++inner.key}`;
}

function target(model: ChatModel, inner: Inner): ChatEntry[] {
  return inner.inHistory ? model.history : model.entries;
}

/** Puts `entry` at the end of `list` and gives back the reactive one, which is the one to change. */
function push<L extends object, T extends L>(list: L[], entry: T): T {
  list.push(entry);
  return list[list.length - 1] as T;
}

function timeOf(stamp: unknown): number | null {
  if (typeof stamp !== "string") return null;
  const at = Date.parse(stamp);
  return Number.isNaN(at) ? null : at;
}

function str(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function num(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

const CARD_KINDS = new Set(["can_use_tool", "elicitation", "request_user_dialog"]);
function isCard(subtype: unknown): boolean {
  return typeof subtype === "string" && CARD_KINDS.has(subtype);
}

function setState(model: ChatModel, state: ChatState): void {
  if (model.state === state) return;
  const busy = state === "running" || state === "requires_action";
  if (busy && model.turnStarted === null) model.turnStarted = Date.now();
  if (!busy) model.turnStarted = null;
  if (state === "running") model.suggestion = null;
  if (state === "idle") {
    // "Turn over, nothing owed": nothing can wait on the user any more.
    model.pending = [];
    model.status = null;
    model.retry = null;
  }
  model.state = state;
}

function takeInfo(model: ChatModel, info: ChatInfo): void {
  model.info = info;
  if (info.model && !model.model) model.model = info.model;
  if (info.permissionMode && !model.mode) model.mode = info.permissionMode;
  if (info.sessionId) model.session = info.sessionId;
  askEffort(model);
}

const EFFORTS = new Set<string>(["low", "medium", "high", "xhigh", "max", "auto"] satisfies ChatEffort[]);
/** The chats whose effort was asked of the CLI this page. */
const effortAsked = new Set<number>();

/** The model button names the model and its effort, and no frame tells of the model before a turn,
 * of the effort never: once its process is up (its info came), a chat asks the CLI's settings for
 * them, as the model menu does. */
function askEffort(model: ChatModel): void {
  const id = model.id;
  if ((model.effort !== null && model.model !== null) || effortAsked.has(id) || model.state === "exited") return;
  effortAsked.add(id);
  api
    .chatControl<{ applied?: { effort?: unknown; model?: unknown } }>(id, { kind: "settings" })
    .then((said) => {
      const effort = said?.applied?.effort;
      const named = said?.applied?.model;
      if (typeof effort === "string" && EFFORTS.has(effort) && models.get(id)?.effort === null) noteControl(id, { kind: "setEffort", level: effort as ChatEffort });
      if (typeof named === "string" && named && models.get(id)?.model === null) noteControl(id, { kind: "setModel", model: named });
    })
    .catch(() => effortAsked.delete(id));
}

function apply(model: ChatModel, inner: Inner, frame: ChatFrame, live: boolean): void {
  if (!frame || typeof frame !== "object") return;
  switch (frame.type) {
    case "stream_event":
      return streamEvent(model, inner, frame as StreamEventFrame);
    case "assistant":
      return assistant(model, inner, frame as AssistantFrame);
    case "user":
      return user(model, inner, frame as UserFrame);
    case "result":
      return result(model, inner, frame as ResultFrame);
    case "system":
      return system(model, inner, frame as { subtype: string; [key: string]: unknown }, live);
    case "prompt_suggestion": {
      const text = str((frame as { suggestion?: unknown }).suggestion);
      model.suggestion = text && chatSettings.value.suggestions ? text : null;
      return;
    }
    case "rate_limit_event": {
      const info = record((frame as { rate_limit_info?: unknown }).rate_limit_info);
      if (!info) return;
      const status = str(info.status) ?? "allowed";
      model.rateLimit = status === "allowed" ? null : { status, resetsAt: num(info.resetsAt), type: str(info.rateLimitType), utilization: num(info.utilization) };
      return;
    }
    case "conversation_reset": {
      const trigger = str((frame as { trigger?: unknown }).trigger);
      if (trigger === "clear") {
        model.history = [];
        model.entries = [];
        model.commands = [];
        model.changes = [];
        model.tasks = [];
        model.todos = null;
        inner.tools.clear();
        inner.commands.clear();
        inner.changes.clear();
        inner.tasks.clear();
        inner.taskTools.clear();
        inner.bashTasks.clear();
        inner.blocks.clear();
        inner.taskList.clear();
        model.contextUsed = null;
      }
      model.cost = null;
      const session = str((frame as { new_conversation_id?: unknown }).new_conversation_id);
      if (session) model.session = session;
      push(target(model, inner), { kind: "reset", key: nextKey(inner, "r"), trigger } as ResetEntry);
      return;
    }
    case "tool_progress": {
      const id = str((frame as { tool_use_id?: unknown }).tool_use_id);
      const tool = id ? inner.tools.get(id) : undefined;
      const elapsed = num((frame as { elapsed_time_seconds?: unknown }).elapsed_time_seconds);
      if (tool && elapsed !== null) tool.elapsed = elapsed;
      return;
    }
    case "control_request": {
      const request = frame as ControlRequestFrame;
      if (!isCard(request.request?.subtype) || model.pending.some((each) => each.requestId === request.request_id)) return;
      model.pending.push({ requestId: request.request_id, request: request.request });
      return;
    }
    case "control_cancel_request": {
      const id = str((frame as { request_id?: unknown }).request_id);
      model.pending = model.pending.filter((each) => each.requestId !== id);
      return;
    }
    case "command_lifecycle": {
      const uuid = str((frame as { command_uuid?: unknown }).command_uuid);
      const state = str((frame as { state?: unknown }).state);
      if (!uuid || state === "queued") return;
      unqueue(model, inner, uuid, live ? Date.now() : null);
      return;
    }
    case "pitwall":
      return pitwall(model, inner, frame as { kind: string; [key: string]: unknown }, live);
    default:
      return;
  }
}

function pitwall(model: ChatModel, inner: Inner, frame: { kind: string; [key: string]: unknown }, live: boolean): void {
  switch (frame.kind) {
    case "info":
      if (frame.info) takeInfo(model, frame.info as ChatInfo);
      return;
    case "history": {
      const frames = Array.isArray(frame.frames) ? (frame.frames as ChatFrame[]) : [];
      const was = inner.inHistory;
      inner.inHistory = true;
      for (const each of frames) apply(model, inner, each, false);
      inner.inHistory = was;
      settleHistory(model);
      return;
    }
    case "state":
      if (typeof frame.state === "string") setState(model, frame.state as ChatState);
      return;
    case "shell":
      return shell(model, inner, frame);
    case "sent": {
      const uuid = str(frame.uuid);
      const message = record(frame.message);
      if (!uuid || !message) return;
      const text = str(message.text) ?? "";
      const all = Array.isArray(message.images) ? (message.images as { mediaType: string; data: string | null }[]) : [];
      const images = all.filter((each): each is { mediaType: string; data: string } => !!each && typeof each.data === "string");
      // Read again from the buffer, a picture comes without its data (the core keeps none of it
      // there): it is counted with the attachments instead of drawn.
      const documents = (Array.isArray(message.documents) ? message.documents.length : 0) + all.length - images.length;
      const priority = (str(message.priority) ?? "next") as QueuedMessage["priority"];
      model.suggestion = null;
      // Sent while a turn runs (now, or at that point of the frames read again): it waits in the
      // CLI's queue until its lifecycle says it began.
      if ((model.state === "running" || model.state === "requires_action") && priority !== "now" && !inner.users.has(uuid)) {
        if (!model.queued.some((each) => each.uuid === uuid)) model.queued.push({ uuid, text, priority });
        inner.pendingUsers.set(uuid, { images, documents });
        return;
      }
      addUser(model, inner, { uuid, text, images, attached: documents, time: live ? Date.now() : null });
      if (live && model.turnStarted === null) model.turnStarted = Date.now();
      return;
    }
    case "answered": {
      const id = str(frame.requestId);
      model.pending = model.pending.filter((each) => each.requestId !== id);
      return;
    }
    case "unsent":
      return unsent(model, inner, frame);
    case "error": {
      const text = str(frame.text) ?? "";
      push(target(model, inner), { kind: "error", key: nextKey(inner, "e"), code: str(frame.reason), text } as ErrorEntry);
      return;
    }
    default:
      return;
  }
}

/** Messages the process ended before it took (`unsent`): out of the conversation and the queue,
 * their words back in the composer, unless it holds them already (the buffer read again). */
function unsent(model: ChatModel, inner: Inner, frame: { [key: string]: unknown }): void {
  const uuids = Array.isArray(frame.uuids) ? frame.uuids.filter((each): each is string => typeof each === "string") : [];
  for (const uuid of uuids) {
    const entry = inner.users.get(uuid);
    if (entry) {
      for (const list of [model.entries, model.history]) {
        const at = list.indexOf(entry);
        if (at >= 0) list.splice(at, 1);
      }
      inner.users.delete(uuid);
    }
    inner.pendingUsers.delete(uuid);
  }
  model.queued = model.queued.filter((each) => !uuids.includes(each.uuid));
  const text = str(frame.text) ?? "";
  if (!text) return;
  const draft = draftOf(model.id);
  if (draft.text.includes(text)) return;
  draft.text = draft.text.trim() ? `${text}\n\n${draft.text}` : text;
}

/** A queued message began (or was taken into the turn): out of the queue, into the conversation. */
function unqueue(model: ChatModel, inner: Inner, uuid: string, time: number | null): void {
  const at = model.queued.findIndex((each) => each.uuid === uuid);
  if (at < 0) return;
  const [queued] = model.queued.splice(at, 1);
  const extra = inner.pendingUsers.get(uuid);
  inner.pendingUsers.delete(uuid);
  addUser(model, inner, { uuid, text: queued.text, images: extra?.images ?? [], attached: extra?.documents ?? 0, time });
}

/** Messages the core says it took out of the queue (an interrupt that cancelled them, a cancel). */
export function dropQueued(id: number, uuids: string[]): void {
  const model = models.get(id);
  if (!model) return;
  model.queued = model.queued.filter((each) => !uuids.includes(each.uuid));
  const inner = inners.get(id);
  for (const uuid of uuids) inner?.pendingUsers.delete(uuid);
}

function addUser(model: ChatModel, inner: Inner, from: { uuid: string | null; text: string; images: UserEntry["images"]; attached: number; time: number | null }): UserEntry | null {
  if (from.uuid) {
    const known = inner.users.get(from.uuid);
    if (known) {
      if (from.time && !known.time) known.time = from.time;
      return known;
    }
  }
  const entry = push(target(model, inner), {
    kind: "user",
    key: nextKey(inner, "u"),
    uuid: from.uuid,
    text: from.text,
    images: from.images,
    attached: from.attached,
    time: from.time,
    history: inner.inHistory,
  } as UserEntry);
  if (from.uuid) {
    inner.users.set(from.uuid, entry);
    inner.before.set(from.uuid, inner.chain);
  }
  return entry;
}

// ---- the assistant's words, streamed then settled

function streamEvent(model: ChatModel, inner: Inner, frame: StreamEventFrame): void {
  if (frame.parent_tool_use_id) return;
  const event = frame.event;
  if (!event) return;
  model.retry = null;
  const list = target(model, inner);
  switch (event.type) {
    case "message_start": {
      const message = record((event as { message?: unknown }).message);
      inner.streaming = str(message?.id) ?? `m${++inner.key}`;
      return;
    }
    case "content_block_start": {
      const message = inner.streaming;
      const block = event.content_block as Record<string, unknown> | undefined;
      if (!message || !block || typeof event.index !== "number") return;
      const key = `${message}:${event.index}`;
      if (inner.blocks.has(key)) return;
      const now = Date.now();
      if (block.type === "text") {
        inner.blocks.set(key, push(list, { kind: "text", key: nextKey(inner, "t"), message, text: str(block.text) ?? "", streaming: true, uuid: null, time: now, history: inner.inHistory } as TextEntry));
      } else if (block.type === "thinking") {
        inner.blocks.set(key, push(list, { kind: "thinking", key: nextKey(inner, "k"), message, text: str(block.thinking) ?? "", streaming: true, started: now, ended: null, history: inner.inHistory } as ThinkingEntry));
      } else if ((block.type === "tool_use" || block.type === "server_tool_use") && typeof block.id === "string") {
        const tool = addTool(model, inner, block.id, str(block.name) ?? "", {}, true);
        inner.blocks.set(key, tool);
      }
      return;
    }
    case "content_block_delta": {
      const message = inner.streaming;
      if (!message || typeof event.index !== "number" || !event.delta) return;
      const entry = inner.blocks.get(`${message}:${event.index}`);
      if (!entry || !entry.streaming) return;
      const delta = event.delta;
      if (entry.kind === "text" && delta.type === "text_delta") entry.text += delta.text ?? "";
      else if (entry.kind === "thinking" && delta.type === "thinking_delta") entry.text += delta.thinking ?? "";
      else if (entry.kind === "tool" && delta.type === "input_json_delta") entry.partial += delta.partial_json ?? "";
      return;
    }
    case "content_block_stop": {
      // Thinking is over when its block is (the text after it streams on); it stays unsettled
      // until the message's frame gives its whole text.
      const message = inner.streaming;
      if (!message || typeof event.index !== "number") return;
      const entry = inner.blocks.get(`${message}:${event.index}`);
      if (entry?.kind === "thinking" && entry.ended === null) entry.ended = Date.now();
      return;
    }
    default:
      return;
  }
}

function assistant(model: ChatModel, inner: Inner, frame: AssistantFrame): void {
  const message = frame.message;
  if (!message) return;
  model.retry = null;
  // A subagent's: it counts toward its task, and stays out of the conversation.
  if (frame.parent_tool_use_id) {
    subagentStep(inner, frame.parent_tool_use_id, message.content ?? [], message.usage, message.id);
    return;
  }
  if (frame.uuid) inner.chain = frame.uuid;
  const time = timeOf(frame.timestamp) ?? (inner.inHistory ? null : Date.now());
  if (Array.isArray(frame.supersedes) && frame.supersedes.length) drop(model, frame.supersedes);
  if (message.usage && message.id && !inner.inHistory) {
    const usage = message.usage;
    model.contextUsed = (usage.input_tokens ?? 0) + (usage.cache_read_input_tokens ?? 0) + (usage.cache_creation_input_tokens ?? 0) + (usage.output_tokens ?? 0);
  }
  const list = target(model, inner);
  if (frame.error) {
    const text = (message.content ?? []).map((block) => (block.type === "text" ? (block as { text: string }).text : "")).join("\n").trim();
    // Its streamed text, if any came, is the same words: the error takes its place.
    removeStreamed(model, inner, message.id, "text");
    push(list, { kind: "error", key: nextKey(inner, "e"), code: frame.error, text } as ErrorEntry);
    return;
  }
  for (const block of message.content ?? []) {
    if (block.type === "text") {
      const text = (block as { text: string }).text ?? "";
      const streamed = unsettled(list, message.id, "text") as TextEntry | undefined;
      if (streamed) {
        streamed.text = text;
        streamed.streaming = false;
        streamed.uuid = frame.uuid ?? null;
        streamed.time = time;
      } else if (text.trim()) {
        push(list, { kind: "text", key: nextKey(inner, "t"), message: message.id, text, streaming: false, uuid: frame.uuid ?? null, time, history: inner.inHistory } as TextEntry);
      }
    } else if (block.type === "thinking") {
      const text = (block as { thinking: string }).thinking ?? "";
      const streamed = unsettled(list, message.id, "thinking") as ThinkingEntry | undefined;
      if (streamed) {
        streamed.text = text || streamed.text;
        streamed.streaming = false;
        streamed.ended = streamed.ended ?? Date.now();
      } else if (text.trim()) {
        push(list, { kind: "thinking", key: nextKey(inner, "k"), message: message.id, text, streaming: false, started: null, ended: null, history: inner.inHistory } as ThinkingEntry);
      }
    } else if ((block.type === "tool_use" || block.type === "server_tool_use") && typeof (block as { id?: unknown }).id === "string") {
      const use = block as { id: string; name: string; input: Record<string, unknown> };
      const known = inner.tools.get(use.id);
      const tool = known ?? addTool(model, inner, use.id, use.name, use.input ?? {}, false);
      // Not streamed: it began when its frame was written (a buffer read later says so too).
      if (!known && !inner.inHistory && time !== null) tool.started = time;
      tool.name = use.name ?? tool.name;
      tool.input = use.input ?? {};
      tool.partial = "";
      tool.streaming = false;
      if (tool.started === null) tool.started = time;
      toolSettled(model, inner, tool);
    }
  }
}

/** The first block of message `id` and `kind` still streaming. */
function unsettled(list: ChatEntry[], id: string, kind: "text" | "thinking"): ChatEntry | undefined {
  for (let i = list.length - 1; i >= 0 && list.length - i < 400; i--) {
    const entry = list[i];
    if (entry.kind === kind && entry.message === id && entry.streaming) {
      // The earliest still streaming: frames settle blocks in their order.
      let first = entry;
      for (let j = i - 1; j >= 0 && i - j < 400; j--) {
        const before = list[j];
        if (before.kind === kind && before.message === id && before.streaming) first = before;
      }
      return first;
    }
  }
  return undefined;
}

function removeStreamed(model: ChatModel, inner: Inner, id: string, kind: "text"): void {
  const list = target(model, inner);
  const keep = list.filter((entry) => !(entry.kind === kind && entry.message === id));
  if (keep.length !== list.length) {
    if (inner.inHistory) model.history = keep;
    else model.entries = keep;
  }
}

/** `supersedes`: earlier frames the CLI takes back (a refusal's fallback, a tool call it couldn't read). */
function drop(model: ChatModel, uuids: string[]): void {
  const gone = new Set(uuids);
  model.entries = model.entries.filter((entry) => !("uuid" in entry && entry.uuid && gone.has(entry.uuid)));
}

function addTool(model: ChatModel, inner: Inner, id: string, name: string, input: Record<string, unknown>, streaming: boolean): ToolEntry {
  const tool = push(target(model, inner), {
    kind: "tool",
    key: nextKey(inner, "x"),
    id,
    name,
    input,
    partial: "",
    streaming,
    result: null,
    started: inner.inHistory ? null : Date.now(),
    ended: null,
    elapsed: null,
    denied: false,
    history: inner.inHistory,
  } as ToolEntry);
  inner.tools.set(id, tool);
  return tool;
}

const EDIT_TOOLS = new Set(["Edit", "MultiEdit", "Write", "NotebookEdit"]);
const AGENT_TOOLS = new Set(["Agent", "Task"]);

/** A tool call's input is whole: its rows in the status line's card, its to-do list. */
function toolSettled(model: ChatModel, inner: Inner, tool: ToolEntry): void {
  const input = tool.input;
  if (tool.name === "Bash" && !inner.commands.has(tool.id)) {
    const command = push(model.commands, {
      key: nextKey(inner, "c"),
      tool: tool.id,
      run: null,
      command: str(input.command) ?? "",
      description: str(input.description),
      state: tool.result ? (tool.result.error ? "failed" : "done") : inner.inHistory ? "done" : "running",
      stdout: "",
      stderr: "",
      code: null,
      interrupted: false,
      background: null,
      started: tool.started,
      ended: null,
      history: inner.inHistory,
    } as CommandEntry);
    inner.commands.set(tool.id, command);
  } else if (EDIT_TOOLS.has(tool.name) && !inner.changes.has(tool.id)) {
    const file = str(input.file_path) ?? str(input.notebook_path) ?? "";
    const change = push(model.changes, {
      key: nextKey(inner, "d"),
      tool: tool.id,
      file,
      kind: tool.name === "Write" ? "write" : tool.name === "NotebookEdit" ? "notebook" : "edit",
      hunks: [],
      added: 0,
      removed: 0,
      state: inner.inHistory ? "done" : "running",
      line: 1,
      history: inner.inHistory,
    } as ChangeEntry);
    inner.changes.set(tool.id, change);
  } else if (AGENT_TOOLS.has(tool.name) && !inner.taskTools.has(tool.id)) {
    const task = push(model.tasks, {
      key: nextKey(inner, "g"),
      task: null,
      tool: tool.id,
      kind: "agent",
      description: str(input.description) ?? str(input.name) ?? "",
      agentType: str(input.subagent_type),
      state: inner.inHistory ? "done" : "running",
      started: tool.started,
      ended: null,
      tokens: null,
      tools: null,
      lastTool: null,
      summary: null,
      background: input.run_in_background === true,
      history: inner.inHistory,
    } as TaskEntry);
    inner.taskTools.set(tool.id, task);
  } else if (tool.name === "TodoWrite" && Array.isArray(input.todos)) {
    model.todos = (input.todos as Record<string, unknown>[]).map((todo) => ({
      content: str(todo.content) ?? "",
      state: (str(todo.status) ?? "pending") as TodoItem["state"],
      active: str(todo.activeForm),
    }));
  } else if (tool.name === "TaskUpdate") {
    const id = str(input.taskId);
    const item = id ? inner.taskList.get(id) : undefined;
    if (item) {
      const status = str(input.status);
      if (status === "deleted") inner.taskList.delete(id!);
      else if (status) item.state = status as TodoItem["state"];
      if (str(input.subject)) item.content = str(input.subject)!;
      if (str(input.activeForm)) item.active = str(input.activeForm);
      model.todos = [...inner.taskList.values()].map((each) => ({ ...each }));
    }
  }
}

function subagentStep(inner: Inner, parent: string, content: ContentBlock[], usage: AssistantFrame["message"]["usage"], message: string): void {
  const task = inner.taskTools.get(parent);
  if (!task) return;
  for (const block of content) {
    if (block.type === "tool_use") {
      task.tools = (task.tools ?? 0) + 1;
      task.lastTool = str((block as { name?: unknown }).name);
    }
  }
  if (usage && !inner.counted.has(message)) {
    inner.counted.add(message);
    task.tokens = (task.tokens ?? 0) + (usage.output_tokens ?? 0);
  }
}

// ---- what comes back: tool results, the user's own lines

const INTERRUPTED = /^\[Request interrupted by user[^\]]*\]$/;

function user(model: ChatModel, inner: Inner, frame: UserFrame): void {
  if (frame.parent_tool_use_id) return;
  const content = frame.message?.content;
  const blocks = Array.isArray(content) ? content : null;
  const results = blocks ? blocks.filter((block) => block.type === "tool_result") : [];
  if (results.length) {
    if (frame.uuid) inner.chain = frame.uuid;
    const at = inner.inHistory ? null : (timeOf(frame.timestamp) ?? Date.now());
    for (const block of results) toolResult(model, inner, block as { tool_use_id: string; content?: unknown; is_error?: boolean }, results.length === 1 ? frame.tool_use_result : undefined, at);
    return;
  }
  const text = blocks ? blocks.map((block) => (block.type === "text" ? (block as { text: string }).text : "")).join("\n") : typeof content === "string" ? content : "";
  const attached = blocks ? blocks.filter((block) => block.type === "image" || block.type === "document").length : 0;
  const time = timeOf(frame.timestamp);
  // The echo of a message a window sent: its time, and it out of the queue.
  if (frame.isReplay) {
    if (frame.uuid) unqueue(model, inner, frame.uuid, time ?? Date.now());
    const known = frame.uuid ? inner.users.get(frame.uuid) : undefined;
    if (known) {
      known.time = known.time ?? time;
      return;
    }
    // What the CLI made itself comes back as a replay too (since 2.1.314): a local command's
    // output (`/context`, `set_model`'s "Set model to …"), its caveat, a slash command's own
    // line. Read as the log's lines are; never drawn as the user's words.
    if (cliLine(model, inner, text, frame.uuid ?? null, time ?? Date.now(), false, attached)) return;
    const words = cleanText(text);
    if (words || attached) addUser(model, inner, { uuid: frame.uuid ?? null, text: words, images: [], attached, time: time ?? Date.now() });
    return;
  }
  if (frame.isSynthetic) return;
  if (cliLine(model, inner, text, frame.uuid ?? null, time, inner.inHistory, attached)) return;
  // Anything else the CLI adds as a user line while it runs (a task's notification, a hook's
  // words) is no message of the user's.
  if (!inner.inHistory) return;
  const words = cleanText(text);
  if (words || attached) addUser(model, inner, { uuid: frame.uuid ?? null, text: words, images: [], attached, time });
}

/** A user line the CLI wrote rather than the user, handled here (drawn as what it is, or
 * skipped); false for the user's own words. `typed`: a slash command's line is drawn as it was
 * typed (the log's; a live one came as the sent echo). */
function cliLine(model: ChatModel, inner: Inner, text: string, uuid: string | null, time: number | null, typed: boolean, attached = 0): boolean {
  const trimmed = text.trim();
  // No words: the CLI's empty line, unless the user sent a picture or a file alone.
  if (!trimmed) return attached === 0;
  if (INTERRUPTED.test(trimmed)) {
    push(target(model, inner), { kind: "interrupt", key: nextKey(inner, "i") } as InterruptEntry);
    return true;
  }
  // The output of a slash command the CLI ran itself (`/context`, `/cost`, `/model`…), or of a
  // `!` line the terminal ran: shown as output, never as the user's words. One short line (a
  // switch's "Set model to …") is a notice; more, or a shell's, is the output it was.
  const local = tagged(text, ["local-command-stdout", "local-command-stderr", "bash-stdout", "bash-stderr"]);
  if (local !== null && !/<bash-input>/.test(text)) {
    const shown = local.trim();
    if (shown) {
      const line = !/<bash-std/.test(text) && !shown.includes("\n") && shown.length <= 160;
      push(target(model, inner), { kind: "notice", key: nextKey(inner, "n"), level: line ? "info" : "output", text: line ? shown.replace(/`([^`]+)`/g, "$1") : local } as NoticeEntry);
    }
    return true;
  }
  const command = /<command-name>\s*([^<]*?)\s*<\/command-name>/.exec(text);
  if (command || /<command-message>/.test(text)) {
    if (typed && command) {
      const args = /<command-args>([\s\S]*?)<\/command-args>/.exec(text)?.[1]?.trim() ?? "";
      const name = command[1].startsWith("/") ? command[1] : `/${command[1]}`;
      addUser(model, inner, { uuid, text: args ? `${name} ${args}` : name, images: [], attached: 0, time });
    }
    return true;
  }
  if (/<bash-input>/.test(text)) {
    // `!` lines carried with a message (the chat's, `ShellCarry`) or written alone (the
    // terminal's): each line, what it printed, then the words that went with them, which keep
    // the message's id.
    const words = cleanText(text.replace(/<bash-input>[\s\S]*?<\/bash-input>/g, ""));
    for (const part of text.matchAll(/<bash-input>([\s\S]*?)<\/bash-input>|((?:<bash-(?:stdout|stderr)>[\s\S]*?<\/bash-(?:stdout|stderr)>)+)/g)) {
      if (part[1] !== undefined) {
        addUser(model, inner, { uuid: null, text: `!${part[1]}`, images: [], attached: 0, time });
        continue;
      }
      const printed = tagged(part[2], ["bash-stdout", "bash-stderr"]);
      if (printed?.trim()) push(target(model, inner), { kind: "notice", key: nextKey(inner, "n"), level: "output", text: printed } as NoticeEntry);
    }
    if (words || attached) addUser(model, inner, { uuid, text: words, images: [], attached, time });
    return true;
  }
  // A line wholly in one of the CLI's tags (a caveat, a task's notification, a hook's words, a
  // reminder) or in several of them, one after the other.
  return cliOnly(trimmed);
}

/** Text made only of tagged blocks (`<a-b>…</a-b>`, one or more): the CLI's, never typed. A
 * walk, not one pattern: a pattern of repeated lazy blocks backtracks without end on long text. */
function cliOnly(text: string): boolean {
  let rest = text;
  while (rest) {
    const open = /^<([a-z][a-z-]*)(?:\s[^>]*)?>/.exec(rest);
    if (!open) return false;
    const close = rest.indexOf(`</${open[1]}>`, open[0].length);
    if (close < 0) return false;
    rest = rest.slice(close + open[1].length + 3).trimStart();
  }
  return true;
}

/** The text of the tags named, or null when the text holds none of them. */
function tagged(text: string, tags: string[]): string | null {
  let found = false;
  const parts: string[] = [];
  for (const tag of tags) {
    const pattern = new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`, "g");
    for (const match of text.matchAll(pattern)) {
      found = true;
      if (match[1].trim()) parts.push(match[1].replace(/\x1b\[[0-9;]*m/g, ""));
    }
  }
  return found ? parts.join("\n") : null;
}

/** The tags the CLI wraps its own additions to a user line in; never the user's words. */
const CLI_TAGS = [
  "system-reminder",
  "local-command-caveat",
  "local-command-stdout",
  "local-command-stderr",
  "command-name",
  "command-message",
  "command-args",
  "bash-stdout",
  "bash-stderr",
  "task-notification",
  "user-prompt-submit-hook",
];
const CLI_BLOCKS = new RegExp(`<(${CLI_TAGS.join("|")})(?:\\s[^>]*)?>[\\s\\S]*?</\\1>`, "g");

/** The user's words without what the CLI adds to them (reminders, a command's caveat or output). */
function cleanText(text: string): string {
  return text.replace(CLI_BLOCKS, "").trim();
}

/** How many tool pictures a chat holds; past them the oldest give their data back (their count
 * stays on the row). */
const PICTURES_MOST = 30;

function resultText(content: unknown): { text: string; images: number; pictures: ToolPicture[] } {
  if (typeof content === "string") return { text: content, images: 0, pictures: [] };
  if (!Array.isArray(content)) return { text: "", images: 0, pictures: [] };
  let images = 0;
  const parts: string[] = [];
  const pictures: ToolPicture[] = [];
  for (const block of content as Record<string, unknown>[]) {
    if (block?.type === "text" && typeof block.text === "string") parts.push(block.text);
    else if (block?.type === "image") {
      images++;
      const source = record(block.source);
      const mediaType = str(source?.media_type);
      const data = str(source?.data);
      if (source?.type === "base64" && mediaType?.startsWith("image/") && data) pictures.push(markRaw({ mediaType, data }));
    }
  }
  return { text: parts.join("\n"), images, pictures };
}

function toolResult(model: ChatModel, inner: Inner, block: { tool_use_id: string; content?: unknown; is_error?: boolean }, data: unknown, now: number | null): void {
  const tool = inner.tools.get(block.tool_use_id);
  const { text, images, pictures } = resultText(block.content);
  const error = block.is_error === true;
  if (tool) {
    tool.result = { text, error, data: data ?? null, images, pictures };
    if (pictures.length) {
      if (!inner.pictured.includes(tool)) inner.pictured.push(tool);
      let held = inner.pictured.reduce((sum, each) => sum + (each.result?.pictures.length ?? 0), 0);
      while (held > PICTURES_MOST && inner.pictured.length > 1) {
        const oldest = inner.pictured.shift();
        if (oldest?.result) {
          held -= oldest.result.pictures.length;
          oldest.result.pictures = [];
        }
      }
    }
    tool.ended = now;
    tool.streaming = false;
  }
  const command = inner.commands.get(block.tool_use_id);
  if (command) commandResult(command, text, error, record(data), now);
  const change = inner.changes.get(block.tool_use_id);
  if (change) changeResult(change, error, record(data));
  const task = inner.taskTools.get(block.tool_use_id);
  if (task && !task.background) {
    // A subagent in the foreground ends with its call's result, whenever its task said so.
    if (task.state === "running") task.state = error ? "failed" : "done";
    if (now !== null) task.ended = now;
  }
  if (tool?.name === "TaskCreate" && !error) {
    const made = record(record(data)?.task);
    const id = str(made?.id);
    if (id) {
      inner.taskList.set(id, { content: str(made?.subject) ?? str(tool.input.subject) ?? "", state: "pending", active: str(tool.input.activeForm) });
      model.todos = [...inner.taskList.values()].map((each) => ({ ...each }));
    }
  }
  if (tool?.name === "TaskList" && !error) {
    const tasks = record(data)?.tasks;
    if (Array.isArray(tasks)) {
      inner.taskList.clear();
      for (const each of tasks as Record<string, unknown>[]) {
        const id = str(each.id);
        if (id) inner.taskList.set(id, { content: str(each.subject) ?? "", state: (str(each.status) ?? "pending") as TodoItem["state"], active: null });
      }
      model.todos = [...inner.taskList.values()].map((each) => ({ ...each }));
    }
  }
}

function commandResult(command: CommandEntry, text: string, error: boolean, data: Record<string, unknown> | null, now: number | null): void {
  command.ended = now;
  if (data) {
    command.stdout = str(data.stdout) ?? "";
    command.stderr = str(data.stderr) ?? "";
    command.interrupted = data.interrupted === true;
    const background = str(data.backgroundTaskId);
    if (background) command.background = background;
  } else {
    command.stdout = text;
  }
  if (error && !command.stdout && !command.stderr) command.stderr = text.replace(/^Exit code \d+\s*\n?/, "");
  const exit = /^Exit code (\d+)/.exec(text);
  command.code = exit ? Number(exit[1]) : error ? null : 0;
  if (command.background && !error) {
    // Sent to the background: it runs on, its row ends with its task.
    command.state = "running";
    command.code = null;
    command.ended = null;
  } else command.state = command.interrupted ? "stopped" : error ? "failed" : "done";
}

function changeResult(change: ChangeEntry, error: boolean, data: Record<string, unknown> | null): void {
  if (error) {
    change.state = "failed";
    return;
  }
  change.state = "done";
  if (!data) return;
  const patch = Array.isArray(data.structuredPatch) ? (data.structuredPatch as Hunk[]).filter((hunk) => hunk && Array.isArray(hunk.lines)) : [];
  if (str(data.filePath)) change.file = str(data.filePath)!;
  if (patch.length) {
    change.hunks = patch;
  } else if (change.kind === "write" && typeof data.content === "string") {
    // A new file (or one written whole): every line of it, added.
    const lines = data.content.replace(/\n$/, "").split("\n");
    change.hunks = [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: lines.length, lines: lines.map((line) => `+${line}`) }];
  } else if (change.kind === "notebook" && typeof data.new_source === "string") {
    const old = typeof data.old_source === "string" ? data.old_source.split("\n") : [];
    const now = data.new_source.split("\n");
    change.hunks = [{ oldStart: 1, oldLines: old.length, newStart: 1, newLines: now.length, lines: [...old.map((line) => `-${line}`), ...now.map((line) => `+${line}`)] }];
  }
  if (change.kind === "write") change.kind = data.type === "create" ? "create" : "write";
  let added = 0;
  let removed = 0;
  for (const hunk of change.hunks) {
    for (const line of hunk.lines) {
      if (line.startsWith("+")) added++;
      else if (line.startsWith("-")) removed++;
    }
  }
  change.added = added;
  change.removed = removed;
  change.line = firstChanged(change.hunks);
}

/** The first line of the file as it is now that the change touched. */
function firstChanged(hunks: Hunk[]): number {
  const hunk = hunks[0];
  if (!hunk) return 1;
  let line = hunk.newStart || 1;
  for (const each of hunk.lines) {
    if (each.startsWith("+") || each.startsWith("-")) return Math.max(1, line);
    if (each.startsWith(" ")) line++;
  }
  return Math.max(1, hunk.newStart);
}

// ---- turns' ends, the CLI's system frames, `!` runs

function result(model: ChatModel, inner: Inner, frame: ResultFrame): void {
  if (frame.parent_tool_use_id) return;
  model.retry = null;
  model.status = null;
  const total = num(frame.total_cost_usd);
  const cost = total === null ? null : model.cost !== null && total >= model.cost ? total - model.cost : total;
  if (total !== null) model.cost = total;
  const usage = frame.modelUsage ? Object.values(frame.modelUsage) : [];
  const window = usage.reduce((most, each) => Math.max(most, each?.contextWindow ?? 0), 0);
  if (window) model.contextWindow = window;
  const list = target(model, inner);
  const failed = frame.is_error === true || (frame.subtype !== "success" && frame.subtype !== undefined);
  if (failed) {
    const text = (frame.errors?.length ? frame.errors.join("\n") : str(frame.result)) ?? "";
    push(list, { kind: "error", key: nextKey(inner, "e"), code: frame.terminal_reason ?? frame.subtype ?? null, text } as ErrorEntry);
  }
  for (const denial of frame.permission_denials ?? []) {
    const tool = inner.tools.get(denial.tool_use_id);
    if (tool) tool.denied = true;
  }
  push(list, {
    kind: "result",
    key: nextKey(inner, "z"),
    duration: num(frame.duration_ms),
    cost,
    error: failed,
    reason: str(frame.terminal_reason),
    time: inner.inHistory ? null : Date.now(),
  } as ResultEntry);
  settleAll(model, inner, frame.terminal_reason?.startsWith("aborted") ? "stopped" : null);
  if (!inner.inHistory && model.state !== "requires_action") model.turnStarted = null;
}

/** A turn ended or the process went: nothing streams any more, and a call without its result
 * stopped with it (`how`; null leaves calls be: a result may still come for one). A turn's end
 * leaves what runs in the background; the process's (`exited`) takes it along too, as no frame of
 * it will come any more (a `!` run is the core's own, and goes on). */
function settleAll(model: ChatModel, inner: Inner, how: "stopped" | "exited" | null): void {
  const now = Date.now();
  for (const entry of inner.blocks.values()) {
    if (entry.streaming) {
      entry.streaming = false;
      if (entry.kind === "thinking") entry.ended = entry.ended ?? now;
    }
  }
  inner.blocks.clear();
  inner.streaming = null;
  if (!how) return;
  const all = how === "exited";
  for (const tool of inner.tools.values()) if (!tool.result && tool.ended === null) tool.ended = now;
  for (const command of model.commands) {
    if (command.state === "running" && (all || !command.background) && command.tool) {
      command.state = "stopped";
      command.ended = now;
    }
  }
  for (const change of model.changes) if (change.state === "running") change.state = "failed";
  for (const task of model.tasks) {
    if (task.state === "running" && (all || !task.background)) {
      task.state = "stopped";
      task.ended = now;
    }
  }
}

/** The log's turns are over: nothing in them still runs. */
function settleHistory(model: ChatModel): void {
  for (const entry of model.history) if ((entry.kind === "text" || entry.kind === "thinking" || entry.kind === "tool") && entry.streaming) entry.streaming = false;
  for (const command of model.commands) if (command.history && command.state === "running") command.state = command.background ? "stopped" : "done";
  for (const change of model.changes) if (change.history && change.state === "running") change.state = "done";
  for (const task of model.tasks) if (task.history && task.state === "running") task.state = "done";
}

function system(model: ChatModel, inner: Inner, frame: { subtype: string; [key: string]: unknown }, live: boolean): void {
  switch (frame.subtype) {
    case "init": {
      const model_ = str(frame.model);
      if (model_) model.model = model_;
      const mode = str(frame.permissionMode);
      if (mode) model.mode = mode;
      const session = str(frame.session_id);
      if (session) model.session = session;
      return;
    }
    case "status": {
      model.status = str(frame.status);
      const mode = str(frame.permissionMode);
      if (mode) model.mode = mode;
      return;
    }
    case "session_state_changed": {
      const state = str(frame.state);
      if (state === "running" || state === "idle" || state === "requires_action") setState(model, state);
      return;
    }
    case "compact_boundary": {
      const meta = record(frame.compact_metadata);
      push(target(model, inner), { kind: "compact", key: nextKey(inner, "p"), trigger: str(meta?.trigger), pre: num(meta?.pre_tokens), post: num(meta?.post_tokens) } as CompactEntry);
      if (num(meta?.post_tokens) !== null) model.contextUsed = num(meta?.post_tokens);
      return;
    }
    case "api_retry":
      if (!live) return;
      model.retry = { attempt: num(frame.attempt), max: num(frame.max_retries), delay: num(frame.retry_delay_ms), status: num(frame.error_status), error: str(frame.error), at: Date.now() };
      return;
    case "task_started":
      return taskStarted(model, inner, frame);
    case "task_progress": {
      const task = taskOf(inner, frame);
      const usage = record(frame.usage);
      if (!task) return;
      if (num(usage?.total_tokens) !== null) task.tokens = num(usage?.total_tokens);
      if (num(usage?.tool_uses) !== null) task.tools = num(usage?.tool_uses);
      if (str(frame.last_tool_name)) task.lastTool = str(frame.last_tool_name);
      if (str(frame.summary)) task.summary = str(frame.summary);
      return;
    }
    case "task_updated": {
      const task = taskOf(inner, frame);
      const status = str(record(frame.patch)?.status);
      if (task && status) taskState(model, inner, task, status);
      return;
    }
    case "task_notification": {
      const task = taskOf(inner, frame);
      if (!task) return;
      const usage = record(frame.usage);
      if (num(usage?.total_tokens) !== null) task.tokens = num(usage?.total_tokens);
      if (str(frame.summary)) task.summary = str(frame.summary);
      taskState(model, inner, task, str(frame.status) ?? "completed");
      return;
    }
    case "background_tasks_changed": {
      const tasks = Array.isArray(frame.tasks) ? (frame.tasks as Record<string, unknown>[]) : [];
      const running = new Set<string>();
      for (const each of tasks) {
        const id = str(each.task_id);
        if (!id) continue;
        running.add(id);
        if (!inner.tasks.has(id)) taskStarted(model, inner, { ...each, subtype: "task_started", is_backgrounded: true });
        const task = inner.tasks.get(id);
        if (task) task.background = true;
      }
      // Replace: a background task no longer listed has ended.
      for (const task of model.tasks) {
        if (task.background && task.task && task.state === "running" && !running.has(task.task)) taskState(model, inner, task, "completed");
      }
      return;
    }
    case "informational": {
      const text = str(frame.content);
      if (!text?.trim()) return;
      const level = str(frame.level) === "warning" ? "warning" : "info";
      push(target(model, inner), { kind: "notice", key: nextKey(inner, "n"), level, text: text.replace(/\x1b\[[0-9;]*m/g, "") } as NoticeEntry);
      return;
    }
    case "local_command_output": {
      const text = str(frame.content);
      if (text?.trim()) push(target(model, inner), { kind: "notice", key: nextKey(inner, "n"), level: "output", text: text.replace(/\x1b\[[0-9;]*m/g, "") } as NoticeEntry);
      return;
    }
    case "permission_denied": {
      const tool = inner.tools.get(str(frame.tool_use_id) ?? "");
      if (tool) tool.denied = true;
      return;
    }
    default:
      return;
  }
}

function taskOf(inner: Inner, frame: Record<string, unknown>): TaskEntry | undefined {
  const id = str(frame.task_id);
  return (id ? inner.tasks.get(id) : undefined) ?? inner.taskTools.get(str(frame.tool_use_id) ?? "");
}

/** The subagent call's own row, not yet tied to a task, that a task told of with no call
 * (`background_tasks_changed`) is: the only running one of that description and agent. */
function untiedAgent(model: ChatModel, frame: Record<string, unknown>): TaskEntry | undefined {
  const description = str(frame.description);
  const agentType = str(frame.subagent_type);
  if (!description || !(str(frame.task_type) ?? "").includes("agent")) return undefined;
  const found = model.tasks.filter(
    (each) => each.kind === "agent" && each.tool && !each.task && each.state === "running" && each.description === description && (!agentType || !each.agentType || each.agentType === agentType),
  );
  return found.length === 1 ? found[0] : undefined;
}

/** `from`, made for a task before its call was known, folds into the call's own row `into`. */
function foldTask(model: ChatModel, from: TaskEntry, into: TaskEntry): void {
  into.task = from.task ?? into.task;
  into.background ||= from.background;
  into.agentType ??= from.agentType;
  into.tokens ??= from.tokens;
  into.tools ??= from.tools;
  into.lastTool ??= from.lastTool;
  into.summary ??= from.summary;
  if (from.state !== "running" && into.state === "running") {
    into.state = from.state;
    into.ended = from.ended;
  }
  const at = model.tasks.indexOf(from);
  if (at >= 0) model.tasks.splice(at, 1);
  if (model.ui.open.delete(from.key)) model.ui.open.add(into.key);
}

function taskStarted(model: ChatModel, inner: Inner, frame: Record<string, unknown>): void {
  const id = str(frame.task_id);
  const tool = str(frame.tool_use_id);
  const type = str(frame.task_type) ?? "";
  // A task that starts in the background is told of first in `background_tasks_changed`, with no
  // call (the SDK: "a task that starts in the background joins the level before its
  // task_started"): it is tied to its subagent call's row then, or once its task_started names the
  // call, so a call never has two rows (the user's word, 16 Nov: the same subagent twice).
  const byId = id ? inner.tasks.get(id) : undefined;
  const byTool = tool ? inner.taskTools.get(tool) : undefined;
  if (byId && byTool && byId !== byTool) foldTask(model, byId, byTool);
  let task = byTool ?? byId ?? (!tool ? untiedAgent(model, frame) : undefined);
  const command = (tool ? inner.commands.get(tool) : undefined) ?? (id ? inner.bashTasks.get(id) : undefined);
  // Every Bash call is a task while it runs: one in the foreground is its Komutlar row alone.
  if (command && frame.is_backgrounded !== true && !command.background) {
    if (id) inner.bashTasks.set(id, command);
    return;
  }
  // A Bash call sent to the background is its command's row, and a task of its own.
  if (command && id) command.background = id;
  if (!task) {
    task = push(model.tasks, {
      key: nextKey(inner, "g"),
      task: id,
      tool,
      kind: type.includes("agent") ? "agent" : type.includes("bash") || type.includes("shell") ? "shell" : "other",
      description: str(frame.description) ?? command?.command ?? "",
      agentType: str(frame.subagent_type),
      state: "running",
      started: inner.inHistory ? null : ((tool ? inner.tools.get(tool)?.started : null) ?? Date.now()),
      ended: null,
      tokens: null,
      tools: null,
      lastTool: null,
      summary: null,
      background: frame.is_backgrounded === true,
      history: inner.inHistory,
    } as TaskEntry);
  } else {
    task.task = id ?? task.task;
    task.tool ??= tool;
    task.agentType ??= str(frame.subagent_type);
    if (frame.is_backgrounded === true) task.background = true;
  }
  if (id) inner.tasks.set(id, task);
  if (tool) inner.taskTools.set(tool, task);
}

function taskState(model: ChatModel, inner: Inner, task: TaskEntry, status: string): void {
  const state: TaskEntry["state"] | null =
    status === "completed" ? "done" : status === "failed" ? "failed" : status === "stopped" || status === "killed" ? "stopped" : status === "running" || status === "pending" ? "running" : null;
  if (!state) return;
  task.state = state;
  if (state !== "running") task.ended = task.ended ?? (inner.inHistory ? null : Date.now());
  // A background Bash run's row ends with its task.
  if (task.task && state !== "running") {
    for (const command of model.commands) {
      if (command.background === task.task && command.state === "running") {
        command.state = state === "done" ? "done" : state;
        command.ended = task.ended;
      }
    }
  }
}

function shell(model: ChatModel, inner: Inner, frame: Record<string, unknown>): void {
  const run = str(frame.run);
  if (!run) return;
  let command = inner.commands.get(`run:${run}`);
  if (!command) {
    command = push(model.commands, {
      key: nextKey(inner, "c"),
      tool: null,
      run,
      command: str(frame.line) ?? "",
      description: null,
      state: "running",
      stdout: "",
      stderr: "",
      code: null,
      interrupted: false,
      background: null,
      started: Date.now(),
      ended: null,
      history: inner.inHistory,
    } as CommandEntry);
    inner.commands.set(`run:${run}`, command);
    push(target(model, inner), { kind: "shell", key: nextKey(inner, "s"), run } as ShellEntry);
  }
  if (str(frame.line) && !command.command) command.command = str(frame.line)!;
  const text = str(frame.text);
  if (text) command.stdout = (command.stdout + text).slice(-SHELL_KEEP);
  if (frame.done === true) {
    const code = num(frame.code);
    command.code = code;
    command.ended = Date.now();
    command.state = code === 0 ? "done" : code === null ? "stopped" : "failed";
  }
}

/** How much of a `!` run's output the page keeps (the core keeps its own bound too). */
const SHELL_KEEP = 512 * 1024;

/** The `!` run of a conversation row. */
export function shellRun(model: ChatModel, run: string): CommandEntry | undefined {
  return model.commands.find((command) => command.run === run);
}

/** The uuid of the conversation's last entry before user message `uuid`, for a fork that keeps
 * what came before it; undefined when the message isn't known, null when nothing came before. */
export function chainBefore(id: number, uuid: string): string | null | undefined {
  return inners.get(id)?.before.get(uuid);
}

/** The uuid of the conversation's last entry now, for a fork that keeps it whole. */
export function chainNow(id: number): string | null {
  return inners.get(id)?.chain ?? null;
}

// ---------------------------------------------------------------- settings

/** The chat's preferences (Settings › Claude › Sohbet), the defaults where unset. */
export const chatSettings = computed<ChatSettings>(() => ({ ...CHAT_DEFAULTS, ...(snapshot.value?.settings.chat ?? {}) }));

// ---------------------------------------------------------------- the composers, the Files window

/** Each chat's composer, while its view is up (ChatView registers it). */
const composers = new Map<number, ComposerApi>();
/** Mentions the Files window sent for a chat whose composer isn't up yet. */
const attaches = new Map<number, ChatAttach[]>();
/** A draft for a chat just opened (a fork's message), put into its composer when it comes up. */
const drafts = new Map<number, string>();
/** The project's chat used last (sent to, or picked), by project. */
const lastUsed = new Map<string, number>();
/** The file the Files window showed last (its path from the project's folder), whichever project
 * it is of: one context is in force at a time, the last one sent. */
const context = ref<ChatContext>(null);

export function registerComposer(id: number, composer: ComposerApi | null): void {
  if (!composer) {
    composers.delete(id);
    return;
  }
  composers.set(id, composer);
  const draft = drafts.get(id);
  if (draft !== undefined) {
    drafts.delete(id);
    composer.setText(draft);
  }
  for (const each of attaches.get(id) ?? []) composer.attach(absolute(each.project, each.path), each.startLine, each.endLine);
  attaches.delete(id);
}

export function composerOf(id: number): ComposerApi | null {
  return composers.get(id) ?? null;
}

/** Puts `text` into the composer of chat `id`, now or when it comes up. */
export function draftFor(id: number, text: string): void {
  const composer = composers.get(id);
  if (composer) composer.setText(text);
  else drafts.set(id, text);
}

export function markUsed(path: string, id: number): void {
  lastUsed.set(path, id);
}

/** The context the composer of a chat of `path` sends along, as the settings let it: the Files
 * window's file when it is of this project (its path as the window sent it, from the project's
 * folder), its lines only while `attachSelection` is on. */
export function contextFor(path: string): ChatContext {
  const settings = chatSettings.value;
  const now = context.value;
  if (!now || now.project !== path || !settings.attachFile) return null;
  if (settings.attachSelection && now.startLine !== undefined) return { project: now.project, path: now.path, startLine: now.startLine, endLine: now.endLine };
  return { project: now.project, path: now.path };
}

/** A path the Files window sent (from the project's folder) made whole, for the composer's mention. */
function absolute(project: string, path: string): string {
  if (path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\")) return path;
  const sep = project.includes("\\") && !project.includes("/") ? "\\" : "/";
  const base = project.replace(/[\\/]+$/, "");
  const rest = path.replace(/^\.?[\\/]+/, "").replace(/[\\/]/g, sep);
  return rest ? base + sep + rest : base;
}

/** The project's chats, in the snapshot's order. */
function chatsOf(path: string): TerminalView[] {
  const project = snapshot.value?.projects.find((each) => each.path === path);
  return (project?.terminals ?? []).filter((view) => view.kind === "chat");
}

async function onAttach(attach: ChatAttach): Promise<void> {
  if (!attach?.project || !attach.path) return;
  const chats = chatsOf(attach.project);
  let id = lastUsed.get(attach.project);
  if (id === undefined || !chats.some((view) => view.id === id)) id = chats[chats.length - 1]?.id;
  if (id === undefined) {
    const opened = await startChat(attach.project);
    if (opened?.kind !== "tab") return;
    id = opened.view.id;
  }
  lastUsed.set(attach.project, id);
  const composer = composers.get(id);
  if (composer) composer.attach(absolute(attach.project, attach.path), attach.startLine, attach.endLine);
  else attaches.set(id, [...(attaches.get(id) ?? []), attach]);
  revealTab(attach.project, id);
}

function onContext(sent: ChatContext): void {
  // One context at a time, the last one sent; none (it names no project) clears that one.
  if (!sent) {
    context.value = null;
    return;
  }
  if (!sent.project || !sent.path) return;
  context.value = { project: sent.project, path: sent.path, startLine: sent.startLine, endLine: sent.endLine };
}

// ---------------------------------------------------------------- listening

/** This page's window. Chats are drawn in the main window's page, and in the terminals' own
 * window's, where the panel stands when it is out of Pitwall's window (lib/panelStand). */
function pageLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "";
  }
}

// While the panel stands in its own window, the main window's page leaves a chat's frames and the
// Files window's "Sohbete ekle" to that window's page (one chat started for it, not two), which
// comes up for it (a start's question may wait there); the main window's chats read on from where
// they stopped once the panel is back.
const page = typeof window !== "undefined" ? pageLabel() : "";
if (page === "main" || page === "terminals") {
  void listen<ChatEvent>("chat", (event) => {
    if (!panelAway()) hear(event.payload);
  });
  void listen<ChatExit>("chat-exit", (event) => ended(event.payload));
  void listen<ChatAttach>(CHAT_ATTACH, (event) => {
    if (panelAway()) return;
    if (page === "terminals") void api.panelAsk(JSON.stringify({ kind: "show" }), true).catch(() => undefined);
    void onAttach(event.payload);
  });
  watch(panelAway, (away, was) => {
    if (was && !away) for (const [id, model] of models) if (model.loaded) void loadChat(id, true);
  });
  void listen<ChatContext>(CHAT_CONTEXT, (event) => onContext(event.payload));
  // A chat whose tab closed is forgotten with it; one that came back from elsewhere (a window
  // that didn't see its frames) reads them again once its view is up.
  const seen = new Set<number>();
  watch(
    () => snapshot.value?.projects,
    (projects) => {
      if (!projects) return;
      const open = new Set<number>();
      for (const project of projects) for (const view of project.terminals ?? []) if (view.kind === "chat") open.add(view.id);
      for (const id of seen) {
        if (!open.has(id)) {
          seen.delete(id);
          forgetChat(id);
        }
      }
      for (const id of open) seen.add(id);
    },
  );
}
