/**
 * What the chat's dialogs share (dialogs/ChatDialogHost.vue draws them): their props, the way they
 * ask the chat's Claude Code something (`api.chatControl`), keys in their lists, and the few
 * words every one of them formats the same way.
 */
import { nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch, type Ref } from "vue";

import { dayName } from "../../../lib/day";
import { language, t, type Key } from "../../../lib/i18n";
import { dayShort, shiftDay } from "../../../lib/period";
import { api, today } from "../../../lib/store";
import { draftAsk, draftTake, isDraft } from "../../../lib/chat/draft";
import { noteControl } from "../../../lib/chat/store";
import type { ChatControl, ChatInfo, ChatModelInfo, ChatState } from "../../../lib/chat/wire";

/** What every dialog is given by the host. `arg`: what followed the command (`/model sonnet`). */
export type DialogProps = { id: number; path: string; session: string | null; info: ChatInfo | null; state: ChatState; arg?: string };

/**
 * A menu over one of the composer's chips, the model's or the mode's (the user's word, 14 and
 * 15 Nov: as the VS Code extension draws them, no dialog): over the chip, else under it where
 * there is more room, in the middle without one. Gone at a click outside it and its chip (the
 * chip's own click is the chip's: ChatView closes the menu then), the window's blur or a resize;
 * Esc is the menu's own key. The keyboard goes to `focus` (else the menu) as it opens.
 */
export function useChipMenu(opts: { anchor: () => HTMLElement | null; width: number; menu: Ref<HTMLElement | null>; focus: () => HTMLElement | null; close: () => void }) {
  const place = ref<Record<string, string>>({});

  function settle(): void {
    const chip = opts.anchor()?.getBoundingClientRect();
    if (!chip) {
      place.value = { left: `${Math.max(8, (window.innerWidth - opts.width) / 2)}px`, top: "15vh", maxHeight: "70vh" };
      return;
    }
    const left = Math.max(8, Math.min(chip.left, window.innerWidth - opts.width - 8));
    const above = chip.top - 14;
    const below = window.innerHeight - chip.bottom - 14;
    place.value =
      above >= 240 || above >= below
        ? { left: `${left}px`, bottom: `${window.innerHeight - chip.top + 6}px`, maxHeight: `${Math.max(160, above)}px` }
        : { left: `${left}px`, top: `${chip.bottom + 6}px`, maxHeight: `${Math.max(160, below)}px` };
  }

  function onOutside(event: PointerEvent): void {
    const target = event.target;
    if (target instanceof Node && (opts.menu.value?.contains(target) || opts.anchor()?.contains(target))) return;
    // A dialog over it (a pick that couldn't be made tells why) keeps it.
    if (target instanceof Element && target.closest("dialog[open]")) return;
    opts.close();
  }

  onMounted(() => {
    settle();
    window.addEventListener("pointerdown", onOutside, true);
    window.addEventListener("blur", opts.close);
    window.addEventListener("resize", opts.close);
    (opts.focus() ?? opts.menu.value)?.focus({ preventScroll: true });
  });

  onBeforeUnmount(() => {
    window.removeEventListener("pointerdown", onOutside, true);
    window.removeEventListener("blur", opts.close);
    window.removeEventListener("resize", opts.close);
  });

  return place;
}

/** A control's error as the core gave it (a string), never longer than a line or two. */
export function errorText(reason: unknown): string {
  const text = (typeof reason === "string" ? reason : reason instanceof Error ? reason.message : String(reason)).trim();
  return text.length > 300 ? `${text.slice(0, 300)}…` : text;
}

/**
 * Asks the chat's Claude Code one thing at a time: the last answer (`value`) or its error
 * (`error`), `busy` while it is under way. An answer to an older question than the last is
 * dropped. The empty panel's chat box, a chat to be (lib/chat/draft), answers for itself.
 */
