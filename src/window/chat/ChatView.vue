<script lang="ts">
/** Where a conversation scrolled away from its end was left: the turn at the view's top (its key)
 * and how far above that top the turn begins. By the chat's own state, in memory only; a view
 * shown again draws from that turn on and stands where it stood. */
const places = new WeakMap<object, { key: string; offset: number }>();
</script>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, shallowRef, watch } from "vue";

import AgentMark from "../../components/AgentMark.vue";
import ClaudeDot from "../../components/ClaudeDot.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import type { Shot } from "../../lib/cardImages";
import { ChatFind } from "../../lib/chat/find";
import { startChat } from "../../lib/chat/open";
import {
  chainBefore,
  chainNow,
  chatModel,
  chatSettings,
  contextFor,
  draftFor,
  dropQueued,
  limitClosed,
  limitKey,
  loadChat,
  markUsed,
  noteControl,
  registerComposer,
  type ChangeEntry,
  type DetailTab,
  type ChatEntry,
  type CommandEntry,
  type TaskEntry,
  type UserEntry,
} from "../../lib/chat/store";
import { parseCommand, skillNames } from "../../lib/chat/commands";
import { noteInfo, takeHandOff } from "../../lib/chat/draft";
import { outgoing, scrub } from "../../lib/chat/composer";
import { EXAMPLES_MOST, chatExamples, keepFirstExamples, setExamples } from "../../lib/chat/examples";
import { planTitle, STARTS_IN, planModeOf } from "../../lib/chat/plan";
import { CHAT_PLAN, CHAT_ROW, type ChatDialogOpen, type ChatMessageRef, type ComposerApi, type PlanMode, type TerminalRef } from "../../lib/chat/ui";
import type { ChatAnswer, ChatControl, ChatOutgoing, ChatPending } from "../../lib/chat/wire";
import { language, t } from "../../lib/i18n";
import { useNativeMenu, type MenuEntry } from "../../lib/nativeMenu";
import { revealTab } from "../../lib/panel";
import { api, snapshot, visible } from "../../lib/store";
import { FIND_LIMIT, findOptions, finds, hostChat } from "../../lib/terminals";
import type { TerminalView } from "../../lib/types";
import ImageLightbox from "../board/ImageLightbox.vue";
import TerminalFind from "../TerminalFind.vue";
import ChatCard from "./ChatCard.vue";
import ChatIcon from "./ChatIcon.vue";
import ChatLongText from "./ChatLongText.vue";
import ChatTurn, { type Turn } from "./ChatTurn.vue";
import Composer from "./Composer.vue";
import DetailCard from "./DetailCard.vue";
import PlanScreen from "./PlanScreen.vue";
import PlanStart from "./PlanStart.vue";
import ChatDialogHost from "./dialogs/ChatDialogHost.vue";
import TodoList from "./TodoList.vue";

/**
 * A chat tab (docs/SOHBET.md, TS3), where a terminal tab draws its xterm: the conversation on the
 * left with the composer at its foot (the cards waiting on the user over it), and over them all
 * the status line, whose buttons open Görevler, Değişiklikler and Komutlar in a card. It has no
 * PTY: the panel only gives it the keyboard (lib/terminals: `hostChat`).
 */
const props = defineProps<{ view: TerminalView }>();
const emit = defineEmits<{ settings: [] }>();

const id = computed(() => props.view.id);
const path = computed(() => props.view.path);
const model = computed(() => chatModel(props.view.id));
const settings = chatSettings;
const ui = computed(() => model.value.ui);

const root = ref<HTMLElement | null>(null);
const scroller = ref<HTMLElement | null>(null);
const thread = ref<HTMLElement | null>(null);
const composer = ref<(InstanceType<typeof Composer> & ComposerApi) | null>(null);

/* ---------- reading the chat ---------- */

onMounted(() => void loadChat(id.value, model.value.loaded));
// Back from out of sight (the window hidden): what came meanwhile is read from the last frame.
watch(visible, (shown) => {
  if (shown) void loadChat(id.value, true);
});

const session = computed(() => model.value.session ?? props.view.chat?.session ?? null);

// Bookmarks are the session's (message uuids in Pitwall's own file, no text).
watch(
  session,
  (now) => {
    if (!now) return;
    void api
      .chatBookmarks(now)
      .then((list) => (model.value.bookmarks = new Set(list)))
      .catch(() => undefined);
  },
  { immediate: true },
);

/* ---------- the turns ---------- */

const all = computed<ChatEntry[]>(() => [...model.value.history, ...model.value.entries]);

const turns = computed<Turn[]>(() => {
  const out: Turn[] = [];
  let current: Turn | null = null;
  for (const entry of all.value) {
    if (entry.kind === "user") {
      current = { key: entry.key, user: entry, items: [], last: false };
      out.push(current);
    } else {
      if (!current) {
        current = { key: `lead-${entry.key}`, user: null, items: [], last: false };
        out.push(current);
      }
      current.items.push(entry);
    }
  }
  if (out.length) out[out.length - 1].last = true;
  return out;
});

/** A long conversation draws its latest turns; older ones come on asking (V125). */
const PAGE = 40;
const shownCount = ref(PAGE);
const hiddenTurns = computed(() => Math.max(0, turns.value.length - shownCount.value));

/* Of those, a view draws only the latest few at first and the older ones by itself right after,
   a few at a time over the top with the view held where it is: a chat's tab is drawn anew each
   time it is shown, and forty turns at once kept the window from answering for 170–200 ms
   (measured 17 Nov). `drawn` is how many of the page's turns are in the page so far; once all
   are, it is the page's own count, so a conversation that grows never loses its oldest turn to
   it. The button for what is beyond the page shows once the page is whole. */
const FIRST = 8;
const drawn = ref(FIRST);
const wanted = computed(() => Math.min(shownCount.value, turns.value.length));
const whole = computed(() => drawn.value >= wanted.value);
const shownTurns = computed(() => turns.value.slice(Math.max(0, turns.value.length - Math.min(drawn.value, shownCount.value))));

/* Turns drawn over the top push what is in view down, and go on pushing for a moment: a turn's
   Markdown comes in a little after the turn itself. So the first turn that was there is held
   where it is in the view (`held`: the turn, and how far down the conversation it begins): each
   time the conversation's height changes, the scroll position takes up what was added above it,
   before anything is painted. A view following its end is left to that. */
let held: { turn: HTMLElement; at: number; want: number | null; owed: number } | null = null;
let letGo: ReturnType<typeof setTimeout> | undefined;

function startOf(turn: HTMLElement, box: HTMLElement): number {
  return turn.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop;
}

/** Holds `turn` where it is, or `want` under the view's top (a view shown again, put back where
 * it was left: its turns have no height yet to scroll to, so the place is taken as they get it,
 * until the user scrolls). A turn held already stays the one. */
function hold(turn: HTMLElement | null, want: number | null = null): void {
  const box = scroller.value;
  clearTimeout(letGo);
  letGo = undefined;
  if (held?.turn.isConnected || !box) return;
  held = turn ? { turn, at: startOf(turn, box), want, owed: 0 } : null;
}

function keepHeld(): void {
  const box = scroller.value;
  if (!held || !box) return;
  if (!held.turn.isConnected) {
    held = null;
    return;
  }
  const at = startOf(held.turn, box);
  if (!following && (held.want !== null || at !== held.at)) {
    // A scroll position is a whole number of pixels here: the part lost is owed to the next
    // move, so the steps never add up to a drift.
    const to = held.want !== null ? at - held.want : box.scrollTop + at - held.at + held.owed;
    box.scrollTop = to;
    const lost = to - box.scrollTop;
    held.owed = held.want === null && Math.abs(lost) < 1 ? lost : 0;
  }
  held.at = at;
}

/** `count` more of the page's turns drawn over the top; what is in view stays where it is. */
async function drawMore(count: number): Promise<void> {
  hold(thread.value?.querySelector<HTMLElement>(":scope > [data-turn]") ?? null);
  const next = drawn.value + count;
  drawn.value = next >= wanted.value ? shownCount.value : next;
  await nextTick();
  keepHeld();
}

/** How many turns the next step draws, by what the last step took: its turns' Markdown, layout
 * and painting come after what is timed here and take a few times as long, and all of it is to
 * fit in a frame or two. */
let stride = 4;
let filler: ReturnType<typeof setTimeout> | undefined;
let filling = false;
let gone = false;

function fillSoon(delay: number): void {
  if (gone || filler !== undefined) return;
  filler = setTimeout(() => {
    filler = undefined;
    void fill();
  }, delay);
}

/** The rest of the page, step by step between frames. It waits while the user scrolls, unless the
 * top of what is drawn is near. */
async function fill(): Promise<void> {
  if (gone || filling) return;
  const box = scroller.value;
  if (!box || whole.value) return settle();
  if (Date.now() - userMoved < 200 && box.scrollTop > box.clientHeight) return fillSoon(100);
  filling = true;
  const began = performance.now();
  const count = stride;
  await drawMore(count);
  stride = Math.max(1, Math.min(6, Math.round((count * 6) / Math.max(1, performance.now() - began))));
  filling = false;
  if (whole.value) settle();
  else fillSoon(30);
}

/** Everything of the page is drawn: from here on the page's count alone says what shows, and the
 * turn held is let go once the last ones drawn have their height. */
function settle(): void {
  if (!whole.value) return;
  if (turns.value.length || model.value.loaded) drawn.value = shownCount.value;
  if (held && letGo === undefined) letGo = setTimeout(() => (held = null), 300);
}

