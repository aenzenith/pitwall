import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, ref } from "vue";

import { codexFuel } from "./codexFuel";
import { clock, dayName } from "./day";
import { language, t } from "./i18n";
import { dayShort } from "./period";
import { api, fuel } from "./store";
import type { Agent, AccountRenew, AgentAccountLogin, AgentAccountSync, AgentSyncReport, ClaudeAccount, LoginEnd } from "./types";

/**
 * Claude accounts, as the core sends them with the fuel (`fuel.accounts`): the main one first,
 * then the added ones in the order added. With none added only Settings › Claude › Accounts shows
 * anything of them (`severalAccounts`): no chip, no submenu, no badge anywhere else.
 */

/** Every account; an older core sends none. */
export const accounts = computed<ClaudeAccount[]>(() => fuel.value?.accounts ?? []);

/** More than the main account: the Fuel page's chip, "+"'s "Start with account" and the
 * popover's name show only then. */
export const severalAccounts = computed(() => accounts.value.length > 1);

/** The one new Claude sessions Pitwall starts use. */
export const accountInUse = computed(() => accounts.value.find((account) => account.inUse) ?? null);

/** Its name as the user gave it, the main one's own default while it has none. */
export function accountName(account: ClaudeAccount): string {
  return account.name ?? t("accounts.main");
}

export function accountById(id: string | null | undefined): ClaudeAccount | null {
  return id ? (accounts.value.find((account) => account.id === id) ?? null) : null;
}

/* ---------- another agent's (Codex's: `fuel.codex.accounts`) ---------- */

/** Codex's accounts, while Codex is on: the main one (its own home, `~/.codex`) first. */
export const codexAccounts = computed<ClaudeAccount[]>(() => codexFuel(fuel.value)?.accounts ?? []);

/** `agent`'s accounts. */
export function accountsOf(agent: Agent): ClaudeAccount[] {
  return agent === "codex" ? codexAccounts.value : accounts.value;
}

/** The one `agent`'s new sessions Pitwall starts use. */
export function accountInUseOf(agent: Agent): ClaudeAccount | null {
  return accountsOf(agent).find((account) => account.inUse) ?? null;
}

export function accountByIdOf(agent: Agent, id: string | null | undefined): ClaudeAccount | null {
  return id ? (accountsOf(agent).find((account) => account.id === id) ?? null) : null;
}

/** The name a terminal tab shows beside its own: the account it runs with (`TerminalView.account`,
 * a shell tab's too), when it is another than the main one and still listed. */
export function accountLabel(id: string | null | undefined): string | null {
  const account = accountById(id);
  return account && !account.main ? accountName(account) : null;
}

/* ---------- the note under the Fuel page's bar ---------- */

/** How long a note that only tells (switched, synced) stays. */
const NOTE_MS = 6_000;

/**
 * What the strip under the Fuel page's bar says after a switch, or of a sync asked for there.
 * `switching`: the core hasn't answered the switch yet (nothing shows); `switched`: the switch was
 * the user's, just now (the strip's first sentence). `error`: why a sync asked for failed, when no
 * server's name says it.
 */
export type AccountNote = {
  id: string;
  /** Whose account: a missing one is Claude's. */
  agent?: Agent;
  state: "switching" | "switched" | AgentAccountSync["state"];
  switched: boolean;
  report: AgentSyncReport | null;
  error: string | null;
  /** Counted up at each change: the timer set for one never clears a later one. */
  seq: number;
};

export const accountNote = ref<AccountNote | null>(null);

let seq = 0;
let timer: number | undefined;

function setNote(next: Omit<AccountNote, "seq"> | null): void {
  window.clearTimeout(timer);
  if (!next) {
    accountNote.value = null;
    return;
  }
  const note = { ...next, seq: ++seq };
  accountNote.value = note;
  // What only tells stays for a moment; a sync under way, one that failed and one that differs
  // stay until something is done about them.
  if (note.state === "switched" || note.state === "done") {
    timer = window.setTimeout(() => {
      if (accountNote.value?.seq === note.seq) accountNote.value = null;
    }, NOTE_MS);
  }
}