export function useAsk<T>(id: () => number) {
  const value = shallowRef<T | null>(null);
  const error = ref<string | null>(null);
  const busy = ref(false);
  let asked = 0;

  async function load(control: ChatControl): Promise<T | null> {
    const mine = ++asked;
    error.value = null;
    if (isDraft(id())) {
      busy.value = false;
      value.value = (draftAsk(id(), control) as T | null) ?? null;
      return value.value;
    }
    busy.value = true;
    try {
      const answer = await api.chatControl<T>(id(), control);
      if (mine !== asked) return null;
      value.value = answer ?? null;
      return value.value;
    } catch (reason) {
      if (mine === asked) error.value = errorText(reason);
      return null;
    } finally {
      if (mine === asked) busy.value = false;
    }
  }

  return { value, error, busy, load };
}

/** One control sent for an action (a pick, a switch): its error, or null once it went through (the
 * composer's chips then show what it set). The empty panel's chat box keeps a pick itself, for the
 * chat it opens; one no start takes is left alone (its menus offer none). */
export async function send(id: number, control: ChatControl): Promise<string | null> {
  if (isDraft(id)) {
    draftTake(id, control);
    return null;
  }
  try {
    await api.chatControl(id, control);
    noteControl(id, control);
    return null;
  } catch (reason) {
    return errorText(reason);
  }
}

/**
 * ↑/↓, Home/End and Return in a dialog's list: `active` is the keyboard's row, `pick` what Return
 * does with it. `root` holds the rows (each with `data-row` set to its index), so the active one
 * is scrolled into view.
 */
export function useListNav(count: () => number, pick: (index: number) => void, root: Ref<HTMLElement | null>) {
  const active = ref(0);

  watch(count, (n) => {
    if (active.value >= n) active.value = Math.max(0, n - 1);
  });

  function show(): void {
    void nextTick(() => root.value?.querySelector(`[data-row="${active.value}"]`)?.scrollIntoView({ block: "nearest" }));
  }

  function move(to: number): void {
    const n = count();
    if (!n) return;
    active.value = Math.max(0, Math.min(n - 1, to));
    show();
  }

  /** True when the key was the list's. A key already taken, or pressed in a text area or on a
   * control of a row (its button, its switch), is left alone; a search field's ↑/↓/Return move
   * and pick in the list under it. */
  function onKey(event: KeyboardEvent): boolean {
    if (event.defaultPrevented || event.isComposing || event.metaKey || event.ctrlKey || event.altKey) return false;
    const target = event.target;
    if (target instanceof HTMLTextAreaElement || target instanceof HTMLButtonElement || target instanceof HTMLSelectElement) return false;
    if (target instanceof HTMLInputElement && target.type !== "text" && target.type !== "search") return false;
    const typing = target instanceof HTMLInputElement;
    switch (event.key) {
      case "ArrowDown":
        move(active.value + 1);
        break;
      case "ArrowUp":
        move(active.value - 1);
        break;
      case "Home":
        if (typing) return false;
        move(0);
        break;
      case "End":
        if (typing) return false;
        move(count() - 1);
        break;
      case "Enter":
        if (event.shiftKey || event.repeat || !count()) return false;
        pick(active.value);
        break;
      default:
        return false;
    }
    event.preventDefault();
    return true;
  }

  return { active, move, onKey };
}

/** The project's folder name: the line under a dialog's title, as the app's other dialogs have it. */
export function projectName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** `file` as a path of the project at `project` when it lies inside its folder; null otherwise. */
export function within(project: string, file: string): string | null {
  const root = project.replace(/[\\/]+$/, "");
  if (!root || !file.startsWith(root)) return null;
  const rest = file.slice(root.length);
  if (!/^[\\/]/.test(rest)) return null;
  const relative = rest.replace(/^[\\/]+/, "");
  return relative || null;
}

