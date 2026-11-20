<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import AgentMark from "../components/AgentMark.vue";
import Icon from "../components/Icon.vue";
import { agentOn } from "../lib/agents";
import { phase } from "../lib/format";
import { t } from "../lib/i18n";
import { isMac, isWindows } from "../lib/platform";
import { api, snapshot } from "../lib/store";
import type { Agent, Folder, Project } from "../lib/types";

/**
 * A right click on a row of the terminals' project column (TerminalProjects): Pitwall's own menu,
 * since its rows carry the states the list's own do (the dev server's, each command's). For a
 * listed project: its dev server under a head that says how it is, its commands (a running one's
 * click stops it; one marked "ask first" asks on the row, a second click runs it) and its links,
 * each in a list to the right, a new tab of the terminal, its files, its editor and Finder, its
 * star, its notifications, its settings and, when it was added, its removal. For a folder of the
 * projects folder that isn't listed: added and shown, added with a star, its editor and Finder.
 */
const emit = defineEmits<{
  /** The dev server's output, in the panel. */
  output: [path: string];
  /** A new tab of the project's: a shell, a chat or an agent's session, as "+" opens them. */
  open: [path: string, kind: "shell" | "chat" | Agent];
  files: [path: string];
  settings: [path: string];
  newCommand: [path: string];
  /** A project to show in the panel (a folder just added). */
  pick: [path: string];
}>();

type Target = { kind: "project"; path: string } | { kind: "folder"; folder: Folder };
type Dot = "on" | "bad" | "busy" | "idle";
type Item = {
  kind: "item";
  key: string;
  label: string;
  icon?: "external" | "terminal" | "restart" | "stop" | "play" | "files" | "editor" | "folder-open" | "star" | "star-filled" | "bell" | "bell-off" | "settings" | "trash" | "plus" | "chat" | "link" | "pulse";
  agent?: Agent;
  keys?: string;
  /** Its list to the right. */
  sub?: SubEntry[];
  enabled: boolean;
  danger?: boolean;
  run?: () => void;
};
type Entry = { kind: "head"; key: string; label: string; dot: Dot; meta?: string } | { kind: "sep"; key: string } | Item;
type SubItem = {
  kind: "item";
  key: string;
  label: string;
  dot?: Dot;
  /** A running command's: its click stops it. */
  stop?: boolean;
  icon?: "link" | "plus" | "terminal" | "chat";
  agent?: Agent;
  meta?: string;
  enabled: boolean;
  /** Its click leaves the menu open (the first click on an "ask first" command). */
  stays?: boolean;
  run: () => void;
};
type SubEntry = { kind: "sep"; key: string } | SubItem;

const target = ref<Target | null>(null);
const open = computed(() => target.value !== null);
const active = ref(-1);
const subOf = ref<number | null>(null);
const subActive = ref(-1);
const inSub = ref(false);
/** The "ask first" command its row asks about; a second click runs it. */
const asking = ref<string | null>(null);
const list = ref<HTMLElement | null>(null);
const subList = ref<HTMLElement | null>(null);
const place = ref<Record<string, string>>({});
const subPlace = ref<Record<string, string>>({});
let leaving: ReturnType<typeof setTimeout> | undefined;

const WIDTH = 248;
const SUB_WIDTH = 240;

/** The project as the snapshot has it now: the menu follows its changes while it is open. */
const project = computed<Project | null>(() => {
  const at = target.value;
  if (at?.kind !== "project") return null;
  return snapshot.value?.projects.find((p) => p.path === at.path) ?? null;
});

const revealLabel = computed(() => t(isMac ? "files.reveal.mac" : isWindows ? "files.reveal.windows" : "files.reveal.linux"));

function sep(key: string): Entry & SubEntry {
  return { kind: "sep", key };
}

function commandDot(status: Project["commands"][number]["status"]): Dot {
  if (status === "running") return "on";
  if (status === "failed") return "bad";
  if (status === "busy") return "busy";
  return "idle";
}

