import { t, type Key } from "./i18n";

/**
 * The system Pitwall runs on, read once from the web view: WebKit on macOS says "Macintosh",
 * WebView2 on Windows "Windows", WebKitGTK on Linux "Linux" or "X11". Chromium's
 * `navigator.userAgentData` (WebView2) is asked first where it exists, then `navigator.platform`
 * (which WebKitGTK keeps true even where a site quirk dresses its user agent up as a Mac's).
 * Anything else counts as macOS, the platform the app was made on. `main.ts` puts it on
 * `<html data-platform>` for CSS.
 */
export type Platform = "mac" | "windows" | "linux";

type Nav = { userAgent?: string; platform?: string; userAgentData?: { platform?: string } };

export function detectPlatform(nav: Nav | undefined): Platform {
  for (const text of [nav?.userAgentData?.platform, nav?.platform, nav?.userAgent]) {
    if (!text) continue;
    if (/windows|win32|win64/i.test(text)) return "windows";
    if (/linux|x11|bsd/i.test(text)) return "linux";
    if (/mac/i.test(text)) return "mac";
  }
  return "mac";
}

export const platform: Platform = detectPlatform(typeof navigator === "undefined" ? undefined : (navigator as Nav));
export const isMac = platform === "mac";
export const isWindows = platform === "windows";
export const isLinux = platform === "linux";

/**
 * The platform's primary modifier is held: ⌘ on macOS, Ctrl on Windows and Linux. Not with Alt
 * there: Windows reports AltGr as Ctrl+Alt, and AltGr+C is a letter (ć) on some keyboards.
 */
export function primary(event: KeyboardEvent | MouseEvent): boolean {
  return isMac ? event.metaKey : event.ctrlKey && !event.altKey;
}

/**
 * The terminal panel's own keys (new tab, clear, copy, paste). On macOS ⌘, which is never the
 * shell's. On Windows and Linux Ctrl+letter is the shell's (Ctrl+C, Ctrl+D, Ctrl+R, Ctrl+T…),
 * so they take Ctrl+Shift, as Windows Terminal, GNOME Terminal and Konsole do.
 */
export function terminalChord(event: KeyboardEvent): boolean {
  return isMac ? event.metaKey : event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
}

/** The terminal keys' modifiers, for `keys()`: `mod+Shift+T` reads `Ctrl+Shift+T`. */
export const TERMINAL_MOD = isMac ? "mod" : "mod+Shift";

/**
 * When a borderless or overlay window drags from its content (`data-tauri-drag-region`). Only on
 * macOS, where the main window's title bar is an overlay; Windows and Linux have their own.
 */
export const dragRegion: "deep" | undefined = isMac ? "deep" : undefined;

type Mod = "ctrl" | "alt" | "shift" | "meta";

const MOD_TOKENS: Record<string, Mod> = {
  MOD: isMac ? "meta" : "ctrl",
  CMDORCTRL: isMac ? "meta" : "ctrl",
  COMMANDORCONTROL: isMac ? "meta" : "ctrl",
  CTRL: "ctrl",
  CONTROL: "ctrl",
  ALT: "alt",
  OPTION: "alt",
  SHIFT: "shift",
  SUPER: "meta",
  META: "meta",
  CMD: "meta",
  COMMAND: "meta",
};

/** macOS: glyphs, in the usual order (⌃⌥⇧⌘), run together. Elsewhere: the names the language gives
 * the keys (Strg in German), Win/Super first, joined with `+`. */
const MAC_MODS: Record<Mod, string> = { ctrl: "⌃", alt: "⌥", shift: "⇧", meta: "⌘" };
const PC_MODS: Record<Mod, Key> = { ctrl: "keys.ctrl", alt: "keys.alt", shift: "keys.shift", meta: isWindows ? "keys.win" : "keys.super" };
const MOD_ORDER: Mod[] = isMac ? ["ctrl", "alt", "shift", "meta"] : ["meta", "ctrl", "alt", "shift"];

function modLabel(mod: Mod): string {
  return isMac ? MAC_MODS[mod] : t(PC_MODS[mod]);
}

/** Keys (as `KeyboardEvent.code` or a name, upper-cased) that don't read as themselves: as macOS
 * writes them, and the catalog's name elsewhere. */
const MAC_KEYS: Record<string, string> = { ENTER: "↵", ESCAPE: "esc", SPACE: "Space", TAB: "⇥", BACKSPACE: "⌫", DELETE: "⌦" };
const PC_KEYS: Record<string, Key> = {
  ENTER: "keys.enter",
  ESCAPE: "keys.esc",
  SPACE: "keys.space",
  TAB: "keys.tab",
  BACKSPACE: "keys.backspace",
  DELETE: "keys.delete",
};
const SYMBOLS: Record<string, string> = {
  ARROWUP: "↑",
  ARROWDOWN: "↓",
  ARROWLEFT: "←",
  ARROWRIGHT: "→",
  MINUS: "-",
  EQUAL: "=",
  COMMA: ",",
  PERIOD: ".",
  SLASH: "/",
  BACKSLASH: "\\",
  SEMICOLON: ";",
  QUOTE: "'",
  BACKQUOTE: "`",
  BRACKETLEFT: "[",
  BRACKETRIGHT: "]",
};

function keyLabel(key: string, macKeys: Record<string, string>): string {
  const named = isMac ? (macKeys[key] ?? MAC_KEYS[key]) : key in PC_KEYS ? t(PC_KEYS[key]) : undefined;
  return named ?? SYMBOLS[key] ?? key.replace(/^KEY/, "").replace(/^DIGIT/, "");
}

/**
 * A key combination as this platform writes it: `keys("mod+Enter")` is `⌘↵` on macOS and
 * `Ctrl+Enter` on Windows and Linux; `keys("Ctrl+Alt+KeyP")` is `⌃⌥P` or `Ctrl+Alt+P`. `mod` is
 * the primary modifier (`primary()`); keys are names or `KeyboardEvent.code`s. `macKeys` replaces
 * some macOS key glyphs (upper-cased names). Only modifiers (a shortcut being recorded) read as
 * just those.
 */
export function keys(combo: string, macKeys: Record<string, string> = {}): string {
  const parts = combo.split("+").map((part) => part.trim().toUpperCase()).filter(Boolean);
  const mods = new Set(parts.filter((part) => part in MOD_TOKENS).map((part) => MOD_TOKENS[part]));
  const key = parts.find((part) => !(part in MOD_TOKENS)) ?? "";
  const all = [...MOD_ORDER.filter((mod) => mods.has(mod)).map(modLabel), key ? keyLabel(key, macKeys) : ""].filter(Boolean);
  return all.join(isMac ? "" : "+");
}

/** The modifiers a global shortcut needs one of, as `keys()` writes them: ⌘, ⌃, ⌥ or Ctrl, Alt, Win. */
export function shortcutModifiers(): { mod1: string; mod2: string; mod3: string } {
  const [mod1, mod2, mod3] = (isMac ? (["meta", "ctrl", "alt"] as const) : (["ctrl", "alt", "meta"] as const)).map(modLabel);
  return { mod1, mod2, mod3 };
}
