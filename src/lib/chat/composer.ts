// The composer's state and the rules it writes a message by (docs/SOHBET.md, The composer).
//
// A chat's draft (its text, the mentions picked into it, its attachments, the stash and the
// prompts sent from that tab for ↑) is the page's memory alone, per chat id, as the quick
// switcher's card is: never written to disk, gone when the app quits (O7, V30).

import { reactive } from "vue";

import { fold, fuzzy } from "../fuzzy";

import type { ChatContext } from "./ui";
import type { ChatOutgoing } from "./wire";

/** What goes with the text. A pasted block is text the user pasted (or a terminal's lines),
 * folded in the composer and sent after what was typed. */
export type Attachment =
  | { kind: "image"; key: string; mediaType: string; data: string; name: string; scaled: boolean }
  | { kind: "pdf"; key: string; data: string; name: string }
  | { kind: "pasted"; key: string; text: string; lines: number; name: string | null };

type Kept = { text: string; mentions: string[]; attachments: Attachment[] };

export type Draft = {
  text: string;
  /** The mentions picked from the `@` menu, as written after `@` (`src/app.ts#5-10`,
   * `agent-Explore`): drawn as chips, deleted whole. */
  mentions: string[];
  attachments: Attachment[];
  /** Ctrl+S: one draft put aside, back after the next send (C22). */
  stash: Kept | null;
  /** The prompts sent from this tab, oldest first (↑/↓, O7). */
  history: string[];
  /** The Files window's file the user took off this message (its chip's ✕), by `contextKey`. */
  contextOff: string | null;
};

const drafts = new Map<number, Draft>();

export function draftOf(id: number): Draft {
  let draft = drafts.get(id);
  if (!draft) {
    draft = reactive<Draft>({ text: "", mentions: [], attachments: [], stash: null, history: [], contextOff: null });
    drafts.set(id, draft);
  }
  return draft;
}

/** A chat tab closed: its draft and its prompts go. */
export function forgetDraft(id: number): void {
  drafts.delete(id);
}

/** Whether there is nothing to send and nothing kept (a closed tab's draft can then go). */
export function isBlank(draft: Draft): boolean {
  return !draft.text && !draft.attachments.length && !draft.stash && !draft.history.length;
}

let counter = 0;
export const newKey = (): string => `a${++counter}`;

/* ---------- limits (contract: ChatOutgoing) ---------- */

/** A message's text, at most (V29): longer, it stays in the composer. */
export const TEXT_MAX = 50_000;
/** A paste longer than this, or of more lines, goes as a folded block marked as pasted (V27, C15). */
export const PASTE_CHARS = 800;
export const PASTE_LINES = 3;
/** An image's base64, at most: the core refuses more, so the page scales it first. */
export const IMAGE_MAX = 5 * 1024 * 1024;
/** A PDF's base64, at most (the API takes 32 MB a request). */
export const PDF_MAX = 30 * 1024 * 1024;
export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

export const lineCount = (text: string): number => text.split("\n").length;

/** A paste that goes as a folded block rather than into the text. */
export const isLongPaste = (text: string): boolean => text.length > PASTE_CHARS || lineCount(text) > PASTE_LINES;

/* ---------- invisible characters (V28, C16) ---------- */

// Unicode tags (U+E0000–E007F, used to smuggle instructions), bidi controls and marks, zero-width
// spaces and joiners, word joiner, invisible operators, the BOM. A zero-width joiner between two
// pictographs (a family emoji) is the emoji's own and stays.
const INVISIBLE = /[\u{E0000}-\u{E007F}‪-‮⁦-⁩‎‏؜​‌⁠-⁤﻿᠎]|(?<![\p{Extended_Pictographic}\p{Emoji_Modifier}]️?)‍|‍(?!\p{Extended_Pictographic})/gu;

/** `text` without its invisible characters, and how many went. */
export function scrub(text: string): { text: string; removed: number } {
  let removed = 0;
  const clean = text.replace(INVISIBLE, () => {
    removed++;
    return "";
  });
  return { text: clean, removed };
}