/** Before the first paint of new turns: enough of them that the view is full and more (they are
 * measured before their Markdown is in, so they only get taller); the rest follow. */
async function cover(): Promise<void> {
  const box = scroller.value;
  if (!box || filling) return;
  filling = true;
  while (!gone && !whole.value && box.scrollHeight < box.clientHeight * 1.5) await drawMore(4);
  filling = false;
  if (whole.value) settle();
  else fillSoon(60);
}

/** The whole page at once: something is about to look through all of it. */
async function drawAll(): Promise<void> {
  clearTimeout(filler);
  filler = undefined;
  if (!whole.value) await drawMore(shownCount.value);
  settle();
}

// A turn that comes while the page still fills joins what is drawn: counted from the end, it would
// otherwise push the oldest drawn turn out, and a view scrolled up would lose its place.
watch(
  () => turns.value.length,
  (now, was) => {
    if (now > was && drawn.value < Math.min(shownCount.value, was)) drawn.value += now - was;
  },
);
// A conversation read after the view came up (a chat's first show) is drawn the same way.
watch(() => turns.value.length, () => void cover(), { flush: "post" });
onBeforeUnmount(() => {
  gone = true;
  clearTimeout(filler);
  clearTimeout(letGo);
});

async function showEarlier(): Promise<void> {
  shownCount.value += PAGE;
  await drawMore(PAGE);
  settle();
}

const commandsByTool = computed(() => {
  const map = new Map<string, CommandEntry>();
  for (const command of model.value.commands) if (command.tool) map.set(command.tool, command);
  return map;
});
const runs = computed(() => {
  const map = new Map<string, CommandEntry>();
  for (const command of model.value.commands) if (command.run) map.set(command.run, command);
  return map;
});
const changesByTool = computed(() => new Map<string, ChangeEntry>(model.value.changes.map((change) => [change.tool, change])));
const tasksByTool = computed(() => {
  const map = new Map<string, TaskEntry>();
  for (const task of model.value.tasks) if (task.tool) map.set(task.tool, task);
  return map;
});

/** The cards waiting on the user, and the tool calls they are about. */
const cards = computed(() => model.value.pending);
const pendingByTool = computed(() => {
  const map = new Map<string, ChatPending>();
  for (const each of cards.value) {
    const tool = (each.request as { tool_use_id?: string }).tool_use_id;
    if (each.request.subtype === "can_use_tool" && tool) map.set(tool, each);
  }
  return map;
});
const waitingTools = computed(() => new Set(pendingByTool.value.keys()));

/* ---------- Claude's plan on its own screen (PL5) ---------- */

/** The first card waiting, when it is Claude's plan: it shows on its screen over the chat, not as a
 * card over the composer; the cards after it wait their turn. */
const planCard = computed(() => {
  const first = cards.value[0];
  return first?.request.subtype === "can_use_tool" && first.request.tool_name === "ExitPlanMode" ? first : null;
});
const planTool = computed(() => (planCard.value ? ((planCard.value.request as { tool_use_id?: string }).tool_use_id ?? null) : null));
const planText = computed(() => {
  const input = planCard.value?.request.subtype === "can_use_tool" ? planCard.value.request.input : null;
  return typeof input?.plan === "string" ? input.plan : "";
});
const planFolded = computed(() => !!planCard.value && ui.value.planFolded === planCard.value.requestId);
/** An answered plan brought up from its row, to read. */
const reading = shallowRef<{ tool: string; plan: string } | null>(null);
watch(planCard, (card) => {
  if (card) reading.value = null;
});
const readingApproved = computed(() => {
  const tool = reading.value?.tool;
  const entry = tool ? all.value.find((each) => each.kind === "tool" && each.id === tool) : null;
  return entry?.kind === "tool" && !entry.denied && !!entry.result && !entry.result.error;
});
/** What the screen holds: the waiting plan, kept in the page while folded (hidden, so that it
 * opens again at once and as it was left: its place in the plan, its open field), else a plan
 * opened to read. */
const planShown = computed(() => {
  if (planCard.value && !(planFolded.value && reading.value)) return { key: planCard.value.requestId, tool: planTool.value, plan: planText.value, asking: true };
  if (reading.value) return { key: `read-${reading.value.tool}`, tool: reading.value.tool, plan: reading.value.plan, asking: false };
  return null;
});
const planVisible = computed(() => !!planShown.value && !(planShown.value.asking && planFolded.value));
// The screen covers the status line's card: it shuts as the screen opens.
watch(planVisible, (shown) => {
  if (shown) ui.value.card = null;
});

/** The bar stays under the screen while it grows out of it, and goes once the screen is whole:
 * the composer doesn't move under the screen's start. */
const planBarHeld = ref(false);
const planBarShown = computed(() => !!planCard.value && (planFolded.value || planBarHeld.value));

const planBar = ref<HTMLElement | null>(null);
const planBarSending = ref(false);
const planStartsIn = computed(() => t(STARTS_IN[planModeOf(id.value, ui.value.planMode)]));

/** Where the screen grows out of when it shows next: the box of the row or the bar clicked, taken
 * at the click (the bar is gone by the time the screen shows). */
let planFrom: DOMRect | null = null;

function planRow(tool: string | null | undefined): HTMLElement | null {
  return tool ? (thread.value?.querySelector<HTMLElement>(`[data-plan-row="${CSS.escape(tool)}"]`) ?? null) : null;
}

function foldPlan(): void {
  if (planCard.value) ui.value.planFolded = planCard.value.requestId;
}

function unfoldPlan(from: HTMLElement | null): void {
  planFrom = from?.getBoundingClientRect() ?? null;
  planBarHeld.value = planFolded.value;
  ui.value.planFolded = null;
}

async function startPlan(mode: PlanMode): Promise<void> {
  const card = planCard.value;
  if (!card || planBarSending.value) return;
  planBarSending.value = true;
  try {
    await answer(card, { kind: "allow", mode });
  } finally {
    planBarSending.value = false;
  }
}

provide(CHAT_PLAN, {
  open(tool, plan, from) {
    planFrom = from.getBoundingClientRect();
    if (planCard.value && planTool.value === tool) {
      planBarHeld.value = planFolded.value;
      ui.value.planFolded = null;
      return;
    }
    reading.value = { tool, plan };
  },
});

/* The screen grows out of what brought it (its row in the conversation, the bar over the
   composer; a plan that just came, its row when in sight, else the composer's place) and shrinks
   into the bar when folded, into its row when answered or closed (PL5D). Opening, its panel
   dissolves in over what brought it as its clip grows from that box to the whole, and the plan's
   words settle into it once it has most of its room; folding, the words go first, the panel
   shrinks onto the bar and dissolves into it at the end. Less motion asked of the system: it only
   fades. The animations are its own, kept to be undone when it shows again (it stays in the page
   while folded); a spinner's or a mark's are left alone. */
const reduceMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const WHOLE = "inset(0px 0px 0px 0px round 0px)";
const planRuns = new WeakMap<Element, Animation[]>();

function clipOf(from: HTMLElement | DOMRect | null, box: DOMRect, within: DOMRect | null, radius = 8): string | null {
  if (!from) return null;
  const r = from instanceof DOMRect ? from : from.getBoundingClientRect();
  if (!r.width || !r.height) return null;
  if (within && (r.bottom <= within.top || r.top >= within.bottom)) return null;
  const top = Math.max(0, r.top - box.top);
  const right = Math.max(0, box.right - r.right);
  const bottom = Math.max(0, box.bottom - r.bottom);
  const left = Math.max(0, r.left - box.left);
  return `inset(${top}px ${right}px ${bottom}px ${left}px round ${radius}px)`;
}

/** The screen's own runs: those left from its last show or fold undone, the new ones kept. */
function planRun(screen: HTMLElement, runs: (Animation | undefined)[]): { runs: Animation[]; finished: Promise<unknown> } {
  for (const old of planRuns.get(screen) ?? []) old.cancel();
  const kept = runs.filter((run): run is Animation => !!run);
  planRuns.set(screen, kept);
  return { runs: kept, finished: Promise.all(kept.map((run) => run.finished)) };
}

function planEnter(el: Element, done: () => void): void {
  const screen = el as HTMLElement;
  const box = screen.getBoundingClientRect();
  const view = scroller.value?.getBoundingClientRect() ?? null;
  const from = planFrom;
  planFrom = null;
  // Brought up by the user (the bar, a row): it takes the keys, as when it first came.
  if (from) screen.focus({ preventScroll: true });
  const whole = () => {
    planBarHeld.value = false;
    done();
  };
  const fromBar = planBarHeld.value;
  const clip = reduceMotion() ? null : (clipOf(from, box, null, fromBar ? 10 : 8) ?? clipOf(planRow(screen.dataset.tool), box, view) ?? clipOf(footBox.value, box, null));
  const words = screen.querySelector(".ps-in") ?? undefined;
  if (!clip) {
    planRun(screen, [screen.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 150, easing: "ease-out" })]).finished.then(whole, whole);
    return;
  }
  planRun(screen, [
    screen.animate([{ clipPath: clip }, { clipPath: WHOLE }], { duration: 340, easing: "cubic-bezier(0.22, 1, 0.36, 1)" }),
    screen.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 110, easing: "linear" }),
    words?.animate([{ opacity: 0, transform: "translateY(12px)" }, { opacity: 1, transform: "none" }], { duration: 240, delay: 110, easing: "cubic-bezier(0.2, 0, 0, 1)", fill: "backwards" }),
  ]).finished.then(whole, whole);
}