/** The project's commands, then "New command…". */
function commandsSub(p: Project): SubEntry[] {
  const made: SubEntry[] = p.commands.map((command): SubItem => {
    const running = command.status === "running";
    const asks = command.confirm && !running && asking.value === command.id;
    return {
      kind: "item",
      key: `command-${command.id}`,
      label: asks ? t("projectMenu.runAgain", { name: command.name }) : command.name,
      dot: commandDot(command.status),
      stop: running,
      enabled: command.status !== "busy",
      stays: command.confirm && !running && !asks,
      run: () => {
        if (running) void api.stopCommand(p.path, command.id);
        // `asks` as the row was drawn: picking it closes the menu first, which forgets `asking`.
        else if (command.confirm && !asks) asking.value = command.id;
        else void api.runCommand(p.path, command.id);
      },
    };
  });
  if (made.length) made.push(sep("cs"));
  made.push({ kind: "item", key: "new-command", label: t("projectMenu.newCommand"), icon: "plus", enabled: true, run: () => emit("newCommand", p.path) });
  return made;
}

/** The project's links, each opened in the browser. */
function linksSub(p: Project): SubEntry[] {
  return (p.settings.links ?? []).map((link, at): SubItem => ({
    kind: "item",
    key: `link-${at}`,
    label: link.name || link.url,
    icon: "link",
    enabled: true,
    run: () => void api.openUrl(link.url),
  }));
}

/** A new tab: a shell, then a chat and each agent that is on, as "+" lists them. */
function terminalSub(p: Project): SubEntry[] {
  const made: SubEntry[] = [{ kind: "item", key: "shell", label: t("terminal.newTab"), icon: "terminal", enabled: true, run: () => emit("open", p.path, "shell") }];
  if (agentOn("claude")) {
    made.push({ kind: "item", key: "chat", label: t("chat.new"), icon: "chat", enabled: true, run: () => emit("open", p.path, "chat") });
    made.push({ kind: "item", key: "claude", label: t("terminal.newClaude"), agent: "claude", enabled: true, run: () => emit("open", p.path, "claude") });
  }
  if (agentOn("codex")) made.push({ kind: "item", key: "codex", label: t("terminal.newCodex"), agent: "codex", enabled: true, run: () => emit("open", p.path, "codex") });
  return made;
}

function serverDot(p: Project): Dot {
  if (p.status === "running") return "on";
  if (p.status === "crashed") return "bad";
  if (p.status === "busy") return "busy";
  return "idle";
}

function projectEntries(p: Project): Entry[] {
  const busy = p.status === "busy";
  // A server a VS Code window runs is that window's: it is shown, never started or stopped here.
  const own = !p.owner;
  const head: Entry = {
    kind: "head",
    key: "server",
    label: busy ? phase(p) : t(`status.${p.status}`),
    dot: serverDot(p),
    meta: p.status === "running" && p.port ? `:${p.port}` : undefined,
  };
  const server: Entry[] = [];
  if (p.status === "running") {
    server.push(
      { kind: "item", key: "browser", label: t("common.openInBrowser"), icon: "external", enabled: true, run: () => void api.openBrowser(p.path) },
      { kind: "item", key: "output", label: t("switcher.showOutput"), icon: "pulse", enabled: true, run: () => emit("output", p.path) },
      { kind: "item", key: "restart", label: t("common.restart"), icon: "restart", enabled: own, run: () => void api.act(p.path, "restart") },
      { kind: "item", key: "stop", label: t("common.stop"), icon: "stop", enabled: own, run: () => void api.act(p.path, "stop") },
    );
  } else {
    server.push({ kind: "item", key: "start", label: t("common.start"), icon: "play", enabled: own && !busy, run: () => void api.act(p.path, "start") });
    if (p.status === "crashed" || busy) server.push({ kind: "item", key: "output", label: t("switcher.showOutput"), icon: "pulse", enabled: true, run: () => emit("output", p.path) });
  }

  const made: Entry[] = [head, ...server, sep("s1")];
  made.push({ kind: "item", key: "commands", label: t("common.commands"), icon: "terminal", sub: commandsSub(p), enabled: true });
  const links = linksSub(p);
  if (links.length) made.push({ kind: "item", key: "links", label: t("common.links"), icon: "link", sub: links, enabled: true });
  made.push({ kind: "item", key: "new", label: t("projectMenu.newIn"), icon: "plus", sub: terminalSub(p), enabled: true });
  // Its keys only where the panel leaves them to the window: elsewhere Ctrl+Shift+E in the panel
  // splits a pane down (lib/panes: `paneAction`).
  made.push(
    sep("s2"),
    { kind: "item", key: "files", label: t("files.show"), icon: "files", keys: isMac ? "⇧⌘E" : undefined, enabled: true, run: () => emit("files", p.path) },
    { kind: "item", key: "editor", label: t("common.openInEditor"), icon: "editor", enabled: true, run: () => void api.openEditor(p.path) },
    { kind: "item", key: "reveal", label: revealLabel.value, icon: "folder-open", enabled: true, run: () => void api.revealFolder(p.path) },
    sep("s3"),
    {
      kind: "item",
      key: "star",
      label: t(p.favourite ? "projectMenu.unfavourite" : "window.addFavourite"),
      icon: p.favourite ? "star-filled" : "star",
      enabled: true,
      run: () => void api.setFavourite(p.path, !p.favourite),
    },
    { kind: "item", key: "mute", label: t(p.muted ? "notifications.unmute" : "notifications.mute"), icon: p.muted ? "bell" : "bell-off", enabled: true, run: () => void api.muteProject(p.path, !p.muted) },
    { kind: "item", key: "settings", label: t("detail.projectSettings"), icon: "settings", enabled: true, run: () => emit("settings", p.path) },
  );
  // Out of the list, star and all, as the list's own menu takes it.
  if (p.added || p.favourite) made.push(sep("s4"), { kind: "item", key: "remove", label: t("window.removeProject"), icon: "trash", danger: true, enabled: true, run: () => void api.removeProject(p.path) });
  return made;
}