/* ---------- the drawn text: chips, the keywords, a command ---------- */

export type Segment = { text: string; kind: "plain" | "mention" | "command" | "skill" | "ultra" };

type Mark = { from: number; to: number; kind: Segment["kind"] };

/** A `/name` standing as a word (at the start, after a space or a bracket), a plugin's
 * `/plugin:name` too; what ends a sentence after it is not its. */
const SLASH_WORD = /(^|[\s(\[])\/([\p{L}\p{N}_-]+(?::[\p{L}\p{N}_-]+)*)(?=$|[\s.,;:!?)\]"'’”])/gu;

/** Where `text` names one of `skills` as a slash word: a skill typed anywhere in a message stands
 * out from its words (the user's word, 16 Nov). */
function skillMarks(text: string, skills: Set<string>): Mark[] {
  const marks: Mark[] = [];
  if (!skills.size) return marks;
  for (const match of text.matchAll(SLASH_WORD)) {
    if (!skills.has(match[2])) continue;
    const from = (match.index ?? 0) + match[1].length;
    marks.push({ from, to: from + 1 + match[2].length, kind: "skill" });
  }
  return marks;
}

/** `text` cut by `marks`, which may not overlap (a later one inside an earlier is dropped). */
function cut(text: string, marks: Mark[]): Segment[] {
  marks.sort((a, b) => a.from - b.from);
  const out: Segment[] = [];
  let at = 0;
  for (const mark of marks) {
    if (mark.from < at) continue;
    if (mark.from > at) out.push({ text: text.slice(at, mark.from), kind: "plain" });
    out.push({ text: text.slice(mark.from, mark.to), kind: mark.kind });
    at = mark.to;
  }
  if (at < text.length) out.push({ text: text.slice(at), kind: "plain" });
  return out;
}

/** A sent message cut into its words and the skills it names (`skillMarks`). */
export function skillSegments(text: string, skills: Set<string>): Segment[] {
  return cut(text, skillMarks(text, skills));
}

/** `text` cut into what the composer draws under the field: the skills and the known commands it
 * names (at its start or further on), the mentions picked, `ultrathink` and `ultracode` (V31, C18). */
export function segments(text: string, mentions: string[], command: (name: string) => boolean, skills: Set<string> = new Set()): Segment[] {
  const marks = skillMarks(text, skills);
  const head = /^\/([^\s/]+)/.exec(text);
  if (head && !marks.some((mark) => mark.from === 0) && command(head[1])) marks.push({ from: 0, to: head[0].length, kind: "command" });
  // A known command named further on is drawn as one too, not only the first (the user's word,
  // 16 Nov: the second went plain).
  for (const match of text.matchAll(SLASH_WORD)) {
    const from = (match.index ?? 0) + match[1].length;
    if (from === 0 || marks.some((mark) => mark.from === from) || !command(match[2])) continue;
    marks.push({ from, to: from + 1 + match[2].length, kind: "command" });
  }
  if (mentions.length) {
    for (const match of text.matchAll(/(^|\s)@(\S+)/g)) {
      const body = match[2];
      if (!mentions.includes(body)) continue;
      const from = (match.index ?? 0) + match[1].length;
      marks.push({ from, to: from + 1 + body.length, kind: "mention" });
    }
  }
  for (const match of text.matchAll(/\bultra(?:think|code)\b/gi)) {
    const from = match.index ?? 0;
    marks.push({ from, to: from + match[0].length, kind: "ultra" });
  }
  return cut(text, marks);
}

/** The mention that ends right at `caret` (for Backspace to take it whole), as its start. */
export function mentionBefore(text: string, caret: number, mentions: string[]): number | null {
  const before = text.slice(0, caret);
  for (const mention of mentions) {
    const token = `@${mention}`;
    if (!before.endsWith(token)) continue;
    const start = caret - token.length;
    if (start === 0 || /\s/.test(text[start - 1])) return start;
  }
  return null;
}

/** The mentions `text` still holds (one deleted by hand is no chip any more). */
export function liveMentions(text: string, mentions: string[]): string[] {
  return mentions.filter((mention) => new RegExp(`(^|\\s)@${mention.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?=\\s|$)`).test(text));
}

/** What is being typed as a mention at `caret`: the `@` word it is in. */
export function mentionAt(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const match = /(^|\s)@([^\s@]*)$/.exec(before);
  if (!match) return null;
  return { start: caret - match[2].length - 1, query: match[2] };
}

/** What is being typed as a command: `/` at the message's start (C3) or after a space mid-text,
 * which only completes (C4). */
export function commandAt(text: string, caret: number): { start: number; query: string; head: boolean } | null {
  const before = text.slice(0, caret);
  const match = /(^|\s)\/([^\s/]*)$/.exec(before);
  if (!match) return null;
  const start = caret - match[2].length - 1;
  return { start, query: match[2], head: start === 0 };
}

/** `path` as the CLI's `@` takes it: relative to the project when it is inside it. */
export function relativePath(path: string, project: string): string {
  const root = project.endsWith("/") ? project : `${project}/`;
  return path.startsWith(root) ? path.slice(root.length) : path;
}

/** A mention's text: the path, then its lines (`#5-10`, `#5`). */
export function mentionText(path: string, startLine?: number, endLine?: number): string {
  if (!startLine) return path;
  return endLine && endLine !== startLine ? `${path}#${startLine}-${endLine}` : `${path}#${startLine}`;
}

/** The Files window's file as the key its chip is taken off by. */
export function contextKey(context: ChatContext): string | null {
  return context ? mentionText(context.path, context.startLine, context.endLine) : null;
}

/* ---------- the message ---------- */

/** The text that goes: what was typed, the Files window's file as a mention, each pasted block. */
export function messageText(typed: string, attachments: Attachment[], context: string | null): string {
  const parts = [typed.trim()];
  if (context) parts.push(`@${context}`);
  for (const each of attachments) if (each.kind === "pasted") parts.push(each.text);
  return parts.filter(Boolean).join("\n\n");
}

export function outgoing(text: string, attachments: Attachment[], priority: ChatOutgoing["priority"]): ChatOutgoing {
  return {
    text,
    images: attachments.flatMap((each) => (each.kind === "image" ? [{ mediaType: each.mediaType, data: each.data }] : [])),
    documents: attachments.flatMap((each) => (each.kind === "pdf" ? [{ mediaType: "application/pdf" as const, data: each.data, name: each.name }] : [])),
    priority,
    pasted: attachments.some((each) => each.kind === "pasted"),
  };
}

/* ---------- pictures and documents (browser only) ---------- */

function readDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("unreadable"));
    reader.readAsDataURL(file);
  });
}

