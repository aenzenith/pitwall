// The quick switcher's `+`: a board card written where the search is. The field holds its title,
// the list the projects it can go to, and Tab opens a note under the field (CaptureNote). What is
// written is lib/capture's draft; this is how the switcher's keys and rows act on it.

import { emitTo } from "@tauri-apps/api/event";
import { computed, nextTick, ref, toRef, watch, type Ref } from "vue";

import { captureOrder, cardTitle, draft, hasDraft, queuedCounts, useAdd } from "../lib/capture";
import { useNoteImages } from "../lib/cardImages";
import { TAB_ROOM } from "../lib/panel";
import { primary } from "../lib/platform";
import { api, snapshot } from "../lib/store";
import { measure } from "../lib/terminals";

/** A title is this long at most, as in the board's card dialog. */
const TITLE_MAX = 200;

/** `text` cut to a title's length, never between the two halves of an emoji: half of one is no
 * text, and the core refuses a card that holds it. */
function titleOf(text: string): string {
  const part = text.slice(0, TITLE_MAX);
  return /[\uD800-\uDBFF]$/.test(part) ? part.slice(0, -1) : part;
}

/** What the switcher lends to it. */
export type CaptureHost = {
  /** The switcher's search text. */
  query: Ref<string>;
  /** The switcher's field: it holds the card's title while one is written. */
  field: () => HTMLInputElement | null;
  /** The note under the field, while it is open. */
  note: () => { field: HTMLTextAreaElement | null; focus: () => void; closeList: () => boolean } | null;
  /** The project the highlight is on (its path), and the highlight put on a project. */
  chosen: () => string | null;
  choose: (path: string | null) => void;
};

/**
 * The size the terminal panel gives a new Claude tab, so its shell starts near that size: what the
 * Board page measures before it gives a card (BoardView: `terminalSize`), as the window and the
 * panel are at their defaults (this window can't measure that one).
 */
function tabSize(): { cols: number; rows: number } | null {
  const host = document.createElement("div");
  host.style.cssText = `position: fixed; left: -10000px; top: 0; width: ${TAB_ROOM.width}px; height: ${TAB_ROOM.height}px; visibility: hidden`;
  document.body.appendChild(host);
  try {
    return measure(host);
  } catch {
    return null;
  } finally {
    host.remove();
  }
}

