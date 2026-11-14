<script setup lang="ts">
import { computed, markRaw, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import Collapse from "../../components/Collapse.vue";
import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { planModeOf, planTitle, STARTS_IN } from "../../lib/chat/plan";
import { peekChat } from "../../lib/chat/store";
import type { PlanMode } from "../../lib/chat/ui";
import type { ChatAnswer } from "../../lib/chat/wire";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";
import ChatIcon from "./ChatIcon.vue";
import PlanStart from "./PlanStart.vue";

/**
 * Claude's plan on a screen of its own over the chat (PL5, the user's pick of 18 Nov): it grows
 * out of the plan's row (ChatView animates it), the plan's headings stand on its left and take the
 * plan to them, the plan itself is the one thing that scrolls, and the decision stays at its foot:
 * "Onayla ve başla" in the mode picked by its arrow, or "Plana devam et", which turns the foot into
 * the words that go back to Claude with the comments left on the plan's lines. Clicks alone
 * answer: no key picks a mode or approves (the user's word, 18 Nov); Esc folds the screen, or
 * closes the words being written. Without a request it shows an answered plan, to read.
 */
const props = defineProps<{
  id: number;
  plan: string;
  /** Asking: the request is waiting on the user. Reading: an answered plan, nothing to answer. */
  asking: boolean;
  /** The cards waiting after it. */
  more: number;
  mode: PlanMode;
  /** An answered plan was approved (its row's "Onaylandı"). */
  approved?: boolean;
  /** The request asking (its id), whose notes are kept while the screen isn't shown. */
  request?: string;
}>();
const emit = defineEmits<{ answer: [answer: ChatAnswer]; fold: []; close: []; pick: [mode: PlanMode] }>();

const root = ref<HTMLElement | null>(null);
const scroller = ref<HTMLElement | null>(null);
const doc = ref<HTMLElement | null>(null);
const html = shallowRef("");

const title = computed(() => planTitle(props.plan));

onMounted(async () => {
  const lib = await import("../../lib/markdownPreview");
  html.value = lib.renderMarkdown(props.plan, { note: t("files.md.note"), tip: t("files.md.tip"), important: t("files.md.important"), warning: t("files.md.warning"), caution: t("files.md.caution") });
});

/* ---------- the headings on the left ---------- */

type Heading = { el: HTMLElement; text: string; depth: number; line: number };
const headings = shallowRef<Heading[]>([]);
/** The heading the plan is read under (its line). */
const current = ref<number | null>(null);

/** The plan's h1–h3, but for its title (a first `# …`, which the head shows), each a step in from
 * the highest of them. Fewer than two give no column: the plan has the width. */
function readHeadings(): void {
  const md = doc.value?.querySelector(".cc-md");
  if (!md) return void (headings.value = []);
  const found = [...md.querySelectorAll<HTMLElement>("h1, h2, h3")].filter((el, at) => !(at === 0 && el === md.firstElementChild && el.tagName === "H1"));
  const top = Math.min(...found.map((el) => Number(el.tagName[1])));
  headings.value = found.map((el) => markRaw({ el, text: (el.textContent ?? "").trim(), depth: Number(el.tagName[1]) - top, line: Number(el.dataset.line) || 0 }));
  spy();
}

const outline = computed(() => (headings.value.length >= 2 ? headings.value : []));

/** Where an element stands in the plan's scroll. */
function offsetOf(el: HTMLElement): number {
  const box = scroller.value!.getBoundingClientRect();
  return el.getBoundingClientRect().top - box.top + scroller.value!.scrollTop;
}

let spyFrame = 0;
function spy(): void {
  if (spyFrame) return;
  spyFrame = requestAnimationFrame(() => {
    spyFrame = 0;
    const box = scroller.value;
    if (!box || !outline.value.length) return void (current.value = null);
    const end = box.scrollTop + box.clientHeight >= box.scrollHeight - 2;
    let at: Heading | null = null;
    for (const each of outline.value) if (offsetOf(each.el) <= box.scrollTop + 40) at = each;
    current.value = end ? outline.value[outline.value.length - 1].line : (at ?? outline.value[0]).line;
  });
}
onBeforeUnmount(() => cancelAnimationFrame(spyFrame));

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function go(heading: Heading): void {
  tocOpen.value = false;
  scroller.value?.scrollTo({ top: Math.max(0, offsetOf(heading.el) - 12), behavior: reduced() ? "auto" : "smooth" });
}

/** A narrow screen has no room for the column: its headings come from a button in the head. */
const tocOpen = ref(false);
const tocButton = ref<HTMLElement | null>(null);
const tocPop = ref<HTMLElement | null>(null);
function onOutside(event: PointerEvent): void {
  const target = event.target;
  if (target instanceof Node && (tocPop.value?.contains(target) || tocButton.value?.contains(target))) return;
  tocOpen.value = false;
}
watch(tocOpen, (open) => (open ? window.addEventListener("pointerdown", onOutside, true) : window.removeEventListener("pointerdown", onOutside, true)));
onBeforeUnmount(() => window.removeEventListener("pointerdown", onOutside, true));

/* ---------- comments on the plan's lines ---------- */

/** A comment's field stands under the block it is on, in a place made for it in the drawn plan. It
 * eases open, and a click on its block again eases it shut (the user's word, 18 Nov): what was
 * written stays with the block and opens again with it; one shut empty goes. */
type Comment = { line: number; excerpt: string; text: string; block: HTMLElement; host: HTMLElement; open: boolean };
const comments = ref<Comment[]>([]);
const fields = new Map<number, HTMLTextAreaElement>();

function fieldRef(line: number, el: unknown): void {
  if (el instanceof HTMLTextAreaElement) fields.set(line, el);
  else fields.delete(line);
}

/** Where a block's comment goes: in a list item right under its own words (before a list inside
 * it), after anything else, after a table whole. */
function hostFor(block: HTMLElement): HTMLElement {
  const host = document.createElement("div");
  host.className = "ps-cmt-host";
  // A place of its own (a block formatting context), so that the field easing in it never lets the
  // margins around it collapse one moment and not the next: the plan under it would leap.
  host.style.display = "flow-root";
  if (block.tagName === "LI") block.insertBefore(host, nested(block));
  else {
    const after = block.closest("table") ?? block;
    after.after(host);
    // It gives the block's bottom margin back and takes it below, so that shut it leaves the
    // spacing as it was: it opens right under the block's words and goes without a jump.
    const gap = getComputedStyle(after).marginBottom;
    host.style.marginTop = `calc(-1 * ${gap})`;
    host.style.marginBottom = gap;
  }
  sized.observe(host);
  return markRaw(host);
}

/** What holds lines rather than being one: the pointer in its gaps is over no line. */
const HOLDERS = new Set(["UL", "OL", "BLOCKQUOTE", "TABLE", "THEAD", "TBODY", "TFOOT", "HR"]);

/** A list item's first block of its own (a list inside it, a paragraph, a comment's place). */
function nested(li: HTMLElement): HTMLElement | null {
  for (const child of li.children) if (child instanceof HTMLElement && (child.dataset.line || child.classList.contains("ps-cmt-host") || HOLDERS.has(child.tagName))) return child;
  return null;
}

/** A list item's own words, before what is nested in it. */
function ownText(block: HTMLElement): string {
  if (block.tagName !== "LI") return block.textContent ?? "";
  const stop = nested(block);
  let text = "";
  for (const node of block.childNodes) {
    if (node === stop) break;
    text += node.textContent ?? "";
  }
  return text;
}

/** The line of the drawn plan a comment can be left on, under the pointer or clicked: a paragraph,
 * a heading, a list item's own words, a code block, a table's row. A gap between them, or in a list
 * or a quote, is no line (the user's word, 18 Nov: "sadece satıra yorum yazılabilsin"). */
function blockAt(target: Element | null): HTMLElement | null {
  if (!target || !props.asking || sent.value || target.closest(".ps-cmt-host")) return null;
  const block = target.closest<HTMLElement>(".cc-md [data-line]");
  if (!block || !doc.value?.contains(block) || !Number.isFinite(Number(block.dataset.line)) || HOLDERS.has(block.tagName)) return null;
  // A loose list item's words are its paragraphs; the item itself is only their holder.
  return block.tagName === "LI" && !ownText(block).trim() ? null : block;
}

function onDocClick(event: MouseEvent): void {
  const target = event.target instanceof Element ? event.target : null;
  if (!target || !doc.value) return;
  // The mark in the margin: its block's comment, opened or shut.
  const mark = target.closest<HTMLElement>("[data-mark]");
  if (mark) {
    const line = Number(mark.dataset.mark);
    const block = comments.value.find((each) => each.line === line)?.block ?? (hover.value?.line === line ? hoverBlock : null);
    if (block && props.asking && !sent.value) toggle(block);
    return;
  }
  const link = target.closest<HTMLElement>("[data-href]");
  if (link) {
    const href = link.dataset.href ?? "";
    if (/^https?:\/\//i.test(href)) void api.openUrl(href);
    return;
  }
  // A drag that selected text is a copy, not a comment.
  if (!window.getSelection()?.isCollapsed) return;
  const block = blockAt(target);
  if (block) toggle(block);
}

/** A block's comment: made and eased open, or opened again, or shut. */
function toggle(block: HTMLElement): void {
  const line = Number(block.dataset.line);
  const known = comments.value.find((each) => each.line === line);
  if (known) {
    if (known.open) shut(known);
    else openField(known);
    return;
  }
  const excerpt = ownText(block).replace(/\s+/g, " ").trim().slice(0, 120);
  // Drawn shut first, so that it eases open once its place is in the page.
  comments.value.push({ line, excerpt, text: "", block: markRaw(block), host: hostFor(block), open: false });
  comments.value.sort((a, b) => a.line - b.line);
  void nextTick(() => {
    const made = comments.value.find((each) => each.line === line);
    if (made) openField(made);
    place();
  });
}

/* ---------- the margin's marks ---------- */

/* No ground is painted under a block (the user's word, 18 Nov): the plan's left margin carries a
   thin line beside the block under the pointer and a comment glyph at its first line, sliding from
   block to block; a block with a comment keeps its line and glyph in Claude's colour. */
type Place = { top: number; height: number };
const hover = shallowRef<(Place & { line: number }) | null>(null);
let hoverBlock: HTMLElement | null = null;
const places = shallowRef(new Map<number, Place>());
/** The blocks whose mark stays: a field open, or words written. One shut empty fades at once, still
 * following its field as that eases shut, rather than staying until it goes. */
const markedLines = computed(() => new Set(comments.value.filter((each) => each.open || each.text.trim()).map((each) => each.line)));
/** The hovered mark eases only from block to block; a field opening or shutting moves the marks
 * with it, frame by frame, with no ease to lag behind (the user's word, 18 Nov). */
const sliding = ref(false);

/** A block's place in the plan, down to the foot of its comment's field while that shows; a list
 * item's, its own words alone, not the list inside it. */
function placeOf(block: HTMLElement, host?: HTMLElement): Place {
  const box = doc.value!.getBoundingClientRect();
  const r = block.getBoundingClientRect();
  const inner = block.tagName === "LI" ? nested(block) : null;
  const own = inner ? inner.getBoundingClientRect().top : r.bottom;
  const field = host?.getBoundingClientRect();
  const bottom = field && field.height > 1 ? Math.max(own, field.bottom) : own;
  return { top: r.top - box.top, height: Math.max(bottom - r.top, 0) };
}

/** Where each commented block and the hovered one stand now (a field eased open moves those under it). */
function place(): void {
  if (!doc.value) return;
  const map = new Map<number, Place>();
  for (const each of comments.value) if (each.block.isConnected) map.set(each.line, placeOf(each.block, each.host));
  places.value = map;
  if (hover.value && hoverBlock?.isConnected) {
    sliding.value = false;
    hover.value = { line: hover.value.line, ...placeOf(hoverBlock) };
  }
}

let hoverFrame = 0;
let hoverTarget: Element | null = null;
let pointer: { x: number; y: number } | null = null;
function onPointerMove(event: PointerEvent): void {
  pointer = { x: event.clientX, y: event.clientY };
  hoverTarget = event.target instanceof Element ? event.target : null;
  follow();
}

/** The block under the pointer, a frame later (a scroll under a still pointer moves it too). */
function follow(): void {
  if (hoverFrame) return;
  hoverFrame = requestAnimationFrame(() => {
    hoverFrame = 0;
    if (!props.asking || sent.value) {
      hoverBlock = null;
      hover.value = null;
      return;
    }
    // Over the mark itself, the block it marks stays the one.
    if (hoverTarget?.closest("[data-mark]")) return;
    const block = blockAt(hoverTarget);
    // In a gap between lines the mark stays where it was, rather than leaping to what holds them.
    if (!block || block === hoverBlock) return;
    hoverBlock = markRaw(block);
    sliding.value = !!hover.value;
    hover.value = { line: Number(block.dataset.line), ...placeOf(block) };
  });
}

function onScroll(): void {
  spy();
  if (!pointer) return;
  hoverTarget = document.elementFromPoint(pointer.x, pointer.y);
  follow();
}

function onPointerLeave(): void {
  cancelAnimationFrame(hoverFrame);
  hoverFrame = 0;
  pointer = null;
  hoverBlock = null;
  hover.value = null;
}

/* The plan's height changes with the window's width, and each comment's place as its field eases
   open or shut; one field easing open as another eases shut leaves the plan's height as it was, so
   each place is watched too, or the marks under them would wait for the end and leap (18 Nov). */
const sized = new ResizeObserver(() => place());
onBeforeUnmount(() => {
  sized.disconnect();
  cancelAnimationFrame(hoverFrame);
});

/** One field open at a time: opening one eases the others shut (the user's word, 18 Nov). */
function openField(comment: Comment): void {
  for (const other of comments.value) if (other !== comment && other.open) shut(other);
  comment.open = true;
  void nextTick(() => fields.get(comment.line)?.focus({ preventScroll: true }));
}

/** Shut: one with words keeps them, an empty one goes once it has eased shut. */
function shut(comment: Comment): void {
  comment.open = false;
  if (!comment.text.trim()) setTimeout(() => drop(comment.line), FOLD_MS);
}

/** Its field eases shut, then the comment and its place go. */
function removeComment(line: number): void {
  const comment = comments.value.find((each) => each.line === line);
  if (!comment) return;
  if (!comment.open) return drop(line);
  comment.open = false;
  setTimeout(() => drop(line), FOLD_MS);
}

/** Collapse's ease (0.2 s) and a little more. */
const FOLD_MS = 240;

function drop(line: number): void {
  const gone = comments.value.find((each) => each.line === line);
  if (!gone || gone.open) return;
  comments.value = comments.value.filter((each) => each.line !== line);
  place();
  // The field leaves its place first, then the place goes.
  void nextTick(() => {
    sized.unobserve(gone.host);
    gone.host.remove();
  });
}

/* ---------- kept while the screen isn't shown ---------- */

/** What was written goes into the chat's memory as the screen folds or the tab is left, and comes
 * back with the screen; answered, it goes. */
function keepNotes(): void {
  const ui = peekChat(props.id)?.ui;
  if (!ui || !props.asking || !props.request) return;
  const kept = comments.value.filter((each) => each.text.trim()).map(({ line, excerpt, text }) => ({ line, excerpt, text }));
  ui.planNotes = !sent.value && (kept.length || feedback.value.trim()) ? { request: props.request, comments: kept, feedback: feedback.value } : null;
}
onBeforeUnmount(keepNotes);

let restored = false;
function restoreNotes(): void {
  if (restored) return;
  restored = true;
  const ui = peekChat(props.id)?.ui;
  const notes = ui?.planNotes;
  if (!ui || !notes) return;
  ui.planNotes = null;
  if (!props.asking || notes.request !== props.request) return;
  feedback.value = notes.feedback;
  for (const each of notes.comments) {
    const block = doc.value?.querySelector<HTMLElement>(`.cc-md [data-line="${each.line}"]`);
    if (block) comments.value.push({ ...each, block: markRaw(block), host: hostFor(block), open: false });
  }
  comments.value.sort((a, b) => a.line - b.line);
}

watch(html, () =>
  void nextTick(() => {
    restoreNotes();
    readHeadings();
    const md = doc.value?.querySelector(".cc-md");
    if (md) sized.observe(md);
  }),
);

/** The comments under each heading, counted beside it on the left. */
const counts = computed(() => {
  const out = new Map<number, number>();
  const list = outline.value;
  for (const each of comments.value) {
    if (!each.text.trim()) continue;
    let under: Heading | null = null;
    for (const heading of list) if (heading.line <= each.line) under = heading;
    if (under) out.set(under.line, (out.get(under.line) ?? 0) + 1);
  }
  return out;
});

const written = computed(() => comments.value.filter((each) => each.text.trim()));

/* ---------- answering ---------- */

const startsIn = computed(() => t(STARTS_IN[planModeOf(props.id, props.mode)]));
const sent = ref<"start" | "keep" | null>(null);
const keeping = ref(false);
const feedback = ref("");
const feedbackField = ref<HTMLTextAreaElement | null>(null);

/** The words that go with "keep planning": the user's, then each comment under the block it is on. */
const message = computed(() => {
  const parts = [feedback.value.trim()];
  for (const each of comments.value) if (each.text.trim()) parts.push(`> ${each.excerpt}\n\n${each.text.trim()}`);
  return parts.filter(Boolean).join("\n\n");
});

function start(mode: PlanMode): void {
  if (sent.value) return;
  sent.value = "start";
  emit("answer", { kind: "allow", mode });
}

function keep(): void {
  if (sent.value) return;
  keeping.value = true;
  void nextTick(() => feedbackField.value?.focus());
}

function keepPlanning(): void {
  if (sent.value) return;
  sent.value = "keep";
  emit("answer", { kind: "deny", message: message.value, interrupt: false });
}

/* ---------- the keyboard ---------- */

/** The keyboard comes to the screen only from nowhere (the page itself) or from this chat's own
 * view (as a card's does): a key meant for another pane never lands here. */
function takeFocus(): void {
  const active = document.activeElement;
  if (active && active !== document.body && active !== document.documentElement) {
    const view = root.value?.closest(".chat-view");
    if (!view || !view.contains(active) || active.closest(".xterm")) return;
  }
  root.value?.focus({ preventScroll: true });
}
onMounted(takeFocus);

function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape" || event.defaultPrevented) return;
  event.preventDefault();
  event.stopPropagation();
  const target = event.target;
  if (tocOpen.value) {
    tocOpen.value = false;
    return;
  }
  // In a comment's field: the field eases shut, as a click on its block shuts it.
  if (target instanceof HTMLTextAreaElement && target !== feedbackField.value) {
    const comment = comments.value.find((each) => fields.get(each.line) === target);
    if (comment) shut(comment);
    root.value?.focus({ preventScroll: true });
    return;
  }
  if (keeping.value && !sent.value) {
    keeping.value = false;
    root.value?.focus({ preventScroll: true });
    return;
  }
  if (props.asking) emit("fold");
  else emit("close");
}
</script>

<template>
  <section ref="root" :class="['ps', { reading: !asking }]" tabindex="-1" :aria-label="t('chatInput.card.plan')" @keydown="onKey">
    <div class="ps-in">
      <header class="ps-head">
        <ChatIcon name="plan" :size="15" class="ps-icon" />
        <span class="ps-name">{{ t("chatInput.card.plan") }}</span>
        <span v-if="title" class="ps-title">{{ title }}</span>
        <span v-if="asking" class="ps-meta">{{ written.length ? t("chatInput.plan.commentCount", { count: written.length }) : t("chatInput.plan.commentHint") }}</span>
        <span v-else-if="approved" class="ps-stamp"><Icon name="check" :size="11" />{{ t("chat.tool.approved") }}</span>
        <span v-if="more > 0" class="cc-more">{{ t("chatInput.card.more", { count: more }) }}</span>
        <span class="grow"></span>
        <button v-if="outline.length" ref="tocButton" type="button" class="ps-toc-btn" :aria-expanded="tocOpen" @click="tocOpen = !tocOpen">
          <Icon name="list" :size="12" />{{ t("chatInput.plan.contents") }}
        </button>
        <button
          type="button"
          class="ps-icon-btn"
          :title="asking ? t('chatInput.plan.fold') : t('common.close')"
          :aria-label="asking ? t('chatInput.plan.fold') : t('common.close')"
          @click="asking ? emit('fold') : emit('close')"
        >
          <Icon :name="asking ? 'minimize' : 'close'" :size="14" />
        </button>
      </header>

      <div class="ps-body">
        <nav v-if="outline.length" class="ps-outline" :aria-label="t('chatInput.plan.contents')">
          <button v-for="each in outline" :key="each.line" type="button" :class="['ps-o', `d${each.depth}`, { on: each.line === current }]" :title="each.text" @click="go(each)">
            <span class="ps-o-text">{{ each.text }}</span>
            <span v-if="counts.get(each.line)" class="ps-o-n">{{ counts.get(each.line) }}</span>
          </button>
        </nav>
        <div ref="scroller" class="ps-doc" @scroll.passive="onScroll">
          <div ref="doc" :class="['ps-plan', 'selectable', { commenting: asking && !sent }]" @click="onDocClick" @pointermove.passive="onPointerMove" @pointerleave="onPointerLeave">
            <!-- eslint-disable-next-line vue/no-v-html -->
            <div v-if="html" class="cc-md" v-html="html"></div>
            <pre v-else class="cc-code small">{{ plan }}</pre>
            <template v-if="asking">
              <span
                v-for="each in comments"
                v-show="places.get(each.line)"
                :key="each.line"
                :class="['ps-mark', 'on', { lit: each.open || hover?.line === each.line, going: !markedLines.has(each.line) }]"
                :style="{ top: `${places.get(each.line)?.top ?? 0}px`, height: `${places.get(each.line)?.height ?? 0}px` }"
                :data-mark="each.line"
                :title="t('chatInput.plan.commentHint')"
                aria-hidden="true"
                ><span class="ps-glyph"><Icon name="chat" :size="13" /></span
              ></span>
              <span
                v-if="hover && !markedLines.has(hover.line)"
                :class="['ps-mark', { slide: sliding }]"
                :style="{ top: `${hover.top}px`, height: `${hover.height}px` }"
                :data-mark="hover.line"
                aria-hidden="true"
                ><span class="ps-glyph"><Icon name="chat" :size="13" /></span
              ></span>
            </template>
          </div>
        </div>
      </div>

      <footer v-if="asking" class="ps-dock">
        <div v-if="!keeping" class="ps-row">
          <div class="ps-q">
            <b>{{ t("chatInput.plan.question") }}</b>
            <span>{{ startsIn }}</span>
          </div>
          <div class="ps-acts">
            <PlanStart :id="id" :mode="mode" :sending="sent === 'start'" :disabled="!!sent" @start="start" @pick="(picked) => emit('pick', picked)" />
            <button type="button" class="cc-btn" :disabled="!!sent" @click="keep"><Icon name="chat" :size="13" />{{ t("chatInput.plan.keepPlanning") }}</button>
          </div>
        </div>
        <div v-else class="ps-keep">
          <div class="ps-q">
            <b>{{ t("chatInput.plan.keeping") }}</b>
            <span>{{ t("chatInput.plan.keepingText") }}</span>
          </div>
          <div v-if="written.length" class="ps-chips">
            <span v-for="each in written" :key="each.line" class="ps-chip" :title="each.excerpt">
              <Icon name="chat" :size="12" />
              <span class="ps-chip-text">{{ each.text }}</span>
              <button type="button" class="ps-chip-x" :aria-label="t('chatInput.plan.removeComment')" :disabled="!!sent" @click="removeComment(each.line)"><Icon name="close" :size="10" /></button>
            </span>
          </div>
          <textarea ref="feedbackField" v-model="feedback" class="cc-textarea" rows="2" :placeholder="t('chatInput.plan.feedback')" :aria-label="t('chatInput.plan.feedback')" :disabled="!!sent"></textarea>
          <div class="ps-row">
            <span class="ps-faint">{{ t("chatInput.plan.reopens") }}</span>
            <div class="ps-acts">
              <button type="button" class="cc-btn quiet" :disabled="!!sent" @click="keeping = false">{{ t("common.cancel") }}</button>
              <button type="button" class="cc-btn primary" :disabled="!!sent" @click="keepPlanning">
                <Spinner v-if="sent === 'keep'" :size="11" /><Icon v-else name="arrow-up-right" :size="12" />{{ t("chatInput.plan.keepSend") }}
              </button>
            </div>
          </div>
        </div>
      </footer>
    </div>

    <div v-if="tocOpen" ref="tocPop" class="ps-toc-pop" role="menu">
      <button v-for="each in outline" :key="each.line" type="button" role="menuitem" :class="['ps-o', `d${each.depth}`, { on: each.line === current }]" @click="go(each)">
        <span class="ps-o-text">{{ each.text }}</span>
        <span v-if="counts.get(each.line)" class="ps-o-n">{{ counts.get(each.line) }}</span>
      </button>
    </div>

    <!-- Each comment's field, in the place made for it under its block. -->
    <template v-for="(each, at) in comments" :key="each.line">
      <Teleport :to="each.host">
        <Collapse :show="each.open" keep>
          <div class="ps-cmt">
            <textarea
              :ref="(el) => fieldRef(each.line, el)"
              v-model="comments[at].text"
              class="cc-textarea ps-cmt-field"
              rows="2"
              :placeholder="t('chatInput.plan.commentPlaceholder')"
              :aria-label="t('chatInput.plan.commentPlaceholder')"
              :disabled="!!sent"
            ></textarea>
            <button type="button" class="cc-link ps-cmt-x" :title="t('chatInput.plan.removeComment')" :aria-label="t('chatInput.plan.removeComment')" :disabled="!!sent" @click="removeComment(each.line)">
              <Icon name="close" :size="11" />
            </button>
          </div>
        </Collapse>
      </Teleport>
    </template>
  </section>
</template>

<style scoped>
/* Over the whole chat, the composer too (ChatView's `.main`); a narrow one folds its column away. */
.ps {
  position: absolute;
  inset: 0;
  z-index: 30;
  container-type: inline-size;
  display: flex;
  flex-direction: column;
  background: var(--bg-log);
  outline: none;
}

.ps-in {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.ps-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 40px;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid var(--line-inset);
  font-size: 13px;
  white-space: nowrap;
}

.ps-icon {
  flex-shrink: 0;
  color: var(--claude-text);
}

.ps-name {
  flex-shrink: 0;
  font-weight: 600;
  color: var(--claude-text);
}

.ps-title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-strong);
}

