<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";

import { filesWindows, minimizeFiles } from "../../lib/explorer";
import FilesWindow from "./FilesWindow.vue";

/* The Files windows, one per project, stacked among themselves above the app's pages: each
   window's own place in the stack (`z`) counts only inside this layer, so none rises over the
   app's dialogs. The layer itself lets every click through to what lies under it. */

/** A click anywhere else in Pitwall folds every window up into its sidebar entry. Not one in a
 * window, on the sidebar's entries (they bring a window up), on what opens and closes a window
 * by itself (`data-files-toggle`: the details' folder button) or where the app's own window is
 * dragged. */
function away(event: PointerEvent): void {
  if (event.button !== 0) return;
  const target = event.target instanceof Element ? event.target : null;
  if (!target || target.closest(".files-win, .files-nav, [data-files-toggle]")) return;
  // A press that drags the app's window (a region's bare ground, not a control in it).
  if (target.closest("[data-tauri-drag-region]") && !target.closest("button, input, select, textarea, a, [role='button'], [role='link']")) return;
  for (const win of filesWindows) if (win.open && !win.minimized) minimizeFiles(win);
}

onMounted(() => document.addEventListener("pointerdown", away, true));
onBeforeUnmount(() => document.removeEventListener("pointerdown", away, true));
</script>

<template>
  <div class="files-layer">
    <FilesWindow v-for="win in filesWindows" :key="win.path" :win="win" />
  </div>
</template>

<style scoped>
.files-layer {
  position: fixed;
  inset: 0;
  z-index: 40;
  pointer-events: none;
}
</style>
