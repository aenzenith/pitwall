import { onUnmounted } from "vue";

/**
 * Gives focus back to whatever had it when a dialog opened, however the dialog ends: a save
 * unmounts it while still open, which would drop focus to the page. Call from the dialog's
 * setup, before it opens.
 */
export function useReturnFocus(): void {
  const opener = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;

  // Once the dialog has left the page: while it is open everything behind it is inert.
  onUnmounted(() => {
    if (opener?.isConnected) opener.focus({ preventScroll: true });
  });
}

/**
 * Opens a modal dialog with no control focused. `showModal` puts the keyboard on a button, which
 * then wears the focus ring before a key was pressed; here it rests on the dialog itself (which
 * shows no ring: tokens.css), so Tab starts at the dialog's first control. Until then Return
 * still gives the dialog's answer (`answer`: the button a dialog would have focused, the safe
 * one where it asks before something is lost), and Esc closes as ever.
 *
 * Call from `onMounted`, in place of `showModal()`. A dialog that opens on a text field focuses
 * that field itself and has no use for this.
 */
export function openUnfocused(dialog: HTMLDialogElement | null, answer?: () => HTMLButtonElement | null): void {
  if (!dialog) return;
  dialog.showModal();
  if (!dialog.hasAttribute("tabindex")) dialog.tabIndex = -1;
  dialog.focus({ preventScroll: true });
  if (!answer) return;

  const onKey = (event: KeyboardEvent): void => {
    // Gone from the page while still open (a save unmounts it): the key is let go.
    if (!dialog.isConnected || !dialog.open) {
      window.removeEventListener("keydown", onKey);
      return;
    }
    // A Return still held from before the dialog opened (it repeats) answers nothing.
    if (event.key !== "Enter" || event.repeat || event.isComposing || event.defaultPrevented) return;

    // A control with the keyboard takes its own Return; so does a dialog opened over this one.
    const active = document.activeElement;
    if (active && active !== document.body && active !== dialog) return;
    const open = document.querySelectorAll("dialog[open]");
    if (open[open.length - 1] !== dialog) return;

    const button = answer();
    if (button && !button.disabled) {
      event.preventDefault();
      button.click();
    }
  };
  window.addEventListener("keydown", onKey);
}
