// The first run (window/SetupView) and the projects folder's finds: what both need to tell.

import { computed, reactive, ref, watch, type ComputedRef, type Ref } from "vue";

import { shortcutLabel } from "./format";
import { t } from "./i18n";
import { isMac } from "./platform";
import { api, snapshot } from "./store";
import type { Candidate } from "./types";

const DAY = 86_400_000;

/** A folder worked on within this long comes checked: three months. */
const RECENT = 90 * DAY;

/** Worked on within the last three months; not when that can't be told. */
export function recent(candidate: Candidate, now: number): boolean {
  return candidate.changedAt !== null && now - candidate.changedAt <= RECENT;
}

/** The newest first, those whose last change can't be told last; names settle a tie. */
export function byNewest(a: Candidate, b: Candidate): number {
  if (a.changedAt !== b.changedAt) {
    if (a.changedAt === null) return 1;
    if (b.changedAt === null) return -1;
    return b.changedAt - a.changedAt;
  }
  return a.name.localeCompare(b.name);
}

function midnight(at: number): number {
  const date = new Date(at);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/**
 * When a folder was last worked on, short enough for a narrow column: `today`, `yesterday`,
 * `3 days`, `8 months`, `1 year`. Days are calendar days; a month is 30 of them, a year 365.
 */
export function changedShort(at: number | null, now: number): string {
  if (at === null) return "—";
  const days = Math.round((midnight(now) - midnight(at)) / DAY);
  if (days <= 0) return t("setup.changed.today");
  if (days === 1) return t("setup.changed.yesterday");
  if (days < 30) return t("setup.changed.days", { count: days });
  if (days < 365) return t("setup.changed.months", { count: Math.floor(days / 30) });
  return t("setup.changed.years", { count: Math.floor(days / 365) });
}

/**
 * A global shortcut (`Ctrl+Alt+KeyP`) as key caps, one per key, in the order `shortcutLabel`
 * writes them: `⌃` `⌥` `P` on macOS (its modifiers are one glyph each, run together), `Ctrl`
 * `Alt` `P` elsewhere.
 */
export function shortcutCaps(hotkey: string): string[] {
  const label = shortcutLabel(hotkey);
  if (!isMac) return label.split("+").filter(Boolean);
  const [, mods = "", key = ""] = /^([⌃⌥⇧⌘]*)(.*)$/u.exec(label) ?? [];
  return [...mods, key].filter(Boolean);
}

type SwitchKey = "suggestProjects" | "shortcut" | "launchAtLogin";

/** What the switches were just set to and the snapshot doesn't say yet: every save carries all of
 * them, so one switch's save never writes an older value over another's. */
const picked = reactive<Partial<Record<SwitchKey, boolean>>>({});

/** Picks whose save the core took while the snapshot still said otherwise: the next snapshot is
 * what is stored. */
const answered = new Set<SwitchKey>();

watch(snapshot, () => {
  for (const key of answered) delete picked[key];
  answered.clear();
});

/**
 * One on/off setting as a switch: `on` is what was just picked until the snapshot says what is
 * stored. A save the core refuses puts the switch back and leaves the reason in `error`.
 */
export function useSwitch(key: SwitchKey): { on: ComputedRef<boolean>; error: Ref<string>; set: (next: boolean) => Promise<void> } {
  const error = ref("");
  const stored = computed(() => snapshot.value?.settings[key] ?? false);
  const on = computed(() => picked[key] ?? stored.value);

  async function set(next: boolean): Promise<void> {
    const settings = snapshot.value?.settings;
    if (!settings) return;
    error.value = "";
    picked[key] = next;
    answered.delete(key);
    try {
      await api.setSettings({ ...settings, ...picked });
    } catch (reason) {
      // Nothing was stored: the switch goes back, unless it was set again meanwhile.
      if (picked[key] === next) delete picked[key];
      error.value = String(reason);
      return;
    }
    if (picked[key] !== next) return;
    if (stored.value === next) delete picked[key];
    else answered.add(key);
  }

  return { on, error, set };
}
