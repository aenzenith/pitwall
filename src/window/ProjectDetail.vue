<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { LogicalPosition } from "@tauri-apps/api/window";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeMark from "../components/ClaudeMark.vue";
import Icon from "../components/Icon.vue";
import Marquee from "../components/Marquee.vue";
import Rich from "../components/Rich.vue";
import Spinner from "../components/Spinner.vue";
import { ago, bareUrl, editorName, gitLine, phase, uptime } from "../lib/format";
import { t } from "../lib/i18n";
import {
  COLLAPSED_HEIGHT,
  outputCollapsed,
  outputHeight,
  outputRequest,
  outputShown,
  terminalCollapsed,
  terminalHeight,
  useResizer,
} from "../lib/panel";
import { api, now, snapshot } from "../lib/store";
import type { CommandView, CustomCommand, LinkSuggestion, Project, ProjectLink } from "../lib/types";
import CommandDialog from "./CommandDialog.vue";
import LinkDialog from "./LinkDialog.vue";

const props = defineProps<{ project: Project }>();

const lines = ref<string[]>([]);
const log = ref<HTMLElement | null>(null);
const stage = ref<HTMLElement | null>(null);
const settingsOpen = ref(false);
/** Output tab: `null` is the dev server, otherwise a command id. */
const tab = ref<string | null>(null);
/** A command waiting for "Run?" to be confirmed. */
const confirming = ref<string | null>(null);
const address = ref<string | null>(null);

// Dev server fields in the settings pane.
const script = ref("");
const port = ref("");
const url = ref("");

/** The add/edit dialog: open with the command being edited, or `null` for a new one. */
const dialog = ref<{ command: CustomCommand | null; running: boolean } | null>(null);

const runsHere = computed(() => props.project.status !== "stopped" && !props.project.owner);
const editor = computed(() => editorName(snapshot.value?.settings.editor));
const commands = computed(() => props.project.commands ?? []);
const claudeTitle = computed(() => {
  const turn = props.project.claude;
  if (!turn) return t(props.project.claudeWorking ? "detail.claudeWorking" : "detail.noSession");
  return t(({ finished: "detail.claudeFinished", asking: "detail.claudeAsking", permission: "detail.claudePermission" } as const)[turn.kind]);
});

const server = computed(() => {
  const p = props.project;
  const title = p.status === "busy" ? phase(p) : t(`status.${p.status}`);
  const sub = [`npm run ${p.script}`];
  if (p.status === "running" && p.port) sub.push(`:${p.port}`);
  if (p.status === "running" && p.startedAt) sub.push(uptime(p.startedAt, now.value));
  return { title, sub: p.issue && p.status !== "running" ? p.issue.text : sub.join(" · ") };
});

/** Tabs: the dev server, then every command that ran in this session. */
const tabs = computed(() => [
  { id: null as string | null, name: t("common.devServer") },
  ...commands.value.filter((c) => c.status !== "idle" || c.result).map((c) => ({ id: c.id as string | null, name: c.name })),
]);

/* ---------- output ---------- */

async function loadOutput(): Promise<void> {
  lines.value = await api.output(props.project.path, tab.value);
  await scrollDown();
}

async function scrollDown(): Promise<void> {
  await nextTick();
  if (log.value) log.value.scrollTop = log.value.scrollHeight;
}

function showTab(id: string | null): void {
  tab.value = id;
  void loadOutput();
}

// The switcher asked for a command's output: that tab, the panel open. After the project
// switch has reset the tab, hence the tick.
watch(
  () => [outputRequest.value, props.project.path] as const,
  async ([request, path]) => {
    if (!request || request.path !== path) return;
    await nextTick();
    settingsOpen.value = false;
    outputCollapsed.value = false;
    showTab(request.job);
    outputRequest.value = null;
  },
  { immediate: true },
);

// The output keeps the height you gave it, whatever the sections above do. It can grow over
// them, up to the whole area under the header; the sections scroll on their own.
const resizer = useResizer(
  outputHeight,
  () => stage.value?.clientHeight ?? 0,
  () => (terminalCollapsed.value ? null : terminalHeight.value),
);
const outputDragging = resizer.dragging;

/** Collapsed, only the tab row shows; the chosen height comes back on expanding. */
const shownOutput = computed(() => (outputCollapsed.value ? COLLAPSED_HEIGHT : outputHeight.value));

function toggleOutput(): void {
  outputCollapsed.value = !outputCollapsed.value;
}

/** The handle doesn't drag a collapsed panel; the button brings it back. */
function startDrag(event: PointerEvent): void {
  if (!outputCollapsed.value) resizer.start(event);
}

