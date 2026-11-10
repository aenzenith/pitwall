<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { accountInUseOf, accountName, accountsOf, askLoginEnds, loginEndedOn, loginEndView, switchAccount, type LoginEndView } from "../../lib/accounts";
import { clockText, cluster, isLow, isStale, left, percentText, readClock } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import { now } from "../../lib/store";
import type { Agent, ClaudeAccount, UsageState, UsageWindow } from "../../lib/types";
import AccountRenewDialog from "../settings/AccountRenewDialog.vue";

/**
 * The Fuel page's account chip, at the bar's end before "x min ago": the account in use by its
 * name, its share left being the page's own. It opens a panel of every account, each with its plan, its session and its
 * week (a bar and when it refills) and the Claude sessions running with it; a click on another one
 * switches to it (`account_use`). "Add account" opens Settings › Claude with the add dialog,
 * "Manage accounts" Settings › Claude. Shown only with more than one account (FuelView).
 * A Claude account's row ends with when its claude.ai sign-in ends (asked as the panel opens);
 * within three days of it or past it, "Renew sign-in" beside that line, a button of its own over
 * the row's corner (a row is a button), opens `AccountRenewDialog`.
 */
/* `agent`: whose accounts (none: Claude's, as before). `plan`: the chip says the plan in use after
 * the name, as beside the agent picker (FuelView with more than one agent on), its panel opening
 * under it from its left edge. Codex's rows say when their limits were last read: Codex writes
 * them at its requests, Pitwall asks nobody. */
const props = defineProps<{ agent?: Agent; plan?: boolean }>();
const emit = defineEmits<{ add: []; manage: [] }>();

const agent = computed<Agent>(() => props.agent ?? "claude");

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const panel = ref<HTMLElement | null>(null);
const open = ref(false);
const place = ref<Record<string, string>>({});
/** The account being switched to: its row's spinner. */
const switching = ref<string | null>(null);
const error = ref("");
/** The account whose sign-in is being renewed: its dialog. */
const renewing = ref<ClaudeAccount | null>(null);

/** One of an account's two bars: what it is and when it refills, its share left. */
type Bar = { key: "session" | "week"; label: string; share: string; fill: number; low: boolean; aria: string };

/** `Mon`, `Pzt`: a week's refill, its day alone. */
function weekday(at: number): string {
  return new Date(at).toLocaleDateString(language.value, { weekday: "short" });
}

function bar(key: Bar["key"], window: UsageWindow): Bar {
  const name = t(key === "session" ? "fuel.name.sessionShort" : "fuel.name.weekShort");
  const when = window.resetsAt === null ? t("fuel.notStarted") : key === "session" ? clockText(window.resetsAt, language.value) : weekday(window.resetsAt);
  const share = percentText(left(window.used), language.value);
  return { key, label: `${name} · ${when}`, share, fill: left(window.used), low: isLow(window.used), aria: t("fuel.gauge.label", { name, value: share }) };
}

/** Why there are no bars, in a word on the row's first line, for a reading the core couldn't renew. */
function stateTag(limits: UsageState): string | null {
  switch (limits.status) {
    case "signedOut":
      return t("fuel.state.signedOut.tag");
    case "expired":
      return t("fuel.state.expired.tag");
    case "rateLimited":
      return t("fuel.state.rateLimited.tag");
    case "error":
      return t("fuel.state.error.tag");
    default:
      return null;
  }
}

type Row = {
  account: ClaudeAccount;
  name: string;
  plan: string | null;
  bars: Bar[];
  /** Its reading is an older one: the bars without their colour. */
  stale: boolean;
  /** A word at the line's end: its state when no bars can say it. */
  tag: string | null;
  /** A line in place of the bars: what to do about it. */
  line: string | null;
  /** Its first reading is on its way. */
  reading: boolean;
  /** Codex's: when its limits were last read. */
  read: string | null;
  /** Claude's: when its sign-in ends, the row's last line; once past, what to do about it. */
  login: (LoginEndView & { line: string }) | null;
  /** Its sign-in ended: the tag says so, in the crash's colour. */
  ended: boolean;
};

