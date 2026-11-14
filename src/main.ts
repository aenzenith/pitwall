import { createApp, watchEffect } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./styles/tokens.css";

import { holdWhileWindowMoves } from "./lib/holding";
import { language } from "./lib/i18n";
import { isWindows, platform } from "./lib/platform";
import { connect, visible } from "./lib/store";
import { followTheme } from "./lib/theme";
import Popover from "./popover/Popover.vue";
import Switcher from "./switcher/Switcher.vue";
import TerminalsApp from "./terminals/TerminalsApp.vue";
import Tower from "./tower/Tower.vue";
import WindowApp from "./window/WindowApp.vue";

// Every window loads the same page; the window label picks the surface.
const label = getCurrentWindow().label;

document.documentElement.dataset.surface = label;

// mac | windows | linux: the styles that differ by system (title bar room, scrollbars, a
// borderless window's ground) branch on it.
document.documentElement.dataset.platform = platform;

// dark | light: the token set every surface is drawn with (tokens.css), from Settings › General
// or the system, and again whenever either changes.
followTheme();

// WebView2 answers browser keys a desktop app doesn't have: reload, print, find, save, view
// source, caret browsing. Keys the page uses itself (the switcher's Ctrl+P, a terminal's Ctrl+R)
// are handled before this sees them, or prevented again here harmlessly.
const BROWSER_KEYS = new Set(["KeyR", "KeyP", "KeyF", "KeyG", "KeyS", "KeyU", "KeyJ", "KeyH", "KeyO"]);
if (isWindows) {
  window.addEventListener("keydown", (event) => {
    const browserKey =
      event.key === "F5" ||
      event.key === "F7" ||
      event.key === "BrowserBack" ||
      event.key === "BrowserForward" ||
      (event.altKey && (event.key === "ArrowLeft" || event.key === "ArrowRight") && !event.ctrlKey) ||
      (event.ctrlKey && !event.altKey && BROWSER_KEYS.has(event.code));
    if (browserKey) event.preventDefault();
  });
}

// A window dragged by its bar selects no text on the way, nor after it while the page still thinks
// the button is down.
holdWhileWindowMoves();

// Screen readers, hyphenation and `text-transform` (the Turkish İ) follow the page language.
watchEffect(() => (document.documentElement.lang = language.value));

// Off screen, every CSS animation stands still (tokens.css: html.paused).
watchEffect(() => document.documentElement.classList.toggle("paused", !visible.value));

// No browser context menu but a text field's (Paste…) and selected text's in selectable output
// (Copy, Look Up; a right-click on a word selects it first). So never Reload, which WebKit offers
// wherever nothing is selected, nor a link's Open and Download.
document.addEventListener("contextmenu", (event) => {
  const target = event.target instanceof Element ? event.target : null;
  const field = target?.closest("input, textarea, [contenteditable]:not([contenteditable='false'])");
  const selected = !!target?.closest(".selectable") && !target.closest("a") && !(getSelection()?.isCollapsed ?? true);
  if (!field && !selected) event.preventDefault();
});
const surfaces = { popover: Popover, switcher: Switcher, tower: Tower, terminals: TerminalsApp } as const;

createApp(surfaces[label as keyof typeof surfaces] ?? WindowApp).mount("#app");
void connect();