// The terminal panel snaps to the output only while the output is there.
watch(settingsOpen, (open) => (outputShown.value = !open), { immediate: true });

/* ---------- dev server ---------- */

function toggleServer(): void {
  void api.act(props.project.path, props.project.status === "running" ? "stop" : "start");
}

async function loadAddress(): Promise<void> {
  address.value = await api.resolveAddress(props.project.path);
}

/* ---------- commands ---------- */

function mark(c: CommandView): string {
  return { running: "●", busy: "", ok: "✓", failed: "✕", idle: "·" }[c.status];
}

function run(c: CommandView): void {
  if (c.confirm && confirming.value !== c.id) {
    confirming.value = c.id;
    return;
  }
  confirming.value = null;
  void api.runCommand(props.project.path, c.id);
  showTab(c.id);
}

function stop(c: CommandView): void {
  void api.stopCommand(props.project.path, c.id);
}

/** The last context menu and its items, freed when the next one opens: freeing them as soon as
 * the popup returns could drop an item's click handler before its click arrives. */
let lastMenu: { close(): Promise<void> }[] = [];

/**
 * Right-click on a command: the native menu with what the row can do.
 *
 * - Items are made one by one: items given inline to `Menu.new` lose their click handlers
 *   (Tauri 2.12 drops them right after building the menu, and dropping one unregisters it).
 * - It opens at the click point: popped up without a position, macOS leaves it stuck open
 *   (clicks outside move it instead of closing it) and the app frozen behind it.
 */
async function commandMenu(c: CommandView, event: MouseEvent): Promise<void> {
  for (const resource of lastMenu.splice(0)) void resource.close();

  const running = c.status === "running";
  const items = await Promise.all([
    MenuItem.new({ text: t(running ? "common.stop" : "common.run"), enabled: c.status !== "busy", action: () => (running ? stop(c) : run(c)) }),
    MenuItem.new({ text: t("detail.menuEdit"), action: () => editCommand(c.id) }),
    PredefinedMenuItem.new({ item: "Separator" }),
    MenuItem.new({ text: t("common.delete"), action: () => removeCommand(c.id) }),
  ]);
  const menu = await Menu.new({ items });
  lastMenu = [menu, ...items];
  await menu.popup(new LogicalPosition(event.clientX, event.clientY));
}

/* ---------- links ---------- */

const links = computed(() => props.project.settings.links ?? []);
/** Addresses the project names itself (git remote, .env, package.json). */
const found = ref<LinkSuggestion[]>([]);
/** The link dialog: the link being edited, by its place, or `null` for a new one. */
const linkDialog = ref<{ index: number | null } | null>(null);
/** The row whose address was just copied, for a moment. */
const copied = ref<number | null>(null);
let copiedTimer = 0;

function sameUrl(a: string, b: string): boolean {
  return a.replace(/\/$/, "") === b.replace(/\/$/, "");
}

/** Found addresses that aren't links yet. */
const suggestions = computed(() => found.value.filter((s) => !links.value.some((l) => sameUrl(l.url, s.url))));

async function loadSuggestions(path: string): Promise<void> {
  const list = await api.linkSuggestions(path);
  if (path === props.project.path) found.value = list;
}

watch(
  () => props.project.path,
  (path) => {
    found.value = [];
    void loadSuggestions(path);
  },
  { immediate: true },
);

/** Links are saved on their own, like commands. */
function saveLinks(list: ProjectLink[]): void {
  void api.setProjectSettings(props.project.path, { ...props.project.settings, links: list });
}

function saveLink(link: ProjectLink): void {
  const list = links.value.map((l) => ({ ...l }));
  const at = linkDialog.value?.index ?? null;
  if (at === null) list.push(link);
  else list[at] = link;
  saveLinks(list);
  linkDialog.value = null;
}

function removeLink(at: number | null): void {
  if (at !== null) saveLinks(links.value.filter((_, i) => i !== at));
  linkDialog.value = null;
}

function addSuggestion(s: LinkSuggestion): void {
  saveLinks([...links.value, { name: s.name, url: s.url }]);
}

function copyLink(at: number): void {
  void api.copyText(links.value[at].url);
  copied.value = at;
  window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => (copied.value = null), 1200);
}