function planLeave(el: Element, done: () => void): void {
  const screen = el as HTMLElement;
  const box = screen.getBoundingClientRect();
  const view = scroller.value?.getBoundingClientRect() ?? null;
  // The bar is in the page already, but its ref is set only after this render.
  const into = planFolded.value ? (footBox.value?.querySelector<HTMLElement>(".plan-bar") ?? null) : null;
  const clip = reduceMotion() ? null : (clipOf(into, box, null, 10) ?? clipOf(planRow(screen.dataset.tool), box, view));
  const words = screen.querySelector(".ps-in") ?? undefined;
  const made = !clip
    ? planRun(screen, [screen.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 140, easing: "ease-in", fill: "forwards" })])
    : planRun(screen, [
        words?.animate([{ opacity: 1, transform: "none" }, { opacity: 0, transform: "translateY(8px)" }], { duration: 120, easing: "cubic-bezier(0.4, 0, 1, 1)", fill: "forwards" }),
        screen.animate([{ clipPath: WHOLE }, { clipPath: clip }], { duration: 280, easing: "cubic-bezier(0.4, 0, 0.2, 1)", fill: "forwards" }),
        screen.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 90, delay: 190, easing: "linear", fill: "forwards" }),
      ]);
  // Folded, it stays in the page, hidden: what its own runs held at their end is let go once it
  // is (a show that came on the way has runs of its own, left alone).
  const gone = () => {
    done();
    for (const run of made.runs) run.cancel();
    if (planRuns.get(screen) === made.runs) planRuns.delete(screen);
  };
  made.finished.then(gone, gone);
}
/** The skills the CLI lists, drawn apart wherever a message names them. */
const skills = computed(() => skillNames(model.value.info));

const busy = computed(() => model.value.state === "running" || model.value.state === "requires_action");
/** Lines no conversation is made of: a notice (a switch's "Set model to …"). */
const QUIET = new Set<ChatEntry["kind"]>(["notice"]);
/** Nothing said yet: the welcome shows over whatever quiet lines came. */
const empty = computed(() => !all.value.some((entry) => !QUIET.has(entry.kind)) && !model.value.queued.length);

/* ---------- time going by ---------- */

const tick = ref(Date.now());
let ticker: ReturnType<typeof setInterval> | undefined;
const ticking = computed(() => busy.value || model.value.commands.some((each) => each.state === "running") || model.value.tasks.some((each) => each.state === "running"));
watch(
  ticking,
  (on) => {
    clearInterval(ticker);
    ticker = undefined;
    tick.value = Date.now();
    if (on) ticker = setInterval(() => (tick.value = Date.now()), 1000);
  },
  { immediate: true },
);
onBeforeUnmount(() => clearInterval(ticker));

// A row opened in the conversation draws a command's or a subagent's part with these.
provide(CHAT_ROW, { id, now: tick });

/** The ring before the words at work: the tab's, without words of its own (the line says it). */
const WORKING = { phase: "working", text: "" } as const;

/** What Claude is at while a turn runs (V34): the words under the last turn. */
/* A yes or no of its own: every frame moves `lastFrame`, and the line is worked out again only
   when the answer changes, not at each word that streams in. */
const silent = computed(() => tick.value - model.value.lastFrame > 60_000);
const phase = computed(() => {
  if (!busy.value || cards.value.length) return null;
  const m = model.value;
  const working = m.commands.some((each) => each.state === "running" && !each.background) || m.tasks.some((each) => each.state === "running" && !each.background);
  if (silent.value && !working) return { text: t("chat.status.silent"), stuck: true };
  if (m.status === "compacting") return { text: t("chat.status.compacting"), stuck: false };
  const last = m.entries[m.entries.length - 1];
  if (last?.kind === "thinking" && last.streaming && !last.ended) return { text: t("chat.status.thinking"), stuck: false };
  if (last?.kind === "text" && last.streaming) return { text: t("chat.status.writing"), stuck: false };
  if (working || (last?.kind === "tool" && !last.result)) return { text: t("chat.status.working"), stuck: false };
  return { text: t("chat.status.thinking"), stuck: false };
});

const elapsed = computed(() => {
  const started = model.value.turnStarted;
  if (!started) return "";
  const seconds = Math.max(0, Math.round((tick.value - started) / 1000));
  return seconds < 60 ? t("chat.time.seconds", { count: seconds }) : t("chat.time.minutes", { minutes: Math.floor(seconds / 60), seconds: seconds % 60 });
});

/* ---------- following the end ---------- */

let following = true;
function atEnd(): boolean {
  const box = scroller.value;
  return !box || box.scrollHeight - box.scrollTop - box.clientHeight < 48;
}

/* Only the user's own scroll (the wheel, a key, a finger, its scroll bar held) lets the end go: the
   conversation growing moves it too (a session's earlier messages coming in, code coloured, a
   picture loaded), and a scroll event read in between used to stop the following halfway, so a
   session opened again started in its middle (the user's word, 14 Nov). */
let userMoved = 0;
let barHeld = false;
function userScrolls(): void {
  userMoved = Date.now();
  if (held) held.want = null;
}
function pressScroller(event: PointerEvent): void {
  // A press on the scroller itself, not on what it holds: its scroll bar.
  if (event.target !== scroller.value) return;
  barHeld = true;
  userScrolls();
  window.addEventListener(
    "pointerup",
    () => {
      barHeld = false;
      userScrolls();
    },
    { once: true },
  );
}

function onScroll(): void {
  if (atEnd()) following = true;
  else if (barHeld || Date.now() - userMoved < 800) following = false;
  ui.value.stuck = following;
  ui.value.scroll = scroller.value?.scrollTop ?? null;
  keepPlace();
  measureUnder();
  // The top of what is drawn comes near while older turns are still to come: they come now.
  const box = scroller.value;
  if (box && !whole.value && box.scrollTop < box.clientHeight) {
    clearTimeout(filler);
    filler = undefined;
    void fill();
  }
}

/** The turn at the view's top, kept for the next time this chat is shown (`places`). */
function keepPlace(): void {
  const box = scroller.value;
  const list = following ? null : thread.value?.querySelectorAll<HTMLElement>(":scope > [data-turn]");
  if (!box || !list?.length) {
    places.delete(ui.value);
    return;
  }
  const edge = box.getBoundingClientRect().top;
  let low = 0;
  let high = list.length - 1;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (list[mid].getBoundingClientRect().bottom > edge) high = mid;
    else low = mid + 1;
  }
  places.set(ui.value, { key: list[low].dataset.turn ?? "", offset: edge - list[low].getBoundingClientRect().top });
}

/** How much of the conversation lies under its bottom edge, up to the fade's height: the fade over
 * the foot shows as much of itself, so the conversation goes under the status line softly and its
 * last row, scrolled to the end, is never dimmed (the user's word, 16 Nov: "sert geçiş yok"). */
const FADE = 28;
const under = ref(0);
function measureUnder(): void {
  const box = scroller.value;
  under.value = box ? Math.min(FADE, Math.max(0, box.scrollHeight - box.scrollTop - box.clientHeight)) : 0;
}

function toEnd(smooth = false): void {
  const box = scroller.value;
  if (!box) return;
  following = true;
  userMoved = 0;
  ui.value.stuck = true;
  // The empty chat's greeting reads from its top in a short pane.
  if (empty.value) return;
  if (smooth && !matchMedia("(prefers-reduced-motion: reduce)").matches) box.scrollTo({ top: box.scrollHeight, behavior: "smooth" });
  else pin(box);
}

/* The view put at its end at once. WebKit scrolls the box in the window's own process: a box whose
   room grows under a view at its end (the composer shrinking after a send) is set back to its end
   in the page, but went on being drawn where it stood, an empty band at its bottom until the next
   scroll or the next words (the user's word, 18 Nov: "mesajı ilk atınca altta boşluk oluşuyor").
   So the end is asked for even where the page has the box there already, a pixel short of it
   first: the window's process is told where the box is, and draws it there. */
function pin(box: HTMLElement): void {
  const end = box.scrollHeight - box.clientHeight;
  if (end <= 0) return;
  if (Math.abs(box.scrollTop - end) < 1) box.scrollTop = end - 1;
  box.scrollTop = end;
}

/** A message sent brings the view to its end: at once, not in a glide that starts before the
 * message is in the conversation and the composer has shrunk (it ended where the end was before
 * them), and again once both have happened; the view follows whatever comes after. */
function followSent(): void {
  toEnd();
  void nextTick(() => requestAnimationFrame(() => following && toEnd()));
}

/** The conversation's scroll bar takes room beside it, and the thread is centred in what is left:
 * the foot is laid out in the same width (`--bar`), so the composer stands flush with the rows
 * (the user's word, 15 Nov). The thread's size changes as the bar comes or goes. */
const bar = ref(0);
function measureBar(): void {
  const box = scroller.value;
  if (box) bar.value = Math.max(0, box.offsetWidth - box.clientWidth);
}

/* A view left away from its end draws from the turn it was left at (and one before it), so it
   can stand there again; one whose place isn't known, or is beyond the page, draws the whole page
   and takes the scroll position it had. */
const place = ui.value.stuck || ui.value.scroll === null ? null : (places.get(ui.value) ?? null);
const placeAt = place ? turns.value.findIndex((turn) => turn.key === place.key) : -1;
const placed = placeAt >= 0 && turns.value.length - placeAt < PAGE;
if (placed) drawn.value = Math.max(FIRST, turns.value.length - placeAt + 1);
else if (!ui.value.stuck && ui.value.scroll !== null) drawn.value = PAGE;

