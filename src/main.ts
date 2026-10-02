import { createApp, watchEffect } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./styles/tokens.css";

import { language } from "./lib/i18n";
import { isWindows, platform } from "./lib/platform";
import { connect, visible } from "./lib/store";
import Popover from "./popover/Popover.vue";
import Switcher from "./switcher/Switcher.vue";
import WindowApp from "./window/WindowApp.vue";

// Both windows load the same page; the window label picks the surface.
const label = getCurrentWindow().label;

document.documentElement.dataset.surface = label;

// mac | windows | linux: the styles that differ by system (title bar room, scrollbars, a
// borderless window's ground) branch on it.
document.documentElement.dataset.platform = platform;

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

// Screen readers, hyphenation and `text-transform` (the Turkish İ) follow the page language.
watchEffect(() => (document.documentElement.lang = language.value));

// Off screen, every CSS animation stands still (tokens.css: html.paused).
watchEffect(() => document.documentElement.classList.toggle("paused", !visible.value));

// No browser context menu (Reload, Inspect…) outside text fields and selectable output.
document.addEventListener("contextmenu", (event) => {
  if (!(event.target instanceof HTMLElement && event.target.closest("input, textarea, .selectable"))) {
    event.preventDefault();
  }
});
const surfaces = { popover: Popover, switcher: Switcher } as const;

createApp(surfaces[label as keyof typeof surfaces] ?? WindowApp).mount("#app");
void connect();
