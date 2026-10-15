// The output pane's lines. What the core sends is text; each line is read once, as it arrives:
// a marker of the core's own, an error and the lines that go with it, the file a line names.
// Nothing here is worked out again when the pane is drawn.

import ISSUES from "./errorLines.json";

/* ---------- markers ---------- */

/** What the core writes into an output itself (core/output.rs): a run ended or began, or Claude
 * read the output or did that to the server. */
export type MarkerKind = "ended" | "started" | "claude-read" | "claude-start" | "claude-stop" | "claude-restart";

/** `detail`: for `ended`, how (`crashed:<code>`, `exited:<code>`, `stopped`; the code may be empty). */
export type Marker = { kind: MarkerKind; at: number; detail: string };

const MARK = "\u001e";
const MARKER_KINDS: ReadonlySet<string> = new Set<MarkerKind>(["ended", "started", "claude-read", "claude-start", "claude-stop", "claude-restart"]);

/** The marker a line is; `null` for a line of output, `"unknown"` for a marker of a kind this
 * version doesn't know (it draws nothing). */
export function readMarker(text: string): Marker | "unknown" | null {
  if (!text.startsWith(MARK)) return null;
  const [kind, at, ...detail] = text.slice(1).split("\t");
  if (!MARKER_KINDS.has(kind)) return "unknown";
  return { kind: kind as MarkerKind, at: Number(at) || 0, detail: detail.join("\t") };
}

/** How a run ended, from its `ended` marker: `code` null when none was given. */
export function runEnd(marker: Marker): { how: "crashed" | "exited" | "stopped" | null; code: string | null } {
  const [how, code] = marker.detail.split(":");
  if (how !== "crashed" && how !== "exited" && how !== "stopped") return { how: null, code: null };
  return { how, code: code || null };
}

/* ---------- errors ---------- */

/** `error`: an error's first line. `trail`: a line that goes on with the error above it (its
 * stack, the code it points at). */
export type Level = "error" | "trail" | null;

/** What a line is, is read off its first characters: an error is named where its line begins,
 * and a file named further in than this is not looked for. It keeps the work on a line small
 * however long the line is (a bundle printed on one, an image as text). */
const HEAD = 2048;

/** What the core takes for an error too, when it names a project's issue (resolve.rs). */
const SHARED = ISSUES.map((pattern) => new RegExp(pattern, "i"));

/** Says there is none: `0 errors`, `no errors found`, `error: 0`, a dumped `error: null,`. */
const NONE = /\b(?:0|no)\s+errors?\b|\berrors?\b["']?:?\s*(?:0|null|undefined|false|none|\[\]|''|"")\s*(?:$|[,;.)\]}])/i;

/** A warning or a note that only talks about errors: `warning: …`, `[WARN] …`, `12:00:01 INFO …`,
 * `[vite] warning: …`, `[2026-10-13 12:00:00] local.WARNING: …`. Before the word: a clock or a
 * date (it ends in a digit, so what follows it is read once), a tag's end, a channel. */
const LESSER = /^\W*(?:\d(?:[\d:.\-T ]*[\dT])?\W+)?(?:\w[\w:-]{0,40}\]\W*)?(?:\w{1,40}\.)?(?:warn(?:ing)?|notice|info|debug|deprecat\w*)\b/i;

const ERRORS = [
  // `error: …`, `error TS2322: …`, `error[E0308]: …`, `PHP Fatal error: …`, `local.ERROR: …`
  /\berror(?:\s+TS\d+|\[E\d+\])?:/i,
  // Laravel's levels above that one: `production.CRITICAL: …`
  /\.(?:CRITICAL|ALERT|EMERGENCY):/,
  // An error or an exception by its class: `RangeError: …`, `PDOException: …`, a line that ends
  // in one. Read from where the name begins (`java.lang.` and `App\Exceptions\` before the
  // class), never from inside it: a long word is gone through once.
  /(?:^|[^\w\\.])(?:[\w\\.]*[\\.])?(?:[A-Z]\w*)?(?:Error|Exception)(?=:|\s*$)/,
  // A level standing on its own: `ERROR in ./src/a.ts`, `[ERROR]`, ` FAIL  src/a.test.ts`.
  /(?:^|[\s[(])(?:ERROR|FATAL|CRITICAL|FAIL(?:ED)?)(?=[\s\])]|$)/,
  /\b(?:failed to \w|cannot find\b|command not found\b|command failed\b|unhandled|uncaught|panicked at\b|segmentation fault\b|npm error\b)/i,
];

