// Which of Seti's icons (VS Code's default file icon theme, lib/setiIcons.ts) the Files window
// draws a file with, picked as VS Code picks it: the file's whole name, then the longest ending
// after one of its dots (`spec.ts` before `ts`), then its language, else the plain file's.
import { languageOf } from "./languages";
import { SETI_FILE, SETI_FILE_EXTENSIONS, SETI_FILE_NAMES, SETI_ICONS, SETI_LANGUAGE_IDS } from "./setiIcons";
import type { ResolvedTheme } from "./theme";

/** languageOf's ids (Shiki's) that VS Code calls otherwise. */
const VSCODE_IDS: Record<string, string> = {
  make: "makefile",
  jsx: "javascriptreact",
  tsx: "typescriptreact",
  coffee: "coffeescript",
  pug: "jade",
  fish: "shellscript",
};

/** Endings VS Code's own languages give another icon than languageOf's would. */
const ENDINGS: Array<[string, string]> = [
  [".gitignore", "ignore"],
  [".gitignore_global", "ignore"],
  [".git-blame-ignore-revs", "ignore"],
  [".editorconfig", "properties"],
  [".conf", "properties"],
  [".cfg", "properties"],
  [".mk", "makefile"],
  [".mak", "makefile"],
  [".code-workspace", "jsonc"],
  [".zshenv", "shellscript"],
  [".zlogin", "shellscript"],
  [".zlogout", "shellscript"],
];

/** Whole names, as above. */
const NAMES: Record<string, string> = {
  ".vscodeignore": "ignore",
  gitconfig: "properties",
  ".envrc": "shellscript",
  "bun.lock": "jsonc",
};

/** VS Code's language for a file's name (lower case): its own where it differs, else languageOf's. */
function languageIdOf(name: string): string | null {
  if (NAMES[name]) return NAMES[name];
  for (const [ending, id] of ENDINGS) if (name.endsWith(ending)) return id;
  // Docker Compose: compose.yml, compose.prod.yaml, docker-compose.yml…
  if (/^compose\.(.+\.)?ya?ml$|docker.*compose.*\.ya?ml$/.test(name)) return "dockercompose";
  const language = languageOf(name);
  return language ? (VSCODE_IDS[language] ?? language) : null;
}

/** The id of `path`'s icon (a path or a name, `/` between its parts). */
export function fileIconId(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1).toLowerCase();
  if (SETI_FILE_NAMES[name]) return SETI_FILE_NAMES[name];
  for (let dot = name.indexOf("."); dot !== -1; dot = name.indexOf(".", dot + 1)) {
    const icon = SETI_FILE_EXTENSIONS[name.slice(dot + 1)];
    if (icon) return icon;
  }
  const language = languageIdOf(name);
  return (language && SETI_LANGUAGE_IDS[language]) || SETI_FILE;
}

/** How `path` is drawn in `theme`: its character in seti.woff and its colour (`null`: the text's). */
export function fileIcon(path: string, theme: ResolvedTheme): { glyph: string; colour: string | null } {
  const [glyph, dark, light] = SETI_ICONS[fileIconId(path)] ?? SETI_ICONS[SETI_FILE];
  return { glyph: String.fromCodePoint(glyph), colour: theme === "light" ? light : dark };
}
