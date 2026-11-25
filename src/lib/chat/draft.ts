/**
 * The empty terminal panel's chat box (window/TerminalEmpty.vue): the chat's own composer
 * (Composer.vue) before there is a chat, every one of its ways as in a chat (the user's word,
 * 25 Nov: "sohbet modu sohbet ile birebir özellikte başlayacak").
 *
 * A box is a draft chat with a negative id of its own (`draftId`, one per box), so what is typed
 * in it is the composer's draft (lib/chat/composer) and a voice take is its own. Its chips' menus
 * (dialogs/ModelDialog, ModeDialog) ask and set the box here instead of a chat's Claude Code
 * (dialogs/shared: `draftAsk`, `draftTake`): what they pick is how the chat it opens starts,
 * `open_chat`'s start, of `start.rs`'s lists alone. What a box can't do before there is a chat (a
 * dialog of the chat's, a `!` line, a command Claude Code answers) opens the chat and is done
 * there once it is ready (`handOff`, taken by ChatView). Everything here is the page's memory.
 */
import { t } from "../i18n";
import { MODEL_LABELS, MODELS } from "../start";
import type { ClaudeModel } from "../types";
import type { Route } from "./commands";
import type { ChatControl, ChatEffort, ChatInfo, ChatModelInfo, ChatPermissionMode } from "./wire";

/** How a box's chat starts, as its chips show it: the model (none: Claude Code's own), the mode,
 * the effort (none: Claude Code's own). */
export type DraftStart = { model: ClaudeModel | null; mode: ChatPermissionMode; effort: ChatEffort | null };

/** The modes a chat starts in (`ChatMode::at_start`): never `dontAsk`, its menu's alone. */
export const START_MODES: ChatPermissionMode[] = ["default", "acceptEdits", "plan", "auto"];

/* ---------- a box's id ---------- */

const ids = new Map<string, number>();
let lastId = 0;

/** The draft chat of the box keyed `key` (a project's path, an empty pane's with its key). */
export function draftId(key: string): number {
  let id = ids.get(key);
  if (id === undefined) {
    id = --lastId;
    ids.set(key, id);
  }
  return id;
}

export function isDraft(id: number): boolean {
  return id < 0;
}

/* ---------- its start, for the chips' menus ---------- */

/** A box as its menus reach it: its start as shown, and a pick of theirs. */
export type DraftBox = { start: () => DraftStart; models: () => ChatModelInfo[]; set: (next: Partial<DraftStart>) => void };
const boxes = new Map<number, DraftBox>();

export function registerDraft(id: number, box: DraftBox | null): void {
  if (box) boxes.set(id, box);
  else boxes.delete(id);
}

/** Of a model's value, the alias a start takes: null for Claude Code's own (`default`),
 * undefined for one no start takes. */
export function startModel(value: string): ClaudeModel | null | undefined {
  if (value === "default") return null;
  return (MODELS as string[]).includes(value) ? (value as ClaudeModel) : undefined;
}

/** A chip menu's question, answered from the box: the models it offers, its model and effort. */
export function draftAsk(id: number, control: ChatControl): unknown {
  const box = boxes.get(id);
  if (!box) return null;
  if (control.kind === "listModels") return { models: box.models() };
  if (control.kind === "settings") {
    const start = box.start();
    return { applied: { model: start.model ?? "default", effort: start.effort } };
  }
  return null;
}

/** A chip menu's pick, kept by the box; false for one a start can't take. */
export function draftTake(id: number, control: ChatControl): boolean {
  const box = boxes.get(id);
  if (!box) return false;
  switch (control.kind) {
    case "setModel": {
      const model = startModel(control.model);
      if (model === undefined) return false;
      box.set({ model });
      return true;
    }
    case "setMode":
      if (!START_MODES.includes(control.mode)) return false;
      box.set({ mode: control.mode });
      return true;
    case "setEffort":
      box.set({ effort: control.level === "auto" ? null : control.level });
      return true;
    default:
      return false;
  }
}

/** Whether a box does `route` itself (its chips' menus and what they set, Settings, a refusal);
 * anything else opens the chat and is done there. */
export function draftTakes(route: Exclude<Route, { kind: "send" }>): boolean {
  switch (route.kind) {
    case "dialog":
      return route.open.kind === "model" || route.open.kind === "mode";
    case "control": {
      const control = route.control;
      if (control.kind === "setModel") return startModel(control.model) !== undefined;
      if (control.kind === "setMode") return START_MODES.includes(control.mode);
      return control.kind === "setEffort";
    }
    case "settings":
    case "refused":
      return true;
    default:
      return false;
  }
}

/* ---------- what the CLI said, for a box ---------- */

/** The latest of each project's chats, and the latest of any: what a box offers before its chat is
 * there (the project's commands, skills and agents; the account's models). */
const infos = new Map<string, ChatInfo>();
let anyInfo: ChatInfo | null = null;

export function noteInfo(path: string, info: ChatInfo): void {
  infos.set(path, info);
  anyInfo = info;
}

const NO_INFO: ChatInfo = { commands: [], agents: [], models: [], outputStyles: [], capabilities: [] };

/** The models a box offers: the CLI's of those a start takes, else the start's own list. */
export function draftModels(path: string): ChatModelInfo[] {
  const told = (infos.get(path) ?? anyInfo)?.models ?? [];
  const taken = told.filter((row) => startModel(row.value) !== undefined);
  if (taken.length) return taken;
  return [{ value: "default", displayName: t("start.model.auto") }, ...MODELS.map((model) => ({ value: model, displayName: t(MODEL_LABELS[model]) }))];
}

/** What a box's composer is told of its chat to be: the project's last chat's commands, skills and
 * agents, the models it offers, and its start's model and mode. */
export function draftInfo(path: string, start: DraftStart): ChatInfo {
  const own = infos.get(path) ?? NO_INFO;
  return { ...own, models: draftModels(path), model: start.model ?? undefined, permissionMode: start.mode };
}

/* ---------- what goes on in the chat ---------- */

const handed = new Map<number, string>();

/** A line the box couldn't run, for the chat `chat` it opened to run once ready. */
export function handOff(chat: number, line: string): void {
  handed.set(chat, line);
}

export function takeHandOff(chat: number): string | null {
  const line = handed.get(chat) ?? null;
  handed.delete(chat);
  return line;
}