let grower: ResizeObserver | null = null;
onMounted(() => {
  following = ui.value.stuck;
  void nextTick(() => {
    const box = scroller.value;
    if (!following && ui.value.scroll !== null && box) {
      const turn = placed && place ? thread.value?.querySelector<HTMLElement>(`:scope > [data-turn="${CSS.escape(place.key)}"]`) : null;
      if (turn && place) {
        hold(turn, -place.offset);
        keepHeld();
      } else box.scrollTop = ui.value.scroll;
    } else toEnd();
    void cover();
  });
  grower = new ResizeObserver(() => {
    if (following) toEnd();
    keepHeld();
    if (lookAgain !== undefined) refind();
    measureBar();
    measureUnder();
  });
  if (thread.value) grower.observe(thread.value);
  // The view's own room too: the foot grows (the status line comes, the composer takes lines) and
  // shrinks (a message sent), and a view at its end stays there.
  if (scroller.value) grower.observe(scroller.value);
});
onBeforeUnmount(() => grower?.disconnect());

/* ---------- the status line and its card (CK3) ---------- */

/** The foot's top row while there is anything to tell, over the notices and the cards (the user's
 * word, 16 Nov: "her zaman üst sırada"): what Claude is at on the left, and always at
 * the right end, as buttons with no ground (the user's word, 16 Nov: "hep sağa sabit", "zeminsiz"),
 * the session's tasks, changed files and commands, each opening the card at its tab. The card stands
 * over the line, the conversation lightly dimmed under it (the user's word, 16 Nov). */
const fileCount = computed(() => new Set(model.value.changes.map((change) => change.file)).size);
const runningTasks = computed(() => model.value.tasks.filter((task) => task.state === "running").length);
const runningCommands = computed(() => model.value.commands.filter((command) => command.state === "running").length);
const waitingCommands = computed(() => model.value.commands.filter((command) => command.tool && pendingByTool.value.has(command.tool)).length);
/** The side questions in the line (the user's word, 16 Nov: "1 ara soru", opened again from
 * there), from the panel's opening until it is closed, open or folded away alike ("kapanana kadar
 * o kısımda kalacak"): how many, whether an answer is still coming, whether the panel shows; null
 * once it was closed. With none asked yet the button is named as the panel is. */
const btwLine = computed(() => {
  const btw = ui.value.btw;
  if (!btw) return null;
  const count = btw.items.length;
  return {
    count,
    label: count ? t("chat.side.btwCount", { count }) : t("chatDialog.btw.title"),
    busy: btw.items.some((item) => item.busy),
    open: dialog.value?.kind === "btw",
  };
});
const lineShown = computed(() => !!phase.value || model.value.tasks.length > 0 || fileCount.value > 0 || model.value.commands.length > 0 || !!btwLine.value);
const WAITING = { phase: "waiting", text: "" } as const;

type Ghost = { tab: DetailTab; count: number; label: string; icon: "agent" | "pencil" | "terminal"; live: { phase: "waiting" | "working"; text: string } | null; state: string | null };

/** The line's buttons, those with anything to open: a ring while a task or a command runs, a dot
 * while a command waits for permission. */
const ghosts = computed<Ghost[]>(() => {
  const m = model.value;
  const list: Ghost[] = [];
  if (m.tasks.length) {
    const running = runningTasks.value;
    list.push({ tab: "tasks", count: m.tasks.length, label: t("chat.side.taskCount", { count: m.tasks.length }), icon: "agent", live: running ? WORKING : null, state: running ? t("chat.side.running", { count: running }) : null });
  }
  if (fileCount.value) list.push({ tab: "changes", count: fileCount.value, label: t("chat.side.files", { count: fileCount.value }), icon: "pencil", live: null, state: null });
  if (m.commands.length) {
    const waiting = waitingCommands.value;
    const running = runningCommands.value;
    list.push({
      tab: "commands",
      count: m.commands.length,
      label: t("chat.side.commandCount", { count: m.commands.length }),
      icon: "terminal",
      live: waiting ? WAITING : running ? WORKING : null,
      state: waiting ? t("chat.side.waiting", { count: waiting }) : running ? t("chat.side.running", { count: running }) : null,
    });
  }
  return list;
});

function openBtw(): void {
  ui.value.card = null;
  dialog.value = { kind: "btw" };
}

/** A row in the conversation opens under itself; the card is the whole list (the user's word, 16 Nov). */
function toggleCard(tab: DetailTab): void {
  ui.value.card = ui.value.card === tab ? null : tab;
}

const mainBox = ref<HTMLElement | null>(null);
const footBox = ref<HTMLElement | null>(null);
const lineBox = ref<HTMLElement | null>(null);
/** Where the line's top is in the main column, and that column's height: the card's room. */
const lineTop = ref(0);
const mainHeight = ref(0);
function measureLine(): void {
  const main = mainBox.value;
  const line = lineBox.value;
  if (!main || !line) return;
  const box = main.getBoundingClientRect();
  mainHeight.value = box.height;
  lineTop.value = Math.max(0, Math.round(line.getBoundingClientRect().top - box.top));
}
/** The card is as tall as what it shows, at most about half the view, and never over its top. */
const cardMost = computed(() => Math.max(120, Math.min(lineTop.value - 12, Math.max(220, Math.round(mainHeight.value * 0.5)))));
let liner: ResizeObserver | null = null;
onMounted(() => {
  liner = new ResizeObserver(measureLine);
  if (mainBox.value) liner.observe(mainBox.value);
  if (footBox.value) liner.observe(footBox.value);
});
watch([lineBox, () => ui.value.card], () => void nextTick(measureLine));
onBeforeUnmount(() => liner?.disconnect());

/** Esc shuts the card first, wherever the keyboard is in this view (the composer too), unless a
 * dialog over it has the key. */
function escCard(event: KeyboardEvent): void {
  if (event.key !== "Escape" || event.isComposing || !ui.value.card) return;
  if (long.value || dialog.value || lightStart.value !== null) return;
  const target = event.target;
  if (!(target instanceof Node) || !root.value?.contains(target)) return;
  event.preventDefault();
  event.stopPropagation();
  ui.value.card = null;
}
watch(
  () => !!ui.value.card,
  (open) => {
    if (open) window.addEventListener("keydown", escCard, true);
    else window.removeEventListener("keydown", escCard, true);
  },
  { immediate: true },
);
onBeforeUnmount(() => window.removeEventListener("keydown", escCard, true));

/* ---------- the composer and what it asks for ---------- */

const notice = ref<{ text: string; terminal?: boolean } | null>(null);
const confirmTerminal = ref<string | null>(null);
const dialog = ref<ChatDialogOpen | null>(null);
const long = shallowRef<{ title: string; text: string } | null>(null);

/* ---------- the chat's pictures shown large ---------- */

/** Every picture of the chat in its order, those sent and those Claude read (a tool's result,
 * while the chat holds it), gathered as one is clicked: ← → step through them all (the user's
 * word, 15 Nov). A key: `<message key>:<index>` or `<tool id>:<index>`, the `data-shot` of its
 * small one. */
type ChatShot = Shot & { key: string };
const lightShots = shallowRef<ChatShot[]>([]);
const lightStart = ref<number | null>(null);
function showPicture(key: string): void {
  const list: ChatShot[] = [];
  const add = (at: string, mediaType: string, data: string) => list.push({ n: list.length + 1, src: `data:${mediaType};base64,${data}`, key: at });
  for (const turn of turns.value) {
    turn.user?.images.forEach((image, i) => add(`${turn.user?.key}:${i}`, image.mediaType, image.data));
    for (const entry of turn.items) if (entry.kind === "tool") entry.result?.pictures.forEach((picture, i) => add(`${entry.id}:${i}`, picture.mediaType, picture.data));
  }
  const at = list.find((each) => each.key === key);
  if (!at) return;
  lightShots.value = list;
  lightStart.value = at.n;
}
function lightThumb(n: number): HTMLElement | null {
  const key = lightShots.value[n - 1]?.key;
  return key ? (root.value?.querySelector<HTMLElement>(`[data-shot="${CSS.escape(key)}"] img`) ?? null) : null;
}

const terminals = computed<TerminalRef[]>(() => {
  const project = snapshot.value?.projects.find((each) => each.path === path.value);
  return (project?.terminals ?? []).filter((view) => view.kind !== "chat").map((view) => ({ id: view.id, name: view.name }));
});
const context = computed(() => contextFor(path.value));
const contextShare = computed(() => {
  const { contextUsed, contextWindow } = model.value;
  return contextUsed !== null && contextWindow ? Math.min(1, contextUsed / contextWindow) : null;
});
const exited = computed(() => model.value.state === "exited");
const starting = computed(() => model.value.state === "starting" && !model.value.info);

// What the CLI says of itself is what the empty panel's chat box offers before its chat is there
// (lib/chat/draft); a line that box couldn't run is run here, once the chat is ready for it.
watch(
  () => model.value.info,
  (info) => info && noteInfo(path.value, info),
  { immediate: true },
);
watch(
  () => !!composer.value && !!model.value.info && !starting.value && !exited.value,
  (ready) => {
    const line = ready ? takeHandOff(id.value) : null;
    if (line) composer.value?.run(line);
  },
  { immediate: true },
);

function fail(error: unknown, terminal = false): void {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  notice.value = { text: text ? t("chat.notice.failedWhy", { why: text }) : t("chat.notice.failed"), terminal };
}