/** Right-click on a link: the native menu, made as the commands' one is. */
async function linkMenu(at: number, event: MouseEvent): Promise<void> {
  for (const resource of lastMenu.splice(0)) void resource.close();

  const link = links.value[at];
  const items = await Promise.all([
    MenuItem.new({ text: t("common.openInBrowser"), action: () => void api.openUrl(link.url) }),
    MenuItem.new({ text: t("link.copyAddress"), action: () => copyLink(at) }),
    MenuItem.new({ text: t("detail.menuEdit"), action: () => (linkDialog.value = { index: at }) }),
    PredefinedMenuItem.new({ item: "Separator" }),
    MenuItem.new({ text: t("common.delete"), action: () => removeLink(at) }),
  ]);
  const menu = await Menu.new({ items });
  lastMenu = [menu, ...items];
  await menu.popup(new LogicalPosition(event.clientX, event.clientY));
}

/* ---------- project settings ---------- */

function loadSettings(): void {
  script.value = props.project.settings.script ?? "";
  port.value = props.project.settings.port ? String(props.project.settings.port) : "";
  url.value = props.project.settings.url ?? "";
}

/** The gear swaps the panel body between the project's details and its settings. */
function toggleSettings(): void {
  if (settingsOpen.value) {
    closeSettings();
    return;
  }
  loadSettings();
  settingsOpen.value = true;
}

function closeSettings(): void {
  settingsOpen.value = false;
  // The log was unmounted; show its newest lines again.
  void scrollDown();
}

/** Opens the dialog on the stored command (not its live view), or empty for a new one. */
function editCommand(id: string | null): void {
  const command = id ? storedCommands().find((c) => c.id === id) : undefined;
  if (id && !command) return;
  dialog.value = { command: command ?? null, running: commands.value.some((c) => c.id === id && c.status === "running") };
}

function storedCommands(): CustomCommand[] {
  return (props.project.settings.commands ?? []).map((c) => ({ ...c }));
}

/** Commands are saved on their own, so half-typed dev server fields aren't saved with them. */
function saveCommands(list: CustomCommand[]): void {
  void api.setProjectSettings(props.project.path, { ...props.project.settings, commands: list });
}

function saveCommand(command: CustomCommand): void {
  const list = storedCommands();

  if (!command.id) {
    list.push({ ...command, id: `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}` });
  } else {
    const at = list.findIndex((c) => c.id === command.id);
    if (at >= 0) list[at] = command;
  }

  saveCommands(list);
  dialog.value = null;
}

/** A running command is stopped too; once deleted it would have no row left to stop it from. */
function removeCommand(id: string): void {
  if (commands.value.some((c) => c.id === id && c.status === "running")) void api.stopCommand(props.project.path, id);
  saveCommands(storedCommands().filter((c) => c.id !== id));
  if (tab.value === id) showTab(null);
}

function saveSettings(): void {
  const parsed = Number(port.value);
  void api.setProjectSettings(props.project.path, {
    ...props.project.settings,
    script: script.value.trim() || undefined,
    port: Number.isInteger(parsed) && parsed > 0 && parsed < 65536 ? parsed : undefined,
    url: url.value.trim() || undefined,
  });
  closeSettings();
}

/* ---------- lifecycle ---------- */

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await listen<{ path: string; job: string | null; line: string }>("output", (event) => {
    if (event.payload.path !== props.project.path || (event.payload.job ?? null) !== tab.value) return;
    lines.value.push(event.payload.line);
    if (lines.value.length > 500) lines.value.splice(0, lines.value.length - 500);
    void scrollDown();
  });
});

onBeforeUnmount(() => {
  unlisten?.();
  outputShown.value = false;
});

watch(
  () => props.project.path,
  () => {
    settingsOpen.value = false;
    dialog.value = null;
    confirming.value = null;
    tab.value = null;
    loadSettings();
    void loadOutput();
    void loadAddress();
  },
  { immediate: true },
);

watch(() => [props.project.status, props.project.url], () => void loadAddress());
</script>

