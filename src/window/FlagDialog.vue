<script setup lang="ts">
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

import { flagOpening, flagRequest } from "../lib/flag";
import { api } from "../lib/store";
import type { Flag } from "../lib/types";
import FlagSheet from "./flag/FlagSheet.vue";

/**
 * The chequered flag: what is still open (core/flag.rs), in one dialog. It comes up in two ways.
 * `day`: asked for from the Today page's bar ("Close the day"). `quit`: a quit asked for from one
 * of Pitwall's own menus was held because something is open; the core says so with `flag-ask`,
 * and again in its answer to a page that wasn't listening yet. Mounted once by WindowApp; it
 * opens itself, once the core has said what is open, so the dialog comes up with its rows in place.
 */
const mode = ref<"day" | "quit" | null>(null);
/** What was open as the dialog came up. */
const first = ref<Flag | null>(null);
/** An answer is on its way; what asked for the dialog, and hasn't been answered yet. */
let asking = false;
let wanted = { day: false, quit: false };

async function open(kind: "day" | "quit"): Promise<void> {
  wanted[kind] = true;
  // Up already: a quit held meanwhile takes it over.
  if (mode.value) {
    if (wanted.quit) mode.value = "quit";
    wanted = { day: false, quit: false };
    flagOpening.value = false;
    return;
  }
  if (asking) return;

  asking = true;
  try {
    // Asked for again while the core answered: once more, for that one.
    while (!mode.value && (wanted.day || wanted.quit)) {
      const day = wanted.day;
      wanted = { day: false, quit: false };
      // From the bar the projects' Git state is read again first; a held quit has just read it.
      const answer = await api.flagState(day);
      // Only a quit that still waits is answered; one let go meanwhile shows nothing.
      if (answer.asked) mode.value = "quit";
      else if (day) mode.value = "day";
      if (mode.value) first.value = answer.open;
    }
    // A quit held after the core had answered the bar's button.
    if (mode.value && wanted.quit) mode.value = "quit";
  } catch {
    // A core that doesn't know the command: nothing to show.
  } finally {
    asking = false;
    wanted = { day: false, quit: false };
    flagOpening.value = false;
  }
}

/** Closed without "Quit anyway": a held quit is given up, and the next one asks again. */
function closed(): void {
  const was = mode.value;
  mode.value = null;
  first.value = null;
  if (was === "quit") void api.flagKeep().catch(() => undefined);
}

watch(flagRequest, () => void open("day"));

let unlisten: UnlistenFn | null = null;
let gone = false;

onMounted(async () => {
  const stop = await getCurrentWindow().listen("flag-ask", () => void open("quit"));
  if (gone) return stop();
  unlisten = stop;
  // A quit held before this page listened: its answer says so.
  void open("quit");
});

onBeforeUnmount(() => {
  gone = true;
  unlisten?.();
});
</script>

<template>
  <FlagSheet v-if="mode && first" :mode="mode" :first="first" @close="closed" />
</template>