async function send(message: ChatOutgoing): Promise<void> {
  markUsed(path.value, id.value);
  notice.value = null;
  if (settings.value.scrollOnSend) followSent();
  try {
    await api.chatSend(id.value, message);
  } catch (error) {
    fail(error);
  }
}

function shell(line: string): void {
  markUsed(path.value, id.value);
  if (settings.value.scrollOnSend) followSent();
  void api.chatShell(id.value, line).catch(fail);
}

async function interrupt(): Promise<void> {
  try {
    const receipt = await api.chatInterrupt(id.value, false);
    if (receipt?.cancelled?.length) dropQueued(id.value, receipt.cancelled);
  } catch (error) {
    fail(error);
  }
}

/** A control, and what follows it (`/plan <task>`) once the CLI took it: never before, so the task
 * never runs in the mode it was meant to leave. */
async function control(asked: ChatControl, then?: ChatOutgoing): Promise<void> {
  try {
    await api.chatControl(id.value, asked);
    noteControl(id.value, asked);
  } catch {
    // The CLI doesn't answer it in this version: the terminal has it. What was to follow is the
    // user's still, back in the composer.
    notice.value = { text: t("chat.notice.unanswered"), terminal: true };
    if (then) composer.value?.setText(then.text);
    return;
  }
  if (then) await send(then);
}

async function cancelQueued(uuid: string): Promise<void> {
  try {
    await api.chatControl(id.value, { kind: "cancelQueued", message: uuid });
    dropQueued(id.value, [uuid]);
  } catch (error) {
    fail(error);
  }
}

/** The cards whose answer is on its way (a second click waits for the first). */
const answering = new Set<string>();

async function answer(pending: ChatPending, given: ChatAnswer): Promise<void> {
  if (answering.has(pending.requestId)) return;
  answering.add(pending.requestId);
  try {
    await api.chatAnswer(id.value, pending.requestId, given);
    model.value.pending = model.value.pending.filter((each) => each.requestId !== pending.requestId);
  } catch (error) {
    // Refused (an answer the core won't build), or no longer waiting (withdrawn, the CLI gone):
    // the core's list of what still waits decides whether the card stays.
    fail(error);
    void loadChat(id.value, true);
  } finally {
    answering.delete(pending.requestId);
  }
}

/** The session goes on in Claude Code's own terminal (a command only the terminal has). */
async function toTerminal(): Promise<void> {
  confirmTerminal.value = null;
  notice.value = null;
  try {
    const opened = await api.chatToTerminal(id.value);
    if (opened.kind === "tab") revealTab(opened.view.path, opened.view.id);
  } catch (error) {
    fail(error);
  }
}

async function restart(): Promise<void> {
  notice.value = null;
  try {
    await api.chatRestart(id.value);
  } catch (error) {
    fail(error);
  }
}

/* ---------- message actions ---------- */

function copy(text: string): void {
  void api.copyText(text).catch(() => undefined);
}

/** A new chat on this session: `at` the last entry it keeps (undefined: all of it, null: none),
 * with `draft` waiting in its composer or `prompt` sent once it is ready. */
async function fork(at: string | null | undefined, draft: string | null = null, prompt: string | null = null): Promise<void> {
  const from = session.value;
  const outgoing: ChatOutgoing | null = prompt ? { text: prompt, images: [], documents: [], priority: "next", pasted: false } : null;
  const opened = await startChat(path.value, {
    fork: from && at !== null ? (at ? { session: from, at } : { session: from }) : null,
    prompt: outgoing,
  });
  if (opened?.kind !== "tab") {
    if (opened === null) return;
    notice.value = { text: t("chat.notice.failed") };
    return;
  }
  if (draft) draftFor(opened.view.id, draft);
  revealTab(opened.view.path, opened.view.id);
}

/** Edit and resend: a new chat forked just before the message, the new words its prompt. */
function editResend(user: UserEntry, text: string): void {
  if (!user.uuid) return;
  void fork(chainBefore(id.value, user.uuid), null, text);
}

/** Fork from here: a new chat that keeps the conversation through this message's turn. */
function forkAfter(user: UserEntry): void {
  const at = turns.value.findIndex((turn) => turn.user === user);
  const next = turns.value.slice(at + 1).find((turn) => turn.user?.uuid)?.user;
  void fork(next?.uuid ? chainBefore(id.value, next.uuid) : chainNow(id.value));
}

/** The dialogs' fork (rewind's "in a new chat"): before that message, its words in the composer. */
function forkFromDialog(at: string | null): void {
  if (at === null) {
    void fork(undefined);
    return;
  }
  const user = all.value.find((entry): entry is UserEntry => entry.kind === "user" && entry.uuid === at);
  void fork(chainBefore(id.value, at), user?.text ?? null);
}

async function resume(other: string): Promise<void> {
  const opened = await startChat(path.value, { session: other });
  if (opened?.kind === "tab") revealTab(opened.view.path, opened.view.id);
}

async function bookmark(user: UserEntry): Promise<void> {
  const now = session.value;
  if (!now || !user.uuid) return;
  const on = !model.value.bookmarks.has(user.uuid);
  try {
    await api.chatBookmark(now, user.uuid, on);
    if (on) model.value.bookmarks.add(user.uuid);
    else model.value.bookmarks.delete(user.uuid);
  } catch (error) {
    fail(error);
  }
}

function resend(text: string): void {
  void send({ text, images: [], documents: [], priority: "next", pasted: false });
}

/** The user's messages, for the rewind and fork dialogs. */
const messages = computed<ChatMessageRef[]>(() =>
  turns.value
    .filter((turn) => turn.user?.uuid)
    .map((turn) => ({
      uuid: turn.user!.uuid!,
      text: turn.user!.text,
      time: turn.user!.time,
      changed: turn.items.filter((entry) => entry.kind === "tool" && changesByTool.value.has(entry.id)).length,
    })),
);

/* ---------- rows opened ---------- */

function toggle(key: string): void {
  const open = ui.value.expanded;
  if (open.has(key)) open.delete(key);
  else open.add(key);
}

/** A dialog asked for from the composer; the model's or the mode's chip pressed while its menu is
 * up closes it. */
function openDialog(open: ChatDialogOpen): void {
  const menu = (open.kind === "model" || open.kind === "mode") && !open.arg;
  dialog.value = menu && dialog.value?.kind === open.kind ? null : open;
}

/** The empty chat's ready prompts (V16, lib/chat/examples): one picked is sent as it reads, not
 * put in the composer first (the user's word, 14 Nov). One that holds invisible characters or is
 * a command's line goes into the composer instead, which checks (V28) and routes what it sends. */
function useExample(text: string): void {
  if (exited.value || editing.value) return;
  if (scrub(text).removed || parseCommand(text)) {
    composer.value?.setText(text);
    return;
  }
  void send(outgoing(text, [], "next"));
  composer.value?.focus();
}

/** A ready prompt written in place: its place in the list (past its end, a new one) and its text so
 * far. A right-click adds one, or edits or removes the one under it; the button under them adds
 * one too (the user's word, 14 Nov). */
const editing = ref<{ at: number; text: string } | null>(null);
let editBox: HTMLInputElement | null = null;
function holdEditBox(el: unknown): void {
  editBox = el instanceof HTMLInputElement ? el : null;
}
const examplesMenu = useNativeMenu();
const roomForMore = computed(() => chatExamples.value.length < EXAMPLES_MOST);

/** The prompts as shown: a new one being written at the end too. */
const exampleRows = computed(() => {
  const rows = [...chatExamples.value];
  if (editing.value && editing.value.at >= rows.length) rows.push("");
  return rows;
});

function editExample(at: number): void {
  editing.value = { at, text: chatExamples.value[at] ?? "" };
  void nextTick(() => {
    editBox?.focus();
    editBox?.select();
  });
}

/** What was written goes in its place, or at the end; written empty, an edited one stays as it was
 * and a new one goes. */
function saveExample(): void {
  const open = editing.value;
  if (!open) return;
  editing.value = null;
  const text = open.text.trim();
  const list = [...chatExamples.value];
  if (!text || list[open.at] === text) return;
  if (open.at < list.length) list[open.at] = text;
  else list.push(text);
  setExamples(list);
}

function onExampleKey(event: KeyboardEvent): void {
  if (event.isComposing) return;
  if (event.key === "Enter") saveExample();
  else if (event.key === "Escape") editing.value = null;
  else return;
  event.preventDefault();
  event.stopPropagation();
}

/** Right-click on the empty chat: a new ready prompt; on one of them, editing or removing it too. */
function examplesMenuAt(event: MouseEvent, at: number | null): void {
  event.preventDefault();
  event.stopPropagation();
  if (starting.value || exited.value) return;
  saveExample();
  const add: MenuEntry = { text: t("chat.empty.add"), enabled: roomForMore.value, action: () => editExample(chatExamples.value.length) };
  const entries: MenuEntry[] =
    at === null
      ? [add]
      : [
          { text: t("common.edit"), action: () => editExample(at) },
          { text: t("common.delete"), action: () => setExamples(chatExamples.value.filter((_, i) => i !== at)) },
          "separator",
          add,
        ];
  void examplesMenu.popup(event, entries);
}

// The first chat that shows them keeps them in the language they were shown in.
watch(
  () => empty.value && !exited.value && !starting.value,
  (shown) => shown && keepFirstExamples(),
  { immediate: true },
);

/* ---------- the panel's hold on it: the keyboard, ⌘F ---------- */

const finder = new ChatFind(() => thread.value, FIND_LIMIT);

