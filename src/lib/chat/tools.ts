/**
 * How the conversation names a tool call on its one-line row (TS3): an icon, a verb in the past
 * tense ("Okudu", "Çalıştırdı"), what it was called on (a path from the project's folder, a
 * command, a pattern) and a few words at its right end (an exit code, lines added).
 */
import { language, t, type Key } from "../i18n";
import type { ChangeEntry, CommandEntry, Hunk, TaskEntry, ToolEntry } from "./store";
import type { DiffLine } from "../unifiedDiff";

export type ToolIcon = "file" | "terminal" | "pencil" | "file-plus" | "search" | "globe" | "plug" | "spark" | "agent" | "list" | "question" | "plan" | "tool";

export type ToolLook = {
  icon: ToolIcon;
  verb: string;
  arg: string;
  mono: boolean;
  meta: string | null;
  tone: "add" | "error" | "warn" | "ok" | null;
  /** A lookup, folded with its neighbours into one row when there are several. */
  quiet: boolean;
  /** What a pointer resting on the row's words reads, when they stand for something longer. */
  hint: string | null;
};

/** Calls that only read: several in a row fold into one ("5 araç çağrısı"). */
export const QUIET_TOOLS = new Set(["Read", "Grep", "Glob", "LS", "WebFetch", "WebSearch", "ToolSearch", "NotebookRead", "TaskGet", "TaskList", "ListMcpResourcesTool", "ReadMcpResourceTool"]);

/** `file` from the project's folder when it is inside it; else as it is. */
export function relPath(project: string, file: string): string {
  const root = project.replace(/[\\/]+$/, "");
  if (file === root) return ".";
  for (const sep of ["/", "\\"]) if (file.startsWith(root + sep)) return file.slice(root.length + 1).replace(/\\/g, "/");
  return file;
}

