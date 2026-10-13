import { describe, expect, it } from "vitest";

import { cardDraft, errorBlock, findRef, isError, lineReader, readMarker, shownLines, type OutputLine } from "./outputLines";

function read(texts: string[]): OutputLine[] {
  const reader = lineReader();
  return texts.map((text, i) => reader(text, i + 1)).filter((line): line is OutputLine => !!line);
}

// The error filter hides every line this says no to, and counts every one it says yes to: the
// forms below are what the servers really print, each written down once.
describe("isError", () => {
  it.each([
    // What the core names a project's issue by (errorLines.json).
    "Error: Cannot find module 'vite'",
    "17:29:51 [vite] Internal server error: Failed to resolve import \"./SipayForm.vue\" from \"resources/js/Pages/Checkout.vue\". Does the file exist?",
    "[vite] error while updating dependencies:",
    "Module not found: Can't resolve './missing'",
    "ReferenceError: foo is not defined",
    " ELIFECYCLE  Command failed with exit code 1.",
    "npm ERR! code ELIFECYCLE",
    // tsc, cargo, PHP, Laravel, webpack, test runners, the shell.
    "src/a.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.",
    "error[E0308]: mismatched types",
    "error: could not compile `paddock` (bin \"paddock\") due to 1 previous error",
    "thread 'main' panicked at src/main.rs:4:5:",
    "PHP Fatal error:  Uncaught Exception: boom in /app/routes/web.php:12",
    "[2026-10-13 12:00:00] local.ERROR: SQLSTATE[42S02]: Base table or view not found",
    "   Illuminate\\Database\\QueryException ",
    "ERROR in ./src/index.js 3:0-24",
    " ERROR  Failed to compile with 1 error",
    "✘ [ERROR] Could not resolve \"./missing\"",
    " FAIL  src/a.test.ts > adds",
    "  17:18:40 App\\Jobs\\DeliverWebhook ...................... 96.02ms FAIL",
    "Traceback (most recent call last):",
    "java.lang.NullPointerException: Cannot invoke \"String.length()\"",
    "(node:4242) UnhandledPromiseRejectionWarning: Error: boom",
    "sh: vite: command not found",
    "npm error Missing script: \"dev\"",
    "[vite] http proxy error: /api/users",
    "12:03:00 PM [vite] (client) Pre-transform error: Failed to parse source for import analysis because the content contains invalid JS syntax.",
    "[2026-10-13 12:00:07] production.CRITICAL: Disk full",
    "error Command failed with exit code 1.",
  ])("an error: %s", (line) => {
    expect(isError(line)).toBe(true);
  });

  it.each([
    "Found 0 errors. Watching for file changes.",
    "0 errors, 2 warnings",
    "error: 0",
    "Errors: 0",
    "No errors found",
    "  error: null,",
    "✖ 3 problems (3 errors, 0 warnings)",
    "warning: unused variable: `error`",
    "[esbuild] warning: an error: handler is missing",
    "12:00:01 WARN  retrying after error: timeout",
    "[2026-10-13 12:00:00] local.WARNING: error: cache miss",
    "(node:4242) Warning: Accessing non-existent property 'x' of module exports",
    "Could not resolve \"@/lib/tax\" (optional)",
    "17:29:51 [vite] hmr update /resources/js/components/ErrorPage.vue",
    "12:01:09 PM [vite] (client) hmr update /src/pages/Error.vue",
    "12:01:15 PM [vite] (client) hmr update /src/pages/error.vue, /src/lib/errors.ts",
    "12:01:10 PM [vite] (client) page reload src/components/ErrorBoundary.vue",
    "[vite] ✓ no errors",
    " ✓ throws on a bad lap (3 ms)",
    "  throw new Error(\"boom\");",
    "   INFO  Processing jobs from the [default] queue.",
    "  ➜  Local:   http://localhost:5173/",
    "stderr: closed",
  ])("no error: %s", (line) => {
    expect(isError(line)).toBe(false);
  });
});

