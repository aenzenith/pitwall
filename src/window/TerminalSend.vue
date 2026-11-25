<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";

import AgentMark from "../components/AgentMark.vue";
import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { shellIcon } from "../lib/shells";
import { api } from "../lib/store";
import { tabStates } from "../lib/tabStates";
import { rowState, sendBox, sendCounts, sendGroups, sendLocks, skipWhy, type SendRow, type SendScope } from "../lib/terminalSend";

/**
 * The terminal panel's send window, over its terminals while it is open (⇧⌘I, or the row's
 * button): on the left the tabs, the project's or every project's (Pitwall's own, by project), each
 * picked by its box or locked with why under its name (a permission prompt, a question, an
 * administrator's shell); on the right what is typed, which goes once to every tab picked in the
 * list shown. Return sends and the window goes, what came of it told on the panel's line; ⇧Return
 * opens a new line, Esc or × closes it. The text is the page's alone (lib/terminalSend), emptied as
 * it is sent.
 */
const props = defineProps<{
  /** The project the panel shows. */
  path: string;
}>();

const emit = defineEmits<{ done: [] }>();

const box = computed(() => sendBox(props.path));
const groups = computed(() => sendGroups(props.path, box.value.scope));
const counts = computed(() => sendCounts(props.path));
/** Every tab listed, in the list's order. */
const rows = computed(() => groups.value.flatMap((group) => [...group.agents, ...group.shells]));
const terms = computed(() => rows.value.map((row) => row.term));
/** The tabs that take nothing now, as far as the window can tell; the core judges each again. */
const locks = computed(() => sendLocks(terms.value));
const live = computed(() => tabStates(terms.value));

const field = ref<HTMLTextAreaElement | null>(null);
const windowEl = ref<HTMLElement | null>(null);
const sending = ref(false);
/** The last send didn't reach the core: the text is back, to be sent again. */
const failed = ref(false);

/** The tabs it would go to: picked and listed. */
const targets = computed(() => rows.value.filter((row) => box.value.picked.has(row.term.id)));
/** Of them, those not locked now, and those that are. */
const going = computed(() => targets.value.filter((row) => !locks.value.has(row.term.id)).length);
const leftOut = computed(() => targets.value.length - going.value);
const ready = computed(() => !!box.value.draft.trim() && going.value > 0 && !sending.value);

watch(
  () => box.value.draft,
  () => (failed.value = false),
);

function pickScope(scope: SendScope): void {
  box.value.scope = scope;
}

/** The rows shown that can be picked now: all of them less the locked. */
const pickable = computed(() => rows.value.filter((row) => !locks.value.has(row.term.id)));
/** Select all's box: every pickable row picked, some, or none. */
const allPicked = computed<"all" | "some" | "none">(() => {
  const count = pickable.value.filter((row) => box.value.picked.has(row.term.id)).length;
  return count === 0 ? "none" : count === pickable.value.length ? "all" : "some";
});

/** Select all: every tab the list shows that isn't locked is picked; with all of them picked
 * already, they are all let go. The tabs of the other list keep their picks. */
function toggleAll(): void {
  const picked = new Set(box.value.picked);
  const letGo = allPicked.value === "all";
  for (const row of pickable.value) {
    if (letGo) picked.delete(row.term.id);
    else picked.add(row.term.id);
  }
  box.value.picked = picked;
}

/** A row's box picks the tab or lets it go; a locked one takes neither. */
function toggle(row: SendRow): void {
  const id = row.term.id;
  if (locks.value.has(id)) return;
  const picked = new Set(box.value.picked);
  if (!picked.delete(id)) picked.add(id);
  box.value.picked = picked;
}

function stateOf(row: SendRow): string {
  return locks.value.get(row.term.id) ?? rowState(row.term, live.value);
}

function keys(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    emit("done");
    return;
  }
  if (event.key === "Tab") trap(event);
}