/** The strip is put away: what it said was seen (the Fuel page left), or acted on. */
export function dismissNote(): void {
  const note = accountNote.value;
  if (note && (note.state === "failed" || note.state === "differs")) setNote(null);
}

/** Whether `note` is of `agent`'s account `id`. */
function isOf(note: AccountNote | null, id: string, agent: Agent): boolean {
  return note?.id === id && (note.agent ?? "claude") === agent;
}

function heard(event: AgentAccountSync, agent: Agent): void {
  const note = accountNote.value;
  const mine = isOf(note, event.id, agent);
  // The sync of an account switched away from while it ran breaks into nothing told of another.
  if (note && !mine && (note.state === "switching" || note.state === "syncing" || event.id !== accountInUseOf(agent)?.id)) return;
  // A switch this window asked for keeps its first sentence through the sync that follows it.
  const switched = mine && !!note?.switched;
  setNote({ id: event.id, agent, state: event.state, switched, report: "report" in event ? event.report : null, error: null });
}

/** The event an agent's accounts are told on: Claude's names as before, Codex's with `codex-`. */
function eventOf(agent: Agent, name: "account-sync" | "account-login"): string {
  return agent === "codex" ? `codex-${name}` : name;
}

/** The main window's: hears `account-sync` and `codex-account-sync` (sent to it alone). */
export async function connectAccounts(): Promise<UnlistenFn> {
  const each = await Promise.all((["claude", "codex"] as const).map((agent) => getCurrentWindow().listen<AgentAccountSync>(eventOf(agent, "account-sync"), (event) => heard(event.payload, agent))));
  return () => each.forEach((unlisten) => unlisten());
}

/** `account-login` (Codex's `codex-account-login`), while the add dialog is open: its sign-in
 * going on. */
export function listenLogin(handler: (event: AgentAccountLogin) => void, agent: Agent = "claude"): Promise<UnlistenFn> {
  return getCurrentWindow().listen<AgentAccountLogin>(eventOf(agent, "account-login"), (event) => handler(event.payload));
}

/**
 * Switches the account in use (the panel's rows, Settings' "Use", the add dialog's "Switch now").
 * Rejects with the core's message; the strip then says nothing of it.
 */
export async function switchAccount(id: string, agent: Agent = "claude"): Promise<void> {
  setNote({ id, agent, state: "switching", switched: true, report: null, error: null });
  try {
    await (agent === "claude" ? api.accountUse(id) : api.agentAccountUse(agent, id));
  } catch (reason) {
    if (isOf(accountNote.value, id, agent) && accountNote.value?.state === "switching") setNote(null);
    throw reason;
  }
  // A sync's event may have come before the answer: it says more.
  if (isOf(accountNote.value, id, agent) && accountNote.value?.state === "switching") setNote({ id, agent, state: "switched", switched: true, report: null, error: null });
}

/** The strip's "Sync" and "Try again": the account's MCP servers brought level now. */
export async function syncAccount(id: string, agent: Agent = "claude"): Promise<void> {
  const note = accountNote.value;
  const switched = isOf(note, id, agent) && !!note?.switched;
  setNote({ id, agent, state: "syncing", switched, report: null, error: null });
  const syncing = (): boolean => isOf(accountNote.value, id, agent) && accountNote.value?.state === "syncing";
  try {
    const report: AgentSyncReport = await (agent === "claude" ? api.accountSync(id) : api.agentAccountSync(agent, id));
    // Unless an event has told how it ended already.
    if (syncing()) {
      const failed = report.servers.some((server) => server.state === "failed");
      setNote({ id, agent, state: failed ? "failed" : "done", switched, report, error: null });
    }
  } catch (reason) {
    if (syncing()) setNote({ id, agent, state: "failed", switched, report: null, error: String(reason) });
  }
}

/* ---------- when a Claude account's sign-in ends (`claude_login_ends`) ---------- */

