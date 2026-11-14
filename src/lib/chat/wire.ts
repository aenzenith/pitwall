/**
 * The chat's wire as the window sees it: what the core sends ("chat" events, `chat_buffer`) and
 * what the window asks of it (`chat_*` commands). The CLI's own frames pass through the core by
 * kind (docs/SOHBET.md); every field here may be missing, and unknown
 * kinds are skipped.
 */

/** Where a chat's process stands: the CLI's `session_state_changed`, plus the core's own ends. */
export type ChatState = "starting" | "idle" | "running" | "requires_action" | "exited";

export type ChatExitReason = "closed" | "error" | "notFound" | "startFailed" | "elsewhere";

export type ChatPermissionMode = "default" | "acceptEdits" | "plan" | "auto" | "dontAsk";
export type ChatEffort = "low" | "medium" | "high" | "xhigh" | "max" | "auto";

/** A slash command, skill, plugin command or MCP prompt the CLI can run (`initialize.commands`). */
export type ChatCommand = { name: string; description?: string; argumentHint?: string; aliases?: string[]; builtin?: boolean };
export type ChatAgentInfo = { name: string; description?: string; model?: string };
export type ChatModelInfo = {
  value: string;
  resolvedModel?: string;
  displayName?: string;
  description?: string;
  supportsEffort?: boolean;
  supportedEffortLevels?: ChatEffort[];
  supportsAdaptiveThinking?: boolean;
  supportsFastMode?: boolean;
  supportsAutoMode?: boolean;
  supportsThinkingOff?: boolean;
};

/** What the core learned of the process (its initialize reply, then each turn's `system/init`). */
export type ChatInfo = {
  version?: string;
  commands: ChatCommand[];
  agents: ChatAgentInfo[];
  models: ChatModelInfo[];
  outputStyle?: string;
  outputStyles: string[];
  permissionMode?: ChatPermissionMode | string;
  model?: string;
  fastModeState?: string;
  fastModeDisabledReason?: string;
  subscriptionType?: string;
  apiProvider?: string;
  /** The initialize reply's and the latest init's capabilities, together. */
  capabilities: string[];
  /** From the latest `system/init`: */
  skills?: string[];
  tools?: string[];
  mcpServers?: { name: string; status: string; source?: string }[];
  plugins?: { name: string; path?: string; version?: string }[];
  terminalCommands?: string[];
  cwd?: string;
  sessionId?: string;
};

// ---------------------------------------------------------------- frames

export type TextBlock = { type: "text"; text: string };
export type ThinkingBlock = { type: "thinking"; thinking: string; signature?: string };
export type ToolUseBlock = { type: "tool_use" | "server_tool_use"; id: string; name: string; input: Record<string, unknown> };
export type ToolResultBlock = {
  type: "tool_result";
  tool_use_id: string;
  content?: string | ({ type: "text"; text: string } | ImageBlock)[];
  is_error?: boolean;
};
export type ImageBlock = { type: "image"; source: { type: "base64"; media_type: string; data: string | null }; omitted?: boolean };
export type DocumentBlock = { type: "document"; source: { type: "base64"; media_type: string; data: string | null }; title?: string };
export type ContentBlock = TextBlock | ThinkingBlock | ToolUseBlock | ToolResultBlock | ImageBlock | DocumentBlock | { type: string; [key: string]: unknown };

type Base = { uuid?: string; session_id?: string; parent_tool_use_id?: string | null; timestamp?: string };

