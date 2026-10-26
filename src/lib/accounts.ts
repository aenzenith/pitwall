import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, ref } from "vue";

import { t } from "./i18n";
import { api, fuel } from "./store";
import type { AccountLogin, AccountSync, ClaudeAccount, SyncReport } from "./types";

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
  state: "switching" | "switched" | AccountSync["state"];
  switched: boolean;
  report: SyncReport | null;
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

function heard(event: AccountSync): void {
  const note = accountNote.value;
  // The sync of an account switched away from while it ran breaks into nothing told of another.
  if (note && note.id !== event.id && (note.state === "switching" || note.state === "syncing" || event.id !== accountInUse.value?.id)) return;
  // A switch this window asked for keeps its first sentence through the sync that follows it.
  const switched = note?.id === event.id && note.switched;
  setNote({ id: event.id, state: event.state, switched, report: "report" in event ? event.report : null, error: null });
}

/** The main window's: hears `account-sync` (sent to it alone). */
export async function connectAccounts(): Promise<UnlistenFn> {
  return getCurrentWindow().listen<AccountSync>("account-sync", (event) => heard(event.payload));
}

/** `account-login`, while the add dialog is open: its sign-in going on. */
export function listenLogin(handler: (event: AccountLogin) => void): Promise<UnlistenFn> {
  return getCurrentWindow().listen<AccountLogin>("account-login", (event) => handler(event.payload));
}

/**
 * Switches the account in use (the panel's rows, Settings' "Use", the add dialog's "Switch now").
 * Rejects with the core's message; the strip then says nothing of it.
 */
export async function switchAccount(id: string): Promise<void> {
  setNote({ id, state: "switching", switched: true, report: null, error: null });
  try {
    await api.accountUse(id);
  } catch (reason) {
    if (accountNote.value?.id === id && accountNote.value.state === "switching") setNote(null);
    throw reason;
  }
  // A sync's event may have come before the answer: it says more.
  if (accountNote.value?.id === id && accountNote.value.state === "switching") setNote({ id, state: "switched", switched: true, report: null, error: null });
}

/** The strip's "Sync" and "Try again": the account's MCP servers brought level now. */
export async function syncAccount(id: string): Promise<void> {
  const note = accountNote.value;
  const switched = note?.id === id && note.switched;
  setNote({ id, state: "syncing", switched, report: null, error: null });
  try {
    const report = await api.accountSync(id);
    // Unless an event has told how it ended already.
    if (accountNote.value?.id === id && accountNote.value.state === "syncing") {
      const failed = report.servers.some((server) => server.state === "failed");
      setNote({ id, state: failed ? "failed" : "done", switched, report, error: null });
    }
  } catch (reason) {
    if (accountNote.value?.id === id && accountNote.value.state === "syncing") setNote({ id, state: "failed", switched, report: null, error: String(reason) });
  }
}
