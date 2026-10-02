<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import ClaudeDot from "../components/ClaudeDot.vue";
import ClaudeMark from "../components/ClaudeMark.vue";
import Icon from "../components/Icon.vue";
import { claudeLine, turnLine } from "../lib/format";
import { language, t } from "../lib/i18n";
import { api, now, snapshot } from "../lib/store";
import type { DayProject, DaySession, DaySpan, DaySummary } from "../lib/types";

const HOUR = 3_600_000;
/** Today is fetched again this often while it is shown. */
const REFRESH_MS = 30_000;
/** The timeline shows at least this many hours. */
const MIN_HOURS = 8;

type Tip = { title: string; detail: string; tone: "claude" | "server" | "crash" | "plain" };
type Block = { key: string; kind: "server" | "work" | "wait" | "commit" | "crash"; style: Record<string, string>; text: string; tip: Tip };

const day = ref<DaySummary | null>(null);
/** The day shown, `YYYY-MM-DD`; null is today. */
const date = ref<string | null>(null);
const selectedPath = ref<string | null>(null);
const tip = ref<(Tip & { x: number; y: number }) | null>(null);
const tipEl = ref<HTMLElement | null>(null);

let asked = 0;

async function load(): Promise<void> {
  const ask = ++asked;
  try {
    const summary = await api.daySummary(date.value);
    if (ask !== asked) return;
    day.value = summary;
    // The selection stays put while the lanes reorder.
    if (!summary.projects.some((p) => p.path === selectedPath.value)) selectedPath.value = summary.projects[0]?.path ?? null;
  } catch {
    // Kept as it was; the next refresh tries again.
  }
}

/* ---------- days ---------- */