const base64Of = (url: string): string => url.slice(url.indexOf(",") + 1);
const typeOf = (url: string): string => url.slice(5, url.indexOf(";"));

/**
 * A picture as the message takes it: PNG, JPEG, GIF or WebP as it is while its base64 is under
 * 5 MB; else (larger, or another kind the page can draw) drawn again on a canvas, smaller each
 * time, until it is. Fails for what the page can't draw.
 */
export async function readImage(file: Blob): Promise<{ mediaType: string; data: string; scaled: boolean }> {
  const url = await readDataUrl(file);
  const type = file.type || typeOf(url);
  if (IMAGE_TYPES.includes(type) && base64Of(url).length < IMAGE_MAX) return { mediaType: type, data: base64Of(url), scaled: false };
  const picture = new Image();
  picture.src = url;
  await picture.decode();
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) throw new Error("no canvas");
  const keepsAlpha = type === "image/png" || type === "image/gif" || type === "image/webp";
  let factor = Math.min(1, Math.sqrt(IMAGE_MAX / Math.max(1, base64Of(url).length)) * 0.95);
  for (let tries = 0; tries < 10; tries++) {
    canvas.width = Math.max(1, Math.round(picture.naturalWidth * factor));
    canvas.height = Math.max(1, Math.round(picture.naturalHeight * factor));
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.drawImage(picture, 0, 0, canvas.width, canvas.height);
    for (const [kind, quality] of keepsAlpha ? ([["image/png", undefined], ["image/jpeg", 0.85]] as const) : ([["image/jpeg", 0.85]] as const)) {
      const out = canvas.toDataURL(kind, quality);
      if (base64Of(out).length < IMAGE_MAX) return { mediaType: kind, data: base64Of(out), scaled: true };
    }
    factor *= 0.75;
  }
  throw new Error("too large");
}

