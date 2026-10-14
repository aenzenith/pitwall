import { inject, type InjectionKey } from "vue";

import type { MenuPoint } from "../../lib/nativeMenu";

/** What a card can ask the Board page to do; the page provides it, its cards inject it. */
export type BoardActions = {
  select: (id: string) => void;
  /** False right after a drag: the click that ends it selects nothing. */
  isClick: () => boolean;
  edit: (id: string) => void;
  menu: (at: MenuPoint, id: string) => void;
  give: (id: string, mode: "new" | "plan") => void;
  /** Given again, with the model and in the mode it was started with last. */
  giveAgain: (id: string) => void;
  /** The panel of ways to give it (the model, the mode, when), under its button; a second ask
   * closes it. */
  giveMenu: (at: MenuPoint, id: string) => void;
  /** The card that panel is open for; null while it is closed. */
  panelFor: () => string | null;
  /** A card that waits for its moment: given now, or left up next without waiting. */
  giveNow: (id: string) => void;
  unschedule: (id: string) => void;
  /** Its session brought up: one in a Pitwall terminal in the card's details, the rest where they run. */
  reveal: (id: string) => void;
  done: (id: string) => void;
  /** A press that may become a drag (one project's board only). */
  press: (event: PointerEvent, id: string) => void;
};

export const BOARD_ACTIONS: InjectionKey<BoardActions> = Symbol("board-actions");

export function useBoardActions(): BoardActions {
  const actions = inject(BOARD_ACTIONS);
  if (!actions) throw new Error("A board card outside the Board page");
  return actions;
}

/** The action a card's button spins for until the core answers. */
export type CardBusy = "give" | "done" | "move" | "reveal" | null;
