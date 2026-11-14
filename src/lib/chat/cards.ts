// What a chat's permission card may offer as "Yes, and don't ask again" (docs/SOHBET.md,
// Permissions): only entries the CLI suggested for that request (`permission_suggestions`), to
// the place the user picks. The core copies the entries named by index and sets the destination
// on each, so the choice of indices and places here decides what lands in a settings file.

import type { CanUseToolRequest, PermissionDestination } from "./wire";

/** Where a rule can be kept, as the card offers them: this project for me, shared with the team,
 * every project, this session alone. */
export const RULE_PLACES: PermissionDestination[] = ["localSettings", "projectSettings", "userSettings", "session"];

export type AlwaysOffer = {
  /** Indices into the request's `permission_suggestions`: what the answer names. */
  suggestions: number[];
  /** Where it may be kept; the first one the card opens on is `initial`. */
  places: PermissionDestination[];
  initial: PermissionDestination;
  /** What would be allowed, to show: `Bash(git status:*)`, folders, a mode. */
  rules: string[];
  directories: string[];
  mode: string | null;
};

/**
 * The "always" offer of a request, or null when there is none (no suggestions, or the CLI said
 * not to offer one). Rules and folders the CLI suggests to allow can go to any of the four
 * places. A suggested mode (Write's "allow all edits") is kept to this session alone: sent with
 * a settings file as its place, it would become that file's default mode. A suggestion that
 * denies, asks, replaces or removes is never offered.
 */
export function alwaysOffer(request: CanUseToolRequest): AlwaysOffer | null {
  if (request.suppress_always_allow_rule) return null;
  const all = Array.isArray(request.permission_suggestions) ? request.permission_suggestions : [];
  const rules: number[] = [];
  const modes: number[] = [];
  all.forEach((entry, at) => {
    if (!entry || typeof entry !== "object") return;
    if (entry.type === "addRules" && entry.behavior === "allow" && Array.isArray(entry.rules) && entry.rules.length) rules.push(at);
    else if (entry.type === "addDirectories" && Array.isArray(entry.directories) && entry.directories.length) rules.push(at);
    else if (entry.type === "setMode" && typeof entry.mode === "string" && entry.mode !== "bypassPermissions") modes.push(at);
  });

  if (rules.length) {
    const named = rules.map((at) => all[at]);
    const first = named[0].destination;
    return {
      suggestions: rules,
      places: RULE_PLACES,
      initial: RULE_PLACES.includes(first) ? first : "localSettings",
      rules: named.flatMap((entry) => (entry.type === "addRules" ? entry.rules.map((rule) => (rule.ruleContent ? `${rule.toolName}(${rule.ruleContent})` : rule.toolName)) : [])),
      directories: named.flatMap((entry) => (entry.type === "addDirectories" ? entry.directories : [])),
      mode: null,
    };
  }
  if (modes.length) {
    const entry = all[modes[0]];
    return { suggestions: [modes[0]], places: ["session"], initial: "session", rules: [], directories: [], mode: entry.type === "setMode" ? entry.mode : null };
  }
  return null;
}

/* ---------- what a card shows ---------- */

export type DiffRow = { kind: "ctx" | "add" | "del"; text: string };

/** Above this many line pairs the diff isn't worked out: the old lines go, the new ones come. */
const DIFF_CELLS = 250_000;

/** A text's lines; the empty one after a closing newline is no line of its own. */
function linesOf(text: string): string[] {
  if (!text) return [];
  const lines = text.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/**
 * The lines an edit changes, as a diff of `before` against `after` (an Edit's `old_string` and
 * `new_string`): the lines both share at the ends kept as context, the middle by its longest
 * common lines.
 */
export function lineDiff(before: string, after: string): DiffRow[] {
  const a = linesOf(before);
  const b = linesOf(after);
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
  const midA = a.slice(head, a.length - tail);
  const midB = b.slice(head, b.length - tail);
  const rows: DiffRow[] = a.slice(0, head).map((text) => ({ kind: "ctx", text }));
  if (midA.length * midB.length > DIFF_CELLS) {
    rows.push(...midA.map((text) => ({ kind: "del" as const, text })), ...midB.map((text) => ({ kind: "add" as const, text })));
  } else {
    // Longest common subsequence, walked from the front.
    const n = midA.length;
    const m = midB.length;
    const table = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) table[i][j] = midA[i] === midB[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && midA[i] === midB[j]) {
        rows.push({ kind: "ctx", text: midA[i] });
        i++;
        j++;
      } else if (i < n && (j >= m || table[i + 1][j] >= table[i][j + 1])) rows.push({ kind: "del", text: midA[i++] });
      else rows.push({ kind: "add", text: midB[j++] });
    }
  }
  rows.push(...a.slice(a.length - tail).map((text) => ({ kind: "ctx" as const, text })));
  return rows;
}

/** An MCP tool's name (`mcp__server__tool`) as its server and its tool, or null. */
export function mcpTool(name: string): { server: string; tool: string } | null {
  const match = /^mcp__(.+?)__(.+)$/.exec(name);
  return match ? { server: match[1], tool: match[2] } : null;
}

/** A tool's input, to read folded: JSON, cut to a size the card can hold. */
export function inputText(input: unknown, max = 20_000): string {
  let text: string;
  try {
    text = JSON.stringify(input, null, 2) ?? "";
  } catch {
    text = String(input);
  }
  return text.length > max ? `${text.slice(0, max)}\n…` : text;
}

/** A field of a tool's input when it is a string, else "". */
export function field(input: Record<string, unknown> | undefined, key: string): string {
  const value = input?.[key];
  return typeof value === "string" ? value : "";
}
