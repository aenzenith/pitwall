<script setup lang="ts">
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import { connectAccounts } from "../lib/accounts";
import { chatAsks } from "../lib/chat/open";
import { t } from "../lib/i18n";
import { panelPath, revealTab, terminalCollapsed } from "../lib/panel";
import { followStand, keepPanelState, listenPanelAsks, standPanel, takePanelState } from "../lib/panelStand";
import { closeChord, dragRegion, terminalChord } from "../lib/platform";
import { api, connectDeps, connectSessions, snapshot } from "../lib/store";
import StartChatDialog from "../window/chat/StartChatDialog.vue";
import TerminalPanel from "../window/TerminalPanel.vue";

/**
 * The terminals' own window (Settings › General, the panel's buttons, the card let go out of
 * Pitwall's window): the panel of the main window's bottom, standing alone, over the same
 * terminals. Its bar keeps the window buttons' room, says what it is, and holds "Always on top" and
 * the way back into Pitwall's window. Closing it puts the panel back there too (the core).
 */

// In its own window the panel never folds.
terminalCollapsed.value = false;
followStand();

const projects = computed(() => snapshot.value?.projects ?? []);
/** The panel's project: the one it shows, else the first listed. */
const project = computed(() => projects.value.find((p) => p.path === panelPath.value) ?? projects.value[0] ?? null);
const panel = ref<InstanceType<typeof TerminalPanel> | null>(null);

/** "Always on top", as the core holds it, and here at once when pressed. */
const onTop = ref(false);
watch(
  () => snapshot.value?.settings.terminalPlace?.onTop,
  (on) => (onTop.value = on ?? false),
  { immediate: true },
);

function toggleOnTop(): void {
  const on = !onTop.value;
  onTop.value = on;
  void api.setTerminalsOnTop(on).catch(() => (onTop.value = !on));
}

/** Back into Pitwall's window, where the panel stood before: along the bottom or as a card. */
function bringBack(): void {
  standPanel(snapshot.value?.settings.terminalPlace?.back === "float" ? "float" : "dock");
}

/** A chat's `/config`: Settings › Terminal, where its settings are, in Pitwall's window, which
 * comes up for it. */
function openChatSettings(): void {
  void emitTo("main", "reveal-page", { page: "settings", tab: "terminal" });
  void api.openWindow();
}

/** ⌘T opens a new terminal here; ⌘W closes the tab shown, and with none left the window. */
function onKey(event: KeyboardEvent): void {
  if (terminalChord(event) && event.code === "KeyT") {
    event.preventDefault();
    if (panel.value) void panel.value.openTerminal();
    return;
  }
  if (!closeChord(event) || event.defaultPrevented || document.querySelector("dialog[open]")) return;
  event.preventDefault();
  if (event.repeat) return;
  if (panel.value) panel.value.closeShown();
  else bringBack();
}

/** The window is up: its page fades in, rather than being there at once (the user's word). A
 * hidden window draws no frames, so the fade starts as it shows. */
const shown = ref(false);

const links: Promise<UnlistenFn>[] = [];
let stopKeeping: (() => void) | null = null;

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  // What the main window's panel showed, handed over as this window opened.
  takePanelState(await api.panelState().catch(() => null));
  stopKeeping = keepPanelState();
  // The sessions tell Claude's state in the tabs; the Garage page's reports name its jobs.
  links.push(connectSessions(), connectDeps(), connectAccounts(), listenPanelAsks());
  // A Claude session in one of these terminals brought up (a notification, the popover): its tab.
  links.push(listen<{ path: string; id: number }>("reveal-terminal", (event) => revealTab(event.payload.path, event.payload.id)));
  await Promise.allSettled(links);
  void api.windowReady().finally(() => requestAnimationFrame(() => requestAnimationFrame(() => (shown.value = true))));
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  stopKeeping?.();
  for (const link of links) void link.then((unlisten) => unlisten(), () => undefined);
});
</script>

<template>
  <div :class="['terminals', { shown }]">
    <header class="bar" :data-tauri-drag-region="dragRegion">
      <Icon name="terminal" :size="13" />
      <span class="title">{{ t("terminals.title") }}</span>
      <span class="grow"></span>
      <button type="button" :class="['bar-button', { on: onTop }]" :aria-pressed="onTop" :title="t('terminals.onTopHint')" @click="toggleOnTop">
        <Icon name="pin" :size="12" />
        <span>{{ t("terminals.onTop") }}</span>
      </button>
      <button type="button" class="bar-button" :title="t('terminals.backHint')" @click="bringBack">
        <Icon name="panel-dock" :size="12" />
        <span>{{ t("terminals.back") }}</span>
      </button>
    </header>
    <main class="room">
      <TerminalPanel v-if="project" ref="panel" :project="project" stand="window" @empty="bringBack" @open-settings="openChatSettings" />
      <p v-else class="none">{{ t("terminals.none") }}</p>
    </main>
    <!-- A chat's start asked from here (the panel's "+", an empty pane): its question, as the main
         window shows it (lib/chat/open: chatAsks). -->
    <StartChatDialog v-if="chatAsks.length" :key="chatAsks[0].id" :ask="chatAsks[0]" />
  </div>
</template>

<style scoped>
.terminals {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-app);
  color: var(--text-soft);
}

/* Its page fades in as the window comes up, a little lower at first. */
.terminals > * {
  opacity: 0;
  transform: translateY(6px);
  transition:
    opacity 240ms ease,
    transform 240ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.terminals.shown > * {
  opacity: 1;
  transform: none;
}

@media (prefers-reduced-motion: reduce) {
  .terminals > * {
    transition: none;
  }
}

/* The window's own bar: room for the traffic lights on macOS (its title bar lies over the page),
   the whole bar dragging the window there. */
.bar {
  flex-shrink: 0;
  height: 38px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 82px;
  border-bottom: 1px solid var(--line);
  font-size: 12px;
  color: var(--text-subtle);
}

:root:not([data-platform="mac"]) .bar {
  padding-left: 14px;
}

.title {
  font-weight: 600;
  color: var(--text-strong);
  white-space: nowrap;
}

.grow {
  flex: 1 1 auto;
}

.bar-button {
  height: 24px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.bar-button:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.bar-button.on {
  color: var(--text-lit);
}

/* The panel's room: TerminalPanel lies over all of it. */
.room {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
}

.none {
  margin: 40px auto;
  font-size: 13px;
  color: var(--text-subtle);
}
</style>