const rows = computed<Row[]>(() =>
  accountsOf(agent.value).map((account) => {
    const limits = account.limits;
    const { session, week } = cluster(limits.usage, now.value, t, language.value);
    const stale = isStale(limits);
    const login = agent.value === "claude" && !account.missing ? loginEndView(account.id, now.value) : null;
    const ended = login?.level === "ended";
    const expired = limits.status === "expired" || ended;
    const bars = account.missing || expired ? [] : [...(session ? [bar("session", session)] : []), ...(week ? [bar("week", week)] : [])];
    return {
      account,
      name: accountName(account),
      plan: limits.plan,
      bars,
      stale,
      tag: account.missing ? t("accounts.missing") : ended ? t("accounts.loginEnd.endedTag") : stale ? stateTag(limits) : null,
      // A sign-in past its end is renewed only by signing in again, not by opening Claude.
      line: !account.missing && expired && !ended ? t("accounts.expiredLine") : null,
      reading: !account.missing && limits.status === "loading" && !limits.usage,
      read: agent.value === "codex" && limits.fetchedAt ? t("fuel.lastRead", { time: readClock(limits.fetchedAt, now.value, language.value) }) : null,
      login: login && { ...login, line: ended ? t("accounts.loginEnd.endedLine", { date: loginEndedOn(account.id, now.value) ?? "" }) : login.text },
      ended,
    };
  }),
);

/* ---------- the chip ---------- */

const chip = computed(() => {
  const account = accountInUseOf(agent.value);
  return account ? { name: accountName(account), plan: props.plan ? account.limits.plan : null } : null;
});

/* ---------- open and close ---------- */

const WIDTH = 384;

function show(): void {
  const box = button.value?.getBoundingClientRect();
  if (!box || open.value) return;
  error.value = "";
  const maxHeight = `${Math.max(160, window.innerHeight - box.bottom - 22)}px`;
  if (props.plan) {
    // Beside the agent picker: under the chip from its left edge, never past the window's right one.
    const leftEdge = Math.max(16, Math.min(box.left, window.innerWidth - WIDTH - 16));
    place.value = { top: `${box.bottom + 6}px`, left: `${leftEdge}px`, maxHeight };
  } else {
    // Under the chip, its right edge on the chip's, never past the window's left edge.
    const right = Math.min(window.innerWidth - box.right, Math.max(16, window.innerWidth - WIDTH - 16));
    place.value = { top: `${box.bottom + 6}px`, right: `${Math.max(16, right)}px`, maxHeight };
  }
  open.value = true;
  if (agent.value === "claude") void askLoginEnds();
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => {
    const rows = buttons();
    (rows.find((row) => row.getAttribute("aria-pressed") === "true") ?? rows[0])?.focus();
  });
}

function hide(): void {
  open.value = false;
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

function close(): void {
  hide();
  button.value?.focus();
}

function onOutside(event: PointerEvent): void {
  if (!(event.target instanceof Node && root.value?.contains(event.target))) hide();
}

onBeforeUnmount(hide);

/** The panel's buttons that take a click, top to bottom: the rows, then its foot's. */
function buttons(): HTMLButtonElement[] {
  return Array.from(panel.value?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
}

/** ↑/↓ move between them, Esc closes; Tab leaving the panel closes it too. */
function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close();
    return;
  }
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  event.preventDefault();
  const list = buttons();
  const at = list.indexOf(document.activeElement as HTMLButtonElement);
  const next = event.key === "ArrowDown" ? Math.min(list.length - 1, at + 1) : Math.max(0, at - 1);
  list[next]?.focus();
}

function onFocusOut(event: FocusEvent): void {
  const to = event.relatedTarget;
  if (open.value && to instanceof Node && !panel.value?.contains(to) && to !== button.value) hide();
}

/* ---------- a row picked ---------- */

async function pick(row: Row): Promise<void> {
  if (switching.value || row.account.missing) return;
  if (row.account.inUse) {
    close();
    return;
  }
  switching.value = row.account.id;
  error.value = "";
  try {
    await switchAccount(row.account.id, agent.value);
    close();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    switching.value = null;
  }
}

function add(): void {
  hide();
  emit("add");
}

/** "Renew sign-in": the panel makes way for its dialog. */
function renew(account: ClaudeAccount): void {
  hide();
  renewing.value = account;
}

function manage(): void {
  hide();
  emit("manage");
}
</script>

