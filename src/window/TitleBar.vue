<script setup lang="ts">
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onBeforeUnmount, onMounted, ref } from "vue";
import Icon from "../components/Icon.vue";
import PitwallGlyph from "../components/PitwallGlyph.vue";
import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { TITLE_BAR } from "../lib/platform";

/**
 * Windows: the main window's title bar, Pitwall's own (the system's is off, `windows.rs`), on the
 * sidebar's ground. The logo, the name and the version, with the way to a new one beside it; the
 * rest of it drags the window and a double click fills the screen, as the system's bar does; and
 * Windows' three buttons at its end. The window's edges, shadow and corners stay the system's.
 */
defineProps<{ version: string | null; next: string | null; updateButton: boolean; installing: boolean }>();
const emit = defineEmits<{ update: [] }>();

const win = getCurrentWindow();
/** Filling the screen: the middle button puts it back, its glyph two boxes. */
const maximized = ref(false);
/** Behind another window: the name and the buttons dim, as the system's do. */
const focused = ref(true);
const unlisten: UnlistenFn[] = [];

/** A button's width, Windows' own. */
const BUTTON = 46;

function minimize(): void {
  void win.minimize();
}

function toggle(): void {
  void win.toggleMaximize();
}

/** Hides the window, as the system's ✕ did (`CloseRequested` in lib.rs). */
function close(): void {
  void win.close();
}

/**
 * A modal dialog makes the bar inert and lays its backdrop over it, so a press there lands on the
 * dialog. One over the bar and outside the dialog's box is the bar's: a button by where it is, the
 * rest a drag (a double press fills the screen). The dialog never hears of it and stays open.
 */
function overBar(event: MouseEvent): boolean {
  const dialog = event.target;
  if (!(dialog instanceof HTMLDialogElement) || !dialog.open || event.clientY >= TITLE_BAR) return false;
  const box = dialog.getBoundingClientRect();
  return event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom;
}

/** The pointer's press: kept from the dialog's backdrop (lib/dialog), its default left alone, or
 * no mouse press would follow. */
function onPointer(event: PointerEvent): void {
  if (overBar(event)) event.stopPropagation();
}

function onPress(event: MouseEvent): void {
  if (!overBar(event)) return;
  event.preventDefault();
  event.stopPropagation();
  if (event.button === 0 && innerWidth - event.clientX > 3 * BUTTON) void (event.detail === 2 ? win.toggleMaximize() : win.startDragging());
}

function onClick(event: MouseEvent): void {
  if (!overBar(event)) return;
  event.preventDefault();
  event.stopPropagation();
  const fromRight = innerWidth - event.clientX;
  if (fromRight <= BUTTON) close();
  else if (fromRight <= 2 * BUTTON) toggle();
  else if (fromRight <= 3 * BUTTON) minimize();
}

onMounted(async () => {
  window.addEventListener("pointerdown", onPointer, true);
  window.addEventListener("mousedown", onPress, true);
  window.addEventListener("click", onClick, true);
  const check = async (): Promise<void> => {
    maximized.value = await win.isMaximized();
  };
  unlisten.push(await win.onResized(() => void check()));
  unlisten.push(await win.onFocusChanged(({ payload }) => (focused.value = payload)));
  await check();
  focused.value = await win.isFocused();
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", onPointer, true);
  window.removeEventListener("mousedown", onPress, true);
  window.removeEventListener("click", onClick, true);
  for (const stop of unlisten) stop();
});
</script>

<template>
  <!-- "deep": the logo, the name and the version drag the window too; the buttons never do. -->
  <header :class="['title-bar', { blurred: !focused }]" data-tauri-drag-region="deep">
    <div class="brand">
      <PitwallGlyph :size="20" lamps="var(--run)" />
      <span class="name">Pitwall</span>
      <span v-if="version" class="version">{{ version }}</span>
      <button
        v-if="updateButton"
        type="button"
        class="update"
        :disabled="installing"
        :aria-busy="installing"
        :aria-label="installing ? t('about.update.installing', { version: next ?? '' }) : t('update.button')"
        :title="installing ? undefined : t('update.button')"
        @click="emit('update')"
      >
        <Spinner v-if="installing" />
        <Icon v-else name="download" :size="13" />
      </button>
    </div>
    <!-- Out of the Tab order, as the system's are; Alt+F4 and Win+↑/↓ do the same from the keys. -->
    <div class="caption">
      <button type="button" tabindex="-1" :aria-label="t('titleBar.minimize')" :title="t('titleBar.minimize')" @click="minimize">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
      </button>
      <button
        type="button"
        tabindex="-1"
        :aria-label="t(maximized ? 'titleBar.restore' : 'titleBar.maximize')"
        :title="t(maximized ? 'titleBar.restore' : 'titleBar.maximize')"
        @click="toggle"
      >
        <svg v-if="maximized" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
          <path d="M2.5 2.5v-.8a1.2 1.2 0 0 1 1.2-1.2h4.6a1.2 1.2 0 0 1 1.2 1.2v4.6a1.2 1.2 0 0 1-1.2 1.2h-.8" />
        </svg>
        <svg v-else width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" rx="1.2" /></svg>
      </button>
      <button type="button" tabindex="-1" class="close" :aria-label="t('titleBar.close')" :title="t('titleBar.close')" @click="close">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="m.5.5 9 9M9.5.5l-9 9" /></svg>
      </button>
    </div>
  </header>
</template>

<style scoped>
/* `TITLE_BAR` tall (lib/platform). Over the Files windows' layer (FilesWindows, 40), under the
   menus a page opens. */
.title-bar {
  position: relative;
  z-index: 45;
  height: 36px;
  display: flex;
  align-items: stretch;
  background: var(--bg-sidebar);
}

.brand {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding-left: 14px;
  color: var(--text-brand);
}

.name {
  font-size: 13px;
  font-weight: 600;
}

.version {
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* The sidebar's update box, a size down for the bar. */
.update {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text);
}

.update:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.caption {
  margin-left: auto;
  display: flex;
}

/* Windows' buttons: the bar's full height, square, the arrow and not the hand. */
.caption button {
  width: 46px;
  height: 100%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: var(--text);
  cursor: default;
}

.caption svg {
  fill: none;
  stroke: currentColor;
  stroke-width: 1;
}

.caption button:hover {
  background: var(--caption-hover);
}

.caption button:active {
  background: var(--caption-press);
}

.caption .close:hover {
  background: var(--caption-close);
  color: var(--caption-close-text);
}

.caption .close:active {
  background: var(--caption-close-press);
  color: var(--caption-close-text);
}

.blurred .name,
.blurred .caption button:not(:hover) {
  color: var(--text-faint);
}
</style>
