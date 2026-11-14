// The composer's `/` commands: the menu built from what the CLI lists (`info.commands`, sectioned),
// and where each typed command goes. The rule (docs/SOHBET.md, Goal): a command the window draws
// itself opens its dialog; one only Claude Code's terminal UI has (the CLI answers "/x isn't
// available in this environment" under `-p`) continues the session in a terminal tab; every other
// listed one, a skill, a plugin's command, `/compact`, `/clear`, `/init`…, goes to Claude as the
// text typed. A command the CLI lists is never sent to the terminal: the runtime list wins over
// the tables here, so one Claude Code makes headless later is simply sent.

import { fuzzy } from "../fuzzy";
import { t, type Key } from "../i18n";
import type { ChatDialogKind, ChatDialogOpen } from "./ui";
import type { ChatCommand, ChatControl, ChatEffort, ChatInfo, ChatModelInfo, ChatPermissionMode } from "./wire";

export type MenuSection = "builtin" | "skills" | "plugins" | "mcp";

/** How a command runs from the composer. */
export type Via = "send" | "dialog" | "terminal" | "settings";

export type MenuCommand = {
  name: string;
  /** The CLI's own words (English, as it lists them) or the window's. */
  description: string;
  hint: string;
  /** Its arguments are needed before it runs (Enter on it in the menu only completes it). */
  needs: boolean;
  aliases: string[];
  section: MenuSection;
  via: Via;
};

export const SECTIONS: { id: MenuSection; key: Key }[] = [
  { id: "builtin", key: "chatInput.menu.builtin" },
  { id: "skills", key: "chatInput.menu.skills" },
  { id: "plugins", key: "chatInput.menu.plugins" },
  { id: "mcp", key: "chatInput.menu.mcp" },
];

/** The built-ins whose dialog the window draws, by every name the CLI knows them by. */
const DIALOGS: Record<string, ChatDialogKind> = {
  model: "model",
  context: "context",
  usage: "usage",
  cost: "usage",
  stats: "usage",
  status: "status",
  mcp: "mcp",
  permissions: "permissions",
  "allowed-tools": "permissions",
  hooks: "hooks",
  memory: "memory",
  skills: "skills",
  "output-style": "styles",
  agents: "agents",
  export: "export",
  help: "help",
  rewind: "rewind",
  checkpoint: "rewind",
  undo: "rewind",
  fork: "rewind",
  branch: "rewind",
  btw: "btw",
  resume: "resume",
  continue: "resume",
  plan: "plan",
};

/** What the window draws for itself and the CLI doesn't list under `-p`: offered in the menu by
 * their first name, with the window's own words (and hint, `needs` when an argument must follow). */
const OWN: { name: string; key: Key; hint?: Key; needs?: boolean }[] = [
  { name: "help", key: "chatInput.cmd.help" },
  { name: "status", key: "chatInput.cmd.status" },
  { name: "permissions", key: "chatInput.cmd.permissions" },
  { name: "hooks", key: "chatInput.cmd.hooks" },
  { name: "memory", key: "chatInput.cmd.memory" },
  { name: "skills", key: "chatInput.cmd.skills" },
  { name: "export", key: "chatInput.cmd.export", hint: "chatInput.hint.export" },
  { name: "rewind", key: "chatInput.cmd.rewind" },
  { name: "btw", key: "chatInput.cmd.btw", hint: "chatInput.hint.btw", needs: true },
  { name: "resume", key: "chatInput.cmd.resume" },
  { name: "plan", key: "chatInput.cmd.plan", hint: "chatInput.hint.plan" },
  { name: "add-dir", key: "chatInput.cmd.addDir" },
];

/** Only Claude Code's terminal UI has these (inventory part 2, §10): typed in a chat, the session
 * goes on in a terminal tab. Never for a command the CLI lists. */
const TERMINAL = new Set([
  "login",
  "logout",
  "keybindings",
  "theme",
  "terminal-setup",
  "ide",
  "install-github-app",
  "install-slack-app",
  "vim",
  "doctor",
  "bug",
  "share",
  "feedback",
  "chrome",
  "sandbox",
  "privacy-settings",
  "release-notes",
  "upgrade",
  "mobile",
  "ios",
  "android",
  "desktop",
  "app",
  "passes",
  "powerup",
  "radio",
  "remote-control",
  "rc",
  "remote-env",
  "restart",
  "update",
  "setup-bedrock",
  "setup-vertex",
  "stickers",
  "subtask",
  "teleport",
  "tp",
  "web-setup",
  "workflows",
  "artifacts",
  "autofix-pr",
  "design-login",
  "statusline",
  "tui",
  "scroll-speed",
  "voice",
  "rate-limit-options",
  "background",
  "bg",
]);

/** Of those, the ones the menu offers (with a terminal mark); the rest work when typed. */
const TERMINAL_IN_MENU = ["login", "logout", "ide", "install-github-app", "bug", "sandbox", "privacy-settings", "release-notes", "remote-control"];