<template>
  <div ref="root" class="accounts">
    <button
      v-if="chip"
      ref="button"
      type="button"
      :class="['chip', { on: open }]"
      aria-haspopup="dialog"
      :aria-expanded="open"
      :aria-label="t('accounts.chip', { name: chip.name })"
      :title="t('accounts.chip', { name: chip.name })"
      @click="open ? hide() : show()"
      @keydown.down.prevent="show"
    >
      <Icon name="user" :size="14" />
      <span class="chip-name">{{ chip.name }}</span>
      <span v-if="chip.plan" class="chip-plan">{{ chip.plan }}</span>
      <Icon name="chevron-down" :size="12" class="chip-chevron" />
    </button>

    <div v-if="open" ref="panel" class="panel" role="dialog" :aria-label="t(agent === 'codex' ? 'accounts.title.codex' : 'accounts.title')" :style="place" @keydown="onKey" @focusout="onFocusOut">
      <span class="panel-label section-label" aria-hidden="true">{{ t(agent === "codex" ? "accounts.title.codex" : "accounts.title") }}</span>
      <div v-for="row in rows" :key="row.account.id" class="slot">
        <button
          type="button"
          :class="['row', { on: row.account.inUse, stale: row.stale }]"
          :aria-pressed="row.account.inUse"
          :disabled="row.account.missing"
          :aria-busy="switching === row.account.id"
          @click="pick(row)"
        >
          <span class="row-head">
            <span class="row-name">{{ row.name }}</span>
            <span v-if="row.plan" class="row-plan">{{ row.plan }}</span>
            <span class="row-end">
              <Spinner v-if="switching === row.account.id" :size="12" />
              <span v-else-if="row.reading" class="row-reading"><Spinner :size="11" />{{ t("accounts.reading") }}</span>
              <span v-else-if="row.tag" :class="['tag', { expired: row.account.limits.status === 'expired', ended: row.ended }]">{{ row.tag }}</span>
              <span v-else-if="row.account.inUse" class="tag">{{ t("accounts.inUse") }}</span>
              <span v-else-if="row.read" class="row-read">{{ row.read }}</span>
            </span>
          </span>
          <span v-if="row.line" class="row-line">{{ row.line }}</span>
          <span v-if="row.bars.length" class="bars">
            <span v-for="each in row.bars" :key="each.key" class="bar" role="img" :aria-label="each.aria">
              <span class="bar-head">
                <span class="bar-label">{{ each.label }}</span>
                <span :class="['bar-share', { low: each.low }]">{{ each.share }}</span>
              </span>
              <span class="bar-track"><span :class="['bar-fill', { low: each.low }]" :style="{ width: `${each.fill}%` }"></span></span>
            </span>
          </span>
          <span v-if="agent === 'claude' && row.account.running > 0 && !row.account.missing" class="row-running">
            <span class="dot" aria-hidden="true"></span>{{ t("accounts.running", { count: row.account.running }) }}
          </span>
          <!-- Codex's: its sessions running, and when the one in use was last read. -->
          <span v-else-if="agent !== 'claude' && ((row.account.running > 0 && !row.account.missing) || (row.read && row.account.inUse))" class="row-foot">
            <span v-if="row.account.running > 0 && !row.account.missing" class="row-running">
              <span class="dot" aria-hidden="true"></span>{{ t("accounts.running.codex", { count: row.account.running }) }}
            </span>
            <span v-if="row.read && row.account.inUse" class="row-read">{{ row.read }}</span>
          </span>
          <!-- Claude's: when its sign-in ends. "Renew sign-in" stands over the room kept for it. -->
          <span v-if="row.login" :class="['row-login', row.login.level]">
            <span class="row-login-text" :title="row.login.title ?? undefined">{{ row.login.line }}</span>
            <span v-if="row.login.renew" class="renew-room" aria-hidden="true">{{ t("accounts.loginEnd.renew") }}</span>
          </span>
        </button>
        <button v-if="row.login?.renew" type="button" class="renew" :aria-label="t('accounts.loginEnd.renewFor', { name: row.name })" @click="renew(row.account)">
          {{ t("accounts.loginEnd.renew") }}
        </button>
      </div>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <div class="rule" aria-hidden="true"></div>
      <div class="foot">
        <button type="button" class="foot-button" @click="add"><Icon name="plus" :size="13" />{{ t("accounts.add") }}</button>
        <button type="button" class="foot-button manage" @click="manage">{{ t("accounts.manage") }}</button>
      </div>
    </div>
    <AccountRenewDialog v-if="renewing" :account="renewing" @close="renewing = null" />
  </div>
</template>

<style scoped>
.accounts {
  display: flex;
  flex-shrink: 1;
  min-width: 0;
}

/* The account in use: a button of the bar as the refresh one is, its ground only under the pointer
   or while the panel is open. */
.chip {
  min-width: 0;
  height: 30px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 9px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text);
  white-space: nowrap;
}

.chip:hover,
.chip.on {
  background: var(--bg-icon-hover);
}

.chip > :deep(svg) {
  flex-shrink: 0;
  color: var(--text-muted);
}