export function useCapture(host: CaptureHost) {
  /** A card is being written: the field holds its title. */
  const on = ref(false);
  /** Its note is open under the field, with the keyboard. */
  const noteOpen = ref(false);
  /** The listed project looked at last, as the core told it; null when none was. */
  const looked = ref<string | null>(null);
  /** The cards in each project's "Up next"; null until the board has answered. */
  const counts = ref<Map<string, number> | null>(null);
  /** Why the last add, give or pasted image was refused. */
  const error = ref("");
  const giving = ref(false);

  /** The projects the card can go to, the likeliest first. */
  const projects = computed(() => captureOrder(snapshot.value?.projects ?? [], looked.value));
  const listed = (path: string | null): path is string => path !== null && projects.value.some((project) => project.path === path);

  const images = useNoteImages(
    toRef(draft, "note"),
    () => host.note()?.field ?? null,
    (message) => (error.value = message),
  );
  const adder = useAdd(draft, { target: () => (listed(host.chosen()) ? host.chosen() : null), images, refused: () => error.value !== "", add: api.boardAdd });
  // The note closed, and its field's undo with it: a pasted image it no longer names is let go
  // (all of them, once it was emptied). While it is open, ⌘Z brings a token back to its picture.
  watch(noteOpen, (open) => {
    if (!open) images.forget();
  });

  /** The add or the give is on its way. */
  const busy = computed(() => adder.adding.value || giving.value);
  /** A card was begun and left: `+` brings it back. */
  const waiting = computed(() => hasDraft());

  /** Asks the core which project was looked at last. */
  async function look(): Promise<void> {
    try {
      looked.value = await api.lastLooked();
    } catch {
      // Kept as it was.
    }
  }

  /** The counts and the likeliest project, as they are now. `fresh`: no project was chosen for
   * this card yet, so the highlight follows the first row unless it was moved meanwhile. */
  async function refresh(fresh: boolean): Promise<void> {
    void api
      .boardState()
      .then((cards) => (counts.value = queuedCounts(cards)))
      .catch(() => undefined);
    const before = host.chosen();
    await look();
    if (on.value && fresh && host.chosen() === before) host.choose(projects.value[0]?.path ?? null);
  }

  /** `+` was typed (`typed`: what followed it): the card left unfinished comes back, a title
   * typed with the `+` in place of its own. A note it has is in sight at once. */
  function enter(typed: string): void {
    const kept = hasDraft();
    on.value = true;
    error.value = "";
    if (typed) draft.title = titleOf(typed);
    host.query.value = "";
    if (!kept || !listed(draft.path)) draft.path = null;
    const fresh = draft.path === null;
    host.choose(draft.path ?? projects.value[0]?.path ?? null);
    noteOpen.value = draft.note.trim() !== "" && host.chosen() !== null;
    if (noteOpen.value) void nextTick(() => host.note()?.focus());
    void refresh(fresh);
  }

  /** The switcher's search text changed: true when it is a card's (`+` first), not a search. */
  function take(text: string): boolean {
    if (on.value) return true;
    const typed = cardTitle(text);
    if (typed === null) return false;
    enter(typed);
    return true;
  }

  /** The `+` was deleted: the title is a search again. A note stays for the next `+`. */
  function leave(): void {
    const text = draft.title;
    draft.title = "";
    draft.path = hasDraft() ? host.chosen() : null;
    on.value = false;
    noteOpen.value = false;
    error.value = "";
    host.query.value = text;
  }

  /** The switcher opens: on its search, whatever it was left on. A card left unfinished keeps
   * its project and waits for `+`. */
  function reset(): void {
    if (on.value) draft.path = hasDraft() ? host.chosen() : null;
    on.value = false;
    noteOpen.value = false;
    error.value = "";
    void look();
  }

  function openNote(): void {
    if (host.chosen() === null) return;
    noteOpen.value = true;
    void nextTick(() => host.note()?.focus());
  }

  /** Back to the title, the note kept. */
  function closeNote(): void {
    noteOpen.value = false;
    const field = host.field();
    if (!field) return;
    field.focus({ preventScroll: true });
    field.setSelectionRange(field.value.length, field.value.length);
  }

  /** The keyboard came to the title: the list of projects is under it again. */
  function titleFocused(): void {
    if (on.value) noteOpen.value = false;
  }

  /**
   * The highlight went from the project at `before` to another (the switcher tells: its rows are
   * made after this). With the note open and `before` no longer listed, its project left the list
   * and the row that fell into its place took the card: the note closes back to the projects, what
   * was written kept, so the project the card would go to is seen before any Return.
   */
  function chosenChanged(path: string | null, before: string | null): void {
    if (on.value && noteOpen.value && before !== null && path !== before && !listed(before)) closeNote();
  }

  /**
   * Adds the card to the chosen project's "Up next", as the board's own "Add card" does, and the
   * switcher goes. `give`: it is then given to Claude as the board's "Give to Claude" does (a new
   * Claude tab in the project's terminals), and Pitwall's window comes up on it. A card that was
   * added but couldn't be given stays in "Up next", and the switcher says why.
   */
  async function submit(give: boolean): Promise<void> {
    if (busy.value) return;
    error.value = "";
    let card;
    try {
      card = await adder.add();
    } catch (failure) {
      error.value = String(failure);
      return;
    }
    if (!card) return;

    if (!give) {
      void api.hideSwitcher();
      return;
    }

    giving.value = true;
    try {
      const tab = await api.boardGive(card.id, "new", tabSize());
      if (tab) void emitTo("main", "reveal-terminal", { path: card.path, id: tab.id });
      void api.openWindow();
    } catch (failure) {
      noteOpen.value = false;
      error.value = String(failure);
      void refresh(false);
    } finally {
      giving.value = false;
    }
  }

  /** A click on a project's row: the card goes there. */
  function addTo(path: string, give: boolean): void {
    host.choose(path);
    void submit(give);
  }

  /**
   * A key while a card is written; true when it was the card's (the switcher then leaves it
   * alone). In the title: Return adds, with ⌘ (Ctrl) adds and gives, Tab opens the note, and a
   * Backspace before the first character takes the `+` away. In the note: Return is a new line,
   * ⇧Return adds, ⌘Return adds and gives, ⇧Tab or Esc goes back to the title.
   */
  function onKey(event: KeyboardEvent): boolean {
    if (!on.value) return false;

    if (noteOpen.value) {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!host.note()?.closeList()) closeNote();
        return true;
      }
      if (event.key === "Tab") {
        event.preventDefault();
        if (event.shiftKey) closeNote();
        return true;
      }
      if (event.key === "Enter") {
        const mod = primary(event);
        if (!mod && !event.shiftKey) return true;
        event.preventDefault();
        if (!event.repeat) void submit(mod);
        return true;
      }
      // The caret's own keys.
      return event.key === "ArrowUp" || event.key === "ArrowDown";
    }

    if (event.key === "Enter") {
      event.preventDefault();
      if (!event.repeat) void submit(primary(event));
      return true;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      if (!event.shiftKey) openNote();
      return true;
    }
    if (event.key === "Backspace" && !event.repeat) {
      const field = host.field();
      if (field && event.target === field && field.selectionStart === 0 && field.selectionEnd === 0) {
        event.preventDefault();
        leave();
        return true;
      }
    }
    return false;
  }

  return {
    on,
    noteOpen,
    looked,
    counts,
    error,
    busy,
    waiting,
    projects,
    shots: images.shots,
    onPaste: images.onPaste,
    removeImage: images.remove,
    take,
    reset,
    titleFocused,
    chosenChanged,
    addTo,
    onKey,
  };
}