/** The keyboard stays in the window: Tab past its last control comes back to its first. */
function trap(event: KeyboardEvent): void {
  const all = [...(windowEl.value?.querySelectorAll<HTMLElement>("button:not(:disabled), textarea") ?? [])];
  if (!all.length) return;
  const first = all[0];
  const last = all[all.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function fieldKeys(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  void send();
}

async function send(): Promise<void> {
  const text = box.value.draft;
  const sentTo = targets.value;
  if (!ready.value || !sentTo.length) return;
  const sent = box.value;
  // Each tab as it was sent to (its name, its project, its kind): one that closes meanwhile is
  // still named. Another project's tab is named with its project.
  const named = new Map(
    groups.value.flatMap((group) =>
      [...group.agents, ...group.shells].map((row) => [row.term.id, { path: group.path, name: group.path === props.path ? row.name : `${group.name} › ${row.name}`, kind: row.term.kind }] as const),
    ),
  );
  const previews = new Map(locks.value);
  sending.value = true;
  failed.value = false;
  sent.draft = "";
  try {
    const results = await api.sendToTabs(
      sentTo.map((row) => row.term.id),
      text,
    );
    sent.outcome = {
      sent: results.filter((result) => !result.skipped).length,
      skipped: results
        .filter((result) => result.skipped)
        .map((result) => ({
          id: result.id,
          path: named.get(result.id)?.path ?? props.path,
          name: named.get(result.id)?.name ?? "",
          // A dialog the window saw already is named as it named it (the tool asking).
          why: (result.skipped === "dialog" && previews.get(result.id)) || skipWhy(result.skipped!, named.get(result.id)?.kind),
        })),
    };
    emit("done");
  } catch {
    // Nothing went: the text comes back to be sent again.
    if (!sent.draft) sent.draft = text;
    failed.value = true;
    void nextTick(() => field.value?.focus());
  } finally {
    sending.value = false;
  }
}

function focus(): void {
  field.value?.focus();
}

onMounted(focus);

defineExpose({ focus });
</script>

<template>
  <!-- The terminals darkened behind it; a click there closes it, what is typed kept. -->
  <div class="send-shade" @pointerdown.self.prevent="emit('done')">
    <div ref="windowEl" class="send-window" role="dialog" aria-modal="true" :aria-label="t('terminal.send.title')" @keydown="keys">
      <header class="head">
        <Icon name="fan-out" :size="13" class="head-mark" />
        <span class="title">{{ t("terminal.send.title") }}</span>
        <button type="button" class="x" :aria-label="t('common.close')" :title="t('common.close')" @click="emit('done')">×</button>
      </header>
      <div class="body">
        <div class="list">
          <div class="seg" role="group" :aria-label="t('terminal.send.scope')">
            <button type="button" :aria-pressed="box.scope === 'project'" @click="pickScope('project')">
              {{ t("terminal.send.scopeProject") }}<span class="count">{{ counts.project }}</span>
            </button>
            <button type="button" :aria-pressed="box.scope === 'all'" @click="pickScope('all')">
              {{ t("window.filter.all") }}<span class="count">{{ counts.all }}</span>
            </button>
          </div>
          <!-- Select all, over the list: what it shows, but for the locked. -->
          <button
            v-if="groups.length"
            type="button"
            class="row all"
            role="checkbox"
            :aria-checked="allPicked === 'all' ? 'true' : allPicked === 'some' ? 'mixed' : 'false'"
            :disabled="!pickable.length"
            @click="toggleAll"
          >
            <span :class="['pick', { on: allPicked !== 'none' }]"><Icon v-if="allPicked !== 'none'" :name="allPicked === 'all' ? 'check' : 'minus'" :size="9" /></span>
            <span class="name">{{ t("terminal.send.all") }}</span>
          </button>
          <div class="rows">
            <p v-if="!groups.length" class="empty">{{ t("terminal.send.noTabs") }}</p>
            <template v-for="group in groups" :key="group.path">
              <div v-if="box.scope === 'all'" class="group-head">
                <span class="group-name">{{ group.name }}</span><span class="count">{{ group.agents.length + group.shells.length }}</span>
              </div>
              <template v-for="(part, at) in [group.agents, group.shells]" :key="at">
                <div v-if="at === 1 && part.length && group.agents.length" class="rule" aria-hidden="true"></div>
                <button
                  v-for="row in part"
                  :key="row.term.id"
                  type="button"
                  :class="['row', { on: box.picked.has(row.term.id), locked: locks.has(row.term.id) }]"
                  role="checkbox"
                  :aria-checked="!locks.has(row.term.id) && box.picked.has(row.term.id)"
                  :aria-disabled="locks.has(row.term.id)"
                  @click="toggle(row)"
                >
                  <span v-if="locks.has(row.term.id)" class="pick lock"><Icon name="lock" :size="10" /></span>
                  <span v-else :class="['pick', { on: box.picked.has(row.term.id) }]"><Icon v-if="box.picked.has(row.term.id)" name="check" :size="9" /></span>
                  <span class="mark">
                    <AgentMark v-if="row.term.kind !== 'shell'" :agent="row.term.kind === 'chat' ? 'claude' : row.term.kind" :size="11" />
                    <Icon v-else :name="shellIcon(row.term.shell)" :size="11" />
                  </span>
                  <span class="text">
                    <span class="name">{{ row.name }}</span>
                    <span :class="['state', { why: locks.has(row.term.id) }]">{{ stateOf(row) }}</span>
                  </span>
                </button>
              </template>
            </template>
          </div>
        </div>
        <div class="message">
          <textarea
            ref="field"
            v-model="box.draft"
            class="input"
            spellcheck="false"
            :placeholder="t('terminal.send.placeholder')"
            :aria-label="t('terminal.send.placeholder')"
            @keydown="fieldKeys"
          ></textarea>
          <div class="foot">
            <Icon name="fan-out" :size="12" class="foot-mark" />
            <!-- Where it goes, and how many of the tabs picked are left out (each one's why is in the list). -->
            <span class="line" aria-live="polite">
              <template v-if="failed">
                <span class="why">{{ t("terminal.send.why.failed") }}</span>
              </template>
              <template v-else>
                <strong>{{ going ? t("terminal.send.goes", { count: going }) : t("terminal.send.none") }}</strong>
                <template v-if="leftOut">
                  <span class="dot">·</span><span class="why">{{ t("terminal.send.leftOut", { count: leftOut }) }}</span>
                </template>
              </template>
            </span>
            <span class="grow"></span>
            <kbd class="key" aria-hidden="true"><Icon name="enter" :size="10" /></kbd>
            <button type="button" class="go" :disabled="!ready" @click="send">
              <Spinner v-if="sending" :size="11" />
              <template v-else>{{ t("terminal.send.send") }}</template>
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.send-shade {
  position: absolute;
  inset: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  border-radius: 8px;
  background: var(--scrim-soft);
  animation: send-shade-in 0.16s ease-out;
}

.send-window {
  display: flex;
  flex-direction: column;
  width: min(780px, 100%);
  height: min(500px, 100%);
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--line-bold);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-dialog);
  user-select: none;
  -webkit-user-select: none;
  /* It comes up as Pitwall's other dialogs do (RenameDialog, CommandDialog). */
  animation: send-pop 0.16s ease-out;
}

.head {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 8px 0 12px;
  border-bottom: 1px solid var(--line);
}

.head-mark {
  color: var(--text-faint);
}

.title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-strong);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.x {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  font-size: 15px;
  line-height: 1;
  color: var(--text-faint);
}