/** Python's errors begin here and are named on their last line. */
const TRACEBACK = /^Traceback \(most recent call last\)/;

/** npm reports one failure over many lines, each under the same word. */
const NPM = /^npm (?:ERR!|error)/i;

/** webpack reports an error over the lines from `ERROR in …` to the next empty one, and a
 * warning the same way: whatever those say (`Module not found: Error: …`, `Failed to parse
 * source map …`), they are that one error, or none. */
const WEBPACK = /^ERROR in\b/;
const WEBPACK_WARNING = /^(?:WARNING in|Module Warning)\b/;

/** Goes on with the error above it without being indented under it: a code frame's gutter, a
 * stack frame, a line that only leads to those. */
const FOLLOWS = /^(?:>\s*(?:\d+\s*)?|\d+\s*|\s*)\||^#\d+\s|^(?:Require stack:|Caused by:|Stack trace:|\[stacktrace\]|- \S)/;

/** Indented under an error and its, whatever it says: a stack frame, a line of the code the
 * error points at (`    at new TypeError (…)`, `  12 |   throw new TypeError("x")`). */
const FRAME = /^\s*(?:at\s|(?:>\s*)?(?:\d+\s*)?[|│▕])/;

/** Says how things stand, not what went wrong: under an error, its lines end here. Next's and
 * Vitest's ticks and circles, a line of a log (it begins with its clock or date, or with
 * artisan's `INFO`), cargo's steps. */
const STATUS = /^\s*(?:[✓✔○⚠]|\[?(?:\d{1,2}:\d{2}|\d{4}-\d{2}-\d{2})\b|(?:INFO|WARN|NOTICE|DEBUG)\s|(?:Compiling|Checking|Finished|Running|Fresh|Building|Downloading|Downloaded|Updating|Locking)\s)/;

/** Whether a line says an error happened, in the forms dev servers print: Vite, webpack, Next,
 * tsc, Node, Laravel and PHP, Python, cargo. A count of none and a warning are not one. */
export function isError(line: string): boolean {
  const text = line.length > HEAD ? line.slice(0, HEAD) : line;
  if (SHARED.some((pattern) => pattern.test(text))) return true;
  if (NONE.test(text) || LESSER.test(text)) return false;
  return TRACEBACK.test(text) || ERRORS.some((pattern) => pattern.test(text));
}

/**
 * Tells each line's level, in the order the lines come: whether a line belongs to an error
 * depends on the one before it. One per output; a marker in between isn't given to it.
 */
export function levels(): (text: string) => Level {
  /** The error whose lines may still go on, and how far its first line is indented. `traceback`:
   * Python's, still to be named. `npm`: npm's report. `webpack`: its block. */
  let open: { depth: number; kind: "error" | "traceback" | "npm" | "webpack" } | null = null;
  /** Inside a warning of webpack's. */
  let warning = false;

  return (line) => {
    const text = line.length > HEAD ? line.slice(0, HEAD) : line;
    // Where the line's text begins; -1 for an empty line, which ends an error.
    const depth = text.search(/\S/);

    warning = depth >= 0 && !WEBPACK.test(text) && (warning || WEBPACK_WARNING.test(text));
    if (warning) {
      open = null;
      return null;
    }

    const error = isError(text);
    if (open) {
      if (open.kind === "webpack" ? depth >= 0 && !WEBPACK.test(text) : FOLLOWS.test(text) || (open.kind === "npm" && NPM.test(text))) return "trail";
      // Deeper than the error's own line: a server that indents everything goes on with nothing.
      // Not a line that is an error itself (Next puts each of its own a step in) nor one that
      // says how things stand: those are not this error's.
      if (depth > open.depth && (open.kind === "traceback" || FRAME.test(text) || !(error || STATUS.test(text)))) return "trail";
      if (open.kind === "traceback" && depth >= 0) {
        open = null;
        return "trail";
      }
    }
    if (!error) {
      open = null;
      return null;
    }
    open = { depth, kind: TRACEBACK.test(text) ? "traceback" : NPM.test(text) ? "npm" : WEBPACK.test(text) ? "webpack" : "error" };
    return "error";
  };
}

/* ---------- file references ---------- */

/** The file a line names, as printed, and the place in it; `start`–`end`: where the line says so. */
export type FileRef = { file: string; line: number; column: number | null; start: number; end: number };

/** How a file's name ends: the last of it, a dot, an extension. */
const NAME_END = String.raw`[\w\-\])]\.[A-Za-z][A-Za-z0-9]{0,7}`;

/** Where a line names a place in a file: by the end of the file's name and what stands after
 * it. The path before that is read from there backwards (`pathStart`); looked for from its
 * front, every character of a long word would be a place to begin. `path`: what the name is
 * part of, `relative` for a path that must begin with `./` or `../`, `quoted` for none (the
 * name stands whole between quotes). */
const PLACES: { place: RegExp; path: "any" | "relative" | "quoted" }[] = [
  // Checkout.vue:42:11 · at fn (/abs/file.js:10:3) · --> src/main.rs:4:18
  { place: new RegExp(String.raw`(${NAME_END}):(\d+)(?::(\d+))?`, "g"), path: "any" },
  // ./src/a.ts(12,5) · #0 /app/Http/Kernel.php(12)
  { place: new RegExp(String.raw`(${NAME_END})\((\d+)(?:,(\d+))?\)`, "g"), path: "any" },
  // File "x.py", line 3
  { place: /File "([^"]+)", line (\d+)()/g, path: "quoted" },
  // in /app/routes/web.php on line 12
  { place: new RegExp(String.raw`(${NAME_END}) on line (\d+)()`, "g"), path: "any" },
  // ERROR in ./src/index.js 3:0-24
  { place: new RegExp(String.raw`(${NAME_END}) (\d+):(\d+)`, "g"), path: "relative" },
];

