import { computed, ref, watchEffect } from "vue";

import { snapshot } from "./store";
import type { Theme } from "./types";

/**
 * How the app is drawn. Settings › General picks it (`settings.theme`): as the system is, dark, or
 * light. The page's root carries the answer as `data-theme`, which picks the token set
 * (styles/tokens.css); what draws by itself (a terminal, a canvas) reads `resolvedTheme`.
 */
export type ResolvedTheme = "dark" | "light";

/** What is drawn for a dark ground and stays so in the light theme (styles/tokens.css). */
export const NIGHT = "night";

const systemQuery = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: light)") : null;
/** The system's own appearance, followed while it changes. */
const systemLight = ref(systemQuery?.matches ?? false);
systemQuery?.addEventListener("change", (event) => (systemLight.value = event.matches));

/** `theme` as it is drawn. A core that sends none yet, or anything unknown, follows the system. */
export function resolveTheme(theme: Theme | undefined, light: boolean): ResolvedTheme {
  if (theme === "dark" || theme === "light") return theme;
  return light ? "light" : "dark";
}

/** The theme in use. Before the first snapshot arrives it is the system's. */
export const resolvedTheme = computed<ResolvedTheme>(() => resolveTheme(snapshot.value?.settings.theme, systemLight.value));

/** The theme `element` is drawn in: dark inside a night, whatever the app's is. */
export function themeAt(element: Element | null): ResolvedTheme {
  return element?.closest(`.${NIGHT}`) ? "dark" : resolvedTheme.value;
}

/** Keeps the page's root on the theme in use, from now on (main.ts, once for each surface). */
export function followTheme(): void {
  watchEffect(() => (document.documentElement.dataset.theme = resolvedTheme.value));
}
