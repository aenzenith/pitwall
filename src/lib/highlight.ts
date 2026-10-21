// The page's side of the colouring worker (highlight.worker.ts): one worker, made when the first
// file is shown; a request answered after a newer one for the same view is dropped by the caller.

export type HighlightRequest = { id: number; text: string; lang: string; dark: boolean };
/** `lines`: per line, [length, palette index, font style] triples; null when the language
 * couldn't colour the text (plain text then). `name`: the language as VS Code names it. */
export type HighlightResult = { id: number; name: string | null; palette: string[]; lines: number[][] | null };

let worker: Worker | null = null;
let next = 0;
const waiting = new Map<number, (result: HighlightResult) => void>();

function start(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./highlight.worker.ts", import.meta.url), { type: "module" });
  worker.addEventListener("message", (event: MessageEvent<HighlightResult>) => {
    waiting.get(event.data.id)?.(event.data);
    waiting.delete(event.data.id);
  });
  return worker;
}

/** `text` coloured as `lang`, for the dark or the light theme. */
export function highlight(text: string, lang: string, dark: boolean): Promise<HighlightResult> {
  const id = ++next;
  return new Promise((resolve) => {
    waiting.set(id, resolve);
    start().postMessage({ id, text, lang, dark } satisfies HighlightRequest);
  });
}

/** Font style bits, as Shiki gives them. */
export const ITALIC = 1;
export const BOLD = 2;
export const UNDERLINE = 4;
