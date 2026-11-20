import { listen } from "@tauri-apps/api/event";
import { reactive } from "vue";

import { api } from "../store";

/**
 * Voice messages in the chat (SM2 + SM3, the user's word, 20 Nov): the mic beside Send, tapped or
 * held, or ⇧⌘M. The core records and writes the take down on this machine (src-tauri/src/voice);
 * this page only drives it by the take's id and hears of it on the `voice` event. Nothing of the
 * sound comes here: the level for the meter, then the text.
 *
 * One take at a time in the whole app (one microphone): `voice.take` is it, whichever chat it is
 * for. A press shorter than `HOLD_MS` is a tap (the take ends by itself on quiet); a longer one is
 * a hold, which ends when it is let go and is cancelled when let go away from the button.
 */

export type VoiceModel = "small" | "large";

export type VoiceStatus = {
  /** Whether this machine's processor runs the engine. */
  supported: boolean;
  models: Array<{ model: VoiceModel; ready: boolean; bytes: number }>;
  downloading: { model: VoiceModel; done: number; total: number } | null;
};

type VoiceEvent =
  | { kind: "level"; id: string; level: number }
  | { kind: "transcribing"; id: string }
  | { kind: "done"; id: string; text: string | null; end: "stopped" | "quiet" | "limit" }
  | { kind: "failed"; id: string; problem: VoiceProblem }
  | { kind: "cancelled"; id: string }
  | { kind: "download"; model: VoiceModel; done: number; total: number }
  | { kind: "downloaded"; model: VoiceModel; ok: boolean; problem: DownloadProblem | null };

export type VoiceProblem = "denied" | "noDevice" | "failed" | "missing";
export type DownloadProblem = "network" | "http" | "size" | "hash" | "write";

/** How a take came out, as its chat hears it. */
export type TakeEnd =
  | { kind: "text"; text: string; limit: boolean }
  | { kind: "empty" }
  | { kind: "failed"; problem: VoiceProblem }
  | { kind: "cancelled" };

/** A press held longer than this is a hold; a shorter one, a tap. */
export const HOLD_MS = 300;
/** The levels the meters draw, the newest last: a wide row's worth (one each 50 ms). */
const LEVELS = 320;

export type Take = {
  id: string;
  /** The chat it is for. */
  chat: number;
  phase: "recording" | "transcribing";
  /** `press`: still down, not yet a tap or a hold; `tap`: ends by itself; `hold`: ends when let go. */
  how: "press" | "tap" | "hold";
  /** A hold dragged away from its button (or Esc while held): letting go sends nothing. */
  cancelArmed: boolean;
  started: number;
  levels: number[];
};

export const voice = reactive({
  status: null as VoiceStatus | null,
  /** The model being fetched, as the core last told of it. */
  download: null as { model: VoiceModel; done: number; total: number } | null,
  /** How the last fetch ended badly, until the next one. */
  downloadFailed: null as { model: VoiceModel; problem: DownloadProblem } | null,
  take: null as Take | null,
});

const ends = new Map<string, (end: TakeEnd) => void>();
const downloaded = new Set<(model: VoiceModel, ok: boolean) => void>();
let listening = false;
let count = 0;

function hear(event: VoiceEvent): void {
  const take = voice.take;
  switch (event.kind) {
    case "level":
      if (take && take.id === event.id) {
        take.levels.push(event.level);
        if (take.levels.length > LEVELS) take.levels.splice(0, take.levels.length - LEVELS);
      }
      return;
    case "transcribing":
      if (take && take.id === event.id) take.phase = "transcribing";
      return;
    case "done":
      return finish(event.id, event.text ? { kind: "text", text: event.text, limit: event.end === "limit" } : { kind: "empty" });
    case "failed":
      if (event.problem === "missing") void refreshVoice();
      return finish(event.id, { kind: "failed", problem: event.problem });
    case "cancelled":
      return finish(event.id, { kind: "cancelled" });
    case "download":
      voice.download = { model: event.model, done: event.done, total: event.total };
      return;
    case "downloaded":
      voice.download = null;
      voice.downloadFailed = event.ok || !event.problem ? null : { model: event.model, problem: event.problem };
      void refreshVoice().then(() => downloaded.forEach((then) => then(event.model, event.ok)));
      return;
  }
}

