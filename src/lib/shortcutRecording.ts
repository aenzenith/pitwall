// One shortcut is recorded at a time, whichever part of Settings asked: the quick switcher's
// (window/SettingsView.vue) or the mini tower's (window/settings/TowerSection.vue). Each listens
// for keys on the whole window, so two under way would both take the same chord.

/** Ends the recording under way, without a word to the core: the one that starts keeps both
 * shortcuts stood down. */
let end: (() => void) | null = null;

/** A recording starts: the one under way, if any, ends. `stop` ends this one the same way. */
export function recordingStarts(stop: () => void): void {
  const before = end;
  end = stop;
  if (before && before !== stop) before();
}

/** A recording ended by itself: its keys came, or it was given up. */
export function recordingEnded(stop: () => void): void {
  if (end === stop) end = null;
}