<template>
  <section class="detail" :aria-label="t('window.projectDetails')">
    <!-- Settings mode: only the name and a back button; everything else waits for the way back. -->
    <!-- Settings: same height as the list's toolbar, so both bottom lines run as one. -->
    <header v-if="settingsOpen" class="head bar" data-tauri-drag-region="deep">
      <div class="name">{{ t("detail.settingsHeading") }}</div>
      <!-- Same spot as the settings button, so it toggles in place. -->
      <button type="button" class="icon" :aria-label="t('detail.backToDetails')" :title="t('common.back')" @click="toggleSettings">
        <Icon name="back" :size="16" />
      </button>
    </header>

    <!-- Name, path and branch drag the window; the settings button stays clickable. -->
    <header v-else class="head" data-tauri-drag-region="deep">
      <div class="title-row">
        <div class="name">{{ project.name }}</div>
        <button type="button" class="icon" :aria-label="t('detail.projectSettings')" :title="t('detail.projectSettings')" @click="toggleSettings">
          <Icon name="settings" :size="16" />
        </button>
      </div>
      <div class="path" :title="project.path">{{ project.path }}</div>
      <div v-if="project.git" class="git">{{ gitLine(project.git) }}</div>
    </header>

    <!-- Project settings, grouped: each per-project setting has its own fieldset. -->
    <div v-if="settingsOpen" class="settings-pane">
      <form class="group" :aria-label="t('detail.serverSettings')" @submit.prevent="saveSettings">
        <div class="section-label">{{ t("common.devServer") }}</div>
        <label class="field">{{ t("common.script") }} <input v-model="script" class="mono" :placeholder="project.script" spellcheck="false" /></label>
        <label class="field">{{ t("detail.port") }} <input v-model="port" class="mono" inputmode="numeric" :placeholder="t('detail.portPlaceholder')" /></label>
        <label class="field">{{ t("detail.address") }} <input v-model="url" class="mono" :placeholder="t('detail.addressPlaceholder')" spellcheck="false" /></label>
        <p class="hint">{{ t("detail.settingsHint") }}</p>
        <div class="form-actions">
          <button type="submit" class="control primary">{{ t("common.save") }}</button>
        </div>
      </form>

      <section class="group" :aria-label="t('common.commands')">
        <div class="section-label">{{ t("common.commands") }}</div>

        <div v-for="c in project.settings.commands ?? []" :key="c.id" class="cmd-item">
          <div class="cmd-text">
            <span class="cmd-name">{{ c.name }}</span>
            <span class="cmd-line">{{ c.command }}</span>
          </div>
          <span v-if="c.keepRunning" class="tag run">{{ t("detail.tagKeepsRunning") }}</span>
          <span v-if="c.confirm" class="tag ask">{{ t("detail.tagAsksFirst") }}</span>
          <span v-if="c.withServer" class="tag">{{ t("detail.tagWithServer") }}</span>
          <button type="button" class="icon small" :aria-label="t('common.editName', { name: c.name })" :title="t('common.edit')" @click="editCommand(c.id)">
            <Icon name="pencil" :size="13" />
          </button>
          <button type="button" class="icon small" :aria-label="t('common.deleteName', { name: c.name })" :title="t('common.delete')" @click="removeCommand(c.id)">×</button>
        </div>

        <button type="button" class="control add" @click="editCommand(null)">{{ t("detail.addCommand") }}</button>

        <p class="hint">
          <Rich :text="t('detail.commandsHint')">
            <template #build><code>npm run build</code></template>
            <template #vite><code>vite build</code></template>
          </Rich>
        </p>
      </section>
    </div>

    <!-- The sections scroll on their own; the output sits over their lower part. -->
    <div v-else ref="stage" :class="['stage', { animated: !outputDragging, collapsed: outputCollapsed }]">
      <div class="sections" :style="{ bottom: `${shownOutput}px` }">
        <section class="block" :aria-label="t('common.devServer')">
          <div class="section-label">{{ t("common.devServer") }}</div>
          <div class="server-row">
            <div class="server-text">
              <span class="server-title"><span :class="['state-dot', project.status]"></span>{{ server.title }}</span>
              <span :class="['server-sub', { bad: project.status === 'crashed' }]"><Marquee :text="server.sub" /></span>
            </div>
            <button v-if="project.status === 'busy'" type="button" class="control busy" disabled>
              <Spinner :size="12" />
            </button>
            <button v-else-if="project.status === 'running'" type="button" class="control stop" @click="toggleServer">
              <Icon name="stop" :size="11" /> {{ t("common.stop") }}
            </button>
            <button v-else type="button" class="control start" @click="toggleServer">
              <Icon name="play" :size="11" /> {{ t("common.start") }}
            </button>
          </div>
          <div class="server-links">
            <button type="button" class="control link" :title="address ?? t('common.openInBrowser')" @click="api.openBrowser(project.path)">
              <Icon name="external" :size="13" />
              <span class="link-text">{{ t("common.openInBrowser") }}</span>
            </button>
            <button v-if="project.status === 'running'" type="button" class="control square" :aria-label="t('common.restart')" :title="t('common.restart')" @click="api.act(project.path, 'restart')">
              <Icon name="restart" :size="14" />
            </button>
            <button type="button" class="control fixed" @click="api.openEditor(project.path)"><Icon name="editor" :size="13" /> {{ editor }}</button>
          </div>
          <p v-if="!project.owner && project.openIn && project.status !== 'running'" class="note">{{ t("detail.openInWindow", { window: project.openIn }) }}</p>
        </section>

        <!-- The project's other addresses: a click opens one (its tab comes forward if open). -->
        <section class="block links" :aria-label="t('common.links')">
          <div class="commands-head">
            <div class="section-label">{{ t("common.links") }}</div>
            <button type="button" class="text-button" @click="linkDialog = { index: null }">{{ t("detail.add") }}</button>
          </div>
          <p v-if="!links.length" class="empty-commands">{{ t("detail.noLinks") }}</p>
          <div v-for="(l, i) in links" :key="`${i}:${l.url}`" class="link-row" @contextmenu="linkMenu(i, $event)">
            <button type="button" class="link-open" :title="l.url" @click="api.openUrl(l.url)">
              <span class="link-icon"><Icon name="link" :size="13" /></span>
              <span class="link-name">{{ l.name }}</span>
              <span class="link-url">{{ bareUrl(l.url) }}</span>
            </button>
            <button
              type="button"
              class="icon small link-action"
              :aria-label="t('link.copyAddress')"
              :title="t(copied === i ? 'link.copied' : 'link.copyAddress')"
              @click="copyLink(i)"
            >
              <Icon :name="copied === i ? 'check' : 'copy'" :size="13" />
            </button>
            <button type="button" class="icon small link-action" :aria-label="t('common.editName', { name: l.name })" :title="t('common.edit')" @click="linkDialog = { index: i }">
              <Icon name="pencil" :size="13" />
            </button>
          </div>
          <div v-if="suggestions.length" class="suggest">
            <span>{{ t("link.suggested") }}</span>
            <button v-for="s in suggestions.slice(0, 3)" :key="s.url" type="button" class="chip" :title="s.url" @click="addSuggestion(s)">+ {{ s.name }}</button>
          </div>
        </section>

        <section class="block" :aria-label="t('window.col.claude')">
          <div class="section-label">Claude</div>
          <div :class="['card', { hot: project.claude }]">
            <span :class="['card-mark', { quiet: !project.claude && !project.claudeWorking }]"><ClaudeMark :size="18" /></span>
            <div class="card-text">
              <span class="card-title">{{ claudeTitle }}</span>
              <span class="card-sub">
                {{
                  project.claude
                    ? t("detail.notLookedAt", { ago: ago(project.claude.at, now) })
                    : t(project.claudeWorking ? "detail.workingHint" : "detail.sessionsHint")
                }}
              </span>
            </div>
            <button v-if="project.claude" type="button" class="seen" @click="api.markSeen(project.path)">{{ t("detail.markSeen") }}</button>
          </div>
        </section>

        <section class="block commands" :aria-label="t('common.commands')">
          <div class="commands-head">
            <div class="section-label">{{ t("common.commands") }}</div>
            <button type="button" class="text-button" @click="editCommand(null)">{{ t("detail.add") }}</button>
          </div>
          <p v-if="!commands.length" class="empty-commands">{{ t("detail.noCommands") }}</p>
          <div v-for="c in commands" :key="c.id" :class="['cmd-row', { focused: tab === c.id }]" @contextmenu="commandMenu(c, $event)">
            <template v-if="confirming === c.id">
              <span class="confirm-text">{{ t("detail.confirmRun", { name: c.name }) }}</span>
              <button type="button" class="control small-btn" @click="confirming = null">{{ t("common.cancel") }}</button>
              <button type="button" class="control primary small-btn" @click="run(c)">{{ t("common.run") }}</button>
            </template>
            <template v-else>
              <span :class="['mark', c.status]">
                <Spinner v-if="c.status === 'busy'" :size="10" />
                <template v-else>{{ mark(c) }}</template>
              </span>
              <button type="button" class="cmd-open" :title="t('detail.showOutput', { name: c.name })" @click="showTab(c.id)">
                <span class="cmd-name">{{ c.name }}</span>
                <span class="cmd-line">{{ c.command }}</span>
              </button>
              <span v-if="c.status === 'running'" class="cmd-state">{{ uptime(c.startedAt, now) }}</span>
              <button type="button" class="icon cmd-edit" :aria-label="t('common.editName', { name: c.name })" :title="t('common.edit')" @click="editCommand(c.id)">
                <Icon name="pencil" :size="13" />
              </button>
              <button
                v-if="c.status === 'running'"
                type="button"
                class="icon"
                :aria-label="t('common.stopName', { name: c.name })"
                :title="t('common.stop')"
                @click="stop(c)"
              >
                <Icon name="stop" :size="12" />
              </button>
              <button
                v-else
                type="button"
                class="icon"
                :disabled="c.status === 'busy'"
                :aria-label="t('common.runName', { name: c.name })"
                :title="t('common.run')"
                @click="run(c)"
              >
                <Icon name="play" :size="12" />
              </button>
            </template>
          </div>
        </section>
      </div>

      <div class="out" :style="{ height: `${shownOutput}px` }">
        <div
          class="grip"
          role="separator"
          aria-orientation="horizontal"
          :aria-label="t('detail.resizeOutput')"
          tabindex="0"
          :aria-valuenow="Math.round(shownOutput)"
          @pointerdown="startDrag"
          @pointermove="resizer.move"
          @pointerup="resizer.end"
          @pointercancel="resizer.end"
          @keydown="resizer.nudge"
        ></div>
        <div class="tabs" role="tablist" :aria-label="t('detail.output')">
          <button
            v-for="item in tabs"
            :key="item.id ?? 'server'"
            type="button"
            role="tab"
            :aria-selected="tab === item.id"
            :class="['tab', { on: tab === item.id }]"
            @click="showTab(item.id)"
          >
            {{ item.name }}
          </button>
          <span class="grow"></span>
          <span v-if="tab === null && project.status === 'running' && runsHere" class="up">{{ uptime(project.startedAt, now) }}</span>
          <button
            type="button"
            :class="['collapse', { up: outputCollapsed }]"
            :aria-expanded="!outputCollapsed"
            :aria-label="t(outputCollapsed ? 'detail.expandOutput' : 'detail.collapseOutput')"
            :title="t(outputCollapsed ? 'common.expand' : 'common.collapse')"
            @click="toggleOutput"
          >
            <Icon name="chevron" :size="13" />
          </button>
        </div>
        <div ref="log" class="log selectable">
          <template v-if="lines.length">
            <span v-for="(line, index) in lines" :key="index" :class="{ cmd: line.startsWith('$ '), sys: line.startsWith('[pitwall]') }">{{ line }}</span>
          </template>
          <span v-else-if="tab === null && project.owner" class="dim">{{ t("detail.noServerOutput") }}</span>
          <span v-else-if="tab === null" class="dim">{{ t("detail.notRunning") }}</span>
          <span v-else class="dim">{{ t("detail.noOutput") }}</span>
        </div>
      </div>
    </div>

    <LinkDialog
      v-if="linkDialog"
      :link="linkDialog.index === null ? null : (links[linkDialog.index] ?? null)"
      :project-name="project.name"
      :suggestions="suggestions"
      @save="saveLink"
      @remove="removeLink(linkDialog?.index ?? null)"
      @close="linkDialog = null"
    />
    <CommandDialog
      v-if="dialog"
      :command="dialog.command"
      :running="dialog.running"
      :project-name="project.name"
      @save="saveCommand"
      @close="dialog = null"
    />
  </section>
