<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import AgentMark from "../components/AgentMark.vue";
import ClaudeDot from "../components/ClaudeDot.vue";
import Icon from "../components/Icon.vue";
import { ago } from "../lib/format";
import { t } from "../lib/i18n";
import { recentTabs } from "../lib/panel";
import { keys } from "../lib/platform";
import { shellIcon } from "../lib/shells";
import { now, snapshot } from "../lib/store";
import { tabStates } from "../lib/tabStates";
import { distinctNames } from "../lib/tabs";
import type { TerminalView } from "../lib/types";

/**
 * ⌃⇥ (Ctrl+Tab on Windows and Linux): every project's terminal tabs, the panel's project's first
 * (the one the panel shows, then those last looked at, lib/panel: `recentTabs`, then the rest),
 * then, under a line, the other projects' in the same order. Each row has its project under its
 * name. Held, ⇥ goes down and ⇧⇥ up; letting ⌃ go, or a click on a row, opens the one picked with
 * its project, the keyboard in it (`pick`); ⎋, a press elsewhere or the window losing the keyboard
 * leave things as they were. A quick press shows no list: it goes to the row under the tab shown.
 * The list stands in the middle of the window, which it dims, and comes in as the quick switcher
 * does. Its look is "+"'s menu's (AddMenu). It shows the names the tab row shows and the agent's
 * state, nothing it reads itself. The key is taken before anything in the window hears it, a
 * terminal's shell too.
 */
const props = defineProps<{
  /** The panel's project, and the tab it shows (none: an output, or nothing open). */
  path: string;
  current: number | null;
}>();
const emit = defineEmits<{ pick: [path: string, id: number] }>();

type Item = { term: TerminalView; path: string; project: string; name: string; when: "now" | number | null };

/** The list shows only once the key has been held this long: a quick press switches unseen. */
const SHOW_AFTER_MS = 160;
/** What a press sets off after its pointerdown, all of it swallowed until the press is over. */
const PRESS_EVENTS = ["mousedown", "mouseup", "click", "auxclick", "dblclick", "contextmenu"] as const;

/** ⌃ is held since ⌃⇥. */
const held = ref(false);
/** The list is drawn. */
const shown = ref(false);
const items = ref<Item[]>([]);
/** The row picked. */
const at = ref(0);
const list = ref<HTMLElement | null>(null);
let showing: ReturnType<typeof setTimeout> | undefined;
/** ⌃ is down, as the window last heard: WebKit can hand over a quick ⌃ let go (a flag change)
 * before the ⇥ pressed under it (a key), measured in a bare WKWebView at 10 ms apart. */
let control = false;
/** A press under way since its pointerdown, and what its pointerup does: open the row pressed or
 * leave things as they were. */
let pressing: "open" | "leave" | null = null;

const states = computed(() => tabStates(items.value.map((item) => item.term)));

/** The tabs to switch between: the panel's project's, then every other project's; of each, the
 * one shown, those looked at from the latest, then the rest as listed. */
function gather(): Item[] {
  const all = new Map<number, Item>();
  for (const project of snapshot.value?.projects ?? []) {
    const names = distinctNames(project.terminals);
    for (const term of project.terminals) all.set(term.id, { term, path: project.path, project: project.name, name: names.get(term.id) ?? term.name, when: null });
  }
  const out: Item[] = [];
  const take = (id: number, when: Item["when"]): void => {
    const item = all.get(id);
    if (!item) return;
    all.delete(id);
    out.push({ ...item, when });
  };
  if (props.current !== null) take(props.current, "now");
  for (const each of recentTabs.value) take(each.id, each.at);
  out.push(...all.values());
  return [...out.filter((item) => item.path === props.path), ...out.filter((item) => item.path !== props.path)];
}

function begin(back: boolean): void {
  const found = gather();
  const first = found[0]?.when === "now" ? 1 : 0;
  // Nothing to go to: the key does nothing.
  if (found.length <= first) return;
  items.value = found;
  at.value = back ? found.length - 1 : first;
  held.value = true;
  // ⌃ let go already: a quick press, which switches at once.
  if (!control) return commit();
  showing = setTimeout(show, SHOW_AFTER_MS);
  window.addEventListener("keyup", onKeyUp, true);
  window.addEventListener("blur", onLost);
  window.addEventListener("pointerdown", onPress, true);
}

function show(): void {
  clearTimeout(showing);
  if (!held.value || shown.value) return;
  shown.value = true;
}

