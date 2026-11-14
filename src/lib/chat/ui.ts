/**
 * Seams between the chat's window parts, each kept apart:
 * the conversation (`ChatView` and `lib/chat/store.ts`), the composer and the cards
 * (`Composer.vue`, `ChatCard.vue`), the dialogs (`dialogs/ChatDialogHost.vue`) and the surfaces
 * around them (Settings, Sessions, the Files window). Each side keeps to these shapes.
 */
import type { InjectionKey, Ref } from "vue";

import type { ChatInfo, ChatPending, ChatSettings, ChatState } from "./wire";

/** The dialogs `ChatDialogHost` draws. */
export type ChatDialogKind =
  | "model" // model, effort, thinking, fast (B5)
  | "mode" // permission mode menu (B5)
  | "context" // context usage, cache clock (B6)
  | "usage" // usage, limits, session cost (B6)
  | "status" // get_status sections (B9)
  | "mcp" // MCP servers, reconnect, toggle, sign-in (B8)
  | "permissions" // permission rules (B9)
  | "hooks" // hooks listing (B9)
  | "memory" // memory files (B9)
  | "skills" // skills (B9)
  | "styles" // output styles (B9)
  | "agents" // the agents the CLI listed (initialize.agents)
  | "export" // export the conversation (save dialog through the core)
  | "help" // commands and keys
  | "rewind" // rewind picker and its confirmation (B10)
  | "btw" // side question panel (B11)
  | "resume" // the project's earlier sessions, opened in this tab or a new chat
  | "plan"; // the session's current plan (get_plan)

export type ChatDialogOpen = { kind: ChatDialogKind; /** What followed the command, if any. */ arg?: string };

/** A user message of the conversation, for rewind and fork. */
export type ChatMessageRef = { uuid: string; text: string; time: number | null; /** Files the turn after it changed. */ changed: number };

/** Props of `ChatDialogHost.vue`. */
export type ChatDialogProps = {
  id: number;
  path: string;
  session: string | null;
  info: ChatInfo | null;
  state: ChatState;
  open: ChatDialogOpen | null;
  messages: ChatMessageRef[];
};
/** What `ChatDialogHost.vue` emits (Vue emits by these names). */
export type ChatDialogEmits = {
  close: [];
  /** Put this text into the composer (help picking a command, resume's prompt…). */
  insert: [text: string];
  /** Open a new chat on this session forked at `at` (a user message uuid), or the whole session. */
  fork: [at: string | null];
  /** Open this earlier session in a chat (resume dialog). */
  resume: [session: string];
};

/** A message waiting in the queue (sent while a turn ran), drawn over the composer. */
export type QueuedMessage = { uuid: string; text: string; priority: "next" | "now" | "later" };

/** A terminal tab of the same project, offered as context in the composer's attach menu. */
export type TerminalRef = { id: number; name: string };

/** Props of `Composer.vue`. */
export type ComposerProps = {
  id: number;
  path: string;
  info: ChatInfo | null;
  state: ChatState;
  settings: ChatSettings;
  /** The prompt suggestion after the last turn (ghost text), or null. */
  suggestion: string | null;
  queued: QueuedMessage[];
  /** The Files window's open file of this project (and its selection), when the setting lets it go along. */
  context: ChatContext;
  terminals: TerminalRef[];
  /** Current values the chips show (latest `system/init`, `status` frames, controls the user sent). */
  model: string | null;
  mode: string | null;
  effort: string | null;
  /** Context used, 0..1, or null when unknown. */
  contextShare: number | null;
  disabled: boolean;
};
/**
 * What `Composer.vue` emits. `send` carries the finished message (the uuid is the core's);
 * `shell` a `!` line; `dialog` a built-in the window draws itself; `terminal` a command only the
 * terminal has ("Terminalde sürdür"); `settings` opens Settings › Claude › Sohbet.
 */
export type ComposerEmits = {
  send: [message: import("./wire").ChatOutgoing];
  shell: [line: string];
  interrupt: [];
  dialog: [open: ChatDialogOpen];
  /** A control, and the message to send once it is through (`/plan <task>`), if any. */
  control: [control: import("./wire").ChatControl, then?: import("./wire").ChatOutgoing];
  cancelQueued: [uuid: string];
  terminal: [command: string];
  settings: [];
};
/** What `Composer.vue` exposes (`defineExpose`). */
export type ComposerApi = {
  focus: () => void;
  insert: (text: string) => void;
  /** Add a mention chip for a file (absolute path) and its line range. */
  attach: (path: string, startLine?: number, endLine?: number) => void;
  /** Replace the draft (edit and resend, prompt history). */
  setText: (text: string) => void;
};

/** Props of `ChatCard.vue`: one pending CLI request, drawn as its card. */
export type ChatCardProps = {
  id: number;
  path: string;
  pending: ChatPending;
  /** How many more cards wait after this one. */
  more: number;
  /** A thin form for the Komutlar row of a Bash call (buttons only). */
  compact: boolean;
};
export type ChatCardEmits = {
  answer: [answer: import("./wire").ChatAnswer];
  /** A request the chat can't draw: the session goes on in Claude Code's terminal ("Terminalde sürdür"). */
  terminal: [];
};

/**
 * Files window → main window events (Tauri `emitTo("main", …)`).
 * `chat-attach`: "Sohbete ekle" on a file or a selection: the main window adds it as a mention to
 * the chat of that project that was used last (or opens one when the project has none).
 * `chat-context`: the file the Files window shows now for a project (null when it shows none),
 * which goes along with a message while Settings' "attachFile" is on.
 */
export const CHAT_ATTACH = "chat-attach";
export type ChatAttach = { project: string; path: string; startLine?: number; endLine?: number };
export const CHAT_CONTEXT = "chat-context";
export type ChatContext = { project: string; path: string; startLine?: number; endLine?: number } | null;

/** Each permission mode's icon, on the composer's chip and beside its name in the mode's menu
 * (the user's word, 14 Nov: a narrow composer shows the chips by their icons alone). */
export const MODE_ICONS = { default: "shield", acceptEdits: "pencil", plan: "plan", auto: "zap", dontAsk: "ban" } as const;

/** What a tool's row in the conversation needs to draw what it did when it opens (a command's
 * output and buttons, a subagent's state): the chat's id and the clock that ticks while something
 * runs. Given by `ChatView`, so the turns needn't pass it down and re-draw each second. */
export type ChatRowContext = { id: Ref<number>; now: Ref<number> };
export const CHAT_ROW: InjectionKey<ChatRowContext> = Symbol("chat-row");

/** The modes Claude's plan can start in once approved (PlanStart), in the menu's order. */
export const PLAN_MODES = ["acceptEdits", "auto", "default"] as const;
export type PlanMode = (typeof PLAN_MODES)[number];

/** A plan's row in the conversation brings its plan up on the plan's screen (ChatView, PL5): the
 * waiting one as it asks, an answered one to read. `from` is the row, the screen grows out of it. */
export type ChatPlanContext = { open: (tool: string, plan: string, from: HTMLElement) => void };
export const CHAT_PLAN: InjectionKey<ChatPlanContext> = Symbol("chat-plan");

/** What the user wrote on a waiting plan (its request's id), kept while its screen is folded or the
 * chat's tab is left, so that nothing written is lost before it is answered. */
export type PlanNotes = { request: string; comments: { line: number; excerpt: string; text: string }[]; feedback: string };