</template>

<style scoped>
.detail {
  width: 380px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
  min-height: 0;
  overflow: hidden;
}

/* 94px: toolbar (56) + table header (38), so its line meets the table header's line.
   The 13px top puts the gear's centre at 28px, where the settings header's back button sits. */
.head {
  height: 94px;
  flex-shrink: 0;
  padding: 13px 16px 0 20px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  border-bottom: 1px solid var(--line);
}

/* Settings header matches the list toolbar (56px with its line), so the two bottom lines meet. */
.head.bar {
  height: 56px;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  padding: 0 16px 0 20px;
  border-bottom: 1px solid var(--line);
}

.title-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.name {
  font-size: 17px;
  font-weight: 600;
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.head.bar .name {
  font-size: 15px;
}

.path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.git {
  font-family: var(--font-mono);
  font-size: 12px;
  color: #c7ccd3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.icon {
  width: 30px;
  height: 30px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
  font-size: 16px;
}

.icon.small {
  width: 26px;
  height: 26px;
}

.icon:hover:not(:disabled) {
  background: #262a33;
  color: var(--text-strong);
}

.icon:disabled {
  opacity: 0.4;
}

.control {
  height: 30px;
  padding: 0 12px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  font-size: 13px;
}

.control:hover:not(:disabled) {
  background: #2c3039;
}

.control:disabled {
  opacity: 0.6;
}

.control.primary {
  background: #2a3a30;
  border-color: #3d5446;
  color: #cdebd8;
}

.control.primary:hover {
  background: #33473a;
}

.control.start {
  background: #1d2a22;
  border-color: #2d4537;
  color: #b9e6c9;
}

.control.stop {
  background: #2a1b1b;
  border-color: #4a2b2b;
  color: #f2b8b8;
}

.control.busy {
  color: var(--text-muted);
}

.control.square {
  width: 30px;
  padding: 0;
  justify-content: center;
}

.control.link {
  flex-grow: 1;
  min-width: 0;
}

.control.fixed {
  flex-shrink: 0;
  white-space: nowrap;
}

.link-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.block {
  flex-shrink: 0;
  padding: 14px 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  border-bottom: 1px solid var(--line);
}

/* Dev server: straight in the section, no card around it. */
.server-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.state-dot {
  width: 9px;
  height: 9px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 1.5px solid var(--idle-ring);
}

.state-dot.running {
  border: 0;
  background: var(--run);
  box-shadow: 0 0 0 4px rgba(115, 201, 145, 0.15);
}

.state-dot.crashed {
  border: 0;
  background: var(--crash);
}

.state-dot.busy {
  border-top-color: transparent;
  animation: spin 0.8s linear infinite;
}

.server-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
}

