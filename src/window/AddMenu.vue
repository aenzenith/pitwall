<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import AgentMark from "../components/AgentMark.vue";
import Icon from "../components/Icon.vue";
import { accountName, accounts, severalAccounts } from "../lib/accounts";
import { agentOn } from "../lib/agents";
import { t } from "../lib/i18n";
import { isMac } from "../lib/platform";
import { SHELL_LABELS, shellIcon } from "../lib/shells";
import { CODEX_MODE_LABELS, CODEX_MODELS, CODEX_MODES, MODE_LABELS, MODEL_LABELS, MODELS, MODES, projectChoice, projectCodexChoice } from "../lib/start";
import { codexAccountName, codexAccounts, severalCodexAccounts } from "../lib/terminals";
import type { Agent, AgentStartChoice, Project, Shell, ShellChoice } from "../lib/types";

/**
 * The terminal panel's "+": Pitwall's own menu, since a row of the system's can't both open at a
 * click and open a submenu. A row for a new tab, then one for each agent that is on ("New Claude",
 * "New Codex"); a click on a row opens it at once as the project's settings say. The `›` at a
 * row's end (hovered, or → from the keyboard) opens its details to the right: an agent's model,
 * mode and, with more than one, account, each opening at once with that one change; on Windows
 * the shells, then each as an administrator's. Its look is the board's give menu's.
 */
const props = defineProps<{
  project: Project;
  /** The shells installed (Windows alone; none elsewhere). */
  shells: Shell[];
}>();

const emit = defineEmits<{
  /** A shell tab: the shell of the list picked, or (none) the default one. */
  shell: [choice: ShellChoice | null];
  /** An agent's tab, with its start (none: Claude as the project's settings say, as today) and,
   * when one was picked, its account. */
  agent: [start: AgentStartChoice | null, account: string | null];
}>();

type SubItem = {
  kind: "item";
  key: string;
  label: string;
  mono?: boolean;
  icon?: "terminal" | "powershell" | "shield";
  tone?: "nav" | "link" | "warn";
  checked: boolean;
  enabled: boolean;
  run: () => void;
};
type SubEntry = { kind: "head"; key: string; label: string } | { kind: "sep"; key: string } | SubItem;
type Row = { id: "shell" | Agent; label: string; keys: string | null; sub: SubEntry[] | null; run: () => void };

const open = ref(false);
/** The row under the pointer or the keyboard. */
const active = ref(0);
/** The row whose details show, and the item of them under the pointer or the keyboard. */
const subOf = ref<number | null>(null);
const subActive = ref(-1);
/** The keyboard is in the details. */
const inSub = ref(false);
const list = ref<HTMLElement | null>(null);
const subList = ref<HTMLElement | null>(null);
const place = ref<Record<string, string>>({});
const subPlace = ref<Record<string, string>>({});
/** The button it opened from, and whether the keyboard opened it (it then goes back there). */
let anchor: HTMLElement | null = null;
let keyedOpen = false;
let leaving: ReturnType<typeof setTimeout> | undefined;

const MAIN_WIDTH = 240;
const newTabKeys = isMac ? "⌘T" : "Ctrl+Shift+T";

function head(key: string, label: string): SubEntry {
  return { kind: "head", key, label };
}

function sep(key: string): SubEntry {
  return { kind: "sep", key };
}

/** Claude's details: the models (the project's own checked), the modes, the accounts. */
function claudeSub(): SubEntry[] {
  const { model, mode } = projectChoice(props.project);
  const start = (next: Partial<{ model: typeof model; mode: typeof mode }>) => () => emit("agent", { agent: "claude", model, mode, ...next }, null);
  const entries: SubEntry[] = [
    head("mh", t("start.model")),
    { kind: "item", key: "auto", label: t("start.model.auto"), checked: model === null, enabled: true, run: start({ model: null }) },
    ...MODELS.map((each): SubEntry => ({ kind: "item", key: each, label: t(MODEL_LABELS[each]), checked: model === each, enabled: true, run: start({ model: each }) })),
    sep("s1"),
    head("kh", t("start.mode")),
    ...MODES.map((each): SubEntry => ({ kind: "item", key: `mode-${each}`, label: t(MODE_LABELS[each]), checked: mode === each, enabled: true, run: start({ mode: each }) })),
  ];
  if (severalAccounts.value) {
    entries.push(
      sep("s2"),
      head("hh", t("terminal.account")),
      ...accounts.value.map((account): SubEntry => ({
        kind: "item",
        key: `account-${account.id}`,
        label: accountName(account),
        checked: account.inUse,
        enabled: !account.missing,
        run: () => emit("agent", null, account.id),
      })),
    );
  }
  return entries;
}

