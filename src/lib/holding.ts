/**
 * A press that moves or resizes something (the terminal card or panel, the history's column, a
 * Files window): from the press to the release no text is selected on the way, not even text that
 * can be (a chat's, a file's), as the page takes `holding` (styles/tokens.css).
 */
export function holding(on: boolean): void {
  document.body.classList.toggle("holding", on);
}

/**
 * A card moved by its bar (the terminal card, a Files window): the open hand its bar shows stays
 * everywhere until the release, even where the card stops and the pointer goes on (`moving`,
 * styles/tokens.css).
 */
export function moving(on: boolean): void {
  document.body.classList.toggle("moving", on);
}

/**
 * A window dragged by its bar (`data-tauri-drag-region`): the system moves it, and the page may
 * never hear the button go up, so it would select text as the pointer goes on. It holds until the
 * button is heard going up or the next press anywhere else, which selects as ever.
 */
export function holdWhileWindowMoves(): void {
  let held = false;
  const release = (): void => {
    if (!held) return;
    held = false;
    holding(false);
  };
  document.addEventListener(
    "pointerdown",
    (event) => {
      const target = event.target instanceof Element ? event.target : null;
      const bar = event.button === 0 && target?.closest("[data-tauri-drag-region]") && !target.closest("button, input, select, textarea, a, [role='button'], [role='link']");
      if (!bar) return release();
      held = true;
      holding(true);
    },
    true,
  );
  document.addEventListener("pointerup", release, true);
}