onMounted(() => {
  hostChat(id.value, {
    focus: () => composer.value?.focus(),
    selection: () => finder.selection(),
    search: (query, options, step) => {
      const found = finder.search(query, options, step);
      // Older turns of the page still to be drawn: all of them now, and the search once more
      // where it stands as they get their words, so it counts what it always counted.
      if (query && !whole.value) {
        clearTimeout(lookAgain);
        lookAgain = setTimeout(() => (lookAgain = undefined), 500);
        void drawAll().then(refind);
      }
      return found;
    },
    clear: () => finder.clear(),
  });
  registerComposer(id.value, composer.value);
});
onBeforeUnmount(() => {
  finder.clear();
  hostChat(id.value, null);
  registerComposer(id.value, null);
});

/** Set for a moment after a search made the whole page be drawn: the search is made again as the
 * conversation's height changes (the new turns' Markdown coming in). */
let lookAgain: ReturnType<typeof setTimeout> | undefined;
onBeforeUnmount(() => clearTimeout(lookAgain));

/** The open search, found again where it stands. */
function refind(): void {
  const find = finds.get(id.value);
  if (!find?.query || gone) return;
  const found = finder.search(find.query, findOptions, null);
  find.index = found.index;
  find.count = found.count;
}

// New words while the search is open: found again where it stands.
watch(
  () => all.value.length,
  async () => {
    if (!finds.get(id.value)?.query) return;
    await nextTick();
    refind();
  },
);

/** ⌃O: every thinking block open, or all folded again (V113). */
function onKey(event: KeyboardEvent): void {
  if (event.ctrlKey && !event.metaKey && !event.altKey && event.code === "KeyO") {
    event.preventDefault();
    const open = !ui.value.thinkingOpen;
    ui.value.thinkingOpen = open;
    for (const entry of all.value) {
      if (entry.kind !== "thinking") continue;
      if (open) ui.value.expanded.add(entry.key);
      else ui.value.expanded.delete(entry.key);
    }
  }
}

/* ---------- words for the states ---------- */

const exitText = computed(() => {
  switch (model.value.exit?.reason) {
    case "error":
      return t("chat.exit.error");
    case "notFound":
      return t("chat.exit.notFound");
    case "startFailed":
      return t("chat.exit.startFailed");
    case "elsewhere":
      return t("chat.exit.elsewhere");
    default:
      return t("chat.exit.closed");
  }
});

function clock(at: number): string {
  return new Intl.DateTimeFormat(language.value, { hour: "2-digit", minute: "2-digit" }).format(at);
}

const rateText = computed(() => {
  const limit = model.value.rateLimit;
  if (!limit || limitClosed.value === limitKey(limit)) return null;
  const reset = limit.resetsAt ? clock(limit.resetsAt * (limit.resetsAt < 1e12 ? 1000 : 1)) : null;
  if (limit.status === "rejected") return reset ? t("chat.limit.reachedAt", { time: reset }) : t("chat.limit.reached");
  const share = limit.utilization !== null ? Math.round(limit.utilization * (limit.utilization <= 1 ? 100 : 1)) : null;
  return share !== null ? t("chat.limit.near", { share }) : t("chat.limit.nearSome");
});

function closeLimit() {
  if (model.value.rateLimit) limitClosed.value = limitKey(model.value.rateLimit);
}

const retryText = computed(() => {
  const retry = model.value.retry;
  if (!retry) return null;
  return retry.attempt && retry.max ? t("chat.retry.count", { attempt: retry.attempt, max: retry.max }) : t("chat.retry.plain");
});

</script>

