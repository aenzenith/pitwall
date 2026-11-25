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
 * ⌃⇥ (Ctrl+Tab on Windows and Linux): every project's terminal tabs, the one the panel shows first
 * and then those last looked at (lib/panel: `recentTabs`), the rest after them. Held, ⇥ goes down
 * and ⇧⇥ up; letting ⌃ go opens the one picked with its project, the keyboard in it (`pick`); ⎋,
 * a press elsewhere or the window losing the keyboard leave things as they were. A quick press
 * shows no list: it goes back to the tab looked at before. Its look is "+"'s menu's (AddMenu). It
 * shows the names the tab row shows and the agent's state, nothing it reads itself. The key is
 * taken before anything in the window hears it, a terminal's shell too.
 */
const props = defineProps<{
  /** The panel's project, and the tab it shows (none: an output, or nothing open). */
  path: string;
  current: number | null;
  /** The panel, which the list stands over. */
  anchor: HTMLElement | null;
}>();
const emit = defineEmits<{ pick: [path: string, id: number] }>();

type Item = { term: TerminalView; path: string; project: string; name: string; when: "now" | number | null };

/** The list shows only once the key has been held this long: a quick press switches unseen. */
const SHOW_AFTER_MS = 160;
const WIDTH = 400;
const ROW = 30;

/** ⌃ is held since ⌃⇥. */
const held = ref(false);
/** The list is drawn. */
const shown = ref(false);
const items = ref<Item[]>([]);
/** The row picked. */
const at = ref(0);
const place = ref<Record<string, string>>({});
const list = ref<HTMLElement | null>(null);
let showing: ReturnType<typeof setTimeout> | undefined;
/** ⌃ is down, as the window last heard: WebKit can hand over a quick ⌃ let go (a flag change)
 * before the ⇥ pressed under it (a key), measured in a bare WKWebView at 10 ms apart. */
let control = false;

const states = computed(() => tabStates(items.value.map((item) => item.term)));

/** The tabs to switch between: the one shown, those looked at from the latest, then the panel's
 * project's others and every other project's, as listed. */
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
  const rest = [...all.values()];
  out.push(...rest.filter((item) => item.path === props.path), ...rest.filter((item) => item.path !== props.path));
  return out;
}

/** Over the panel's terminals, under its tab row; a folded panel's row has it above. */
function measure(): void {
  const box = props.anchor?.getBoundingClientRect();
  const width = Math.min(WIDTH, window.innerWidth - 16);
  const middle = box ? box.left + (box.width - width) / 2 : (window.innerWidth - width) / 2;
  const left = Math.max(8, Math.min(middle, window.innerWidth - width - 8));
  const tall = items.value.length * ROW + 64;
  if (box && box.height >= Math.min(tall, 200) + 48) {
    const top = box.top + 40;
    place.value = { left: `${left}px`, top: `${top}px`, width: `${width}px`, maxHeight: `${Math.max(120, Math.min(box.bottom, window.innerHeight) - top - 12)}px` };
  } else {
    const bottom = window.innerHeight - (box?.top ?? window.innerHeight) + 8;
    place.value = { left: `${left}px`, bottom: `${bottom}px`, width: `${width}px`, maxHeight: `${Math.max(120, window.innerHeight - bottom - 16)}px` };
  }
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
  window.addEventListener("blur", end);
  window.addEventListener("pointerdown", onPress, true);
}

function show(): void {
  clearTimeout(showing);
  if (!held.value || shown.value) return;
  measure();
  shown.value = true;
}

function end(): void {
  clearTimeout(showing);
  held.value = false;
  shown.value = false;
  items.value = [];
  window.removeEventListener("keyup", onKeyUp, true);
  window.removeEventListener("blur", end);
  window.removeEventListener("pointerdown", onPress, true);
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

function swallow(event: KeyboardEvent): void {
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
  if (event.key === "Control") commit();
}

/** The window lost the keyboard: whatever is held there is let go. */
function onBlur(): void {
  control = false;
}

/** A press on a row opens it (⌃ still held, a click would be a right click on macOS); one
 * elsewhere leaves things as they were. */
function onPress(event: PointerEvent): void {
  const row = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-recent]") : null;
  if (row && list.value?.contains(row)) {
    event.preventDefault();
    event.stopPropagation();
    at.value = Number(row.dataset.recent);
    return commit();
  }
  end();
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
  end();
});
</script>

<template>
  <Teleport to="body">
    <div v-if="shown" ref="list" class="recent" role="listbox" :aria-label="t('terminal.recent')" :style="place">
      <div class="head" aria-hidden="true">{{ t("terminal.recent") }}</div>
      <div class="rows">
        <div
          v-for="(item, i) in items"
          :key="item.term.id"
          role="option"
          :aria-selected="i === at"
          :data-recent="i"
          :class="['row', { on: i === at }]"
          @mouseenter="at = i"
        >
          <AgentMark v-if="item.term.kind !== 'shell'" :agent="item.term.kind === 'chat' ? 'claude' : item.term.kind" :size="11" />
          <Icon v-else :name="shellIcon(item.term.shell)" :size="11" class="shell" />
          <span class="name">{{ item.name }}</span>
          <span v-if="item.path !== path" class="project">{{ item.project }}</span>
          <Icon v-if="item.term.pinned" name="pin" :size="11" class="pin" :aria-label="t('terminal.pinnedTip')" />
          <span class="grow"></span>
          <ClaudeDot v-if="states.has(item.term.id)" :live="states.get(item.term.id)" />
          <span class="when">{{ when(item) }}</span>
        </div>
      </div>
      <div class="foot" aria-hidden="true">
        <span v-for="(each, i) in footer" :key="i" class="hint">
          <kbd>{{ each.keys }}</kbd> {{ each.text }}
        </span>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.recent {
  position: fixed;
  z-index: 80;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
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

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 30px;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
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

.project {
  flex-shrink: 1;
  min-width: 0;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
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
