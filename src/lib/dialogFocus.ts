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