function end(): void {
  clearTimeout(showing);
  held.value = false;
  shown.value = false;
  items.value = [];
  window.removeEventListener("keyup", onKeyUp, true);
  window.removeEventListener("blur", onLost);
  window.removeEventListener("pointerdown", onPress, true);
}

/** The window lost the keyboard while the list was up: a press under way is let go with it. */
function onLost(): void {
  unguard();
  end();
}

/** The row picked is opened. */
function commit(): void {
  const item = items.value[at.value];
  end();
  if (item) emit("pick", item.path, item.term.id);
}

function step(by: number): void {
  const count = items.value.length;
  at.value = (at.value + by + count) % count;
  show();
}

function swallow(event: Event): void {
  event.preventDefault();
  event.stopPropagation();
}

/** Taken as the window hears it, before a terminal or a field does. */
function onKeyDown(event: KeyboardEvent): void {
  if (event.key === "Control") control = true;
  const tab = event.key === "Tab" && event.ctrlKey && !event.metaKey && !event.altKey;
  if (!held.value) {
    if (!tab || document.querySelector("dialog[open]")) return;
    swallow(event);
    begin(event.shiftKey);
    return;
  }
  if (event.key === "Control" || event.key === "Shift") return;
  // ⌃ was let go where the window didn't hear it: what is picked opens.
  if (!event.ctrlKey) return commit();
  if (tab) {
    swallow(event);
    return step(event.shiftKey ? -1 : 1);
  }
  switch (event.key) {
    case "ArrowDown":
      step(1);
      break;
    case "ArrowUp":
      step(-1);
      break;
    case "Enter":
      commit();
      break;
    case "Escape":
      end();
      break;
    default:
      // Any other key with ⌃ is the terminal's: the list goes, nothing switches.
      return end();
  }
  swallow(event);
}

function onControlUp(event: KeyboardEvent): void {
  if (event.key === "Control") control = false;
}

function onKeyUp(event: KeyboardEvent): void {
  // ⌃ let go in the middle of a press: the press's end says what happens.
  if (event.key === "Control" && !pressing) commit();
}

/** The window lost the keyboard: whatever is held there is let go. */
function onBlur(): void {
  control = false;
}

/** A press on a row opens it, one elsewhere leaves things as they were, each once the press is
 * over: the list stays drawn until then and nothing under it hears the press. With ⌃ held, macOS
 * makes the press a right click, whose context menu WebKit aims after the pointerdown, by a hit
 * test of its own; a list gone by then let what stood under it take the menu, a word of it
 * selected. Elsewhere is the scrim under the list, which takes the menu as it does the press. */
function onPress(event: PointerEvent): void {
  swallow(event);
  if (pressing) return;
  const row = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-recent]") : null;
  const open = !!row && !!list.value?.contains(row);
  if (row && open) at.value = Number(row.dataset.recent);
  pressing = open ? "open" : "leave";
  for (const type of PRESS_EVENTS) window.addEventListener(type, swallow, true);
  window.addEventListener("pointerup", onRelease, true);
  window.addEventListener("pointercancel", onRelease, true);
}

/** The press is over: the mouseup and click that follow its pointerup are swallowed still, then
 * the window hears presses again. */
function onRelease(event: PointerEvent): void {
  swallow(event);
  const open = pressing === "open" && event.type === "pointerup";
  window.removeEventListener("pointerup", onRelease, true);
  window.removeEventListener("pointercancel", onRelease, true);
  setTimeout(unguard);
  if (!held.value) return;
  if (open) commit();
  else end();
}

function unguard(): void {
  pressing = null;
  for (const type of PRESS_EVENTS) window.removeEventListener(type, swallow, true);
  window.removeEventListener("pointerup", onRelease, true);
  window.removeEventListener("pointercancel", onRelease, true);
}

watch(at, () => void nextTick(() => list.value?.querySelector(".on")?.scrollIntoView({ block: "nearest" })));

/** When it was last looked at: now for the tab shown, else how long ago. */
function when(item: Item): string {
  if (item.when === "now") return t("terminal.recentNow");
  return item.when === null ? "" : ago(item.when, now.value);
}

const footer = computed(() => [
  { keys: keys("Tab"), text: t("terminal.recentNext") },
  { keys: keys("Shift+Tab"), text: t("terminal.recentPrevious") },
  { keys: keys("Escape"), text: t("terminal.recentCancel") },
  { keys: keys("Ctrl"), text: t("terminal.recentOpen") },
]);