/** Listed by the CLI but meaningless without a terminal, or internal: kept out of the menu (still
 * sent when typed). */
const HIDDEN = new Set(["color", "focus", "heapdump", "cd", "background", "bg", "statusline", "scroll-speed", "tui", "terminal-setup", "theme", "keybindings", "extra-usage", "workflow-launch-exec"]);

const listedNames = (info: ChatInfo | null): Set<string> => {
  const names = new Set<string>();
  for (const command of info?.commands ?? []) {
    names.add(command.name);
    for (const alias of command.aliases ?? []) names.add(alias);
  }
  return names;
};

function sectionOf(command: ChatCommand, skills: Set<string>): MenuSection {
  if (command.name.startsWith("mcp__") || / \(MCP\)$/.test(command.name)) return "mcp";
  if (command.name.includes(":")) return "plugins";
  if (skills.has(command.name) || !command.builtin) return "skills";
  return "builtin";
}

function viaOf(name: string): Via {
  if (name === "config" || name === "settings") return "settings";
  return DIALOGS[name] ? "dialog" : "send";
}

/** Every command the menu offers, in its sections' order: the CLI's own list (built-ins, skills
 * and commands, plugins' commands, MCP prompts) with the window's dialogs and the terminal's
 * flows it lacks. */
export function menuCommands(info: ChatInfo | null): MenuCommand[] {
  const skills = new Set(info?.skills ?? []);
  const listed = listedNames(info);
  const out: MenuCommand[] = [];
  for (const command of info?.commands ?? []) {
    if (!command.name || command.name.startsWith("__") || HIDDEN.has(command.name)) continue;
    out.push({
      name: command.name,
      description: (command.description ?? "").split("\n")[0],
      hint: command.argumentHint ?? "",
      needs: cliNeeds(command.argumentHint ?? ""),
      aliases: command.aliases ?? [],
      section: sectionOf(command, skills),
      via: viaOf(command.name),
    });
  }
  for (const own of OWN) {
    if (listed.has(own.name)) continue;
    out.push({ name: own.name, description: t(own.key), hint: own.hint ? t(own.hint) : "", needs: own.needs === true, aliases: [], section: "builtin", via: "dialog" });
  }
  for (const name of TERMINAL_IN_MENU) {
    if (listed.has(name)) continue;
    out.push({ name, description: t("chatInput.cmd.terminal"), hint: "", needs: false, aliases: [], section: "builtin", via: "terminal" });
  }
  const order = SECTIONS.map((section) => section.id);
  return out.sort((a, b) => order.indexOf(a.section) - order.indexOf(b.section) || a.name.localeCompare(b.name));
}

/** The menu's commands that match what was typed after `/`, best first: the name or an alias
 * itself, then those that start so, then fuzzy matches. */
export function filterCommands(all: MenuCommand[], query: string): MenuCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return all;
  const scored: { command: MenuCommand; score: number }[] = [];
  for (const command of all) {
    const names = [command.name, ...command.aliases];
    let score = -1;
    for (const name of names) {
      if (name.toLowerCase() === q) score = Math.max(score, 3000);
      else if (name.toLowerCase().startsWith(q)) score = Math.max(score, 2000 - name.length);
      else {
        const match = fuzzy(name, q);
        if (match) score = Math.max(score, match.score);
      }
    }
    if (score >= 0) scored.push({ command, score });
  }
  return scored.sort((a, b) => b.score - a.score).map((each) => each.command);
}

/** Whether a command's arguments are needed before it runs. */
export function needsArgs(command: MenuCommand): boolean {
  return command.needs;
}

/** Of a command the CLI lists, only its own hint tells: one that names a `<required>` argument. */
function cliNeeds(hint: string): boolean {
  return /^\s*</.test(hint) && !/optional/i.test(hint);
}

/** A typed line split as the CLI reads it: `/name rest`; null when it isn't a command. */
export function parseCommand(line: string): { name: string; rest: string } | null {
  const match = /^\/([^\s/]+)(?:\s+([\s\S]*))?$/.exec(line.trim());
  return match ? { name: match[1], rest: (match[2] ?? "").trim() } : null;
}

/** Where a typed command goes. */
export type Route =
  | { kind: "send" }
  | { kind: "dialog"; open: ChatDialogOpen }
  | { kind: "control"; control: ChatControl; /** Text sent after it (`/plan <task>`). */ then?: string }
  | { kind: "settings" }
  | { kind: "terminal"; command: string }
  | { kind: "pickDir" }
  | { kind: "refused"; key: Key };

export type RouteContext = { info: ChatInfo | null; mode: string | null; model: string | null; effort: string | null };

const EFFORTS: ChatEffort[] = ["low", "medium", "high", "xhigh", "max", "auto"];

/** The model of `info` that `value` names (its value, or the model it resolves to); with no value
 * (a chat started without `--model`, before its first turn names one), the CLI's default. */