export type AssistantFrame = Base & {
  type: "assistant";
  message: { id: string; model?: string; content: ContentBlock[]; usage?: Usage; stop_reason?: string | null };
  error?: string;
  aborted?: boolean;
  supersedes?: string[];
  agent_id?: string;
  user_message_uuid?: string;
};
export type UserFrame = Base & {
  type: "user";
  message: { role: "user"; content: string | ContentBlock[] };
  isReplay?: boolean;
  isSynthetic?: boolean;
  tool_use_result?: unknown;
  origin?: { kind: string };
};
export type StreamEventFrame = Base & {
  type: "stream_event";
  event: { type: string; index?: number; delta?: { type: string; text?: string; thinking?: string; partial_json?: string }; content_block?: ContentBlock };
};
export type Usage = { input_tokens?: number; output_tokens?: number; cache_read_input_tokens?: number; cache_creation_input_tokens?: number };
export type ResultFrame = Base & {
  type: "result";
  subtype: string;
  is_error?: boolean;
  result?: string;
  errors?: string[];
  duration_ms?: number;
  num_turns?: number;
  terminal_reason?: string;
  total_cost_usd?: number;
  usage?: Usage;
  modelUsage?: Record<string, { inputTokens?: number; outputTokens?: number; cacheReadInputTokens?: number; cacheCreationInputTokens?: number; contextWindow?: number; costUSD?: number }>;
  permission_denials?: { tool_name: string; tool_use_id: string }[];
  queued_turn_count?: number;
};
export type SystemFrame = Base & { type: "system"; subtype: string; [key: string]: unknown };
export type PromptSuggestionFrame = Base & { type: "prompt_suggestion"; suggestion: string };
export type RateLimitFrame = Base & {
  type: "rate_limit_event";
  rate_limit_info: { status: string; resetsAt?: number; rateLimitType?: string; utilization?: number };
};
export type ConversationResetFrame = Base & { type: "conversation_reset"; new_conversation_id?: string; trigger?: string };
export type ToolProgressFrame = Base & { type: "tool_progress"; tool_use_id: string; tool_name: string; elapsed_time_seconds?: number; task_id?: string };

/** A request of the CLI's the user answers (a card). */
export type CanUseToolRequest = {
  subtype: "can_use_tool";
  tool_name: string;
  input: Record<string, unknown>;
  tool_use_id?: string;
  permission_suggestions?: PermissionUpdate[];
  blocked_path?: string;
  decision_reason?: string;
  decision_reason_type?: string;
  title?: string;
  display_name?: string;
  description?: string;
  default_to_no?: boolean;
  suppress_always_allow_rule?: boolean;
  requires_user_interaction?: boolean;
  agent_id?: string;
  mcp_server?: { name: string; source?: string };
};
export type ElicitationRequest = {
  subtype: "elicitation";
  mcp_server_name: string;
  message: string;
  mode?: "form" | "url";
  url?: string;
  requested_schema?: { type?: string; properties?: Record<string, { type?: string; title?: string; description?: string; enum?: string[]; default?: unknown }>; required?: string[] };
  title?: string;
};
export type UserDialogRequest = { subtype: "request_user_dialog"; dialog_kind: string; payload: unknown; tool_use_id?: string };
export type CliRequest = CanUseToolRequest | ElicitationRequest | UserDialogRequest;
export type ControlRequestFrame = { type: "control_request"; request_id: string; request: CliRequest };
export type ControlCancelFrame = { type: "control_cancel_request"; request_id: string };

export type PermissionUpdate =
  | { type: "addRules" | "replaceRules" | "removeRules"; rules: { toolName: string; ruleContent?: string }[]; behavior: "allow" | "deny" | "ask"; destination: PermissionDestination }
  | { type: "setMode"; mode: string; destination: PermissionDestination }
  | { type: "addDirectories" | "removeDirectories"; directories: string[]; destination: PermissionDestination };
export type PermissionDestination = "userSettings" | "projectSettings" | "localSettings" | "session" | "cliArg";

/** Frames the core makes itself. */
export type PitwallFrame =
  | { type: "pitwall"; kind: "info"; info: ChatInfo }
  | { type: "pitwall"; kind: "history"; frames: ChatFrame[] }
  | { type: "pitwall"; kind: "state"; state: ChatState }
  | { type: "pitwall"; kind: "shell"; run: string; line?: string; text?: string; done?: boolean; code?: number | null }
  | { type: "pitwall"; kind: "sent"; uuid: string; message: ChatOutgoing }
  | { type: "pitwall"; kind: "answered"; requestId: string }
  | { type: "pitwall"; kind: "unsent"; uuids: string[]; text: string }
  | { type: "pitwall"; kind: "error"; reason: string; text?: string };

export type ChatFrame =
  | AssistantFrame
  | UserFrame
  | StreamEventFrame
  | ResultFrame
  | SystemFrame
  | PromptSuggestionFrame
  | RateLimitFrame
  | ConversationResetFrame
  | ToolProgressFrame
  | ControlRequestFrame
  | ControlCancelFrame
  | PitwallFrame
  | (Base & { type: string; [key: string]: unknown });

export type ChatEvent = { id: number; seq: number; frames: ChatFrame[] };
export type ChatExit = { id: number; code: number | null; reason: ChatExitReason; stderr: string | null };
export type ChatPending = { requestId: string; request: CliRequest };
export type ChatBuffer = { seq: number; frames: ChatFrame[]; state: ChatState; info: ChatInfo | null; pending: ChatPending[] };