/* The dot sits on the title's line; the line below lines up with the title text. */
.server-title {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 500;
}

.server-sub {
  padding-left: 19px;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.server-sub.bad {
  color: var(--crash-text);
}

.server-links {
  display: flex;
  gap: 6px;
}

.note {
  margin: 0;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.5;
}

.card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-radius: var(--radius-card);
  background: #181b20;
  border: 1px solid var(--line);
}

/* Claude's spark at the card's left; dimmed while nothing is going on. */
.card-mark {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  background: rgba(217, 119, 87, 0.12);
}

.card-mark.quiet {
  opacity: 0.55;
  filter: saturate(0.6);
}

.card.hot {
  background: var(--claude-bg);
  border-color: var(--claude-line);
}

.card-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.card-title {
  font-size: 13px;
  font-weight: 500;
}

.card-sub {
  font-size: 12px;
  color: var(--text-muted);
}

.seen {
  height: 28px;
  padding: 0 10px;
  border: 1px solid #4a3523;
  border-radius: var(--radius-control);
  background: #2a1f17;
  color: #f3c29b;
  font-size: 12px;
}

/* Commands: no line under it; the output brings its own. */
.commands {
  gap: 4px;
  border-bottom: 0;
}

.commands-head {
  display: flex;
  align-items: center;
  margin-bottom: 4px;
}