/** Codex's details, the same way: its models by their ids, Normal or read-only, its accounts. */
function codexSub(): SubEntry[] {
  const { model, mode } = projectCodexChoice(props.project);
  const start = (next: Partial<{ model: typeof model; mode: typeof mode }>) => () => emit("agent", { agent: "codex", model, mode, ...next }, null);
  const entries: SubEntry[] = [
    head("mh", t("start.model")),
    { kind: "item", key: "auto", label: t("start.codexModel.auto"), checked: model === null, enabled: true, run: start({ model: null }) },
    ...CODEX_MODELS.map((each): SubEntry => ({ kind: "item", key: each, label: each, mono: true, checked: model === each, enabled: true, run: start({ model: each }) })),
    sep("s1"),
    head("kh", t("start.mode")),
    ...CODEX_MODES.map((each): SubEntry => ({ kind: "item", key: `mode-${each}`, label: t(CODEX_MODE_LABELS[each]), checked: mode === each, enabled: true, run: start({ mode: each }) })),
  ];
  if (severalCodexAccounts.value) {
    entries.push(
      sep("s2"),
      head("hh", t("terminal.account")),
      ...codexAccounts.value.map((account): SubEntry => ({
        kind: "item",
        key: `account-${account.id}`,
        label: codexAccountName(account),
        checked: account.inUse,
        enabled: !account.missing,
        run: () => emit("agent", { agent: "codex", model, mode }, account.id),
      })),
    );
  }
  return entries;
}

/** On Windows the shells installed (the one a new tab opens checked), then each as an
 * administrator's under its own head. */
function shellSub(): SubEntry[] {
  const item = (shell: Shell, elevated: boolean): SubEntry => ({
    kind: "item",
    key: `${elevated ? "admin" : "shell"}-${shell}`,
    label: t(SHELL_LABELS[shell]),
    icon: elevated ? "shield" : shellIcon(shell),
    tone: elevated ? "warn" : shellIcon(shell) === "powershell" ? "link" : "nav",
    checked: !elevated && shell === "cmd",
    enabled: true,
    run: () => emit("shell", { shell, elevated }),
  });
  return [...props.shells.map((shell) => item(shell, false)), sep("s1"), head("ah", t("terminal.asAdmin")), ...props.shells.map((shell) => item(shell, true))];
}

// Worked out again each time the menu opens: the project's settings or the accounts may have
// changed since.
const rows = computed<Row[]>(() => {
  void open.value;
  const made: Row[] = [{ id: "shell", label: t("terminal.newTab"), keys: newTabKeys, sub: props.shells.length ? shellSub() : null, run: () => emit("shell", null) }];
  if (agentOn("claude")) {
    made.push({ id: "claude", label: t("terminal.newClaude"), keys: null, sub: claudeSub(), run: () => emit("agent", null, null) });
  }
  if (agentOn("codex")) {
    made.push({ id: "codex", label: t("terminal.newCodex"), keys: null, sub: codexSub(), run: () => emit("agent", projectCodexChoice(props.project), null) });
  }
  return made;
});

const subEntries = computed<SubEntry[]>(() => (subOf.value === null ? [] : (rows.value[subOf.value]?.sub ?? [])));
const subWidth = computed(() => (rows.value[subOf.value ?? 0]?.id === "shell" ? 240 : 220));
/** The items of the details the keyboard moves over, by their place among the entries. */
const subItems = computed(() => subEntries.value.flatMap((entry, at) => (entry.kind === "item" && entry.enabled ? [at] : [])));

/** Opens the menu under `anchor` (above it when there is no room below); `keyed`: from the
 * keyboard, which then moves in it. */