function dayName(at: number): string {
  const d = new Date(at);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function yesterday(): string {
  const d = new Date();
  d.setHours(12, 0, 0, 0);
  d.setDate(d.getDate() - 1);
  return dayName(d.getTime());
}

function previous(): void {
  if (day.value) date.value = dayName(day.value.start - 1);
}

function next(): void {
  if (!day.value || day.value.today) return;
  const name = dayName(day.value.end + 1);
  date.value = name === dayName(Date.now()) ? null : name;
}

const isYesterday = computed(() => date.value === yesterday());

/* ---------- formats ---------- */

function clock(at: number): string {
  return new Date(at).toLocaleTimeString(language.value, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

function duration(ms: number): string {
  if (ms <= 0) return "—";
  const minutes = Math.round(ms / 60_000);
  if (minutes < 1) return t("time.lessThanMinute");
  const hours = Math.floor(minutes / 60);
  return hours ? t("time.duration", { hours, minutes: String(minutes % 60).padStart(2, "0") }) : t("time.durationMinutes", { minutes });
}

/** How long `spans` cover together, overlaps counted once. */
function covered(spans: DaySpan[]): number {
  let total = 0;
  let from = 0;
  let to = -Infinity;
  for (const span of [...spans].sort((a, b) => a.start - b.start)) {
    if (span.start > to) {
      if (to > from) total += to - from;
      from = span.start;
      to = span.end;
    } else {
      to = Math.max(to, span.end);
    }
  }
  if (to > from) total += to - from;
  return total;
}

function sum(spans: DaySpan[]): number {
  return spans.reduce((total, span) => total + span.end - span.start, 0);
}

function spansOf(project: DayProject): DaySpan[] {
  return [...project.work, ...project.wait, ...project.server];
}

/** First and last moment anything happened in `projects`. */
function bounds(projects: DayProject[]): [number, number] | null {
  let lo = Infinity;
  let hi = -Infinity;
  for (const project of projects) {
    for (const span of spansOf(project)) {
      lo = Math.min(lo, span.start);
      hi = Math.max(hi, span.end);
    }
    for (const at of [...project.commits.map((c) => c.at), ...project.crashes]) {
      lo = Math.min(lo, at);
      hi = Math.max(hi, at);
    }
  }
  return Number.isFinite(lo) ? [lo, hi] : null;
}

/* ---------- the day ---------- */

const projects = computed(() => day.value?.projects ?? []);

/** The day shown, known before its summary arrives, so the heading never jumps. */
const shownName = computed(() => date.value ?? (day.value?.today ? day.value.date : dayName(Date.now())));

const dateLabel = computed(() => {
  const [year, month, dayOfMonth] = shownName.value.split("-").map(Number);
  return new Date(year, month - 1, dayOfMonth, 12).toLocaleDateString(language.value, { weekday: "long", day: "numeric", month: "long" });
});

const subtitle = computed(() => {
  const d = day.value;
  // The previous day's range stays out while the next day loads.
  const span = d && d.date === shownName.value ? bounds(projects.value) : null;
  if (!d || !span) return dateLabel.value;
  const range = d.today ? t("day.untilNow", { time: clock(span[0]) }) : `${clock(span[0])} – ${clock(span[1])}`;
  return `${dateLabel.value} · ${range}`;
});

const totals = computed(() => {
  const all = projects.value;
  return [
    { label: t("day.active"), value: duration(covered(all.flatMap(spansOf))) },
    { label: t("day.claudeWorked"), value: duration(sum(all.flatMap((p) => p.work))) },
    { label: t("day.waitedOnYou"), value: duration(sum(all.flatMap((p) => p.wait))) },
    { label: t("day.commits"), value: String(all.reduce((n, p) => n + p.commits.length, 0)) },
  ];
});

/** Whole hours around what happened (and now, today), at least MIN_HOURS of them. */
const range = computed(() => {
  const d = day.value;
  if (!d) return null;
  const span = bounds(projects.value);
  const hours = Math.round((d.end - d.start) / HOUR);
  let lo = span ? span[0] : d.today ? d.now : d.start + 9 * HOUR;
  let hi = span ? span[1] : lo;
  if (d.today) {
    lo = Math.min(lo, d.now);
    hi = Math.max(hi, d.now);
  }
  let from = Math.max(0, Math.floor((lo - d.start) / HOUR));
  let to = Math.min(hours, Math.ceil((hi - d.start) / HOUR) + (d.today ? 1 : 0));
  if (to - from < MIN_HOURS) {
    to = Math.min(hours, from + MIN_HOURS);
    from = Math.max(0, to - MIN_HOURS);
  }
  return { from: d.start + from * HOUR, to: d.start + to * HOUR, hours: to - from };
});

function pct(at: number): number {
  const r = range.value;
  if (!r) return 0;
  return Math.min(100, Math.max(0, ((at - r.from) / (r.to - r.from)) * 100));
}

function place(start: number, end: number): Record<string, string> {
  const left = pct(start);
  return { left: `${left}%`, width: `${Math.max(pct(end) - left, 0.25)}%` };
}

/** The hour row's width, so labels are spaced to fit and step aside for the now marker. */
const axis = ref<HTMLElement | null>(null);
const axisWidth = ref(0);
const axisObserver = new ResizeObserver(([entry]) => (axisWidth.value = entry.contentRect.width));

watch(axis, (element, old) => {
  if (old) axisObserver.unobserve(old);
  if (element) axisObserver.observe(element);
});

const ticks = computed(() => {
  const r = range.value;
  if (!r) return [];
  const perHour = axisWidth.value ? axisWidth.value / r.hours : 60;
  const step = [1, 2, 3, 4, 6].find((hours) => hours * perHour >= 46) ?? 6;
  const now = day.value?.today ? day.value.now : null;
  return Array.from({ length: r.hours + 1 }, (_, i) => {
    const at = r.from + i * HOUR;
    const major = i % step === 0;
    const underNow = now !== null && (Math.abs(at - now) / HOUR) * perHour < 50;
    return { at, left: `${pct(at)}%`, label: major && !underNow ? clock(at) : "", major };
  });
});

const nowLeft = computed(() => (day.value?.today ? `${pct(day.value.now)}%` : null));

function sessionTitle(project: DayProject, id: string | undefined): string {
  return project.sessions.find((s) => s.id === id)?.title ?? "";
}

function blocks(project: DayProject): Block[] {
  const list: Block[] = [];
  const range = (span: DaySpan): string => `${clock(span.start)} – ${clock(span.end)}`;

  project.server.forEach((span, i) =>
    list.push({ key: `s${i}`, kind: "server", style: place(span.start, span.end), text: "", tip: { title: t("day.tip.server", { duration: duration(span.end - span.start) }), detail: range(span), tone: "server" } }),
  );

  // A session's name on the first of its blocks in a row; the next blocks are the same work.
  let last: string | undefined;
  project.work.forEach((span, i) => {
    const title = sessionTitle(project, span.session);
    const text = span.session !== last ? title : "";
    last = span.session;
    list.push({
      key: `w${i}`,
      kind: "work",
      style: place(span.start, span.end),
      text,
      tip: { title: t("day.tip.work", { duration: duration(span.end - span.start) }), detail: title ? `${range(span)} · ${title}` : range(span), tone: "claude" },
    });
  });

  project.wait.forEach((span, i) => {
    const title = sessionTitle(project, span.session);
    const minutes = Math.round((span.end - span.start) / 60_000);
    list.push({
      key: `a${i}`,
      kind: "wait",
      style: place(span.start, span.end),
      text: minutes >= 1 ? t("time.durationMinutes", { minutes }) : "",
      tip: { title: t("day.tip.wait", { duration: duration(span.end - span.start) }), detail: title ? `${range(span)} · ${title}` : range(span), tone: "claude" },
    });
  });

  project.commits.forEach((commit, i) =>
    list.push({ key: `c${i}`, kind: "commit", style: { left: `${pct(commit.at)}%` }, text: "", tip: { title: t("day.tip.commit", { time: clock(commit.at) }), detail: commit.subject, tone: "plain" } }),
  );

  project.crashes.forEach((at, i) =>
    list.push({ key: `x${i}`, kind: "crash", style: { left: `${pct(at)}%` }, text: "✕", tip: { title: t("day.tip.crash", { time: clock(at) }), detail: "", tone: "crash" } }),
  );

  return list;
}

const lanes = computed(() =>
  projects.value.map((project) => {
    const time = covered(spansOf(project));
    return {
      project,
      time,
      total: time ? duration(time) : t("day.commitCount", { count: project.commits.length }),
      blocks: blocks(project),
    };
  }),
);

/* ---------- where the time went ---------- */

/** Project colours, picked at random for now: from the path, so a project keeps its colour.
    Hues far apart, and none of Claude's orange, the server's green or a crash's red. */
const PALETTE = ["#7aa2f7", "#c49ef0", "#6fc6d9", "#e5c07b", "#f08fb6", "#a3acc2"];

function hash(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** Each project its colour; two projects of the day never share one while the palette lasts. */
const colors = computed(() => {
  const taken = new Set<number>();
  const map = new Map<string, string>();
  for (const path of projects.value.map((p) => p.path).sort()) {
    let at = hash(path) % PALETTE.length;
    for (let i = 0; i < PALETTE.length && taken.has(at); i++) at = (at + 1) % PALETTE.length;
    taken.add(at);
    map.set(path, PALETTE[at]);
  }
  return map;
});

const percent = computed(() => new Intl.NumberFormat(language.value, { style: "percent", maximumFractionDigits: 0 }));

/** Each project's share of the day's time, as its lane counts it. */
const split = computed(() => {
  const parts = lanes.value.filter((lane) => lane.time > 0);
  const whole = parts.reduce((total, lane) => total + lane.time, 0);
  return parts.map((lane) => {
    const share = percent.value.format(lane.time / whole);
    return {
      path: lane.project.path,
      name: lane.project.name,
      grow: lane.time / whole,
      share,
      color: colors.value.get(lane.project.path) ?? PALETTE[0],
      tip: { title: lane.project.name, detail: `${lane.total} · ${share}`, tone: "plain" } as Tip,
    };
  });
});

type Live = { phase: "waiting" | "working"; text: string };

/** Live, whatever day is shown: where Claude waits on you (a notification not read yet) or
    works, by project; waiting wins, as in the project list. */
const liveProjects = computed(() => {
  const found = new Map<string, Live>();
  for (const p of snapshot.value?.projects ?? []) {
    if (p.claude) found.set(p.path, { phase: "waiting", text: claudeLine(p, now.value) });
    else if (p.claudeWorking) found.set(p.path, { phase: "working", text: t("claude.working") });
  }
  return found;
});

/** The same, per Claude session. */
const liveSessions = computed(() => {
  const found = new Map<string, Live>();
  for (const p of snapshot.value?.projects ?? []) {
    for (const s of p.claudeSessions) {
      found.set(s.id, s.phase === "waiting" ? { phase: "waiting", text: s.turn ? turnLine(s.turn, now.value) : "" } : { phase: "working", text: t("claude.working") });
    }
  }
  return found;
});

/* ---------- the selected project ---------- */

const selected = computed(() => projects.value.find((p) => p.path === selectedPath.value) ?? projects.value[0] ?? null);

const selectedSpan = computed(() => {
  const span = selected.value ? bounds([selected.value]) : null;
  return span ? `${clock(span[0])} – ${clock(span[1])}` : "";
});

const tiles = computed(() => {
  const p = selected.value;
  if (!p) return [];
  const server = covered(p.server);
  return [
    { label: t("day.claudeWorked"), value: p.work.length ? duration(sum(p.work)) : "", note: "" },
    { label: t("day.waitedOnYou"), value: p.wait.length ? duration(sum(p.wait)) : "", note: "" },
    { label: t("day.serverUp"), value: server ? duration(server) : "", note: p.crashes.length ? t("day.crashCount", { count: p.crashes.length }) : "" },
    { label: t("day.commits"), value: p.commits.length ? String(p.commits.length) : "", note: "" },
  ];
});

const commits = computed(() => [...(selected.value?.commits ?? [])].reverse());

function share(part: number, session: DaySession): string {
  const whole = session.work + session.wait;
  return whole ? `${(part / whole) * 100}%` : "0%";
}

/* ---------- tooltip ---------- */

function showTip(event: PointerEvent, shown: Tip): void {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  tip.value = { ...shown, x: rect.left + rect.width / 2, y: rect.top };
  void nextTick(() => {
    // Kept inside the window.
    const el = tipEl.value;
    if (!el || !tip.value) return;
    const half = el.offsetWidth / 2;
    tip.value.x = Math.min(Math.max(tip.value.x, half + 8), window.innerWidth - half - 8);
  });
}

function hideTip(): void {
  tip.value = null;
}

/* ---------- life ---------- */

let timer: number | undefined;

watch(date, () => {
  hideTip();
  void load();
});

onMounted(() => {
  void load();
  timer = window.setInterval(() => {
    if (!day.value || day.value.today) void load();
  }, REFRESH_MS);
});

onBeforeUnmount(() => {
  window.clearInterval(timer);
  axisObserver.disconnect();
});
</script>

<template>
  <div class="day">
    <div class="column">
      <!-- Its heading and free space drag the window; the day buttons stay clickable. -->
      <header class="bar" data-tauri-drag-region="deep">
        <div class="heading">
          <span class="title">{{ t("day.title") }}</span>
          <span class="subtitle">{{ subtitle }}</span>
        </div>
        <div class="days">
          <button type="button" class="arrow" :aria-label="t('day.previous')" :title="t('day.previous')" @click="previous">
            <Icon name="back" :size="14" />
          </button>
          <div role="tablist" class="tabs">
            <button type="button" role="tab" :aria-selected="isYesterday" :class="{ on: isYesterday }" @click="date = yesterday()">{{ t("day.yesterday") }}</button>
            <button type="button" role="tab" :aria-selected="date === null" :class="{ on: date === null }" @click="date = null">{{ t("day.today") }}</button>
          </div>
          <button type="button" class="arrow" :aria-label="t('day.next')" :title="t('day.next')" :disabled="!day || day.today" @click="next">
            <Icon name="chevron" :size="14" />
          </button>
        </div>
      </header>

      <div class="board">
        <template v-if="day && projects.length">
          <div class="summary">
            <div class="totals">
              <div v-for="total in totals" :key="total.label" class="total">
                <span class="caps">{{ total.label }}</span>
                <span class="total-value">{{ total.value }}</span>
              </div>
            </div>

            <!-- Where the time went: each project's share, in its colour. -->
            <template v-if="split.length">
              <div class="split" :aria-label="t('day.focus')">
                <span
                  v-for="part in split"
                  :key="part.path"
                  class="part"
                  :style="{ flexGrow: part.grow, background: part.color }"
                  @pointerenter="showTip($event, part.tip)"
                  @pointerleave="hideTip"
                  @click="selectedPath = part.path"
                ></span>
              </div>
              <div class="split-legend">
                <button v-for="part in split" :key="part.path" type="button" :class="{ on: part.path === selected?.path }" @click="selectedPath = part.path">
                  <i :style="{ background: part.color }"></i>{{ part.name }}<span class="share">{{ part.share }}</span>
                </button>
              </div>
            </template>
          </div>

          <div class="timeline" :aria-label="t('day.timeline')">
            <div ref="axis" class="axis">
              <span v-for="tick in ticks" :key="tick.at" class="tick" :style="{ left: tick.left }">{{ tick.label }}</span>
              <span v-if="nowLeft" class="now-pill" :style="{ left: nowLeft }"><i></i>{{ clock(day.now) }}</span>
            </div>

            <div class="lanes">
              <button
                v-for="lane in lanes"
                :key="lane.project.path"
                type="button"
                :class="['lane', { on: lane.project.path === selected?.path }]"
                @click="selectedPath = lane.project.path"
                @dblclick="api.openEditor(lane.project.path)"
              >
                <!-- The hint sits on the name only: the blocks have their own tooltips. -->
                <span class="label" :title="t('day.laneTitle')">
                  <span class="name-row">
                    <ClaudeDot :live="liveProjects.get(lane.project.path)" />
                    <span class="name">{{ lane.project.name }}</span>
                  </span>
                  <span class="lane-total under">{{ lane.total }}</span>
                </span>
                <span class="track">
                  <span v-for="tick in ticks" :key="tick.at" :class="['grid', { major: tick.major }]" :style="{ left: tick.left }"></span>
                  <span
                    v-for="block in lane.blocks"
                    :key="block.key"
                    :class="['block', block.kind]"
                    :style="block.style"
                    @pointerenter="showTip($event, block.tip)"
                    @pointerleave="hideTip"
                  >
                    <span v-if="block.text" class="text">{{ block.text }}</span>
                  </span>
                  <span v-if="nowLeft" class="now" :style="{ left: nowLeft }"></span>
                </span>
              </button>
            </div>
          </div>

          <div class="legend">
            <span><i class="key work"></i>{{ t("day.legend.work") }}</span>
            <span><i class="key wait"></i>{{ t("day.legend.wait") }}</span>
            <span><i class="key server"></i>{{ t("day.legend.server") }}</span>
            <span><i class="key commit"></i>{{ t("day.legend.commit") }}</span>
            <span><i class="key crash">✕</i>{{ t("day.legend.crash") }}</span>
          </div>
        </template>

        <section v-else-if="day" class="empty">
          <Icon name="calendar" :size="28" />
          <p>{{ t("day.empty") }}<br />{{ t("day.emptyHint") }}</p>
        </section>
      </div>
    </div>

    <!-- Full height like the project details; its header line meets the toolbar's. -->
    <aside class="side" :aria-label="selected?.name">
      <!-- Always drawn, so its line doesn't pop in when the day arrives. -->
      <div class="side-head" data-tauri-drag-region="deep">
        <template v-if="selected">
          <span class="name-row">
            <ClaudeDot :live="liveProjects.get(selected.path)" />
            <span class="side-name">{{ selected.name }}</span>
          </span>
          <span class="side-span under">{{ selectedSpan }}</span>
        </template>
      </div>

      <template v-if="selected">
        <div class="side-body">
          <div class="tiles">
            <div v-for="tile in tiles" :key="tile.label" class="tile">
              <span class="tile-label">{{ tile.label }}</span>
              <span :class="['tile-value', { none: !tile.value }]">{{ tile.value || "—" }}</span>
              <span v-if="tile.note" class="tile-note">{{ tile.note }}</span>
            </div>
          </div>

          <section v-if="selected.sessions.length" class="group">
            <span class="caps group-title"><ClaudeMark :size="12" />{{ t("day.sessions") }}</span>
            <div
              v-for="session in selected.sessions"
              :key="session.id"
              class="session"
              role="button"
              tabindex="0"
              :title="t('day.sessionTitle')"
              @click="api.revealClaude(selected.path, session.id)"
              @keydown.enter="api.revealClaude(selected.path, session.id)"
            >
              <div class="session-top">
                <ClaudeDot :live="liveSessions.get(session.id)" />
                <span :class="['session-name', { untitled: !session.title }]" :title="session.title ?? ''">{{ session.title || t("day.untitled") }}</span>
                <span v-if="session.turns" class="session-turns">{{ t("day.turns", { count: session.turns }) }}</span>
              </div>
              <div class="session-bottom under">
                <span class="mini">
                  <i class="mini-work" :style="{ width: share(session.work, session) }"></i>
                  <i class="mini-wait" :style="{ width: share(session.wait, session) }"></i>
                </span>
                <span class="mono session-length">{{ duration(session.work + session.wait) }}</span>
              </div>
            </div>
          </section>

          <section v-if="commits.length" class="group">
            <span class="caps group-title">{{ t("day.commits") }}</span>
            <div v-for="commit in commits" :key="commit.at + commit.subject" class="line">
              <span class="mono">{{ clock(commit.at) }}</span>
              <span class="line-text" :title="commit.subject">{{ commit.subject }}</span>
            </div>
          </section>

          <section v-if="selected.commands.length" class="group">
            <span class="caps group-title">{{ t("day.commands") }}</span>
            <div v-for="command in selected.commands" :key="command.name" class="line">
              <span class="line-text">{{ command.name }}</span>
              <span class="mono">{{ command.runs }}×</span>
              <span v-if="command.failed" class="failed">{{ t("day.failed", { count: command.failed }) }}</span>
            </div>
          </section>
        </div>
      </template>
    </aside>

    <div v-if="tip" ref="tipEl" class="tip" :style="{ left: `${tip.x}px`, top: `${tip.y}px` }">
      <span :class="['tip-title', tip.tone]">{{ tip.title }}</span>
      <span v-if="tip.detail" class="tip-detail">{{ tip.detail }}</span>
    </div>
  </div>
</template>

<style scoped>
.day {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  overflow: clip;
}

.column {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

/* The same height and line as the projects toolbar, so switching never moves them. */
.bar {
  height: 56px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.heading {
  flex-grow: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.title {
  font-size: 15px;
  font-weight: 600;
}

/* A line of its own from the start, so the title above never moves. */
.subtitle {
  height: 16px;
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.days {
  display: flex;
  align-items: center;
  gap: 2px;
}

.arrow {
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-muted);
}

.arrow:hover:not(:disabled) {
  background: #262a33;
  color: var(--text-strong);
}

.arrow:disabled {
  opacity: 0.35;
}

.tabs {
  display: flex;
  padding: 2px;
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  background: var(--bg-input);
}

.tabs button {
  height: 26px;
  padding: 0 12px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-subtle);
}

.tabs button.on {
  background: #23272e;
  color: var(--text-strong);
}

.board {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 18px 22px 16px;
}

.caps {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-subtle);
}

:lang(zh) .caps,
:lang(ja) .caps {
  letter-spacing: 0;
}

.summary {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.totals {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 26px;
}

/* Square like the timeline's blocks; slivers stay visible. */
.split {
  display: flex;
  gap: 2px;
  height: 8px;
  margin-top: 4px;
  background: #15171c;
}

.part {
  flex: 0 1 0;
  min-width: 3px;
  cursor: pointer;
}

.part:hover {
  filter: brightness(1.2);
}

.split-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 16px;
}

.split-legend button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0;
  border: 0;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.split-legend button:hover,
.split-legend button.on {
  color: var(--text-strong);
}

.split-legend i {
  width: 8px;
  height: 8px;
  flex-shrink: 0;
}

.share {
  color: #6c727c;
  font-variant-numeric: tabular-nums;
}

.total {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.total-value {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-strong);
  font-variant-numeric: tabular-nums;
}

/* ---------- timeline ---------- */

.timeline {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

/* Lines up with the tracks: lane padding 8 + label 150 + gap 18, and the lane's 8 on the right. */
.axis {
  position: relative;
  flex-shrink: 0;
  height: 20px;
  margin: 0 8px 0 176px;
}

.tick {
  position: absolute;
  top: 0;
  transform: translateX(-50%);
  font-family: var(--font-mono);
  font-size: 11px;
  color: #6c727c;
  white-space: nowrap;
}

.now-pill {
  position: absolute;
  top: -1px;
  z-index: 1;
  transform: translateX(-50%);
  height: 18px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 0 7px;
  border-radius: 9px;
  background: #1d2a22;
  border: 1px solid #2d4537;
  font-family: var(--font-mono);
  font-size: 11px;
  color: #9ad7b0;
  white-space: nowrap;
}

.now-pill i {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--run);
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%,
  100% {
    box-shadow: 0 0 0 0 rgba(115, 201, 145, 0.55);
  }
  50% {
    box-shadow: 0 0 0 5px rgba(115, 201, 145, 0);
  }
}

.lanes {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* The whole row picks the project; no colours, the row just lights up. */
.lane {
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 18px;
  width: 100%;
  flex-shrink: 0;
  margin: 0;
  padding: 6px 8px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
}

.lane:hover {
  background: #16191e;
}

.lane.on {
  background: #181b20;
}

.label {
  width: 150px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

/* Claude's dot right before the name, on its line; the line below starts where the name does
   (dot 7 + gap 8). */
.name-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.under {
  padding-left: 15px;
}

.name {
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}


.lane-total {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
}

/* Square, like the blocks on it. */
.track {
  position: relative;
  flex-grow: 1;
  height: 46px;
  background: #15171c;
  box-shadow: inset 0 0 0 1px #1e2127;
}

.lane.on .track {
  background: #1b1f26;
  box-shadow: inset 0 0 0 1px #2c313a;
}

.grid {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  background: #1b1e24;
  pointer-events: none;
}

.grid.major {
  background: #20242b;
}

.now {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  margin-left: -1px;
  background: var(--run);
  opacity: 0.85;
  pointer-events: none;
}

.block {
  position: absolute;
  box-sizing: border-box;
}

/* A thin rail, with room around it to point at. */
.block.server {
  bottom: 3px;
  height: 12px;
}

.block.server::before {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 4px;
  height: 4px;
  background: var(--run);
  box-shadow: 0 0 6px rgba(115, 201, 145, 0.45);
}

.block.work,
.block.wait {
  top: 18px;
  height: 16px;
  overflow: hidden;
  container-type: inline-size;
}

.block.work {
  background: linear-gradient(180deg, #f6a560, #e3792e);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25);
}

.block.wait {
  border: 1px dashed rgba(240, 136, 62, 0.85);
  background: repeating-linear-gradient(135deg, rgba(240, 136, 62, 0.32) 0 3px, transparent 3px 7px);
}

.text {
  display: block;
  padding: 0 6px;
  font-size: 10px;
  line-height: 16px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.block.work .text {
  font-weight: 600;
  color: #2b1405;
}

.block.wait .text {
  padding: 0 2px;
  font-family: var(--font-mono);
  line-height: 14px;
  text-align: center;
  color: #f3c29b;
}

/* Too narrow to read: no text at all rather than an ellipsis. */
@container (max-width: 60px) {
  .block.work .text {
    display: none;
  }
}

@container (max-width: 34px) {
  .block.wait .text {
    display: none;
  }
}

.block.commit {
  top: 2px;
  width: 14px;
  height: 14px;
  margin-left: -7px;
}

.block.commit::before {
  content: "";
  position: absolute;
  left: 3px;
  top: 3px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-strong);
  box-shadow:
    0 0 0 2px #15171c,
    0 0 0 3px #4a515c;
}

.lane.on .block.commit::before {
  box-shadow:
    0 0 0 2px #1b1f26,
    0 0 0 3px #4a515c;
}

.block.crash {
  bottom: 1px;
  width: 14px;
  height: 14px;
  margin-left: -7px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: #2a1b1b;
  box-shadow: 0 0 0 1px #4a2b2b;
  color: var(--crash-text);
  font-size: 9px;
  font-weight: 700;
}

.block.crash .text {
  padding: 0;
  line-height: 14px;
}

.legend {
  flex-shrink: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
  padding-top: 12px;
  border-top: 1px solid #1e2127;
  font-size: 12px;
  color: var(--text-subtle);
}

.legend span {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.key {
  display: inline-block;
  box-sizing: border-box;
}

.key.work {
  width: 20px;
  height: 12px;
  background: linear-gradient(180deg, #f6a560, #e3792e);
}

.key.wait {
  width: 20px;
  height: 12px;
  border: 1px dashed rgba(240, 136, 62, 0.85);
  background: repeating-linear-gradient(135deg, rgba(240, 136, 62, 0.32) 0 3px, transparent 3px 7px);
}

.key.server {
  width: 20px;
  height: 4px;
  background: var(--run);
  box-shadow: 0 0 6px rgba(115, 201, 145, 0.5);
}

.key.commit {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-strong);
  box-shadow:
    0 0 0 2px var(--bg-app),
    0 0 0 3px #4a515c;
}

.key.crash {
  width: 14px;
  height: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: #2a1b1b;
  color: var(--crash-text);
  font-size: 9px;
  font-style: normal;
  font-weight: 700;
}

.empty {
  flex-grow: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  text-align: center;
  color: var(--text-muted);
}

.empty p {
  margin: 0;
  line-height: 1.6;
}

/* ---------- the selected project ---------- */

/* As wide as the project details and as tall, so the panel line stays where it was. */
.side {
  width: 380px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
}

/* The toolbar's height and line, so the two bottom lines meet. */
.side-head {
  height: 56px;
  flex-shrink: 0;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1px;
  min-width: 0;
  padding: 0 20px;
  border-bottom: 1px solid var(--line);
}

.side-body {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 18px 20px;
  overflow-y: auto;
}

.side-name {
  min-width: 0;
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.side-span,
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.tiles {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.tile {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 10px 12px;
  border-radius: var(--radius-card);
  background: #181b20;
  border: 1px solid var(--line);
}

.tile-label {
  font-size: 11px;
  color: var(--text-subtle);
}

.tile-value {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-strong);
}

.tile-value.none {
  color: #5c626c;
}

.tile-note {
  font-size: 11px;
  color: var(--crash-text);
}

.group {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.session {
  display: flex;
  flex-direction: column;
  gap: 5px;
  /* Clickable as a whole: the hover background reaches past the text a little. */
  margin: 0 -6px;
  padding: 4px 6px;
  border-radius: 6px;
}

.session:hover {
  background: var(--bg-hover);
}

.session:focus-visible {
  outline: none;
  box-shadow: 0 0 0 1px #3a3f48;
}

.session-top {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.session-name {
  flex-grow: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.session-name.untitled {
  color: var(--text-muted);
  font-style: italic;
}

.session-turns {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-subtle);
}

.session-bottom {
  display: flex;
  align-items: center;
  gap: 8px;
}

.session-bottom .mono {
  font-size: 11px;
}

/* Same width on every row, so the bars line up at both ends. */
.session-length {
  flex-shrink: 0;
  min-width: 6ch;
  text-align: right;
}

.mini {
  flex-grow: 1;
  height: 6px;
  display: flex;
  border-radius: 3px;
  overflow: hidden;
  background: #1c1f25;
}

.mini-work {
  background: linear-gradient(180deg, #f6a560, #e3792e);
}

.mini-wait {
  background: repeating-linear-gradient(135deg, rgba(240, 136, 62, 0.55) 0 2px, transparent 2px 4px);
}

.line {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  font-size: 12px;
}

.line-text {
  flex-grow: 1;
  min-width: 0;
  color: #c7ccd3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.line .mono {
  flex-shrink: 0;
}

.failed {
  flex-shrink: 0;
  color: var(--crash-text);
}

/* ---------- tooltip ---------- */

.tip {
  position: fixed;
  z-index: 20;
  transform: translate(-50%, calc(-100% - 8px));
  display: flex;
  flex-direction: column;
  gap: 3px;
  max-width: 340px;
  padding: 9px 11px;
  border-radius: 9px;
  background: #23272e;
  border: 1px solid #3a3f48;
  box-shadow: 0 10px 24px rgba(0, 0, 0, 0.45);
  font-size: 12px;
  pointer-events: none;
}

.tip-title {
  font-weight: 600;
  color: var(--text-strong);
}

.tip-title.claude {
  color: #f3c29b;
}

.tip-title.server {
  color: #9ad7b0;
}

.tip-title.crash {
  color: var(--crash-text);
}

.tip-detail {
  font-family: var(--font-mono);
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