function folderEntries(folder: Folder): Entry[] {
  return [
    { kind: "item", key: "add", label: t("projectMenu.addAndOpen"), icon: "plus", enabled: true, run: () => void addFolder(folder.path, false) },
    { kind: "item", key: "add-star", label: t("projectMenu.addAndStar"), icon: "star", enabled: true, run: () => void addFolder(folder.path, true) },
    sep("f1"),
    { kind: "item", key: "editor", label: t("common.openInEditor"), icon: "editor", enabled: true, run: () => void api.openEditor(folder.path) },
    { kind: "item", key: "reveal", label: revealLabel.value, icon: "folder-open", enabled: true, run: () => void api.revealFolder(folder.path) },
  ];
}

/** Added to Pitwall without a star (as the quick switcher adds one), or with one; then shown. */
async function addFolder(path: string, star: boolean): Promise<void> {
  try {
    await api.addProject(path);
    if (star) await api.setFavourite(path, true);
    emit("pick", path);
  } catch {
    // Not added: the column's row stays a folder's.
  }
}

const entries = computed<Entry[]>(() => {
  const at = target.value;
  if (!at) return [];
  if (at.kind === "folder") return folderEntries(at.folder);
  return project.value ? projectEntries(project.value) : [];
});

/** The rows the keyboard moves over: the items that can be picked, by their place. */
const items = computed(() => entries.value.flatMap((entry, at) => (entry.kind === "item" && entry.enabled ? [at] : [])));
const subEntries = computed<SubEntry[]>(() => {
  const entry = subOf.value === null ? null : entries.value[subOf.value];
  return entry?.kind === "item" ? (entry.sub ?? []) : [];
});
const subItems = computed(() => subEntries.value.flatMap((entry, at) => (entry.kind === "item" && entry.enabled ? [at] : [])));

/** Opens the menu where the pointer is, kept inside the window. */
function show(event: MouseEvent, what: Target): void {
  hide();
  target.value = what;
  asking.value = null;
  active.value = -1;
  const left = Math.max(8, Math.min(event.clientX, window.innerWidth - WIDTH - 8));
  place.value = { left: `${left}px`, top: `${event.clientY}px` };
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => {
    const tall = list.value?.offsetHeight ?? 0;
    const top = Math.max(8, Math.min(event.clientY, window.innerHeight - tall - 8));
    place.value = { ...place.value, top: `${top}px` };
    list.value?.focus();
  });
}

