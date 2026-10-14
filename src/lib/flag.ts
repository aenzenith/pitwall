import { ref } from "vue";

/**
 * The chequered flag asked for from the Today page's bar ("Close the day"): the dialog
 * (window/FlagDialog) comes up once the core has said what is still open. `flagOpening`: that
 * answer is on its way, and the button that asked shows it.
 */
export const flagRequest = ref(0);
export const flagOpening = ref(false);

export function openFlag(): void {
  if (flagOpening.value) return;
  flagOpening.value = true;
  flagRequest.value++;
}

/**
 * A new shell tab asked for from the dialog (a row of "Waiting for push"): the window shows the
 * project in the list, and its terminal panel (window/TerminalPanel) opens the tab as its own
 * "New tab" does, at the panel's size. Whoever opens it clears it; `at` lets one that never finds
 * its panel lapse.
 */
export const shellRequest = ref<{ path: string; at: number } | null>(null);
