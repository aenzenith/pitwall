import type { Ref } from "vue";

/**
 * Closes a modal `<dialog>` on a click on its backdrop, through the dialog's own close event (as
 * Esc does). Both the press and the release must land outside the dialog's box: a press that
 * starts inside (selecting an input's text) and ends outside keeps it open.
 *
 * Bind `down` to the dialog's `pointerdown` and `click` to its `click`.
 */
export function useBackdropClose(dialog: Ref<HTMLDialogElement | null>) {
  let pressed = false;

  /** The backdrop is the dialog's own pseudo-element: its events target the dialog itself. */
  function outside(event: MouseEvent): boolean {
    const el = dialog.value;
    if (!el || event.target !== el) return false;
    const box = el.getBoundingClientRect();
    return event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom;
  }

  return {
    down(event: PointerEvent): void {
      pressed = outside(event);
    },
    click(event: MouseEvent): void {
      if (pressed && outside(event)) dialog.value?.close();
      pressed = false;
    },
  };
}