/** No path is longer. */
const PATH_MAX = 512;

/** A path read backwards, so that it is gone through once: `Checkout.vue`, `./src/a.ts`,
 * `C:\app\a.ts` (the drive apart), a folder in brackets as a whole (`app/(shop)/[id]/page.tsx`,
 * here `)pohs(`). No blanks: a path with one is found from its last part on. */
const PATH_BACKWARDS = /^(?:[\w@.~+\-[\]/\\]|\)[\w.-]+\()*/;

/** Where the path that ends at `end` begins. `relative`: at its first `./` or `../`, -1 for a
 * path without one. */
function pathStart(text: string, end: number, relative: boolean): number {
  const backwards = text.slice(Math.max(0, end - PATH_MAX), end).split("").reverse().join("");
  const start = end - (PATH_BACKWARDS.exec(backwards)?.[0].length ?? 0);
  if (relative) {
    const dots = text.slice(start, end).search(/\.{1,2}\//);
    return dots < 0 ? -1 : start + dots;
  }
  // C:\app\a.ts
  return /\b[A-Za-z]:$/.test(text.slice(Math.max(0, start - 3), start)) && /[\\/]/.test(text.charAt(start)) ? start - 2 : start;
}

/** An address's scheme, when the word a match sits in is one: `https://shop.test:8443`. */
const SCHEME = /^([a-z][a-z0-9+.-]*):\/\//i;

/** The first file a line names with a place in it, or null. An address is no file
 * (`http://localhost:5173`, `shop.test:8443` in one), unless it is a `file://` one. */
export function findRef(line: string): FileRef | null {
  const text = line.length > HEAD ? line.slice(0, HEAD) : line;
  // Every form has a dot and a number in it.
  if (!text.includes(".") || !/\d/.test(text)) return null;

  let first: FileRef | null = null;
  for (const { place, path } of PLACES) {
    place.lastIndex = 0;
    for (let found = place.exec(text); found; found = place.exec(text)) {
      const end = found.index + found[0].length;
      // At the very end of a line that was cut, a place may have lost its last digits.
      if (end === text.length && text !== line) break;

      // The dot of the name's end is one character in.
      const start = path === "quoted" ? found.index : pathStart(text, found.index + 1, path === "relative");
      if (start < 0) continue;
      if (first && start >= first.start) break;

      const word = text.slice(Math.max(text.lastIndexOf(" ", start), text.lastIndexOf("(", start), text.lastIndexOf('"', start)) + 1, end);
      const scheme = SCHEME.exec(word)?.[1].toLowerCase();
      if (scheme && scheme !== "file") {
        // An address holds no file, up to where it ends.
        const blank = text.indexOf(" ", end);
        if (blank < 0) break;
        place.lastIndex = blank;
        continue;
      }

      // file:///Users/x/a.js: the path alone.
      const name = path === "quoted" ? found[1] : text.slice(start, found.index + found[1].length);
      const file = scheme ? name.replace(/^\/+/, "/") : name;
      first = { file, line: Number(found[2]), column: found[3] ? Number(found[3]) : null, start, end };
      break;
    }
  }
  return first;
}

/* ---------- a line ---------- */

/** An output line; `id` only grows, so a line keeps its key as older ones scroll out. */
export type OutputLine = {
  id: number;
  text: string;
  /** `cmd`: the command line a run began with. `sys`: a line Pitwall wrote. */
  kind: "cmd" | "sys" | null;
  /** Set for a line the core wrote as a marker: drawn as a rule, never as text. */
  marker: Marker | null;
  level: Level;
  ref: FileRef | null;
};

/**
 * Reads an output's lines in the order they come. `null` for a line that shows nothing (a
 * marker of a kind this version doesn't know). One reader per output: an error's lines are
 * told by the line before them.
 */
export function lineReader(): (text: string, id: number) => OutputLine | null {
  const level = levels();

  return (text, id) => {
    const marker = readMarker(text);
    if (marker === "unknown") return null;
    if (marker) return { id, text: "", kind: null, marker, level: null, ref: null };

    const kind = text.startsWith("$ ") ? "cmd" : text.startsWith("[pitwall]") ? "sys" : null;
    // Pitwall's own lines are never an error of the project's: to the reader they are an empty
    // line, which ends one.
    return { id, text, kind, marker: null, level: level(kind ? "" : text), ref: kind ? null : findRef(text) };
  };
}

/* ---------- the pane's view ---------- */

/**
 * The markers of one moment as they are drawn. The core writes each thing down as it happens:
 * Claude restarts the server, and the output holds Claude's marker, Pitwall's line that the
 * process ended, how the run ended and that the next one began. Drawn as written, that is one
 * thing said three times. So, of markers with nothing but Pitwall's own lines between them:
 *
 * - a start or restart by Claude is one rule, Claude's, where the new run begins;
 * - after a stop by Claude, how the run ended goes unsaid (the next start is "started again").
 *
 * `moment`: those markers and the lines between them, in the order they came.
 */
function oneMoment(moment: OutputLine[]): OutputLine[] {
  let byClaude: OutputLine | null = null;
  let started: OutputLine | null = null;
  let stopped = false;
  for (const line of moment) {
    const kind = line.marker?.kind;
    if (kind === "claude-start" || kind === "claude-restart") byClaude = line;
    else if (kind === "started") started = line;
    else if (kind === "claude-stop") stopped = true;
  }

  const drawn: OutputLine[] = [];
  for (const line of moment) {
    const kind = line.marker?.kind;
    if (byClaude && started) {
      if (line === started) drawn.push(byClaude);
      if (line === started || line === byClaude || kind === "ended") continue;
    } else if (stopped && kind === "ended") continue;
    drawn.push(line);
  }
  return drawn;
}

/**
 * The lines the pane shows: all of them, or with `errorsOnly` the errors with the lines that
 * belong to them. Markers stay either way, as few as say it (`oneMoment`): of Claude's reads
 * the newest alone, and of several of one kind in a row the last one stands for all.
 */
export function shownLines(lines: OutputLine[], errorsOnly: boolean): OutputLine[] {
  let read = lines.length - 1;
  while (read >= 0 && lines[read].marker?.kind !== "claude-read") read--;

  const shown: OutputLine[] = [];
  for (let at = 0; at < lines.length; at++) {
    let moment = [lines[at]];
    if (lines[at].marker) {
      // On to its last marker, over Pitwall's own lines.
      let last = at;
      for (let next = at + 1; next < lines.length && (lines[next].marker || lines[next].kind === "sys"); next++) if (lines[next].marker) last = next;
      moment = oneMoment(lines.slice(at, last + 1).filter((line) => line.marker?.kind !== "claude-read" || line === lines[read]));
      at = last;
    }

    for (const line of moment) {
      if (errorsOnly && !line.marker && !line.level) continue;
      if (line.marker && shown[shown.length - 1]?.marker?.kind === line.marker.kind) shown.pop();
      shown.push(line);
    }
  }
  return shown;
}

/** The lines as text, as the pane shows them: a marker isn't output. */
export function plainText(lines: OutputLine[]): string {
  return lines
    .filter((line) => !line.marker)
    .map((line) => line.text)
    .join("\n");
}

/* ---------- search ---------- */

/** A place in a line's text: `[start, end)`. */
export type Range = [start: number, end: number];

/** Finds `query` in a text as it is typed, whatever its case; null for no query. The places
 * are in the text itself, which lower-casing it could shift (`İ`). */
export function finder(query: string): ((text: string) => Range[]) | null {
  if (!query) return null;
  const pattern = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
  return (text) => Array.from(text.matchAll(pattern), (found) => [found.index, found.index + found[0].length] as Range);
}

/** A stretch of a line as it is drawn: `ref` inside the file it names, `hit` the number of the
 * search match it is in (in the line's order), -1 outside one. */
export type Segment = { text: string; ref: boolean; hit: number };

/** A line's text cut where its file reference and the search's matches begin and end. */
export function segments(line: OutputLine, hits: Range[]): Segment[] {
  const cuts = new Set<number>([0, line.text.length]);
  if (line.ref) cuts.add(line.ref.start).add(line.ref.end);
  for (const [start, end] of hits) cuts.add(start).add(end);
  const at = [...cuts].sort((a, b) => a - b);

  const parts: Segment[] = [];
  for (let i = 0; i + 1 < at.length; i++) {
    const [start, end] = [at[i], at[i + 1]];
    parts.push({
      text: line.text.slice(start, end),
      ref: !!line.ref && start >= line.ref.start && end <= line.ref.end,
      hit: hits.findIndex(([from, to]) => start >= from && end <= to),
    });
  }
  return parts;
}

/* ---------- from an error to a card ---------- */

/** The error the line at `at` is part of: its first line and those that go on with it. Empty
 * when the line belongs to none. */
export function errorBlock(lines: OutputLine[], at: number): OutputLine[] {
  const part = (line: OutputLine | undefined): boolean => !!line && (!!line.marker || line.level === "trail");
  if (!lines[at]?.level) return [];

  let first = at;
  while (lines[first].level !== "error" && part(lines[first - 1])) first--;
  if (lines[first].level !== "error" && lines[first - 1]?.level === "error") first--;
  let last = at;
  while (part(lines[last + 1])) last++;

  return lines.slice(first, last + 1).filter((line) => !line.marker);
}

const TITLE_MAX = 120;
const NOTE_LINES_MAX = 60;
const NOTE_MAX = 6000;

/** `text` cut to `max` code units, never between the two halves of a character outside the basic
 * plane (an emoji): half of one is no text, and the core refuses a card that holds it. */
function cutText(text: string, max: number): string {
  const part = text.slice(0, max);
  return /[\uD800-\uDBFF]$/.test(part) ? part.slice(0, -1) : part;
}

/** A card for an error: the title its first line (the last one of a Python traceback, where the
 * error is named), without the clock a server puts before it; the note its lines as a code block. */
export function cardDraft(block: OutputLine[]): { title: string; note: string } {
  if (!block.length) return { title: "", note: "" };
  const named = TRACEBACK.test(block[0].text) ? block[block.length - 1] : block[0];
  const said = named.text.replace(/^\s*\d{1,2}:\d{2}(?::\d{2})?(?:\s*[AP]M)?\s+/i, "").replace(/\s+/g, " ").trim();
  const title = said.length > TITLE_MAX ? `${cutText(said, TITLE_MAX - 1)}…` : said;

  const whole = block.slice(0, NOTE_LINES_MAX).map((line) => line.text.trimEnd()).join("\n");
  const cut = block.length > NOTE_LINES_MAX || whole.length > NOTE_MAX;
  const text = cut ? `${cutText(whole, NOTE_MAX)}\n…` : whole;
  const fence = text.includes("```") ? "````" : "```";
  return { title, note: `${fence}\n${text}\n${fence}` };
}