// ---------------------------------------------------------------- what the window sends

export type ChatOutgoing = {
  text: string;
  images: { mediaType: string; data: string }[];
  documents: { mediaType: "application/pdf"; data: string; name: string }[];
  priority: "next" | "now" | "later";
  pasted: boolean;
};

export type ChatAnswer =
  | {
      kind: "allow";
      /** The fields the user edited in the card (see CONTRACT.md for which a tool takes). */
      input?: Record<string, unknown>;
      /** "Always": which of the request's suggestions, to where. */
      always?: { suggestions: number[]; destination: PermissionDestination };
      /** A mode to switch to after the allow. */
      mode?: ChatPermissionMode;
    }
  | { kind: "deny"; message: string; interrupt: boolean }
  | { kind: "elicitation"; action: "accept" | "decline" | "cancel"; content?: Record<string, unknown> }
  | { kind: "dialog"; response: unknown };

export type ChatControl =
  | { kind: "setMode"; mode: ChatPermissionMode }
  | { kind: "setModel"; model: string }
  | { kind: "setThinking"; tokens: number | null }
  | { kind: "setEffort"; level: ChatEffort }
  | { kind: "setFast"; on: boolean }
  /** apply_flag_settings { ultracode } (Claude Code 2.1.284 on). */
  | { kind: "setUltracode"; on: boolean }
  | { kind: "setAgent"; agent: string }
  | { kind: "contextUsage" }
  | { kind: "usage" }
  | { kind: "settings" }
  | { kind: "listModels" }
  | { kind: "permissionRules" }
  | { kind: "hooks" }
  | { kind: "mcpStatus" }
  | { kind: "mcpReconnect"; server: string }
  | { kind: "mcpToggle"; server: string; enabled: boolean }
  | { kind: "mcpAuthenticate"; server: string }
  | { kind: "rewindFiles"; message: string; dryRun: boolean }
  | { kind: "stopTask"; task: string }
  | { kind: "backgroundTasks"; toolUse: string | null }
  | { kind: "taskOutput"; task: string }
  | { kind: "rename"; title: string }
  | { kind: "cancelQueued"; message: string }
  | { kind: "fileSuggestions"; query: string }
  | { kind: "reloadPlugins" }
  | { kind: "reloadSkills" }
  | { kind: "reloadStyles" }
  | { kind: "sideQuestion"; text: string }
  | { kind: "workspaceDiff" }
  | { kind: "suggestionsPaused"; paused: boolean }
  // Dialog readers seen answering on 2.1.325; each may be missing in a later CLI.
  | { kind: "status" } // get_status → { sections }
  | { kind: "memory" } // get_memory_dialog
  | { kind: "skills" } // get_skills_dialog
  | { kind: "export" } // export_conversation → { text, default_filename }
  | { kind: "plan" } // get_plan → { exists, … }
  | { kind: "sessionCost" } // get_session_cost → { text }
  /** apply_flag_settings { outputStyle }: only a style in `info.outputStyles`. */
  | { kind: "setStyle"; style: string };

export type ChatInterruptReceipt = { stillQueued: string[]; cancelled: string[] };

export type ChatSettings = {
  open: "terminal" | "chat";
  sendKey: "enter" | "mod-enter";
  /** What a chat starts in where nothing else names a mode (Settings › Claude › Sohbet). */
  mode: "default" | "auto";
  suggestions: boolean;
  scrollOnSend: boolean;
  timestamps: boolean;
  continueInterrupted: boolean;
  focusView: boolean;
  attachFile: boolean;
  attachSelection: boolean;
};
export const CHAT_DEFAULTS: ChatSettings = {
  open: "terminal",
  sendKey: "enter",
  mode: "auto",
  suggestions: true,
  scrollOnSend: true,
  timestamps: false,
  continueInterrupted: false,
  focusView: false,
  attachFile: true,
  attachSelection: true,
};

export type SessionsMeta = { archived: string[]; groups: { id: string; name: string; sessions: string[] }[]; unread: string[] };
export type SessionsMetaPatch =
  | { archive: string[]; on: boolean }
  | { unread: string[]; on: boolean }
  | { group: { id?: string; name: string } }
  | { ungroup: string }
  | { move: string[]; to: string | null };

/** A chat tab's own part of its `TerminalView`. */
export type ChatView = { session: string | null; state: ChatState; title: string | null };
