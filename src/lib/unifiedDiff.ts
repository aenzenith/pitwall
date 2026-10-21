// `git diff` read for the Files window: the lines of its hunks for the diff view, and the marks
// of the file's own lines for the gutter (added, changed, or lines removed just above).

export type DiffLine = { kind: "hunk" | "context" | "add" | "del"; text: string; old: number | null; new: number | null };
export type GutterMark = "add" | "mod" | "del";

export type ParsedDiff = { lines: DiffLine[]; gutter: Map<number, GutterMark>; added: number; removed: number };

export function parseDiff(diff: string): ParsedDiff {
  const lines: DiffLine[] = [];
  const gutter = new Map<number, GutterMark>();
  let added = 0;
  let removed = 0;
  let old = 0;
  let now = 0;
  let inHunk = false;
  // The run of removed and added lines being read: closed by a context line or a new hunk.
  let dels = 0;
  let adds: number[] = [];

  const close = (): void => {
    if (adds.length) adds.forEach((line, i) => gutter.set(line, i < dels ? "mod" : "add"));
    else if (dels) gutter.set(Math.max(now, 1), gutter.get(Math.max(now, 1)) ?? "del");
    dels = 0;
    adds = [];
  };

  for (const raw of diff.split("\n")) {
    const head = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw);
    if (head) {
      close();
      old = Number(head[1]);
      now = Number(head[2]);
      inHunk = true;
      lines.push({ kind: "hunk", text: raw, old: null, new: null });
      continue;
    }
    if (!inHunk) continue;
    const sign = raw[0];
    const text = raw.slice(1);
    if (sign === "+") {
      adds.push(now);
      lines.push({ kind: "add", text, old: null, new: now++ });
      added++;
    } else if (sign === "-") {
      dels++;
      lines.push({ kind: "del", text, old: old++, new: null });
      removed++;
    } else if (sign === " ") {
      close();
      lines.push({ kind: "context", text, old: old++, new: now++ });
    }
    // "\ No newline at end of file" and anything else: not a line of the file.
  }
  close();
  return { lines, gutter, added, removed };
}