function finish(id: string, end: TakeEnd): void {
  if (voice.take?.id === id) voice.take = null;
  const then = ends.get(id);
  ends.delete(id);
  then?.(end);
}

/** Starts hearing the core's `voice` events (once) and reads where the models stand. */
export function useVoice(): void {
  if (!listening) {
    listening = true;
    void listen<VoiceEvent>("voice", (event) => hear(event.payload));
  }
  if (!voice.status) void refreshVoice();
}

export async function refreshVoice(): Promise<void> {
  try {
    const status = await api.voiceStatus();
    voice.status = status;
    if (status.downloading) voice.download = status.downloading;
  } catch {
    // The status comes again with the next look.
  }
}

export function modelReady(model: VoiceModel): boolean {
  return !!voice.status?.models.find((each) => each.model === model)?.ready;
}

export function modelBytes(model: VoiceModel): number {
  return voice.status?.models.find((each) => each.model === model)?.bytes ?? 0;
}

/** Calls `then` once each fetch ends (ok or not); the returned function stops it. */
export function onDownloaded(then: (model: VoiceModel, ok: boolean) => void): () => void {
  downloaded.add(then);
  return () => downloaded.delete(then);
}

export async function downloadModel(model: VoiceModel): Promise<void> {
  voice.downloadFailed = null;
  voice.download = { model, done: 0, total: modelBytes(model) };
  try {
    await api.voiceDownload(model);
  } catch {
    // Another fetch goes already: the status says which.
    await refreshVoice();
    voice.download = voice.status?.downloading ?? null;
  }
}

export function cancelDownload(): void {
  void api.voiceDownloadCancel();
}

/**
 * Starts a take for `chat`, pressed (`press`) or tapped by the keyboard at once. `then` hears how
 * it ended. Resolves with what kept it from starting, if anything: `missing` (no model), or the
 * core's word.
 */
export async function startTake(chat: number, how: Take["how"], language: string, prompt: string, then: (end: TakeEnd) => void): Promise<string | null> {
  const before = voice.take;
  if (before) cancelTake(before.id);
  const id = `chat-${chat}-${++count}`;
  voice.take = { id, chat, phase: "recording", how, cancelArmed: false, started: Date.now(), levels: [] };
  ends.set(id, then);
  try {
    await api.voiceStart(id, language, prompt);
    if (how === "tap") void api.voiceByItself(id);
    return null;
  } catch (reason) {
    ends.delete(id);
    if (voice.take?.id === id) voice.take = null;
    const word = String(reason);
    if (word === "missing") void refreshVoice();
    return word;
  }
}

/** The press was let go before `HOLD_MS`: the take ends by itself from now on. */
export function tapTake(id: string): void {
  const take = voice.take;
  if (take?.id !== id) return;
  take.how = "tap";
  void api.voiceByItself(id);
}

/** The press lasted: the take ends when it is let go. */
export function holdTake(id: string): void {
  const take = voice.take;
  if (take?.id === id && take.how === "press") take.how = "hold";
}

export function stopTake(id: string): void {
  void api.voiceStop(id);
}

/** The take ends for nothing; one being written down already is let go at once, its text unused. */
export function cancelTake(id: string): void {
  void api.voiceCancel(id);
  if (voice.take?.id === id && voice.take.phase === "transcribing") finish(id, { kind: "cancelled" });
}

/** The shortcut's keys: ⇧⌘M, or Ctrl+Shift+M off the Mac. */
export function isVoiceKey(event: KeyboardEvent, mac: boolean): boolean {
  const mod = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
  return mod && event.shiftKey && !event.altKey && event.code === "KeyM";
}

/** A take's length as the row shows it: `0:07`. */
export function takeTime(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** The project's name, whisper's hint for the words it will hear: the folder's last part. */
export function promptOf(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

/** A model's size as the system's own windows write it (decimal megabytes): `191 MB`. */
export function megabytes(bytes: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(Math.round(bytes / 1e6))} MB`;
}