function hide(): void {
  target.value = null;
  closeSub();
  clearTimeout(leaving);
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

function onOutside(event: PointerEvent): void {
  const at = event.target;
  if (at instanceof Node && (list.value?.contains(at) || subList.value?.contains(at))) return;
  hide();
}

/** Row `at`'s list beside it, kept inside the window. */
function openSub(at: number, keyed = false): void {
  clearTimeout(leaving);
  const entry = entries.value[at];
  if (entry?.kind !== "item" || !entry.sub) return closeSub();
  if (subOf.value !== at) {
    subOf.value = at;
    subActive.value = -1;
    asking.value = null;
    const row = list.value?.querySelector<HTMLElement>(`[data-at="${at}"]`)?.getBoundingClientRect();
    const main = list.value?.getBoundingClientRect();
    if (row && main) {
      const right = main.right - 4 + SUB_WIDTH <= window.innerWidth - 8;
      const left = right ? main.right - 4 : Math.max(8, main.left - SUB_WIDTH + 4);
      subPlace.value = { left: `${left}px`, top: `${row.top - 6}px`, width: `${SUB_WIDTH}px` };
      void nextTick(() => {
        const tall = subList.value?.offsetHeight ?? 0;
        const top = Math.max(8, Math.min(row.top - 6, window.innerHeight - tall - 8));
        subPlace.value = { ...subPlace.value, top: `${top}px` };
      });
    }
  }
  inSub.value = keyed;
  if (keyed) {
    subActive.value = subItems.value[0] ?? -1;
    void nextTick(() => subList.value?.focus());
  }
}

function closeSub(): void {
  subOf.value = null;
  subActive.value = -1;
  inSub.value = false;
  asking.value = null;
}

/** The pointer on another row: its list opens (a row with one), another's goes a moment later,
 * unless the pointer reaches it first. */
function hoverRow(at: number): void {
  active.value = at;
  const entry = entries.value[at];
  if (entry?.kind === "item" && entry.sub) return openSub(at);
  if (subOf.value === null) return;
  clearTimeout(leaving);
  leaving = setTimeout(closeSub, 220);
}

function keepSub(): void {
  clearTimeout(leaving);
}

function pick(at: number): void {
  const entry = entries.value[at];
  if (entry?.kind !== "item" || !entry.enabled) return;
  if (entry.sub) return openSub(at, true);
  hide();
  entry.run?.();
}

function pickSub(at: number): void {
  const entry = subEntries.value[at];
  if (entry?.kind !== "item" || !entry.enabled) return;
  if (!entry.stays) hide();
  entry.run();
}

function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const own = inSub.value ? subItems.value : items.value;
  const now = inSub.value ? subActive : active;
  const here = own.indexOf(now.value);
  switch (event.key) {
    case "ArrowDown":
      now.value = own[here < 0 ? 0 : Math.min(own.length - 1, here + 1)] ?? now.value;
      break;
    case "ArrowUp":
      now.value = own[here < 0 ? own.length - 1 : Math.max(0, here - 1)] ?? now.value;
      break;
    case "Home":
      now.value = own[0] ?? -1;
      break;
    case "End":
      now.value = own[own.length - 1] ?? -1;
      break;
    case "ArrowRight":
      if (!inSub.value && active.value >= 0) openSub(active.value, true);
      break;
    case "ArrowLeft":
    case "Escape":
      if (inSub.value || (event.key === "Escape" && subOf.value !== null)) {
        closeSub();
        void nextTick(() => list.value?.focus());
      } else if (event.key === "Escape") hide();
      break;
    case "Enter":
    case " ":
      if (inSub.value) pickSub(subActive.value);
      else pick(active.value);
      break;
    case "Tab":
      hide();
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
    <template v-if="open && entries.length">
      <div
        ref="list"
        class="menu"
        role="menu"
        tabindex="-1"
        :aria-label="target?.kind === 'folder' ? target.folder.name : project?.name"
        :style="{ ...place, width: `${WIDTH}px` }"
        @keydown="onKey"
        @mousedown.prevent
        @contextmenu.prevent
      >
        <template v-for="(entry, i) in entries" :key="entry.key">
          <div v-if="entry.kind === 'head'" class="mh" aria-hidden="true">
            <span :class="['dot', entry.dot]"></span>
            <span class="mh-label">{{ t("common.devServer") }} · {{ entry.label }}</span>
            <span v-if="entry.meta" class="meta">{{ entry.meta }}</span>
          </div>
          <div v-else-if="entry.kind === 'sep'" class="msep" role="separator"></div>
          <div
            v-else
            role="menuitem"
            :data-at="i"
            :aria-haspopup="entry.sub ? 'menu' : undefined"
            :aria-expanded="entry.sub ? subOf === i : undefined"
            :aria-disabled="!entry.enabled || undefined"
            :class="['mi', { hv: active === i || subOf === i, off: !entry.enabled, danger: entry.danger }]"
            @mouseenter="hoverRow(i)"
            @click="pick(i)"
          >
            <AgentMark v-if="entry.agent" :agent="entry.agent" :size="14" />
            <Icon v-else-if="entry.icon" :name="entry.icon" :size="14" class="mark" />
            <span class="lb">{{ entry.label }}</span>
            <span v-if="entry.keys" class="kb">{{ entry.keys }}</span>
            <span v-if="entry.sub" class="ch"><Icon name="chevron" :size="13" /></span>
          </div>
        </template>
      </div>
      <div
        v-if="subOf !== null && subEntries.length"
        ref="subList"
        class="menu"
        role="menu"
        tabindex="-1"
        :aria-label="(entries[subOf] as Item | undefined)?.label"
        :style="subPlace"
        @keydown="onKey"
        @mousedown.prevent
        @contextmenu.prevent
        @mouseenter="keepSub"
      >
        <template v-for="(entry, j) in subEntries" :key="entry.key">
          <div v-if="entry.kind === 'sep'" class="msep" role="separator"></div>
          <div
            v-else
            role="menuitem"
            :aria-disabled="!entry.enabled || undefined"
            :class="['mi', 'si', { hv: subActive === j, off: !entry.enabled, asking: !!asking && entry.key === `command-${asking}` }]"
            @mouseenter="entry.enabled && (subActive = j)"
            @mouseleave="subActive === j && (subActive = -1)"
            @click="pickSub(j)"
          >
            <span v-if="entry.dot" :class="['dot', entry.dot]"></span>
            <AgentMark v-else-if="entry.agent" :agent="entry.agent" :size="14" />
            <Icon v-else-if="entry.icon" :name="entry.icon" :size="14" class="mark" />
            <span class="lb">{{ entry.label }}</span>
            <span v-if="entry.stop" class="stop" :title="t('common.stop')"><Icon name="stop" :size="11" /></span>
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

.mi.hv {
  background: var(--bg-lit);
}

.mi.off {
  opacity: 0.45;
}

.mi:not(:has(.ch)) {
  padding-right: 10px;
}

.mark {
  flex-shrink: 0;
  color: var(--text-muted);
}

.danger,
.danger .mark {
  color: var(--crash-text);
}

/* An "ask first" command whose row asks: its question in the warning's colour. */
.mi.asking .lb {
  color: var(--warn);
}

.lb {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.kb {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.ch {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 30px;
  flex-shrink: 0;
  color: var(--text-muted);
}

/* The server's head: its state, then its port. */
.mh {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-faint);
  white-space: nowrap;
}

.mh-label {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.meta {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-weight: 400;
  color: var(--text-subtle);
}

.msep {
  height: 1px;
  flex-shrink: 0;
  margin: 5px 6px;
  background: var(--line-strong);
}

.si {
  padding-left: 10px;
}

/* The states the list draws: running green, failed red, at work a ring; idle a faint ring. */
.dot {
  width: 7px;
  height: 7px;
  margin: 0 3.5px;
  flex-shrink: 0;
  box-sizing: border-box;
  border-radius: 50%;
  border: 1.5px solid var(--idle-ring);
}

.dot.on {
  border: 0;
  background: var(--run);
}

.dot.bad {
  border: 0;
  background: var(--crash);
}

.dot.busy {
  border-color: var(--text-faint);
  border-top-color: transparent;
  animation: spin 0.8s linear infinite;
}

/* A running command's stop square, at its row's end. */
.stop {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  color: var(--text-muted);
}

.mi.hv .stop {
  color: var(--crash-text);
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .dot.busy {
    animation: none;
  }
}
</style>