/** A path as it is shown: a project's own as `./…`, one in a home folder with `~`. Display only. */
export function shownPath(project: string, file: string): string {
  const inside = within(project, file);
  if (inside) return `./${inside}`;
  return file.replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+(?=\\|$)/, "~");
}

/** Text the CLI hands over as it is stored (a rule, a hook's command): its invisible and control
 * characters made visible, as the terminal shows them. */
export function reveal(text: string): string {
  return text.replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u00ad\u200b-\u200f\u202a-\u202e\u2060-\u2064\u2066-\u2069\ufeff]/g, (ch) => `\\u{${ch.codePointAt(0)!.toString(16)}}`);
}

/** The model the chat runs, as one of the CLI's rows: by its value, else by the id it resolves to. */
export function modelRow(models: ChatModelInfo[], model: string | null | undefined): ChatModelInfo | null {
  if (!model) return null;
  return models.find((row) => row.value === model) ?? models.find((row) => row.resolvedModel === model) ?? null;
}

/** A model as people read it: its display name where the CLI listed it; a model id by the row
 * that names it (`Opus 5.5`), not by the "Default" row that resolves to it too. */
export function modelName(info: ChatInfo | null, model: string | null | undefined): string {
  if (!model) return "";
  const models = info?.models ?? [];
  const row = models.find((item) => item.value === model) ?? models.find((item) => item.resolvedModel === model && item.value !== "default") ?? modelRow(models, model);
  return row?.displayName ?? model;
}

/** Numbers as the app's language writes them. */
export function count(value: number): string {
  return new Intl.NumberFormat(language.value).format(value);
}

/** A share, `0.034` as `3.4 %`: a tenth under ten, whole above. */
export function share(part: number): string {
  const value = Math.max(0, part);
  return new Intl.NumberFormat(language.value, { style: "percent", maximumFractionDigits: value < 0.1 && value > 0 ? 1 : 0 }).format(value);
}

/** When something happens or happened, from a time in ms or an ISO string: `14:05` today, the day
 * before as yesterday, another day by its date. */
export function when(at: number | string | null | undefined): string {
  if (at === null || at === undefined) return "";
  const ms = typeof at === "number" ? at : Date.parse(at);
  if (!Number.isFinite(ms)) return "";
  const time = new Intl.DateTimeFormat(language.value, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).format(ms);
  const day = dayName(ms);
  if (day === today.value) return t("chatDialog.when.today", { time });
  if (day === shiftDay(today.value, -1)) return t("chatDialog.when.yesterday", { time });
  if (day === shiftDay(today.value, 1)) return t("chatDialog.when.tomorrow", { time });
  // A refill later this week by its weekday (`Mon 20:59`), any other day by its date.
  const ahead = day > today.value && day <= shiftDay(today.value, 6);
  const date = ahead ? new Intl.DateTimeFormat(language.value, { weekday: "short" }).format(ms) : dayShort(day);
  return t("chatDialog.when.on", { date, time });
}

/** Where a rule, a hook or a folder grant comes from, in the app's words; an unknown source keeps
 * the CLI's own label (`fallback`), else its name. */
const SOURCES: Record<string, Key> = {
  userSettings: "chatDialog.source.user",
  projectSettings: "chatDialog.source.project",
  localSettings: "chatDialog.source.local",
  flagSettings: "chatDialog.source.flag",
  policySettings: "chatDialog.source.policy",
  cliArg: "chatDialog.source.cli",
  command: "chatDialog.source.command",
  session: "chatDialog.source.session",
  sessionHook: "chatDialog.source.session",
  mcpMount: "chatDialog.source.mcp",
  toolsNarrowing: "chatDialog.source.tools",
  mcpServerPolicy: "chatDialog.source.mcpPolicy",
  hostCredential: "chatDialog.source.host",
};

export function sourceName(name: string | undefined, fallback?: string): string {
  if (!name) return fallback ?? "";
  return SOURCES[name] ? t(SOURCES[name]) : (fallback ?? name);
}