<template>
  <div ref="root" class="chat-view" @keydown="onKey">
    <div ref="mainBox" class="main">
      <TerminalFind :id="id" class="find" />
      <!-- A click on the conversation keeps the keyboard in the view (⌘F, the panes' keys, the arrows). -->
      <div
        ref="scroller"
        class="scroll"
        data-scroll
        tabindex="-1"
        @scroll.passive="onScroll"
        @wheel.passive="userScrolls"
        @touchmove.passive="userScrolls"
        @keydown="userScrolls"
        @pointerdown="pressScroller"
      >
        <div ref="thread" :class="['thread', { blank: empty && !exited }]">
          <!-- Nothing yet: what a chat is for, and a few ways to begin (V16). -->
          <!-- Only once the buffer was read: a chat opened with its first message never flashes it. -->
          <div v-if="empty && !exited && model.loaded" class="empty" @contextmenu="examplesMenuAt($event, null)">
            <AgentMark agent="claude" :size="28" />
            <h2>{{ t("chat.empty.title") }}</h2>
            <p>{{ starting ? t("chat.empty.starting") : t("chat.empty.text") }}</p>
            <div v-if="!starting" class="examples">
              <template v-for="(text, i) in exampleRows" :key="i">
                <!-- Written in place: the return key's mark where the arrow was, green. -->
                <div v-if="editing && editing.at === i" class="example editing">
                  <input
                    :ref="holdEditBox"
                    v-model="editing.text"
                    class="example-input"
                    type="text"
                    maxlength="1000"
                    :placeholder="t('chat.empty.addPlaceholder')"
                    :aria-label="t('chat.empty.addPlaceholder')"
                    @keydown="onExampleKey"
                    @blur="saveExample"
                  />
                  <button type="button" class="example-save" :title="t('common.save')" :aria-label="t('common.save')" @mousedown.prevent @click="saveExample">
                    <Icon name="enter" :size="13" />
                  </button>
                </div>
                <button v-else type="button" class="example" @click="useExample(text)" @contextmenu="examplesMenuAt($event, i)">
                  <span class="example-text">{{ text }}</span>
                  <Icon name="arrow-right" :size="13" class="example-go" />
                </button>
              </template>
              <div v-if="!editing && roomForMore" class="example-tools">
                <button type="button" class="example-tool" @click="editExample(chatExamples.length)"><Icon name="plus" :size="10" />{{ t("chat.empty.add") }}</button>
              </div>
            </div>
            <Spinner v-else :size="14" />
          </div>
          <button v-if="hiddenTurns && whole" type="button" class="earlier" @click="showEarlier">{{ t("chat.earlier", { count: hiddenTurns }) }}</button>
          <ChatTurn
            v-for="turn in shownTurns"
            :key="turn.key"
            :data-turn="turn.key"
            :turn="turn"
            :project="path"
            :commands="commandsByTool"
            :changes="changesByTool"
            :tasks="tasksByTool"
            :runs="runs"
            :waiting="waitingTools"
            :skills="skills"
            :expanded="ui.expanded"
            :bookmarked="!!turn.user?.uuid && model.bookmarks.has(turn.user.uuid)"
            :focus="settings.focusView"
            :timestamps="settings.timestamps"
            :running="turn.last && busy"
            :can-fork="!!session"
            @toggle="toggle"
            @long="(title, text) => (long = { title, text })"
            @copy="copy"
            @edit="editResend"
            @rewind="(user) => (dialog = { kind: 'rewind', arg: user.uuid ?? undefined })"
            @fork="forkAfter"
            @bookmark="bookmark"
            @resend="resend"
            @picture="showPicture"
          />
          <!-- The process ended: why, and the two ways on. -->
          <div v-if="exited" class="state-card" role="alert">
            <div class="state-head">
              <AgentMark agent="claude" :size="14" />
              <span>{{ exitText }}</span>
            </div>
            <pre v-if="model.exit?.stderr" class="stderr selectable">{{ model.exit.stderr }}</pre>
            <div class="state-actions">
              <button v-if="model.exit?.reason !== 'notFound'" type="button" class="control primary" @click="restart"><Icon name="restart" :size="12" />{{ t("chat.exit.restart") }}</button>
              <button v-if="session" type="button" class="control" @click="toTerminal"><Icon name="terminal" :size="12" />{{ t("chat.exit.terminal") }}</button>
            </div>
          </div>
        </div>
      </div>

      <!-- While the card shows, the whole view dims lightly under it (the user's word, 16 Nov), the
           line's words and buttons over the dim; a press on the dim shuts the card. -->
      <Transition name="scrim">
        <div v-if="ui.card && lineShown" class="scrim" aria-hidden="true" @pointerdown="ui.card = null"></div>
      </Transition>

      <!-- Over the composer: the status line, then notices, the to-do list, the cards waiting on the user. -->
      <div ref="footBox" class="foot" :style="{ '--bar': `${bar}px`, '--fade': under / FADE }">
        <button v-if="!ui.stuck && !empty" type="button" class="to-end" :title="t('chat.toEnd')" :aria-label="t('chat.toEnd')" @click="toEnd(true)">
          <Icon name="chevron-down" :size="14" />
        </button>
        <!-- Always the foot's top row, over the notices, the to-do list and the cards (the user's
             word, 16 Nov). -->
        <div v-if="lineShown" ref="lineBox" class="status-line">
          <!-- What Claude is at while the turn runs: the breathing ring a working tab's name has
               before it, and a light sweeping over the words; nothing turning (the user's word,
               15 Nov: a spinner that never seemed to turn on its centre; 16 Nov: the tab's ring). -->
          <div v-if="phase" :class="['phase', { stuck: phase.stuck }]" role="status">
            <Icon v-if="phase.stuck" name="info" :size="12" />
            <ClaudeDot v-else :live="WORKING" />
            <span :class="['words', { shimmer: !phase.stuck }]"
              >{{ phase.text
              }}<span v-if="!phase.stuck" class="band" aria-hidden="true"><span class="lit">{{ phase.text }}</span></span></span
            >
            <span class="faint">{{ elapsed }}</span>
            <button v-if="phase.stuck" type="button" class="control small" @click="interrupt">{{ t("chat.status.stop") }}</button>
          </div>
          <span class="grow"></span>
          <div class="ghosts" role="group" :aria-label="t('chat.side.title')">
            <!-- The side questions, until their panel is closed: it opens again as it was left;
                 lit while it shows, the icon blinking while an answer is still coming. -->
            <button
              v-if="btwLine"
              type="button"
              :class="['ghost', { on: btwLine.open }]"
              :aria-pressed="btwLine.open"
              :aria-label="btwLine.label"
              @click="openBtw"
            >
              <ChatIcon name="question" :size="13" :class="['lead', { working: btwLine.busy }]" />
              <span class="word" aria-hidden="true">{{ btwLine.label }}</span>
              <span v-if="btwLine.count" class="num" aria-hidden="true">{{ btwLine.count }}</span>
            </button>
            <button
              v-for="ghost in ghosts"
              :key="ghost.tab"
              type="button"
              :class="['ghost', { on: ui.card === ghost.tab }]"
              :aria-pressed="ui.card === ghost.tab"
              :aria-label="ghost.state ? `${ghost.label}, ${ghost.state}` : ghost.label"
              :title="ghost.state ?? undefined"
              @click="toggleCard(ghost.tab)"
            >
              <!-- Something of it at work: its own icon blinks, no ring (the user's word, 16 Nov);
                   orange while a command waits for permission. -->
              <ChatIcon :name="ghost.icon" :size="13" :class="['lead', ghost.live?.phase]" />
              <!-- In a narrow line, the number alone. -->
              <span class="word" aria-hidden="true">{{ ghost.label }}</span>
              <span class="num" aria-hidden="true">{{ ghost.count }}</span>
            </button>
          </div>
          <!-- The card stands on the line, as wide as the composer (CK3, the user's pick of 16 Nov). -->
          <Transition name="card">
            <DetailCard
              v-if="ui.card"
              :id="id"
              class="card"
              :path="path"
              :model="model"
              :now="tick"
              :pending-by-tool="pendingByTool"
              :tab="ui.card"
              :most="cardMost"
              @tab="(tab) => (ui.card = tab)"
              @close="ui.card = null"
              @long="(title, text) => (long = { title, text })"
              @answer="answer"
            />
          </Transition>
        </div>
        <div v-if="rateText" :class="['banner', { bad: model.rateLimit?.status === 'rejected' }]">
          <Icon name="fuel" :size="12" /><span class="grow">{{ rateText }}</span>
          <button type="button" class="x" :aria-label="t('common.close')" @click="closeLimit">×</button>
        </div>
        <div v-if="retryText" class="banner"><Spinner :size="11" /><span>{{ retryText }}</span></div>
        <div v-if="notice" class="banner bad">
          <span class="grow">{{ notice.text }}</span>
          <button v-if="notice.terminal && session" type="button" class="control small" @click="toTerminal">{{ t("chat.exit.terminal") }}</button>
          <button type="button" class="x" :aria-label="t('common.close')" @click="notice = null">×</button>
        </div>
        <div v-if="confirmTerminal !== null" class="banner ask" role="alertdialog">
          <span class="grow">{{ t("chat.terminal.ask", { command: confirmTerminal }) }}</span>
          <button type="button" class="control small" @click="confirmTerminal = null">{{ t("common.cancel") }}</button>
          <button type="button" class="control small primary" @click="toTerminal">{{ t("chat.exit.terminal") }}</button>
        </div>
        <!-- The to-do list and the card scroll inside their own room, so the conversation keeps some. -->
        <div v-if="model.todos?.length || (cards.length && !planCard) || planBarShown" class="pinned">
          <TodoList v-if="model.todos?.length" :todos="model.todos" :folded="ui.todosFolded" @fold="ui.todosFolded = !ui.todosFolded" />
          <ChatCard v-if="cards.length && !planCard" :id="id" :key="cards[0].requestId" :path="path" :pending="cards[0]" :more="cards.length - 1" :compact="false" @answer="(given) => answer(cards[0], given)" @terminal="toTerminal" />
          <!-- Claude's plan folded away: it waits here, opened again or approved as it is. -->
          <div v-if="planBarShown" ref="planBar" class="plan-bar">
            <ChatIcon name="plan" :size="14" class="plan-bar-icon" />
            <div class="plan-bar-q">
              <b>{{ t("chatInput.plan.waiting") }}</b>
              <span>{{ planTitle(planText) || planStartsIn }}</span>
            </div>
            <span v-if="cards.length > 1" class="cc-more">{{ t("chatInput.card.more", { count: cards.length - 1 }) }}</span>
            <div class="plan-bar-acts">
              <button type="button" class="cc-btn" @click="unfoldPlan(planBar)"><Icon name="maximize" :size="12" />{{ t("chatInput.plan.open") }}</button>
              <PlanStart :id="id" :mode="ui.planMode" :sending="planBarSending" @start="startPlan" @pick="(picked) => (ui.planMode = picked)" />
            </div>
          </div>
        </div>
        <Composer
          :id="id"
          ref="composer"
          :path="path"
          :info="model.info"
          :state="model.state"
          :settings="settings"
          :suggestion="settings.suggestions ? model.suggestion : null"
          :queued="model.queued"
          :context="context"
          :terminals="terminals"
          :model="model.model"
          :mode="model.mode"
          :effort="model.effort"
          :context-share="contextShare"
          :disabled="exited"
          @send="send"
          @shell="shell"
          @interrupt="interrupt"
          @dialog="openDialog"
          @control="control"
          @cancel-queued="cancelQueued"
          @terminal="(command) => (confirmTerminal = command)"
          @settings="emit('settings')"
        />
      </div>

      <!-- Claude's plan on its own screen over the chat, the composer too (PL5). -->
      <Transition :css="false" @enter="planEnter" @leave="planLeave">
        <PlanScreen
          v-if="planShown"
          v-show="planVisible"
          :id="id"
          :key="planShown.key"
          :data-tool="planShown.tool ?? undefined"
          :plan="planShown.plan"
          :asking="planShown.asking"
          :more="planShown.asking ? cards.length - 1 : 0"
          :mode="ui.planMode"
          :approved="!planShown.asking && readingApproved"
          :request="planShown.asking ? planShown.key : undefined"
          @answer="(given) => planCard && answer(planCard, given)"
          @fold="foldPlan"
          @close="reading = null"
          @pick="(picked) => (ui.planMode = picked)"
        />
      </Transition>
    </div>

    <ChatDialogHost :id="id" :path="path" :session="session" :info="model.info" :state="model.state" :open="dialog" :messages="messages" @close="dialog = null" @insert="(text) => composer?.insert(text)" @fork="forkFromDialog" @resume="resume" />
    <ChatLongText v-if="long" :title="long.title" :text="long.text" @close="long = null" />
    <ImageLightbox v-if="lightStart !== null" :images="lightShots" :start="lightStart" :thumb="lightThumb" @close="lightStart = null" />
  </div>
</template>

<style scoped>
.chat-view {
  position: relative;
  display: flex;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  background: var(--bg-log);
  color: var(--text);
}

.main {
  position: relative;
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}
.scroll:focus {
  outline: none;
}

.thread {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: 900px;
  margin: 0 auto;
  padding: 16px 20px 12px;
}

/* The search bar over the conversation's top right, as over a terminal's. */
.find {
  z-index: 5;
}

/* Nothing said yet: the thread fills the room, so the welcome stands in its middle (the user's
   word, 14 Nov). */
.thread.blank {
  box-sizing: border-box;
  min-height: 100%;
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  margin: auto;
  padding: 16px 12px;
  text-align: center;
  color: var(--text-soft);
}

.empty h2 {
  margin: 6px 0 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-strong);
}

.empty p {
  max-width: 440px;
  margin: 0;
  font-size: 13px;
  line-height: 1.55;
}

.examples {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  width: min(100%, 420px);
  margin-top: 12px;
}

/* A row is sent at a click: its arrow on the right says so. The add button under the rows stays
   faint, so the welcome reads as ways to begin (the user's word, 14 Nov: "çok göze batacak şekilde
   olmasın"). A right-click adds, edits or removes one. */
.example-tools {
  display: flex;
  justify-content: center;
  gap: 14px;
  margin-top: 2px;
}

.example-tool {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 4px;
  border: 0;
  border-radius: 5px;
  background: none;
  color: var(--text-faint);
  font: inherit;
  font-size: 11.5px;
}

.example-tool:hover {
  color: var(--text-muted);
}

.example {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px 8px 12px;
  border: 1px solid var(--line-inset);
  border-radius: 8px;
  background: var(--bg-panel);
  color: var(--text-soft);
  font: inherit;
  font-size: 12.5px;
  text-align: left;
}

.example:hover {
  background: var(--bg-row-hover);
  color: var(--text);
}

.example-text {
  flex: 1;
  min-width: 0;
}

/* The arrow only brightens under the pointer; it doesn't move (the user's word, 14 Nov). */
.example-go {
  flex-shrink: 0;
  color: var(--text);
  opacity: 0.35;
  transition: opacity 0.15s ease;
}

.example:hover .example-go {
  opacity: 1;
}

.example.editing {
  padding: 4px 4px 4px 12px;
  border-color: var(--focus-ring);
  background: var(--bg-panel);
}

.example-input {
  flex: 1;
  min-width: 0;
  height: 26px;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text-strong);
  font: inherit;
  font-size: 12.5px;
}

.example-input::placeholder {
  color: var(--text-faint);
}

/* Where the arrow was: the return key, green, which keeps it. */
.example-save {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--run);
}

.example-save:hover {
  background: var(--run-bg);
}

@media (prefers-reduced-motion: reduce) {
  .example-go {
    transition: none;
  }
}

.earlier {
  align-self: center;
  padding: 4px 10px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text-link);
  font: inherit;
  font-size: 12px;
}