describe("an error's lines", () => {
  const levels = (texts: string[]) => read(texts).map((line) => line.level);

  it("Node: the stack under the error, to the first line that isn't part of it", () => {
    expect(
      levels(["Error: Cannot find module 'vite'", "Require stack:", "- /app/crash.js", "    at Module._resolveFilename (node:internal/modules/cjs/loader:1145:15)", "  code: 'MODULE_NOT_FOUND'", "}", "", "Node.js v20.11.0"]),
    ).toEqual(["error", "trail", "trail", "trail", "trail", null, null, null]);
  });

  it("cargo: the code frame, and the next error is its own", () => {
    expect(levels(["error[E0308]: mismatched types", " --> src/main.rs:4:18", "  |", "4 |     let x: i32 = \"a\";", "  |            ^^^ expected `i32`", "", "error: aborting due to 1 previous error"])).toEqual([
      "error",
      "trail",
      "trail",
      "trail",
      "trail",
      null,
      "error",
    ]);
  });

  it("Python: one error from the traceback to the line that names it", () => {
    expect(levels(["Traceback (most recent call last):", '  File "x.py", line 3, in <module>', "    foo()", "NameError: name 'foo' is not defined", "done"])).toEqual(["error", "trail", "trail", "trail", null]);
  });

  it("npm: one error however many lines it reports it on", () => {
    expect(levels(["npm error code ELIFECYCLE", "npm error errno 1", "npm error command failed", "> paddock@1.0.0 dev"])).toEqual(["error", "trail", "trail", null]);
  });

  it("a server that indents every line: the lines after a failure are not its", () => {
    expect(levels(["  17:18:40 App\\Jobs\\DeliverWebhook ... 96.02ms FAIL", "  17:18:41 App\\Jobs\\ImportLapTimes ... RUNNING", "  17:18:42 App\\Jobs\\ImportLapTimes ... 12ms DONE"])).toEqual(["error", null, null]);
  });

  it("Next, which puts its own lines a step in: each error is one, and a line that says how things stand is none's", () => {
    expect(levels(["Failed to compile.", " ⨯ Error: Hydration failed because the server rendered HTML didn't match the client.", " ⨯ unhandledRejection: Error: boom", "    at app/api/route.ts:4:9", " ✓ Compiled in 300ms (212 modules)"])).toEqual([
      "error",
      "error",
      "error",
      "trail",
      null,
    ]);
    expect(levels(["Failed to compile.", " ✓ Compiled in 300ms (212 modules)", " GET / 200 in 12ms"])).toEqual(["error", null, null]);
  });

  it("cargo: its steps after a failed build are not the error's", () => {
    expect(levels(['error: could not compile `app` (bin "app") due to 1 previous error; 1 warning emitted', "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.52s", "     Running `target/debug/app`"])).toEqual(["error", null, null]);
  });

  it("Laravel: an error under a failed line is its own, and a line of the log ends an error", () => {
    expect(levels(["  2026_10_02_000000_add_failed_jobs_table ........................... 4ms FAIL", '   ERROR  Command "migrate:frsh" is not defined.'])).toEqual(["error", "error"]);
    expect(levels(["[2026-10-13 12:00:07] production.CRITICAL: Disk full", "   INFO  Running migrations.", "  2026_10_01_000000_create_errors_table ............................. 12ms DONE"])).toEqual(["error", null, null]);
    expect(levels(["PHP Fatal error:  Uncaught Error: Call to undefined function foo() in /app/routes/web.php:12", "  thrown in /app/routes/web.php on line 12", "  2026-10-13 12:00:05 /api/users ........ ~ 0.50ms"])).toEqual(["error", "trail", null]);
  });

  it("a stack frame or a line of code that names an error is still the error's above it", () => {
    expect(levels(["TypeError: x is not a function", "    at new TypeError (<anonymous>)", '  12 |   throw new TypeError("x");', "> 13 |   new ReferenceError()"])).toEqual(["error", "trail", "trail", "trail"]);
  });

  it("webpack: an error is one from `ERROR in` to the next empty line, and a warning's lines are none", () => {
    expect(
      levels(["ERROR in ./src/index.js 3:0-24", "Module not found: Error: Can't resolve './missing' in '/app/src'", "resolve './missing' in '/app/src'", "", "ERROR in ./src/app.ts:12:5", "TS2322: Type 'string' is not assignable to type 'number'.", "ERROR in ./src/b.ts:1:1"]),
    ).toEqual(["error", "trail", "trail", null, "error", "trail", "error"]);
    expect(
      levels(["WARNING in ./src/big.js", "Module Warning (from ./node_modules/source-map-loader/dist/cjs.js):", "Failed to parse source map from '/app/node_modules/x/x.js.map' file: Error: ENOENT: no such file or directory", "", "Module not found: Can't resolve './missing'"]),
    ).toEqual([null, null, null, null, "error"]);
  });

  it("Pitwall's own lines and markers are never errors, and a marker doesn't cut an error in two", () => {
    const lines = read(["TypeError: x is not a function", "\u001eclaude-read\t1\t", "    at run (/app/a.js:3:1)", "[pitwall] process ended (code 1, signal -)", "    not part of it"]);
    expect(lines.map((line) => line.level)).toEqual(["error", null, "trail", null, null]);
    expect(errorBlock(lines, 2).map((line) => line.id)).toEqual([1, 3]);
  });
});