function show(from: HTMLElement, keyed = false): void {
  if (open.value) return hide();
  const box = from.getBoundingClientRect();
  anchor = from;
  keyedOpen = keyed;
  const tall = rows.value.length * 30 + 12;
  const below = window.innerHeight - box.bottom - 8 >= tall;
  const left = Math.max(8, Math.min(box.left, window.innerWidth - MAIN_WIDTH - 8));
  place.value = below ? { left: `${left}px`, top: `${box.bottom + 4}px` } : { left: `${left}px`, bottom: `${window.innerHeight - box.top + 4}px` };
  open.value = true;
  active.value = 0;
  closeSub();
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => list.value?.focus());
}

function hide(): void {
  open.value = false;
  closeSub();
  clearTimeout(leaving);
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

/** Closed from the keyboard: it goes back to the button. */
function close(): void {
  hide();
  if (keyedOpen) anchor?.focus();
}

/** A press outside closes it; one on its own button is that button's click, which closes it. */
function onOutside(event: PointerEvent): void {
  const target = event.target;
  if (target instanceof Node && (list.value?.contains(target) || subList.value?.contains(target) || anchor?.contains(target))) return;
  hide();
}

/** Shows row `at`'s details beside it, kept inside the window. */
function openSub(at: number, keyed = false): void {
  clearTimeout(leaving);
  const row = rows.value[at];
  if (!row?.sub) return closeSub();
  if (subOf.value !== at) {
    subOf.value = at;
    subActive.value = -1;
    const item = list.value?.querySelectorAll<HTMLElement>(".mi")[at]?.getBoundingClientRect();
    const main = list.value?.getBoundingClientRect();
    if (item && main) {
      const width = subWidth.value;
      const right = main.right - 4 + width <= window.innerWidth - 8;
      const left = right ? main.right - 4 : Math.max(8, main.left - width + 4);
      subPlace.value = { left: `${left}px`, top: `${item.top - 6}px`, width: `${width}px` };
      // Kept above the window's bottom once its height is known.
      void nextTick(() => {
        const tall = subList.value?.offsetHeight ?? 0;
        const top = Math.max(8, Math.min(item.top - 6, window.innerHeight - tall - 8));
        subPlace.value = { ...subPlace.value, top: `${top}px` };
      });
    }
  }
  inSub.value = keyed;
  if (keyed) {
    const checked = subItems.value.find((index) => (subEntries.value[index] as SubItem).checked);
    subActive.value = checked ?? subItems.value[0] ?? -1;
    void nextTick(() => subList.value?.focus());
  }
}

function closeSub(): void {
  subOf.value = null;
  subActive.value = -1;
  inSub.value = false;
}

/** The pointer left the details' row for another: they go a moment later, unless it reaches
 * them first (it may cross a row on its way). */
function hoverRow(at: number): void {
  active.value = at;
  if (subOf.value === null || subOf.value === at) return;
  clearTimeout(leaving);
  leaving = setTimeout(closeSub, 220);
}

/** The pointer reached the details: they stay. */
function keepSub(): void {
  clearTimeout(leaving);
}

function pick(at: number): void {
  const row = rows.value[at];
  if (!row) return;
  close();
  row.run();
}

function pickSub(at: number): void {
  const entry = subEntries.value[at];
  if (entry?.kind !== "item" || !entry.enabled) return;
  close();
  entry.run();
}

function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const count = rows.value.length;
  const items = subItems.value;
  const here = items.indexOf(subActive.value);
  switch (event.key) {
    case "ArrowDown":
      if (inSub.value) subActive.value = items[Math.min(items.length - 1, here + 1)] ?? subActive.value;
      else active.value = (active.value + 1) % count;
      break;
    case "ArrowUp":
      if (inSub.value) subActive.value = items[Math.max(0, here - 1)] ?? subActive.value;
      else active.value = (active.value - 1 + count) % count;
      break;
    case "Home":
      if (inSub.value) subActive.value = items[0] ?? -1;
      else active.value = 0;
      break;
    case "End":
      if (inSub.value) subActive.value = items[items.length - 1] ?? -1;
      else active.value = count - 1;
      break;
    case "ArrowRight":
      if (!inSub.value) openSub(active.value, true);
      break;
    case "ArrowLeft":
      if (inSub.value) {
        closeSub();
        void nextTick(() => list.value?.focus());
      }
      break;
    case "Enter":
    case " ":
      if (inSub.value) pickSub(subActive.value);
      else pick(active.value);
      break;
    case "Escape":
      if (inSub.value) {
        closeSub();
        void nextTick(() => list.value?.focus());
      } else close();
      break;
    case "Tab":
      close();
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

defineExpose({ show, hide, open });

onBeforeUnmount(hide);
</script>

<template>
  <Teleport to="body">
    <template v-if="open">
      <div
        ref="list"
        class="menu"
        role="menu"
        tabindex="-1"
        :aria-label="t('terminal.newTab')"
        :style="{ ...place, width: `${MAIN_WIDTH}px` }"
        @keydown="onKey"
        @mousedown.prevent
      >
        <div
          v-for="(row, i) in rows"
          :key="row.id"
          role="menuitem"
          :aria-haspopup="row.sub ? 'menu' : undefined"
          :aria-expanded="row.sub ? subOf === i : undefined"
          :class="['mi', { hv: active === i || subOf === i, plain: !row.sub }]"
          @mouseenter="hoverRow(i)"
          @click="pick(i)"
        >
          <Icon v-if="row.id === 'shell'" name="terminal" :size="14" class="mark-shell" />
          <AgentMark v-else :agent="row.id" :size="14" />
          <span class="lb">{{ row.label }}</span>
          <span v-if="row.keys" class="kb">{{ row.keys }}</span>
          <span v-if="row.sub" class="ch" @mouseenter="openSub(i)" @click.stop="subOf === i ? closeSub() : openSub(i)">
            <Icon name="chevron" :size="13" />
          </span>
        </div>
      </div>
      <div
        v-if="subOf !== null"
        ref="subList"
        class="menu"
        role="menu"
        tabindex="-1"
        :aria-label="rows[subOf]?.label"
        :style="subPlace"
        @keydown="onKey"
        @mousedown.prevent
        @mouseenter="keepSub"
      >
        <template v-for="(entry, j) in subEntries" :key="entry.key">
          <div v-if="entry.kind === 'head'" class="mh" aria-hidden="true">{{ entry.label }}</div>
          <div v-else-if="entry.kind === 'sep'" class="msep" role="separator"></div>
          <div
            v-else
            role="menuitemradio"
            :aria-checked="entry.checked"
            :aria-disabled="!entry.enabled || undefined"
            :class="['mi', 'plain', 'si', { hv: subActive === j, off: !entry.enabled }]"
            @mouseenter="entry.enabled && (subActive = j)"
            @mouseleave="subActive === j && (subActive = -1)"
            @click="pickSub(j)"
          >
            <span class="ck"><Icon v-if="entry.checked" name="check" :size="13" /></span>
            <Icon v-if="entry.icon" :name="entry.icon" :size="14" :class="`tone-${entry.tone}`" />
            <span :class="['lb', { mono: entry.mono }]">{{ entry.label }}</span>
          </div>
        </template>
      </div>
    </template>
  </Teleport>
</template>

<style scoped>
.menu {
  position: fixed;
  z-index: 70;
  box-sizing: border-box;
  padding: 5px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
  user-select: none;
}

.menu:focus-visible {
  outline: none;
}

.mi {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 30px;
  flex-shrink: 0;
  padding: 0 0 0 10px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: default;
}

.mi.plain {
  padding-right: 10px;
}

.mi.hv {
  background: var(--bg-lit);
}

.mi.off {
  opacity: 0.45;
}

.lb {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lb.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.kb {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  padding-right: 10px;
}

/* The details' arrow: its own part of the row, set off by a line while the row is hovered. */
.ch {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 30px;
  flex-shrink: 0;
  color: var(--text-muted);
  border-left: 1px solid transparent;
}

.mi.hv .ch {
  border-left-color: var(--line-card-on);
}

.mark-shell,
.tone-nav {
  flex-shrink: 0;
  color: var(--text-nav);
}

.tone-link {
  flex-shrink: 0;
  color: var(--text-link);
}

.tone-warn {
  flex-shrink: 0;
  color: var(--warn);
}

.mh {
  padding: 8px 10px 4px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.07em;
  text-transform: uppercase;
  color: var(--text-faint);
}

.msep {
  height: 1px;
  flex-shrink: 0;
  margin: 5px 6px;
  background: var(--line-strong);
}

.si {
  padding-left: 8px;
}

.ck {
  width: 14px;
  flex-shrink: 0;
  display: inline-flex;
  justify-content: center;
  color: var(--run);
}

.ck :deep(svg) {
  stroke-width: 2.4;
}
</style>