/** What Claude Code said of each Claude account's sign-in, by the account's id: only those it
 * answered for (none from a Claude Code before 2.1.318, none while Claude is off). In memory only. */
export const loginEnds = ref<Record<string, LoginEnd>>({});

let askingEnds = false;
let askAgain = false;

/** Asks the core again (it keeps each answer 10 minutes): as Settings › Claude shows, as the Fuel
 * page does and its account panel opens, after a renewal. One asking at a time; one asked for
 * meanwhile follows it. */
export async function askLoginEnds(): Promise<void> {
  if (askingEnds) {
    askAgain = true;
    return;
  }
  askingEnds = true;
  try {
    do {
      askAgain = false;
      try {
        const listed = await api.claudeLoginEnds();
        loginEnds.value = Object.fromEntries(listed.map((end) => [end.id, end]));
      } catch {
        // An older core: nothing is shown.
      }
    } while (askAgain);
  } finally {
    askingEnds = false;
  }
}

/** Claude Code warns of a sign-in's end three days before it: so does Pitwall. */
const NEAR_DAYS = 3;
const DAY_MS = 86_400_000;

/** How an account's sign-in end is shown. `far`: plain; `near`: within three days, in the warning's
 * colour; `ended` and `out` (signed out): in the crash's, with "Renew sign-in". */
export type LoginEndView = {
  level: "far" | "near" | "ended" | "out";
  /** The line under the account's name. */
  text: string;
  /** The full date and time, as its tooltip. */
  title: string | null;
  /** Its sign-in can be renewed from here. */
  renew: boolean;
};

/** Local days from `from`'s to `to`'s. */
function daysApart(from: number, to: number): number {
  const day = (at: number): number => {
    const date = new Date(at);
    return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  };
  return Math.round((day(to) - day(from)) / DAY_MS);
}

const fullDates = new Map<string, Intl.DateTimeFormat>();

/** `Monday 16 November 2026 at 13:19`, in the app's language. */
function fullDate(at: number): string {
  let format = fullDates.get(language.value);
  if (!format) {
    format = new Intl.DateTimeFormat(language.value, { dateStyle: "full", timeStyle: "short", hourCycle: "h23" });
    fullDates.set(language.value, format);
  }
  return format.format(at);
}

/** What Claude account `id`'s row says of its sign-in's end at `now`; null when nothing is known. */
export function loginEndView(id: string, now: number): LoginEndView | null {
  const end = loginEnds.value[id];
  if (!end) return null;
  if (end.loggedIn === false) return { level: "out", text: t("accounts.loginEnd.out"), title: null, renew: true };
  if (end.endsAt === null) return null;
  const at = end.endsAt;
  const title = fullDate(at);
  const date = dayShort(dayName(at));
  if (at <= now) return { level: "ended", text: t("accounts.loginEnd.ended", { date }), title, renew: true };
  const days = daysApart(now, at);
  if (days <= 0) return { level: "near", text: t("accounts.loginEnd.today", { time: clock(at) }), title, renew: true };
  if (days === 1) return { level: "near", text: t("accounts.loginEnd.tomorrow", { time: clock(at) }), title, renew: true };
  if (days <= NEAR_DAYS) return { level: "near", text: t("accounts.loginEnd.near", { count: days, date: `${date} ${clock(at)}` }), title, renew: true };
  return { level: "far", text: t("accounts.loginEnd.far", { count: days, date }), title, renew: false };
}

/** The date Claude account `id`'s sign-in ended on, `8 Nov`; null while it hasn't. */
export function loginEndedOn(id: string, now: number): string | null {
  const at = loginEnds.value[id]?.endsAt;
  return at != null && at <= now ? dayShort(dayName(at)) : null;
}

/** `account-renew`, while a renewal's dialog is open: its sign-in going on. */
export function listenRenew(handler: (event: AccountRenew) => void): Promise<UnlistenFn> {
  return getCurrentWindow().listen<AccountRenew>("account-renew", (event) => handler(event.payload));
}
