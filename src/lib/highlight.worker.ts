// Colours a file off the page's thread: Shiki with VS Code's TextMate grammars, each loaded the
// first time a file of its language is shown, and JavaScript's own regular expressions (no
// WebAssembly). What comes back is compact: per line, [length, colour, font style] triples.

import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { bundledLanguages, bundledLanguagesInfo } from "shiki/langs";

import { darkCode, lightCode } from "./codeThemes";
import type { HighlightRequest, HighlightResult } from "./highlight";

let core: Promise<HighlighterCore> | null = null;
const loaded = new Set<string>();

function highlighter(): Promise<HighlighterCore> {
  core ??= createHighlighterCore({ themes: [darkCode, lightCode], langs: [], engine: createJavaScriptRegexEngine({ forgiving: true }) });
  return core;
}

async function load(shiki: HighlighterCore, lang: string): Promise<boolean> {
  if (loaded.has(lang)) return true;
  const grammar = bundledLanguages[lang as keyof typeof bundledLanguages];
  if (!grammar) return false;
  await shiki.loadLanguage(grammar);
  loaded.add(lang);
  return true;
}

const reply = (result: HighlightResult): void => (self as unknown as { postMessage(data: HighlightResult): void }).postMessage(result);

self.addEventListener("message", async (event: MessageEvent<HighlightRequest>) => {
  const { id, text, lang, dark } = event.data;
  const name = bundledLanguagesInfo.find((info) => info.id === lang)?.name ?? null;
  try {
    const shiki = await highlighter();
    if (!(await load(shiki, lang))) return reply({ id, name: null, palette: [], lines: null });
    const tokens = shiki.codeToTokensBase(text, { lang, theme: dark ? darkCode.name : lightCode.name, tokenizeMaxLineLength: 10_000, tokenizeTimeLimit: 300 });
    const palette: string[] = [];
    const index = new Map<string, number>();
    const lines = tokens.map((line) => {
      const out: number[] = [];
      for (const token of line) {
        const colour = token.color ?? "";
        let at = index.get(colour);
        if (at === undefined) {
          at = palette.push(colour) - 1;
          index.set(colour, at);
        }
        out.push(token.content.length, at, token.fontStyle ?? 0);
      }
      return out;
    });
    reply({ id, name, palette, lines });
  } catch {
    reply({ id, name, palette: [], lines: null });
  }
});