/** A PDF as a `document` block's base64; fails past `PDF_MAX`. */
export async function readPdf(file: Blob): Promise<string> {
  const data = base64Of(await readDataUrl(file));
  if (data.length > PDF_MAX) throw new Error("too large");
  return data;
}

/** The files a paste or a drop carries that the composer takes. */
export function takenFiles(data: DataTransfer | null): File[] {
  return Array.from(data?.files ?? []).filter((file) => file.type.startsWith("image/") || file.type === "application/pdf");
}

/* ---------- the `@` menu's files ---------- */

export type FileHit = { path: string; dir: boolean };

/** One entry of `file_suggestions` (its shape isn't documented: a string, or an object naming a
 * path), as a hit; null for anything else. */
export function suggestionHit(entry: unknown): FileHit | null {
  if (typeof entry === "string") return entry ? { path: entry, dir: entry.endsWith("/") } : null;
  if (!entry || typeof entry !== "object") return null;
  const record = entry as Record<string, unknown>;
  const path = [record.path, record.displayText, record.value, record.name, record.id].find((each): each is string => typeof each === "string" && each.length > 0);
  if (!path) return null;
  const kind = String(record.type ?? record.kind ?? "");
  return { path, dir: kind === "directory" || kind === "dir" || kind === "folder" || record.isDirectory === true || path.endsWith("/") };
}

/**
 * The `@` menu's files for `query` (paths relative to the project): those whose path holds what
 * was typed as typed, the name before the folder (a name that is it, then one that starts with
 * it, then one that holds it, then a folder that does), shorter paths first; only when none does,
 * the loose ones, every letter in its order (Claude Code's own suggestions, then the Files
 * window's names by their score). The user's word, 14 Nov: "sonuçlar daralmıyor; hiç bir şey
 * bulamazsa fallback'lara geçsin". Without a query: Claude Code's, then the project's top level.
 */
export function mentionFiles(cli: FileHit[], names: FileHit[], query: string, limit = 40): FileHit[] {
  const seen = new Set<string>();
  const all: FileHit[] = [];
  for (const hit of [...cli, ...names]) {
    if (seen.has(hit.path)) continue;
    seen.add(hit.path);
    all.push(hit);
  }
  if (!query) return [...cli, ...names.filter((hit) => !hit.path.replace(/\/$/, "").includes("/"))].filter((hit, at, list) => list.findIndex((each) => each.path === hit.path) === at).slice(0, limit);

  const wanted = fold(query);
  const tight = all
    .flatMap((hit) => {
      const path = fold(hit.path.replace(/\/$/, ""));
      const at = path.indexOf(wanted);
      if (at < 0) return [];
      const name = path.slice(path.lastIndexOf("/") + 1);
      const stem = name.replace(/\.[^.]*$/, "");
      const rank = name === wanted || stem === wanted ? 0 : name.startsWith(wanted) ? 1 : name.includes(wanted) ? 2 : 3;
      return [{ hit, rank }];
    })
    .sort((a, b) => a.rank - b.rank || a.hit.path.length - b.hit.path.length || a.hit.path.localeCompare(b.hit.path))
    .map((each) => each.hit);
  if (tight.length) return tight.slice(0, limit);

  const cliPaths = new Set(cli.map((hit) => hit.path));
  const loose = names
    .filter((hit) => !cliPaths.has(hit.path))
    .flatMap((hit) => {
      const match = fuzzy(hit.path, query);
      return match ? [{ hit, score: match.score }] : [];
    })
    .sort((a, b) => b.score - a.score || a.hit.path.length - b.hit.path.length)
    .map((each) => each.hit);
  return [...cli, ...loose].slice(0, limit);
}