.ps-meta {
  min-width: 0;
  flex-shrink: 1000;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
  color: var(--text-subtle);
}

.ps-stamp {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--run-text);
}

.grow {
  flex: 1;
}

.ps-icon-btn,
.ps-toc-btn {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  height: 26px;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.ps-icon-btn {
  width: 26px;
}

/* The narrow screen's headings button has no ground (the user's word, 18 Nov): its words light
   while its menu is open. */
.ps-toc-btn {
  display: none;
  padding: 0 6px;
  transition: color 0.12s;
}

.ps-toc-btn:hover {
  color: var(--text);
}

.ps-toc-btn[aria-expanded="true"] {
  color: var(--text-strong);
}

.ps-icon-btn:hover {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.ps-body {
  flex: 1;
  min-height: 0;
  display: flex;
}

/* The plan's headings, as the Files window's outline lists a file's: the one read lit, its comments
   counted in Claude's colour. Long ones are cut; the whole is the button's title. */
.ps-outline {
  flex-shrink: 0;
  width: 200px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 12px 8px;
  overflow-y: auto;
  border-right: 1px solid var(--line-inset);
}

.ps-o {
  position: relative;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 26px;
  padding: 0 8px;
  border: none;
  border-radius: 6px;
  background: transparent;
  font-size: 12.5px;
  text-align: left;
  color: var(--text-muted);
}

.ps-o.d1 {
  padding-left: 20px;
  font-size: 12px;
}

.ps-o.d2 {
  padding-left: 32px;
  font-size: 12px;
}

.ps-o:hover {
  background: var(--bg-hover);
  color: var(--text-strong);
}

.ps-o.on {
  background: var(--bg-lit);
  color: var(--text-strong);
}

.ps-o.on::before {
  content: "";
  position: absolute;
  top: 5px;
  bottom: 5px;
  left: 0;
  width: 2px;
  border-radius: 2px;
  background: var(--claude-line);
}

.ps-o-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ps-o-n {
  flex-shrink: 0;
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--claude-text);
}

/* The one scroll: the plan's. */
.ps-doc {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* Its left margin holds the marks. */
.ps-plan {
  position: relative;
  max-width: 800px;
  margin: 0 auto;
  padding: 16px 24px 28px 44px;
}

.ps-plan .cc-md {
  font-size: 13.5px;
}

.ps-plan.commenting :deep([data-line]) {
  cursor: pointer;
}

/* The margin's marks: a line beside the block, its glyph at the block's first line. The hovered
   one slides from block to block. */
.ps-mark {
  position: absolute;
  left: 0;
  width: 40px;
  cursor: pointer;
  user-select: none;
  animation: ps-mark-in 0.12s ease-out;
}

.ps-mark.slide {
  transition:
    top 0.12s cubic-bezier(0.2, 0, 0, 1),
    height 0.12s cubic-bezier(0.2, 0, 0, 1);
}

.ps-mark.going {
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.12s ease-out;
}

.ps-mark::before {
  content: "";
  position: absolute;
  top: 1px;
  bottom: 1px;
  right: 10px;
  width: 2px;
  border-radius: 2px;
  background: var(--line-lit);
}

.ps-glyph {
  position: absolute;
  top: 2px;
  left: 6px;
  display: inline-flex;
  color: var(--text-subtle);
  transition: color 0.12s;
}

.ps-mark:hover .ps-glyph {
  color: var(--text-strong);
}

/* A comment's mark takes the hovered one's place as it is, with no fade to flicker through. */
.ps-mark.on {
  animation: none;
}

.ps-mark.on::before {
  background: var(--claude-line);
}

.ps-mark.on .ps-glyph,
.ps-mark.on:hover .ps-glyph {
  color: var(--claude-text);
}

.ps-mark.on.lit::before {
  background: var(--claude-text);
}

@keyframes ps-mark-in {
  from {
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ps-mark,
  .ps-mark.slide,
  .ps-mark.going {
    transition: none;
    animation: none;
  }
}

.ps-cmt {
  position: relative;
  padding: 6px 0 10px;
}

.ps-cmt-field {
  min-height: 52px;
  padding-right: 32px;
  border-color: var(--claude-line);
}

.ps-cmt-x {
  position: absolute;
  top: 11px;
  right: 4px;
}

.ps-dock {
  flex-shrink: 0;
  padding: 10px 14px 12px;
  border-top: 1px solid var(--line-inset);
  background: var(--bg-panel);
}

.ps-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
}

.ps-q {
  flex: 1 1 200px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.ps-q b {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-strong);
}

.ps-q span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--text-subtle);
}