.earlier:hover {
  background: var(--bg-row-hover);
}

.phase {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 22px;
  font-size: 12.5px;
  color: var(--claude-text);
}

/* The words at work: dimmed, a band of their full colour sweeping over them from left to right,
   in either theme. Plain under reduced motion.
   The band is the same words in their full colour over the dimmed ones, seen through a window
   that fades at both ends: the window slides across while the words in it slide back as far, so
   they stand still. Only transforms move, which the compositor runs by itself: nothing is drawn
   again while it sweeps. (A gradient clipped to the text and moved by `background-position` drew
   the words anew at every frame: 15 points of a core for one chat at work, measured 17 Nov.)
   The numbers keep that gradient's look: a band 0.9 of the words wide, from wholly before them
   to wholly after them in 2 s. */
.shimmer {
  position: relative;
  color: color-mix(in srgb, var(--claude-text) 50%, transparent);
}

.band {
  position: absolute;
  inset: 0;
  pointer-events: none;
  -webkit-mask: linear-gradient(90deg, transparent, #000 50%, transparent) 0 0 / 90% 100% no-repeat;
  mask: linear-gradient(90deg, transparent, #000 50%, transparent) 0 0 / 90% 100% no-repeat;
  animation: shimmer-band 2s linear infinite;
}

/* The same box as the band's, so the two slides are the same length to the last part of a pixel
   and the words in the band never creep over those under them. Cut where those are. */
.lit {
  position: absolute;
  inset: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--claude-text);
  animation: shimmer-lit 2s linear infinite;
}

/* Added to the dimmed words instead of drawn over them, the band's words need only the other
   half of the colour: a letter's soft edge then comes out as the one gradient drew it, where
   words drawn twice would thicken it. */
@supports (mix-blend-mode: plus-lighter) {
  .shimmer {
    isolation: isolate;
  }

  .band {
    mix-blend-mode: plus-lighter;
  }

  .lit {
    color: color-mix(in srgb, var(--claude-text) 50%, transparent);
  }
}

@keyframes shimmer-band {
  from {
    transform: translateX(-95%);
  }
  to {
    transform: translateX(105%);
  }
}

@keyframes shimmer-lit {
  from {
    transform: translateX(95%);
  }
  to {
    transform: translateX(-105%);
  }
}

@media (prefers-reduced-motion: reduce) {
  .shimmer {
    color: inherit;
  }

  .band {
    display: none;
  }
}

.phase.stuck {
  color: var(--crash-text);
}

.faint {
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}

.state-card {
  padding: 12px 14px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: var(--bg-panel);
}

.state-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  font-size: 13px;
  color: var(--text-strong);
}

.stderr {
  max-height: 160px;
  margin: 8px 0 0;
  overflow: auto;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 11.5px;
  white-space: pre-wrap;
  color: var(--crash-text);
}

.state-actions {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}

/* Centred over the composer: clear of the rows' metas and chevrons along the right. */
.to-end {
  position: absolute;
  top: -40px;
  left: calc(50% - 14px);
  z-index: 4;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 1px solid var(--line-dialog);
  border-radius: 50%;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  color: var(--text-muted);
}

.to-end:hover {
  color: var(--text);
}

/* As wide as the thread and over it: centred in the room the scroll bar leaves (`--bar`). */
.foot {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: none;
  width: calc(100% - var(--bar, 0px));
  max-width: 900px;
  box-sizing: border-box;
  margin: 0 auto 0 max(0px, calc((100% - var(--bar, 0px) - 900px) / 2));
  padding: 0 20px 14px;
}

/* The card and the to-do list never take more than about two thirds of the conversation's height;
   what doesn't fit scrolls inside them. */
.foot {
  max-height: 72%;
}

/* The conversation fades into the foot as it goes under it, dark to clear, never cut at a hard edge;
   as strong as there is conversation under the edge (--fade). Over the user's message, which
   stands sticky (2) and would otherwise cover it (the user's word, 16 Nov: "geçiş sıkıntısı"). */
.foot::before {
  content: "";
  position: absolute;
  z-index: 3;
  right: 0;
  bottom: 100%;
  left: 0;
  height: 28px;
  background: linear-gradient(to top, var(--bg-log), transparent);
  opacity: var(--fade, 0);
  pointer-events: none;
}

.foot > :not(.pinned) {
  flex-shrink: 0;
}

.pinned {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 0 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* Short of room, the to-do list gives its up before a question's card does, down to its least
   (TodoList: its head, and one item while open). */
.pinned > .todo-list {
  flex-shrink: 10000;
}

/* Claude's plan folded onto the composer: one row, Claude's colour in its edge and head as a card's. */
.plan-bar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  box-sizing: border-box;
  padding: 7px 8px 7px 12px;
  border: 1px solid var(--claude-line);
  border-radius: 10px;
  background: var(--bg-panel);
}

.plan-bar-icon {
  flex-shrink: 0;
  color: var(--claude-text);
}

.plan-bar-q {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.plan-bar-q b {
  font-size: 13px;
  font-weight: 600;
  color: var(--claude-text);
}

.plan-bar-q span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-subtle);
}

.plan-bar-acts {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
}

.banner {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border: 1px solid var(--warn-line);
  border-radius: 8px;
  background: var(--warn-bg);
  font-size: 12.5px;
  color: var(--warn-text);
}

.banner.bad {
  border-color: var(--crash-line);
  background: var(--crash-bg);
  color: var(--crash-text);
}

.banner.ask {
  border-color: var(--line-strong);
  background: var(--bg-panel);
  color: var(--text);
}

.grow {
  flex: 1;
  min-width: 0;
}

.x {
  padding: 0 4px;
  border: 0;
  background: none;
  color: inherit;
  font-size: 15px;
  line-height: 1;
}

.control {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 11px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-control);
  color: var(--text-soft);
  font: inherit;
  font-size: 12.5px;
  white-space: nowrap;
}

.control.small {
  height: 24px;
  padding: 0 9px;
  font-size: 12px;
}

.control:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.control.primary {
  border-color: var(--btn-primary-line);
  background: var(--btn-primary-bg);
  color: var(--btn-primary-text);
}

.control.primary:hover {
  background: var(--btn-primary-hover);
}

/* ---- the status line and its card ---- */

/* The foot's top row, close over what comes under it; the card stands on it, over the dim (19). */
.status-line {
  position: relative;
  z-index: 20;
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 26px;
  margin: -2px 0 -4px;
}

/* Its words go before the buttons do: cut at the end. */
.status-line .phase {
  min-width: 0;
  padding-left: 2px;
  overflow: hidden;
  white-space: nowrap;
}

.status-line .phase .words {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.status-line {
  container-type: inline-size;
}

.ghost .num {
  display: none;
}

@container (max-width: 380px) {
  .ghost .word {
    display: none;
  }

  .ghost .num {
    display: inline;
  }
}

/* Always at the right end; the last one's words end where the composer does. */
.ghosts {
  display: flex;
  align-items: center;
  flex: none;
  margin-right: -7px;
}

/* No ground, hovered or pressed (the user's word, 16 Nov): only the words brighten. */
.ghost {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 7px;
  border: 0;
  border-radius: var(--radius-control);
  background: none;
  color: var(--text-muted);
  font: inherit;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  /* Lit and dimmed again gently, never at once (the user's word, 16 Nov: "pat diye renk değişiyor"). */
  transition: color 0.18s ease;
}

.ghost .lead {
  color: var(--text-faint);
  transition: color 0.18s ease;
}

.ghost:hover,
.ghost.on {
  color: var(--text-strong);
}

.ghost:hover .lead,
.ghost.on .lead {
  color: var(--text-soft);
}

/* At work, its icon blinks as a tool row's does; waiting on the user, in Claude's colour too. */
.ghost .lead.working,
.ghost .lead.waiting {
  animation: ghost-blink 1.6s ease-in-out infinite;
}

.ghost .lead.working {
  color: var(--text-soft);
}

.ghost .lead.waiting {
  color: var(--claude-text);
}

@keyframes ghost-blink {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.25;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ghost .lead.working,
  .ghost .lead.waiting {
    animation: none;
  }
}

.ghost:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.card {
  position: absolute;
  left: 0;
  right: 0;
  bottom: calc(100% + 6px);
}

.card-enter-active,
.card-leave-active {
  transition:
    opacity 0.16s ease,
    transform 0.16s ease;
}

.card-enter-from,
.card-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

@media (prefers-reduced-motion: reduce) {
  .card-enter-active,
  .card-leave-active,
  .scrim-enter-active,
  .scrim-leave-active {
    transition: none;
  }
}

/* Over the whole view, the composer too; under the line (20) and its card. */
.scrim {
  position: absolute;
  inset: 0;
  z-index: 19;
  background: var(--scrim-soft);
}

.scrim-enter-active,
.scrim-leave-active {
  transition: opacity 0.22s ease;
}

.scrim-enter-from,
.scrim-leave-to {
  opacity: 0;
}
</style>

<!-- The search's marks (lib/chat/find: the CSS Custom Highlight API), in the terminal search's
     colours. Not scoped: highlights are named for the whole page. -->
<style>
::highlight(chat-find) {
  background-color: var(--find-bg);
}

::highlight(chat-find-now) {
  background-color: var(--find-on-bg);
  color: var(--find-on-text);
}
</style>