onMounted(() => {
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onControlUp, true);
  window.addEventListener("blur", onBlur);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeyDown, true);
  window.removeEventListener("keyup", onControlUp, true);
  window.removeEventListener("blur", onBlur);
  unguard();
  end();
});
</script>

<template>
  <Teleport to="body">
    <!-- Over the whole window, dimmed, the list in its middle: a press beside the list lands here,
         never on what stands under it (onPress). It comes in as the quick switcher does and goes at
         once. -->
    <div v-if="shown" class="scrim">
      <div ref="list" class="recent" role="listbox" :aria-label="t('terminal.recent')">
        <div class="head" aria-hidden="true">{{ t("terminal.recent") }}</div>
        <div class="rows">
          <template v-for="(item, i) in items" :key="item.term.id">
            <!-- The line between the panel's project's tabs and the others'. -->
            <div v-if="i > 0 && item.path !== path && items[i - 1].path === path" class="sep" aria-hidden="true"></div>
            <div
              role="option"
              :aria-selected="i === at"
              :data-recent="i"
              :class="['row', { on: i === at }]"
              @mouseenter="at = i"
            >
              <div class="line">
                <AgentMark v-if="item.term.kind !== 'shell'" :agent="item.term.kind === 'chat' ? 'claude' : item.term.kind" :size="11" />
                <Icon v-else :name="shellIcon(item.term.shell)" :size="11" class="shell" />
                <span class="name">{{ item.name }}</span>
                <Icon v-if="item.term.pinned" name="pin" :size="11" class="pin" :aria-label="t('terminal.pinnedTip')" />
                <span class="grow"></span>
                <ClaudeDot v-if="states.has(item.term.id)" :live="states.get(item.term.id)" />
                <span class="when">{{ when(item) }}</span>
              </div>
              <div class="project">{{ item.project }}</div>
            </div>
          </template>
        </div>
        <div class="foot" aria-hidden="true">
          <span v-for="(each, i) in footer" :key="i" class="hint">
            <kbd>{{ each.keys }}</kbd> {{ each.text }}
          </span>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.recent {
  position: relative;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  width: min(400px, 100vw - 16px);
  max-height: calc(100vh - 32px);
  padding: 5px;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
  user-select: none;
  cursor: default;
}

.head {
  padding: 6px 10px 4px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.07em;
  text-transform: uppercase;
  color: var(--text-faint);
}

.rows {
  min-height: 0;
  overflow-y: auto;
}

.scrim {
  position: fixed;
  inset: 0;
  z-index: 80;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* The dim, the chat's light one (ChatView's `.scrim`); on Windows it stops under Pitwall's own
   title bar, as a dialog's does (tokens.css: dialog::backdrop). */
.scrim::before {
  content: "";
  position: absolute;
  inset: 0;
  background: var(--scrim-soft);
}

:root[data-platform="windows"] .scrim::before {
  top: 36px;
}

/* In as the quick switcher's panel comes (Switcher.vue: `.panel.shown`), the dim fading in with
   it; out at once. */
.scrim::before {
  animation: recent-fade 160ms ease-out;
}

.recent {
  transform-origin: 50% 0;
  animation:
    recent-fade 160ms ease-out,
    recent-rise 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .recent {
    animation: recent-fade 160ms ease-out;
  }
}

@keyframes recent-fade {
  from {
    opacity: 0;
  }
}

@keyframes recent-rise {
  from {
    transform: translateY(10px) scale(0.985);
  }
}

.row {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1px;
  height: 40px;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
}

/* AddMenu's line between its parts. */
.sep {
  height: 1px;
  margin: 5px 6px;
  background: var(--line-strong);
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.row.on {
  background: var(--bg-lit);
}

.shell {
  flex-shrink: 0;
  color: var(--text-nav);
}

.name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row.on .name {
  color: var(--text-strong);
}

/* Under the name, from where the name starts (the mark's 11 px and the gap's 8). */
.project {
  min-width: 0;
  padding-left: 19px;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 11.5px;
  color: var(--text-faint);
}

.pin {
  flex-shrink: 0;
  color: var(--text-faint);
}

.grow {
  flex: 1 1 auto;
}

.when {
  flex-shrink: 0;
  font-size: 11.5px;
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}

.foot {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin-top: 5px;
  padding: 7px 10px 4px;
  border-top: 1px solid var(--line-strong);
  font-size: 11.5px;
  color: var(--text-faint);
}

kbd {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}
</style>