.x:hover {
  background: var(--bg-control-hover);
  color: var(--text-strong);
}

.body {
  flex: 1;
  display: flex;
  min-height: 0;
}

.list {
  flex: 0 0 clamp(210px, 40%, 300px);
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-height: 0;
  padding: 8px 6px 8px 8px;
  border-right: 1px solid var(--line);
}

.seg {
  flex: none;
  display: flex;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-seg);
}

.seg button {
  flex: 1 1 0;
  min-width: 0;
  height: 24px;
  padding: 0 6px;
  overflow: hidden;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.seg button:hover:not([aria-pressed="true"]) {
  color: var(--text);
}

.seg button[aria-pressed="true"] {
  background: var(--bg-seg-on);
  box-shadow: var(--shadow-seg-on);
  color: var(--text-strong);
}

.count {
  margin-left: 5px;
  font-size: 11px;
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}

/* Select all: a row of its own over the list, a line under it. */
.row.all {
  flex: none;
  align-items: center;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--line);
  border-radius: 6px 6px 0 0;
}

.row.all .pick {
  margin-top: 0;
}

.row.all:disabled {
  background: transparent;
  cursor: default;
}

.row.all:disabled .name {
  color: var(--text-faint);
}

.rows {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.empty {
  margin: 10px 6px;
  font-size: 12px;
  color: var(--text-faint);
}

.group-head {
  display: flex;
  align-items: baseline;
  padding: 8px 6px 3px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
}

.group-name {
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.rule {
  height: 1px;
  margin: 4px 6px;
  background: var(--line);
}

.row {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  width: 100%;
  padding: 5px 6px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  text-align: left;
}

.row:hover:not(.locked) {
  background: var(--bg-control-hover);
}

.row.on {
  background: var(--bg-control);
}

.row.locked {
  cursor: default;
}

.pick {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  width: 13px;
  height: 13px;
  margin-top: 2px;
  border: 1px solid var(--line-heavy);
  border-radius: 3px;
  color: var(--bg-app);
}

.pick.on {
  border-color: var(--text-strong);
  background: var(--text-strong);
}

.pick.lock {
  border: 0;
  color: var(--text-faint);
}

.mark {
  flex-shrink: 0;
  display: inline-flex;
  margin-top: 3px;
  color: var(--text-muted);
}

.text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.name {
  overflow: hidden;
  font-size: 12px;
  color: var(--text);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.row.locked .name {
  color: var(--text-muted);
}

.state {
  overflow: hidden;
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.state.why {
  color: var(--warn);
}

.message {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 10px 10px 8px 12px;
}

.input {
  flex: 1;
  box-sizing: border-box;
  min-height: 0;
  width: 100%;
  margin: 0;
  padding: 0;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  font-family: var(--font);
  font-size: 13px;
  line-height: 20px;
  color: var(--text);
  user-select: text;
  -webkit-user-select: text;
}

.input::placeholder {
  color: var(--text-faint);
}

.foot {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  height: 30px;
  padding-top: 8px;
  border-top: 1px solid var(--line);
}

.foot-mark {
  flex-shrink: 0;
  color: var(--text-faint);
}

.line {
  min-width: 0;
  overflow: hidden;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.line strong {
  font-weight: 500;
  color: var(--text-muted);
}

.dot {
  margin: 0 5px;
}

.why {
  color: var(--warn);
}

.grow {
  flex: 1;
}

.key {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 3px;
  box-sizing: border-box;
  border: 1px solid var(--line-strong);
  border-radius: 4px;
  font-family: var(--font);
  color: var(--text-faint);
}

.go {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 64px;
  height: 26px;
  padding: 0 12px;
  border: 0;
  border-radius: 6px;
  background: var(--text-strong);
  font-size: 12px;
  font-weight: 600;
  color: var(--bg-app);
}

.go:hover:not(:disabled) {
  background: var(--text-lit);
}

.go:disabled {
  background: var(--bg-control-hover);
  color: var(--text-faint);
  cursor: default;
}

@keyframes send-shade-in {
  from {
    opacity: 0;
  }
}

@keyframes send-pop {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
}

@media (prefers-reduced-motion: reduce) {
  .send-shade,
  .send-window {
    animation: none;
  }
}
</style>
