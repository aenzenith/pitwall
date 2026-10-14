// A card written from the quick switcher (`+`): the draft, the order its projects are offered in,
// and the one gate between a Return and the board. No component state and nothing of the core's:
// what adds the card is handed in (`AddPort`).

import { reactive, ref } from "vue";

import { byRank } from "./fuzzy";
import type { Card, NewImage, Project } from "./types";

/** What starts a card in the switcher's field; a Japanese or Chinese keyboard types the wide one. */
const PLUS = /^\s*[+＋]/;

/** `text` starts a card: what follows the `+`; null when it doesn't. */
export function cardTitle(text: string): string | null {
  const plus = PLUS.exec(text);
  return plus ? text.slice(plus[0].length).trimStart() : null;
}

/** A card being written: its title, its note (the images it names are held beside it) and the
 * project it is for (`path`; null until one is chosen). */
export type Draft = { title: string; note: string; path: string | null };

/**
 * The card being written in the quick switcher. It outlives the switcher's window going away
 * (which it does the moment it loses the keyboard) and comes back when `+` is typed again; it
 * lasts while the app runs, in this page's memory only. Only `useAdd` turns it into a card.
 */
export const draft: Draft = reactive({ title: "", note: "", path: null });

/** Something of a card is written: a title or a note (an image is named by its note). */
export function hasDraft(card: Draft = draft): boolean {
  return card.title.trim() !== "" || card.note.trim() !== "";
}

/**
 * The projects a new card is offered for: first the one looked at last (`looked`: the core's
 * `last_looked`, null when none was), then the switcher's own order (Claude waiting, Claude
 * working, running, the rest by name).
 */
export function captureOrder(projects: Project[], looked: string | null): Project[] {
  const ranked = byRank(projects);
  const at = looked === null ? -1 : ranked.findIndex((project) => project.path === looked);
  if (at > 0) ranked.unshift(...ranked.splice(at, 1));
  return ranked;
}

/** How many cards each project's "Up next" holds, by project path. */
export function queuedCounts(cards: Card[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const card of cards) {
    if (card.column === "queued") counts.set(card.path, (counts.get(card.path) ?? 0) + 1);
  }
  return counts;
}

/**
 * The key belongs to an input method: a composition is under way, or this is the Return (or Esc,
 * or an arrow) that ends one. WebKit ends the composition before it sends that key, so
 * `isComposing` is already false there; the key still carries the code input methods use (229).
 */
export function composing(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

/** What `useAdd` needs of its surroundings. */
export type AddPort = {
  /** The listed project the card goes to; null when there is none. */
  target: () => string | null;
  /** The note's pasted images: every one read, those `note` names, and none held any longer. */
  images: { settled: () => Promise<unknown>; taken: (note: string) => NewImage[]; clear: () => void };
  /** An image was refused while they were read: its message shows and nothing is added. */
  refused: () => boolean;
  /** The board's own "Add card" (`board_add`). */
  add: (path: string, title: string, note: string, images: NewImage[]) => Promise<Card>;
};

/**
 * The only way `card` becomes a card on the board, and it does so once: while one add is on its
 * way another is not started (a Return pressed twice, a held one, a click on top of it), and once
 * it is added the draft is empty, so the next Return has nothing to add.
 */
export function useAdd(card: Draft, port: AddPort) {
  const adding = ref(false);

  /**
   * Resolves with the card added, or with null when nothing was: no title, no project, an add
   * already on its way, an image refused. Rejects when the core refuses; the draft stays then.
   */
  async function add(): Promise<Card | null> {
    const title = card.title.trim();
    const path = port.target();
    if (adding.value || !title || !path) return null;

    adding.value = true;
    try {
      const note = card.note.trim();
      await port.images.settled();
      // An image that couldn't be read took its token out of the note, and says so.
      if (port.refused()) return null;
      const added = await port.add(path, title, note, port.images.taken(note));
      // The draft is that card now: nothing of it is left for another Return.
      card.title = "";
      card.note = "";
      card.path = null;
      port.images.clear();
      return added;
    } finally {
      adding.value = false;
    }
  }

  return { adding, add };
}