.commands-head .section-label {
  flex-grow: 1;
}

.text-button {
  height: 24px;
  padding: 0 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.text-button:hover {
  background: #262a33;
  color: var(--text-strong);
}

.empty-commands {
  margin: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

/* The highlight reaches 8px past the block on both sides; the content keeps the block's edges,
   so the run button sits as far from the side as the dev server's buttons. */
.cmd-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 40px;
  margin: 0 -8px;
  padding: 0 8px;
  border-radius: var(--radius-control);
}

.cmd-row.focused {
  background: #1b1e24;
}

.mark {
  width: 16px;
  display: inline-flex;
  justify-content: center;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 600;
  color: #6c727c;
}

.mark.running {
  color: var(--run);
  font-size: 10px;
}

.mark.ok {
  color: var(--run);
}

.mark.failed {
  color: var(--crash-text);
}

.mark.busy {
  color: var(--text-muted);
}

.cmd-open {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
  padding: 4px 0;
  border: 0;
  background: transparent;
  text-align: left;
}

.cmd-name {
  font-weight: 500;
}

.cmd-line {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Links sit close, like a list; the head and the suggestions keep their own room. */
.links {
  gap: 2px;
}

/* A link: one button across the row opens it; copy and edit show on hover. */
.link-row {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 30px;
  margin: 0 -8px;
  padding: 0 4px 0 8px;
  border-radius: var(--radius-control);
}

.link-row:hover {
  background: #1b1e24;
}

.link-open {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  height: 30px;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: left;
}

.link-icon {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--text-subtle);
}

.link-name {
  flex-shrink: 0;
  max-width: 45%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link-url {
  flex-grow: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.link-action {
  opacity: 0;
}

.link-row:hover .link-action,
.link-action:focus-visible {
  opacity: 1;
}

/* Found in the project, not added yet: one click adds. */
.suggest {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-subtle);
}

.chip {
  height: 24px;
  padding: 0 9px;
  border: 1px dashed #3a3f48;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: #b4bac3;
  white-space: nowrap;
}

.chip:hover {
  border-style: solid;
  background: #1c1f25;
  color: var(--text-strong);
}

/* The edit button shows on hover, like a list row's accessory; it keeps its place either way. */
.cmd-edit {
  margin-right: -6px;
  opacity: 0;
}

.cmd-row:hover .cmd-edit,
.cmd-edit:focus-visible {
  opacity: 1;
}

/* Only while it runs: how long it has been up. */
.cmd-state {
  font-size: 12px;
  white-space: nowrap;
  color: #c7ccd3;
}

.confirm-text {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
  color: #f3c29b;
}

.small-btn {
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
}

/* Everything under the header. The sections scroll in the part above the output, so their
   scrollbar ends where the output begins; 24px stays free under their end. */
.stage {
  flex: 1 1 auto;
  min-height: 0;
  position: relative;
}

.sections {
  position: absolute;
  inset: 0;
  padding-bottom: 24px;
  overflow-y: auto;
}

/* Pinned to the bottom with the height you gave it; grown, it covers the sections. */
.out {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  overflow: clip;
  background: var(--bg-detail);
  border-top: 1px solid var(--line);
  box-shadow: 0 -12px 24px rgba(0, 0, 0, 0.22);
}

/* Collapsing and expanding slide (COLLAPSE_MS in lib/panel); a drag follows the pointer. */
.stage.animated .out {
  transition: height 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.stage.animated .sections {
  transition: bottom 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.stage.collapsed .grip {
  cursor: default;
}

.stage.collapsed .grip::after {
  opacity: 0;
}

.stage.collapsed .log {
  opacity: 0;
}

.grow {
  flex-grow: 1;
}

/* Points down while open (collapse), up while collapsed (expand). */
.collapse {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.collapse:hover {
  background: #262a33;
  color: var(--text-strong);
}

.collapse :deep(svg) {
  transform: rotate(90deg);
  transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.collapse.up :deep(svg) {
  transform: rotate(-90deg);
}

@media (prefers-reduced-motion: reduce) {
  .stage.animated .out,
  .stage.animated .sections,
  .log,
  .collapse :deep(svg) {
    transition: none;
  }
}

/* Handle and tabs keep their size; only the log gives way when the panel is short. */
.grip {
  flex-shrink: 0;
  height: 9px;
  margin: 0 20px;
  cursor: row-resize;
  position: relative;
  outline: none;
}

.grip::after {
  content: "";
  position: absolute;
  left: 50%;
  top: 3px;
  width: 36px;
  height: 3px;
  margin-left: -18px;
  border-radius: 2px;
  background: #2f343d;
}

.grip:hover::after,
.grip:focus-visible::after {
  background: #4a515c;
}

/* Output tabs: the active one is white, the rest faded; nothing more. */
.tabs {
  flex-shrink: 0;
  height: 24px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 12px 0 14px;
  overflow: hidden;
}

.tab {
  padding: 4px 6px;
  border: 0;
  background: transparent;
  font-size: 12px;
  color: #6c727c;
  white-space: nowrap;
}

.tab:hover {
  color: var(--text-muted);
}

.tab.on {
  color: #fff;
}

.up {
  flex-shrink: 0;
  white-space: nowrap;
  padding-right: 6px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

.log {
  flex: 1 1 auto;
  min-height: 0;
  transition: opacity 160ms ease;
  margin: 6px 20px 20px;
  box-sizing: border-box;
  padding: 12px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid #1f2228;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.7;
  color: #a9afb8;
  display: flex;
  flex-direction: column;
  overflow: auto;
}

.log span {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.log .cmd {
  color: var(--text);
}

.log .sys {
  color: var(--claude-text);
}

.log .dim {
  white-space: normal;
  color: var(--text-subtle);
}

/* Settings pane */
.settings-pane {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 16px 20px 20px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.group {
  margin: 0;
  padding: 0 0 18px;
  border: 0;
  border-bottom: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group:last-child {
  border-bottom: 0;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-muted);
}

.field input {
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.field input.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.hint code {
  font-family: var(--font-mono);
}

.form-actions {
  display: flex;
  gap: 8px;
}

.cmd-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 6px 8px 10px;
  border-radius: 8px;
  background: #181b20;
  border: 1px solid var(--line);
}

.cmd-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
}

.tag {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: 9px;
  font-size: 11px;
  white-space: nowrap;
  background: #23262d;
  color: var(--text-muted);
}

.tag.run {
  background: #1d2a22;
  color: var(--run);
}

.tag.ask {
  background: #2a1f17;
  color: #f3c29b;
}

.control.add {
  align-self: flex-start;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