export function modelOf(info: ChatInfo | null, value: string | null): ChatModelInfo | null {
  if (!info) return null;
  const models = info.models ?? [];
  if (!value) return models.find((model) => model.value === "default") ?? null;
  return models.find((model) => model.value === value) ?? models.find((model) => model.value !== "default" && model.resolvedModel === value) ?? models.find((model) => model.resolvedModel === value) ?? null;
}

/** Where `/name rest` goes. A command not known here and not listed goes as text: the CLI tells
 * Claude it is unknown, as Claude Code does. */
export function routeCommand(name: string, rest: string, ctx: RouteContext): Route {
  const listed = listedNames(ctx.info);
  const arg = rest || undefined;
  const dialog = (kind: ChatDialogKind): Route => ({ kind: "dialog", open: arg ? { kind, arg } : { kind } });

  switch (name) {
    case "model": {
      const picked = rest ? (ctx.info?.models ?? []).find((model) => model.value === rest || model.resolvedModel === rest) : null;
      return picked ? { kind: "control", control: { kind: "setModel", model: picked.value } } : dialog("model");
    }
    case "effort": {
      if (!rest) return dialog("model");
      const level = rest.toLowerCase() as ChatEffort;
      return EFFORTS.includes(level) ? { kind: "control", control: { kind: "setEffort", level } } : { kind: "send" };
    }
    case "fast": {
      const on = rest === "on" ? true : rest === "off" ? false : ctx.info?.fastModeState !== "on";
      return { kind: "control", control: { kind: "setFast", on } };
    }
    case "plan": {
      if (rest === "open" || (!rest && ctx.mode === "plan")) return { kind: "dialog", open: { kind: "plan" } };
      const control: ChatControl = { kind: "setMode", mode: "plan" };
      return rest ? { kind: "control", control, then: rest } : { kind: "control", control };
    }
    case "output-style": {
      if (!rest) return dialog("styles");
      return (ctx.info?.outputStyles ?? []).includes(rest) ? { kind: "control", control: { kind: "setStyle", style: rest } } : { kind: "send" };
    }
    case "mcp": {
      const [verb, server] = rest.split(/\s+/);
      if (verb === "reconnect" && server && server !== "all") return { kind: "control", control: { kind: "mcpReconnect", server } };
      if ((verb === "enable" || verb === "disable") && server && server !== "all") return { kind: "control", control: { kind: "mcpToggle", server, enabled: verb === "enable" } };
      return dialog("mcp");
    }
    case "rename":
      return rest ? { kind: "control", control: { kind: "rename", title: rest } } : { kind: "send" };
    case "config":
    case "settings":
      return rest ? { kind: "send" } : { kind: "settings" };
    case "add-dir":
      return { kind: "pickDir" };
    case "cd":
      return { kind: "refused", key: "chatInput.notice.cd" };
  }
  if (DIALOGS[name]) return dialog(DIALOGS[name]);
  if (TERMINAL.has(name) && !listed.has(name)) return { kind: "terminal", command: rest ? `/${name} ${rest}` : `/${name}` };
  return { kind: "send" };
}

/** The names a skill, a plugin's command or an MCP prompt goes by in the CLI's list (aliases too):
 * typed anywhere in a message, each is drawn as one (`skillSegments`). */
export function skillNames(info: ChatInfo | null): Set<string> {
  const skills = new Set(info?.skills ?? []);
  const out = new Set(skills);
  for (const command of info?.commands ?? []) {
    if (!command.name || sectionOf(command, skills) === "builtin") continue;
    out.add(command.name);
    for (const alias of command.aliases ?? []) out.add(alias);
  }
  return out;
}

/** Whether `name` is a command the composer knows (listed, or the window's own): typed at the
 * message's start it is drawn as one. */
export function isKnownCommand(name: string, info: ChatInfo | null): boolean {
  return listedNames(info).has(name) || !!DIALOGS[name] || TERMINAL.has(name) || name === "add-dir" || name === "config";
}

/** The permission modes Shift+Tab goes through, as Claude Code's cycle: auto only where the
 * model takes it; `dontAsk` is never in it (its dialog alone). */
export function nextMode(mode: string | null, model: ChatModelInfo | null): ChatPermissionMode {
  const cycle: ChatPermissionMode[] = ["default", "acceptEdits", "plan"];
  if (model?.supportsAutoMode) cycle.push("auto");
  const at = cycle.indexOf((mode ?? "default") as ChatPermissionMode);
  return cycle[(at + 1) % cycle.length];
}

/** The effort level after `effort` among the model's own (a click on the chip). */
export function nextEffort(effort: string | null, model: ChatModelInfo | null): ChatEffort | null {
  const levels = model?.supportedEffortLevels ?? [];
  if (!levels.length) return null;
  const at = levels.indexOf((effort ?? "") as ChatEffort);
  return levels[(at + 1) % levels.length];
}
