// One clock for every answer streaming in. Each draw makes the web view lay the page out and
// paint again; answers streaming side by side, each on a timer of its own, made it do so once
// per answer. On one clock they are drawn in the same turn, and the page is laid out once.

/** How often streaming words are drawn again. */
const STREAM_MS = 90;

const waiting = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | undefined;
let last = 0;

/** `draw` is called at the clock's next beat, once: at once when none came in the last
 * `STREAM_MS`, else that long after the last one. */
export function atStreamBeat(draw: () => void): void {
  waiting.add(draw);
  if (timer !== undefined) return;
  timer = setTimeout(
    () => {
      timer = undefined;
      last = Date.now();
      const all = [...waiting];
      waiting.clear();
      for (const each of all) each();
    },
    Math.max(0, STREAM_MS - (Date.now() - last)),
  );
}

/** `draw` no longer waits for a beat. */
export function leaveStreamBeat(draw: () => void): void {
  waiting.delete(draw);
}