.ps-acts {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
}

.ps-keep {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* In the column, the question takes its own height, not the row's share. */
.ps-keep .ps-q {
  flex: none;
}

.ps-keep .cc-textarea {
  min-height: 56px;
  border-color: var(--claude-line);
}

.ps-faint {
  flex: 1 1 200px;
  min-width: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.ps-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.ps-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  height: 24px;
  box-sizing: border-box;
  padding: 0 4px 0 9px;
  border: 1px solid var(--claude-line);
  border-radius: 12px;
  background: var(--claude-bg-hover);
  font-size: 12px;
  color: var(--claude-text);
}

.ps-chip-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ps-chip-x {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: inherit;
}

.ps-chip-x:hover:not(:disabled) {
  background: var(--bg-icon-hover);
}

/* The narrow screen's headings, under the head's button. */
.ps-toc-pop {
  position: absolute;
  z-index: 2;
  top: 38px;
  right: 40px;
  width: min(260px, calc(100% - 56px));
  max-height: 60%;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 6px;
  overflow-y: auto;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
}

@container (max-width: 560px) {
  .ps-outline {
    display: none;
  }

  .ps-toc-btn {
    display: inline-flex;
  }

  .ps-meta {
    display: none;
  }

  .ps-plan {
    padding: 14px 16px 24px 40px;
  }
}
</style>