describe("findRef", () => {
  it.each([
    ['  "./SipayForm.vue" from Checkout.vue:42:11', "Checkout.vue", 42, 11],
    ["src/a.ts(12,5): error TS2322: Type 'string'", "src/a.ts", 12, 5],
    ["./src/a.ts(12,5)", "./src/a.ts", 12, 5],
    ["    at render (/Users/ada/paddock/src/file.js:10:3)", "/Users/ada/paddock/src/file.js", 10, 3],
    ["    at load (file:///Users/ada/paddock/node_modules/vite/dist/dep-C3az.js:42499:41)", "/Users/ada/paddock/node_modules/vite/dist/dep-C3az.js", 42499, 41],
    ['  File "x.py", line 3, in <module>', "x.py", 3, null],
    [" --> src/main.rs:4:18", "src/main.rs", 4, 18],
    ["  File: /Users/ada/paddock/resources/js/Pages/Checkout.vue:42:11", "/Users/ada/paddock/resources/js/Pages/Checkout.vue", 42, 11],
    ["PHP Fatal error:  Uncaught Exception: boom in /app/routes/web.php:12", "/app/routes/web.php", 12, null],
    ["  thrown in /app/routes/web.php on line 12", "/app/routes/web.php", 12, null],
    ["#0 /app/Http/Kernel.php(142): handle()", "/app/Http/Kernel.php", 142, null],
    ["ERROR in ./src/index.js 3:0-24", "./src/index.js", 3, 0],
    [" ⨯ ./app/(shop)/[id]/page.tsx:5:1", "./app/(shop)/[id]/page.tsx", 5, 1],
    ["at C:\\app\\src\\a.ts:7:2", "C:\\app\\src\\a.ts", 7, 2],
    ["see http://localhost:5173/ then src/b.ts:9", "src/b.ts", 9, null],
  ])("%s", (text, file, line, column) => {
    const ref = findRef(text);
    expect(ref && [ref.file, ref.line, ref.column]).toEqual([file, line, column]);
    expect(text.slice(ref!.start, ref!.end)).toContain(file.replace(/^\//, ""));
  });

  it.each([
    "  ➜  Local:   http://localhost:5173/",
    "  ➜  APP_URL: https://telemetry-api.test:8443/",
    "GET http://localhost:5173/src/App.vue:3:1 200",
    "    at Module._load (node:internal/modules/cjs/loader:986:27)",
    "17:29:51 [vite] hmr update /resources/js/Pages/Cart.vue",
    "  17:02:12 App\\Jobs\\ImportLapTimes ..................... 842.19ms DONE",
    " ✓ resources/js/lib/sectors.test.ts (18 tests) 12ms",
    "listening on 192.168.1.5:3000",
  ])("no file: %s", (text) => {
    expect(findRef(text)).toBeNull();
  });
});

describe("markers", () => {
  it("reads the core's own, and draws nothing for a kind it doesn't know", () => {
    expect(readMarker("\u001eended\t1760352480000\tcrashed:1")).toEqual({ kind: "ended", at: 1760352480000, detail: "crashed:1" });
    expect(readMarker("\u001estarted\t1760352483000\t")).toEqual({ kind: "started", at: 1760352483000, detail: "" });
    expect(readMarker("\u001eclaude-dance\t1\t")).toBe("unknown");
    expect(readMarker("plain")).toBeNull();
    expect(read(["a", "\u001eclaude-dance\t1\t", "b"]).map((line) => line.text)).toEqual(["a", "b"]);
  });

  // The rings as the core writes them, one marker per thing that happened.
  const mark = (kind: string, at: number, detail = "") => `\u001e${kind}\t${at}\t${detail}`;
  const drawn = (texts: string[], errorsOnly = false) => shownLines(read(texts), errorsOnly).map((line) => (line.marker ? `${line.marker.kind} ${line.marker.at}` : line.text));
  const ENDED = "[pitwall] process ended (code -, signal 15)";

  it("a start or restart by Claude is one rule, where the new run begins", () => {
    expect(drawn([mark("claude-start", 1), mark("started", 1), "$ npm run dev"])).toEqual(["claude-start 1", "$ npm run dev"]);
    expect(drawn(["ready", mark("claude-restart", 2), ENDED, mark("ended", 3, "stopped"), mark("started", 3), "$ npm run dev"])).toEqual(["ready", ENDED, "claude-restart 2", "$ npm run dev"]);
    expect(drawn(["ready", ENDED, "[pitwall] gave up", mark("claude-start", 5), mark("ended", 3, "crashed:1"), mark("started", 5), "$ npm run dev"])).toEqual(["ready", ENDED, "[pitwall] gave up", "claude-start 5", "$ npm run dev"]);
  });

  it("a stop by Claude is its rule alone; the start after it says who started", () => {
    expect(drawn(["ready", mark("claude-stop", 2), ENDED, mark("ended", 3, "stopped"), mark("started", 9), "$ npm run dev"])).toEqual(["ready", "claude-stop 2", ENDED, "started 9", "$ npm run dev"]);
    expect(drawn(["ready", mark("claude-stop", 2), ENDED, mark("claude-read", 4), mark("claude-start", 6), mark("ended", 3, "stopped"), mark("started", 6), "$ npm run dev"])).toEqual([
      "ready",
      "claude-stop 2",
      ENDED,
      "claude-read 4",
      "claude-start 6",
      "$ npm run dev",
    ]);
  });

  it("a run that ended on its own keeps both of its rules", () => {
    expect(drawn(["Error: boom", "[pitwall] process ended (code 1, signal -)", mark("ended", 3, "crashed:1"), mark("started", 6), "$ npm run dev", mark("claude-read", 7), "ready", mark("claude-stop", 8)])).toEqual([
      "Error: boom",
      "[pitwall] process ended (code 1, signal -)",
      "ended 3",
      "started 6",
      "$ npm run dev",
      "claude-read 7",
      "ready",
      "claude-stop 8",
    ]);
  });

  it("of Claude's reads the newest alone, and the filter keeps the rules", () => {
    const polled = ["Error: a", mark("claude-read", 1), mark("claude-read", 2), "fine", mark("claude-read", 3), "Error: b"];
    expect(drawn(polled)).toEqual(["Error: a", "fine", "claude-read 3", "Error: b"]);
    expect(drawn(polled, true)).toEqual(["Error: a", "claude-read 3", "Error: b"]);
    expect(drawn(["Error: a", mark("claude-restart", 2), ENDED, mark("ended", 3, "stopped"), mark("started", 3), "$ npm run dev", "fine"], true)).toEqual(["Error: a", "claude-restart 2"]);
  });
});

describe("cardDraft", () => {
  it("the error's first line without its clock, and its lines as a code block", () => {
    const lines = read(["17:29:51 [vite] Internal server error: Failed to resolve import", "  File: /app/Checkout.vue:42:11", "ok"]);
    expect(cardDraft(errorBlock(lines, 1))).toEqual({ title: "[vite] Internal server error: Failed to resolve import", note: "```\n17:29:51 [vite] Internal server error: Failed to resolve import\n  File: /app/Checkout.vue:42:11\n```" });
  });

  it("a traceback is named by its last line; a long error is cut", () => {
    const lines = read(["Traceback (most recent call last):", '  File "x.py", line 3, in <module>', "NameError: name 'foo' is not defined"]);
    expect(cardDraft(errorBlock(lines, 0)).title).toBe("NameError: name 'foo' is not defined");

    const long = read([`Error: ${"x".repeat(300)}`, ...Array.from({ length: 80 }, (_, i) => `    at frame${i} (/app/a.js:${i}:1)`)]);
    const draft = cardDraft(errorBlock(long, 0));
    expect(draft.title).toHaveLength(120);
    expect(draft.note.split("\n")).toHaveLength(63);
  });
});

// A line is read on the UI's own thread as it arrives: one that took seconds froze the window.
// The shapes that did, at the longest a line comes (65 536 characters, a line mirrored from a
// VS Code window), alone and under an error whose lines may go on.
describe("a long line", () => {
  const N = 65536;
  const base64 = Array.from({ length: N }, (_, i) => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"[(i * 7 + (i >> 3)) % 64]).join("");

  it.each([
    ["an image as text in a Vite error", `Failed to load url data:image/png;base64,${base64} in /src/App.vue. Does the file exist?`],
    ["letters and digits, a dot at the end", `${"a1".repeat(N / 2)} x.y`],
    ["a path without end", `${"ab/".repeat(N / 3)} x.y 1`],
    ["a path of dotted folders", `${"node_modules/.pnpm/".repeat(N / 19)}x.js:1:1`],
    ["a dotted chain", "A.".repeat(N / 2)],
    ["a digit, then dashes", `1${"-".repeat(N)}x`],
    ["a digit, then blanks", `5${" ".repeat(N)}x`],
    ["a quote mark, then blanks", `> ${" ".repeat(N)}x`],
    ["dots of a progress bar", `${".".repeat(N)} 63 / 1200 (  5%)`],
    ["folders in brackets", `${"(a)".repeat(N / 3)}.ts:1`],
    ["clocks", `${"1:".repeat(N / 2)}${"a".repeat(N / 2)}`],
    ["one tag again and again", "[vite]".repeat(N / 6)],
    ["addresses", "x://a.b(1)a.b:1 ".repeat(N / 16)],
    ["names that end like a file's", "a.b 1:2 ".repeat(N / 8)],
  ])("%s", (_, text) => {
    for (const before of [[], ["Error: boom"]]) {
      const reader = lineReader();
      before.forEach((line, i) => reader(line, i + 1));
      const start = performance.now();
      reader(text, 9);
      expect(performance.now() - start).toBeLessThan(100);
    }
  });

  it("is still read for the error it begins with and the file it names there", () => {
    const line = read([`src/a.ts(12,5): error TS2322: ${"x".repeat(N)} src/b.ts:3:1`])[0];
    expect([line.level, line.ref?.file, line.ref?.line]).toEqual(["error", "src/a.ts", 12]);
  });
});