.chip-name {
  min-width: 0;
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.chip-plan {
  flex-shrink: 0;
  color: var(--text-subtle);
}

.chip-chevron {
  transition: transform 0.15s;
}

.chip.on .chip-chevron {
  transform: rotate(180deg);
}

/* Over the page, under the chip. */
.panel {
  position: fixed;
  z-index: 60;
  width: 384px;
  max-width: calc(100vw - 32px);
  box-sizing: border-box;
  overflow-y: auto;
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-panel);
  background: var(--bg-dialog);
  box-shadow: var(--shadow-popover);
  cursor: default;
}

.panel-label {
  padding: 6px 10px 4px;
}

/* A whole row is the account's button: a click switches to it. */
.row {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px 12px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  text-align: left;
}

.row:hover:not(:disabled),
.row:focus-visible {
  background: var(--bg-hover);
}

.row.on {
  background: var(--bg-control-on);
}

.row:disabled {
  opacity: 0.55;
}

.row-head {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.row-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--text-strong);
}

.row-plan {
  flex-shrink: 0;
  color: var(--text-subtle);
  white-space: nowrap;
}

.row-end {
  margin-left: auto;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  color: var(--text-muted);
}

.tag {
  padding: 2px 7px;
  border-radius: 5px;
  background: var(--bg-tag);
  font-size: 11px;
  color: var(--text-soft);
  white-space: nowrap;
}

.tag.expired {
  color: var(--claude-text);
}

.tag.ended {
  color: var(--crash-text);
}

.row-reading {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-subtle);
}

.row-line {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-muted);
}

.bars {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}

.bar {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}

.bar-head {
  display: flex;
  justify-content: space-between;
  gap: 6px;
  font-size: 11px;
  color: var(--text-muted);
}

.bar-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bar-share {
  flex-shrink: 0;
  font-family: var(--font-mono);
  color: var(--claude-text);
  font-variant-numeric: tabular-nums;
}

.bar-track {
  position: relative;
  height: 4px;
  border-radius: 2px;
  background: var(--line-strong);
  overflow: hidden;
}

.bar-fill {
  position: absolute;
  inset: 0 auto 0 0;
  background: var(--claude);
}

.bar-share.low {
  color: var(--crash-text);
}

.bar-fill.low {
  background: var(--crash);
}

/* An older reading: no state colour, as the page's dials. */
.row.stale .bar-share {
  color: var(--text-muted);
}

.row.stale .bar-fill {
  background: var(--idle-ring);
}

/* A row and, over its bottom right corner, its "Renew sign-in" (a button can't hold one). */
.slot {
  position: relative;
  display: flex;
  flex-direction: column;
}

.row-login {
  display: flex;
  align-items: flex-end;
  gap: 10px;
  font-size: 11px;
  line-height: 1.45;
  color: var(--text-subtle);
}

/* A row is a button, whose words don't wrap by themselves. */
.row-login-text {
  flex-grow: 1;
  min-width: 0;
  white-space: normal;
  overflow-wrap: anywhere;
}

.row-login.near {
  color: var(--warn-text);
}

.row-login.ended {
  font-size: 12px;
  color: var(--text-muted);
}

.row-login.out {
  color: var(--crash-text);
}

/* The button's size, unseen, so the line's text never runs under it. */
.renew-room,
.renew {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  height: 24px;
  box-sizing: border-box;
  padding: 0 10px;
  border: 1px solid transparent;
  font-size: 12px;
  white-space: nowrap;
}

.renew-room {
  visibility: hidden;
}

.renew {
  position: absolute;
  right: 12px;
  bottom: 10px;
  border-color: var(--line-strong);
  border-radius: 6px;
  background: var(--bg-control);
  color: var(--text);
}

.renew:hover,
.renew:focus-visible {
  background: var(--bg-control-hover);
}

.row-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}

.row-read {
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.row-running {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--run-text);
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--run);
}

.error {
  margin: 2px 12px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--crash-text);
  overflow-wrap: anywhere;
}

.rule {
  height: 1px;
  margin: 4px;
  background: var(--line);
}

.foot {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 2px 2px;
}

.foot-button {
  height: 30px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 0 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-soft);
  white-space: nowrap;
}

.foot-button.manage {
  margin-left: auto;
  color: var(--text-muted);
}

.foot-button:hover,
.foot-button:focus-visible {
  background: var(--bg-hover);
  color: var(--text-strong);
}

@media (prefers-reduced-motion: reduce) {
  .chip-chevron {
    transition: none;
  }
}
</style>