function s(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** The first line of a command, for a row. */
export function firstLine(text: string): string {
  const line = text.split("\n")[0];
  return text.includes("\n") ? `${line} …` : line;
}

/** "+3 −1", or "yeni +18" for a file the call made. */
export function changeMeta(change: Pick<ChangeEntry, "kind" | "added" | "removed" | "state">): string | null {
  if (change.state === "failed") return t("chat.tool.failed");
  if (change.state === "running") return null;
  if (change.kind === "create") return t("chat.change.new", { added: change.added });
  const parts: string[] = [];
  if (change.added) parts.push(`+${change.added}`);
  if (change.removed) parts.push(`−${change.removed}`);
  return parts.join(" ") || null;
}

/** "çıkış 0", "çıkış 2", or why there is none. */
export function exitMeta(command: Pick<CommandEntry, "state" | "code" | "background" | "interrupted">): string | null {
  if (command.background && command.state === "running") return t("chat.command.background");
  if (command.state === "running") return null;
  if (command.interrupted || command.state === "stopped") return t("chat.command.stopped");
  // A command that went well says nothing of it (the user's word, 15 Nov: "exit 0" told nothing);
  // one that failed says so, with its code.
  if (command.code !== null && command.code !== 0) return t("chat.command.exit", { code: command.code });
  return command.state === "failed" ? t("chat.tool.failed") : null;
}

export function taskMeta(task: Pick<TaskEntry, "state" | "tokens">): string | null {
  if (task.state === "failed") return t("chat.task.failed");
  if (task.state === "stopped") return t("chat.task.stopped");
  if (task.tokens) return t("chat.task.tokens", { count: compactNumber(task.tokens) });
  return null;
}

/** 12 400 → "12,4 bin" as the language writes it. */
export function compactNumber(value: number): string {
  try {
    return new Intl.NumberFormat(language.value, { notation: "compact", maximumFractionDigits: 1 }).format(value);
  } catch {
    return String(value);
  }
}

/** The verb of each tool's row ("Okudu"), and of a fold's count of them. */
export const VERBS: Record<string, Key> = {
  Read: "chat.tool.read",
  NotebookRead: "chat.tool.read",
  Bash: "chat.tool.bash",
  Edit: "chat.tool.edit",
  MultiEdit: "chat.tool.edit",
  NotebookEdit: "chat.tool.edit",
  Write: "chat.tool.write",
  Grep: "chat.tool.grep",
  Glob: "chat.tool.glob",
  LS: "chat.tool.glob",
  WebFetch: "chat.tool.fetch",
  WebSearch: "chat.tool.webSearch",
  Skill: "chat.tool.skill",
  Agent: "chat.tool.agent",
  Task: "chat.tool.agent",
  TodoWrite: "chat.tool.todos",
  TaskCreate: "chat.tool.todos",
  TaskUpdate: "chat.tool.todos",
  TaskList: "chat.tool.todos",
  TaskGet: "chat.tool.todos",
  AskUserQuestion: "chat.tool.ask",
  ExitPlanMode: "chat.tool.plan",
  EnterPlanMode: "chat.tool.enterPlan",
  ToolSearch: "chat.tool.toolSearch",
  TaskStop: "chat.tool.stop",
  Monitor: "chat.tool.monitor",
};

/** The row of a tool call. `change`, `command`, `task`: what it did, drawn when its row opens. */
export function toolLook(tool: ToolEntry, project: string, change?: ChangeEntry, command?: CommandEntry, task?: TaskEntry): ToolLook {
  const input = tool.input;
  const name = tool.name;
  const failed = tool.result?.error === true;
  const base: ToolLook = { icon: "tool", verb: t(VERBS[name] ?? "chat.tool.used"), arg: "", mono: true, meta: null, tone: null, quiet: QUIET_TOOLS.has(name), hint: null };
  if (name.startsWith("mcp__")) {
    const [, server = "", ...rest] = name.split("__");
    return { ...base, icon: "plug", verb: server, arg: rest.join("__"), mono: false, meta: failed ? t("chat.tool.failed") : null, tone: failed ? "error" : null, quiet: false };
  }
  switch (name) {
    case "Read": {
      const range = typeof input.offset === "number" ? ` :${input.offset}` : "";
      return { ...base, icon: "file", arg: relPath(project, s(input.file_path)) + range, meta: failed ? t("chat.tool.failed") : null, tone: failed ? "error" : null };
    }
    case "Bash": {
      // What the command is for, as Claude names it, else the command; the command itself shows
      // on the pointer and in its row when it opens.
      const line = s(input.command) || (tool.partial ? partialField(tool.partial, "command") : "");
      const told = s(input.description) || (tool.partial ? partialField(tool.partial, "description") : "");
      return {
        ...base,
        icon: "terminal",
        arg: told || firstLine(line),
        mono: !told,
        hint: told ? line : null,
        meta: command ? exitMeta(command) : null,
        tone: command?.state === "failed" ? "error" : null,
      };
    }
    case "Edit":
    case "MultiEdit":
    case "NotebookEdit":
    case "Write": {
      const file = s(input.file_path) || s(input.notebook_path) || partialField(tool.partial, "file_path");
      const made = change?.kind === "create";
      return {
        ...base,
        icon: made || (name === "Write" && !change) ? "file-plus" : "pencil",
        arg: relPath(project, file),
        meta: change ? changeMeta(change) : null,
        tone: change?.state === "failed" ? "error" : change && (change.added || change.kind === "create") ? "add" : null,
      };
    }
    case "Grep":
      return { ...base, icon: "search", arg: [s(input.pattern), s(input.glob) || (s(input.path) ? relPath(project, s(input.path)) : "")].filter(Boolean).join("  ") };
    case "Glob":
    case "LS":
      return { ...base, icon: "search", arg: s(input.pattern) || relPath(project, s(input.path)) };
    case "WebFetch":
      return { ...base, icon: "globe", arg: s(input.url) };
    case "WebSearch":
      return { ...base, icon: "globe", arg: s(input.query), mono: false };
    case "Skill":
      return { ...base, icon: "spark", arg: s(input.skill) || s(input.command) || s(input.name) };
    case "Agent":
    case "Task":
      return {
        ...base,
        icon: "agent",
        arg: s(input.description) || s(input.name),
        mono: false,
        meta: task ? (task.state === "running" ? null : taskMeta(task)) : null,
        tone: task?.state === "failed" ? "error" : null,
      };
    case "TodoWrite":
    case "TaskCreate":
    case "TaskUpdate":
    case "TaskList":
    case "TaskGet":
      return { ...base, icon: "list", arg: name === "TaskCreate" ? s(input.subject) : "", mono: false };
    case "AskUserQuestion": {
      const questions = Array.isArray(input.questions) ? (input.questions as Record<string, unknown>[]) : [];
      return { ...base, icon: "question", arg: s(questions[0]?.header) || s(questions[0]?.question), mono: false };
    }
    case "ExitPlanMode":
    case "EnterPlanMode": {
      // An approved plan says so (PL5D); a plan sent back is "declined".
      const approved = name === "ExitPlanMode" && !tool.denied && !!tool.result && !tool.result.error;
      return { ...base, icon: "plan", mono: false, meta: tool.denied ? t("chat.tool.declined") : approved ? t("chat.tool.approved") : null, tone: approved ? "ok" : null };
    }
    case "ToolSearch":
      return { ...base, icon: "search", arg: s(input.query) };
    default:
      return { ...base, arg: name, mono: false, meta: failed ? t("chat.tool.failed") : null, tone: failed ? "error" : null };
  }
}

/** A string field of a tool's input while its JSON still streams in. */
function partialField(json: string, field: string): string {
  const match = new RegExp(`"${field}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`).exec(json);
  if (!match) return "";
  try {
    return JSON.parse(`"${match[1].replace(/\\$/, "")}"`) as string;
  } catch {
    return match[1];
  }
}

/** The hunks of one change as the diff view draws them (lib/unifiedDiff's lines). */
export function diffLines(hunks: Hunk[]): DiffLine[] {
  const lines: DiffLine[] = [];
  for (const hunk of hunks) {
    let old = hunk.oldStart;
    let now = hunk.newStart;
    lines.push({ kind: "hunk", text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`, old: null, new: null });
    for (const raw of hunk.lines) {
      const sign = raw[0];
      const text = raw.slice(1);
      if (sign === "+") lines.push({ kind: "add", text, old: null, new: now++ });
      else if (sign === "-") lines.push({ kind: "del", text, old: old++, new: null });
      else if (sign === " ") lines.push({ kind: "context", text, old: old++, new: now++ });
    }
  }
  return lines;
}

/** The input of a call, as `key: value` lines, for a row opened (a value cut when long). */
export function inputLines(input: Record<string, unknown>): Array<{ key: string; value: string }> {
  return Object.entries(input)
    .filter(([, value]) => value !== undefined && value !== null && value !== "")
    .map(([key, value]) => {
      const text = typeof value === "string" ? value : JSON.stringify(value, null, 1);
      return { key, value: text.length > 2000 ? `${text.slice(0, 2000)} …` : text };
    });
}
